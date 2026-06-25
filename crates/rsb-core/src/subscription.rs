// 节点订阅实现
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub url: String,
    pub user_agent: Option<String>,
    pub update_interval: Option<u64>,
}

impl Subscription {
    pub fn new(url: String) -> Self {
        Self {
            url,
            user_agent: Some("rsbox/0.1.0".to_string()),
            update_interval: Some(3600),
        }
    }

    /// 获取订阅内容
    pub async fn fetch(&self) -> Result<Vec<Value>> {
        tracing::info!("Fetching subscription from {}", self.url);

        let client = reqwest::Client::builder()
            .user_agent(self.user_agent.as_deref().unwrap_or("rsbox/0.1.0"))
            .timeout(Duration::from_secs(30))
            .build()?;

        let response = client
            .get(&self.url)
            .send()
            .await
            .context("Failed to fetch subscription")?;

        let content = response.text().await?;

        // 尝试解析
        self.parse_content(&content)
    }

    fn parse_content(&self, content: &str) -> Result<Vec<Value>> {
        let content = content.trim();

        // 如果是 Base64 编码
        if !content.starts_with("vmess://")
            && !content.starts_with("vless://")
            && !content.starts_with("ss://")
            && !content.starts_with("trojan://")
            && !content.starts_with("hysteria2://")
        {
            // 尝试 Base64 解码
            if let Ok(decoded) = general_purpose::STANDARD.decode(content) {
                if let Ok(decoded_str) = String::from_utf8(decoded) {
                    return self.parse_share_links(&decoded_str);
                }
            }
        }

        // 直接解析分享链接
        self.parse_share_links(content)
    }

    fn parse_share_links(&self, content: &str) -> Result<Vec<Value>> {
        let mut outbounds = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Ok(outbound) = self.parse_single_link(line) {
                outbounds.push(outbound);
            }
        }

        Ok(outbounds)
    }

    fn parse_single_link(&self, link: &str) -> Result<Value> {
        if link.starts_with("vmess://") {
            self.parse_vmess(link)
        } else if link.starts_with("vless://") {
            self.parse_vless(link)
        } else if link.starts_with("ss://") {
            self.parse_shadowsocks(link)
        } else if link.starts_with("trojan://") {
            self.parse_trojan(link)
        } else if link.starts_with("hysteria2://") || link.starts_with("hy2://") {
            self.parse_hysteria2(link)
        } else {
            anyhow::bail!("Unknown link format: {}", link)
        }
    }

    fn parse_vmess(&self, link: &str) -> Result<Value> {
        let encoded = link.strip_prefix("vmess://").context("Invalid vmess link")?;
        let decoded = general_purpose::STANDARD.decode(encoded)?;
        let json_str = String::from_utf8(decoded)?;
        let config: HashMap<String, Value> = serde_json::from_str(&json_str)?;

        Ok(serde_json::json!({
            "type": "vmess",
            "tag": config.get("ps").and_then(|v| v.as_str()).unwrap_or("vmess"),
            "server": config.get("add").and_then(|v| v.as_str()).unwrap_or(""),
            "server_port": config.get("port").and_then(|v| v.as_u64()).unwrap_or(443),
            "uuid": config.get("id").and_then(|v| v.as_str()).unwrap_or(""),
            "security": config.get("scy").and_then(|v| v.as_str()).unwrap_or("auto"),
            "alter_id": config.get("aid").and_then(|v| v.as_u64()).unwrap_or(0),
            "tls": {
                "enabled": config.get("tls").and_then(|v| v.as_str()) == Some("tls"),
                "server_name": config.get("sni").and_then(|v| v.as_str()).unwrap_or(""),
            }
        }))
    }

    fn parse_vless(&self, link: &str) -> Result<Value> {
        let url = url::Url::parse(link)?;

        let uuid = url.username();
        let host = url.host_str().context("No host")?;
        let port = url.port().unwrap_or(443);

        let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

        Ok(serde_json::json!({
            "type": "vless",
            "tag": url.fragment().unwrap_or("vless"),
            "server": host,
            "server_port": port,
            "uuid": uuid,
            "flow": params.get("flow").cloned().unwrap_or_default(),
            "tls": {
                "enabled": params.get("security").map(|s| s == "tls").unwrap_or(false),
                "server_name": params.get("sni").cloned().unwrap_or_default(),
            }
        }))
    }

    fn parse_shadowsocks(&self, link: &str) -> Result<Value> {
        let url = url::Url::parse(link)?;

        // ss://method:password@server:port
        let userinfo = url.username();
        let decoded = general_purpose::STANDARD.decode(userinfo)?;
        let decoded_str = String::from_utf8(decoded)?;

        let parts: Vec<&str> = decoded_str.split(':').collect();
        if parts.len() < 2 {
            anyhow::bail!("Invalid shadowsocks format");
        }

        let method = parts[0];
        let password = parts[1..].join(":");

        Ok(serde_json::json!({
            "type": "shadowsocks",
            "tag": url.fragment().unwrap_or("ss"),
            "server": url.host_str().unwrap_or(""),
            "server_port": url.port().unwrap_or(443),
            "method": method,
            "password": password,
        }))
    }

    fn parse_trojan(&self, link: &str) -> Result<Value> {
        let url = url::Url::parse(link)?;

        let password = url.username();
        let host = url.host_str().context("No host")?;
        let port = url.port().unwrap_or(443);

        let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

        Ok(serde_json::json!({
            "type": "trojan",
            "tag": url.fragment().unwrap_or("trojan"),
            "server": host,
            "server_port": port,
            "password": password,
            "tls": {
                "enabled": true,
                "server_name": params.get("sni").cloned().unwrap_or_else(|| host.to_string()),
                "insecure": params.get("allowInsecure").map(|s| s == "1").unwrap_or(false),
            }
        }))
    }

    fn parse_hysteria2(&self, link: &str) -> Result<Value> {
        let link = if link.starts_with("hy2://") {
            link.replace("hy2://", "hysteria2://")
        } else {
            link.to_string()
        };

        let url = url::Url::parse(&link)?;

        let host = url.host_str().context("No host")?;
        let port = url.port().unwrap_or(443);
        let password = url.username();

        let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

        Ok(serde_json::json!({
            "type": "hysteria2",
            "tag": url.fragment().unwrap_or("hy2"),
            "server": host,
            "server_port": port,
            "password": password,
            "up_mbps": params.get("up").and_then(|s| s.parse::<u32>().ok()).unwrap_or(100),
            "down_mbps": params.get("down").and_then(|s| s.parse::<u32>().ok()).unwrap_or(100),
            "obfs": params.get("obfs").map(|obfs_type| {
                serde_json::json!({
                    "type": obfs_type,
                    "password": params.get("obfs-password").cloned().unwrap_or_default(),
                })
            }),
            "tls": {
                "enabled": true,
                "server_name": params.get("sni").cloned().unwrap_or_else(|| host.to_string()),
                "insecure": params.get("insecure").map(|s| s == "1").unwrap_or(false),
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vmess() {
        let sub = Subscription::new("".to_string());
        let link = "vmess://eyJhZGQiOiIxMjcuMC4wLjEiLCJhaWQiOjAsImhvc3QiOiIiLCJpZCI6IjEyMzQiLCJuZXQiOiJ0Y3AiLCJwYXRoIjoiIiwicG9ydCI6NDQzLCJwcyI6InRlc3QiLCJzY3kiOiJhdXRvIiwic25pIjoiIiwidGxzIjoidGxzIiwidHlwZSI6Im5vbmUiLCJ2IjoyfQ==";
        let result = sub.parse_vmess(link);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_vless() {
        let sub = Subscription::new("".to_string());
        let link = "vless://uuid@example.com:443?security=tls&sni=example.com#test";
        let result = sub.parse_vless(link);
        assert!(result.is_ok());
    }
}
