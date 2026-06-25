// 分流订阅实现
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSetSubscription {
    pub url: String,
    pub user_agent: Option<String>,
    pub update_interval: Option<u64>, // seconds
    pub download_detour: Option<String>,
}

impl RuleSetSubscription {
    pub fn new(url: String) -> Self {
        Self {
            url,
            user_agent: Some("rsbox/0.1.0".to_string()),
            update_interval: Some(86400), // 24 hours
            download_detour: None,
        }
    }

    /// 获取规则集内容
    pub async fn fetch(&self) -> Result<RuleSetContent> {
        tracing::info!(url = %self.url, "Fetching rule set subscription");

        let client = Client::builder()
            .user_agent(self.user_agent.as_deref().unwrap_or("rsbox/0.1.0"))
            .timeout(Duration::from_secs(30))
            .build()?;

        let response = client
            .get(&self.url)
            .send()
            .await
            .context("Failed to fetch rule set")?;

        let content = response.text().await?;

        self.parse_content(&content)
    }

    fn parse_content(&self, content: &str) -> Result<RuleSetContent> {
        // 尝试解析为 JSON
        if let Ok(json_content) = serde_json::from_str::<JsonRuleSet>(content) {
            return Ok(RuleSetContent::from_json(json_content));
        }

        // 尝试解析为纯文本规则
        Ok(RuleSetContent::from_text(content))
    }
}

#[derive(Debug, Clone)]
pub struct RuleSetContent {
    pub domains: Vec<String>,
    pub domain_suffixes: Vec<String>,
    pub domain_keywords: Vec<String>,
    pub ip_cidrs: Vec<String>,
}

impl RuleSetContent {
    fn from_json(json: JsonRuleSet) -> Self {
        let mut content = Self {
            domains: Vec::new(),
            domain_suffixes: Vec::new(),
            domain_keywords: Vec::new(),
            ip_cidrs: Vec::new(),
        };

        for rule in json.rules {
            match rule.rule_type.as_str() {
                "domain" => content.domains.push(rule.value),
                "domain_suffix" => content.domain_suffixes.push(rule.value),
                "domain_keyword" => content.domain_keywords.push(rule.value),
                "ip_cidr" => content.ip_cidrs.push(rule.value),
                _ => {}
            }
        }

        content
    }

    fn from_text(text: &str) -> Self {
        let mut content = Self {
            domains: Vec::new(),
            domain_suffixes: Vec::new(),
            domain_keywords: Vec::new(),
            ip_cidrs: Vec::new(),
        };

        for line in text.lines() {
            let line = line.trim();

            // 跳过注释和空行
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }

            // 解析不同格式
            if line.starts_with("DOMAIN,") {
                if let Some(domain) = line.strip_prefix("DOMAIN,") {
                    content.domains.push(domain.to_string());
                }
            } else if line.starts_with("DOMAIN-SUFFIX,") {
                if let Some(suffix) = line.strip_prefix("DOMAIN-SUFFIX,") {
                    content.domain_suffixes.push(suffix.to_string());
                }
            } else if line.starts_with("DOMAIN-KEYWORD,") {
                if let Some(keyword) = line.strip_prefix("DOMAIN-KEYWORD,") {
                    content.domain_keywords.push(keyword.to_string());
                }
            } else if line.starts_with("IP-CIDR,") || line.starts_with("IP-CIDR6,") {
                let parts: Vec<&str> = line.split(',').collect();
                if parts.len() >= 2 {
                    content.ip_cidrs.push(parts[1].to_string());
                }
            } else {
                // 默认当作域名
                content.domains.push(line.to_string());
            }
        }

        content
    }
}

#[derive(Debug, Deserialize)]
struct JsonRuleSet {
    version: Option<u32>,
    rules: Vec<JsonRule>,
}

#[derive(Debug, Deserialize)]
struct JsonRule {
    #[serde(rename = "type")]
    rule_type: String,
    value: String,
}

/// 规则订阅管理器
pub struct RuleSubscriptionManager {
    subscriptions: Vec<RuleSetSubscription>,
}

impl RuleSubscriptionManager {
    pub fn new(subscriptions: Vec<RuleSetSubscription>) -> Self {
        Self { subscriptions }
    }

    /// 更新所有订阅
    pub async fn update_all(&self) -> Result<Vec<(String, RuleSetContent)>> {
        let mut results = Vec::new();

        for sub in &self.subscriptions {
            match sub.fetch().await {
                Ok(content) => {
                    tracing::info!(url = %sub.url, "Successfully fetched rule set");
                    results.push((sub.url.clone(), content));
                }
                Err(e) => {
                    tracing::error!(url = %sub.url, error = %e, "Failed to fetch rule set");
                }
            }
        }

        Ok(results)
    }

    /// 启动自动更新任务
    pub async fn start_auto_update(self: std::sync::Arc<Self>) {
        tokio::spawn(async move {
            loop {
                // 找到最小的更新间隔
                let min_interval = self
                    .subscriptions
                    .iter()
                    .filter_map(|s| s.update_interval)
                    .min()
                    .unwrap_or(3600);

                tokio::time::sleep(Duration::from_secs(min_interval)).await;

                tracing::info!("Starting automatic rule set update");

                match self.update_all().await {
                    Ok(results) => {
                        tracing::info!(count = results.len(), "Rule sets updated");
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to update rule sets");
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_text_rules() {
        let content = r#"
# 这是注释
DOMAIN,example.com
DOMAIN-SUFFIX,google.com
DOMAIN-KEYWORD,github
IP-CIDR,1.1.1.0/24
        "#;

        let rules = RuleSetContent::from_text(content);
        assert_eq!(rules.domains.len(), 1);
        assert_eq!(rules.domain_suffixes.len(), 1);
        assert_eq!(rules.domain_keywords.len(), 1);
        assert_eq!(rules.ip_cidrs.len(), 1);
    }

    #[tokio::test]
    #[ignore] // 需要网络
    async fn test_fetch_subscription() {
        let sub = RuleSetSubscription::new(
            "https://raw.githubusercontent.com/Loyalsoldier/surge-rules/release/ruleset/direct.txt"
                .to_string(),
        );

        let content = sub.fetch().await.unwrap();
        assert!(!content.domains.is_empty() || !content.domain_suffixes.is_empty());
    }
}
