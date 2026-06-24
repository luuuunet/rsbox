use crate::SharedConnectionManager;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Bidirectional proxy connection returned by outbounds.
pub type ProxyConn = Box<dyn ProxyStream>;

pub trait ProxyStream: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T> ProxyStream for T where T: AsyncRead + AsyncWrite + Send + Unpin {}

pub fn proxy_box<S: ProxyStream + 'static>(stream: S) -> ProxyConn {
    Box::new(stream)
}

pub fn tcp_stream<S: ProxyStream + 'static>(stream: S) -> ProxyConn {
    proxy_box(stream)
}

/// Wraps a connection so it is untracked when dropped and counts traffic.
pub struct TrackedStream {
    inner: ProxyConn,
    connections: SharedConnectionManager,
    id: u64,
    inbound_tag: String,
    outbound_tag: String,
}

impl TrackedStream {
    pub fn new(
        inner: ProxyConn,
        connections: SharedConnectionManager,
        id: u64,
        inbound_tag: String,
        outbound_tag: String,
    ) -> Self {
        Self {
            inner,
            connections,
            id,
            inbound_tag,
            outbound_tag,
        }
    }
}

impl Drop for TrackedStream {
    fn drop(&mut self) {
        self.connections.untrack(self.id);
    }
}

impl AsyncRead for TrackedStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let poll = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &poll {
            let n = buf.filled().len().saturating_sub(before) as u64;
            if n > 0 {
                self.connections
                    .record_traffic(&self.inbound_tag, &self.outbound_tag, 0, n);
            }
        }
        poll
    }
}

impl AsyncWrite for TrackedStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let poll = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &poll {
            if *n > 0 {
                self.connections.record_traffic(
                    &self.inbound_tag,
                    &self.outbound_tag,
                    *n as u64,
                    0,
                );
            }
        }
        poll
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

pub fn tracked_stream(
    inner: ProxyConn,
    connections: SharedConnectionManager,
    id: u64,
) -> ProxyConn {
    let (inbound_tag, outbound_tag) = connections
        .connection_info(id)
        .unwrap_or_else(|| ("unknown".into(), "unknown".into()));
    Box::new(TrackedStream::new(
        inner,
        connections,
        id,
        inbound_tag,
        outbound_tag,
    ))
}

/// Adapter for split read/write halves (e.g. Quinn bidi streams).
pub struct SplitProxy {
    reader: Pin<Box<dyn AsyncRead + Send + Unpin>>,
    writer: Pin<Box<dyn AsyncWrite + Send + Unpin>>,
}

impl AsyncRead for SplitProxy {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.reader).poll_read(cx, buf)
    }
}

impl AsyncWrite for SplitProxy {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.writer).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.writer).poll_shutdown(cx)
    }
}

impl SplitProxy {
    pub fn new(
        reader: impl AsyncRead + Send + Unpin + 'static,
        writer: impl AsyncWrite + Send + Unpin + 'static,
    ) -> Self {
        Self {
            reader: Box::pin(reader),
            writer: Box::pin(writer),
        }
    }
}
