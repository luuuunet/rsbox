use crate::transport::{self};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct HttpOutbound {
    tag: String,
    server: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    tls: Option<Value>,
}

impl HttpOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        Ok(Self {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("http outbound: server required")?
                .to_string(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("http outbound: server_port required")? as u16,
            username: raw
                .get("username")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            password: raw
                .get("password")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            tls: raw.get("tls").cloned(),
        })
    }
}

#[async_trait]
impl Outbound for HttpOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_HTTP
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp]
    }
    async fn dial_tcp(&self, destination: SocketAddr, _domain: Option<&str>) -> Result<ProxyConn, BoxError> {
        let use_tls = self
            .tls
            .as_ref()
            .map(|t| t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false))
            .unwrap_or(false);
        let mut stream: Box<dyn rsb_core::ProxyStream> = if use_tls {
            Box::new(
                transport::tls_connect(&self.server, self.port, self.tls.as_ref(), None).await?,
            )
        } else {
            Box::new(transport::tcp_connect(&self.server, self.port).await?)
        };
        let target = match destination {
            SocketAddr::V4(v4) => format!("{}:{}", v4.ip(), v4.port()),
            SocketAddr::V6(v6) => format!("[{}]:{}", v6.ip(), v6.port()),
        };
        let mut req = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n");
        if let (Some(u), Some(p)) = (&self.username, &self.password) {
            use base64::{engine::general_purpose::STANDARD, Engine};
            let auth = STANDARD.encode(format!("{u}:{p}"));
            req.push_str(&format!("Proxy-Authorization: Basic {auth}\r\n"));
        }
        req.push_str("\r\n");
        stream.write_all(req.as_bytes()).await?;
        let mut resp = vec![0u8; 1024];
        let n = stream.read(&mut resp).await?;
        let text = std::str::from_utf8(&resp[..n])?;
        if !text.contains("200") {
            anyhow::bail!("http proxy connect failed: {text}");
        }
        Ok(stream)
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        anyhow::bail!("http outbound does not support udp")
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}
