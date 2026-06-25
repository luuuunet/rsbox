use crate::transport;
use anyhow::{Context, Result};
use async_trait::async_trait;
use quinn::{ClientConfig, Endpoint, TransportConfig};
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket, SplitProxy};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;

/// TUIC v5 client (QUIC + password token).
pub struct TuicOutbound {
    tag: String,
    server: String,
    port: u16,
    uuid: Uuid,
    password: String,
    sni: String,
    insecure: bool,
    connection: tokio::sync::Mutex<Option<Arc<quinn::Connection>>>,
}

impl TuicOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let uuid_str = raw
            .get("uuid")
            .and_then(|v| v.as_str())
            .context("tuic: uuid required")?;
        let tls = raw.get("tls");
        let server = raw
            .get("server")
            .and_then(|v| v.as_str())
            .context("tuic: server required")?
            .to_string();
        Ok(Self {
            tag,
            server: server.clone(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("tuic: server_port required")? as u16,
            uuid: Uuid::parse_str(uuid_str)?,
            password: raw
                .get("password")
                .and_then(|v| v.as_str())
                .context("tuic: password required")?
                .to_string(),
            sni: tls
                .and_then(|t| t.get("server_name"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or(server),
            insecure: tls
                .and_then(|t| t.get("insecure"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            connection: tokio::sync::Mutex::new(None),
        })
    }

    async fn connect(&self) -> Result<Arc<quinn::Connection>> {
        let mut guard = self.connection.lock().await;
        if let Some(c) = guard.as_ref() {
            if c.close_reason().is_none() {
                return Ok(c.clone());
            }
        }
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let mut tls_cfg = rustls::ClientConfig::builder()
            .with_root_certificates({
                let mut roots = rustls::RootCertStore::empty();
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                roots
            })
            .with_no_client_auth();
        if self.insecure {
            tls_cfg
                .dangerous()
                .set_certificate_verifier(Arc::new(transport::SkipVerifier));
        }
        let mut client_cfg = ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(tls_cfg)?,
        ));
        let mut transport_cfg = TransportConfig::default();
        transport_cfg.keep_alive_interval(Some(Duration::from_secs(10)));
        client_cfg.transport_config(Arc::new(transport_cfg));
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_cfg);
        let addr = tokio::net::lookup_host(format!("{}:{}", self.server, self.port))
            .await?
            .next()
            .context("resolve tuic server")?;
        let conn = endpoint.connect(addr, &self.sni)?.await?;
        *guard = Some(Arc::new(conn.clone()));
        Ok(Arc::new(conn))
    }
}

#[async_trait]
impl Outbound for TuicOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_TUIC
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(&self, destination: SocketAddr) -> Result<ProxyConn, BoxError> {
        let conn = self.connect().await?;
        let (mut send, mut recv) = conn.open_bi().await?;
        let header = build_tuic_connect(self.uuid, &self.password, destination);
        send.write_all(&header).await?;
        let mut resp = [0u8; 1];
        recv.read_exact(&mut resp).await?;
        if resp[0] != 0 {
            anyhow::bail!("tuic connect rejected");
        }
        Ok(Box::new(SplitProxy::new(recv, send)))
    }
    async fn dial_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let conn = self.connect().await?;
        let (mut send, mut recv) = conn.open_bi().await?;
        let header = build_tuic_packet(self.uuid, &self.password, destination);
        send.write_all(&header).await?;
        let mut resp = [0u8; 1];
        recv.read_exact(&mut resp).await?;
        if resp[0] != 0 {
            anyhow::bail!("tuic udp associate rejected");
        }
        Ok(crate::udp_over_tcp::tunneled_udp(TuicBiStream {
            read: recv,
            write: send,
        })
        .await)
    }
    async fn close(&self) -> Result<(), BoxError> {
        if let Some(c) = self.connection.lock().await.take() {
            c.close(0u32.into(), b"close");
        }
        Ok(())
    }
}

fn build_tuic_connect(uuid: Uuid, password: &str, dest: SocketAddr) -> Vec<u8> {
    build_tuic_header(0x01, uuid, password, dest)
}

fn build_tuic_packet(uuid: Uuid, password: &str, dest: SocketAddr) -> Vec<u8> {
    build_tuic_header(0x03, uuid, password, dest)
}

fn build_tuic_header(cmd: u8, uuid: Uuid, password: &str, dest: SocketAddr) -> Vec<u8> {
    let mut buf = vec![cmd];
    buf.extend_from_slice(uuid.as_bytes());
    let pass = password.as_bytes();
    buf.push(pass.len() as u8);
    buf.extend_from_slice(pass);
    match dest {
        SocketAddr::V4(v4) => {
            buf.push(0x01);
            buf.extend_from_slice(&v4.ip().octets());
            buf.extend_from_slice(&v4.port().to_be_bytes());
        },
        SocketAddr::V6(v6) => {
            buf.push(0x03);
            buf.extend_from_slice(&v6.ip().octets());
            buf.extend_from_slice(&v6.port().to_be_bytes());
        },
    }
    buf
}

struct TuicBiStream {
    read: quinn::RecvStream,
    write: quinn::SendStream,
}

impl tokio::io::AsyncRead for TuicBiStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.read).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for TuicBiStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.write)
            .poll_write(cx, buf)
            .map_err(std::io::Error::other)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.write)
            .poll_flush(cx)
            .map_err(std::io::Error::other)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.write)
            .poll_shutdown(cx)
            .map_err(std::io::Error::other)
    }
}

use rsb_core::Inbound;

pub struct TuicInbound {
    tag: String,
    listen: SocketAddr,
    users: Vec<(Uuid, String)>,
    cert: String,
    key: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl TuicInbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let listen = crate::direct::parse_listen(&raw)?;
        let mut users = Vec::new();
        if let Some(arr) = raw.get("users").and_then(|v| v.as_array()) {
            for u in arr {
                let id = u
                    .get("uuid")
                    .and_then(|v| v.as_str())
                    .context("tuic user uuid")?;
                let pass = u
                    .get("password")
                    .and_then(|v| v.as_str())
                    .context("tuic user password")?;
                users.push((Uuid::parse_str(id)?, pass.to_string()));
            }
        }
        anyhow::ensure!(!users.is_empty(), "tuic inbound: users required");
        let tls = raw.get("tls").context("tuic inbound: tls required")?;
        let cert = tls
            .get("certificate_path")
            .or_else(|| tls.get("certificate"))
            .and_then(|v| v.as_str())
            .context("tuic cert")?
            .to_string();
        let key = tls
            .get("key_path")
            .or_else(|| tls.get("key"))
            .and_then(|v| v.as_str())
            .context("tuic key")?
            .to_string();
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen,
            users,
            cert,
            key,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for TuicInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_TUIC
    }
    async fn start(&self) -> Result<(), BoxError> {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let server_config = build_tuic_server_config(&self.cert, &self.key)?;
        let endpoint = quinn::Endpoint::server(server_config, self.listen)
            .map_err(|e| anyhow::anyhow!("tuic endpoint: {e}"))?;
        tracing::info!(tag = %self.tag, %self.listen, users = self.users.len(), "tuic inbound listening");
        let users = self.users.clone();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    incoming = endpoint.accept() => {
                        let Some(incoming) = incoming else { break };
                        let users = users.clone();
                        tokio::spawn(async move {
                            if let Ok(conn) = incoming.await {
                                if let Err(err) = serve_tuic_connection(conn, users).await {
                                    tracing::debug!(error = %err, "tuic session ended");
                                }
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

fn build_tuic_server_config(cert: &str, key: &str) -> Result<quinn::ServerConfig> {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use std::fs::File;
    use std::io::BufReader;
    let cert_file = File::open(cert).with_context(|| format!("open cert {cert}"))?;
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut BufReader::new(cert_file)).collect::<Result<Vec<_>, _>>()?;
    let key_file = File::open(key).with_context(|| format!("open key {key}"))?;
    let key = rustls_pemfile::pkcs8_private_keys(&mut BufReader::new(key_file))
        .next()
        .transpose()?
        .map(PrivateKeyDer::Pkcs8)
        .context("read private key")?;
    let mut tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    tls.alpn_protocols = vec![b"h3".to_vec()];
    let mut server_cfg = quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(tls)?,
    ));
    server_cfg.transport = Arc::new(TransportConfig::default());
    Ok(server_cfg)
}

async fn serve_tuic_connection(conn: quinn::Connection, users: Vec<(Uuid, String)>) -> Result<()> {
    loop {
        let (mut send, mut recv) = match conn.accept_bi().await {
            Ok(v) => v,
            Err(_) => break,
        };
        let users = users.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_tuic_stream(&mut send, &mut recv, users).await {
                tracing::debug!(error = %err, "tuic stream failed");
            }
        });
    }
    Ok(())
}

async fn handle_tuic_stream(
    send: &mut quinn::SendStream,
    recv: &mut quinn::RecvStream,
    users: Vec<(Uuid, String)>,
) -> Result<()> {
    let mut header = vec![0u8; 512];
    let n = recv.read(&mut header).await?.context("tuic header")?;
    let mut cursor = &header[..n];
    if cursor.is_empty() || cursor[0] != 0x01 {
        anyhow::bail!("invalid tuic command");
    }
    cursor = &cursor[1..];
    if cursor.len() < 16 {
        anyhow::bail!("truncated tuic uuid");
    }
    let uid = Uuid::from_bytes(cursor[..16].try_into()?);
    cursor = &cursor[16..];
    if cursor.is_empty() {
        anyhow::bail!("truncated tuic password len");
    }
    let pass_len = cursor[0] as usize;
    cursor = &cursor[1..];
    if cursor.len() < pass_len + 1 {
        anyhow::bail!("truncated tuic password");
    }
    let pass = std::str::from_utf8(&cursor[..pass_len])?;
    cursor = &cursor[pass_len..];
    if !users.iter().any(|(u, p)| *u == uid && p == pass) {
        send.write_all(&[1]).await?;
        anyhow::bail!("tuic auth failed");
    }
    let atyp = cursor[0];
    cursor = &cursor[1..];
    let dest = match atyp {
        0x01 => {
            if cursor.len() < 6 {
                anyhow::bail!("truncated ipv4");
            }
            let ip = std::net::Ipv4Addr::new(cursor[0], cursor[1], cursor[2], cursor[3]);
            let port = u16::from_be_bytes([cursor[4], cursor[5]]);
            SocketAddr::from((ip, port))
        },
        0x03 => {
            if cursor.len() < 18 {
                anyhow::bail!("truncated ipv6");
            }
            let ip = std::net::Ipv6Addr::from(<[u8; 16]>::try_from(&cursor[..16]).unwrap());
            let port = u16::from_be_bytes([cursor[16], cursor[17]]);
            SocketAddr::from((ip, port))
        },
        _ => anyhow::bail!("unsupported tuic address type {atyp}"),
    };
    send.write_all(&[0]).await?;
    let mut remote = tokio::net::TcpStream::connect(dest).await?;
    let (mut cr, mut cw) = remote.split();
    let mut sr = recv;
    let mut sw = send;
    tokio::select! {
        _ = tokio::io::copy(&mut sr, &mut cw) => {}
        _ = tokio::io::copy(&mut cr, &mut sw) => {}
    }
    Ok(())
}
