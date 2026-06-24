//! Built-in geoip codes and remote geosite/geoip rule-set URLs.

use rsb_config::RuleSet;
use std::collections::HashSet;

pub fn builtin_geoip_private_cidrs() -> Vec<String> {
    vec![
        "10.0.0.0/8".into(),
        "172.16.0.0/12".into(),
        "192.168.0.0/16".into(),
        "127.0.0.0/8".into(),
        "169.254.0.0/16".into(),
        "::1/128".into(),
        "fc00::/7".into(),
        "fe80::/10".into(),
    ]
}

pub fn collect_remote_geo_rule_sets(
    geosite: &HashSet<String>,
    geoip: &HashSet<String>,
) -> Vec<(String, RuleSet)> {
    let mut out = Vec::new();
    for code in geosite {
        let tag = format!("geosite-{code}");
        out.push((
            tag.clone(),
            RuleSet {
                tag: Some(tag),
                format: Some("binary".into()),
                url: Some(format!(
                    "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-{code}.srs"
                )),
                ..Default::default()
            },
        ));
    }
    for code in geoip {
        if code == "private" {
            continue;
        }
        let tag = format!("geoip-{code}");
        out.push((
            tag.clone(),
            RuleSet {
                tag: Some(tag),
                format: Some("binary".into()),
                url: Some(format!(
                    "https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-{code}.srs"
                )),
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
