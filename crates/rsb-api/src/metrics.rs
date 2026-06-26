// 实时指标监控实现
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metrics {
    pub connections_active: u64,
    pub connections_total: u64,
    pub connections_failed: u64,

    pub bytes_sent: u64,
    pub bytes_received: u64,

    pub requests_total: u64,
    pub requests_success: u64,
    pub requests_failed: u64,

    pub latency_p50: f64,
    pub latency_p95: f64,
    pub latency_p99: f64,

    pub errors_total: u64,
    pub errors_by_type: std::collections::HashMap<String, u64>,

    pub uptime_seconds: u64,
    pub last_updated: std::time::SystemTime,
}

pub struct MetricsCollector {
    metrics: Arc<RwLock<Metrics>>,
    latency_samples: Arc<RwLock<Vec<Duration>>>,
    max_samples: usize,
    start_time: Instant,
}

impl MetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(Metrics::default())),
            latency_samples: Arc::new(RwLock::new(Vec::new())),
            max_samples: 1000,
            start_time: Instant::now(),
        }
    }

    /// 记录新连接
    pub async fn record_connection(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.connections_active += 1;
        metrics.connections_total += 1;
    }

    /// 记录连接关闭
    pub async fn record_connection_closed(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.connections_active = metrics.connections_active.saturating_sub(1);
    }

    /// 记录连接失败
    pub async fn record_connection_failed(&self) {
        let mut metrics = self.metrics.write().await;
        metrics.connections_failed += 1;
    }

    /// 记录流量
    pub async fn record_traffic(&self, sent: u64, received: u64) {
        let mut metrics = self.metrics.write().await;
        metrics.bytes_sent += sent;
        metrics.bytes_received += received;
    }

    /// 记录请求
    pub async fn record_request(&self, success: bool, latency: Duration) {
        let mut metrics = self.metrics.write().await;
        metrics.requests_total += 1;

        if success {
            metrics.requests_success += 1;
        } else {
            metrics.requests_failed += 1;
        }

        // 添加延迟样本
        let mut samples = self.latency_samples.write().await;
        samples.push(latency);

        // 限制样本数量
        if samples.len() > self.max_samples {
            samples.remove(0);
        }

        // 更新延迟百分位
        drop(samples);
        self.update_latency_percentiles().await;
    }

    /// 记录错误
    pub async fn record_error(&self, error_type: String) {
        let mut metrics = self.metrics.write().await;
        metrics.errors_total += 1;
        *metrics.errors_by_type.entry(error_type).or_insert(0) += 1;
    }

    /// 更新延迟百分位
    async fn update_latency_percentiles(&self) {
        let samples = self.latency_samples.read().await;

        if samples.is_empty() {
            return;
        }

        let mut sorted: Vec<f64> = samples
            .iter()
            .map(|d| d.as_secs_f64() * 1000.0) // 转换为毫秒
            .collect();

        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());

        let mut metrics = self.metrics.write().await;

        metrics.latency_p50 = percentile(&sorted, 0.50);
        metrics.latency_p95 = percentile(&sorted, 0.95);
        metrics.latency_p99 = percentile(&sorted, 0.99);
    }

    /// 获取当前指标
    pub async fn get_metrics(&self) -> Metrics {
        let mut metrics = self.metrics.read().await.clone();
        metrics.uptime_seconds = self.start_time.elapsed().as_secs();
        metrics.last_updated = std::time::SystemTime::now();
        metrics
    }

    /// 重置指标
    pub async fn reset(&self) {
        let mut metrics = self.metrics.write().await;
        *metrics = Metrics::default();

        let mut samples = self.latency_samples.write().await;
        samples.clear();
    }

    /// 获取指标摘要
    pub async fn summary(&self) -> String {
        let metrics = self.get_metrics().await;

        format!(
            "Connections: {} active, {} total, {} failed | \
             Traffic: {} sent, {} recv | \
             Requests: {} total, {} success ({:.1}%) | \
             Latency: p50={:.1}ms, p95={:.1}ms, p99={:.1}ms | \
             Errors: {}",
            metrics.connections_active,
            metrics.connections_total,
            metrics.connections_failed,
            format_bytes(metrics.bytes_sent),
            format_bytes(metrics.bytes_received),
            metrics.requests_total,
            metrics.requests_success,
            if metrics.requests_total > 0 {
                (metrics.requests_success as f64 / metrics.requests_total as f64) * 100.0
            } else {
                0.0
            },
            metrics.latency_p50,
            metrics.latency_p95,
            metrics.latency_p99,
            metrics.errors_total
        )
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            connections_active: 0,
            connections_total: 0,
            connections_failed: 0,
            bytes_sent: 0,
            bytes_received: 0,
            requests_total: 0,
            requests_success: 0,
            requests_failed: 0,
            latency_p50: 0.0,
            latency_p95: 0.0,
            latency_p99: 0.0,
            errors_total: 0,
            errors_by_type: std::collections::HashMap::new(),
            uptime_seconds: 0,
            last_updated: std::time::SystemTime::now(),
        }
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }

    let index = ((sorted.len() as f64 - 1.0) * p) as usize;
    sorted[index]
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_collector() {
        let collector = MetricsCollector::new();

        collector.record_connection().await;
        collector.record_request(true, Duration::from_millis(50)).await;
        collector.record_traffic(1024, 2048).await;

        let metrics = collector.get_metrics().await;
        assert_eq!(metrics.connections_active, 1);
        assert_eq!(metrics.bytes_sent, 1024);
        assert_eq!(metrics.bytes_received, 2048);
    }
}
