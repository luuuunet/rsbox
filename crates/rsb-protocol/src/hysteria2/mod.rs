mod auth;
mod client;
mod obfs;
mod obfs_socket;
mod protocol;
mod relay;
mod server;
mod udp_client;

pub use client::Hysteria2Outbound;

use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound};
use serde::Deserialize;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Deserialize)]
struct Hy2Tls {
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
struct Hy2User {
    password: String,
}

pub struct Hysteria2Inbound {
    tag: String,
    config: server::Hy2ServerConfig,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl Hysteria2Inbound {
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
            .context("hysteria2 inbound: listen_port required")? as u16;
        let tls: Hy2Tls = serde_json::from_value(
            raw.get("tls")
                .cloned()
                .unwrap_or(Value::Object(Default::default())),
        )?;
        if !tls.enabled {
            anyhow::bail!("hysteria2 inbound: tls.enabled is required");
        }
        let cert = tls
            .certificate_path
            .or(tls.certificate)
            .context("hysteria2 inbound: tls certificate_path required")?;
        let key = tls
            .key_path
            .or(tls.key)
            .context("hysteria2 inbound: tls key_path required")?;
        let mut users: Vec<Hy2User> = raw
            .get("users")
            .map(|v| serde_json::from_value(v.clone()))
            .transpose()?
            .unwrap_or_default();
        if users.is_empty() {
            if let Some(password) = raw.get("password").and_then(|v| v.as_str()) {
                users.push(Hy2User {
                    password: password.to_string(),
                });
            }
        }
        if users.is_empty() {
            anyhow::bail!("hysteria2 inbound: users or password required");
        }
        let up_mbps = raw.get("up_mbps").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let down_mbps = raw.get("down_mbps").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let obfs_password = raw
            .get("obfs")
            .and_then(|o| o.get("password"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let listen_addr: SocketAddr = format!("{listen}:{port}").parse()?;
        Ok(Self {
            tag: tag.clone(),
            config: server::Hy2ServerConfig {
                listen: listen_addr,
                inbound_tag: tag,
                cert_path: cert,
                key_path: key,
                passwords: users.into_iter().map(|u| u.password).collect(),
                up_mbps,
                down_mbps,
                udp: raw.get("udp").and_then(|v| v.as_bool()).unwrap_or(true),
                obfs_password,
                connections,
            },
            handle: Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for Hysteria2Inbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_HYSTERIA2
    }
    async fn start(&self) -> Result<(), BoxError> {
        let cfg = Arc::new(self.config.clone());
        let handle = tokio::spawn(async move {
            if let Err(err) = server::run(cfg).await {
                tracing::error!(error = %err, "hysteria2 server exited");
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }
    async fn close(&self) -> Result<(), BoxError> {
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}
