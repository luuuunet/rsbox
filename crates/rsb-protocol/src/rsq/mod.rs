mod auth;
mod bandwidth;
mod brutal;
mod cert;
mod client;
mod control;
mod obfs;
mod obfs_socket;
mod protocol;
mod quic;
mod relay;
mod server;
mod share;
mod stream;
mod traffic;
mod udp_client;
mod udp_demux;
mod udp_fragment;

#[cfg(test)]
mod e2e;

pub use cert::write_dev_certs;
pub use client::RsqOutbound;
pub use share::RsqShareLink;

use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound};
use serde::Deserialize;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
struct RsqTls {
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
struct RsqUser {
    password: String,
}

fn parse_obfs(raw: &Value, users: &[RsqUser]) -> Result<Option<Arc<obfs::RsqObfs>>> {
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
        anyhow::bail!("rsq inbound: obfs.password required when multiple users are configured");
    } else {
        users[0].password.clone()
    };
    Ok(Some(Arc::new(obfs::RsqObfs::with_version(&password, version))))
}

pub struct RsqInbound {
    tag: String,
    config: server::RsqServerConfig,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl RsqInbound {
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
            .context("rsq inbound: listen_port required")? as u16;
        let tls: RsqTls = serde_json::from_value(
            raw.get("tls")
                .cloned()
                .unwrap_or(Value::Object(Default::default())),
        )?;
        if !tls.enabled {
            anyhow::bail!("rsq inbound: tls.enabled is required");
        }
        let cert = tls
            .certificate_path
            .or(tls.certificate)
            .context("rsq inbound: tls certificate_path required")?;
        let key = tls
            .key_path
            .or(tls.key)
            .context("rsq inbound: tls key_path required")?;
        let mut users: Vec<RsqUser> = raw
            .get("users")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()?
            .unwrap_or_default();
        if users.is_empty() {
            if let Some(password) = raw.get("password").and_then(|v| v.as_str()) {
                users.push(RsqUser {
                    password: password.to_string(),
                });
            }
        }
        if users.is_empty() {
            anyhow::bail!("rsq inbound: users or password required");
        }
        let obfs = parse_obfs(&raw, &users)?;
        let listen_addr: SocketAddr = format!("{listen}:{port}").parse()?;
        Ok(Self {
            tag: tag.clone(),
            config: server::RsqServerConfig {
                listen: listen_addr,
                inbound_tag: tag,
                cert_path: cert,
                key_path: key,
                passwords: users.into_iter().map(|u| u.password).collect(),
                up_mbps: raw.get("up_mbps").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                down_mbps: raw.get("down_mbps").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                udp: raw.get("udp").and_then(|v| v.as_bool()).unwrap_or(true),
                obfs,
                connections,
            },
            handle: Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for RsqInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_RSQ
    }
    async fn start(&self) -> Result<(), BoxError> {
        let cfg = Arc::new(self.config.clone());
        let handle = tokio::spawn(async move {
            if let Err(err) = server::run(cfg).await {
                tracing::error!(error = %err, "rsq server exited");
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
