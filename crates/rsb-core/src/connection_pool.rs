// 连接池管理实现
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::net::TcpStream;

pub struct ConnectionPool {
    max_idle: usize,
    max_lifetime: Duration,
    idle_timeout: Duration,
    connections: Arc<Mutex<VecDeque<PooledConnection>>>,
    stats: Arc<Mutex<PoolStats>>,
}

struct PooledConnection {
    stream: TcpStream,
    created_at: Instant,
    last_used: Instant,
    use_count: usize,
}

#[derive(Debug, Default)]
struct PoolStats {
    total_connections: usize,
    active_connections: usize,
    idle_connections: usize,
    reused_connections: usize,
    created_connections: usize,
}

impl ConnectionPool {
    pub fn new(max_idle: usize, max_lifetime: Duration, idle_timeout: Duration) -> Self {
        Self {
            max_idle,
            max_lifetime,
            idle_timeout,
            connections: Arc::new(Mutex::new(VecDeque::new())),
            stats: Arc::new(Mutex::new(PoolStats::default())),
        }
    }

    /// 从池中获取连接或创建新连接
    pub async fn acquire(&self, addr: std::net::SocketAddr) -> Result<TcpStream> {
        let mut pool = self.connections.lock().await;

        // 清理过期连接
        self.cleanup_expired(&mut pool).await;

        // 尝试从池中获取可用连接
        while let Some(mut pooled) = pool.pop_front() {
            // 检查连接是否仍然有效
            if pooled.is_valid() {
                pooled.last_used = Instant::now();
                pooled.use_count += 1;

                let mut stats = self.stats.lock().await;
                stats.reused_connections += 1;
                stats.active_connections += 1;
                stats.idle_connections = pool.len();

                tracing::debug!(
                    use_count = pooled.use_count,
                    age_secs = pooled.created_at.elapsed().as_secs(),
                    "Reusing connection from pool"
                );

                return Ok(pooled.stream);
            }
        }

        // 池中没有可用连接，创建新连接
        let stream = TcpStream::connect(addr).await?;

        let mut stats = self.stats.lock().await;
        stats.created_connections += 1;
        stats.active_connections += 1;
        stats.total_connections += 1;

        tracing::debug!(
            total = stats.total_connections,
            active = stats.active_connections,
            "Created new connection"
        );

        Ok(stream)
    }

    /// 归还连接到池
    pub async fn release(&self, stream: TcpStream) {
        let mut pool = self.connections.lock().await;

        // 检查池大小限制
        if pool.len() >= self.max_idle {
            tracing::debug!("Pool full, dropping connection");
            let mut stats = self.stats.lock().await;
            stats.active_connections = stats.active_connections.saturating_sub(1);
            return;
        }

        // 归还到池中
        pool.push_back(PooledConnection {
            stream,
            created_at: Instant::now(),
            last_used: Instant::now(),
            use_count: 0,
        });

        let mut stats = self.stats.lock().await;
        stats.active_connections = stats.active_connections.saturating_sub(1);
        stats.idle_connections = pool.len();

        tracing::debug!(
            idle = stats.idle_connections,
            "Connection returned to pool"
        );
    }

    /// 清理过期连接
    async fn cleanup_expired(&self, pool: &mut VecDeque<PooledConnection>) {
        let now = Instant::now();
        let mut removed = 0;

        pool.retain(|conn| {
            let age = now.duration_since(conn.created_at);
            let idle_time = now.duration_since(conn.last_used);

            let keep = age < self.max_lifetime && idle_time < self.idle_timeout;

            if !keep {
                removed += 1;
            }

            keep
        });

        if removed > 0 {
            tracing::debug!(removed = removed, "Cleaned up expired connections");
        }
    }

    /// 获取池统计信息
    pub async fn stats(&self) -> PoolStats {
        let stats = self.stats.lock().await;
        PoolStats {
            total_connections: stats.total_connections,
            active_connections: stats.active_connections,
            idle_connections: stats.idle_connections,
            reused_connections: stats.reused_connections,
            created_connections: stats.created_connections,
        }
    }

    /// 启动后台清理任务
    pub fn start_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));

            loop {
                interval.tick().await;

                let mut pool = self.connections.lock().await;
                self.cleanup_expired(&mut pool).await;
            }
        });
    }
}

impl PooledConnection {
    fn is_valid(&self) -> bool {
        let age = self.created_at.elapsed();
        let idle_time = self.last_used.elapsed();

        // 检查是否超过最大生命周期
        if age > Duration::from_secs(300) {
            return false;
        }

        // 检查是否空闲太久
        if idle_time > Duration::from_secs(90) {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_pool() {
        let pool = ConnectionPool::new(
            10,
            Duration::from_secs(300),
            Duration::from_secs(90),
        );

        let addr: std::net::SocketAddr = "127.0.0.1:8080".parse().unwrap();

        // 获取连接应该创建新连接
        let conn1 = pool.acquire(addr).await;
        assert!(conn1.is_ok());
    }
}
