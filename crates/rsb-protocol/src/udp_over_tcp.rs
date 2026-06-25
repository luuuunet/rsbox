//! TCP-tunneled UDP relay for protocols without native UDP in this build path.
//!
//! Wire format per packet: `[atyp][host][port][len][payload]`

use async_trait::async_trait;
use rsb_core::{ProxyUdpIo, ProxyUdpSocket};
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

pub async fn tcp_tunneled_udp(connect: TcpStream) -> ProxyUdpSocket {
    tunneled_udp(connect).await
}

pub async fn tunneled_udp<S>(stream: S) -> ProxyUdpSocket
where
    S: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut reader = reader;
        loop {
            match read_frame(&mut reader).await {
                Ok((payload, addr)) => {
                    let _ = tx.send((payload, addr));
                },
                Err(_) => break,
            }
        }
    });
    ProxyUdpSocket::from_io(Arc::new(TcpTunneledUdp {
        writer: tokio::sync::Mutex::new(writer),
        rx: tokio::sync::Mutex::new(rx),
    }))
}

struct TcpTunneledUdp<W> {
    writer: tokio::sync::Mutex<W>,
    rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<(Vec<u8>, SocketAddr)>>,
}

#[async_trait]
impl<W> ProxyUdpIo for TcpTunneledUdp<W>
where
    W: AsyncWrite + Send + Sync + Unpin,
{
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize> {
        let frame = encode_frame(buf, target);
        let mut writer = self.writer.lock().await;
        writer.write_all(&frame).await?;
        Ok(buf.len())
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let mut guard = self.rx.lock().await;
        let (payload, addr) = guard
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "tcp udp closed"))?;
        let n = payload.len().min(buf.len());
        buf[..n].copy_from_slice(&payload[..n]);
        Ok((n, addr))
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        Ok("0.0.0.0:0".parse().unwrap())
    }
}

fn encode_frame(buf: &[u8], target: SocketAddr) -> Vec<u8> {
    let mut frame = Vec::with_capacity(64 + buf.len());
    match target {
        SocketAddr::V4(v4) => {
            frame.push(0x01);
            frame.extend_from_slice(&v4.ip().octets());
            frame.extend_from_slice(&v4.port().to_be_bytes());
        },
        SocketAddr::V6(v6) => {
            frame.push(0x04);
            frame.extend_from_slice(&v6.ip().octets());
            frame.extend_from_slice(&v6.port().to_be_bytes());
        },
    }
    frame.extend_from_slice(&(buf.len() as u16).to_be_bytes());
    frame.extend_from_slice(buf);
    frame
}

async fn read_frame<R>(reader: &mut R) -> std::io::Result<(Vec<u8>, SocketAddr)>
where
    R: AsyncRead + Unpin,
{
    let mut atyp = [0u8; 1];
    reader.read_exact(&mut atyp).await?;
    let addr = match atyp[0] {
        0x01 => {
            let mut ip = [0u8; 4];
            reader.read_exact(&mut ip).await?;
            let mut port = [0u8; 2];
            reader.read_exact(&mut port).await?;
            SocketAddr::from((Ipv4Addr::from(ip), u16::from_be_bytes(port)))
        },
        0x03 => {
            let mut l = [0u8; 1];
            reader.read_exact(&mut l).await?;
            let mut domain = vec![0u8; l[0] as usize];
            reader.read_exact(&mut domain).await?;
            let mut port = [0u8; 2];
            reader.read_exact(&mut port).await?;
            let host = std::str::from_utf8(&domain)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let port = u16::from_be_bytes(port);
            let mut addrs = tokio::net::lookup_host(format!("{host}:{port}")).await?;
            addrs
                .next()
                .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "resolve host"))?
        },
        0x04 => {
            let mut ip = [0u8; 16];
            reader.read_exact(&mut ip).await?;
            let mut port = [0u8; 2];
            reader.read_exact(&mut port).await?;
            SocketAddr::from((Ipv6Addr::from(ip), u16::from_be_bytes(port)))
        },
        other => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unsupported atyp {other}"),
            ));
        },
    };
    let mut len_buf = [0u8; 2];
    reader.read_exact(&mut len_buf).await?;
    let n = u16::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; n];
    reader.read_exact(&mut payload).await?;
    Ok((payload, addr))
}
