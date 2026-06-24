//! sing-box endpoints (wireguard, tailscale).

use anyhow::{Context, Result};
use serde_json::Value;
use tracing::info;

#[cfg(not(feature = "wireguard-tunnel"))]
use tokio::net::UdpSocket;

pub struct WireGuardEndpoint {
    tag: String,
    raw: Value,
    #[cfg(feature = "wireguard-tunnel")]
    tunnel: rsb_wireguard::WireGuardTunnel,
    #[cfg(not(feature = "wireguard-tunnel"))]
    stub: WireGuardStub,
}

#[cfg(not(feature = "wireguard-tunnel"))]
struct WireGuardStub {
    tag: String,
    listen_port: u16,
    peers: usize,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl WireGuardEndpoint {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        raw.get("private_key")
            .and_then(|v| v.as_str())
            .context("wireguard: private_key required")?;
        Ok(Self {
            tag: tag.clone(),
            raw: raw.clone(),
            #[cfg(feature = "wireguard-tunnel")]
            tunnel: rsb_wireguard::WireGuardTunnel::new(tag),
            #[cfg(not(feature = "wireguard-tunnel"))]
            stub: WireGuardStub::new(tag, &raw)?,
        })
    }

    pub async fn start(&self) -> Result<()> {
        #[cfg(feature = "wireguard-tunnel")]
        {
            return self.tunnel.start(self.raw.clone()).await;
        }
        #[cfg(not(feature = "wireguard-tunnel"))]
        {
            self.stub.start().await
        }
    }

    pub async fn close(&self) -> Result<()> {
        #[cfg(feature = "wireguard-tunnel")]
        {
            return self.tunnel.close().await;
        }
        #[cfg(not(feature = "wireguard-tunnel"))]
        {
            self.stub.close().await
        }
    }
}

#[cfg(not(feature = "wireguard-tunnel"))]
impl WireGuardStub {
    fn new(tag: String, raw: &Value) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let listen_port = raw
            .get("listen_port")
            .and_then(|v| v.as_u64())
            .unwrap_or(51820) as u16;
        let peers = raw
            .get("peers")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        Ok(Self {
            tag,
            listen_port,
            peers,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }

    async fn start(&self) -> Result<()> {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", self.listen_port))
            .await
            .context("wireguard udp bind")?;
        info!(
            tag = %self.tag,
            listen_port = self.listen_port,
            peers = self.peers,
            "wireguard endpoint (stub) — rebuild with `--features wireguard-tunnel` for boringtun"
        );
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            let mut buf = vec![0u8; 65535];
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    recv = socket.recv_from(&mut buf) => {
                        let Ok((n, src)) = recv else { break };
                        tracing::trace!(bytes = n, %src, "wireguard udp (no crypto without wireguard-tunnel feature)");
                    }
                }
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }

    async fn close(&self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}

pub struct TailscaleEndpoint {
    tag: String,
    raw: Value,
    #[cfg(feature = "wireguard-tunnel")]
    tunnel: tokio::sync::Mutex<Option<rsb_wireguard::WireGuardTunnel>>,
    shutdown: tokio::sync::watch::Sender<bool>,
}

impl TailscaleEndpoint {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            raw,
            #[cfg(feature = "wireguard-tunnel")]
            tunnel: tokio::sync::Mutex::new(None),
            shutdown,
        })
    }

    pub async fn start(&self) -> Result<()> {
        let wg = crate::tailscale_embedded::resolve_wireguard_config(&self.raw).await?;
        info!(
            tag = %self.tag,
            interface = wg.get("interface_name").and_then(|v| v.as_str()),
            peers = wg.get("peers").and_then(|v| v.as_array()).map(|a| a.len()),
            "tailscale endpoint (embedded wireguard)"
        );
        #[cfg(feature = "wireguard-tunnel")]
        {
            let tunnel = rsb_wireguard::WireGuardTunnel::new(self.tag.clone());
            tunnel.start(wg).await?;
            *self.tunnel.lock().await = Some(tunnel);
        }
        #[cfg(not(feature = "wireguard-tunnel"))]
        {
            tracing::warn!(
                tag = %self.tag,
                "tailscale embedded keys ready — enable `wireguard-tunnel` feature for data plane"
            );
        }
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        let _ = self.shutdown.send(true);
        #[cfg(feature = "wireguard-tunnel")]
        if let Some(tunnel) = self.tunnel.lock().await.take() {
            tunnel.close().await?;
        }
        Ok(())
    }
}

pub fn build_endpoints(options: &rsb_config::Options) -> Result<Vec<EndpointHandle>> {
    let mut out = Vec::new();
    for (i, ep) in options.endpoints.iter().enumerate() {
        let tag = ep.tag.clone().unwrap_or_else(|| format!("endpoint-{i}"));
        match ep.kind.as_str() {
            rsb_constant::TYPE_WIREGUARD => {
                out.push(EndpointHandle::WireGuard(WireGuardEndpoint::new(
                    tag,
                    ep.raw.clone(),
                )?));
            }
            rsb_constant::TYPE_TAILSCALE => {
                out.push(EndpointHandle::Tailscale(TailscaleEndpoint::new(
                    tag,
                    ep.raw.clone(),
                )?));
            }
            other => anyhow::bail!(
                "unknown endpoint type: {other} (known: {})",
                rsb_constant::ALL_ENDPOINT_TYPES.join(", ")
            ),
        }
    }
    Ok(out)
}

pub enum EndpointHandle {
    WireGuard(WireGuardEndpoint),
    Tailscale(TailscaleEndpoint),
}

impl EndpointHandle {
    pub async fn start(&self) -> Result<()> {
        match self {
            Self::WireGuard(e) => e.start().await,
            Self::Tailscale(e) => e.start().await,
        }
    }

    pub async fn close(&self) -> Result<()> {
        match self {
            Self::WireGuard(e) => e.close().await,
            Self::Tailscale(e) => e.close().await,
        }
    }
}
