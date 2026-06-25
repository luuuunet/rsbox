// DNS 规则系统实现
use anyhow::Result;
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct DnsRule {
    // 匹配条件
    pub domain: Option<Vec<String>>,
    pub domain_suffix: Option<Vec<String>>,
    pub domain_keyword: Option<Vec<String>>,
    pub domain_regex: Option<Vec<regex::Regex>>,
    pub geosite: Option<Vec<String>>,
    pub source_ip_cidr: Option<Vec<ipnet::IpNet>>,
    pub query_type: Option<Vec<String>>,
    pub outbound: Option<Vec<String>>,
    pub clash_mode: Option<String>,
    pub invert: bool,

    // 目标配置
    pub server: String,
    pub disable_cache: bool,
    pub client_subnet: Option<String>,
}

impl DnsRule {
    pub fn match_domain(&self, domain: &str) -> bool {
        // 如果没有任何匹配条件，跳过
        if self.domain.is_none()
            && self.domain_suffix.is_none()
            && self.domain_keyword.is_none()
            && self.domain_regex.is_none()
            && self.geosite.is_none()
        {
            return false;
        }

        let mut matched = false;

        // 精确匹配
        if let Some(ref domains) = self.domain {
            if domains.iter().any(|d| d == domain) {
                matched = true;
            }
        }

        // 后缀匹配
        if !matched {
            if let Some(ref suffixes) = self.domain_suffix {
                if suffixes.iter().any(|suffix| {
                    domain == suffix || domain.ends_with(&format!(".{}", suffix))
                }) {
                    matched = true;
                }
            }
        }

        // 关键字匹配
        if !matched {
            if let Some(ref keywords) = self.domain_keyword {
                if keywords.iter().any(|keyword| domain.contains(keyword)) {
                    matched = true;
                }
            }
        }

        // 正则匹配
        if !matched {
            if let Some(ref regexes) = self.domain_regex {
                if regexes.iter().any(|re| re.is_match(domain)) {
                    matched = true;
                }
            }
        }

        // GeoSite 匹配
        if !matched {
            if let Some(ref geosites) = self.geosite {
                // TODO: 实现 GeoSite 查询
                matched = false;
            }
        }

        // 应用 invert
        if self.invert {
            !matched
        } else {
            matched
        }
    }

    pub fn match_source_ip(&self, ip: &IpAddr) -> bool {
        if let Some(ref cidrs) = self.source_ip_cidr {
            cidrs.iter().any(|cidr| cidr.contains(ip))
        } else {
            true // 没有来源 IP 限制
        }
    }

    pub fn match_query_type(&self, query_type: &str) -> bool {
        if let Some(ref types) = self.query_type {
            types.iter().any(|t| t == query_type)
        } else {
            true // 没有查询类型限制
        }
    }
}

pub struct DnsRouter {
    rules: Vec<DnsRule>,
    default_server: String,
}

impl DnsRouter {
    pub fn new(rules: Vec<DnsRule>, default_server: String) -> Self {
        Self {
            rules,
            default_server,
        }
    }

    pub fn route(
        &self,
        domain: &str,
        source_ip: Option<&IpAddr>,
        query_type: &str,
    ) -> &str {
        for rule in &self.rules {
            // 检查域名匹配
            if !rule.match_domain(domain) {
                continue;
            }

            // 检查来源 IP
            if let Some(ip) = source_ip {
                if !rule.match_source_ip(ip) {
                    continue;
                }
            }

            // 检查查询类型
            if !rule.match_query_type(query_type) {
                continue;
            }

            tracing::debug!(
                domain = %domain,
                server = %rule.server,
                "DNS rule matched"
            );

            return &rule.server;
        }

        &self.default_server
    }

    pub fn should_cache(&self, domain: &str) -> bool {
        for rule in &self.rules {
            if rule.match_domain(domain) {
                return !rule.disable_cache;
            }
        }
        true
    }

    pub fn get_client_subnet(&self, domain: &str) -> Option<String> {
        for rule in &self.rules {
            if rule.match_domain(domain) {
                return rule.client_subnet.clone();
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dns_rule_matching() {
        let rule = DnsRule {
            domain: Some(vec!["example.com".to_string()]),
            domain_suffix: Some(vec!["google.com".to_string()]),
            domain_keyword: Some(vec!["github".to_string()]),
            domain_regex: None,
            geosite: None,
            source_ip_cidr: None,
            query_type: None,
            outbound: None,
            clash_mode: None,
            invert: false,
            server: "1.1.1.1".to_string(),
            disable_cache: false,
            client_subnet: None,
        };

        assert!(rule.match_domain("example.com"));
        assert!(rule.match_domain("www.google.com"));
        assert!(rule.match_domain("github.com"));
        assert!(!rule.match_domain("baidu.com"));
    }

    #[test]
    fn test_dns_router() {
        let rules = vec![
            DnsRule {
                domain_suffix: Some(vec!["cn".to_string()]),
                server: "223.5.5.5".to_string(),
                ..Default::default()
            },
            DnsRule {
                domain_suffix: Some(vec!["google.com".to_string()]),
                server: "8.8.8.8".to_string(),
                ..Default::default()
            },
        ];

        let router = DnsRouter::new(rules, "1.1.1.1".to_string());

        assert_eq!(router.route("baidu.cn", None, "A"), "223.5.5.5");
        assert_eq!(router.route("www.google.com", None, "A"), "8.8.8.8");
        assert_eq!(router.route("example.com", None, "A"), "1.1.1.1");
    }
}

impl Default for DnsRule {
    fn default() -> Self {
        Self {
            domain: None,
            domain_suffix: None,
            domain_keyword: None,
            domain_regex: None,
            geosite: None,
            source_ip_cidr: None,
            query_type: None,
            outbound: None,
            clash_mode: None,
            invert: false,
            server: String::new(),
            disable_cache: false,
            client_subnet: None,
        }
    }
}
