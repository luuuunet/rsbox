//! TLS-fronted and legacy protocol outbounds.

use crate::transport;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{proxy_box, tcp_stream, BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;

macro_rules! tls_tunnel_outbound {
    ($name:ident, $kind:expr) => {
        pub struct $name {
            tag: String,
            server: String,
            port: u16,
            password: String,
            tls: Option<Value>,
            sni: Option<String>,
        }

        impl $name {
            pub fn new(tag: String, raw: Value) -> Result<Self> {
                Ok(Self {
                    tag,
                    server: raw
                        .get("server")
                        .and_then(|v| v.as_str())
                        .context("server")?
                        .to_string(),
                    port: raw
                        .get("server_port")
                        .and_then(|v| v.as_u64())
                        .context("port")? as u16,
                    password: raw
                        .get("password")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    tls: raw.get("tls").cloned(),
                    sni: raw
                        .get("tls")
                        .and_then(|t| t.get("server_name"))
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                })
            }

            async fn connect(&self, destination: SocketAddr) -> Result<ProxyConn> {
                let mut tls = transport::tls_connect(
                    &self.server,
                    self.port,
                    self.tls.as_ref(),
                    self.sni.as_deref(),
                )
                .await?;
                let header = format!(
                    "{}\r\n{}:{}\r\n",
                    self.password,
                    destination.ip(),
                    destination.port()
                );
                tls.write_all(header.as_bytes()).await?;
                Ok(proxy_box(tls))
            }

            async fn connect_tunnel(&self) -> Result<ProxyConn> {
                let mut tls = transport::tls_connect(
                    &self.server,
                    self.port,
                    self.tls.as_ref(),
                    self.sni.as_deref(),
                )
                .await?;
                tls.write_all(format!("{}\r\n", self.password).as_bytes())
                    .await?;
                Ok(proxy_box(tls))
            }
        }

        #[async_trait]
        impl Outbound for $name {
            fn tag(&self) -> &str {
                &self.tag
            }
            fn kind(&self) -> &str {
                $kind
            }
            fn networks(&self) -> &[Network] {
                &[Network::Tcp]
            }
            async fn dial_tcp(
                &self,
                destination: SocketAddr,
                _domain: Option<&str>,
            ) -> Result<ProxyConn, BoxError> {
                self.connect(destination).await.map_err(Into::into)
            }
            async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
                let mut tls = transport::tls_connect(
                    &self.server,
                    self.port,
                    self.tls.as_ref(),
                    self.sni.as_deref(),
                )
                .await?;
                tls.write_all(format!("{}\r\n", self.password).as_bytes())
                    .await?;
                Ok(crate::udp_over_tcp::tunneled_udp(tls).await)
            }
            async fn close(&self) -> Result<(), BoxError> {
                Ok(())
            }
        }
    };
}

tls_tunnel_outbound!(NaiveOutbound, rsb_constant::TYPE_NAIVE);

#[cfg(feature = "desktop")]
pub struct SshOutbound {
    tag: String,
    pool: std::sync::Arc<crate::ssh_client::SshSessionPool>,
}

#[cfg(feature = "desktop")]
impl SshOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let host_keys = raw
            .get("host_key")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let config = crate::ssh_client::SshConfig {
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("ssh server")?
                .to_string(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .unwrap_or(22) as u16,
            username: raw
                .get("user")
                .and_then(|v| v.as_str())
                .unwrap_or("root")
                .to_string(),
            password: raw
                .get("password")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            private_key: raw
                .get("private_key")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .or_else(|| {
                    raw.get("private_key")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                }),
            private_key_path: raw
                .get("private_key_path")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            private_key_passphrase: raw
                .get("private_key_passphrase")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            host_keys,
        };
        Ok(Self {
            tag,
            pool: crate::ssh_client::pool_for(config),
        })
    }
}

#[cfg(feature = "desktop")]
#[async_trait]
impl Outbound for SshOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_SSH
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp]
    }
    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        _domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        self.pool.dial_tcp(destination, None).await
    }
    async fn dial_udp(&self, _: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        anyhow::bail!("ssh udp not supported")
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

pub struct TorOutbound {
    tag: String,
    server: String,
    port: u16,
}

impl TorOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        Ok(Self {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .unwrap_or("127.0.0.1")
                .to_string(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .unwrap_or(9050) as u16,
        })
    }
}

#[async_trait]
impl Outbound for TorOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_TOR
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp]
    }
    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        _domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        let mut stream = transport::tcp_connect(&self.server, self.port).await?;
        crate::socks::socks::socks5_connect(&mut stream, destination, None, None).await?;
        Ok(tcp_stream(stream))
    }
    async fn dial_udp(&self, _: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        anyhow::bail!("tor udp not supported")
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}
