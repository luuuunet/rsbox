use crate::transport::{self};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound, Network, Outbound, ProxyConn, ProxyUdpSocket, SharedOutboundManager};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use std::pin::Pin;
use tokio::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};
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
    detour: Option<String>,
    shared: Arc<SharedOutboundManager>,
}

impl VlessOutbound {
    pub fn new(tag: String, raw: Value, shared: Arc<SharedOutboundManager>) -> Result<Self> {
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
            detour: crate::detour::detour_tag(&raw),
            shared,
        })
    }

    fn use_tls(&self) -> bool {
        self.tls
            .as_ref()
            .map(|t| t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true))
            .unwrap_or(false)
    }

    async fn connect(&self, destination: SocketAddr) -> Result<ProxyConn> {
        tracing::debug!(%destination, "vless outbound connect start");
        let vision = crate::xtls_vision::is_vision_flow(self.flow.as_deref());
        if self.detour.is_none() && self.use_tls() && vision {
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
        let mut stream = crate::detour::dial_server_link(
            &self.shared,
            self.detour.as_deref(),
            &self.server,
            self.port,
            self.tls.as_ref(),
            self.sni.as_deref(),
        )
        .await?;
        tracing::debug!(%destination, "vless outbound tls dial ok");
        let header = build_vless_request(self.uuid, destination, self.flow.as_deref(), 1);
        stream.write_all(&header).await?;
        stream.flush().await?;
        tracing::debug!(header_len = header.len(), "vless outbound header sent");
        if self.use_tls() && !vision {
            read_vless_ack(&mut stream).await?;
            Ok(stream)
        } else {
            Ok(VlessResponseStream::new(stream))
        }
    }

    async fn connect_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket> {
        let xudp = self.packet_encoding == "xudp";
        let stream = if self.detour.is_some() || !self.use_tls() {
            crate::detour::dial_server_link(
                &self.shared,
                self.detour.as_deref(),
                &self.server,
                self.port,
                self.tls.as_ref(),
                self.sni.as_deref(),
            )
            .await?
        } else {
            rsb_core::proxy_box(
                transport::tls_connect(
                    &self.server,
                    self.port,
                    self.tls.as_ref(),
                    self.sni.as_deref(),
                )
                .await?,
            )
        };
        if xudp {
            let mut stream = stream;
            let header = crate::xudp::vless_xudp_request_header(self.uuid, self.flow.as_deref());
            stream.write_all(&header).await?;
            return Ok(crate::xudp::xudp_over_stream(stream, Some(destination)).await);
        }
        let mut stream = stream;
        let header = build_vless_request(self.uuid, destination, self.flow.as_deref(), 2);
        stream.write_all(&header).await?;
        stream.flush().await?;
        read_vless_ack(&mut stream).await?;
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
    buf.extend_from_slice(&transport::encode_port_first_socksaddr(dest));
    buf
}

/// Returns total byte length of a VLESS request when `buf` is complete; `None` if more data needed.
pub fn vless_request_len(buf: &[u8]) -> Result<Option<usize>> {
    if buf.is_empty() {
        return Ok(None);
    }
    if buf[0] != 0 {
        anyhow::bail!("invalid vless version {}", buf[0]);
    }
    if buf.len() < 1 + 16 + 1 {
        return Ok(None);
    }
    let addon_len = buf[1 + 16] as usize;
    let base = 1 + 16 + 1 + addon_len;
    if buf.len() < base + 4 {
        return Ok(None);
    }
    let atyp = buf[base + 3];
    let addr_start = base + 4;
    let addr_len = match atyp {
        0x01 => {
            if buf.len() < addr_start + 4 {
                return Ok(None);
            }
            4
        },
        0x02 => {
            if buf.len() < addr_start + 1 {
                return Ok(None);
            }
            let dl = buf[addr_start] as usize;
            if buf.len() < addr_start + 1 + dl {
                return Ok(None);
            }
            1 + dl
        },
        0x03 => {
            if buf.len() < addr_start + 16 {
                return Ok(None);
            }
            16
        },
        other => anyhow::bail!("unsupported address type {other}"),
    };
    Ok(Some(addr_start + addr_len))
}

pub async fn read_vless_request(stream: &mut (impl AsyncRead + Unpin)) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(128);
    loop {
        let mut tmp = [0u8; 256];
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            anyhow::bail!("truncated vless request");
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(len) = vless_request_len(&buf)? {
            buf.truncate(len);
            return Ok(buf);
        }
        if buf.len() > 4096 {
            anyhow::bail!("vless request too large");
        }
    }
}

pub async fn read_vless_ack(stream: &mut (impl AsyncRead + Unpin)) -> Result<()> {
    let mut resp = [0u8; 2];
    tokio::time::timeout(std::time::Duration::from_secs(15), stream.read_exact(&mut resp))
        .await
        .context("vless response timeout")??;
    if resp != [0, 0] {
        anyhow::bail!("invalid vless response {:?}", resp);
    }
    Ok(())
}

struct VlessResponseStream {
    inner: rsb_core::ProxyConn,
    response_read: bool,
    ack_buf: Vec<u8>,
}

impl VlessResponseStream {
    fn new(inner: rsb_core::ProxyConn) -> rsb_core::ProxyConn {
        rsb_core::proxy_box(Self {
            inner,
            response_read: false,
            ack_buf: Vec::new(),
        })
    }
}

impl AsyncRead for VlessResponseStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if !self.response_read {
            if self.ack_buf.len() < 2 {
                let mut tmp = [0u8; 2];
                let start = self.ack_buf.len();
                let mut read_buf = tokio::io::ReadBuf::new(&mut tmp[start..]);
                match Pin::new(&mut self.inner).poll_read(cx, &mut read_buf) {
                    std::task::Poll::Ready(Ok(())) => {
                        self.ack_buf.extend_from_slice(read_buf.filled());
                        if self.ack_buf.len() < 2 {
                            return std::task::Poll::Pending;
                        }
                    }
                    std::task::Poll::Ready(Err(e)) => return std::task::Poll::Ready(Err(e)),
                    std::task::Poll::Pending => return std::task::Poll::Pending,
                }
            }
            if self.ack_buf[0] != 0 {
                return std::task::Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "invalid vless response version",
                )));
            }
            self.response_read = true;
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for VlessResponseStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
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
    connections: rsb_core::SharedConnectionManager,
    tls_cert: String,
    tls_key: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl VlessInbound {
    pub fn new(tag: String, raw: Value, connections: rsb_core::SharedConnectionManager) -> Result<Self> {
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
            connections,
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
                                serve_vless(stream, acceptor, users, connections, inbound_tag)
                                    .await
                            {
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
    mut stream: TcpStream,
    acceptor: tokio_rustls::TlsAcceptor,
    users: Vec<Uuid>,
    connections: rsb_core::SharedConnectionManager,
    inbound_tag: String,
) -> Result<()> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;
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
    tls.write_all(&[0, 0]).await?;
    let session = crate::user_relay::begin_for_uuid(
        &connections,
        &inbound_tag,
        &uid,
        Some(dest),
        Some(host),
    )?;
    crate::inbound_proxy::relay_streams_user(&session, &mut tls, &mut remote).await
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
