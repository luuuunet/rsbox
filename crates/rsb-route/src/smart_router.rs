// 智能路由选择实现
use anyhow::Result;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct SmartRouter {
    routes: Vec<Route>,
    metrics: Arc<DashMap<String, RouteMetrics>>,
}

#[derive(Clone)]
pub struct Route {
    pub name: String,
    pub outbound: String,
    pub priority: u32,
}

#[derive(Clone)]
pub struct RouteMetrics {
    pub latency: Duration,
    pub success_rate: f64,
    pub bandwidth: u64,
    pub last_failure: Option<Instant>,
    pub total_requests: u64,
    pub failed_requests: u64,
    pub last_updated: Instant,
}

impl SmartRouter {
    pub fn new(routes: Vec<Route>) -> Self {
        let metrics = Arc::new(DashMap::new());

        // 初始化指标
        for route in &routes {
            metrics.insert(
                route.name.clone(),
                RouteMetrics {
                    latency: Duration::from_millis(100),
                    success_rate: 1.0,
                    bandwidth: 0,
                    last_failure: None,
                    total_requests: 0,
                    failed_requests: 0,
                    last_updated: Instant::now(),
                },
            );
        }

        Self { routes, metrics }
    }

    /// 选择最佳路由
    pub fn select_best_route(&self, _destination: &str) -> Option<&Route> {
        self.routes
            .iter()
            .max_by_key(|route| self.calculate_score(route))
    }

    /// 计算路由得分
    fn calculate_score(&self, route: &Route) -> u64 {
        let metrics = match self.metrics.get(&route.name) {
            Some(m) => m.clone(),
            None => return 0,
        };

        // 延迟得分（越低越好）
        let latency_score = if metrics.latency.as_millis() > 0 {
            1000 / metrics.latency.as_millis() as u64
        } else {
            1000
        };

        // 成功率得分
        let success_score = (metrics.success_rate * 1000.0) as u64;

        // 带宽得分
        let bandwidth_score = metrics.bandwidth / 1024;

        // 优先级得分
        let priority_score = route.priority as u64 * 100;

        // 最近失败惩罚
        let failure_penalty = if let Some(last_failure) = metrics.last_failure {
            let since_failure = Instant::now().duration_since(last_failure);
            if since_failure < Duration::from_secs(60) {
                500 // 最近失败，减分
            } else {
                0
            }
        } else {
            0
        };

        latency_score + success_score + bandwidth_score + priority_score - failure_penalty
    }

    /// 记录成功请求
    pub fn record_success(&self, route_name: &str, latency: Duration, bytes: u64) {
        if let Some(mut metrics) = self.metrics.get_mut(route_name) {
            metrics.total_requests += 1;
            metrics.latency = latency;
            metrics.bandwidth = bytes;
            metrics.success_rate =
                (metrics.total_requests - metrics.failed_requests) as f64
                    / metrics.total_requests as f64;
            metrics.last_updated = Instant::now();

            tracing::debug!(
                route = %route_name,
                latency_ms = latency.as_millis(),
                success_rate = %metrics.success_rate,
                "Route success recorded"
            );
        }
    }

    /// 记录失败请求
    pub fn record_failure(&self, route_name: &str) {
        if let Some(mut metrics) = self.metrics.get_mut(route_name) {
            metrics.total_requests += 1;
            metrics.failed_requests += 1;
            metrics.last_failure = Some(Instant::now());
            metrics.success_rate =
                (metrics.total_requests - metrics.failed_requests) as f64
                    / metrics.total_requests as f64;
            metrics.last_updated = Instant::now();

            tracing::warn!(
                route = %route_name,
                success_rate = %metrics.success_rate,
                "Route failure recorded"
            );
        }
    }

    /// 获取所有路由指标
    pub fn get_all_metrics(&self) -> Vec<(String, RouteMetrics)> {
        self.metrics
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smart_router() {
        let routes = vec![
            Route {
                name: "route1".to_string(),
                outbound: "proxy1".to_string(),
                priority: 10,
            },
            Route {
                name: "route2".to_string(),
                outbound: "proxy2".to_string(),
                priority: 5,
            },
        ];

        let router = SmartRouter::new(routes);

        // 记录成功
        router.record_success("route1", Duration::from_millis(50), 1024);

        // 选择最佳路由
        let best = router.select_best_route("example.com");
        assert!(best.is_some());
    }
}
