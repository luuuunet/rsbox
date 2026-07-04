use crate::rate_limit::RateLimiter;
use crate::user_registry::{UserLimits, UserRegistry, UserRuntime};
use anyhow::{bail, Result};
use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Clone, Debug, Default)]
pub struct TrafficStats {
    pub uplink: Arc<AtomicU64>,
    pub downlink: Arc<AtomicU64>,
}

impl TrafficStats {
    pub fn new() -> Self {
        Self {
            uplink: Arc::new(AtomicU64::new(0)),
            downlink: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn add_uplink(&self, n: u64) {
        self.uplink.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_downlink(&self, n: u64) {
        self.downlink.fetch_add(n, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> (u64, u64) {
        (
            self.uplink.load(Ordering::Relaxed),
            self.downlink.load(Ordering::Relaxed),
        )
    }
}

#[derive(Clone, Debug)]
pub struct ConnectionInfo {
    pub id: u64,
    pub inbound_tag: String,
    pub outbound_tag: String,
    pub network: String,
    pub source: Option<SocketAddr>,
    pub destination: Option<SocketAddr>,
    pub domain: Option<String>,
    pub user: Option<String>,
    pub started_at: u64,
}

pub struct ConnectionManager {
    next_id: AtomicU64,
    active: DashMap<u64, ConnectionInfo>,
    total: AtomicU64,
    global: TrafficStats,
    by_outbound: DashMap<String, TrafficStats>,
    by_inbound: DashMap<String, TrafficStats>,
    by_user: DashMap<String, TrafficStats>,
    users: Arc<UserRegistry>,
    limiters: DashMap<String, Arc<RateLimiter>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self::with_registry(Arc::new(UserRegistry::new()))
    }

    pub fn with_registry(users: Arc<UserRegistry>) -> Self {
        Self {
            next_id: AtomicU64::new(1),
            active: DashMap::new(),
            total: AtomicU64::new(0),
            global: TrafficStats::new(),
            by_outbound: DashMap::new(),
            by_inbound: DashMap::new(),
            by_user: DashMap::new(),
            users,
            limiters: DashMap::new(),
        }
    }

    pub fn users(&self) -> Arc<UserRegistry> {
        self.users.clone()
    }

    pub fn global_stats(&self) -> TrafficStats {
        self.global.clone()
    }

    pub fn outbound_stats(&self, tag: &str) -> TrafficStats {
        self.by_outbound.entry(tag.to_string()).or_default().clone()
    }

    pub fn inbound_stats(&self, tag: &str) -> TrafficStats {
        self.by_inbound.entry(tag.to_string()).or_default().clone()
    }

    pub fn user_stats(&self, name: &str) -> TrafficStats {
        self.by_user.entry(name.to_string()).or_default().clone()
    }

    pub fn user_limiter(&self, name: &str, speed_bps: Option<u64>) -> Option<Arc<RateLimiter>> {
        let bps = speed_bps.filter(|v| *v > 0)?;
        Some(
            self.limiters
                .entry(name.to_string())
                .or_insert_with(|| Arc::new(RateLimiter::new(bps)))
                .clone(),
        )
    }

    /// Check quotas and increment active connection count for a panel user.
    pub fn acquire_user(&self, name: &str, limits: &UserLimits) -> Result<UserSessionGuard> {
        let runtime = self.users.runtime(name);
        if let Some(max) = limits.max_connections {
            let active = runtime.active.load(Ordering::SeqCst);
            if active >= max {
                bail!("user `{name}` connection limit reached ({max})");
            }
        }
        if let Some(max_bytes) = limits.max_traffic_bytes {
            if runtime.total() >= max_bytes {
                bail!("user `{name}` traffic quota exceeded");
            }
        }
        runtime.active.fetch_add(1, Ordering::SeqCst);
        Ok(UserSessionGuard {
            _name: name.to_string(),
            runtime: Some(runtime),
        })
    }

    pub fn record_traffic(
        &self,
        inbound_tag: &str,
        outbound_tag: &str,
        uplink: u64,
        downlink: u64,
        user: Option<&str>,
    ) {
        self.global.add_uplink(uplink);
        self.global.add_downlink(downlink);
        self.outbound_stats(outbound_tag).add_uplink(uplink);
        self.outbound_stats(outbound_tag).add_downlink(downlink);
        self.inbound_stats(inbound_tag).add_uplink(uplink);
        self.inbound_stats(inbound_tag).add_downlink(downlink);
        if let Some(name) = user {
            self.user_stats(name).add_uplink(uplink);
            self.user_stats(name).add_downlink(downlink);
            let rt = self.users.runtime(name);
            rt.uplink.fetch_add(uplink, Ordering::Relaxed);
            rt.downlink.fetch_add(downlink, Ordering::Relaxed);
        }
    }

    /// Returns false when user traffic quota is exceeded (connection should be dropped).
    pub fn user_quota_ok(&self, name: &str, limits: &UserLimits) -> bool {
        limits
            .max_traffic_bytes
            .map(|max| self.users.runtime(name).total() < max)
            .unwrap_or(true)
    }

    pub fn v2ray_stat_entries(&self) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        let (up, down) = self.global.snapshot();
        out.push(("global>>>uplink".into(), up));
        out.push(("global>>>downlink".into(), down));
        for entry in self.by_inbound.iter() {
            let (up, down) = entry.value().snapshot();
            let tag = entry.key();
            out.push((format!("inbound>>>{tag}>>>traffic>>>uplink"), up));
            out.push((format!("inbound>>>{tag}>>>traffic>>>downlink"), down));
        }
        for entry in self.by_outbound.iter() {
            let (up, down) = entry.value().snapshot();
            let tag = entry.key();
            out.push((format!("outbound>>>{tag}>>>traffic>>>uplink"), up));
            out.push((format!("outbound>>>{tag}>>>traffic>>>downlink"), down));
        }
        for entry in self.by_user.iter() {
            let (up, down) = entry.value().snapshot();
            let name = entry.key();
            out.push((format!("user>>>{name}>>>traffic>>>uplink"), up));
            out.push((format!("user>>>{name}>>>traffic>>>downlink"), down));
        }
        out
    }

    pub fn query_v2ray_stats(&self, pattern: &str, reset: bool) -> Vec<(String, u64)> {
        let entries = self.v2ray_stat_entries();
        let matched: Vec<_> = if pattern.is_empty() {
            entries
        } else {
            entries
                .into_iter()
                .filter(|(name, _)| name.contains(pattern))
                .collect()
        };
        if reset {
            for (name, _) in &matched {
                self.reset_stat_name(name);
            }
        }
        matched
    }

    fn reset_stat_name(&self, name: &str) {
        if name == "global>>>uplink" {
            self.global.uplink.store(0, Ordering::Relaxed);
        } else if name == "global>>>downlink" {
            self.global.downlink.store(0, Ordering::Relaxed);
        } else if let Some(rest) = name.strip_prefix("inbound>>>") {
            if let Some((tag, _)) = rest.split_once(">>>traffic>>>") {
                let stats = self.inbound_stats(tag);
                if name.ends_with("uplink") {
                    stats.uplink.store(0, Ordering::Relaxed);
                } else {
                    stats.downlink.store(0, Ordering::Relaxed);
                }
            }
        } else if let Some(rest) = name.strip_prefix("outbound>>>") {
            if let Some((tag, _)) = rest.split_once(">>>traffic>>>") {
                let stats = self.outbound_stats(tag);
                if name.ends_with("uplink") {
                    stats.uplink.store(0, Ordering::Relaxed);
                } else {
                    stats.downlink.store(0, Ordering::Relaxed);
                }
            }
        } else if let Some(rest) = name.strip_prefix("user>>>") {
            if let Some((user, _)) = rest.split_once(">>>traffic>>>") {
                let stats = self.user_stats(user);
                if name.ends_with("uplink") {
                    stats.uplink.store(0, Ordering::Relaxed);
                } else {
                    stats.downlink.store(0, Ordering::Relaxed);
                }
                let rt = self.users.runtime(user);
                if name.ends_with("uplink") {
                    rt.uplink.store(0, Ordering::Relaxed);
                } else {
                    rt.downlink.store(0, Ordering::Relaxed);
                }
            }
        }
    }

    pub fn track(
        &self,
        inbound_tag: &str,
        outbound_tag: &str,
        network: &str,
        source: Option<SocketAddr>,
        destination: Option<SocketAddr>,
        domain: Option<String>,
        user: Option<String>,
    ) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.total.fetch_add(1, Ordering::Relaxed);
        self.active.insert(
            id,
            ConnectionInfo {
                id,
                inbound_tag: inbound_tag.to_string(),
                outbound_tag: outbound_tag.to_string(),
                network: network.to_string(),
                source,
                destination,
                domain,
                user,
                started_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
        );
        id
    }

    pub fn connection_info(&self, id: u64) -> Option<(String, String, Option<String>)> {
        self.active.get(&id).map(|c| {
            (
                c.inbound_tag.clone(),
                c.outbound_tag.clone(),
                c.user.clone(),
            )
        })
    }

    pub fn untrack(&self, id: u64) {
        self.active.remove(&id);
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn user_active_count(&self, name: &str) -> usize {
        self.users
            .runtime(name)
            .active
            .load(Ordering::Relaxed)
    }

    pub fn total_count(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    pub fn list(&self) -> Vec<ConnectionInfo> {
        self.active.iter().map(|e| e.value().clone()).collect()
    }

    pub fn resolve_user(&self, uuid: &Uuid) -> Option<Arc<crate::user_registry::UserRecord>> {
        self.users.lookup_uuid(uuid)
    }

    pub fn reload_users(&self, options: &rsb_config::Options) {
        self.users.reload_from_options(options);
    }
}

pub struct UserSessionGuard {
    _name: String,
    runtime: Option<Arc<UserRuntime>>,
}

impl UserSessionGuard {
    /// Placeholder for per-stream relays inside an already-acquired QUIC session.
    pub fn detached() -> Self {
        Self {
            _name: String::new(),
            runtime: None,
        }
    }
}

impl Drop for UserSessionGuard {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedConnectionManager = Arc<ConnectionManager>;
