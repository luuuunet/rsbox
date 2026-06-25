use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket, SharedOutboundManager};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
/// Runtime selector control for clash_api.
#[derive(Clone)]
pub struct SelectorControl {
    tag: String,
    outbounds: Vec<String>,
    selected: Arc<AtomicUsize>,
}

impl SelectorControl {
    pub fn tag(&self) -> &str {
        &self.tag
    }

    pub fn outbounds(&self) -> &[String] {
        &self.outbounds
    }

    pub fn selected(&self) -> String {
        let idx = self.selected.load(Ordering::SeqCst);
        self.outbounds
            .get(idx)
            .cloned()
            .or_else(|| self.outbounds.first().cloned())
            .unwrap_or_default()
    }

    pub fn select(&self, child: &str) -> Result<()> {
        let idx = self
            .outbounds
            .iter()
            .position(|t| t == child)
            .with_context(|| format!("outbound `{child}` not in selector `{}`", self.tag))?;
        self.selected.store(idx, Ordering::SeqCst);
        Ok(())
    }
}

pub struct OutboundController {
    selectors: parking_lot::RwLock<std::collections::HashMap<String, SelectorControl>>,
    urltests: parking_lot::RwLock<std::collections::HashMap<String, UrlTestControl>>,
    shared: Arc<SharedOutboundManager>,
}

impl OutboundController {
    pub fn new(shared: Arc<SharedOutboundManager>) -> Self {
        Self {
            selectors: parking_lot::RwLock::new(std::collections::HashMap::new()),
            urltests: parking_lot::RwLock::new(std::collections::HashMap::new()),
            shared,
        }
    }

    pub fn register_selector(&self, control: SelectorControl) {
        self.selectors
            .write()
            .insert(control.tag().to_string(), control);
    }

    pub fn register_urltest(&self, control: UrlTestControl) {
        self.urltests
            .write()
            .insert(control.tag().to_string(), control);
    }

    pub fn select(&self, selector_tag: &str, child: &str) -> Result<()> {
        self.selectors
            .read()
            .get(selector_tag)
            .context("selector not found")?
            .select(child)
    }

    pub fn selected(&self, group_tag: &str) -> Option<String> {
        if let Some(s) = self.selectors.read().get(group_tag) {
            return Some(s.selected());
        }
        self.urltests.read().get(group_tag).map(|u| u.selected())
    }

    /// Run latency probe for a urltest/selector group; returns (selected_tag, delays).
    pub async fn run_url_test(&self, group_tag: &str) -> Result<(String, Vec<(String, u32)>)> {
        let urltest = self.urltests.read().get(group_tag).cloned();
        if let Some(ut) = urltest {
            let delays = ut.run_probe().await?;
            let selected = ut.selected();
            return Ok((selected, delays));
        }
        let selector = self.selectors.read().get(group_tag).cloned();
        if let Some(sel) = selector {
            return Ok((sel.selected(), vec![]));
        }
        anyhow::bail!("group not found: {group_tag}")
    }

    pub fn restore_selectors(&self, selectors: &std::collections::HashMap<String, String>) {
        let map = self.selectors.read();
        for (group, child) in selectors {
            if let Some(sel) = map.get(group) {
                let _ = sel.select(child);
            }
        }
    }

    pub fn list_proxies(&self) -> Result<serde_json::Value> {
        let mgr = self.shared.get()?;
        let mut proxies = serde_json::Map::new();
        for control in self.selectors.read().values() {
            let mut entry = serde_json::Map::new();
            entry.insert("type".into(), "Selector".into());
            entry.insert("now".into(), control.selected().into());
            entry.insert(
                "all".into(),
                serde_json::Value::Array(
                    control
                        .outbounds()
                        .iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
                ),
            );
            proxies.insert(control.tag().to_string(), serde_json::Value::Object(entry));
        }
        for (tag, kind) in mgr.outbound_kinds() {
            if proxies.contains_key(&tag) {
                continue;
            }
            let mut entry = serde_json::Map::new();
            entry.insert("type".into(), kind.into());
            entry.insert("history".into(), serde_json::json!([]));
            proxies.insert(tag, serde_json::Value::Object(entry));
        }
        Ok(serde_json::Value::Object(proxies))
    }
}

/// Runtime urltest control for API probes.
#[derive(Clone)]
pub struct UrlTestControl {
    tag: String,
    outbounds: Vec<String>,
    url: String,
    selected: Arc<AtomicUsize>,
    shared: Arc<SharedOutboundManager>,
}

impl UrlTestControl {
    pub fn tag(&self) -> &str {
        &self.tag
    }

    pub fn selected(&self) -> String {
        let idx = self.selected.load(Ordering::SeqCst);
        self.outbounds
            .get(idx)
            .cloned()
            .or_else(|| self.outbounds.first().cloned())
            .unwrap_or_default()
    }

    pub async fn run_probe(&self) -> Result<Vec<(String, u32)>> {
        let raw = crate::urltest::probe_all(&self.outbounds, &self.url, &self.shared).await;
        let mut best_idx = 0usize;
        let mut best_ms = u32::MAX;
        let mut out = Vec::new();
        for (tag, delay) in raw {
            if let Some(ms) = delay {
                out.push((tag.clone(), ms));
                if ms < best_ms {
                    if let Some(idx) = self.outbounds.iter().position(|t| t == &tag) {
                        best_ms = ms;
                        best_idx = idx;
                    }
                }
            }
        }
        if !out.is_empty() {
            self.selected.store(best_idx, Ordering::SeqCst);
        }
        Ok(out)
    }
}

/// Selector with runtime hot-switch via `select_outbound`.
pub struct SelectorOutbound {
    tag: String,
    outbounds: Vec<String>,
    selected: Arc<AtomicUsize>,
    shared: Arc<SharedOutboundManager>,
}

impl SelectorOutbound {
    pub fn new(
        tag: String,
        raw: Value,
        shared: Arc<rsb_core::SharedOutboundManager>,
    ) -> Result<Self> {
        let outbounds: Vec<String> = raw
            .get("outbounds")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let default = raw
            .get("default")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let selected_idx = default
            .and_then(|d| outbounds.iter().position(|t| t == &d))
            .unwrap_or(0);
        Ok(Self {
            tag,
            outbounds,
            selected: Arc::new(AtomicUsize::new(selected_idx)),
            shared,
        })
    }

    pub fn select_outbound(&self, tag: &str) -> Result<()> {
        self.control().select(tag)
    }

    pub fn control(&self) -> SelectorControl {
        SelectorControl {
            tag: self.tag.clone(),
            outbounds: self.outbounds.clone(),
            selected: self.selected.clone(),
        }
    }

    fn selected_tag(&self) -> Result<&str> {
        let idx = self.selected.load(Ordering::SeqCst);
        self.outbounds
            .get(idx)
            .map(String::as_str)
            .or_else(|| self.outbounds.first().map(String::as_str))
            .context("selector has no outbounds configured")
    }
}

#[async_trait]
impl Outbound for SelectorOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_SELECTOR
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(&self, destination: SocketAddr, domain: Option<&str>) -> Result<ProxyConn, BoxError> {
        let child = self.selected_tag()?;
        self.shared.get()?.get(child)?.dial_tcp(destination, domain).await
    }
    async fn dial_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let child = self.selected_tag()?;
        self.shared.get()?.get(child)?.dial_udp(destination).await
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

pub struct UrlTestOutbound {
    tag: String,
    outbounds: Vec<String>,
    url: String,
    interval: Duration,
    selected: Arc<AtomicUsize>,
    shared: Arc<SharedOutboundManager>,
    handle: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl UrlTestOutbound {
    pub fn new(
        tag: String,
        raw: Value,
        shared: Arc<rsb_core::SharedOutboundManager>,
    ) -> Result<Self> {
        let outbounds: Vec<String> = raw
            .get("outbounds")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let url = raw
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("https://www.gstatic.com/generate_204")
            .to_string();
        let interval_secs = raw
            .get("interval")
            .and_then(|v| v.as_str())
            .and_then(|s| {
                if s.ends_with('s') {
                    s.trim_end_matches('s').parse().ok()
                } else {
                    s.parse().ok()
                }
            })
            .unwrap_or(300);
        Ok(Self {
            tag,
            outbounds,
            url,
            interval: Duration::from_secs(interval_secs),
            selected: Arc::new(AtomicUsize::new(0)),
            shared,
            handle: parking_lot::Mutex::new(None),
        })
    }

    pub fn control(&self) -> UrlTestControl {
        UrlTestControl {
            tag: self.tag.clone(),
            outbounds: self.outbounds.clone(),
            url: self.url.clone(),
            selected: self.selected.clone(),
            shared: self.shared.clone(),
        }
    }

    pub fn start_probe(&self) {
        if self.outbounds.is_empty() {
            return;
        }
        let ctrl = self.control();
        let interval = self.interval;
        let handle = tokio::spawn(async move {
            loop {
                let _ = ctrl.run_probe().await;
                tokio::time::sleep(interval).await;
            }
        });
        *self.handle.lock() = Some(handle);
    }

    fn selected_tag(&self) -> Result<&str> {
        let idx = self.selected.load(Ordering::SeqCst);
        self.outbounds
            .get(idx)
            .map(String::as_str)
            .or_else(|| self.outbounds.first().map(String::as_str))
            .context("urltest has no outbounds configured")
    }
}

#[async_trait]
impl Outbound for UrlTestOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_URLTEST
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(&self, destination: SocketAddr, domain: Option<&str>) -> Result<ProxyConn, BoxError> {
        if self.handle.lock().is_none() {
            self.start_probe();
        }
        let child = self.selected_tag()?;
        self.shared.get()?.get(child)?.dial_tcp(destination, domain).await
    }
    async fn dial_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let child = self.selected_tag()?;
        self.shared.get()?.get(child)?.dial_udp(destination).await
    }
    async fn close(&self) -> Result<(), BoxError> {
        if let Some(h) = self.handle.lock().take() {
            h.abort();
        }
        Ok(())
    }
}
