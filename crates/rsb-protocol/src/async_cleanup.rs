use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::net::TcpStream;
use tokio::io::AsyncWriteExt;
use std::pin::Pin;
use std::task::{Context, Poll};

/// 清理请求
enum CleanupRequest {
    TcpStream(TcpStream),
}

/// 全局异步清理器
pub struct AsyncCleanup {
    sender: mpsc::UnboundedSender<CleanupRequest>,
}

impl AsyncCleanup {
    /// 创建全局清理器并启动后台任务
    pub fn new() -> Arc<Self> {
        let (tx, mut rx) = mpsc::unbounded_channel();

        // 启动后台清理任务
        tokio::spawn(async move {
            tracing::info!("✅ AsyncCleanup: 后台清理任务已启动");

            let mut count = 0u64;

            while let Some(request) = rx.recv().await {
                match request {
                    CleanupRequest::TcpStream(mut stream) => {
                        // 异步清理 TcpStream
                        match stream.shutdown().await {
                            Ok(_) => {
                                count += 1;
                                tracing::trace!("✅ AsyncCleanup: TcpStream #{} 已清理", count);
                            }
                            Err(e) => {
                                tracing::debug!("⚠️ AsyncCleanup: shutdown 失败: {}", e);
                            }
                        }
                        // stream 在这里被 drop
                    }
                }
            }

            tracing::info!("🛑 AsyncCleanup: 后台清理任务已退出，共清理 {} 个连接", count);
        });

        Arc::new(Self { sender: tx })
    }

    /// 请求清理 TcpStream
    pub fn cleanup_stream(&self, stream: TcpStream) {
        if let Err(_) = self.sender.send(CleanupRequest::TcpStream(stream)) {
            tracing::warn!("⚠️ AsyncCleanup: 发送清理请求失败（清理任务已关闭）");
        }
    }
}

/// 自动清理的 TcpStream 包装器
///
/// 在 Drop 时自动发送到后台清理任务，确保连接 100% 被清理
/// 类似 Go 的 defer conn.Close()
pub struct AutoCleanStream {
    stream: Option<TcpStream>,
    cleanup: Arc<AsyncCleanup>,
}

impl AutoCleanStream {
    /// 创建自动清理的 TcpStream
    pub fn new(stream: TcpStream, cleanup: Arc<AsyncCleanup>) -> Self {
        Self {
            stream: Some(stream),
            cleanup,
        }
    }

    /// 获取内部 stream 的可变引用
    pub fn get_mut(&mut self) -> &mut TcpStream {
        self.stream.as_mut().expect("AutoCleanStream: stream 已被取出")
    }

    /// 手动清理（如果需要显式控制）
    pub async fn close(mut self) -> std::io::Result<()> {
        if let Some(mut stream) = self.stream.take() {
            stream.shutdown().await
        } else {
            Ok(())
        }
    }
}

impl Drop for AutoCleanStream {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            // ✅ 在 Drop 时发送到后台清理
            // 无论任务如何退出（正常/错误/panic），都会执行这里
            self.cleanup.cleanup_stream(stream);
            tracing::trace!("📤 AutoCleanStream::drop - 已发送清理请求");
        }
    }
}

// 实现 AsyncRead，转发到内部 stream
impl tokio::io::AsyncRead for AutoCleanStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(self.stream.as_mut().expect("stream is None")).poll_read(cx, buf)
    }
}

// 实现 AsyncWrite，转发到内部 stream
impl tokio::io::AsyncWrite for AutoCleanStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(self.stream.as_mut().expect("stream is None")).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(self.stream.as_mut().expect("stream is None")).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(self.stream.as_mut().expect("stream is None")).poll_shutdown(cx)
    }
}

// 实现 Unpin，因为 TcpStream 是 Unpin 的
impl Unpin for AutoCleanStream {}
