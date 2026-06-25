use anyhow::Result;
use async_trait::async_trait;
use rsb_core::{tcp_stream, BoxError, Inbound, Network, Outbound, ProxyConn, ProxyUdpSocket};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub struct DirectOutbound {
    tag: String,
    bind_interface: Option<String>,
}

impl DirectOutbound {
    pub fn new(tag: String, bind_interface: Option<String>) -> Self {
        Self {
            tag,
            bind_interface,
        }
    }
}

#[async_trait]
impl Outbound for DirectOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_DIRECT
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(&self, destination: SocketAddr, _domain: Option<&str>) -> Result<ProxyConn, BoxError> {
        let stream = rsb_core::tcp_connect_via(destination, self.bind_interface.as_deref()).await?;
        Ok(tcp_stream(stream))
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let socket = rsb_core::udp_bind_via(self.bind_interface.as_deref()).await?;
        Ok(ProxyUdpSocket::from_tokio(socket))
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

pub struct BlockOutbound {
    tag: String,
}

impl BlockOutbound {
    pub fn new(tag: String) -> Self {
        Self { tag }
    }
}

#[async_trait]
impl Outbound for BlockOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_BLOCK
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(&self, _destination: SocketAddr, _domain: Option<&str>) -> Result<ProxyConn, BoxError> {
        anyhow::bail!("connection blocked by outbound `{}`", self.tag)
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        anyhow::bail!("connection blocked by outbound `{}`", self.tag)
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

pub struct DirectInbound {
    tag: String,
    listen: SocketAddr,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl DirectInbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let listen = parse_listen(&raw)?;
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for DirectInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_DIRECT
    }
    async fn start(&self) -> Result<(), BoxError> {
        let listener = TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, "direct inbound listening");
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    accept = listener.accept() => {
                        if accept.is_err() { break; }
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

pub fn parse_listen(raw: &Value) -> Result<SocketAddr> {
    let listen = raw
        .get("listen")
        .and_then(|v| v.as_str())
        .unwrap_or("127.0.0.1");
    let port = raw
        .get("listen_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(1080) as u16;
    Ok(format!("{listen}:{port}").parse()?)
}
