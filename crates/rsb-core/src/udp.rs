use async_trait::async_trait;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// UDP socket returned by outbounds (direct or proxied).
#[derive(Clone)]
pub struct ProxyUdpSocket {
    inner: Arc<dyn ProxyUdpIo>,
}

#[async_trait]
pub trait ProxyUdpIo: Send + Sync {
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize>;
    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)>;
    fn local_addr(&self) -> std::io::Result<SocketAddr>;
}

pub struct TokioUdpAdapter(pub UdpSocket);

#[async_trait]
impl ProxyUdpIo for TokioUdpAdapter {
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize> {
        self.0.send_to(buf, target).await
    }
    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        self.0.recv_from(buf).await
    }
    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.0.local_addr()
    }
}

impl ProxyUdpSocket {
    pub async fn bind(addr: SocketAddr) -> std::io::Result<Self> {
        Ok(Self::from_tokio(UdpSocket::bind(addr).await?))
    }

    pub fn from_tokio(socket: UdpSocket) -> Self {
        Self {
            inner: Arc::new(TokioUdpAdapter(socket)),
        }
    }

    pub fn from_io(inner: Arc<dyn ProxyUdpIo>) -> Self {
        Self { inner }
    }

    pub async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize> {
        self.inner.send_to(buf, target).await
    }

    pub async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        self.inner.recv_from(buf).await
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.inner.local_addr()
    }
}
