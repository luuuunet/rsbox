//! Buffered QUIC recv stream (prefix bytes before first read).

use quinn::RecvStream;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};

pub struct PrefixedRecvStream {
    inner: RecvStream,
    prefix: Vec<u8>,
    pos: usize,
}

impl PrefixedRecvStream {
    pub fn new(inner: RecvStream, prefix: Vec<u8>) -> Self {
        Self {
            inner,
            prefix,
            pos: 0,
        }
    }
}

impl AsyncRead for PrefixedRecvStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.pos < self.prefix.len() {
            let remain = &self.prefix[self.pos..];
            let take = remain.len().min(buf.remaining());
            buf.put_slice(&remain[..take]);
            self.pos += take;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}
