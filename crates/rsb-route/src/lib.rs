mod embedded_geo;
mod geo;
mod rule_cache;
mod srs;

use embedded_geo::{embedded_geo_srs, embedded_geosite_cn_text};
use geo::{
    builtin_geoip_private_cidrs, collect_geo_tags_from_rules, collect_remote_geo_rule_sets,
    expand_china_direct_preset,
};
use rule_cache::RuleSetCache;

use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_config::{RouteOptions, RuleSet};
use rsb_core::{is_private_ip, Metadata, Router};
use srs::parse_srs;
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use tracing::info;

pub struct RuleRouter {
    rules: RouteOptions,
    default_tag: String,
    rule_sets: Arc<HashMap<String, Arc<CompiledRuleSet>>>,
    rule_set_cache: Option<RuleSetCache>,
}

#[derive(Clone)]
pub struct CompiledRuleSet {
    domains: Vec<String>,
    domain_suffixes: Vec<String>,
    domain_keywords: Vec<String>,
    ip_cidrs: Vec<String>,
    ip_ranges: Vec<(IpAddr, IpAddr)>,
    /// Lowercased exact domains for O(1) lookup.
    domain_set: std::collections::HashSet<String>,
    /// Lowercased suffixes (no leading dot) for O(labels) match.
    suffix_set: std::collections::HashSet<String>,
}

impl CompiledRuleSet {
    fn finalize(mut self) -> Self {
        self.domain_set = self
            .domains
            .iter()
            .map(|d| d.to_ascii_lowercase())
            .collect();
        self.suffix_set = self
            .domain_suffixes
            .iter()
            .map(|s| {
                s.trim_start_matches('*')
                    .trim_start_matches('.')
                    .to_ascii_lowercase()
            })
            .filter(|s| !s.is_empty())
            .collect();
        // Exact domains stay exact-only (do NOT promote into suffix_set —
        // otherwise adservice.google.com would make *.google.com match).
        self
    }
}

impl RuleRouter {
    pub fn new(mut rules: RouteOptions, default_tag: String) -> Self {
        expand_china_direct_preset(&mut rules, &default_tag);
        let rule_set_cache = rules
            .rule_set_download
            .as_ref()
            .and_then(|opt| opt.path.as_ref())
            .map(RuleSetCache::new)
            .or_else(|| Some(RuleSetCache::default_path()));
        let rule_sets = Arc::new(HashMap::new());
        Self {
            rules,
            default_tag,
            rule_sets,
            rule_set_cache,
        }
    }

    pub async fn load_rule_sets(&mut self) -> Result<()> {
        let mut map = HashMap::new();
        for (i, rs) in self.rules.rule_set.iter().enumerate() {
            let tag = rs.tag.clone().unwrap_or_else(|| format!("ruleset-{i}"));
            let compiled = load_rule_set(&tag, rs, self.rule_set_cache.as_ref())
                .await
                .with_context(|| format!("load rule_set `{tag}`"))?;
            info!(tag = %tag, domains = compiled.domains.len(), "rule-set loaded");
            map.insert(tag, Arc::new(compiled));
        }
        let (geosite_tags, geoip_tags) = collect_geo_tags_from_rules(&self.rules.rules);
        for (tag, rs) in collect_remote_geo_rule_sets(&geosite_tags, &geoip_tags) {
            if map.contains_key(&tag) {
                continue;
            }
            // geosite-cn: prefer fast pre-extracted text (SRS succinct parse is too slow).
            if tag == "geosite-cn" {
                let compiled = parse_text_rules(embedded_geosite_cn_text());
                info!(
                    tag = %tag,
                    domains = compiled.domains.len(),
                    suffixes = compiled.domain_suffixes.len(),
                    "embedded geosite text loaded"
                );
                map.insert(tag, Arc::new(compiled));
                continue;
            }
            // Prefer binary baked into rsbox (offline / no download).
            if let Some(bytes) = embedded_geo_srs(&tag) {
                match parse_srs(bytes) {
                    Ok(parsed) => {
                        info!(
                            tag = %tag,
                            domains = parsed.domains.len(),
                            cidrs = parsed.ip_cidrs.len(),
                            "embedded geo rule-set loaded"
                        );
                        if let Some(cache) = self.rule_set_cache.as_ref() {
                            let path = cache.file_path(&tag, true);
                            if let Some(parent) = path.parent() {
                                let _ = std::fs::create_dir_all(parent);
                            }
                            let _ = std::fs::write(&path, bytes);
                        }
                        map.insert(
                            tag,
                            Arc::new(
                                CompiledRuleSet {
                                    domains: parsed.domains,
                                    domain_suffixes: parsed.domain_suffixes,
                                    domain_keywords: parsed.domain_keywords,
                                    ip_cidrs: parsed.ip_cidrs,
                                    ip_ranges: parsed.ip_ranges,
                                    domain_set: Default::default(),
                                    suffix_set: Default::default(),
                                }
                                .finalize(),
                            ),
                        );
                        continue;
                    },
                    Err(err) => {
                        tracing::warn!(tag = %tag, error = %err, "embedded geo parse failed");
                    },
                }
            }
            match load_rule_set(&tag, &rs, self.rule_set_cache.as_ref()).await {
                Ok(compiled) => {
                    info!(tag = %tag, domains = compiled.domains.len(), cidrs = compiled.ip_cidrs.len(), "geo rule-set loaded");
                    map.insert(tag, Arc::new(compiled));
                },
                Err(err) => {
                    tracing::warn!(tag = %tag, error = %err, "geo rule-set load failed");
                },
            }
        }
        if geoip_tags.contains("private") {
            map.insert(
                "geoip-private".into(),
                Arc::new(
                    CompiledRuleSet {
                        domains: Vec::new(),
                        domain_suffixes: Vec::new(),
                        domain_keywords: Vec::new(),
                        ip_cidrs: builtin_geoip_private_cidrs(),
                        ip_ranges: Vec::new(),
                        domain_set: Default::default(),
                        suffix_set: Default::default(),
                    }
                    .finalize(),
                ),
            );
        }
        self.rule_sets = Arc::new(map);
        Ok(())
    }
}

#[async_trait]
impl Router for RuleRouter {
    async fn route(&self, metadata: &Metadata) -> Result<String> {
        for rule in &self.rules.rules {
            if !rule.inbound.is_empty() && !rule.inbound.iter().any(|t| t == &metadata.inbound_tag)
            {
                continue;
            }
            if !rule.network.is_empty() {
                let net = match metadata.network {
                    rsb_core::Network::Tcp => "tcp",
                    rsb_core::Network::Udp => "udp",
                };
                if !rule.network.iter().any(|n| n.eq_ignore_ascii_case(net)) {
                    continue;
                }
            }
            if !rule.port.is_empty() {
                let Some(dest) = metadata.destination else {
                    continue;
                };
                if !port_rule_matches(&rule.port, dest.port()) {
                    continue;
                }
            }
            if !rule.source_ip_cidr.is_empty() {
                let Some(src) = metadata.source else {
                    continue;
                };
                if !rule.source_ip_cidr.iter().any(|c| ip_in_cidr(src.ip(), c)) {
                    continue;
                }
            }
            if !rule.protocol.is_empty() {
                let Some(detected) = metadata.protocol.as_deref() else {
                    continue;
                };
                if !rule
                    .protocol
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(detected))
                {
                    continue;
                }
            }
            if !rule.process_name.is_empty() {
                let Some(name) = metadata.process_name.as_deref() else {
                    continue;
                };
                if !rule
                    .process_name
                    .iter()
                    .any(|n| process_name_matches(n, name))
                {
                    continue;
                }
            }
            if !rule.process_path.is_empty() {
                let Some(path) = metadata.process_path.as_deref() else {
                    continue;
                };
                if !rule
                    .process_path
                    .iter()
                    .any(|p| process_path_matches(p, path))
                {
                    continue;
                }
            }
            if !rule.process_path_regex.is_empty() {
                let Some(path) = metadata.process_path.as_deref() else {
                    continue;
                };
                if !rule
                    .process_path_regex
                    .iter()
                    .any(|re| process_path_regex_matches(re, path))
                {
                    continue;
                }
            }
            if rule.ip_is_private {
                let Some(dest) = metadata.destination else {
                    continue;
                };
                if !is_private_ip(dest.ip()) {
                    continue;
                }
                return Ok(rule_outbound(rule).unwrap_or(self.default_tag.clone()));
            }
            if !rule.geosite.is_empty() || !rule.geoip.is_empty() {
                let mut matched = false;
                for code in &rule.geosite {
                    let tag = format!("geosite-{code}");
                    if let Some(rs) = self.rule_sets.get(&tag) {
                        if rule_set_matches(rs, metadata) {
                            matched = true;
                            break;
                        }
                    }
                }
                if !matched {
                    for code in &rule.geoip {
                        if code == "private"
                            && metadata
                                .destination
                                .map(|d| is_private_ip(d.ip()))
                                .unwrap_or(false)
                        {
                            matched = true;
                            break;
                        }
                        let tag = format!("geoip-{code}");
                        if let Some(rs) = self.rule_sets.get(&tag) {
                            if metadata
                                .destination
                                .map(|d| geoip_rule_set_matches(rs, d.ip()))
                                .unwrap_or(false)
                            {
                                matched = true;
                                break;
                            }
                        }
                    }
                }
                if !matched {
                    continue;
                }
                return Ok(rule_outbound(rule).unwrap_or(self.default_tag.clone()));
            }
            if !rule.rule_set.is_empty() {
                let mut matched = false;
                for rs_tag in &rule.rule_set {
                    if let Some(rs) = self.rule_sets.get(rs_tag) {
                        if rule_set_matches(rs, metadata) {
                            matched = true;
                            break;
                        }
                    }
                }
                if !matched {
                    continue;
                }
                return Ok(rule_outbound(rule).unwrap_or(self.default_tag.clone()));
            }
            if let Some(domain) = &metadata.domain {
                if rule.domain.iter().any(|d| d == domain) {
                    return Ok(rule_outbound(rule).unwrap_or(self.default_tag.clone()));
                }
                if rule.domain_suffix.iter().any(|s| domain_matches_suffix(domain, s)) {
                    return Ok(rule_outbound(rule).unwrap_or(self.default_tag.clone()));
                }
                if rule.domain_keyword.iter().any(|k| domain.contains(k)) {
                    return Ok(rule_outbound(rule).unwrap_or(self.default_tag.clone()));
                }
            }
            if let Some(dest) = metadata.destination {
                let ip = dest.ip();
                for cidr in &rule.ip_cidr {
                    if ip_in_cidr(ip, cidr) {
                        return Ok(rule_outbound(rule).unwrap_or(self.default_tag.clone()));
                    }
                }
            }
            if rule_constraint_only_match(rule) {
                return Ok(rule_outbound(rule).unwrap_or(self.default_tag.clone()));
            }
        }
        Ok(self.default_tag.clone())
    }
}

/// Suffix match with label boundary: `le.com` must not match `google.com`.
fn domain_matches_suffix(domain: &str, suffix: &str) -> bool {
    let s = suffix
        .trim_start_matches('*')
        .trim_start_matches('.')
        .trim();
    if s.is_empty() {
        return false;
    }
    domain.eq_ignore_ascii_case(s)
        || domain
            .len()
            > s.len()
            && domain.as_bytes().get(domain.len() - s.len() - 1) == Some(&b'.')
            && domain[domain.len() - s.len()..].eq_ignore_ascii_case(s)
}

fn rule_constraint_only_match(rule: &rsb_config::RouteRule) -> bool {
    let has_constraint = !rule.protocol.is_empty()
        || !rule.process_name.is_empty()
        || !rule.process_path.is_empty()
        || !rule.process_path_regex.is_empty();
    has_constraint && rule_positive_matchers_empty(rule)
}

fn process_name_matches(expected: &str, actual: &str) -> bool {
    expected.eq_ignore_ascii_case(actual)
}

fn process_path_matches(expected: &str, actual: &str) -> bool {
    expected.eq_ignore_ascii_case(actual)
        || std::path::Path::new(actual)
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|name| expected.eq_ignore_ascii_case(name))
}

fn process_path_regex_matches(pattern: &str, path: &str) -> bool {
    regex::Regex::new(pattern)
        .map(|re| re.is_match(path))
        .unwrap_or(false)
}

fn rule_positive_matchers_empty(rule: &rsb_config::RouteRule) -> bool {
    !rule.ip_is_private
        && rule.geosite.is_empty()
        && rule.geoip.is_empty()
        && rule.rule_set.is_empty()
        && rule.domain.is_empty()
        && rule.domain_suffix.is_empty()
        && rule.domain_keyword.is_empty()
        && rule.ip_cidr.is_empty()
}

fn geoip_rule_set_matches(rs: &CompiledRuleSet, ip: IpAddr) -> bool {
    for cidr in &rs.ip_cidrs {
        if ip_in_cidr(ip, cidr) {
            return true;
        }
    }
    for (from, to) in &rs.ip_ranges {
        if ip_in_range(ip, *from, *to) {
            return true;
        }
    }
    false
}

fn rule_set_matches(rs: &CompiledRuleSet, metadata: &Metadata) -> bool {
    if let Some(domain) = &metadata.domain {
        let host = domain.to_ascii_lowercase();
        if rs.domain_set.contains(&host) || domain_matches_suffix_set(&host, &rs.suffix_set) {
            return true;
        }
        if rs.domain_keywords.iter().any(|k| host.contains(&k.to_ascii_lowercase())) {
            return true;
        }
    }
    if let Some(dest) = metadata.destination {
        for cidr in &rs.ip_cidrs {
            if ip_in_cidr(dest.ip(), cidr) {
                return true;
            }
        }
        for (from, to) in &rs.ip_ranges {
            if ip_in_range(dest.ip(), *from, *to) {
                return true;
            }
        }
    }
    false
}

/// Match host against a set of suffixes in O(label count), not O(rule count).
fn domain_matches_suffix_set(host: &str, suffixes: &std::collections::HashSet<String>) -> bool {
    if suffixes.is_empty() {
        return false;
    }
    if suffixes.contains(host) {
        return true;
    }
    let mut rest = host;
    while let Some(idx) = rest.find('.') {
        rest = &rest[idx + 1..];
        if suffixes.contains(rest) {
            return true;
        }
    }
    false
}

async fn load_rule_set(
    tag: &str,
    rs: &RuleSet,
    cache: Option<&RuleSetCache>,
) -> Result<CompiledRuleSet> {
    let binary = rs.format.as_deref() == Some("binary")
        || rs.path.as_deref().is_some_and(|p| p.ends_with(".srs"))
        || rs.url.as_deref().is_some_and(|u| u.ends_with(".srs"))
        || rs.urls.iter().any(|u| u.ends_with(".srs"));

    if binary {
        let bytes = if let Some(path) = &rs.path {
            tokio::fs::read(path)
                .await
                .with_context(|| format!("read binary rule-set `{path}`"))?
        } else if rs.url.is_some() || !rs.urls.is_empty() {
            let urls = rule_set_urls(rs);
            if let Some(cache) = cache {
                cache.read_or_fetch_urls(tag, &urls, true).await?
            } else {
                fetch_first_url(&urls).await?
            }
        } else {
            anyhow::bail!("binary rule-set requires path or url");
        };
        let parsed = parse_srs(&bytes)?;
        return Ok(CompiledRuleSet {
            domains: parsed.domains,
            domain_suffixes: parsed.domain_suffixes,
            domain_keywords: parsed.domain_keywords,
            ip_cidrs: parsed.ip_cidrs,
            ip_ranges: parsed.ip_ranges,
            domain_set: Default::default(),
            suffix_set: Default::default(),
        }
        .finalize());
    }

    let text = if let Some(path) = &rs.path {
        tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("read rule-set path `{path}`"))?
    } else if rs.url.is_some() || !rs.urls.is_empty() {
        let urls = rule_set_urls(rs);
        if let Some(cache) = cache {
            let bytes = cache.read_or_fetch_urls(tag, &urls, false).await?;
            String::from_utf8(bytes).context("rule-set cache is not valid utf-8")?
        } else {
            let bytes = fetch_first_url(&urls).await?;
            String::from_utf8(bytes).context("rule-set body is not valid utf-8")?
        }
    } else if let Some(inline) = rs.raw.get("rules").and_then(|v| v.as_array()) {
        return parse_json_rules(inline);
    } else {
        anyhow::bail!("rule-set requires path, url, or inline rules");
    };
    Ok(parse_text_rules(&text))
}

fn rule_set_urls(rs: &RuleSet) -> Vec<String> {
    let mut urls = Vec::new();
    if let Some(u) = &rs.url {
        urls.push(u.clone());
    }
    for u in &rs.urls {
        if !urls.iter().any(|x| x == u) {
            urls.push(u.clone());
        }
    }
    urls
}

async fn fetch_first_url(urls: &[String]) -> Result<Vec<u8>> {
    let mut last_err: Option<anyhow::Error> = None;
    for url in urls {
        match reqwest::get(url).await {
            Ok(resp) if resp.status().is_success() => {
                return Ok(resp
                    .bytes()
                    .await
                    .with_context(|| format!("read rule-set body `{url}`"))?
                    .to_vec());
            },
            Ok(resp) => {
                last_err = Some(anyhow::anyhow!(
                    "fetch rule-set `{url}` status {}",
                    resp.status()
                ));
            },
            Err(err) => {
                last_err = Some(anyhow::Error::new(err).context(format!("fetch rule-set `{url}`")));
            },
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no rule-set urls")))
}

fn parse_text_rules(text: &str) -> CompiledRuleSet {
    let mut domains = Vec::new();
    let mut domain_suffixes = Vec::new();
    let mut domain_keywords = Vec::new();
    let mut ip_cidrs = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
            continue;
        }
        if line.contains('/') && line.chars().next().unwrap_or('a').is_ascii_digit() {
            ip_cidrs.push(line.to_string());
        } else if line.starts_with("*.") || line.starts_with('.') {
            domain_suffixes.push(line.trim_start_matches('*').to_string());
        } else if line.starts_with('+') {
            domain_keywords.push(line.trim_start_matches('+').to_string());
        } else {
            domains.push(line.to_string());
        }
    }
    CompiledRuleSet {
        domains,
        domain_suffixes,
        domain_keywords,
        ip_cidrs,
        ip_ranges: Vec::new(),
        domain_set: Default::default(),
        suffix_set: Default::default(),
    }
    .finalize()
}

fn parse_json_rules(rules: &[serde_json::Value]) -> Result<CompiledRuleSet> {
    let mut compiled = CompiledRuleSet {
        domains: Vec::new(),
        domain_suffixes: Vec::new(),
        domain_keywords: Vec::new(),
        ip_cidrs: Vec::new(),
        ip_ranges: Vec::new(),
        domain_set: Default::default(),
        suffix_set: Default::default(),
    };
    for rule in rules {
        if let Some(arr) = rule.get("domain").and_then(|v| v.as_array()) {
            compiled
                .domains
                .extend(arr.iter().filter_map(|v| v.as_str().map(str::to_string)));
        }
        if let Some(arr) = rule.get("domain_suffix").and_then(|v| v.as_array()) {
            compiled
                .domain_suffixes
                .extend(arr.iter().filter_map(|v| v.as_str().map(str::to_string)));
        }
        if let Some(arr) = rule.get("domain_keyword").and_then(|v| v.as_array()) {
            compiled
                .domain_keywords
                .extend(arr.iter().filter_map(|v| v.as_str().map(str::to_string)));
        }
        if let Some(arr) = rule.get("ip_cidr").and_then(|v| v.as_array()) {
            compiled
                .ip_cidrs
                .extend(arr.iter().filter_map(|v| v.as_str().map(str::to_string)));
        }
    }
    Ok(compiled.finalize())
}

fn rule_outbound(rule: &rsb_config::RouteRule) -> Option<String> {
    rule.outbound.clone()
}

fn port_rule_matches(ports: &[serde_json::Value], port: u16) -> bool {
    for p in ports {
        if let Some(n) = p.as_u64() {
            if n as u16 == port {
                return true;
            }
        } else if let Some(s) = p.as_str() {
            if s.contains(':') {
                let (_, pr) = s.rsplit_once(':').unwrap_or(("", s));
                if pr.parse::<u16>().ok() == Some(port) {
                    return true;
                }
            } else if s.contains('-') {
                let parts: Vec<_> = s.split('-').collect();
                if parts.len() == 2 {
                    if let (Ok(from), Ok(to)) = (parts[0].parse::<u16>(), parts[1].parse::<u16>()) {
                        if port >= from && port <= to {
                            return true;
                        }
                    }
                }
            } else if s.parse::<u16>().ok() == Some(port) {
                return true;
            }
        }
    }
    false
}

fn ip_in_range(ip: IpAddr, from: IpAddr, to: IpAddr) -> bool {
    match (ip, from, to) {
        (IpAddr::V4(a), IpAddr::V4(f), IpAddr::V4(t)) => {
            let ai = u32::from(a);
            ai >= u32::from(f) && ai <= u32::from(t)
        },
        (IpAddr::V6(a), IpAddr::V6(f), IpAddr::V6(t)) => {
            let ai = u128::from(a);
            ai >= u128::from(f) && ai <= u128::from(t)
        },
        _ => false,
    }
}

fn ip_in_cidr(ip: IpAddr, cidr: &str) -> bool {
    let Some((net, prefix)) = cidr.split_once('/') else {
        return false;
    };
    let Ok(net_ip) = net.parse::<IpAddr>() else {
        return false;
    };
    let Ok(prefix_len) = prefix.parse::<u8>() else {
        return false;
    };
    match (ip, net_ip) {
        (IpAddr::V4(a), IpAddr::V4(b)) => {
            let mask = if prefix_len >= 32 {
                u32::MAX
            } else {
                u32::MAX << (32 - prefix_len)
            };
            (u32::from(a) & mask) == (u32::from(b) & mask)
        },
        (IpAddr::V6(a), IpAddr::V6(b)) => {
            let ai = u128::from(a);
            let bi = u128::from(b);
            let mask = if prefix_len >= 128 {
                u128::MAX
            } else {
                u128::MAX << (128 - prefix_len)
            };
            (ai & mask) == (bi & mask)
        },
        _ => false,
    }
}

pub fn load_local_rule_set(path: &Path) -> Result<CompiledRuleSet> {
    let text = std::fs::read_to_string(path)?;
    Ok(parse_text_rules(&text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rsb_config::{RouteOptions, RouteRule};
    use rsb_core::{Metadata, Network};
    use std::collections::HashMap;
    use std::net::{Ipv4Addr, SocketAddr};

    #[tokio::test]
    async fn route_protocol_only_rule() {
        let router = RuleRouter {
            rules: RouteOptions {
                rules: vec![RouteRule {
                    protocol: vec!["tls".into()],
                    outbound: Some("proxy".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            default_tag: "direct".into(),
            rule_sets: Arc::new(HashMap::new()),
            rule_set_cache: None,
        };
        let metadata = Metadata {
            network: Network::Tcp,
            destination: Some(SocketAddr::from((Ipv4Addr::LOCALHOST, 443))),
            protocol: Some("tls".into()),
            ..Default::default()
        };
        assert_eq!(router.route(&metadata).await.unwrap(), "proxy");
    }

    #[tokio::test]
    async fn route_process_name_only_rule() {
        let router = RuleRouter {
            rules: RouteOptions {
                rules: vec![RouteRule {
                    process_name: vec!["curl".into()],
                    outbound: Some("proxy".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            default_tag: "direct".into(),
            rule_sets: Arc::new(HashMap::new()),
            rule_set_cache: None,
        };
        let metadata = Metadata {
            network: Network::Tcp,
            process_name: Some("curl".into()),
            ..Default::default()
        };
        assert_eq!(router.route(&metadata).await.unwrap(), "proxy");
    }

    #[tokio::test]
    async fn china_direct_preset_and_private() {
        let router = RuleRouter::new(
            RouteOptions {
                preset: Some("china-direct".into()),
                direct_tag: Some("direct".into()),
                final_tag: Some("proxy".into()),
                ..Default::default()
            },
            "proxy".into(),
        );
        assert!(router
            .rules
            .rules
            .iter()
            .any(|r| r.geosite.iter().any(|g| g == "cn")));
        assert!(router
            .rules
            .rules
            .iter()
            .any(|r| r.geoip.iter().any(|g| g == "cn")));
        assert!(router.rules.rules.iter().any(|r| r.ip_is_private));

        let metadata = Metadata {
            network: Network::Tcp,
            destination: Some(SocketAddr::from((Ipv4Addr::new(192, 168, 1, 1), 443))),
            ..Default::default()
        };
        assert_eq!(router.route(&metadata).await.unwrap(), "direct");
    }

    #[test]
    fn domain_suffix_label_boundary() {
        assert!(domain_matches_suffix("www.baidu.com", ".baidu.com"));
        assert!(domain_matches_suffix("baidu.com", "baidu.com"));
        assert!(!domain_matches_suffix("google.com", "le.com"));
        assert!(domain_matches_suffix("foo.com.cn", ".cn"));
    }
}
