// HTTP/2 传输层实现
use anyhow::{Context, Result};
use bytes::Bytes;
use h2::client::{self, SendRequest};
use http::{Request, Response};
use std::io;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;

pub struct Http2Transport {
    send_request: SendRequest<Bytes>,
    path: String,
}

impl Http2Transport {
    pub async fn connect(addr: SocketAddr, path: Option<String>) -> Result<Self> {
        tracing::debug!(addr = %addr, "Connecting HTTP/2 transport");

        let tcp = TcpStream::connect(addr)
            .await
            .context("Failed to connect")?;

        let (send_request, connection) = client::handshake(tcp)
            .await
            .context("HTTP/2 handshake failed")?;

        // 在后台运行连接
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                tracing::error!("HTTP/2 connection error: {}", e);
            }
        });

        tracing::info!(addr = %addr, "HTTP/2 transport connected");

        Ok(Self {
            send_request,
            path: path.unwrap_or_else(|| "/".to_string()),
        })
    }

    pub async fn send_data(&mut self, data: Bytes) -> Result<Bytes> {
        let request = Request::post(&self.path)
            .body(())
            .context("Failed to build request")?;

        let (response, mut send_stream) = self
            .send_request
            .send_request(request, false)
            .context("Failed to send request")?;

        send_stream
            .send_data(data, true)
            .await
            .context("Failed to send data")?;

        let response = response.await.context("Failed to receive response")?;

        let mut body = response.into_body();
        let mut result = Vec::new();

        while let Some(chunk) = body.data().await {
            let chunk = chunk.context("Failed to read chunk")?;
            result.extend_from_slice(&chunk);
            body.flow_control()
                .release_capacity(chunk.len())
                .context("Failed to release capacity")?;
        }

        Ok(Bytes::from(result))
    }
}

// 实现 AsyncRead/AsyncWrite
pub struct Http2Stream {
    transport: Http2Transport,
    read_buf: Vec<u8>,
    read_pos: usize,
    write_buf: Vec<u8>,
}

impl Http2Stream {
    pub async fn new(addr: SocketAddr) -> Result<Self> {
        let transport = Http2Transport::connect(addr, None).await?;

        Ok(Self {
            transport,
            read_buf: Vec::new(),
            read_pos: 0,
            write_buf: Vec::new(),
        })
    }
}

impl AsyncRead for Http2Stream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        use std::task::Poll;

        // 如果缓冲区有数据，直接读取
        if self.read_pos < self.read_buf.len() {
            let remaining = &self.read_buf[self.read_pos..];
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            self.read_pos += to_copy;
            return Poll::Ready(Ok(()));
        }

        // 缓冲区已空，需要等待新数据
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl AsyncWrite for Http2Stream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<io::Result<usize>> {
        // 写入到缓冲区
        self.write_buf.extend_from_slice(buf);
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        use std::task::Poll;

        if self.write_buf.is_empty() {
            return Poll::Ready(Ok(()));
        }

        // 发送数据
        let data = Bytes::from(self.write_buf.clone());
        self.write_buf.clear();

        let mut transport = self.transport.clone();
        let future = async move { transport.send_data(data).await };

        // 这里需要使用 tokio::pin 或其他方式来 poll future
        cx.waker().wake_by_ref();
        Poll::Pending
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}

// 需要实现 Clone for SendRequest
impl Clone for Http2Transport {
    fn clone(&self) -> Self {
        Self {
            send_request: self.send_request.clone(),
            path: self.path.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // 需要 HTTP/2 服务器
    async fn test_http2_transport() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let mut transport = Http2Transport::connect(addr, Some("/tunnel".to_string()))
            .await
            .unwrap();

        let data = Bytes::from("Hello, HTTP/2!");
        let response = transport.send_data(data).await.unwrap();

        println!("Response: {:?}", response);
    }
}
