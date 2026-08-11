use crate::{
    tracked_stream, Metadata, OutboundManager, ProxyConn, ProxyUdpSocket, Router,
    SharedConnectionManager,
};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

/// Late-bound outbound manager (selectors resolve children after the full graph is built).
pub struct SharedOutboundManager {
    inner: RwLock<Option<Arc<OutboundManager>>>,
}

impl SharedOutboundManager {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }

    pub fn set(&self, manager: Arc<OutboundManager>) {
        *self.inner.write().expect("shared outbound manager lock") = Some(manager);
    }

    pub fn get(&self) -> Result<Arc<OutboundManager>> {
        self.inner
            .read()
            .expect("shared outbound manager lock")
            .clone()
            .context("outbound manager not initialized")
    }
}

impl Default for SharedOutboundManager {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Dialer {
    manager: Arc<OutboundManager>,
    router: Arc<dyn Router>,
    connections: SharedConnectionManager,
}

impl Dialer {
    pub fn new(
        manager: Arc<OutboundManager>,
        router: Arc<dyn Router>,
        connections: SharedConnectionManager,
    ) -> Self {
        Self {
            manager,
            router,
            connections,
        }
    }

    pub fn connections(&self) -> SharedConnectionManager {
        self.connections.clone()
    }

    pub async fn dial_tcp(
        &self,
        metadata: &Metadata,
        destination: SocketAddr,
    ) -> Result<ProxyConn> {
        let tag = self.router.route(metadata).await?;
        let conn_id = self.connections.track(
            &metadata.inbound_tag,
            &tag,
            "tcp",
            metadata.source,
            Some(destination),
            metadata.domain.clone(),
            metadata.user.clone(),
        );
        let mut result = self
            .manager
            .get(&tag)?
            .dial_tcp(destination, metadata.domain.as_deref())
            .await
            .with_context(|| format!("dial via outbound `{tag}`"));
        if result.is_err() {
            self.connections.untrack(conn_id);
        } else {
            result = result.map(|conn| tracked_stream(conn, self.connections.clone(), conn_id));
        }
        result
    }

    /// 仅做路由选择（不拨号），供 CONNECT 在 DNS 前判断是否 direct。
    pub async fn route_tag(&self, metadata: &Metadata) -> Result<String> {
        self.router.route(metadata).await
    }

    pub fn is_direct_outbound(&self, tag: &str) -> bool {
        self.manager
            .get(tag)
            .map(|o| o.kind() == rsb_constant::TYPE_DIRECT)
            .unwrap_or(false)
    }

    pub async fn dial_udp(
        &self,
        metadata: &Metadata,
        destination: SocketAddr,
    ) -> Result<ProxyUdpSocket> {
        let tag = self.router.route(metadata).await?;
        self.manager
            .get(&tag)?
            .dial_udp(destination)
            .await
            .with_context(|| format!("dial udp via outbound `{tag}`"))
    }
}
