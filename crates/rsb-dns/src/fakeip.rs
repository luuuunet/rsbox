// FakeIP 实现
use anyhow::Result;
use dashmap::DashMap;
use ipnet::{Ipv4Net, Ipv6Net};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

pub struct FakeIpPool {
    inet4_range: Ipv4Net,
    inet6_range: Ipv6Net,
    inet4_offset: AtomicU32,
    inet6_offset: AtomicU64,
    domain_to_ip: Arc<DashMap<String, IpAddr>>,
    ip_to_domain: Arc<DashMap<IpAddr, String>>,
}

impl FakeIpPool {
    pub fn new(inet4_range: &str, inet6_range: &str) -> Result<Self> {
        Ok(Self {
            inet4_range: inet4_range.parse()?,
            inet6_range: inet6_range.parse()?,
            inet4_offset: AtomicU32::new(1),
            inet6_offset: AtomicU64::new(1),
            domain_to_ip: Arc::new(DashMap::new()),
            ip_to_domain: Arc::new(DashMap::new()),
        })
    }

    /// 为域名分配 FakeIP
    pub fn lookup(&self, domain: &str) -> IpAddr {
        // 检查缓存
        if let Some(ip) = self.domain_to_ip.get(domain) {
            tracing::trace!(domain = %domain, ip = %ip.value(), "FakeIP cache hit");
            return *ip.value();
        }

        // 分配新的 IP
        let ip = self.allocate_ipv4();
        self.domain_to_ip.insert(domain.to_string(), ip);
        self.ip_to_domain.insert(ip, domain.to_string());

        tracing::debug!(domain = %domain, ip = %ip, "Allocated FakeIP");
        ip
    }

    /// 反向查询：从 FakeIP 查找域名
    pub fn reverse_lookup(&self, ip: &IpAddr) -> Option<String> {
        self.ip_to_domain.get(ip).map(|entry| entry.value().clone())
    }

    /// 检查是否为 FakeIP
    pub fn is_fake_ip(&self, ip: &IpAddr) -> bool {
        match ip {
            IpAddr::V4(v4) => self.inet4_range.contains(v4),
            IpAddr::V6(v6) => self.inet6_range.contains(v6),
        }
    }

    /// 分配 IPv4
    fn allocate_ipv4(&self) -> IpAddr {
        let offset = self.inet4_offset.fetch_add(1, Ordering::Relaxed);
        let base = u32::from(self.inet4_range.network());
        let max = u32::from(self.inet4_range.broadcast());

        // 循环使用地址池
        let ip_u32 = base + (offset % (max - base));
        IpAddr::V4(Ipv4Addr::from(ip_u32))
    }

    /// 分配 IPv6
    #[allow(dead_code)]
    fn allocate_ipv6(&self) -> IpAddr {
        let offset = self.inet6_offset.fetch_add(1, Ordering::Relaxed);
        let base = u128::from(self.inet6_range.network());
        let ip_u128 = base + offset as u128;
        IpAddr::V6(Ipv6Addr::from(ip_u128))
    }

    /// 清除过期映射
    pub fn cleanup(&self, max_entries: usize) {
        if self.domain_to_ip.len() > max_entries {
            // 简单的清理策略：清除一半
            let to_remove = self.domain_to_ip.len() / 2;
            let mut removed = 0;

            self.domain_to_ip.retain(|domain, ip| {
                if removed >= to_remove {
                    return true;
                }
                self.ip_to_domain.remove(ip);
                removed += 1;
                tracing::trace!(domain = %domain, ip = %ip, "Removed FakeIP mapping");
                false
            });
        }
    }

    /// 获取统计信息
    pub fn stats(&self) -> FakeIpStats {
        FakeIpStats {
            total_mappings: self.domain_to_ip.len(),
            ipv4_offset: self.inet4_offset.load(Ordering::Relaxed),
            ipv6_offset: self.inet6_offset.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FakeIpStats {
    pub total_mappings: usize,
    pub ipv4_offset: u32,
    pub ipv6_offset: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fakeip_allocation() {
        let pool = FakeIpPool::new("198.18.0.0/15", "fc00::/18").unwrap();

        let ip1 = pool.lookup("www.google.com");
        let ip2 = pool.lookup("www.github.com");
        let ip3 = pool.lookup("www.google.com"); // 应该返回相同 IP

        assert_ne!(ip1, ip2);
        assert_eq!(ip1, ip3);

        assert_eq!(pool.reverse_lookup(&ip1), Some("www.google.com".to_string()));
        assert!(pool.is_fake_ip(&ip1));
    }

    #[test]
    fn test_fakeip_cleanup() {
        let pool = FakeIpPool::new("198.18.0.0/15", "fc00::/18").unwrap();

        for i in 0..100 {
            pool.lookup(&format!("test{}.com", i));
        }

        assert_eq!(pool.stats().total_mappings, 100);

        pool.cleanup(50);
        assert!(pool.stats().total_mappings <= 50);
    }
}
