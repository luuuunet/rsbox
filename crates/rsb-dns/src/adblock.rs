/// DNS-based Ad Blocking Module
use anyhow::Result;
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct AdBlockConfig {
    pub enabled: bool,
    pub rules: Vec<String>,
    pub custom_rules: Vec<String>,
    pub update_interval: u64,
}

pub struct AdBlockFilter {
    domains: HashSet<String>,
}

impl AdBlockFilter {
    pub fn new() -> Self {
        Self {
            domains: HashSet::new(),
        }
    }

    pub fn should_block(&self, domain: &str) -> bool {
        if self.domains.contains(domain) {
            return true;
        }
        for blocked in &self.domains {
            if domain.ends_with(&format!(".{}", blocked)) {
                return true;
            }
        }
        false
    }

    pub fn parse_rule(&mut self, rule: &str) {
        if rule.starts_with("||") {
            let domain = rule.trim_start_matches("||").trim_end_matches('^');
            self.domains.insert(domain.to_string());
        } else {
            self.domains.insert(rule.to_string());
        }
    }
}
