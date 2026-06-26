use crate::transport::{self};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound, Network, Outbound, ProxyConn, ProxyUdpSocket};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

pub struct VlessOutbound {
    tag: String,
    server: String,
    port: u16,
    uuid: Uuid,
    flow: Option<String>,
    packet_encoding: String,
    tls: Option<Value>,
    sni: Option<String>,
}

impl VlessOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let uuid_str = raw
            .get("uuid")
            .and_then(|v| v.as_str())
            .context("vless: uuid required")?;
        Ok(Self {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("vless: server required")?
                .to_string(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("vless: server_port required")? as u16,
            uuid: Uuid::parse_str(uuid_str).context("vless: invalid uuid")?,
            flow: raw.get("flow").and_then(|v| v.as_str()).map(str::to_string),
            packet_encoding: raw
                .get("packet_encoding")
                .and_then(|v| v.as_str())
                .unwrap_or("xudp")
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
        let use_tls = self
            .tls
            .as_ref()
            .map(|t| t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true))
            .unwrap_or(false);
        let vision = crate::xtls_vision::is_vision_flow(self.flow.as_deref());
        if use_tls && vision {
            let mut tls = transport::tls_connect(
                &self.server,
                self.port,
                self.tls.as_ref(),
                self.sni.as_deref(),
            )
            .await?;
            let header = build_vless_request(self.uuid, destination, self.flow.as_deref(), 1);
            tls.write_all(&header).await?;
            return crate::xtls_vision::vision_relay(tls, self.uuid).await;
        }
        let mut stream: Box<dyn rsb_core::ProxyStream> = if use_tls {
            Box::new(
                transport::tls_connect(
                    &self.server,
                    self.port,
                    self.tls.as_ref(),
                    self.sni.as_deref(),
                )
                .await?,
            )
        } else {
            Box::new(transport::tcp_connect(&self.server, self.port).await?)
        };
        let header = build_vless_request(self.uuid, destination, self.flow.as_deref(), 1);
        stream.write_all(&header).await?;
        Ok(stream)
    }

    async fn connect_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket> {
        let use_tls = self
            .tls
            .as_ref()
            .map(|t| t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true))
            .unwrap_or(false);
        let xudp = self.packet_encoding == "xudp";
        if use_tls {
            let stream = transport::tls_connect(
                &self.server,
                self.port,
                self.tls.as_ref(),
                self.sni.as_deref(),
            )
            .await?;
            if xudp {
                let mut stream = stream;
                let header =
                    crate::xudp::vless_xudp_request_header(self.uuid, self.flow.as_deref());
                stream.write_all(&header).await?;
                return Ok(crate::xudp::xudp_over_stream(stream, Some(destination)).await);
            }
            let mut stream = stream;
            let header = build_vless_request(self.uuid, destination, self.flow.as_deref(), 2);
            stream.write_all(&header).await?;
            return Ok(crate::udp_over_tcp::tunneled_udp(stream).await);
        }
        let mut stream = transport::tcp_connect(&self.server, self.port).await?;
        if xudp {
            let header = crate::xudp::vless_xudp_request_header(self.uuid, self.flow.as_deref());
            stream.write_all(&header).await?;
            return Ok(crate::xudp::xudp_over_stream(stream, Some(destination)).await);
        }
        let header = build_vless_request(self.uuid, destination, self.flow.as_deref(), 2);
        stream.write_all(&header).await?;
        Ok(crate::udp_over_tcp::tunneled_udp(stream).await)
    }
}

fn build_vless_request(uuid: Uuid, dest: SocketAddr, flow: Option<&str>, command: u8) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(0);
    buf.extend_from_slice(uuid.as_bytes());
    match flow.filter(|f| !f.is_empty()) {
        Some(f) if crate::xtls_vision::is_vision_flow(Some(f)) => {
            let pb = crate::xtls_vision::encode_vision_addons(f);
            buf.push(pb.len() as u8);
            buf.extend_from_slice(&pb);
        },
        Some(f) => {
            buf.push(f.len() as u8);
            buf.extend_from_slice(f.as_bytes());
        },
        None => buf.push(0),
    }
    buf.push(command);
    buf.extend_from_slice(&dest.port().to_be_bytes());
    buf.extend_from_slice(&transport::encode_vless_address(dest));
    buf
}

#[async_trait]
impl Outbound for VlessOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_VLESS
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

pub struct VlessInbound {
    tag: String,
    listen: SocketAddr,
    users: Vec<Uuid>,
    tls_cert: String,
    tls_key: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl VlessInbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let listen = crate::direct::parse_listen(&raw)?;
        let mut users = Vec::new();
        if let Some(arr) = raw.get("users").and_then(|v| v.as_array()) {
            for u in arr {
                if let Some(id) = u.get("uuid").and_then(|v| v.as_str()) {
                    users.push(Uuid::parse_str(id)?);
                }
            }
        }
        if users.is_empty() {
            if let Some(id) = raw.get("uuid").and_then(|v| v.as_str()) {
                users.push(Uuid::parse_str(id)?);
            }
        }
        anyhow::ensure!(!users.is_empty(), "vless inbound: uuid/users required");
        let tls = raw.get("tls").context("vless inbound: tls required")?;
        let cert = tls
            .get("certificate_path")
            .or_else(|| tls.get("certificate"))
            .and_then(|v| v.as_str())
            .context("vless inbound: certificate")?
            .to_string();
        let key = tls
            .get("key_path")
            .or_else(|| tls.get("key"))
            .and_then(|v| v.as_str())
            .context("vless inbound: key")?
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
impl Inbound for VlessInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_VLESS
    }
    async fn start(&self) -> Result<(), BoxError> {
        let acceptor = crate::trojan::build_tls_acceptor(&self.tls_cert, &self.tls_key)?;
        let listener = TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, "vless inbound listening");
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
                            if let Err(err) = serve_vless(stream, acceptor, users).await {
                                tracing::debug!(error = %err, "vless client failed");
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

async fn serve_vless(
    stream: TcpStream,
    acceptor: tokio_rustls::TlsAcceptor,
    users: Vec<Uuid>,
) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let mut tls = acceptor.accept(stream).await?;
    let mut header = vec![0u8; 512];
    let n = tls.read(&mut header).await?;
    let mut cursor = &header[..n];
    if cursor.is_empty() || cursor[0] != 0 {
        anyhow::bail!("invalid vless version");
    }
    cursor = &cursor[1..];
    if cursor.len() < 16 {
        anyhow::bail!("truncated vless uuid");
    }
    let uid = Uuid::from_bytes(cursor[..16].try_into()?);
    if !users.contains(&uid) {
        anyhow::bail!("vless auth failed");
    }
    cursor = &cursor[16..];
    let addon_len = cursor[0] as usize;
    cursor = &cursor[1 + addon_len..];
    if cursor.len() < 3 {
        anyhow::bail!("truncated vless header");
    }
    let _cmd = cursor[0];
    let port = u16::from_be_bytes([cursor[1], cursor[2]]);
    let atyp = cursor[3];
    cursor = &cursor[4..];
    let (host, consumed) = crate::vless::read_address(cursor, atyp)?;
    cursor = &cursor[consumed..];
    let dest: SocketAddr = tokio::net::lookup_host(format!("{host}:{port}"))
        .await?
        .next()
        .with_context(|| format!("resolve {host}"))?;
    let mut remote = TcpStream::connect(dest).await?;
    crate::inbound_proxy::relay_streams(&mut tls, &mut remote).await
}

pub fn read_address(data: &[u8], atyp: u8) -> Result<(String, usize)> {
    match atyp {
        0x01 => {
            if data.len() < 4 {
                anyhow::bail!("truncated ipv4");
            }
            let ip = std::net::Ipv4Addr::new(data[0], data[1], data[2], data[3]);
            Ok((ip.to_string(), 4))
        },
        0x02 => {
            if data.is_empty() {
                anyhow::bail!("truncated domain");
            }
            let len = data[0] as usize;
            if data.len() < 1 + len {
                anyhow::bail!("truncated domain name");
            }
            Ok((std::str::from_utf8(&data[1..1 + len])?.to_string(), 1 + len))
        },
        0x03 => {
            if data.len() < 16 {
                anyhow::bail!("truncated ipv6");
            }
            let ip = std::net::Ipv6Addr::from(<[u8; 16]>::try_from(&data[..16]).unwrap());
            Ok((ip.to_string(), 16))
        },
        _ => anyhow::bail!("unsupported address type {atyp}"),
    }
}
