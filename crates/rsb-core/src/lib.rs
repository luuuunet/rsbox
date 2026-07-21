use async_trait::async_trait;
use std::net::SocketAddr;

pub mod connection_manager;
pub mod interface;
pub mod platform;
pub mod process;
pub mod proxy_stream;
pub mod rate_limit;
pub mod runtime;
pub mod subscription;
pub mod udp;
pub mod user_registry;

pub use connection_manager::{
    ConnectionInfo, ConnectionManager, SharedConnectionManager, TrafficStats, UserSessionGuard,
};
pub use rate_limit::RateLimiter;
pub use subscription::{merge_outbound_providers, Subscription};
pub use user_registry::{trojan_password_hash, UserLimits, UserRecord, UserRegistry};
pub use interface::{detect_default_interface, is_private_ip, tcp_connect_via, udp_bind_via};
pub use platform::{install_routes, route_add, route_delete};
pub use process::{lookup_process_for_tcp_stream, lookup_process_for_tuple, ProcessInfo};
pub use proxy_stream::{
    proxy_box, tcp_stream, tracked_stream, ProxyConn, ProxyStream, SplitProxy, TrackedStream,
};
pub use runtime::{Dialer, SharedOutboundManager};
pub use udp::{ProxyUdpIo, ProxyUdpSocket, TokioUdpAdapter};

#[derive(Debug, Clone, Default)]
pub struct Metadata {
    pub network: Network,
    pub source: Option<SocketAddr>,
    pub destination: Option<SocketAddr>,
    pub domain: Option<String>,
    pub protocol: Option<String>,
    pub process_name: Option<String>,
    pub process_path: Option<String>,
    pub inbound_tag: String,
    pub inbound_type: String,
    /// Panel user name / email for per-user traffic stats (v2ray `user>>>name>>>...`).
    pub user: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Network {
    #[default]
    Tcp,
    Udp,
}

pub type BoxError = anyhow::Error;

#[async_trait]
pub trait Inbound: Send + Sync {
    fn tag(&self) -> &str;
    fn kind(&self) -> &str;
    async fn start(&self) -> Result<(), BoxError>;
    async fn close(&self) -> Result<(), BoxError>;
}

#[async_trait]
pub trait Outbound: Send + Sync {
    fn tag(&self) -> &str;
    fn kind(&self) -> &str;
    fn networks(&self) -> &[Network];
    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError>;
    async fn dial_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError>;
    async fn close(&self) -> Result<(), BoxError>;
}

#[async_trait]
pub trait Router: Send + Sync {
    async fn route(&self, metadata: &Metadata) -> Result<String, BoxError>;
}

pub struct InboundManager {
    inbounds: Vec<Box<dyn Inbound>>,
}

impl InboundManager {
    pub fn new(inbounds: Vec<Box<dyn Inbound>>) -> Self {
        Self { inbounds }
    }

    pub fn inbounds(&self) -> &[Box<dyn Inbound>] {
        &self.inbounds
    }

    pub async fn start_all(&self) -> Result<(), BoxError> {
        for inbound in &self.inbounds {
            inbound.start().await?;
        }
        Ok(())
    }

    pub async fn close_all(&self) -> Result<(), BoxError> {
        for inbound in &self.inbounds {
            inbound.close().await?;
        }
        Ok(())
    }
}

pub struct OutboundManager {
    outbounds: Vec<Box<dyn Outbound>>,
    default_tag: String,
    index: std::collections::HashMap<String, usize>,
}

impl OutboundManager {
    pub fn new(outbounds: Vec<Box<dyn Outbound>>, default_tag: String) -> Result<Self, BoxError> {
        let mut index = std::collections::HashMap::new();
        for (i, ob) in outbounds.iter().enumerate() {
            index.insert(ob.tag().to_string(), i);
        }
        if !index.contains_key(&default_tag) {
            anyhow::bail!("default outbound tag not found: {default_tag}");
        }
        Ok(Self {
            outbounds,
            default_tag,
            index,
        })
    }

    pub fn get(&self, tag: &str) -> Result<&dyn Outbound, BoxError> {
        let idx = self
            .index
            .get(tag)
            .with_context(|| format!("outbound not found: {tag}"))?;
        Ok(self.outbounds[*idx].as_ref())
    }

    pub fn default(&self) -> Result<&dyn Outbound, BoxError> {
        self.get(&self.default_tag)
    }

    pub fn tags(&self) -> Vec<String> {
        self.outbounds.iter().map(|o| o.tag().to_string()).collect()
    }

    pub fn outbound_kinds(&self) -> Vec<(String, String)> {
        self.outbounds
            .iter()
            .map(|o| (o.tag().to_string(), o.kind().to_string()))
            .collect()
    }

    pub async fn close_all(&self) -> Result<(), BoxError> {
        for outbound in &self.outbounds {
            outbound.close().await?;
        }
        Ok(())
    }
}

use anyhow::Context;
