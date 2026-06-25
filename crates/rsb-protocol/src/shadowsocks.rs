use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{
    tcp_stream, BoxError, Inbound, Network, Outbound, ProxyConn, ProxyUdpIo,
    ProxyUdpSocket,
};
use serde_json::Value;
use shadowsocks::config::ServerConfig;
use shadowsocks::config::ServerType;
use shadowsocks::context::{Context as SsContext, SharedContext};
use shadowsocks::crypto::CipherKind;
use shadowsocks::relay::socks5::Address;
use shadowsocks::relay::tcprelay::proxy_stream::{ProxyClientStream, ProxyServerStream};
use shadowsocks::relay::udprelay::proxy_socket::ProxySocket;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};

static SS_CLIENT_CTX: std::sync::OnceLock<SharedContext> = std::sync::OnceLock::new();
static SS_SERVER_CTX: std::sync::OnceLock<SharedContext> = std::sync::OnceLock::new();

fn ss_client_context() -> SharedContext {
    SS_CLIENT_CTX
        .get_or_init(|| SsContext::new_shared(ServerType::Local))
        .clone()
}

fn ss_server_context() -> SharedContext {
    SS_SERVER_CTX
        .get_or_init(|| SsContext::new_shared(ServerType::Server))
        .clone()
}

pub struct ShadowsocksOutbound {
    tag: String,
    server_config: Arc<ServerConfig>,
}

impl ShadowsocksOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        Ok(Self {
            tag,
            server_config: Arc::new(parse_server_config(&raw)?),
        })
    }
}

fn parse_server_config(raw: &Value) -> Result<ServerConfig> {
    let server = raw
        .get("server")
        .and_then(|v| v.as_str())
        .context("shadowsocks: server required")?;
    let port = raw
        .get("server_port")
        .and_then(|v| v.as_u64())
        .context("shadowsocks: server_port required")? as u16;
    let password = raw
        .get("password")
        .and_then(|v| v.as_str())
        .context("shadowsocks: password required")?;
    let method = raw
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("aes-256-gcm");
    Ok(ServerConfig::new(
        (server, port),
        password,
        parse_cipher(method)?,
    )?)
}

fn parse_cipher(method: &str) -> Result<CipherKind> {
    match method.to_lowercase().as_str() {
        "aes-128-gcm" => Ok(CipherKind::AES_128_GCM),
        "aes-256-gcm" => Ok(CipherKind::AES_256_GCM),
        "chacha20-ietf-poly1305" | "chacha20-poly1305" => Ok(CipherKind::CHACHA20_POLY1305),
        other => anyhow::bail!("unsupported shadowsocks method: {other}"),
    }
}

#[async_trait]
impl Outbound for ShadowsocksOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_SHADOWSOCKS
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(&self, destination: SocketAddr) -> Result<ProxyConn, BoxError> {
        let stream = ProxyClientStream::connect(
            ss_client_context(),
            &self.server_config,
            Address::from(destination),
        )
        .await
        .context("shadowsocks connect")?;
        Ok(tcp_stream(stream))
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let socket = ProxySocket::connect(ss_client_context(), &self.server_config)
            .await
            .context("shadowsocks udp connect")?;
        Ok(ProxyUdpSocket::from_io(Arc::new(SsUdpIo {
            socket: tokio::sync::Mutex::new(socket),
        })))
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

struct SsUdpIo {
    socket: tokio::sync::Mutex<ProxySocket<shadowsocks::net::UdpSocket>>,
}

#[async_trait]
impl ProxyUdpIo for SsUdpIo {
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize> {
        let socket = self.socket.lock().await;
        socket
            .send(&Address::from(target), buf)
            .await
            .map_err(std::io::Error::other)
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let socket = self.socket.lock().await;
        let mut recv_buf = vec![0u8; 65536];
        let (n, addr, _) = socket
            .recv(&mut recv_buf)
            .await
            .map_err(std::io::Error::other)?;
        let src = ss_address_to_socket_addr(&addr)?;
        let copy = n.min(buf.len());
        buf[..copy].copy_from_slice(&recv_buf[..copy]);
        Ok((copy, src))
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        Ok("0.0.0.0:0".parse().unwrap())
    }
}

fn ss_address_to_socket_addr(addr: &Address) -> std::io::Result<SocketAddr> {
    match addr {
        Address::SocketAddress(sa) => Ok(*sa),
        Address::DomainNameAddress(host, port) => {
            let mut addrs = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), *port))?;
            addrs
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "resolve ss addr"))
        }
    }
}

pub struct ShadowsocksInbound {
    tag: String,
    listen: SocketAddr,
    server_config: Arc<ServerConfig>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ShadowsocksInbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen: crate::direct::parse_listen(&raw)?,
            server_config: Arc::new(parse_server_config(&raw)?),
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for ShadowsocksInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_SHADOWSOCKS
    }
    async fn start(&self) -> Result<(), BoxError> {
        let listener = TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, "shadowsocks inbound listening");
        let cfg = self.server_config.clone();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    accept = listener.accept() => {
                        let Ok((stream, _)) = accept else { break };
                        let cfg = cfg.clone();
                        tokio::spawn(async move {
                            if let Err(err) = serve_ss_client(stream, cfg).await {
                                tracing::debug!(error = %err, "ss client failed");
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

async fn serve_ss_client(stream: TcpStream, server_config: Arc<ServerConfig>) -> Result<()> {
    let mut ss = ProxyServerStream::from_stream(
        ss_server_context(),
        stream,
        server_config.method(),
        server_config.key(),
    );
    let target = ss.handshake().await?;
    let mut remote = match target {
        Address::SocketAddress(addr) => TcpStream::connect(addr).await?,
        Address::DomainNameAddress(domain, port) => {
            let addr = tokio::net::lookup_host(format!("{domain}:{port}"))
                .await?
                .next()
                .context("resolve ss target")?;
            TcpStream::connect(addr).await?
        }
    };
    crate::inbound_proxy::relay_streams(&mut ss, &mut remote).await
}
