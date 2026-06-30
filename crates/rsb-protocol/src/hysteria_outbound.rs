// Hysteria v1 出站实现
use anyhow::{Context, Result};
use async_trait::async_trait;
use quinn::{ClientConfig, Connection, Endpoint, WriteError};
use rsb_core::{proxy_box, BoxError, Network, Outbound, ProxyConn};
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
    pub fn new(tag: String, raw: serde_json::Value) -> Result<Self> {
        let tls = raw.get("tls");
        let auth_str = raw
            .get("auth_str")
            .and_then(|v| v.as_str())
            .or_else(|| raw.get("auth").and_then(|v| v.as_str()))
            .map(str::to_string);
        Ok(Self {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("hysteria: server required")?
                .to_string(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("hysteria: server_port required")? as u16,
            auth_str,
            obfs: raw
                .get("obfs")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            up_mbps: raw.get("up_mbps").and_then(|v| v.as_u64()).unwrap_or(10) as u32,
            down_mbps: raw.get("down_mbps").and_then(|v| v.as_u64()).unwrap_or(10) as u32,
            sni: tls
                .and_then(|t| t.get("server_name"))
                .and_then(|v| v.as_str())
                .or_else(|| raw.get("sni").and_then(|v| v.as_str()))
                .map(str::to_string),
            insecure: tls
                .and_then(|t| t.get("insecure"))
                .and_then(|v| v.as_bool())
                .or_else(|| raw.get("insecure").and_then(|v| v.as_bool()))
                .unwrap_or(false),
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

        let tls_cfg = if self.insecure {
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::transport::SkipVerifier))
                .with_no_client_auth()
        } else {
            rustls::ClientConfig::builder()
                .with_root_certificates({
                    let mut roots = rustls::RootCertStore::empty();
                    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                    roots
                })
                .with_no_client_auth()
        };
        let mut tls_cfg = tls_cfg;
        tls_cfg.alpn_protocols = vec![b"hysteria".to_vec()];

        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_concurrent_bidi_streams(100u32.into());
        transport_config.max_concurrent_uni_streams(100u32.into());
        transport_config.max_idle_timeout(Some(std::time::Duration::from_secs(30).try_into()?));

        let mut client_config = ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(tls_cfg)?,
        ));
        client_config.transport_config(Arc::new(transport_config));

        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);

        let sni = self.sni.as_deref().unwrap_or(&self.server);
        let addr = tokio::net::lookup_host(format!("{}:{}", self.server, self.port))
            .await
            .context("resolve hysteria server")?
            .next()
            .context("no hysteria server address")?;

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
        rsb_constant::TYPE_HYSTERIA
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

        Ok(proxy_box(HysteriaStream { send, recv }))
    }

    async fn dial_udp(&self, _destination: SocketAddr) -> Result<rsb_core::ProxyUdpSocket, BoxError> {
        let conn = self.get_connection().await?;
        let (mut send, recv) = conn.open_bi().await?;
        let header = b"UDP\n";
        send.write_all(header).await?;
        Ok(crate::udp_over_tcp::tunneled_udp(HysteriaStream { send, recv }).await)
    }

    async fn close(&self) -> Result<(), BoxError> {
        if let Some(conn) = self.connection.lock().await.take() {
            conn.close(0u32.into(), b"shutdown");
        }
        Ok(())
    }
}

struct HysteriaStream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};

fn map_write_err(e: WriteError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e)
}

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
        use std::task::Poll;
        match Pin::new(&mut self.send).poll_write(cx, buf) {
            Poll::Ready(Ok(n)) => Poll::Ready(Ok(n)),
            Poll::Ready(Err(e)) => Poll::Ready(Err(map_write_err(e))),
            Poll::Pending => Poll::Pending,
        }
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
