//! Built-in geoip codes, remote geosite/geoip rule-set URLs, and China-direct presets.

use rsb_config::{RouteOptions, RouteRule, RuleSet};
use std::collections::HashSet;

pub fn builtin_geoip_private_cidrs() -> Vec<String> {
    vec![
        "10.0.0.0/8".into(),
        "172.16.0.0/12".into(),
        "192.168.0.0/16".into(),
        "127.0.0.0/8".into(),
        "169.254.0.0/16".into(),
        "100.64.0.0/10".into(),
        "0.0.0.0/8".into(),
        "192.0.0.0/24".into(),
        "192.0.2.0/24".into(),
        "198.18.0.0/15".into(),
        "198.51.100.0/24".into(),
        "203.0.113.0/24".into(),
        "224.0.0.0/3".into(),
        "::1/128".into(),
        "fc00::/7".into(),
        "fe80::/10".into(),
    ]
}

/// Comprehensive China-direct rule list for smart split.
///
/// Uses SagerNet `geosite-cn` / `geoip-cn` (community-maintained, very large)
/// plus `.cn` / private fallbacks so offline / download-fail still works.
pub fn china_direct_rules(direct_tag: &str) -> Vec<RouteRule> {
    let direct = Some(direct_tag.to_string());
    vec![
        RouteRule {
            geoip: vec!["private".into()],
            outbound: direct.clone(),
            ..Default::default()
        },
        RouteRule {
            ip_is_private: true,
            outbound: direct.clone(),
            ..Default::default()
        },
        RouteRule {
            domain_suffix: vec![
                ".cn".into(),
                ".中国".into(),
                ".xn--fiqs8s".into(),
                ".xn--fiqz9s".into(),
            ],
            outbound: direct.clone(),
            ..Default::default()
        },
        RouteRule {
            geosite: vec!["cn".into()],
            outbound: direct.clone(),
            ..Default::default()
        },
        RouteRule {
            geoip: vec!["cn".into()],
            outbound: direct,
            ..Default::default()
        },
    ]
}

/// Expand `route.preset = "china-direct"|"smart"` into concrete rules.
///
/// User-defined `rules` keep higher priority (prepended). `final` defaults to
/// `proxy_tag` / current default outbound when unset.
pub fn expand_china_direct_preset(route: &mut RouteOptions, default_proxy_tag: &str) {
    let preset = route.preset.as_deref().unwrap_or("").to_ascii_lowercase();
    if preset != "china-direct" && preset != "smart" {
        return;
    }
    let direct = route
        .direct_tag
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("direct");
    let proxy = route
        .proxy_tag
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(default_proxy_tag);
    if route.final_tag.is_none() && route.final_.is_none() {
        route.final_tag = Some(proxy.to_string());
    }
    let mut preset_rules = china_direct_rules(direct);
    let mut user_rules = std::mem::take(&mut route.rules);
    user_rules.append(&mut preset_rules);
    route.rules = user_rules;
    tracing::info!(
        preset = %preset,
        direct = %direct,
        proxy = %proxy,
        rules = route.rules.len(),
        "route preset china-direct applied"
    );
}

fn geosite_urls(code: &str) -> Vec<String> {
    vec![
        format!(
            "https://cdn.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geosite-{code}.srs"
        ),
        format!(
            "https://fastly.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geosite-{code}.srs"
        ),
        format!(
            "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-{code}.srs"
        ),
    ]
}

fn geoip_urls(code: &str) -> Vec<String> {
    vec![
        format!("https://cdn.jsdelivr.net/gh/SagerNet/sing-geoip@rule-set/geoip-{code}.srs"),
        format!("https://fastly.jsdelivr.net/gh/SagerNet/sing-geoip@rule-set/geoip-{code}.srs"),
        format!(
            "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-{code}.srs"
        ),
    ]
}

pub fn collect_remote_geo_rule_sets(
    geosite: &HashSet<String>,
    geoip: &HashSet<String>,
) -> Vec<(String, RuleSet)> {
    let mut out = Vec::new();
    for code in geosite {
        let tag = format!("geosite-{code}");
        let urls = geosite_urls(code);
        out.push((
            tag.clone(),
            RuleSet {
                tag: Some(tag),
                format: Some("binary".into()),
                url: urls.first().cloned(),
                urls,
                ..Default::default()
            },
        ));
    }
    for code in geoip {
        if code == "private" {
            continue;
        }
        let tag = format!("geoip-{code}");
        let urls = geoip_urls(code);
        out.push((
            tag.clone(),
            RuleSet {
                tag: Some(tag),
                format: Some("binary".into()),
                url: urls.first().cloned(),
                urls,
                ..Default::default()
            },
        ));
    }
    out
}

pub fn collect_geo_tags_from_rules(
    rules: &[rsb_config::RouteRule],
) -> (HashSet<String>, HashSet<String>) {
    let mut geosite = HashSet::new();
    let mut geoip = HashSet::new();
    for rule in rules {
        for g in &rule.geosite {
            geosite.insert(g.clone());
        }
        for g in &rule.geoip {
            geoip.insert(g.clone());
        }
    }
    (geosite, geoip)
}
