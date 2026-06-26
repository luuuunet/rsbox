// 带宽预测和自适应实现
use anyhow::Result;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct BandwidthEstimator {
    samples: VecDeque<Sample>,
    window_size: usize,
    min_samples: usize,
}

#[derive(Clone)]
struct Sample {
    bytes: u64,
    duration: Duration,
    timestamp: Instant,
}

#[derive(Debug, Clone)]
pub struct ConnectionParams {
    pub window_size: usize,
    pub buffer_size: usize,
    pub congestion_control: String,
    pub multiplexing: bool,
}

impl BandwidthEstimator {
    pub fn new(window_size: usize) -> Self {
        Self {
            samples: VecDeque::new(),
            window_size,
            min_samples: 5,
        }
    }

    /// 添加样本
    pub fn add_sample(&mut self, bytes: u64, duration: Duration) {
        self.samples.push_back(Sample {
            bytes,
            duration,
            timestamp: Instant::now(),
        });

        // 限制样本数量
        while self.samples.len() > self.window_size {
            self.samples.pop_front();
        }
    }

    /// 估算可用带宽
    pub fn estimate_available_bandwidth(&self) -> Option<u64> {
        if self.samples.len() < self.min_samples {
            return None;
        }

        let total_bytes: u64 = self.samples.iter().map(|s| s.bytes).sum();
        let total_duration: Duration = self.samples.iter().map(|s| s.duration).sum();

        if total_duration.as_millis() == 0 {
            return None;
        }

        // 字节/秒
        let bandwidth = (total_bytes as f64 * 1000.0) / total_duration.as_millis() as f64;

        Some(bandwidth as u64)
    }

    /// 计算带宽变化趋势
    pub fn calculate_trend(&self) -> Option<f64> {
        if self.samples.len() < self.min_samples {
            return None;
        }

        let mid = self.samples.len() / 2;
        let first_half = &self.samples.as_slices().0[..mid];
        let second_half = &self.samples.as_slices().0[mid..];

        let first_avg = self.calculate_average(first_half);
        let second_avg = self.calculate_average(second_half);

        if first_avg == 0.0 {
            return None;
        }

        // 返回变化率
        Some((second_avg - first_avg) / first_avg)
    }

    fn calculate_average(&self, samples: &[Sample]) -> f64 {
        if samples.is_empty() {
            return 0.0;
        }

        let total_bytes: u64 = samples.iter().map(|s| s.bytes).sum();
        let total_duration: Duration = samples.iter().map(|s| s.duration).sum();

        if total_duration.as_millis() == 0 {
            return 0.0;
        }

        (total_bytes as f64 * 1000.0) / total_duration.as_millis() as f64
    }

    /// 自适应调整参数
    pub fn adjust_parameters(&self) -> Option<ConnectionParams> {
        let bandwidth = self.estimate_available_bandwidth()?;
        let trend = self.calculate_trend().unwrap_or(0.0);

        tracing::debug!(
            bandwidth_kbps = bandwidth / 1024,
            trend = %trend,
            "Adjusting connection parameters"
        );

        Some(ConnectionParams {
            window_size: self.calculate_optimal_window(bandwidth),
            buffer_size: self.calculate_optimal_buffer(bandwidth),
            congestion_control: self.select_algorithm(bandwidth),
            multiplexing: bandwidth > 1024 * 1024, // > 1 MB/s
        })
    }

    fn calculate_optimal_window(&self, bandwidth: u64) -> usize {
        // 带宽越高，窗口越大
        if bandwidth > 10 * 1024 * 1024 {
            // > 10 MB/s
            128 * 1024
        } else if bandwidth > 1024 * 1024 {
            // > 1 MB/s
            64 * 1024
        } else {
            32 * 1024
        }
    }

    fn calculate_optimal_buffer(&self, bandwidth: u64) -> usize {
        // 带宽越高，缓冲区越大
        if bandwidth > 10 * 1024 * 1024 {
            256 * 1024
        } else if bandwidth > 1024 * 1024 {
            128 * 1024
        } else {
            64 * 1024
        }
    }

    fn select_algorithm(&self, bandwidth: u64) -> String {
        // 高带宽使用 BBR，低带宽使用 Cubic
        if bandwidth > 5 * 1024 * 1024 {
            "bbr".to_string()
        } else {
            "cubic".to_string()
        }
    }

    /// 清理旧样本
    pub fn cleanup_old_samples(&mut self, max_age: Duration) {
        let now = Instant::now();

        self.samples.retain(|sample| {
            now.duration_since(sample.timestamp) < max_age
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bandwidth_estimation() {
        let mut estimator = BandwidthEstimator::new(10);

        // 添加样本
        for _ in 0..10 {
            estimator.add_sample(1024 * 1024, Duration::from_secs(1));
        }

        let bandwidth = estimator.estimate_available_bandwidth();
        assert!(bandwidth.is_some());

        let params = estimator.adjust_parameters();
        assert!(params.is_some());
    }
}
