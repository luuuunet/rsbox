// 流量统计和QoS实现
use anyhow::Result;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::interval;

pub struct QoSManager {
    bandwidth_limit: Option<u64>, // bytes per second
    stats: Arc<RwLock<TrafficStats>>,
    samples: Arc<RwLock<VecDeque<Sample>>>,
    window_size: usize,
}

#[derive(Default)]
pub struct TrafficStats {
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub total_packets_sent: u64,
    pub total_packets_received: u64,
    pub current_upload_speed: u64,   // bytes/s
    pub current_download_speed: u64, // bytes/s
    pub peak_upload_speed: u64,
    pub peak_download_speed: u64,
}

#[derive(Clone)]
struct Sample {
    timestamp: Instant,
    bytes_sent: u64,
    bytes_received: u64,
}

impl QoSManager {
    pub fn new(bandwidth_limit: Option<u64>) -> Self {
        Self {
            bandwidth_limit,
            stats: Arc::new(RwLock::new(TrafficStats::default())),
            samples: Arc::new(RwLock::new(VecDeque::new())),
            window_size: 10, // 10 秒窗口
        }
    }

    /// 记录发送流量
    pub async fn record_sent(&self, bytes: u64) {
        let mut stats = self.stats.write().await;
        stats.total_bytes_sent += bytes;
        stats.total_packets_sent += 1;

        // 添加样本
        let mut samples = self.samples.write().await;
        samples.push_back(Sample {
            timestamp: Instant::now(),
            bytes_sent: bytes,
            bytes_received: 0,
        });

        // 限制样本数量
        if samples.len() > self.window_size * 10 {
            samples.pop_front();
        }

        tracing::trace!(bytes = bytes, total = stats.total_bytes_sent, "Traffic sent");
    }

    /// 记录接收流量
    pub async fn record_received(&self, bytes: u64) {
        let mut stats = self.stats.write().await;
        stats.total_bytes_received += bytes;
        stats.total_packets_received += 1;

        // 添加样本
        let mut samples = self.samples.write().await;
        samples.push_back(Sample {
            timestamp: Instant::now(),
            bytes_sent: 0,
            bytes_received: bytes,
        });

        // 限制样本数量
        if samples.len() > self.window_size * 10 {
            samples.pop_front();
        }

        tracing::trace!(bytes = bytes, total = stats.total_bytes_received, "Traffic received");
    }

    /// 计算当前速度
    async fn calculate_speeds(&self) {
        let samples = self.samples.read().await;
        let now = Instant::now();
        let window = Duration::from_secs(self.window_size as u64);

        let mut bytes_sent = 0u64;
        let mut bytes_received = 0u64;

        for sample in samples.iter().rev() {
            if now.duration_since(sample.timestamp) <= window {
                bytes_sent += sample.bytes_sent;
                bytes_received += sample.bytes_received;
            } else {
                break;
            }
        }

        let upload_speed = bytes_sent / self.window_size as u64;
        let download_speed = bytes_received / self.window_size as u64;

        let mut stats = self.stats.write().await;
        stats.current_upload_speed = upload_speed;
        stats.current_download_speed = download_speed;
        stats.peak_upload_speed = stats.peak_upload_speed.max(upload_speed);
        stats.peak_download_speed = stats.peak_download_speed.max(download_speed);
    }

    /// 检查带宽限制
    pub async fn check_bandwidth_limit(&self) -> bool {
        if let Some(limit) = self.bandwidth_limit {
            let stats = self.stats.read().await;
            stats.current_upload_speed < limit
        } else {
            true
        }
    }

    /// 获取统计信息
    pub async fn get_stats(&self) -> TrafficStats {
        let stats = self.stats.read().await;
        TrafficStats {
            total_bytes_sent: stats.total_bytes_sent,
            total_bytes_received: stats.total_bytes_received,
            total_packets_sent: stats.total_packets_sent,
            total_packets_received: stats.total_packets_received,
            current_upload_speed: stats.current_upload_speed,
            current_download_speed: stats.current_download_speed,
            peak_upload_speed: stats.peak_upload_speed,
            peak_download_speed: stats.peak_download_speed,
        }
    }

    /// 启动统计任务
    pub fn start_stats_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(1));

            loop {
                ticker.tick().await;
                self.calculate_speeds().await;

                // 每 10 秒输出一次统计
                if ticker.ticks() % 10 == 0 {
                    let stats = self.get_stats().await;
                    tracing::info!(
                        upload_kbps = stats.current_upload_speed / 1024,
                        download_kbps = stats.current_download_speed / 1024,
                        total_sent_mb = stats.total_bytes_sent / 1024 / 1024,
                        total_received_mb = stats.total_bytes_received / 1024 / 1024,
                        "Traffic statistics"
                    );
                }
            }
        });
    }

    /// 格式化速度
    pub fn format_speed(bytes_per_sec: u64) -> String {
        if bytes_per_sec < 1024 {
            format!("{} B/s", bytes_per_sec)
        } else if bytes_per_sec < 1024 * 1024 {
            format!("{:.2} KB/s", bytes_per_sec as f64 / 1024.0)
        } else {
            format!("{:.2} MB/s", bytes_per_sec as f64 / 1024.0 / 1024.0)
        }
    }

    /// 格式化数据量
    pub fn format_bytes(bytes: u64) -> String {
        if bytes < 1024 {
            format!("{} B", bytes)
        } else if bytes < 1024 * 1024 {
            format!("{:.2} KB", bytes as f64 / 1024.0)
        } else if bytes < 1024 * 1024 * 1024 {
            format!("{:.2} MB", bytes as f64 / 1024.0 / 1024.0)
        } else {
            format!("{:.2} GB", bytes as f64 / 1024.0 / 1024.0 / 1024.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_traffic_stats() {
        let qos = Arc::new(QoSManager::new(None));

        qos.record_sent(1024).await;
        qos.record_received(2048).await;

        let stats = qos.get_stats().await;
        assert_eq!(stats.total_bytes_sent, 1024);
        assert_eq!(stats.total_bytes_received, 2048);
    }

    #[test]
    fn test_format_speed() {
        assert_eq!(QoSManager::format_speed(512), "512 B/s");
        assert_eq!(QoSManager::format_speed(1024), "1.00 KB/s");
        assert_eq!(QoSManager::format_speed(1024 * 1024), "1.00 MB/s");
    }
}
