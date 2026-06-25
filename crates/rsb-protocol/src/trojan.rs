use crate::transport::{self, sha224_hex};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{proxy_box, BoxError, Inbound, Network, Outbound, ProxyConn, ProxyUdpSocket};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct TrojanOutbound {
    tag: String,
    server: String,
    port: u16,
    password: String,
    tls: Option<Value>,
    sni: Option<String>,
}

impl TrojanOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
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
        let hash = sha224_hex(&self.password);
        let target = format_address(destination);
        let header = format!("{hash}\r\n{target}\r\n");
        tls.write_all(header.as_bytes()).await?;
        Ok(proxy_box(tls))
    }

    async fn connect_udp(&self) -> Result<ProxyUdpSocket> {
        let mut tls = transport::tls_connect(
            &self.server,
            self.port,
            self.tls.as_ref(),
            self.sni.as_deref(),
        )
        .await?;
        let hash = sha224_hex(&self.password);
        tls.write_all(format!("{hash}\r\n").as_bytes()).await?;
        tls.write_all(b"UDP\r\n").await?;
        Ok(crate::udp_over_tcp::tunneled_udp(tls).await)
    }
}

fn format_address(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v4) => format!("{}:{}", v4.ip(), v4.port()),
        SocketAddr::V6(v6) => format!("[{}]:{}", v6.ip(), v6.port()),
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
    async fn dial_tcp(&self, destination: SocketAddr, _domain: Option<&str>) -> Result<ProxyConn, BoxError> {
        self.connect(destination).await
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        self.connect_udp().await
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

pub struct TrojanInbound {
    tag: String,
    listen: SocketAddr,
    users: Vec<String>,
    tls_cert: String,
    tls_key: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl TrojanInbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
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
                        tokio::spawn(async move {
                            if let Err(err) = serve_trojan(stream, acceptor, users).await {
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
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    use rustls::ServerConfig;
    use std::fs::File;
    use std::io::BufReader;
    let cert_file = File::open(cert).with_context(|| format!("open cert {cert}"))?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_reader).collect::<Result<Vec<_>, _>>()?;
    let key_file = File::open(key).with_context(|| format!("open key {key}"))?;
    let mut key_reader = BufReader::new(key_file);
    let key = rustls_pemfile::pkcs8_private_keys(&mut key_reader)
        .next()
        .transpose()?
        .map(PrivateKeyDer::Pkcs8)
        .context("read private key")?;
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(cfg)))
}

pub(crate) async fn serve_trojan(
    stream: TcpStream,
    acceptor: tokio_rustls::TlsAcceptor,
    users: Vec<String>,
) -> Result<()> {
    let mut tls = acceptor.accept(stream).await?;
    let mut buf = vec![0u8; 56 + 2 + 256];
    let n = tls.read(&mut buf).await?;
    let text = std::str::from_utf8(&buf[..n])?;
    let Some((hash, rest)) = text.split_once("\r\n") else {
        anyhow::bail!("invalid trojan header");
    };
    if !users.iter().any(|u| u == hash) {
        anyhow::bail!("trojan auth failed");
    }
    let Some((target, _)) = rest.split_once("\r\n") else {
        anyhow::bail!("invalid trojan target");
    };
    let dest: SocketAddr = if target.starts_with('[') {
        target.parse()?
    } else {
        tokio::net::lookup_host(target)
            .await?
            .next()
            .with_context(|| format!("resolve {target}"))?
    };
    let mut remote = TcpStream::connect(dest).await?;
    crate::inbound_proxy::relay_streams(&mut tls, &mut remote).await
}
