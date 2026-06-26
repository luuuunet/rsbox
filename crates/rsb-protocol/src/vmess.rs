use crate::transport::{self, address_from_socket};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound, Network, Outbound, ProxyConn, ProxyUdpSocket};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

pub struct VmessOutbound {
    tag: String,
    server: String,
    port: u16,
    uuid: Uuid,
    security: String,
    packet_encoding: String,
    global_padding: bool,
    authenticated_length: bool,
    tls: Option<Value>,
    sni: Option<String>,
}

impl VmessOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let uuid_str = raw
            .get("uuid")
            .and_then(|v| v.as_str())
            .context("vmess: uuid required")?;
        Ok(Self {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("vmess: server required")?
                .to_string(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("vmess: server_port required")? as u16,
            uuid: Uuid::parse_str(uuid_str).context("vmess: invalid uuid")?,
            security: raw
                .get("security")
                .and_then(|v| v.as_str())
                .unwrap_or("auto")
                .to_string(),
            packet_encoding: raw
                .get("packet_encoding")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            global_padding: raw
                .get("global_padding")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            authenticated_length: raw
                .get("authenticated_length")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
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
        let header = build_vmess_header(
            self.uuid,
            destination,
            1,
            self.global_padding,
            self.authenticated_length,
            &self.security,
        )?;
        stream.write_all(&header).await?;
        Ok(stream)
    }

    async fn connect_udp_tunnel(&self, destination: SocketAddr) -> Result<ProxyUdpSocket> {
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
                let header = build_vmess_mux_xudp_header(
                    self.uuid,
                    self.global_padding,
                    self.authenticated_length,
                )?;
                stream.write_all(&header).await?;
                return Ok(crate::xudp::xudp_over_stream(stream, Some(destination)).await);
            }
            let mut stream = stream;
            let header = build_vmess_header(
                self.uuid,
                destination,
                2,
                self.global_padding,
                self.authenticated_length,
                &self.security,
            )?;
            stream.write_all(&header).await?;
            return Ok(crate::udp_over_tcp::tunneled_udp(stream).await);
        }
        let mut stream = transport::tcp_connect(&self.server, self.port).await?;
        if xudp {
            let header = build_vmess_mux_xudp_header(
                self.uuid,
                self.global_padding,
                self.authenticated_length,
            )?;
            stream.write_all(&header).await?;
            return Ok(crate::xudp::xudp_over_stream(stream, Some(destination)).await);
        }
        let header = build_vmess_header(
            self.uuid,
            destination,
            2,
            self.global_padding,
            self.authenticated_length,
            &self.security,
        )?;
        stream.write_all(&header).await?;
        Ok(crate::udp_over_tcp::tunneled_udp(stream).await)
    }
}

fn build_vmess_header(
    uuid: Uuid,
    dest: SocketAddr,
    command: u8,
    global_padding: bool,
    authenticated_length: bool,
    security: &str,
) -> Result<Vec<u8>> {
    use aes_gcm::aead::KeyInit;

    use rand::RngCore;
    let mut req = Vec::new();
    req.push(1); // version
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    req.extend_from_slice(&(ts as u32).to_be_bytes());
    let mut nonce = [0u8; 16];
    rand::rng().fill_bytes(&mut nonce);
    req.extend_from_slice(&nonce);
    req.push(0); // alterId
    let mut options = 0u8;
    if global_padding {
        options |= 0x08;
    }
    if authenticated_length {
        options |= 0x10;
    }
    req.push(options);
    req.push(0); // pfs
    let padding_len = (rand::random::<u8>() % 16) as usize;
    req.push(padding_len as u8);
    // Security type
    let security_type = match security {
        "aes-128-gcm" => 3,
        "chacha20-poly1305" => 4,
        "none" => 5,
        "zero" => 0,
        _ => 1, // auto
    };
    req.push(security_type);
    req.push(0); // reserved
    req.push(command);
    req.extend_from_slice(&dest.port().to_be_bytes());
    req.extend_from_slice(&address_from_socket(dest));
    req.resize(req.len() + padding_len, 0);
    encrypt_vmess_body(uuid, req)
}

fn build_vmess_mux_xudp_header(
    uuid: Uuid,
    global_padding: bool,
    authenticated_length: bool,
) -> Result<Vec<u8>> {
    use rand::RngCore;
    let mut req = Vec::new();
    req.push(1);
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    req.extend_from_slice(&(ts as u32).to_be_bytes());
    let mut nonce = [0u8; 16];
    rand::rng().fill_bytes(&mut nonce);
    req.extend_from_slice(&nonce);
    req.push(0);
    let mut options = 0u8;
    if global_padding {
        options |= 0x08;
    }
    if authenticated_length {
        options |= 0x10;
    }
    req.push(options);
    req.push(0);
    let padding_len = (rand::random::<u8>() % 16) as usize;
    req.push(padding_len as u8);
    req.push(1);
    req.push(0);
    req.push(3); // mux
    req.extend_from_slice(&crate::xudp::MUX_XUDP_PORT.to_be_bytes());
    req.push(0x02); // domain
    let host = crate::xudp::mux_xudp_target().0;
    req.push(host.len() as u8);
    req.extend_from_slice(host.as_bytes());
    req.resize(req.len() + padding_len, 0);
    encrypt_vmess_body(uuid, req)
}

fn encrypt_vmess_body(uuid: Uuid, req: Vec<u8>) -> Result<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes128Gcm, Nonce};
    use rand::RngCore;
    let mut cmd_key = [0u8; 16];
    cmd_key.copy_from_slice(&uuid.as_bytes()[..16]);
    let cipher = Aes128Gcm::new_from_slice(&cmd_key).context("vmess key")?;
    let mut iv = [0u8; 12];
    rand::rng().fill_bytes(&mut iv);
    let encrypted = cipher
        .encrypt(Nonce::from_slice(&iv), req.as_ref())
        .map_err(|e| anyhow::anyhow!("vmess encrypt: {e}"))?;
    let mut out = Vec::new();
    out.extend_from_slice(uuid.as_bytes());
    out.extend_from_slice(&iv);
    out.extend_from_slice(&encrypted);
    Ok(out)
}

#[async_trait]
impl Outbound for VmessOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_VMESS
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
        self.connect_udp_tunnel(destination).await
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

pub struct VmessInbound {
    tag: String,
    listen: SocketAddr,
    users: Vec<Uuid>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl VmessInbound {
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
        anyhow::ensure!(!users.is_empty(), "vmess inbound: uuid/users required");
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen,
            users,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for VmessInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_VMESS
    }
    async fn start(&self) -> Result<(), BoxError> {
        let listener = TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, "vmess inbound listening");
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
                        let users = users.clone();
                        tokio::spawn(async move {
                            let mut stream = stream;
                            if let Err(err) = serve_vmess(stream, users).await {
                                tracing::debug!(error = %err, "vmess client failed");
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

async fn serve_vmess(mut stream: TcpStream, users: Vec<Uuid>) -> Result<()> {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes128Gcm, Nonce};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut io = stream;
    let mut buf = vec![0u8; 4096];
    let n = io.read(&mut buf).await?;
    if n < 16 + 12 + 16 {
        anyhow::bail!("truncated vmess header");
    }
    let uid = Uuid::from_bytes(buf[..16].try_into()?);
    if !users.contains(&uid) {
        anyhow::bail!("vmess auth failed");
    }
    let mut cmd_key = [0u8; 16];
    cmd_key.copy_from_slice(&uid.as_bytes()[..16]);
    let cipher = Aes128Gcm::new_from_slice(&cmd_key)?;
    let iv = &buf[16..28];
    let body = cipher
        .decrypt(Nonce::from_slice(iv), &buf[28..n])
        .map_err(|e| anyhow::anyhow!("vmess decrypt: {e}"))?;
    if body.len() < 12 {
        anyhow::bail!("truncated vmess body");
    }
    let port = u16::from_be_bytes([body[9], body[10]]);
    let atyp = body[11];
    let (host, _) = crate::vless::read_address(&body[12..], atyp)?;
    let dest: SocketAddr = tokio::net::lookup_host(format!("{host}:{port}"))
        .await?
        .next()
        .context("resolve vmess target")?;
    let remote = TcpStream::connect(dest).await?;
    crate::inbound_proxy::relay_bidirectional(&mut io, remote).await
}
