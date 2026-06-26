// 智能健康检查实现
use anyhow::Result;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;

pub struct IntelligentHealthCheck {
    check_types: Vec<HealthCheckType>,
    scoring_weights: ScoringWeights,
}

#[derive(Debug, Clone)]
pub enum HealthCheckType {
    TcpConnect,
    HttpRequest,
    DnsQuery,
    ActualTraffic,
    LatencyTest,
    BandwidthTest,
    PacketLoss,
}

#[derive(Clone)]
pub struct ScoringWeights {
    pub connectivity: f64,
    pub latency: f64,
    pub bandwidth: f64,
    pub stability: f64,
    pub packet_loss: f64,
}

#[derive(Debug, Clone)]
pub struct HealthScore {
    pub overall: f64,        // 0-100
    pub connectivity: f64,
    pub latency_ms: f64,
    pub bandwidth_mbps: f64,
    pub packet_loss: f64,
    pub stability: f64,
    pub recommendation: Recommendation,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Recommendation {
    Excellent,
    Good,
    Fair,
    Poor,
    Critical,
}

impl IntelligentHealthCheck {
    pub fn new() -> Self {
        Self {
            check_types: vec![
                HealthCheckType::TcpConnect,
                HealthCheckType::LatencyTest,
                HealthCheckType::PacketLoss,
            ],
            scoring_weights: ScoringWeights {
                connectivity: 0.3,
                latency: 0.3,
                bandwidth: 0.2,
                stability: 0.15,
                packet_loss: 0.05,
            },
        }
    }

    /// 综合健康检查
    pub async fn comprehensive_check(&self, target: SocketAddr) -> Result<HealthScore> {
        let start = Instant::now();

        // 1. TCP 连接测试
        let connectivity = self.check_tcp_connect(&target).await?;

        // 2. 延迟测试（多次采样）
        let latency = self.measure_latency(&target, 5).await?;

        // 3. 丢包率测试
        let packet_loss = self.measure_packet_loss(&target).await?;

        // 4. 稳定性评分
        let stability = self.calculate_stability(&latency);

        // 计算综合得分
        let overall = self.calculate_overall_score(
            connectivity,
            latency,
            0.0, // bandwidth placeholder
            stability,
            packet_loss,
        );

        let recommendation = self.get_recommendation(overall);

        tracing::info!(
            target = %target,
            score = overall,
            latency_ms = latency,
            packet_loss = packet_loss,
            recommendation = ?recommendation,
            elapsed_ms = start.elapsed().as_millis(),
            "Health check completed"
        );

        Ok(HealthScore {
            overall,
            connectivity,
            latency_ms: latency,
            bandwidth_mbps: 0.0,
            packet_loss,
            stability,
            recommendation,
        })
    }

    /// TCP 连接测试
    async fn check_tcp_connect(&self, target: &SocketAddr) -> Result<f64> {
        match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(target)).await {
            Ok(Ok(_)) => Ok(100.0),
            Ok(Err(_)) => Ok(0.0),
            Err(_) => Ok(0.0),
        }
    }

    /// 延迟测试（多次采样）
    async fn measure_latency(&self, target: &SocketAddr, samples: usize) -> Result<f64> {
        let mut latencies = Vec::new();

        for _ in 0..samples {
            let start = Instant::now();
            if TcpStream::connect(target).await.is_ok() {
                latencies.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        if latencies.is_empty() {
            return Ok(999.0);
        }

        // 计算中位数
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
        Ok(latencies[latencies.len() / 2])
    }

    /// 丢包率测试
    async fn measure_packet_loss(&self, target: &SocketAddr) -> Result<f64> {
        let total_attempts = 10;
        let mut successful = 0;

        for _ in 0..total_attempts {
            if TcpStream::connect(target).await.is_ok() {
                successful += 1;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        Ok(1.0 - (successful as f64 / total_attempts as f64))
    }

    /// 计算稳定性
    fn calculate_stability(&self, latency: &f64) -> f64 {
        // 简化：基于延迟计算稳定性
        if *latency < 50.0 {
            100.0
        } else if *latency < 100.0 {
            80.0
        } else if *latency < 200.0 {
            60.0
        } else {
            40.0
        }
    }

    /// 计算综合得分
    fn calculate_overall_score(
        &self,
        connectivity: f64,
        latency: f64,
        bandwidth: f64,
        stability: f64,
        packet_loss: f64,
    ) -> f64 {
        let w = &self.scoring_weights;

        let latency_score = self.latency_to_score(latency);
        let packet_loss_score = (1.0 - packet_loss) * 100.0;

        let score = connectivity * w.connectivity
            + latency_score * w.latency
            + bandwidth * w.bandwidth
            + stability * w.stability
            + packet_loss_score * w.packet_loss;

        score.min(100.0).max(0.0)
    }

    /// 延迟转换为得分
    fn latency_to_score(&self, latency: f64) -> f64 {
        if latency < 50.0 {
            100.0
        } else if latency < 100.0 {
            90.0
        } else if latency < 200.0 {
            70.0
        } else if latency < 500.0 {
            40.0
        } else {
            20.0
        }
    }

    /// 获取推荐
    fn get_recommendation(&self, score: f64) -> Recommendation {
        if score >= 90.0 {
            Recommendation::Excellent
        } else if score >= 70.0 {
            Recommendation::Good
        } else if score >= 50.0 {
            Recommendation::Fair
        } else if score >= 30.0 {
            Recommendation::Poor
        } else {
            Recommendation::Critical
        }
    }
}

impl Default for IntelligentHealthCheck {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check() {
        let checker = IntelligentHealthCheck::new();
        let target: SocketAddr = "127.0.0.1:8080".parse().unwrap();

        // 健康检查应该完成（即使失败）
        let result = checker.comprehensive_check(target).await;
        assert!(result.is_ok());
    }
}
