//! Salamander obfs wrapper for quinn `AsyncUdpSocket`.

use super::obfs::Salamander;
use quinn::udp::{self, RecvMeta, Transmit};
use quinn::{AsyncUdpSocket, UdpPoller};
use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::{pin, Pin};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use tokio::io::Interest;

pub fn endpoint_with_obfs(
    bind: SocketAddr,
    obfs: Arc<Salamander>,
) -> io::Result<(quinn::Endpoint, SocketAddr)> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let runtime = quinn::default_runtime().ok_or_else(|| io::Error::other("no async runtime"))?;
    let std_sock = std::net::UdpSocket::bind(bind)?;
    if bind.is_ipv6() {
        let _ = std_sock.set_nonblocking(true);
    }
    let local = std_sock.local_addr()?;
    let io = tokio::net::UdpSocket::from_std(std_sock)?;
    let inner = udp::UdpSocketState::new((&io).into())?;
    let socket: Arc<dyn AsyncUdpSocket> = Arc::new(ObfsUdpSocket {
        io,
        inner,
        obfs,
        send_scratch: Mutex::new(Vec::new()),
    });
    let ep = quinn::Endpoint::new_with_abstract_socket(
        quinn::EndpointConfig::default(),
        None,
        socket,
        runtime,
    )?;
    Ok((ep, local))
}

pub fn endpoint_with_obfs_server(
    bind: SocketAddr,
    server_config: quinn::ServerConfig,
    obfs: Arc<Salamander>,
) -> io::Result<quinn::Endpoint> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let runtime = quinn::default_runtime().ok_or_else(|| io::Error::other("no async runtime"))?;
    let std_sock = std::net::UdpSocket::bind(bind)?;
    let io = tokio::net::UdpSocket::from_std(std_sock)?;
    let inner = udp::UdpSocketState::new((&io).into())?;
    let socket: Arc<dyn AsyncUdpSocket> = Arc::new(ObfsUdpSocket {
        io,
        inner,
        obfs,
        send_scratch: Mutex::new(Vec::new()),
    });
    quinn::Endpoint::new_with_abstract_socket(
        quinn::EndpointConfig::default(),
        Some(server_config),
        socket,
        runtime,
    )
}

struct ObfsUdpSocket {
    io: tokio::net::UdpSocket,
    inner: udp::UdpSocketState,
    obfs: Arc<Salamander>,
    send_scratch: Mutex<Vec<u8>>,
}

impl Debug for ObfsUdpSocket {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObfsUdpSocket").finish_non_exhaustive()
    }
}

impl AsyncUdpSocket for ObfsUdpSocket {
    fn create_io_poller(self: Arc<Self>) -> Pin<Box<dyn UdpPoller>> {
        Box::pin(ObfsUdpPoller {
            socket: self,
            fut: None,
        })
    }

    fn try_send(&self, transmit: &Transmit) -> io::Result<()> {
        let mut encoded = self.send_scratch.lock().unwrap();
        encoded.clear();
        self.obfs.encode(transmit.contents, &mut encoded);
        let modified = Transmit {
            destination: transmit.destination,
            ecn: transmit.ecn,
            contents: &encoded,
            segment_size: transmit.segment_size,
            src_ip: transmit.src_ip,
        };
        self.io.try_io(Interest::WRITABLE, || {
            self.inner.send((&self.io).into(), &modified)
        })
    }

    fn poll_recv(
        &self,
        cx: &mut Context,
        bufs: &mut [io::IoSliceMut<'_>],
        meta: &mut [RecvMeta],
    ) -> Poll<io::Result<usize>> {
        loop {
            if !self.io.poll_recv_ready(cx).is_ready() {
                return Poll::Pending;
            }
            match self.io.try_io(Interest::READABLE, || {
                self.inner.recv((&self.io).into(), bufs, meta)
            }) {
                Ok(count) => {
                    for i in 0..count {
                        let len = meta[i].len;
                        if len == 0 {
                            continue;
                        }
                        unsafe {
                            let slice = std::slice::from_raw_parts_mut(bufs[i].as_mut_ptr(), len);
                            if let Some(decoded) = self.obfs.decode_owned(slice) {
                                let n = decoded.len().min(slice.len());
                                slice[..n].copy_from_slice(&decoded[..n]);
                                meta[i].len = n;
                            }
                        }
                    }
                    return Poll::Ready(Ok(count));
                },
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {},
                Err(e) => return Poll::Ready(Err(e)),
            }
        }
    }

    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.io.local_addr()
    }

    fn may_fragment(&self) -> bool {
        self.inner.may_fragment()
    }

    fn max_transmit_segments(&self) -> usize {
        self.inner.max_gso_segments()
    }

    fn max_receive_segments(&self) -> usize {
        self.inner.gro_segments()
    }
}

struct ObfsUdpPoller {
    socket: Arc<ObfsUdpSocket>,
    fut: Option<Pin<Box<dyn Future<Output = io::Result<()>> + Send + Sync>>>,
}

impl Debug for ObfsUdpPoller {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObfsUdpPoller").finish_non_exhaustive()
    }
}

impl UdpPoller for ObfsUdpPoller {
    fn poll_writable(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        if self.fut.is_none() {
            let socket = self.socket.clone();
            self.fut = Some(Box::pin(async move { socket.io.writable().await }));
        }
        let result = pin!(self.fut.as_mut().unwrap()).poll(cx);
        if result.is_ready() {
            self.fut = None;
        }
        result
    }
}
