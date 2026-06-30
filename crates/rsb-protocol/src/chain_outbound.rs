//! Chain outbound — route through the last configured proxy in the chain.
//!
//! Full multi-hop chaining requires each outbound's `detour` field (sing-box style).
//! Until detour dialing is wired, the last tag in `outbounds` handles the connection.

use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket, SharedOutboundManager};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct ChainOutbound {
    tag: String,
    shared: Arc<SharedOutboundManager>,
    outbound_tags: Vec<String>,
}

impl ChainOutbound {
    pub fn new(tag: String, raw: Value, shared: Arc<SharedOutboundManager>) -> Result<Self> {
        let outbound_tags = raw
            .get("outbounds")
            .and_then(|v| v.as_array())
            .context("chain: outbounds array required")?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect::<Vec<_>>();
        if outbound_tags.is_empty() {
            anyhow::bail!("chain: at least one outbound tag required");
        }
        if outbound_tags.len() > 1 {
            tracing::warn!(
                tag = %tag,
                hops = outbound_tags.len(),
                "chain: multi-hop detour not implemented; using last outbound only"
            );
        }
        Ok(Self {
            tag,
            shared,
            outbound_tags,
        })
    }

    fn last_tag(&self) -> Result<&str> {
        self.outbound_tags
            .last()
            .map(String::as_str)
            .context("chain: empty outbounds")
    }
}

#[async_trait]
impl Outbound for ChainOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        rsb_constant::TYPE_CHAIN
    }

    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }

    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        let tag = self.last_tag()?;
        self.shared.get()?.get(tag)?.dial_tcp(destination, domain).await
    }

    async fn dial_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let tag = self.last_tag()?;
        self.shared.get()?.get(tag)?.dial_udp(destination).await
    }

    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}
