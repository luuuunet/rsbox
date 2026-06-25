//! WireGuard outbound — userspace tunnel + interface-bound dials.

use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{tcp_stream, BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct WireGuardOutbound {
    tag: String,
    #[cfg(feature = "wireguard-tunnel")]
    tunnel: Arc<rsb_wireguard::WireGuardTunnel>,
    interface_name: String,
    raw: Value,
    started: std::sync::Arc<tokio::sync::Mutex<bool>>,
}

impl WireGuardOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        raw.get("private_key")
            .context("wireguard outbound: private_key required")?;
        let interface_name = raw
            .get("interface_name")
            .and_then(|v| v.as_str())
            .unwrap_or("wg0")
            .to_string();
        Ok(Self {
            tag: tag.clone(),
            #[cfg(feature = "wireguard-tunnel")]
            tunnel: Arc::new(rsb_wireguard::WireGuardTunnel::new(tag)),
            interface_name,
            raw,
            started: std::sync::Arc::new(tokio::sync::Mutex::new(false)),
        })
    }

    async fn ensure_started(&self) -> Result<()> {
        let mut guard = self.started.lock().await;
        if *guard {
            return Ok(());
        }

        // Hold lock during entire start to prevent race condition
        #[cfg(feature = "wireguard-tunnel")]
        {
            self.tunnel.start(self.raw.clone()).await?;
            rsb_wireguard::install_routes(&self.raw).await.ok();
        }
        #[cfg(not(feature = "wireguard-tunnel"))]
        anyhow::bail!("wireguard outbound requires `wireguard-tunnel` feature");

        *guard = true;
        Ok(())
    }
}

#[async_trait]
impl Outbound for WireGuardOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_WIREGUARD
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(&self, destination: SocketAddr) -> Result<ProxyConn, BoxError> {
        self.ensure_started().await?;
        let stream = rsb_core::tcp_connect_via(destination, Some(&self.interface_name)).await?;
        Ok(tcp_stream(stream))
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        self.ensure_started().await?;
        let socket = rsb_core::udp_bind_via(Some(&self.interface_name)).await?;
        Ok(ProxyUdpSocket::from_tokio(socket))
    }
    async fn close(&self) -> Result<(), BoxError> {
        #[cfg(feature = "wireguard-tunnel")]
        self.tunnel.close().await.ok();
        Ok(())
    }
}
