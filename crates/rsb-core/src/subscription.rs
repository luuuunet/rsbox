// 节点订阅实现
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
        self.fetch_from_base(None).await
    }

    pub async fn fetch_from_base(&self, config_dir: Option<&Path>) -> Result<Vec<Value>> {
        if self.url.starts_with("file://") {
            let path = resolve_file_path(&self.url, config_dir)?;
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("read subscription file: {}", path.display()))?;
            return self.parse_content(&content);
        }

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
            && !content.starts_with("rsq://")
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

            match self.parse_single_link(line) {
                Ok(outbound) => outbounds.push(outbound),
                Err(err) => tracing::warn!(line = %line, error = %err, "subscription: skip invalid link"),
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
        } else if link.starts_with("rsq://") {
            self.parse_rsq(link)
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

    fn parse_rsq(&self, link: &str) -> Result<Value> {
        let url = url::Url::parse(link)?;
        let host = url.host_str().context("No host")?;
        let port = url.port().unwrap_or(443);
        let password = decode_url_userinfo(url.username());
        let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

        Ok(serde_json::json!({
            "type": "rsq",
            "tag": url.fragment().unwrap_or("rsq"),
            "server": host,
            "server_port": port,
            "password": password,
            "up_mbps": params.get("up").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
            "down_mbps": params.get("down").and_then(|s| s.parse::<u32>().ok()).unwrap_or(0),
            "traffic_profile": params.get("profile").cloned().unwrap_or_else(|| "video".to_string()),
            "obfs": params.get("obfs").map(|obfs_type| {
                let version = params
                    .get("obfs-version")
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(1);
                serde_json::json!({
                    "enabled": true,
                    "type": obfs_type,
                    "password": params.get("obfs-password").cloned().unwrap_or_default(),
                    "version": version,
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

fn decode_url_userinfo(input: &str) -> String {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16)
            {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Fetch `outbound_providers` and append parsed nodes to `options.outbounds`.
pub async fn merge_outbound_providers(
    options: &mut rsb_config::Options,
    config_dir: Option<&Path>,
) -> anyhow::Result<usize> {
    if options.outbound_providers.is_empty() {
        return Ok(0);
    }
    let mut added = 0usize;
    let mut existing_tags: std::collections::HashSet<String> = options
        .outbounds
        .iter()
        .filter_map(|ob| ob.tag.clone())
        .collect();
    for provider in options.outbound_providers.clone() {
        if provider.kind != "subscription" {
            tracing::warn!(kind = %provider.kind, "unsupported outbound provider type");
            continue;
        }
        let mut sub = Subscription::new(provider.url);
        if let Some(ua) = provider.user_agent {
            sub.user_agent = Some(ua);
        }
        let nodes = sub.fetch_from_base(config_dir).await?;
        let mut node_tags = Vec::new();
        for node in nodes {
            match value_to_outbound(node) {
                Ok(ob) => {
                    if let Some(ref tag) = ob.tag {
                        if !existing_tags.insert(tag.clone()) {
                            tracing::warn!(tag = %tag, "skip duplicate subscription outbound tag");
                            continue;
                        }
                        node_tags.push(tag.clone());
                    }
                    added += 1;
                    options.outbounds.push(ob);
                }
                Err(err) => tracing::warn!(error = %err, "skip subscription node"),
            }
        }
        let sel_tag = provider.selector_tag.or(provider.tag);
        if let Some(sel_tag) = sel_tag {
            if !node_tags.is_empty() {
                if existing_tags.insert(sel_tag.clone()) {
                    options.outbounds.push(rsb_config::Outbound {
                        kind: "selector".to_string(),
                        tag: Some(sel_tag),
                        raw: serde_json::json!({ "outbounds": node_tags }),
                    });
                } else {
                    tracing::warn!(tag = %sel_tag, "skip duplicate selector tag from subscription");
                }
            }
        }
    }
    Ok(added)
}

fn resolve_file_path(url: &str, config_dir: Option<&Path>) -> Result<PathBuf> {
    let raw = url.strip_prefix("file://").context("invalid file url")?;
    let raw = raw.trim_start_matches('/');
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        return Ok(path);
    }
    let mut candidates = Vec::new();
    if let Some(dir) = config_dir {
        candidates.push(dir.join(&path));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(&path));
    }
    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }
    candidates
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve subscription file path: {url}"))
}

fn value_to_outbound(mut value: Value) -> anyhow::Result<rsb_config::Outbound> {
    let obj = value
        .as_object_mut()
        .context("subscription node must be a JSON object")?;
    let kind = obj
        .remove("type")
        .and_then(|v| v.as_str().map(str::to_string))
        .context("subscription node missing type")?;
    let tag = obj.remove("tag").and_then(|v| v.as_str().map(str::to_string));
    Ok(rsb_config::Outbound {
        kind,
        tag,
        raw: Value::Object(std::mem::take(obj)),
    })
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

    #[test]
    fn test_parse_rsq() {
        let sub = Subscription::new("".to_string());
        let link = "rsq://secret@example.com:8443?up=50&down=100&sni=example.com&profile=video#node1";
        let result = sub.parse_rsq(link).unwrap();
        assert_eq!(result["type"], "rsq");
        assert_eq!(result["password"], "secret");
        assert_eq!(result["server_port"], 8443);
    }

    #[test]
    fn test_value_to_outbound() {
        let node = serde_json::json!({
            "type": "rsq",
            "tag": "node1",
            "server": "1.2.3.4",
            "server_port": 8443,
            "password": "secret"
        });
        let ob = super::value_to_outbound(node).unwrap();
        assert_eq!(ob.kind, "rsq");
        assert_eq!(ob.tag.as_deref(), Some("node1"));
    }

    #[test]
    fn test_parse_rsq_obfs_version() {
        let sub = Subscription::new("".to_string());
        let link = "rsq://secret@example.com:8443?obfs=salamander&obfs-password=pw&obfs-version=2#v2node";
        let result = sub.parse_rsq(link).unwrap();
        assert_eq!(result["obfs"]["version"], 2);
    }

    #[test]
    fn test_resolve_file_path_relative() {
        let base = Path::new("D:/proj/examples");
        let p = super::resolve_file_path("file://subscriptions/rsq-local.txt", Some(base)).unwrap();
        assert!(p.ends_with("subscriptions/rsq-local.txt") || p.ends_with("subscriptions\\rsq-local.txt"));
    }

    #[test]
    fn test_decode_url_userinfo() {
        assert_eq!(super::decode_url_userinfo("plain"), "plain");
        assert_eq!(super::decode_url_userinfo("p%40ss"), "p@ss");
    }

    #[test]
    fn test_parse_rsq_encoded_password() {
        let sub = Subscription::new("".to_string());
        let node = sub
            .parse_rsq("rsq://p%40ss@127.0.0.1:443?sni=x&insecure=1#n1")
            .unwrap();
        assert_eq!(node["password"], "p@ss");
    }

    #[test]
    fn test_merge_subscription_selector() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/subscriptions/rsq-local.txt");
        let content = std::fs::read_to_string(&path).expect("read rsq-local.txt");
        let sub = Subscription::new("".to_string());
        let nodes = sub.parse_share_links(&content).unwrap();
        let mut options = rsb_config::Options::from_json(
            r#"{"inbounds":[],"outbounds":[{"type":"direct","tag":"direct"}],"outbound_providers":[{"type":"subscription","url":"file://subscriptions/rsq-local.txt","selector_tag":"rsq-sub"}],"route":{"final":"rsq-sub"}}"#,
        )
        .unwrap();
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let added = rt
            .block_on(super::merge_outbound_providers(&mut options, Some(&base)))
            .unwrap();
        assert_eq!(added, nodes.len());
        assert!(options.outbounds.iter().any(|o| o.kind == "selector" && o.tag.as_deref() == Some("rsq-sub")));
    }

    #[test]
    fn test_parse_local_subscription_file() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples/subscriptions/rsq-local.txt");
        let content = std::fs::read_to_string(&path).expect("read rsq-local.txt");
        let sub = Subscription::new("".to_string());
        let nodes = sub.parse_share_links(&content).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0]["type"], "rsq");
    }
}
