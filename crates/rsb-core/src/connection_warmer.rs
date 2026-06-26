// 连接预热实现
use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;

pub struct ConnectionWarmer {
    target_servers: Vec<SocketAddr>,
    warm_connections: usize,
    preconnect_on_start: bool,
    connection_pool: Arc<super::connection_pool::ConnectionPool>,
}

impl ConnectionWarmer {
    pub fn new(
        target_servers: Vec<SocketAddr>,
        warm_connections: usize,
        connection_pool: Arc<super::connection_pool::ConnectionPool>,
    ) -> Self {
        Self {
            target_servers,
            warm_connections,
            preconnect_on_start: true,
            connection_pool,
        }
    }

    /// 预热所有连接
    pub async fn warm_up(&self) -> Result<()> {
        tracing::info!(
            servers = self.target_servers.len(),
            connections_per_server = self.warm_connections,
            "Starting connection warm-up"
        );

        let semaphore = Arc::new(Semaphore::new(50)); // 限制并发
        let mut tasks = Vec::new();

        for server in &self.target_servers {
            for i in 0..self.warm_connections {
                let server = *server;
                let pool = self.connection_pool.clone();
                let sem = semaphore.clone();

                let task = tokio::spawn(async move {
                    let _permit = sem.acquire().await.unwrap();

                    match TcpStream::connect(server).await {
                        Ok(stream) => {
                            tracing::debug!(
                                server = %server,
                                connection = i,
                                "Connection warmed"
                            );

                            // 归还到连接池
                            pool.release(stream).await;
                        }
                        Err(e) => {
                            tracing::warn!(
                                server = %server,
                                error = %e,
                                "Failed to warm connection"
                            );
                        }
                    }
                });

                tasks.push(task);
            }
        }

        // 等待所有任务完成
        for task in tasks {
            task.await?;
        }

        tracing::info!("Connection warm-up completed");

        Ok(())
    }

    /// 启动后台预热任务
    pub fn start_background_warming(self: Arc<Self>) {
        if !self.preconnect_on_start {
            return;
        }

        tokio::spawn(async move {
            // 立即预热一次
            if let Err(e) = self.warm_up().await {
                tracing::error!(error = %e, "Initial warm-up failed");
            }

            // 定期刷新连接
            let mut interval = tokio::time::interval(Duration::from_secs(300));

            loop {
                interval.tick().await;

                if let Err(e) = self.warm_up().await {
                    tracing::error!(error = %e, "Background warm-up failed");
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection_warmer() {
        use super::super::connection_pool::ConnectionPool;

        let pool = Arc::new(ConnectionPool::new(
            10,
            Duration::from_secs(300),
            Duration::from_secs(90),
        ));

        let warmer = ConnectionWarmer::new(
            vec!["127.0.0.1:8080".parse().unwrap()],
            2,
            pool,
        );

        // 预热应该不报错（即使连接失败）
        let result = warmer.warm_up().await;
        assert!(result.is_ok());
    }
}
