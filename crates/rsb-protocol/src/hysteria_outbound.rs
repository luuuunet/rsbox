// Hysteria v1 出站实现
use anyhow::{Context, Result};
use async_trait::async_trait;
use quinn::{ClientConfig, Connection, Endpoint};
use rsb_core::{BoxError, Network, Outbound, ProxyConn};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct HysteriaOutbound {
    tag: String,
    server: String,
    port: u16,
    auth_str: Option<String>,
    obfs: Option<String>,
    up_mbps: u32,
    down_mbps: u32,
    sni: Option<String>,
    insecure: bool,
    connection: Arc<Mutex<Option<Connection>>>,
}

impl HysteriaOutbound {
    pub fn parse(tag: String, config: &serde_json::Value) -> Result<Self> {
        Ok(Self {
            tag,
            server: config["server"]
                .as_str()
                .context("server required")?
                .to_string(),
            port: config["server_port"].as_u64().context("port required")? as u16,
            auth_str: config["auth_str"].as_str().map(|s| s.to_string()),
            obfs: config["obfs"].as_str().map(|s| s.to_string()),
            up_mbps: config["up_mbps"].as_u64().unwrap_or(10) as u32,
            down_mbps: config["down_mbps"].as_u64().unwrap_or(10) as u32,
            sni: config["sni"].as_str().map(|s| s.to_string()),
            insecure: config["insecure"].as_bool().unwrap_or(false),
            connection: Arc::new(Mutex::new(None)),
        })
    }

    async fn get_connection(&self) -> Result<Connection> {
        let mut guard = self.connection.lock().await;

        if let Some(conn) = guard.as_ref() {
            if conn.close_reason().is_none() {
                return Ok(conn.clone());
            }
        }

        let conn = self.connect().await?;
        *guard = Some(conn.clone());
        Ok(conn)
    }

    async fn connect(&self) -> Result<Connection> {
        tracing::info!(
            server = %self.server,
            port = self.port,
            "Connecting to Hysteria v1 server"
        );

        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();

        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs()? {
            roots.add(cert).ok();
        }

        let mut client_config = if self.insecure {
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::transport::SkipVerifier))
                .with_no_client_auth()
        } else {
            rustls::ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth()
        };

        client_config.alpn_protocols = vec![b"hysteria".to_vec()];

        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_concurrent_bidi_streams(100u32.into());
        transport_config.max_concurrent_uni_streams(100u32.into());
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(30).try_into()?));

        let mut client_config = ClientConfig::new(Arc::new(client_config));
        client_config.transport_config(Arc::new(transport_config));

        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);

        let sni = self.sni.as_deref().unwrap_or(&self.server);
        let addr: SocketAddr = format!("{}:{}", self.server, self.port).parse()?;

        let conn = endpoint
            .connect(addr, sni)?
            .await
            .context("Failed to connect to Hysteria v1 server")?;

        tracing::info!("Hysteria v1 connection established");
        Ok(conn)
    }
}

#[async_trait]
impl Outbound for HysteriaOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        "hysteria"
    }

    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }

    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        _domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        let conn = self.get_connection().await?;
        let (mut send, recv) = conn.open_bi().await?;

        // Hysteria v1 协议：发送目标地址
        let addr_bytes = format!("{}", destination).into_bytes();
        send.write_all(&addr_bytes).await?;

        Ok(Box::new(HysteriaStream { send, recv }))
    }

    async fn dial_udp(&self) -> Result<rsb_core::ProxyUdpSocket, BoxError> {
        todo!("Hysteria v1 UDP not implemented yet")
    }
}

struct HysteriaStream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};

impl AsyncRead for HysteriaStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::pin::Pin;
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for HysteriaStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        use std::pin::Pin;
        Pin::new(&mut self.send).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::pin::Pin;
        Pin::new(&mut self.send).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::pin::Pin;
        Pin::new(&mut self.send).poll_shutdown(cx)
    }
}
