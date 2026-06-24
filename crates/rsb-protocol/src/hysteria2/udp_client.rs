use super::protocol::{self, UdpMessage};
use async_trait::async_trait;
use bytes::BytesMut;
use quinn::Connection;
use rsb_core::{ProxyUdpIo, ProxyUdpSocket};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

pub fn hy2_udp_socket(conn: Arc<Connection>, session_id: u32) -> ProxyUdpSocket {
    let (tx, rx) = mpsc::unbounded_channel();
    let conn_reader = conn.clone();
    tokio::spawn(async move {
        loop {
            match conn_reader.read_datagram().await {
                Ok(data) => {
                    let mut cursor = &data[..];
                    if let Ok(msg) = protocol::UdpMessage::decode(&mut cursor) {
                        if msg.session_id == session_id {
                            if let Ok(addr) = parse_addr(&msg.addr) {
                                let _ = tx.send((msg.payload, addr));
                            }
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
    ProxyUdpSocket::from_io(Arc::new(Hy2UdpIo {
        conn,
        session_id,
        rx: tokio::sync::Mutex::new(rx),
    }))
}

struct Hy2UdpIo {
    conn: Arc<Connection>,
    session_id: u32,
    rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<(Vec<u8>, SocketAddr)>>,
}

#[async_trait]
impl ProxyUdpIo for Hy2UdpIo {
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize> {
        let addr = format_address(target);
        let mut out = BytesMut::new();
        UdpMessage {
            session_id: self.session_id,
            packet_id: 0,
            fragment_id: 0,
            fragment_count: 1,
            addr,
            payload: buf.to_vec(),
        }
        .encode(&mut out);
        self.conn
            .send_datagram(out.freeze())
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(buf.len())
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let mut guard = self.rx.lock().await;
        let (payload, addr) = guard
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "hy2 udp closed"))?;
        let n = payload.len().min(buf.len());
        buf[..n].copy_from_slice(&payload[..n]);
        Ok((n, addr))
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        Ok("0.0.0.0:0".parse().unwrap())
    }
}

fn format_address(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v4) => format!("{}:{}", v4.ip(), v4.port()),
        SocketAddr::V6(v6) => format!("[{}]:{}", v6.ip(), v6.port()),
    }
}

fn parse_addr(s: &str) -> std::io::Result<SocketAddr> {
    if let Ok(a) = s.parse() {
        return Ok(a);
    }
    std::net::ToSocketAddrs::to_socket_addrs(s)?
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "resolve udp addr"))
}
