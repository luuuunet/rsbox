// WebSocket 传输实现
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use http::HeaderMap;
use std::io;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message, WebSocketStream};

pub struct WebSocketTransport<S> {
    ws: WebSocketStream<S>,
}

impl WebSocketTransport<tokio::net::TcpStream> {
    pub async fn connect(
        uri: &str,
        headers: Option<HeaderMap>,
        early_data: Option<Vec<u8>>,
    ) -> Result<Self> {
        tracing::debug!(uri = %uri, "Connecting to WebSocket");

        let mut request = http::Request::builder().uri(uri);

        // 添加自定义 headers
        if let Some(headers) = headers {
            for (name, value) in headers.iter() {
                request = request.header(name, value);
            }
        }

        // 处理 early data
        if let Some(data) = early_data {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            request = request.header("Sec-WebSocket-Protocol", encoded);
        }

        let (ws, _) = connect_async(request.body(())?).await?;

        tracing::info!("WebSocket connected");

        Ok(Self { ws })
    }
}

impl<S> AsyncRead for WebSocketTransport<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        use std::task::Poll;

        match self.ws.poll_next_unpin(cx) {
            Poll::Ready(Some(Ok(msg))) => {
                match msg {
                    Message::Binary(data) => {
                        let to_copy = data.len().min(buf.remaining());
                        buf.put_slice(&data[..to_copy]);
                        Poll::Ready(Ok(()))
                    }
                    Message::Close(_) => Poll::Ready(Ok(())),
                    _ => {
                        // 忽略其他类型的消息
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                }
            }
            Poll::Ready(Some(Err(e))) => {
                Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e)))
            }
            Poll::Ready(None) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<S> AsyncWrite for WebSocketTransport<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        use std::task::Poll;

        let msg = Message::Binary(buf.to_vec());
        match self.ws.poll_ready_unpin(cx) {
            Poll::Ready(Ok(())) => {
                match self.ws.start_send_unpin(msg) {
                    Ok(()) => Poll::Ready(Ok(buf.len())),
                    Err(e) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
                }
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        use std::task::Poll;

        match self.ws.poll_flush_unpin(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        use std::task::Poll;

        match self.ws.poll_close_unpin(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(e)) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, e))),
            Poll::Pending => Poll::Pending,
        }
    }
}
