// 连接状态监控实现
use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::interval;

pub struct ConnectionMonitor {
    check_interval: Duration,
    timeout: Duration,
    failure_threshold: usize,
    connections: Arc<RwLock<Vec<MonitoredConnection>>>,
}

#[derive(Clone)]
pub struct MonitoredConnection {
    pub id: String,
    pub addr: std::net::SocketAddr,
    pub created_at: Instant,
    pub last_check: Instant,
    pub last_success: Instant,
    pub consecutive_failures: usize,
    pub total_checks: usize,
    pub total_failures: usize,
    pub status: ConnectionStatus,
    pub latency: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Healthy,
    Degraded,
    Failed,
    Unknown,
}

impl ConnectionMonitor {
    pub fn new(check_interval: Duration, timeout: Duration, failure_threshold: usize) -> Self {
        Self {
            check_interval,
            timeout,
            failure_threshold,
            connections: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// 添加连接到监控
    pub async fn add_connection(&self, id: String, addr: std::net::SocketAddr) {
        let mut conns = self.connections.write().await;

        conns.push(MonitoredConnection {
            id: id.clone(),
            addr,
            created_at: Instant::now(),
            last_check: Instant::now(),
            last_success: Instant::now(),
            consecutive_failures: 0,
            total_checks: 0,
            total_failures: 0,
            status: ConnectionStatus::Unknown,
            latency: None,
        });

        tracing::debug!(id = %id, addr = %addr, "Connection added to monitor");
    }

    /// 移除连接
    pub async fn remove_connection(&self, id: &str) {
        let mut conns = self.connections.write().await;
        conns.retain(|c| c.id != id);

        tracing::debug!(id = %id, "Connection removed from monitor");
    }

    /// 启动监控任务
    pub fn start_monitoring(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = interval(self.check_interval);

            loop {
                ticker.tick().await;

                let conns = {
                    let conns = self.connections.read().await;
                    conns.clone()
                };

                for conn in conns {
                    self.check_connection(conn).await;
                }

                // 输出监控统计
                self.log_statistics().await;
            }
        });
    }

    /// 检查单个连接
    async fn check_connection(&self, mut conn: MonitoredConnection) {
        let start = Instant::now();

        // 执行健康检查（简单的 TCP 连接测试）
        let result = tokio::time::timeout(
            self.timeout,
            self.health_check(&conn.addr),
        )
        .await;

        let latency = start.elapsed();
        conn.last_check = Instant::now();
        conn.total_checks += 1;

        match result {
            Ok(Ok(())) => {
                // 检查成功
                conn.consecutive_failures = 0;
                conn.last_success = Instant::now();
                conn.latency = Some(latency);

                // 更新状态
                conn.status = if latency < Duration::from_millis(100) {
                    ConnectionStatus::Healthy
                } else if latency < Duration::from_secs(1) {
                    ConnectionStatus::Degraded
                } else {
                    ConnectionStatus::Degraded
                };

                tracing::trace!(
                    id = %conn.id,
                    latency_ms = latency.as_millis(),
                    status = ?conn.status,
                    "Health check passed"
                );
            }
            Ok(Err(e)) | Err(_) => {
                // 检查失败
                conn.consecutive_failures += 1;
                conn.total_failures += 1;
                conn.latency = None;

                if conn.consecutive_failures >= self.failure_threshold {
                    conn.status = ConnectionStatus::Failed;

                    tracing::warn!(
                        id = %conn.id,
                        consecutive_failures = conn.consecutive_failures,
                        "Connection marked as failed"
                    );

                    // 触发重连或切换
                    self.handle_connection_failure(&conn).await;
                } else {
                    conn.status = ConnectionStatus::Degraded;

                    tracing::debug!(
                        id = %conn.id,
                        consecutive_failures = conn.consecutive_failures,
                        "Health check failed"
                    );
                }
            }
        }

        // 更新连接状态
        self.update_connection(conn).await;
    }

    /// 简单的健康检查
    async fn health_check(&self, addr: &std::net::SocketAddr) -> Result<()> {
        use tokio::net::TcpStream;

        // 尝试建立 TCP 连接
        let _stream = TcpStream::connect(addr).await?;
        Ok(())
    }

    /// 更新连接状态
    async fn update_connection(&self, updated: MonitoredConnection) {
        let mut conns = self.connections.write().await;

        if let Some(conn) = conns.iter_mut().find(|c| c.id == updated.id) {
            *conn = updated;
        }
    }

    /// 处理连接失败
    async fn handle_connection_failure(&self, conn: &MonitoredConnection) {
        tracing::error!(
            id = %conn.id,
            addr = %conn.addr,
            consecutive_failures = conn.consecutive_failures,
            total_failures = conn.total_failures,
            uptime_secs = conn.created_at.elapsed().as_secs(),
            "Connection failure threshold reached"
        );

        // TODO: 触发重连或切换到备用连接
        // 这里可以发送事件通知上层处理
    }

    /// 输出监控统计
    async fn log_statistics(&self) {
        let conns = self.connections.read().await;

        if conns.is_empty() {
            return;
        }

        let total = conns.len();
        let healthy = conns.iter().filter(|c| c.status == ConnectionStatus::Healthy).count();
        let degraded = conns.iter().filter(|c| c.status == ConnectionStatus::Degraded).count();
        let failed = conns.iter().filter(|c| c.status == ConnectionStatus::Failed).count();

        let avg_latency = conns
            .iter()
            .filter_map(|c| c.latency)
            .map(|d| d.as_millis())
            .sum::<u128>() as f64
            / conns.iter().filter(|c| c.latency.is_some()).count().max(1) as f64;

        tracing::info!(
            total = total,
            healthy = healthy,
            degraded = degraded,
            failed = failed,
            avg_latency_ms = avg_latency as u64,
            "Connection monitor statistics"
        );
    }

    /// 获取所有连接状态
    pub async fn get_all_connections(&self) -> Vec<MonitoredConnection> {
        let conns = self.connections.read().await;
        conns.clone()
    }

    /// 获取健康的连接数
    pub async fn healthy_count(&self) -> usize {
        let conns = self.connections.read().await;
        conns.iter().filter(|c| c.status == ConnectionStatus::Healthy).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_monitor() {
        let monitor = Arc::new(ConnectionMonitor::new(
            Duration::from_secs(5),
            Duration::from_secs(2),
            3,
        ));

        let addr: std::net::SocketAddr = "127.0.0.1:8080".parse().unwrap();
        monitor.add_connection("test-conn".to_string(), addr).await;

        let conns = monitor.get_all_connections().await;
        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].id, "test-conn");
    }
}
