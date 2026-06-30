//! sing-box compatible ShadowTLS outbound (v1/v2/v3 tunnel).

use crate::shadowtls::client;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket};
use serde_json::Value;
use std::net::SocketAddr;

pub struct ShadowTlsOutbound {
    tag: String,
    version: u8,
    password: String,
    server: String,
    port: u16,
    tls: Option<Value>,
    sni: Option<String>,
    strict_mode: bool,
}

impl ShadowTlsOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let tls = raw.get("tls").cloned();
        let version = raw.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u8;
        if tls.as_ref().is_none_or(|t| !t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true)) {
            anyhow::bail!("shadowtls: tls.enabled is required");
        }
        Ok(Self {
            tag,
            version,
            password: raw
                .get("password")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("shadowtls: server required")?
                .to_string(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("shadowtls: server_port required")? as u16,
            tls,
            sni: raw
                .get("tls")
                .and_then(|t| t.get("server_name"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            strict_mode: raw
                .get("strict_mode")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
    }

    fn tls_config(&self) -> Option<Value> {
        let mut tls = self.tls.clone().unwrap_or_else(|| serde_json::json!({ "enabled": true }));
        if let Some(obj) = tls.as_object_mut() {
            if self.version == 1 {
                obj.insert("min_version".into(), serde_json::json!("1.2"));
                obj.insert("max_version".into(), serde_json::json!("1.2"));
            }
            if self.version == 3 && self.strict_mode {
                obj.insert("min_version".into(), serde_json::json!("1.3"));
            }
        }
        Some(tls)
    }

    async fn open_tunnel(&self) -> Result<ProxyConn> {
        if self.version >= 2 && self.password.is_empty() {
            anyhow::bail!("shadowtls v{}/v3: password required", self.version);
        }
        client::connect(
            self.version,
            &self.server,
            self.port,
            &self.password,
            self.tls_config().as_ref(),
            self.sni.as_deref(),
            self.strict_mode,
        )
        .await
    }
}

#[async_trait]
impl Outbound for ShadowTlsOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_SHADOWTLS
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp]
    }
    async fn dial_tcp(
        &self,
        _destination: SocketAddr,
        _domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        self.open_tunnel().await.map_err(Into::into)
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        anyhow::bail!("shadowtls outbound does not support UDP")
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}
