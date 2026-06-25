// TUIC 出站实现
use anyhow::{Context, Result};
use async_trait::async_trait;
use quinn::{ClientConfig, Connection, Endpoint};
use rsb_core::{BoxError, Network, Outbound, ProxyConn};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct TuicOutbound {
    tag: String,
    server: String,
    port: u16,
    uuid: String,
    password: String,
    congestion_control: String,
    udp_relay_mode: String,
    zero_rtt_handshake: bool,
    heartbeat: u64,
    connection: Arc<Mutex<Option<Connection>>>,
}

impl TuicOutbound {
    pub fn parse(tag: String, config: &serde_json::Value) -> Result<Self> {
        Ok(Self {
            tag,
            server: config["server"]
                .as_str()
                .context("server required")?
                .to_string(),
            port: config["server_port"].as_u64().context("port required")? as u16,
            uuid: config["uuid"]
                .as_str()
                .context("uuid required")?
                .to_string(),
            password: config["password"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            congestion_control: config["congestion_control"]
                .as_str()
                .unwrap_or("cubic")
                .to_string(),
            udp_relay_mode: config["udp_relay_mode"]
                .as_str()
                .unwrap_or("native")
                .to_string(),
            zero_rtt_handshake: config["zero_rtt_handshake"].as_bool().unwrap_or(false),
            heartbeat: config["heartbeat"].as_u64().unwrap_or(10000),
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
            "Connecting to TUIC server"
        );

        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();

        let mut client_config = rustls::ClientConfig::builder()
            .with_root_certificates({
                let mut roots = rustls::RootCertStore::empty();
                for cert in rustls_native_certs::load_native_certs()? {
                    roots.add(cert).ok();
                }
                roots
            })
            .with_no_client_auth();

        client_config.alpn_protocols = vec![b"h3".to_vec()];

        let mut transport_config = quinn::TransportConfig::default();
        transport_config.max_concurrent_bidi_streams(100u32.into());
        transport_config.max_concurrent_uni_streams(100u32.into());

        let mut client_config = ClientConfig::new(Arc::new(client_config));
        client_config.transport_config(Arc::new(transport_config));

        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_config);

        let addr: SocketAddr = format!("{}:{}", self.server, self.port).parse()?;
        let conn = endpoint
            .connect(addr, &self.server)?
            .await
            .context("Failed to connect")?;

        // TUIC 认证
        self.authenticate(&conn).await?;

        tracing::info!("TUIC connection established");
        Ok(conn)
    }

    async fn authenticate(&self, conn: &Connection) -> Result<()> {
        // TUIC 协议认证
        let (mut send, mut recv) = conn.open_bi().await?;

        // 发送认证信息
        let auth_data = format!("{}:{}", self.uuid, self.password);
        send.write_all(auth_data.as_bytes()).await?;
        send.finish().await?;

        // 读取响应
        let mut buf = vec![0u8; 1];
        recv.read_exact(&mut buf).await?;

        if buf[0] != 0x00 {
            anyhow::bail!("TUIC authentication failed");
        }

        tracing::debug!("TUIC authentication successful");
        Ok(())
    }
}

#[async_trait]
impl Outbound for TuicOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        "tuic"
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

        // TUIC 协议：发送目标地址
        let addr_bytes = format!("{}", destination).into_bytes();
        send.write_all(&addr_bytes).await?;

        Ok(Box::new(TuicStream { send, recv }))
    }

    async fn dial_udp(&self) -> Result<rsb_core::ProxyUdpSocket, BoxError> {
        // TUIC UDP 支持
        todo!("TUIC UDP not implemented yet")
    }
}

struct TuicStream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

impl AsyncRead for TuicStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        use std::pin::Pin;
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for TuicStream {
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
