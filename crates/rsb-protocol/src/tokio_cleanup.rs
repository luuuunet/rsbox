// ============================================
// tokio 多路复用 + 自动清理僵尸连接
// ============================================

use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

// 连接追踪器
struct ConnectionTracker {
    connections: Arc<dashmap::DashMap<u64, Arc<tokio::sync::Notify>>>,
    counter: AtomicUsize,
}

impl ConnectionTracker {
    fn new() -> Self {
        Self {
            connections: Arc::new(dashmap::DashMap::new()),
            counter: AtomicUsize::new(0),
        }
    }

    // 注册连接
    fn register(&self) -> (u64, Arc<tokio::sync::Notify>) {
        let id = self.counter.fetch_add(1, Ordering::SeqCst) as u64;
        let notify = Arc::new(tokio::sync::Notify::new());
        self.connections.insert(id, notify.clone());
        (id, notify)
    }

    // 注销连接
    fn unregister(&self, id: u64) {
        self.connections.remove(&id);
    }

    // 启动自动清理任务
    fn start_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(30)); // 每 30 秒检查一次

            loop {
                interval.tick().await;

                let count = self.connections.len();
                if count > 100 {
                    tracing::warn!("Too many active connections: {}, triggering cleanup", count);

                    // 通知所有连接进行健康检查
                    for entry in self.connections.iter() {
                        entry.value().notify_one();
                    }
                }

                tracing::debug!("Active connections: {}", count);
            }
        });
    }
}

// 改进的连接处理（带自动清理）
async fn handle_connection_with_cleanup(
    mut stream: TcpStream,
    peer: SocketAddr,
    tag: String,
    kind: String,
    mode: ProxyMode,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    tracker: Arc<ConnectionTracker>,
) -> Result<()> {
    let (conn_id, cleanup_notify) = tracker.register();

    // 确保连接被注销
    let _guard = scopeguard::guard(conn_id, |id| {
        tracker.unregister(id);
    });

    // 使用 select! 监听清理信号
    tokio::select! {
        result = tokio::time::timeout(
            Duration::from_secs(300),
            handle_client(&mut stream, peer, &tag, &kind, mode, dialer, dns)
        ) => {
            match result {
                Ok(Ok(())) => {
                    tracing::trace!("Connection completed");
                }
                Ok(Err(e)) => {
                    tracing::debug!(error = %e, "Connection failed");
                }
                Err(_) => {
                    tracing::debug!("Connection timeout");
                }
            }
        }
        _ = cleanup_notify.notified() => {
            tracing::debug!(conn_id, "Connection cleanup triggered");
        }
    }

    // 强制关闭
    use tokio::io::AsyncWriteExt;
    let _ = stream.shutdown().await;

    Ok(())
}

// 在 MixedInbound 中使用
impl MixedInbound {
    pub fn new(...) -> Result<Self> {
        // ...
        let tracker = Arc::new(ConnectionTracker::new());
        tracker.clone().start_cleanup_task();

        // 保存 tracker
        Ok(Self {
            // ...
            tracker,
        })
    }
}
