/// DNS Anti-Pollution Module
///
/// Protects against DNS hijacking and pollution, especially useful in China.

use anyhow::Result;
use std::net::IpAddr;
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Configuration for anti-pollution feature
#[derive(Debug, Clone)]
pub struct AntiPollutionConfig {
    /// Enable anti-pollution checks
    pub enabled: bool,
    /// List of trusted DNS server tags
    pub trusted_servers: Vec<String>,
    /// Check method to use
    pub check_method: CheckMethod,
    /// Override TTL for polluted results
    pub ttl_override: Option<u32>,
    /// Known poison IP ranges
    pub poison_ips: Vec<String>,
}

/// DNS query validation method
#[derive(Debug, Clone, PartialEq)]
pub enum CheckMethod {
    /// Query both local and trusted, compare results
    DualQuery,
    /// Only use trusted DNS servers
    TrustedOnly,
    /// Try local first, fallback to trusted on failure
    Fallback,
}

/// Anti-pollution DNS resolver
pub struct AntiPollution {
    config: AntiPollutionConfig,
    poison_ips: HashSet<IpAddr>,
    cache: std::sync::Arc<tokio::sync::RwLock<Cache>>,
}

struct Cache {
    entries: std::collections::HashMap<String, CacheEntry>,
}

struct CacheEntry {
    addrs: Vec<IpAddr>,
    expire_at: Instant,
}

impl AntiPollution {
    pub fn new(config: AntiPollutionConfig) -> Self {
        let mut poison_ips = HashSet::new();

        // Add configured poison IPs
        for ip_str in &config.poison_ips {
            if let Ok(ip) = ip_str.parse() {
                poison_ips.insert(ip);
            }
        }

        Self {
            config,
            poison_ips,
            cache: std::sync::Arc::new(tokio::sync::RwLock::new(Cache {
                entries: std::collections::HashMap::new(),
            })),
        }
    }

    /// Resolve domain with anti-pollution checks
    pub async fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>> {
        // Check cache first
        if let Some(cached) = self.get_cached(domain).await {
            return Ok(cached);
        }

        let result = match self.config.check_method {
            CheckMethod::DualQuery => self.dual_query(domain).await,
            CheckMethod::TrustedOnly => self.query_trusted(domain).await,
            CheckMethod::Fallback => self.fallback_query(domain).await,
        };

        // Cache the result
        if let Ok(ref addrs) = result {
            self.cache_result(domain, addrs.clone()).await;
        }

        result
    }

    async fn dual_query(&self, domain: &str) -> Result<Vec<IpAddr>> {
        // Query both local and trusted DNS servers
        let (local_result, trusted_result) = tokio::join!(
            self.query_local(domain),
            self.query_trusted(domain)
        );

        // Validate results
        if let Ok(local_addrs) = local_result {
            if !self.is_polluted(&local_addrs) {
                return Ok(local_addrs);
            }
            log::warn!("DNS pollution detected for {}, using trusted DNS", domain);
        }

        trusted_result
    }

    async fn fallback_query(&self, domain: &str) -> Result<Vec<IpAddr>> {
        match self.query_local(domain).await {
            Ok(addrs) if !self.is_polluted(&addrs) => Ok(addrs),
            _ => {
                log::debug!("Local DNS failed or polluted for {}, using trusted DNS", domain);
                self.query_trusted(domain).await
            }
        }
    }

    async fn query_local(&self, _domain: &str) -> Result<Vec<IpAddr>> {
        // TODO: Implement local DNS query
        // This should query the system's default DNS
        anyhow::bail!("Not implemented")
    }

    async fn query_trusted(&self, _domain: &str) -> Result<Vec<IpAddr>> {
        // TODO: Implement trusted DNS query
        // This should query DoH/DoT servers
        anyhow::bail!("Not implemented")
    }

    /// Check if DNS result is polluted
    fn is_polluted(&self, addrs: &[IpAddr]) -> bool {
        for addr in addrs {
            if self.is_poison_ip(addr) {
                return true;
            }
        }
        false
    }

    /// Check if IP is a known poison IP
    fn is_poison_ip(&self, addr: &IpAddr) -> bool {
        // Check configured poison IPs
        if self.poison_ips.contains(addr) {
            return true;
        }

        // Check common poison IP patterns
        match addr {
            IpAddr::V4(ip) => {
                let octets = ip.octets();
                // Private/reserved ranges often used in poisoning
                matches!(octets[0], 0 | 10 | 127)
                    || (octets[0] == 169 && octets[1] == 254)  // Link-local
                    || (octets[0] == 203 && octets[1] == 98)   // Known poison
                    || (octets[0] == 159 && octets[1] == 226)  // Known poison
            }
            _ => false,
        }
    }

    async fn get_cached(&self, domain: &str) -> Option<Vec<IpAddr>> {
        let cache = self.cache.read().await;
        if let Some(entry) = cache.entries.get(domain) {
            if entry.expire_at > Instant::now() {
                return Some(entry.addrs.clone());
            }
        }
        None
    }

    async fn cache_result(&self, domain: &str, addrs: Vec<IpAddr>) {
        let mut cache = self.cache.write().await;
        let ttl = self.config.ttl_override.unwrap_or(300);
        cache.entries.insert(
            domain.to_string(),
            CacheEntry {
                addrs,
                expire_at: Instant::now() + Duration::from_secs(ttl as u64),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_poison_ip_detection() {
        let config = AntiPollutionConfig {
            enabled: true,
            trusted_servers: vec![],
            check_method: CheckMethod::DualQuery,
            ttl_override: None,
            poison_ips: vec!["203.98.7.65".to_string()],
        };

        let ap = AntiPollution::new(config);

        // Should detect poison IPs
        assert!(ap.is_poison_ip(&"127.0.0.1".parse().unwrap()));
        assert!(ap.is_poison_ip(&"10.0.0.1".parse().unwrap()));
        assert!(ap.is_poison_ip(&"203.98.7.65".parse().unwrap()));

        // Should not flag valid IPs
        assert!(!ap.is_poison_ip(&"1.1.1.1".parse().unwrap()));
        assert!(!ap.is_poison_ip(&"8.8.8.8".parse().unwrap()));
    }
}
