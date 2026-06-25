//! DNS outbound — resolve hostnames via configured DNS router.

use anyhow::Result;
use async_trait::async_trait;
use rsb_core::{tcp_stream, BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket};
use rsb_dns::DnsRouter;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpStream, UdpSocket};

pub struct DnsOutbound {
    tag: String,
    dns: Arc<DnsRouter>,
}

impl DnsOutbound {
    pub fn new(tag: String, _raw: Value, dns: Arc<DnsRouter>) -> Result<Self> {
        Ok(Self { tag, dns })
    }

    async fn resolve(&self, destination: SocketAddr) -> Result<SocketAddr> {
        if let Some(domain) = self.dns.reverse_lookup(destination.ip()) {
            let addrs = self.dns.lookup(&domain).await?;
            if let Some(ip) = addrs.first() {
                return Ok(SocketAddr::new(*ip, destination.port()));
            }
        }
        Ok(destination)
    }
}

#[async_trait]
impl Outbound for DnsOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_DNS
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(&self, destination: SocketAddr) -> Result<ProxyConn, BoxError> {
        let dest = self.resolve(destination).await?;
        Ok(tcp_stream(TcpStream::connect(dest).await?))
    }
    async fn dial_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let dest = self.resolve(destination).await?;
        Ok(ProxyUdpSocket::from_io(Arc::new(UdpDial {
            socket: UdpSocket::bind("0.0.0.0:0").await?,
            dest,
        })))
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

struct UdpDial {
    socket: UdpSocket,
    dest: SocketAddr,
}

#[async_trait]
impl rsb_core::ProxyUdpIo for UdpDial {
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize> {
        self.socket.send_to(buf, target).await
    }
    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        self.socket.recv_from(buf).await
    }
    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }
}
