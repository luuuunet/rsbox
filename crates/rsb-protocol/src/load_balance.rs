// 负载均衡实现
use anyhow::Result;
use rsb_core::{BoxError, Outbound};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use dashmap::DashMap;

#[derive(Debug, Clone)]
pub enum LoadBalanceStrategy {
    RoundRobin,
    Random,
    LeastConnections,
    ConsistentHash,
}

pub struct LoadBalancer {
    tag: String,
    outbounds: Vec<Arc<dyn Outbound>>,
    strategy: LoadBalanceStrategy,
    counter: Arc<AtomicUsize>,
    connections: Arc<DashMap<String, AtomicUsize>>,
}

impl LoadBalancer {
    pub fn new(
        tag: String,
        outbounds: Vec<Arc<dyn Outbound>>,
        strategy: LoadBalanceStrategy,
    ) -> Self {
        Self {
            tag,
            outbounds,
            strategy,
            counter: Arc::new(AtomicUsize::new(0)),
            connections: Arc::new(DashMap::new()),
        }
    }

    pub fn select(&self, key: Option<&str>) -> Arc<dyn Outbound> {
        if self.outbounds.is_empty() {
            panic!("No outbounds available");
        }

        match self.strategy {
            LoadBalanceStrategy::RoundRobin => self.select_round_robin(),
            LoadBalanceStrategy::Random => self.select_random(),
            LoadBalanceStrategy::LeastConnections => self.select_least_connections(),
            LoadBalanceStrategy::ConsistentHash => {
                self.select_consistent_hash(key.unwrap_or(""))
            }
        }
    }

    fn select_round_robin(&self) -> Arc<dyn Outbound> {
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.outbounds.len();
        self.outbounds[idx].clone()
    }

    fn select_random(&self) -> Arc<dyn Outbound> {
        let idx = rand::random::<usize>() % self.outbounds.len();
        self.outbounds[idx].clone()
    }

    fn select_least_connections(&self) -> Arc<dyn Outbound> {
        self.outbounds
            .iter()
            .min_by_key(|ob| {
                self.connections
                    .entry(ob.tag().to_string())
                    .or_insert(AtomicUsize::new(0))
                    .load(Ordering::Relaxed)
            })
            .unwrap()
            .clone()
    }

    fn select_consistent_hash(&self, key: &str) -> Arc<dyn Outbound> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash = hasher.finish();

        let idx = (hash % self.outbounds.len() as u64) as usize;
        self.outbounds[idx].clone()
    }

    pub fn record_connection(&self, tag: &str) {
        self.connections
            .entry(tag.to_string())
            .or_insert(AtomicUsize::new(0))
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn release_connection(&self, tag: &str) {
        if let Some(entry) = self.connections.get(tag) {
            entry.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_robin() {
        let lb = LoadBalancer::new(
            "lb".to_string(),
            vec![],
            LoadBalanceStrategy::RoundRobin,
        );
        // 测试需要实际的 Outbound 实现
    }
}
