//! ShadowTLS v2 record framing (sing-shadowtls v2_conn + v2_hash).

use crate::shadowtls::constants::*;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};

type HmacSha1 = Hmac<Sha1>;

pub struct HashReadConn<S> {
    inner: S,
    hmac: HmacSha1,
}

impl<S> HashReadConn<S> {
    pub fn new(inner: S, password: &str) -> Self {
        Self {
            inner,
            hmac: HmacSha1::new_from_slice(password.as_bytes())
                .expect("shadowtls v2 hmac key"),
        }
    }

    pub fn sum(&self) -> [u8; HMAC_SIZE_V2] {
        let mut out = [0u8; HMAC_SIZE_V2];
        out.copy_from_slice(&self.hmac.clone().finalize().into_bytes()[..HMAC_SIZE_V2]);
        out
    }

    pub fn into_inner(self) -> S {
        self.inner
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for HashReadConn<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        let res = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &res {
            let filled = buf.filled();
            if filled.len() > before {
                self.hmac.update(&filled[before..]);
            }
        }
        res
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for HashReadConn<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

pub struct ShadowConn<S> {
    pub(crate) inner: S,
    read_remaining: usize,
}

impl<S> ShadowConn<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            read_remaining: 0,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for ShadowConn<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.read_remaining > 0 {
            let mut tmp = ReadBuf::new(buf.initialize_unfilled());
            let cap = self.read_remaining.min(tmp.remaining());
            tmp.initialize_unfilled_to(cap);
            let res = Pin::new(&mut self.inner).poll_read(cx, &mut tmp);
            if let Poll::Ready(Ok(())) = res {
                let n = tmp.filled().len();
                buf.advance(n);
                self.read_remaining -= n;
            }
            return res;
        }
        let mut header = [0u8; TLS_HEADER_SIZE];
        let mut hb = ReadBuf::new(&mut header);
        match Pin::new(&mut self.inner).poll_read(cx, &mut hb) {
            Poll::Ready(Ok(())) if hb.filled().len() == TLS_HEADER_SIZE => {}
            Poll::Ready(Ok(())) => return Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        }
        if header[0] != APPLICATION_DATA {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("shadowtls v2 unexpected record type {}", header[0]),
            )));
        }
        let length = u16::from_be_bytes([header[3], header[4]]) as usize;
        let mut tmp = ReadBuf::new(buf.initialize_unfilled());
        let cap = length.min(tmp.remaining());
        tmp.initialize_unfilled_to(cap);
        let res = Pin::new(&mut self.inner).poll_read(cx, &mut tmp);
        if let Poll::Ready(Ok(())) = &res {
            let n = tmp.filled().len();
            buf.advance(n);
            self.read_remaining = length.saturating_sub(n);
        }
        res
    }
}

pub struct V2ClientConn<S> {
    shadow: ShadowConn<S>,
    first_write: bool,
    auth_prefix: [u8; HMAC_SIZE_V2],
}

impl<S> V2ClientConn<S> {
    pub fn new(inner: S, auth_prefix: [u8; HMAC_SIZE_V2]) -> Self {
        Self {
            shadow: ShadowConn::new(inner),
            first_write: true,
            auth_prefix,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for V2ClientConn<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.shadow).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for V2ClientConn<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.first_write {
            self.first_write = false;
            let mut frame = Vec::with_capacity(HMAC_SIZE_V2 + TLS_HEADER_SIZE + buf.len());
            frame.extend_from_slice(&self.auth_prefix);
            wrap_app_data(&mut frame, buf);
            let n = buf.len();
            return match Pin::new(&mut self.shadow.inner).poll_write(cx, &frame) {
                Poll::Ready(Ok(_)) => Poll::Ready(Ok(n)),
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => Poll::Pending,
            };
        }
        let mut frame = Vec::with_capacity(TLS_HEADER_SIZE + buf.len());
        wrap_app_data(&mut frame, buf);
        let n = buf.len();
        match Pin::new(&mut self.shadow.inner).poll_write(cx, &frame) {
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(n)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.shadow.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.shadow.inner).poll_shutdown(cx)
    }
}

fn wrap_app_data(out: &mut Vec<u8>, payload: &[u8]) {
    out.push(APPLICATION_DATA);
    out.extend_from_slice(&TLS_VERSION_12);
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
}
