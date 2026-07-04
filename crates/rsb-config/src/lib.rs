use anyhow::{bail, Context, Result};
use rsb_constant as c;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

#[cfg(test)]
#[path = "config_tests.rs"]
mod config_tests;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Options {
    #[serde(default)]
    pub log: LogOptions,
    #[serde(default)]
    pub dns: Option<DnsOptions>,
    #[serde(default)]
    pub inbounds: Vec<Inbound>,
    #[serde(default)]
    pub outbounds: Vec<Outbound>,
    #[serde(default)]
    pub route: Option<RouteOptions>,
    #[serde(default)]
    pub endpoints: Vec<Endpoint>,
    #[serde(default)]
    pub services: Vec<Service>,
    #[serde(default)]
    pub experimental: Option<ExperimentalOptions>,
    #[serde(default)]
    pub outbound_providers: Vec<OutboundProvider>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LogOptions {
    #[serde(default)]
    pub disabled: bool,
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default)]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Inbound {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(flatten)]
    pub raw: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Outbound {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(flatten)]
    pub raw: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Endpoint {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(flatten)]
    pub raw: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Service {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(flatten)]
    pub raw: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RouteOptions {
    #[serde(default)]
    pub rules: Vec<RouteRule>,
    #[serde(default)]
    pub rule_set: Vec<RuleSet>,
    #[serde(default)]
    pub final_: Option<String>,
    #[serde(default, rename = "final")]
    pub final_tag: Option<String>,
    #[serde(default)]
    pub auto_detect_interface: bool,
    #[serde(default)]
    pub default_interface: Option<String>,
    #[serde(default)]
    pub rule_set_download: Option<RuleSetDownloadOptions>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RuleSetDownloadOptions {
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RuleSet {
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(flatten)]
    pub raw: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RouteRule {
    #[serde(default)]
    pub inbound: Vec<String>,
    #[serde(default)]
    pub network: Vec<String>,
    #[serde(default)]
    pub port: Vec<serde_json::Value>,
    #[serde(default)]
    pub ip_cidr: Vec<String>,
    #[serde(default)]
    pub domain: Vec<String>,
    #[serde(default)]
    pub domain_suffix: Vec<String>,
    #[serde(default)]
    pub domain_keyword: Vec<String>,
    #[serde(default)]
    pub rule_set: Vec<String>,
    #[serde(default)]
    pub geosite: Vec<String>,
    #[serde(default)]
    pub geoip: Vec<String>,
    #[serde(default)]
    pub source_ip_cidr: Vec<String>,
    #[serde(default)]
    pub protocol: Vec<String>,
    #[serde(default)]
    pub process_name: Vec<String>,
    #[serde(default)]
    pub process_path: Vec<String>,
    #[serde(default)]
    pub process_path_regex: Vec<String>,
    #[serde(default)]
    pub outbound: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DnsOptions {
    #[serde(default)]
    pub servers: Vec<DnsServer>,
    #[serde(default)]
    pub rules: Vec<Value>,
    #[serde(default, rename = "final")]
    pub final_tag: Option<String>,
    #[serde(default)]
    pub strategy: Option<String>,
    #[serde(default)]
    pub fakeip: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DnsServer {
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub detour: Option<String>,
    #[serde(flatten)]
    pub raw: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OutboundProvider {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    /// When set, append a `selector` outbound grouping all nodes from this provider.
    #[serde(default)]
    pub selector_tag: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ExperimentalOptions {
    #[serde(default)]
    pub clash_api: Option<ClashApiOptions>,
    #[serde(default)]
    pub cache_file: Option<Value>,
    #[serde(default)]
    pub v2ray_api: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ClashApiOptions {
    #[serde(default)]
    pub external_controller: Option<String>,
    #[serde(default)]
    pub secret: Option<String>,
}

impl Options {
    pub fn from_json(text: &str) -> Result<Self> {
        let options: Self = serde_json::from_str(text).context("parse sing-box config json")?;
        options.validate()?;
        Ok(options)
    }

    pub fn validate(&self) -> Result<()> {
        if self.inbounds.is_empty() && self.outbounds.is_empty() {
            bail!("config must contain at least one inbound or outbound");
        }
        let mut tags = HashSet::new();
        for ob in &self.outbounds {
            if let Some(tag) = &ob.tag {
                if !tags.insert(format!("out:{tag}")) {
                    bail!("duplicate outbound tag: {tag}");
                }
            }
        }
        for ib in &self.inbounds {
            if let Some(tag) = &ib.tag {
                if !tags.insert(format!("in:{tag}")) {
                    bail!("duplicate inbound tag: {tag}");
                }
            }
        }
        Ok(())
    }

    pub fn route_final(&self) -> Option<&str> {
        self.route
            .as_ref()
            .and_then(|r| r.final_tag.as_deref().or(r.final_.as_deref()))
    }

    /// Default outbound tag: `route.final`, else first outbound's explicit tag, else `"0"`.
    pub fn default_outbound_tag(&self) -> Result<String> {
        if let Some(tag) = self.route_final() {
            return Ok(tag.to_string());
        }
        let Some(first) = self.outbounds.first() else {
            anyhow::bail!("no outbounds configured");
        };
        Ok(self.outbound_tag(first, 0))
    }

    pub fn inbound_tag(&self, inbound: &Inbound, index: usize) -> String {
        inbound.tag.clone().unwrap_or_else(|| index.to_string())
    }

    pub fn outbound_tag(&self, outbound: &Outbound, index: usize) -> String {
        outbound.tag.clone().unwrap_or_else(|| index.to_string())
    }

    pub fn is_known_inbound(kind: &str) -> bool {
        c::ALL_INBOUND_TYPES.contains(&kind)
    }

    pub fn is_known_outbound(kind: &str) -> bool {
        c::ALL_OUTBOUND_TYPES.contains(&kind)
    }
}

fn default_log_level() -> String {
    "info".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal() {
        let cfg = Options::from_json(
            r#"{"inbounds":[{"type":"mixed","listen":"127.0.0.1","listen_port":7890}],"outbounds":[{"type":"direct","tag":"direct"}]}"#,
        )
        .unwrap();
        assert_eq!(cfg.inbounds[0].kind, "mixed");
    }

    #[test]
    fn default_outbound_without_tag() {
        let cfg = Options::from_json(
            r#"{"inbounds":[],"outbounds":[{"type":"direct"},{"type":"block","tag":"block"}]}"#,
        )
        .unwrap();
        assert_eq!(cfg.default_outbound_tag().unwrap(), "0");
    }

    #[test]
    fn default_outbound_from_route_final() {
        let cfg = Options::from_json(
            r#"{"outbounds":[{"type":"direct","tag":"d"},{"type":"block","tag":"b"}],"route":{"final":"b"}}"#,
        )
        .unwrap();
        assert_eq!(cfg.default_outbound_tag().unwrap(), "b");
    }
}
