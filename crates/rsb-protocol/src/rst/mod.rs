//! RST — HTTP/3 over QUIC (ALPN `h3`) with Brutal CC and RSQ-style UDP obfs.
//!
//! Auth uses HTTP/3 `POST /auth` (rst-* headers). TCP/UDP data plane follows
//! the Hy2 pattern (QUIC bidi streams + datagrams) after auth.

mod auth;
mod brutal;
mod client;
mod dest;
#[cfg(test)]
mod e2e;
mod obfs;
mod obfs_socket;
mod protocol;
mod quic;
mod relay;
mod server;
mod share;
mod traffic;
mod udp_client;

pub use client::RstOutbound;
pub use share::RstShareLink;

use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound};
use serde::Deserialize;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Deserialize)]
struct RstTls {
    #[serde(default)]
    enabled: bool,
    certificate_path: Option<String>,
    key_path: Option<String>,
    #[serde(default)]
    certificate: Option<String>,
    #[serde(default)]
    key: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RstUser {
    password: String,
}

fn parse_obfs(raw: &Value, users: &[RstUser]) -> Result<Option<Arc<obfs::RstObfs>>> {
    let obfs_cfg = match raw.get("obfs") {
        Some(v) => v,
        None => return Ok(None),
    };
    if obfs_cfg.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
        return Ok(None);
    }
    let version = obfs::ObfsVersion::parse(obfs_cfg.get("version").and_then(|v| v.as_u64()));
    let password = if let Some(pass) = obfs_cfg.get("password").and_then(|v| v.as_str()) {
        pass.to_string()
    } else if users.len() > 1 {
        anyhow::bail!("rst inbound: obfs.password required when multiple users are configured");
    } else if users.is_empty() {
        anyhow::bail!("rst inbound: obfs requires password");
    } else {
        users[0].password.clone()
    };
    Ok(Some(Arc::new(obfs::RstObfs::with_version(&password, version))))
}

pub struct RstInbound {
    tag: String,
    config: server::RstServerConfig,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl RstInbound {
    pub fn new(
        tag: String,
        raw: Value,
        connections: rsb_core::SharedConnectionManager,
    ) -> Result<Self> {
        let listen = raw
            .get("listen")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0.0");
        let port = raw
            .get("listen_port")
            .and_then(|v| v.as_u64())
            .context("rst inbound: listen_port required")? as u16;
        let tls: RstTls = serde_json::from_value(
            raw.get("tls")
                .cloned()
                .unwrap_or(Value::Object(Default::default())),
        )?;
        if !tls.enabled {
            anyhow::bail!("rst inbound: tls.enabled is required");
        }
        let cert = tls
            .certificate_path
            .or(tls.certificate)
            .context("rst inbound: tls certificate_path required")?;
        let key = tls
            .key_path
            .or(tls.key)
            .context("rst inbound: tls key_path required")?;
        let mut users: Vec<RstUser> = raw
            .get("users")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()?
            .unwrap_or_default();
        if users.is_empty() {
            if let Some(password) = raw.get("password").and_then(|v| v.as_str()) {
                users.push(RstUser {
                    password: password.to_string(),
                });
            }
        }
        if users.is_empty() {
            anyhow::bail!("rst inbound: users or password required");
        }
        let obfs = parse_obfs(&raw, &users)?;
        let listen_addr: SocketAddr = format!("{listen}:{port}").parse()?;
        Ok(Self {
            tag: tag.clone(),
            config: server::RstServerConfig {
                listen: listen_addr,
                inbound_tag: tag,
                cert_path: cert,
                key_path: key,
                passwords: users.into_iter().map(|u| u.password).collect(),
                up_mbps: raw.get("up_mbps").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                down_mbps: raw.get("down_mbps").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                udp: raw.get("udp").and_then(|v| v.as_bool()).unwrap_or(true),
                allow_private: raw
                    .get("allow_private")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                connections,
                obfs,
            },
            handle: Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for RstInbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        rsb_constant::TYPE_RST
    }

    async fn start(&self) -> Result<(), BoxError> {
        let cfg = Arc::new(self.config.clone());
        let handle = tokio::spawn(async move {
            if let Err(err) = server::run(cfg).await {
                tracing::error!(error = %err, "rst inbound exited");
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }

    async fn close(&self) -> Result<(), BoxError> {
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
            let _ = tokio::time::timeout(Duration::from_secs(3), h).await;
        }
        Ok(())
    }
}
