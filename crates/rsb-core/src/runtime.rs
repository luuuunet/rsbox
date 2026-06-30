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
