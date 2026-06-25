// 故障转移实现
use anyhow::Result;
use rsb_core::Outbound;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::interval;
use dashmap::DashMap;

pub struct Failover {
    tag: String,
    outbounds: Vec<Arc<dyn Outbound>>,
    health_check_url: String,
    check_interval: Duration,
    health_status: Arc<DashMap<String, HealthStatus>>,
}

#[derive(Debug, Clone)]
struct HealthStatus {
    healthy: bool,
    last_check: Instant,
    consecutive_failures: usize,
}

impl Failover {
    pub fn new(
        tag: String,
        outbounds: Vec<Arc<dyn Outbound>>,
        health_check_url: String,
        check_interval: Duration,
    ) -> Self {
        let health_status = Arc::new(DashMap::new());

        // 初始化所有节点为健康
        for outbound in &outbounds {
            health_status.insert(
                outbound.tag().to_string(),
                HealthStatus {
                    healthy: true,
                    last_check: Instant::now(),
                    consecutive_failures: 0,
                },
            );
        }

        Self {
            tag,
            outbounds,
            health_check_url,
            check_interval,
            health_status,
        }
    }

    pub fn select(&self) -> Option<Arc<dyn Outbound>> {
        // 选择第一个健康的节点
        for outbound in &self.outbounds {
            if let Some(status) = self.health_status.get(outbound.tag()) {
                if status.healthy {
                    tracing::debug!(
                        tag = %outbound.tag(),
                        "Selected healthy outbound"
                    );
                    return Some(outbound.clone());
                }
            }
        }

        tracing::warn!("No healthy outbound available, using first one");
        self.outbounds.first().cloned()
    }

    pub async fn start_health_check(self: Arc<Self>) {
        let mut ticker = interval(self.check_interval);

        tokio::spawn(async move {
            loop {
                ticker.tick().await;

                tracing::debug!("Starting health check");

                for outbound in &self.outbounds {
                    let outbound_clone = outbound.clone();
                    let url = self.health_check_url.clone();
                    let health_status = self.health_status.clone();
                    let tag = outbound.tag().to_string();

                    tokio::spawn(async move {
                        let is_healthy = Self::check_health(&outbound_clone, &url).await;

                        health_status
                            .entry(tag.clone())
                            .and_modify(|status| {
                                if is_healthy {
                                    status.healthy = true;
                                    status.consecutive_failures = 0;
                                    tracing::info!(tag = %tag, "Health check passed");
                                } else {
                                    status.consecutive_failures += 1;
                                    if status.consecutive_failures >= 3 {
                                        status.healthy = false;
                                        tracing::warn!(
                                            tag = %tag,
                                            failures = status.consecutive_failures,
                                            "Health check failed, marking as unhealthy"
                                        );
                                    }
                                }
                                status.last_check = Instant::now();
                            });
                    });
                }
            }
        });
    }

    async fn check_health(outbound: &Arc<dyn Outbound>, url: &str) -> bool {
        use tokio::time::timeout;

        let result = timeout(
            Duration::from_secs(5),
            async {
                // 尝试连接到健康检查 URL
                let addr: std::net::SocketAddr = url.parse().ok()?;
                outbound.dial_tcp(addr, None).await.ok()
            }
        ).await;

        result.is_ok() && result.unwrap().is_some()
    }

    pub fn get_health_status(&self) -> Vec<(String, bool)> {
        self.health_status
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().healthy))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failover_creation() {
        let failover = Failover::new(
            "failover".to_string(),
            vec![],
            "https://www.gstatic.com/generate_204".to_string(),
            Duration::from_secs(60),
        );

        assert_eq!(failover.tag, "failover");
    }
}
