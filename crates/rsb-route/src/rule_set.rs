// Rule Set 规则集实现
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSet {
    pub tag: String,
    #[serde(rename = "type")]
    pub type_: RuleSetType,
    pub format: RuleSetFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_detour: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_interval: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSetType {
    Local,
    Remote,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleSetFormat {
    Source,
    Binary,
}

#[derive(Debug, Clone)]
pub struct RuleSetMatcher {
    domains: Vec<String>,
    domain_suffixes: Vec<String>,
    domain_keywords: Vec<String>,
    ip_cidrs: Vec<ipnet::IpNet>,
}

impl RuleSet {
    pub async fn load(&self) -> Result<RuleSetMatcher> {
        match self.type_ {
            RuleSetType::Local => self.load_local().await,
            RuleSetType::Remote => self.load_remote().await,
        }
    }

    async fn load_local(&self) -> Result<RuleSetMatcher> {
        let path = self.path.as_ref().context("Local rule set requires path")?;

        tracing::info!(tag = %self.tag, path = ?path, "Loading local rule set");

        let content = tokio::fs::read_to_string(path).await?;
        self.parse_content(&content)
    }

    async fn load_remote(&self) -> Result<RuleSetMatcher> {
        let url = self.url.as_ref().context("Remote rule set requires URL")?;

        tracing::info!(tag = %self.tag, url = %url, "Downloading remote rule set");

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        let response = client.get(url).send().await?;
        let content = response.text().await?;

        self.parse_content(&content)
    }

    fn parse_content(&self, content: &str) -> Result<RuleSetMatcher> {
        match self.format {
            RuleSetFormat::Source => self.parse_source_format(content),
            RuleSetFormat::Binary => self.parse_binary_format(content.as_bytes()),
        }
    }

    fn parse_source_format(&self, content: &str) -> Result<RuleSetMatcher> {
        let data: SourceRuleSet = serde_json::from_str(content)?;

        let mut domains = Vec::new();
        let mut domain_suffixes = Vec::new();
        let mut domain_keywords = Vec::new();
        let mut ip_cidrs = Vec::new();

        for rule in data.rules {
            match rule {
                SourceRule::Domain(d) => domains.push(d),
                SourceRule::DomainSuffix(s) => domain_suffixes.push(s),
                SourceRule::DomainKeyword(k) => domain_keywords.push(k),
                SourceRule::IpCidr(cidr) => {
                    if let Ok(net) = cidr.parse() {
                        ip_cidrs.push(net);
                    }
                }
            }
        }

        Ok(RuleSetMatcher {
            domains,
            domain_suffixes,
            domain_keywords,
            ip_cidrs,
        })
    }

    fn parse_binary_format(&self, _data: &[u8]) -> Result<RuleSetMatcher> {
        // TODO: 实现二进制格式解析
        anyhow::bail!("Binary format not yet implemented")
    }
}

impl RuleSetMatcher {
    pub fn match_domain(&self, domain: &str) -> bool {
        // 精确匹配
        if self.domains.iter().any(|d| d == domain) {
            return true;
        }

        // 后缀匹配
        if self.domain_suffixes.iter().any(|suffix| {
            domain == suffix || domain.ends_with(&format!(".{}", suffix))
        }) {
            return true;
        }

        // 关键字匹配
        if self.domain_keywords.iter().any(|keyword| domain.contains(keyword)) {
            return true;
        }

        false
    }

    pub fn match_ip(&self, ip: &IpAddr) -> bool {
        self.ip_cidrs.iter().any(|net| net.contains(ip))
    }
}

#[derive(Debug, Deserialize)]
struct SourceRuleSet {
    version: u32,
    rules: Vec<SourceRule>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SourceRule {
    Domain(String),
    DomainSuffix(String),
    DomainKeyword(String),
    IpCidr(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_set_matcher() {
        let matcher = RuleSetMatcher {
            domains: vec!["example.com".to_string()],
            domain_suffixes: vec!["google.com".to_string()],
            domain_keywords: vec!["github".to_string()],
            ip_cidrs: vec!["1.1.1.0/24".parse().unwrap()],
        };

        assert!(matcher.match_domain("example.com"));
        assert!(matcher.match_domain("www.google.com"));
        assert!(matcher.match_domain("github.com"));
        assert!(matcher.match_ip(&"1.1.1.1".parse().unwrap()));
    }
}
