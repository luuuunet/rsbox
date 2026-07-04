use super::udp_demux::UdpDemux;
use super::udp_fragment;
use async_trait::async_trait;
use quinn::Connection;
use rsb_core::{ProxyUdpIo, ProxyUdpSocket};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use tokio::sync::mpsc;

pub fn rsq_udp_socket(
    conn: Arc<Connection>,
    demux: Arc<UdpDemux>,
    session_id: u32,
) -> ProxyUdpSocket {
    let rx = demux.register(session_id);
    ProxyUdpSocket::from_io(Arc::new(RsqUdpIo {
        conn,
        demux: demux.clone(),
        session_id,
        rx: tokio::sync::Mutex::new(rx),
        packet_id: AtomicU16::new(0),
    }))
}

struct RsqUdpIo {
    conn: Arc<Connection>,
    demux: Arc<UdpDemux>,
    session_id: u32,
    rx: tokio::sync::Mutex<mpsc::Receiver<(Vec<u8>, SocketAddr)>>,
    packet_id: AtomicU16,
}

#[async_trait]
impl ProxyUdpIo for RsqUdpIo {
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize> {
        let addr = format_address(target);
        let packet_id = self.packet_id.fetch_add(1, Ordering::Relaxed);
        let frames = udp_fragment::fragment_payload(self.session_id, packet_id, &addr, buf)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        for (i, frame) in frames.iter().enumerate() {
            self.conn
                .send_datagram(frame.clone().freeze())
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            if i + 1 < frames.len() {
                tokio::task::yield_now().await;
            }
        }
        Ok(buf.len())
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let mut guard = self.rx.lock().await;
        let (payload, addr) = guard
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "rsq udp closed"))?;
        let n = payload.len().min(buf.len());
        buf[..n].copy_from_slice(&payload[..n]);
        Ok((n, addr))
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        Ok("0.0.0.0:0".parse().unwrap())
    }
}

impl Drop for RsqUdpIo {
    fn drop(&mut self) {
        let close = udp_fragment::encode_udp_session_close(self.session_id);
        let _ = self.conn.send_datagram(close.freeze());
        self.demux.unregister(self.session_id);
    }
}

fn format_address(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v4) => format!("{}:{}", v4.ip(), v4.port()),
        SocketAddr::V6(v6) => format!("[{}]:{}", v6.ip(), v6.port()),
    }
}
