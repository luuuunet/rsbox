//! ShadowTLS inbound (v2/v3).

use crate::build_context::BuildContext;
use crate::direct::parse_listen;
use crate::shadowtls::server::{serve_v3, V3ServerConfig};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound};
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub struct ShadowTlsInbound {
    tag: String,
    listen: SocketAddr,
    version: u8,
    v3: Option<V3ServerConfig>,
    connections: rsb_core::SharedConnectionManager,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ShadowTlsInbound {
    pub fn new(
        tag: String,
        raw: Value,
        inbound_addrs: &HashMap<String, SocketAddr>,
        connections: rsb_core::SharedConnectionManager,
    ) -> Result<Self> {
        let listen = parse_listen(&raw)?;
        let version = raw.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u8;
        let v3 = if version == 3 {
            Some(parse_v3_config(&raw, inbound_addrs)?)
        } else {
            anyhow::bail!("shadowtls inbound: only version 3 is supported natively (use sing-box sidecar for v1/v2)");
        };
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen,
            version,
            v3,
            connections,
            shutdown,
            handle: Mutex::new(None),
        })
    }

    pub fn new_with_context(
        tag: String,
        raw: Value,
        ctx: &BuildContext,
        connections: rsb_core::SharedConnectionManager,
    ) -> Result<Self> {
        Self::new(tag, raw, &ctx.inbound_listen_by_tag, connections)
    }
}

fn parse_v3_config(raw: &Value, inbound_addrs: &HashMap<String, SocketAddr>) -> Result<V3ServerConfig> {
    let mut users = Vec::new();
    if let Some(arr) = raw.get("users").and_then(|v| v.as_array()) {
        for u in arr {
            let name = u
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("user")
                .to_string();
            let password = u
                .get("password")
                .and_then(|v| v.as_str())
                .context("shadowtls: user password")?
                .to_string();
            users.push((name, password));
        }
    }
    if users.is_empty() {
        let password = raw
            .get("password")
            .and_then(|v| v.as_str())
            .context("shadowtls inbound: password/users required")?
            .to_string();
        users.push(("default".into(), password));
    }
    let hs = raw.get("handshake").context("shadowtls inbound: handshake required")?;
    let handshake_server = hs
        .get("server")
        .and_then(|v| v.as_str())
        .context("shadowtls inbound: handshake.server")?
        .to_string();
    let handshake_port = hs
        .get("server_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(443) as u16;
    let strict_mode = raw
        .get("strict_mode")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let detour_tag = raw
        .get("detour")
        .and_then(|v| v.as_str())
        .context("shadowtls inbound: detour tag required")?;
    let detour = inbound_addrs
        .get(detour_tag)
        .copied()
        .with_context(|| format!("shadowtls inbound: detour inbound '{detour_tag}' not found"))?;
    Ok(V3ServerConfig {
        users,
        handshake_server,
        handshake_port,
        strict_mode,
        detour,
    })
}

#[async_trait]
impl Inbound for ShadowTlsInbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        rsb_constant::TYPE_SHADOWTLS
    }

    async fn start(&self) -> Result<(), BoxError> {
        if self.version != 3 {
            anyhow::bail!("shadowtls inbound: unsupported version {}", self.version);
        }
        let cfg = self.v3.clone().context("shadowtls v3 config")?;
        let connections = self.connections.clone();
        let inbound_tag = self.tag.clone();
        let listener = TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, "shadowtls v3 inbound listening");
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    accept = listener.accept() => {
                        let Ok((stream, _)) = accept else { break };
                        let cfg = cfg.clone();
                        let connections = connections.clone();
                        let inbound_tag = inbound_tag.clone();
                        tokio::spawn(async move {
                            if let Err(err) =
                                serve_v3(stream, cfg, connections, inbound_tag).await
                            {
                                tracing::warn!(error = %err, "shadowtls client failed");
                            }
                        });
                    }
                }
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }

    async fn close(&self) -> Result<(), BoxError> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}
