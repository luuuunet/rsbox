use crate::transport::{self, encode_trojan_tcp, encode_trojan_udp, sha224_hex, trojan_key};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound, Network, Outbound, ProxyConn, ProxyUdpSocket, SharedOutboundManager};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct TrojanOutbound {
    tag: String,
    server: String,
    port: u16,
    password: String,
    tls: Option<Value>,
    sni: Option<String>,
    detour: Option<String>,
    shared: Arc<SharedOutboundManager>,
}

impl TrojanOutbound {
    pub fn new(tag: String, raw: Value, shared: Arc<SharedOutboundManager>) -> Result<Self> {
        Ok(Self {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("trojan: server required")?
                .to_string(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("trojan: server_port required")? as u16,
            password: raw
                .get("password")
                .and_then(|v| v.as_str())
                .context("trojan: password required")?
                .to_string(),
            tls: raw.get("tls").cloned(),
            sni: raw
                .get("tls")
                .and_then(|t| t.get("server_name"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            detour: crate::detour::detour_tag(&raw),
            shared,
        })
    }

    async fn connect(&self, destination: SocketAddr) -> Result<ProxyConn> {
        let mut stream = crate::detour::dial_server_link(
            &self.shared,
            self.detour.as_deref(),
            &self.server,
            self.port,
            self.tls.as_ref(),
            self.sni.as_deref(),
        )
        .await?;
        let key = trojan_key(&self.password);
        let header = encode_trojan_tcp(&key, destination);
        stream.write_all(&header).await?;
        Ok(stream)
    }

    async fn connect_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket> {
        let mut stream = crate::detour::dial_server_link(
            &self.shared,
            self.detour.as_deref(),
            &self.server,
            self.port,
            self.tls.as_ref(),
            self.sni.as_deref(),
        )
        .await?;
        let key = trojan_key(&self.password);
        stream.write_all(&encode_trojan_udp(&key, destination)).await?;
        Ok(crate::udp_over_tcp::tunneled_udp(stream).await)
    }
}

#[async_trait]
impl Outbound for TrojanOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_TROJAN
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        _domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        self.connect(destination).await
    }
    async fn dial_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        self.connect_udp(destination).await
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

pub struct TrojanInbound {
    tag: String,
    listen: SocketAddr,
    users: Vec<String>,
    connections: rsb_core::SharedConnectionManager,
    tls_cert: String,
    tls_key: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl TrojanInbound {
    pub fn new(tag: String, raw: Value, connections: rsb_core::SharedConnectionManager) -> Result<Self> {
        let listen = crate::direct::parse_listen(&raw)?;
        let mut users = Vec::new();
        if let Some(arr) = raw.get("users").and_then(|v| v.as_array()) {
            for u in arr {
                if let Some(p) = u.get("password").and_then(|v| v.as_str()) {
                    users.push(sha224_hex(p));
                }
            }
        }
        if users.is_empty() {
            if let Some(p) = raw.get("password").and_then(|v| v.as_str()) {
                users.push(sha224_hex(p));
            }
        }
        anyhow::ensure!(!users.is_empty(), "trojan inbound: password/users required");
        let tls = raw.get("tls").context("trojan inbound: tls required")?;
        let cert = tls
            .get("certificate_path")
            .or_else(|| tls.get("certificate"))
            .and_then(|v| v.as_str())
            .context("trojan inbound: tls certificate_path")?
            .to_string();
        let key = tls
            .get("key_path")
            .or_else(|| tls.get("key"))
            .and_then(|v| v.as_str())
            .context("trojan inbound: tls key_path")?
            .to_string();
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen,
            users,
            connections,
            tls_cert: cert,
            tls_key: key,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for TrojanInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_TROJAN
    }
    async fn start(&self) -> Result<(), BoxError> {
        let acceptor = build_tls_acceptor(&self.tls_cert, &self.tls_key)?;
        let listener = TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, "trojan inbound listening");
        let users = self.users.clone();
        let connections = self.connections.clone();
        let inbound_tag = self.tag.clone();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    accept = listener.accept() => {
                        let Ok((stream, _)) = accept else { break };
                        let acceptor = acceptor.clone();
                        let users = users.clone();
                        let connections = connections.clone();
                        let inbound_tag = inbound_tag.clone();
                        tokio::spawn(async move {
                            let mut stream = stream;
                            if let Err(err) =
                                serve_trojan(stream, acceptor, users, connections, inbound_tag)
                                    .await
                            {
                                tracing::debug!(error = %err, "trojan client failed");
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

pub fn build_tls_acceptor(cert: &str, key: &str) -> Result<tokio_rustls::TlsAcceptor> {
    use rustls::pki_types::CertificateDer;
    use rustls::ServerConfig;
    use std::fs::File;
    use std::io::BufReader;
    let cert_file = File::open(cert).with_context(|| format!("open cert {cert}"))?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_reader).collect::<Result<Vec<_>, _>>()?;
    let key_file = File::open(key).with_context(|| format!("open key {key}"))?;
    let mut key_reader = BufReader::new(key_file);
    let key = rustls_pemfile::private_key(&mut key_reader)
        .context("parse key pem")?
        .context("read private key")?;
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(cfg)))
}

pub(crate) async fn serve_trojan(
    mut stream: TcpStream,
    acceptor: tokio_rustls::TlsAcceptor,
    users: Vec<String>,
    connections: rsb_core::SharedConnectionManager,
    inbound_tag: String,
) -> Result<()> {
    let mut tls = acceptor.accept(stream).await?;
    let mut buf = vec![0u8; 4096];
    let n = tls.read(&mut buf).await?;
    if n < 56 + 2 + 1 + 7 + 2 {
        anyhow::bail!("truncated trojan header");
    }
    let key = std::str::from_utf8(&buf[..56]).context("trojan key utf8")?;
    if !users.iter().any(|u| u == key) {
        anyhow::bail!("trojan auth failed");
    }
    let mut off = 56;
    if &buf[off..off + 2] != b"\r\n" {
        anyhow::bail!("invalid trojan header delimiter");
    }
    off += 2;
    let _cmd = buf[off];
    off += 1;
    let (dest, consumed) = transport::decode_socks_address(&buf[off..])?;
    off += consumed;
    if off + 2 > n || &buf[off..off + 2] != b"\r\n" {
        anyhow::bail!("invalid trojan header trailer");
    }
    off += 2;
    let dest = match dest {
        transport::DecodedSocksAddr::Ip(addr) => addr,
        transport::DecodedSocksAddr::Domain(host, port) => tokio::net::lookup_host(format!("{host}:{port}"))
            .await?
            .next()
            .with_context(|| format!("resolve {host}"))?,
    };
    let mut remote = TcpStream::connect(dest).await?;
    if off < n {
        remote.write_all(&buf[off..n]).await?;
    }
    let session = crate::user_relay::begin_for_trojan_hash(
        &connections,
        &inbound_tag,
        key,
        Some(dest),
        None,
    )?;
    crate::inbound_proxy::relay_streams_user(&session, &mut tls, &mut remote).await
}
