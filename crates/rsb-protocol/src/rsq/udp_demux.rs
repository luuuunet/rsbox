//! Per-connection UDP datagram demultiplexer (one read_datagram loop).

use super::protocol;
use super::udp_fragment::{self, UdpReassembler};
use dashmap::DashMap;
use quinn::Connection;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Per-session inbound queue; excess packets are dropped (not buffered without bound).
const UDP_DEMUX_QUEUE_CAP: usize = 128;

pub struct UdpDemux {
    sessions: DashMap<u32, mpsc::Sender<(Vec<u8>, SocketAddr)>>,
    started: AtomicBool,
}

impl UdpDemux {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            sessions: DashMap::new(),
            started: AtomicBool::new(false),
        })
    }

    pub fn register(&self, session_id: u32) -> mpsc::Receiver<(Vec<u8>, SocketAddr)> {
        let (tx, rx) = mpsc::channel(UDP_DEMUX_QUEUE_CAP);
        self.sessions.insert(session_id, tx);
        rx
    }

    pub fn unregister(&self, session_id: u32) {
        self.sessions.remove(&session_id);
    }

    pub fn clear(&self) {
        self.sessions.clear();
    }

    pub fn ensure_reader(self: &Arc<Self>, conn: Arc<Connection>) {
        if self
            .started
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        let demux = self.clone();
        tokio::spawn(async move {
            demux.read_loop(conn).await;
        });
    }

    async fn read_loop(self: Arc<Self>, conn: Arc<Connection>) {
        let mut reassembler = UdpReassembler::new();
        loop {
            match conn.read_datagram().await {
                Ok(data) => {
                    let mut cursor = &data[..];
                    let Ok(msg) = protocol::UdpMessage::decode(&mut cursor) else {
                        continue;
                    };
                    if udp_fragment::is_udp_session_close(&msg) {
                        self.unregister(msg.session_id);
                        continue;
                    }
                    if udp_fragment::ensure_fragment_ready(&msg).is_err() {
                        continue;
                    }
                    match reassembler.ingest(msg) {
                        Ok(Some(complete)) => {
                            let Ok(addr) = parse_addr(&complete.addr) else {
                                continue;
                            };
                            if let Some(tx) = self.sessions.get(&complete.session_id) {
                                match tx.try_send((complete.payload, addr)) {
                                    Ok(()) => {}
                                    Err(mpsc::error::TrySendError::Full(_)) => {
                                        tracing::debug!(
                                            session_id = complete.session_id,
                                            "rsq udp demux queue full, drop packet"
                                        );
                                    }
                                    Err(mpsc::error::TrySendError::Closed(_)) => {}
                                }
                            }
                        }
                        Ok(None) => {
                            reassembler.prune_expired();
                            reassembler.prune_stale(256);
                        }
                        Err(_) => {
                            reassembler.prune_expired();
                        }
                    }
                }
                Err(_) => break,
            }
        }
        self.clear();
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
