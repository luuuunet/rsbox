// ============================================
// tokio 重写版本 - 解决僵尸连接
// ============================================

// 关键改进：
// 1. 使用 tokio::time::timeout 防止连接挂起
// 2. 使用 tokio::select! 确保可被取消
// 3. 明确的资源清理路径

use tokio::time::{timeout, Duration};
use tokio::sync::Notify;
use std::sync::Arc;

// 改进的连接处理
async fn improved_handle_connection(
    mut stream: TcpStream,
    peer: SocketAddr,
    tag: String,
    kind: String,
    mode: ProxyMode,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    shutdown: Arc<Notify>,
) -> Result<()> {
    // 使用 select! 确保可被取消
    tokio::select! {
        result = async {
            // 添加超时保护
            let result = timeout(
                Duration::from_secs(300), // 5 分钟超时
                handle_client(&mut stream, peer, &tag, &kind, mode, dialer, dns)
            ).await;

            match result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e),
                Err(_) => {
                    tracing::debug!("Connection timeout");
                    Err(anyhow::anyhow!("timeout"))
                }
            }
        } => {
            if let Err(e) = result {
                tracing::debug!(error = %e, "Connection failed");
            }
        }
        _ = shutdown.notified() => {
            tracing::debug!("Connection cancelled by shutdown");
        }
    }

    // 确保 stream 被正确关闭
    use tokio::io::AsyncWriteExt;
    let _ = stream.shutdown().await;

    Ok(())
}

// 在 start() 函数中使用
let shutdown_notify = Arc::new(Notify::new());

tokio::spawn(async move {
    improved_handle_connection(
        stream,
        peer,
        tag,
        kind,
        mode,
        dialer,
        dns,
        shutdown_notify.clone(),
    ).await;
});
