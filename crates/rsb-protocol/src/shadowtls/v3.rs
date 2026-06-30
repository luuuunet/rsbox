//! ShadowTLS v3 handshake + post-handshake connection.

use crate::shadowtls::constants::*;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::io;
use std::pin::Pin;
use std::sync::Mutex as StdMutex;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};


type HmacSha1 = Hmac<Sha1>;

fn kdf(password: &str, server_random: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(server_random);
    hasher.finalize().to_vec()
}

pub(crate) fn kdf_public(password: &str, server_random: &[u8]) -> Vec<u8> {
    kdf(password, server_random)
}

fn server_hello_cipher(hs: &[u8]) -> Option<u16> {
    const SESSION_ID_LEN_IDX: usize = 1 + 3 + 2 + TLS_RANDOM_SIZE;
    if hs.len() <= SESSION_ID_LEN_IDX + 3 {
        return None;
    }
    let mut i = SESSION_ID_LEN_IDX;
    let session_id_len = hs[i] as usize;
    i += 1 + session_id_len + 2 + 1;
    if i + 2 > hs.len() {
        return None;
    }
    Some(u16::from_be_bytes([hs[i], hs[i + 1]]))
}

fn is_server_hello_tls13(hs: &[u8]) -> bool {
    const SESSION_ID_LEN_IDX: usize = 1 + 3 + 2 + TLS_RANDOM_SIZE;
    if hs.len() <= SESSION_ID_LEN_IDX + 3 {
        return false;
    }
    let mut i = SESSION_ID_LEN_IDX;
    let session_id_len = hs[i] as usize;
    i += 1 + session_id_len + 2 + 1;
    if i + 2 > hs.len() {
        return false;
    }
    let ext_len = u16::from_be_bytes([hs[i], hs[i + 1]]) as usize;
    i += 2;
    if i + ext_len > hs.len() {
        return false;
    }
    let exts = &hs[i..i + ext_len];
    let mut j = 0usize;
    while j + 4 <= exts.len() {
        let ext_type = u16::from_be_bytes([exts[j], exts[j + 1]]);
        let ext_val_len = u16::from_be_bytes([exts[j + 2], exts[j + 3]]) as usize;
        j += 4;
        if ext_type == 43 {
            if ext_val_len == 2 && j + 2 <= exts.len() {
                return u16::from_be_bytes([exts[j], exts[j + 1]]) == 0x0304;
            }
            return false;
        }
        if j + ext_val_len > exts.len() {
            break;
        }
        j += ext_val_len;
    }
    false
}

fn unwrap_shadowtls_app_record(record: &mut Vec<u8>, key: &[u8]) {
    xor_slice(&mut record[TLS_HMAC_HEADER_SIZE_V3..], key);
    record.copy_within(0..TLS_HEADER_SIZE, HMAC_SIZE_V3);
    let plen = (record.len() - TLS_HMAC_HEADER_SIZE_V3) as u16;
    record[HMAC_SIZE_V3 + 3..HMAC_SIZE_V3 + 5].copy_from_slice(&plen.to_be_bytes());
    record.drain(..HMAC_SIZE_V3);
}

fn xor_slice(data: &mut [u8], key: &[u8]) {
    for (i, b) in data.iter_mut().enumerate() {
        *b ^= key[i % key.len()];
    }
}

pub struct V3HandshakeConn<S> {
    inner: S,
    password: String,
    pending: Vec<u8>,
    server_random: Option<Vec<u8>>,
    read_hmac: Option<HmacSha1>,
    read_hmac_key: Option<Vec<u8>>,
    is_tls13: bool,
    authorized: bool,
}

pub(crate) fn build_client_auth_frame(password: &str, server_random: &[u8]) -> io::Result<Vec<u8>> {
    let mut mac = HmacSha1::new_from_slice(password.as_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    mac.update(server_random);
    mac.update(b"C");
    mac.update(b"");
    let hash = mac.finalize().into_bytes();
    let mut frame = Vec::with_capacity(TLS_HMAC_HEADER_SIZE_V3);
    frame.push(APPLICATION_DATA);
    frame.extend_from_slice(&TLS_VERSION_12);
    frame.extend_from_slice(&(HMAC_SIZE_V3 as u16).to_be_bytes());
    frame.extend_from_slice(&hash[..HMAC_SIZE_V3]);
    Ok(frame)
}

pub(crate) async fn send_client_auth_frame<S>(stream: &mut S, password: &str, server_random: &[u8]) -> io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let frame = build_client_auth_frame(password, server_random)?;
    stream.write_all(&frame).await?;
    stream.flush().await?;
    Ok(())
}

/// After rustls completes, the TCP stream may still contain TLS handshake records that
/// were forwarded during ShadowTLS relay but not consumed by rustls. Discard them and
/// stash the first post-auth ApplicationData frame for [`VerifiedConn`].
pub(crate) async fn stash_post_handshake_records<S>(stream: &mut S, auth: &mut V3AuthState) -> io::Result<()>
where
    S: AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    loop {
        let mut hdr = [0u8; 5];
        match tokio::time::timeout(std::time::Duration::from_millis(150), stream.read(&mut hdr)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(5)) => {}
            Ok(Ok(_)) => break,
            Ok(Err(e)) => return Err(e),
            Err(_) => break,
        }
        let len = u16::from_be_bytes([hdr[3], hdr[4]]) as usize;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await?;
        if hdr[0] == APPLICATION_DATA {
            auth.pending.extend_from_slice(&hdr);
            auth.pending.extend_from_slice(&body);
            break;
        }
        tracing::trace!(
            record_type = hdr[0],
            len,
            "shadowtls v3: discard post-handshake tls record"
        );
    }
    Ok(())
}

impl<S> V3HandshakeConn<S> {
    pub fn new(inner: S, password: String) -> Self {
        Self {
            inner,
            password,
            pending: Vec::new(),
            server_random: None,
            read_hmac: None,
            read_hmac_key: None,
            is_tls13: false,
            authorized: false,
        }
    }

    pub fn into_parts(self) -> (S, V3AuthState) {
        (
            self.inner,
            V3AuthState {
                password: self.password,
                server_random: self.server_random.unwrap_or_default(),
                is_tls13: self.is_tls13,
                authorized: self.authorized,
                read_hmac: self.read_hmac,
                pending: self.pending,
            },
        )
    }
}

pub struct V3AuthState {
    pub password: String,
    pub server_random: Vec<u8>,
    pub is_tls13: bool,
    pub authorized: bool,
    pub read_hmac: Option<HmacSha1>,
    pub pending: Vec<u8>,
}

impl<S: AsyncRead + Unpin> AsyncRead for V3HandshakeConn<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if !self.pending.is_empty() {
            let n = self.pending.len().min(buf.remaining());
            buf.put_slice(&self.pending[..n]);
            self.pending.drain(..n);
            return Poll::Ready(Ok(()));
        }
        let mut header = [0u8; TLS_HEADER_SIZE];
        let mut hb = ReadBuf::new(&mut header);
        match Pin::new(&mut self.inner).poll_read(cx, &mut hb) {
            Poll::Ready(Ok(())) if hb.filled().len() == TLS_HEADER_SIZE => {}
            Poll::Ready(Ok(())) => return Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        }
        let length = u16::from_be_bytes([header[3], header[4]]) as usize;
        let mut record = vec![0u8; TLS_HEADER_SIZE + length];
        record[..TLS_HEADER_SIZE].copy_from_slice(&header);
        let mut rb = ReadBuf::new(&mut record[TLS_HEADER_SIZE..]);
        while rb.filled().len() < length {
            match Pin::new(&mut self.inner).poll_read(cx, &mut rb) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        let mut pass_record = record;
        match pass_record[0] {
            HANDSHAKE if pass_record.len() > SERVER_RANDOM_INDEX + TLS_RANDOM_SIZE && pass_record[5] == SERVER_HELLO => {
                let random = pass_record[SERVER_RANDOM_INDEX..SERVER_RANDOM_INDEX + TLS_RANDOM_SIZE].to_vec();
                let mut mac = HmacSha1::new_from_slice(self.password.as_bytes())
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                mac.update(&random);
                self.read_hmac = Some(mac);
                self.read_hmac_key = Some(kdf(&self.password, &random));
                self.server_random = Some(random);
                self.is_tls13 = is_server_hello_tls13(&pass_record[5..])
                    || server_hello_cipher(&pass_record[5..]).is_some_and(|c| (0x1301..=0x1303).contains(&c));
                if !self.is_tls13 {
                    self.authorized = true;
                }
            }
            APPLICATION_DATA => {
                self.authorized = false;
                if pass_record.len() > TLS_HMAC_HEADER_SIZE_V3 {
                    if let Some(ref mut mac) = self.read_hmac {
                        mac.update(&pass_record[TLS_HMAC_HEADER_SIZE_V3..]);
                        let sum = mac.clone().finalize().into_bytes();
                        if sum[..HMAC_SIZE_V3]
                            == pass_record[TLS_HEADER_SIZE..TLS_HMAC_HEADER_SIZE_V3]
                        {
                            if let Some(ref key) = self.read_hmac_key {
                                unwrap_shadowtls_app_record(&mut pass_record, key);
                            }
                            self.authorized = true;
                            tracing::trace!(
                                record_len = pass_record.len(),
                                "shadowtls v3 app data unwrap ok"
                            );
                        } else {
                            tracing::warn!(
                                record_len = pass_record.len(),
                                expected = ?&sum[..HMAC_SIZE_V3],
                                got = ?&pass_record[TLS_HEADER_SIZE..TLS_HMAC_HEADER_SIZE_V3],
                                "shadowtls v3 app data hmac miss during handshake"
                            );
                        }
                    } else {
                        tracing::warn!(
                            record_len = pass_record.len(),
                            "shadowtls v3 app data before server hello random captured"
                        );
                    }
                }
            }
            _ => {}
        }
        self.pending = pass_record;
        if !self.pending.is_empty() {
            let n = self.pending.len().min(buf.remaining());
            buf.put_slice(&self.pending[..n]);
            self.pending.drain(..n);
        }
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for V3HandshakeConn<S> {
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

pub struct VerifiedConn<S> {
    inner: S,
    hmac_add: StdMutex<HmacSha1>,
    hmac_verify: StdMutex<HmacSha1>,
    hmac_ignore: StdMutex<Option<HmacSha1>>,
    pending: StdMutex<Vec<u8>>,
}

impl<S> VerifiedConn<S> {
    pub fn from_auth(inner: S, auth: V3AuthState) -> io::Result<Self> {
        Self::from_auth_inner(inner, auth, false)
    }

    pub fn from_auth_server(inner: S, auth: V3AuthState) -> io::Result<Self> {
        Self::from_auth_inner(inner, auth, true)
    }

    fn from_auth_inner(inner: S, auth: V3AuthState, server: bool) -> io::Result<Self> {
        let (write_tag, read_tag) = if server { (b"S", b"C") } else { (b"C", b"S") };
        let mut add = HmacSha1::new_from_slice(auth.password.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        add.update(&auth.server_random);
        add.update(write_tag);
        let mut verify = HmacSha1::new_from_slice(auth.password.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        verify.update(&auth.server_random);
        verify.update(read_tag);
        Ok(Self {
            inner,
            hmac_add: StdMutex::new(add),
            hmac_verify: StdMutex::new(verify),
            hmac_ignore: StdMutex::new(auth.read_hmac),
            pending: StdMutex::new(auth.pending),
        })
    }

    fn verify_ignore(body: &[u8], mac: &mut HmacSha1) -> bool {
        mac.update(&body[HMAC_SIZE_V3..]);
        let hash = mac.clone().finalize().into_bytes();
        hash[..HMAC_SIZE_V3] == body[..HMAC_SIZE_V3]
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for VerifiedConn<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        loop {
            if let Ok(mut pending) = this.pending.lock() {
                if !pending.is_empty() {
                    let n = pending.len().min(buf.remaining());
                    buf.put_slice(&pending[..n]);
                    pending.drain(..n);
                    return Poll::Ready(Ok(()));
                }
            }
            let mut header = [0u8; TLS_HEADER_SIZE];
            let mut hb = ReadBuf::new(&mut header);
            match Pin::new(&mut this.inner).poll_read(cx, &mut hb) {
                Poll::Ready(Ok(())) if hb.filled().len() == TLS_HEADER_SIZE => {}
                Poll::Ready(Ok(())) => return Poll::Ready(Ok(())),
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
            if header[0] == ALERT {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "shadowtls v3 alert",
                )));
            }
            if header[0] != APPLICATION_DATA {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "shadowtls v3 bad record",
                )));
            }
            let length = u16::from_be_bytes([header[3], header[4]]) as usize;
            let mut body = vec![0u8; length];
            let mut rb = ReadBuf::new(&mut body);
            while rb.filled().len() < length {
                match Pin::new(&mut this.inner).poll_read(cx, &mut rb) {
                    Poll::Ready(Ok(())) => {}
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => return Poll::Pending,
                }
            }
            if let Ok(mut ignore) = this.hmac_ignore.lock() {
                if let Some(ref mut ig) = *ignore {
                    if Self::verify_ignore(&body, ig) {
                        *ignore = None;
                        continue;
                    }
                    *ignore = None;
                }
            }
            let mut verify = this.hmac_verify.lock().expect("lock");
            verify.update(&body[HMAC_SIZE_V3..]);
            let hash = verify.clone().finalize().into_bytes();
            verify.update(&hash[..HMAC_SIZE_V3]);
            if hash[..HMAC_SIZE_V3] != body[..HMAC_SIZE_V3] {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "shadowtls v3 hmac mismatch",
                )));
            }
            let payload = body[HMAC_SIZE_V3..].to_vec();
            let n = payload.len().min(buf.remaining());
            buf.put_slice(&payload[..n]);
            if n < payload.len() {
                this.pending
                    .lock()
                    .expect("lock")
                    .extend_from_slice(&payload[n..]);
            }
            return Poll::Ready(Ok(()));
        }
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for VerifiedConn<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let mut frame = Vec::with_capacity(TLS_HMAC_HEADER_SIZE_V3 + buf.len());
        frame.push(APPLICATION_DATA);
        frame.extend_from_slice(&TLS_VERSION_12);
        frame.extend_from_slice(&((HMAC_SIZE_V3 + buf.len()) as u16).to_be_bytes());
        let mut add = this.hmac_add.lock().expect("lock");
        add.update(buf);
        let hash = add.clone().finalize().into_bytes();
        add.update(&hash[..HMAC_SIZE_V3]);
        frame.extend_from_slice(&hash[..HMAC_SIZE_V3]);
        frame.extend_from_slice(buf);
        match Pin::new(&mut this.inner).poll_write(cx, &frame) {
            Poll::Ready(Ok(_)) => Poll::Ready(Ok(buf.len())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, Mac};
    use sha1::Sha1;

    #[test]
    fn handshake_hmac_chains_xored_payloads_not_hash_bytes() {
        let password = "st_test";
        let server_random = [7u8; TLS_RANDOM_SIZE];
        let key = kdf_public(password, &server_random);
        let mut hmac_write = HmacSha1::new_from_slice(password.as_bytes()).unwrap();
        hmac_write.update(&server_random);

        let mut xored1 = b"record-one".to_vec();
        xor_slice(&mut xored1, &key);
        hmac_write.update(&xored1);
        let h1 = hmac_write.clone().finalize().into_bytes();

        let mut xored2 = b"record-two".to_vec();
        xor_slice(&mut xored2, &key);
        hmac_write.update(&xored2);
        let h2 = hmac_write.clone().finalize().into_bytes();

        assert_ne!(h1[..HMAC_SIZE_V3], h2[..HMAC_SIZE_V3]);

        let mut read_mac = Hmac::<Sha1>::new_from_slice(password.as_bytes()).unwrap();
        read_mac.update(&server_random);
        read_mac.update(&xored1);
        read_mac.update(&xored2);
        let sum = read_mac.finalize().into_bytes();
        assert_eq!(sum[..HMAC_SIZE_V3], h2[..HMAC_SIZE_V3]);
    }
}
