// Source IP/Port 路由规则实现
use anyhow::Result;
use std::net::{IpAddr, SocketAddr};

#[derive(Debug, Clone)]
pub struct SourceRule {
    pub source_ip_cidr: Vec<ipnet::IpNet>,
    pub source_port: Vec<u16>,
    pub source_port_range: Vec<(u16, u16)>,
    pub outbound: String,
}

impl SourceRule {
    pub fn new(outbound: String) -> Self {
        Self {
            source_ip_cidr: Vec::new(),
            source_port: Vec::new(),
            source_port_range: Vec::new(),
            outbound,
        }
    }

    pub fn match_source(&self, addr: &SocketAddr) -> bool {
        // 检查 IP
        if !self.source_ip_cidr.is_empty() {
            let matched_ip = self.source_ip_cidr.iter().any(|net| net.contains(&addr.ip()));
            if !matched_ip {
                return false;
            }
        }

        // 检查端口
        let port = addr.port();
        if !self.source_port.is_empty() {
            if !self.source_port.contains(&port) {
                return false;
            }
        }

        // 检查端口范围
        if !self.source_port_range.is_empty() {
            let matched_range = self
                .source_port_range
                .iter()
                .any(|(min, max)| port >= *min && port <= *max);
            if !matched_range {
                return false;
            }
        }

        true
    }
}

// Port Range 支持
#[derive(Debug, Clone)]
pub struct PortRangeRule {
    pub port_ranges: Vec<(u16, u16)>,
    pub outbound: String,
}

impl PortRangeRule {
    pub fn match_port(&self, port: u16) -> bool {
        self.port_ranges
            .iter()
            .any(|(min, max)| port >= *min && port <= *max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_ip_matching() {
        let mut rule = SourceRule::new("proxy".to_string());
        rule.source_ip_cidr
            .push("192.168.1.0/24".parse().unwrap());

        let addr1: SocketAddr = "192.168.1.100:12345".parse().unwrap();
        let addr2: SocketAddr = "10.0.0.1:12345".parse().unwrap();

        assert!(rule.match_source(&addr1));
        assert!(!rule.match_source(&addr2));
    }

    #[test]
    fn test_source_port_matching() {
        let mut rule = SourceRule::new("proxy".to_string());
        rule.source_port.push(8080);
        rule.source_port_range.push((10000, 20000));

        let addr1: SocketAddr = "192.168.1.1:8080".parse().unwrap();
        let addr2: SocketAddr = "192.168.1.1:15000".parse().unwrap();
        let addr3: SocketAddr = "192.168.1.1:9090".parse().unwrap();

        assert!(rule.match_source(&addr1));
        assert!(rule.match_source(&addr2));
        assert!(!rule.match_source(&addr3));
    }

    #[test]
    fn test_port_range() {
        let rule = PortRangeRule {
            port_ranges: vec![(80, 80), (443, 443), (8000, 9000)],
            outbound: "proxy".to_string(),
        };

        assert!(rule.match_port(80));
        assert!(rule.match_port(443));
        assert!(rule.match_port(8500));
        assert!(!rule.match_port(81));
        assert!(!rule.match_port(9001));
    }
}
