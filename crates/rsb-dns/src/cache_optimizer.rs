// DNS 缓存优化实现
use anyhow::Result;
use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::interval;

pub struct DnsCacheOptimizer {
    cache: Arc<DashMap<String, CachedRecord>>,
    ttl_multiplier: f64,
    prefetch_threshold: f64,
    max_cache_size: usize,
}

#[derive(Clone)]
pub struct CachedRecord {
    pub addr: IpAddr,
    pub cached_at: Instant,
    pub ttl: Duration,
    pub hits: usize,
}

impl DnsCacheOptimizer {
    pub fn new(ttl_multiplier: f64, prefetch_threshold: f64, max_cache_size: usize) -> Self {
        Self {
            cache: Arc::new(DashMap::new()),
            ttl_multiplier,
            prefetch_threshold,
            max_cache_size,
        }
    }

    /// 获取或查询 DNS
    pub async fn get_or_fetch(&self, domain: &str) -> Result<IpAddr> {
        // 1. 检查缓存
        if let Some(mut cached) = self.cache.get_mut(domain) {
            cached.hits += 1;

            if !cached.is_expired() {
                let remaining_ratio = cached.remaining_ratio();

                tracing::trace!(
                    domain = %domain,
                    hits = cached.hits,
                    remaining_ratio = %remaining_ratio,
                    "Cache hit"
                );

                // 2. 预取：TTL 剩余 20% 时后台更新
                if remaining_ratio < self.prefetch_threshold {
                    tracing::debug!(
                        domain = %domain,
                        remaining_ratio = %remaining_ratio,
                        "Prefetching DNS in background"
                    );

                    let domain_clone = domain.to_string();
                    let cache = self.cache.clone();
                    tokio::spawn(async move {
                        if let Ok(addr) = Self::fetch(&domain_clone).await {
                            cache.insert(domain_clone, CachedRecord::new(addr));
                        }
                    });
                }

                return Ok(cached.addr);
            } else {
                tracing::debug!(domain = %domain, "Cache expired");
            }
        }

        // 3. 缓存失效或不存在，查询
        let addr = Self::fetch(domain).await?;

        // 检查缓存大小限制
        if self.cache.len() >= self.max_cache_size {
            self.evict_lru().await;
        }

        self.cache.insert(domain.to_string(), CachedRecord::new(addr));

        tracing::debug!(
            domain = %domain,
            addr = %addr,
            "DNS cached"
        );

        Ok(addr)
    }

    /// 实际的 DNS 查询
    async fn fetch(domain: &str) -> Result<IpAddr> {
        use tokio::net::lookup_host;

        let mut addrs = lookup_host(format!("{}:0", domain)).await?;

        addrs
            .next()
            .map(|addr| addr.ip())
            .ok_or_else(|| anyhow::anyhow!("No address found"))
    }

    /// LRU 驱逐策略
    async fn evict_lru(&self) {
        // 找到最少使用的记录
        let mut min_hits = usize::MAX;
        let mut evict_key = None;

        for entry in self.cache.iter() {
            if entry.value().hits < min_hits {
                min_hits = entry.value().hits;
                evict_key = Some(entry.key().clone());
            }
        }

        if let Some(key) = evict_key {
            self.cache.remove(&key);
            tracing::debug!(domain = %key, hits = min_hits, "Evicted from cache");
        }
    }

    /// 清理过期缓存
    pub async fn cleanup_expired(&self) {
        let mut expired = Vec::new();

        for entry in self.cache.iter() {
            if entry.value().is_expired() {
                expired.push(entry.key().clone());
            }
        }

        for key in expired {
            self.cache.remove(&key);
            tracing::trace!(domain = %key, "Removed expired cache");
        }
    }

    /// 启动后台清理任务
    pub fn start_cleanup_task(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(60));

            loop {
                ticker.tick().await;
                self.cleanup_expired().await;

                tracing::debug!(
                    cache_size = self.cache.len(),
                    "DNS cache cleanup completed"
                );
            }
        });
    }

    /// 获取缓存统计
    pub fn stats(&self) -> CacheStats {
        let mut total_hits = 0;
        let mut expired = 0;

        for entry in self.cache.iter() {
            total_hits += entry.value().hits;
            if entry.value().is_expired() {
                expired += 1;
            }
        }

        CacheStats {
            total_entries: self.cache.len(),
            total_hits,
            expired_entries: expired,
        }
    }
}

impl CachedRecord {
    pub fn new(addr: IpAddr) -> Self {
        Self {
            addr,
            cached_at: Instant::now(),
            ttl: Duration::from_secs(300), // 默认 5 分钟
            hits: 0,
        }
    }

    pub fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }

    pub fn remaining_ratio(&self) -> f64 {
        let elapsed = self.cached_at.elapsed();
        let remaining = self.ttl.checked_sub(elapsed).unwrap_or(Duration::ZERO);

        remaining.as_secs_f64() / self.ttl.as_secs_f64()
    }
}

#[derive(Debug)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_hits: usize,
    pub expired_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dns_cache() {
        let optimizer = Arc::new(DnsCacheOptimizer::new(2.0, 0.2, 1000));

        // 第一次查询
        let result = optimizer.get_or_fetch("google.com").await;
        assert!(result.is_ok());

        // 第二次应该命中缓存
        let result2 = optimizer.get_or_fetch("google.com").await;
        assert!(result2.is_ok());

        let stats = optimizer.stats();
        assert_eq!(stats.total_entries, 1);
    }
}
