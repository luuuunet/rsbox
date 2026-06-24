use dashmap::DashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
    pub started_at: u64,
}

pub struct ConnectionManager {
    next_id: AtomicU64,
    active: DashMap<u64, ConnectionInfo>,
    total: AtomicU64,
    global: TrafficStats,
    by_outbound: DashMap<String, TrafficStats>,
    by_inbound: DashMap<String, TrafficStats>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            active: DashMap::new(),
            total: AtomicU64::new(0),
            global: TrafficStats::new(),
            by_outbound: DashMap::new(),
            by_inbound: DashMap::new(),
        }
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

    pub fn record_traffic(
        &self,
        inbound_tag: &str,
        outbound_tag: &str,
        uplink: u64,
        downlink: u64,
    ) {
        self.global.add_uplink(uplink);
        self.global.add_downlink(downlink);
        self.outbound_stats(outbound_tag).add_uplink(uplink);
        self.outbound_stats(outbound_tag).add_downlink(downlink);
        self.inbound_stats(inbound_tag).add_uplink(uplink);
        self.inbound_stats(inbound_tag).add_downlink(downlink);
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
        out
    }

    pub fn track(
        &self,
        inbound_tag: &str,
        outbound_tag: &str,
        network: &str,
        source: Option<SocketAddr>,
        destination: Option<SocketAddr>,
        domain: Option<String>,
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
                started_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            },
        );
        id
    }

    pub fn connection_info(&self, id: u64) -> Option<(String, String)> {
        self.active
            .get(&id)
            .map(|c| (c.inbound_tag.clone(), c.outbound_tag.clone()))
    }

    pub fn untrack(&self, id: u64) {
        self.active.remove(&id);
    }

    pub fn active_count(&self) -> usize {
        self.active.len()
    }

    pub fn total_count(&self) -> u64 {
        self.total.load(Ordering::Relaxed)
    }

    pub fn list(&self) -> Vec<ConnectionInfo> {
        self.active.iter().map(|e| e.value().clone()).collect()
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}

pub type SharedConnectionManager = Arc<ConnectionManager>;
