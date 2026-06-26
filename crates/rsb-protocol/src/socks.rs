use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{tcp_stream, BoxError, Network, Outbound, ProxyConn, ProxyUdpIo, ProxyUdpSocket};
use serde_json::Value;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::net::{TcpStream, UdpSocket};

pub struct SocksOutbound {
    tag: String,
    server: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
}

impl SocksOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let server = raw
            .get("server")
            .and_then(|v| v.as_str())
            .context("socks outbound: missing server")?
            .to_string();
        let port = raw
            .get("server_port")
            .and_then(|v| v.as_u64())
            .context("socks outbound: missing server_port")? as u16;
        Ok(Self {
            tag,
            server,
            port,
            username: raw
                .get("username")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            password: raw
                .get("password")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        })
    }

    async fn connect_proxy(&self) -> Result<TcpStream> {
        let addr: SocketAddr = format!("{}:{}", self.server, self.port)
            .parse()
            .context("parse socks server")?;
        TcpStream::connect(addr)
            .await
            .context("connect socks server")
    }
}

#[async_trait]
impl Outbound for SocksOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_SOCKS
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }

    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        _domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        let mut stream = self.connect_proxy().await?;
        socks::socks5_connect(
            &mut stream,
            destination,
            self.username.as_deref(),
            self.password.as_deref(),
        )
        .await?;
        Ok(tcp_stream(stream))
    }

    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let mut stream = self.connect_proxy().await?;
        socks::socks5_auth(
            &mut stream,
            self.username.as_deref(),
            self.password.as_deref(),
        )
        .await?;
        let relay = socks::socks5_udp_associate(&mut stream).await?;
        let udp = UdpSocket::bind("0.0.0.0:0").await?;
        Ok(ProxyUdpSocket::from_io(Arc::new(SocksUdpIo {
            udp,
            relay,
            _tcp: stream,
        })))
    }

    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

struct SocksUdpIo {
    udp: UdpSocket,
    relay: SocketAddr,
    _tcp: TcpStream,
}

#[async_trait]
impl ProxyUdpIo for SocksUdpIo {
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize> {
        let packet = encode_socks_udp_packet(buf, target);
        self.udp.send_to(&packet, self.relay).await?;
        Ok(buf.len())
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let mut packet = vec![0u8; 65536];
        let (n, _) = self.udp.recv_from(&mut packet).await?;
        decode_socks_udp_packet(&packet[..n], buf)
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.udp.local_addr()
    }
}

fn encode_socks_udp_packet(payload: &[u8], target: SocketAddr) -> Vec<u8> {
    let mut packet = Vec::with_capacity(10 + payload.len());
    packet.extend_from_slice(&[0x00, 0x00, 0x00]); // RSV + FRAG
    match target {
        SocketAddr::V4(v4) => {
            packet.push(0x01);
            packet.extend_from_slice(&v4.ip().octets());
            packet.extend_from_slice(&v4.port().to_be_bytes());
        },
        SocketAddr::V6(v6) => {
            packet.push(0x04);
            packet.extend_from_slice(&v6.ip().octets());
            packet.extend_from_slice(&v6.port().to_be_bytes());
        },
    }
    packet.extend_from_slice(payload);
    packet
}

fn decode_socks_udp_packet(packet: &[u8], buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
    if packet.len() < 4 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "truncated socks udp packet",
        ));
    }
    let atyp = packet[3];
    let (addr, off) = match atyp {
        0x01 => {
            if packet.len() < 10 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "truncated socks udp ipv4",
                ));
            }
            let ip = Ipv4Addr::new(packet[4], packet[5], packet[6], packet[7]);
            let port = u16::from_be_bytes([packet[8], packet[9]]);
            (SocketAddr::from((ip, port)), 10)
        },
        0x03 => {
            let len = packet[4] as usize;
            if packet.len() < 5 + len + 2 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "truncated socks udp domain",
                ));
            }
            let host = std::str::from_utf8(&packet[5..5 + len])
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let port = u16::from_be_bytes([packet[5 + len], packet[5 + len + 1]]);
            let mut addrs = std::net::ToSocketAddrs::to_socket_addrs(&(host, port))?;
            let addr = addrs
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "resolve host"))?;
            (addr, 5 + len + 2)
        },
        0x04 => {
            if packet.len() < 22 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "truncated socks udp ipv6",
                ));
            }
            let ip = Ipv6Addr::from(<[u8; 16]>::try_from(&packet[4..20]).unwrap());
            let port = u16::from_be_bytes([packet[20], packet[21]]);
            (SocketAddr::from((ip, port)), 22)
        },
        other => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported socks udp atyp {other}"),
            ));
        },
    };
    let payload = &packet[off..];
    let n = payload.len().min(buf.len());
    buf[..n].copy_from_slice(&payload[..n]);
    Ok((n, addr))
}

pub mod socks {
    use anyhow::{Context, Result};
    use std::net::SocketAddr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    pub async fn socks5_auth(
        stream: &mut TcpStream,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<()> {
        let auth_method = if username.is_some() { 0x02u8 } else { 0x00u8 };
        stream.write_all(&[0x05, 0x01, auth_method]).await?;
        let mut resp = [0u8; 2];
        stream.read_exact(&mut resp).await?;
        if resp[0] != 0x05 {
            anyhow::bail!("invalid socks version");
        }
        if resp[1] == 0x02 {
            let user = username.unwrap_or("");
            let pass = password.unwrap_or("");
            let mut auth = vec![0x01, user.len() as u8];
            auth.extend_from_slice(user.as_bytes());
            auth.push(pass.len() as u8);
            auth.extend_from_slice(pass.as_bytes());
            stream.write_all(&auth).await?;
            let mut auth_resp = [0u8; 2];
            stream.read_exact(&mut auth_resp).await?;
            if auth_resp[1] != 0x00 {
                anyhow::bail!("socks auth failed");
            }
        } else if resp[1] != 0x00 {
            anyhow::bail!("unsupported socks auth method");
        }
        Ok(())
    }

    pub async fn socks5_udp_associate(stream: &mut TcpStream) -> Result<SocketAddr> {
        let req = [0x05u8, 0x03, 0x00, 0x01, 0, 0, 0, 0, 0, 0];
        stream.write_all(&req).await?;
        read_socks_bind_addr(stream).await
    }

    pub async fn socks5_connect(
        stream: &mut TcpStream,
        destination: SocketAddr,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<()> {
        socks5_auth(stream, username, password).await?;
        let (host, port) = match destination {
            SocketAddr::V4(v4) => (format!("{}", v4.ip()), v4.port()),
            SocketAddr::V6(v6) => (format!("{}", v6.ip()), v6.port()),
        };
        let mut req = vec![0x05, 0x01, 0x00, 0x03, host.len() as u8];
        req.extend_from_slice(host.as_bytes());
        req.extend_from_slice(&port.to_be_bytes());
        stream.write_all(&req).await?;
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await?;
        if header[1] != 0x00 {
            anyhow::bail!("socks connect failed: {}", header[1]);
        }
        skip_socks_addr(stream, header[3]).await
    }

    async fn read_socks_bind_addr(stream: &mut TcpStream) -> Result<SocketAddr> {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await?;
        if header[1] != 0x00 {
            anyhow::bail!("socks udp associate failed: {}", header[1]);
        }
        match header[3] {
            0x01 => {
                let mut rest = [0u8; 6];
                stream.read_exact(&mut rest).await?;
                let ip = std::net::Ipv4Addr::new(rest[0], rest[1], rest[2], rest[3]);
                let port = u16::from_be_bytes([rest[4], rest[5]]);
                Ok(SocketAddr::from((ip, port)))
            },
            0x03 => {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await?;
                let mut domain = vec![0u8; len[0] as usize];
                stream.read_exact(&mut domain).await?;
                let mut port = [0u8; 2];
                stream.read_exact(&mut port).await?;
                let host = std::str::from_utf8(&domain)?;
                let port = u16::from_be_bytes(port);
                let mut addrs = tokio::net::lookup_host(format!("{host}:{port}")).await?;
                addrs
                    .next()
                    .with_context(|| format!("resolve socks relay {host}:{port}"))
            },
            0x04 => {
                let mut rest = [0u8; 18];
                stream.read_exact(&mut rest).await?;
                let ip = std::net::Ipv6Addr::from(<[u8; 16]>::try_from(&rest[..16]).unwrap());
                let port = u16::from_be_bytes([rest[16], rest[17]]);
                Ok(SocketAddr::from((ip, port)))
            },
            _ => anyhow::bail!("invalid socks address type"),
        }
    }

    async fn skip_socks_addr(stream: &mut TcpStream, atyp: u8) -> Result<()> {
        match atyp {
            0x01 => {
                let mut rest = [0u8; 6];
                stream.read_exact(&mut rest).await?;
            },
            0x03 => {
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await?;
                let mut rest = vec![0u8; len[0] as usize + 2];
                stream.read_exact(&mut rest).await?;
            },
            0x04 => {
                let mut rest = [0u8; 18];
                stream.read_exact(&mut rest).await?;
            },
            _ => anyhow::bail!("invalid socks address type"),
        }
        Ok(())
    }
}
