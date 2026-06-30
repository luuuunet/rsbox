//! sing-box compatible AnyTLS outbound (via anytls-rs).

use crate::duration::parse_duration_str;
use anyhow::{Context, Result};
use anytls_rs::client::{Client, SessionPoolConfig};
use anytls_rs::padding::PaddingFactory;
use async_trait::async_trait;
use bytes::Bytes;
use rsb_core::{proxy_box, BoxError, Network, Outbound, ProxyConn, ProxyUdpIo, ProxyUdpSocket};
use rustls::pki_types::ServerName;
use serde_json::Value;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::TlsConnector;

pub struct AnyTlsOutbound {
    tag: String,
    client: Arc<Client>,
}

impl AnyTlsOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let tls = raw.get("tls").context("anytls: tls required")?;
        if !tls.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true) {
            anyhow::bail!("anytls: tls.enabled is required");
        }
        let server = raw
            .get("server")
            .and_then(|v| v.as_str())
            .context("anytls: server required")?;
        let port = raw
            .get("server_port")
            .and_then(|v| v.as_u64())
            .context("anytls: server_port required")? as u16;
        let password = raw
            .get("password")
            .and_then(|v| v.as_str())
            .context("anytls: password required")?;

        let sni = tls
            .get("server_name")
            .and_then(|v| v.as_str())
            .unwrap_or(server);
        let insecure = tls
            .get("insecure")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let tls_cfg = crate::transport::build_tls_config(Some(tls), insecure)?;
        let tls_connector = Arc::new(TlsConnector::from(tls_cfg));
        let server_name = ServerName::try_from(sni)
            .map_err(|_| anyhow::anyhow!("anytls: invalid tls.server_name: {sni}"))?
            .to_owned();

        let pool_config = SessionPoolConfig {
            check_interval: raw
                .get("idle_session_check_interval")
                .and_then(|v| v.as_str())
                .and_then(parse_duration_str)
                .unwrap_or_else(|| std::time::Duration::from_secs(30)),
            idle_timeout: raw
                .get("idle_session_timeout")
                .and_then(|v| v.as_str())
                .and_then(parse_duration_str)
                .unwrap_or_else(|| std::time::Duration::from_secs(30)),
            min_idle_sessions: raw
                .get("min_idle_session")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
        };

        let padding = PaddingFactory::default();
        let client = Arc::new(Client::with_pool_config(
            password,
            format!("{server}:{port}"),
            server_name,
            tls_connector,
            padding,
            pool_config,
        ));

        Ok(Self { tag, client })
    }

    fn destination_host(destination: SocketAddr, domain: Option<&str>) -> (String, u16) {
        if let Some(host) = domain {
            return (host.to_string(), destination.port());
        }
        (destination.ip().to_string(), destination.port())
    }
}

struct AnyTlsStream {
    stream: Arc<anytls_rs::session::Stream>,
    _session: Arc<anytls_rs::session::Session>,
}

impl AsyncRead for AnyTlsStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let reader = self.stream.reader().clone();
        let remaining = buf.remaining();
        let mut read_fut = Box::pin(async move {
            let mut guard = reader.lock().await;
            let mut temp = vec![0u8; remaining];
            let n = guard.read(&mut temp).await?;
            Ok::<_, std::io::Error>((n, temp))
        });
        match std::future::Future::poll(read_fut.as_mut(), cx) {
            Poll::Ready(Ok((0, _))) => Poll::Ready(Ok(())),
            Poll::Ready(Ok((n, temp))) => {
                buf.put_slice(&temp[..n]);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for AnyTlsStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.stream.is_closed() {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "anytls stream closed",
            )));
        }
        match self.stream.send_data(Bytes::copy_from_slice(buf)) {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(_) => Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "anytls session channel closed",
            ))),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

struct AnyTlsUdp {
    stream: Arc<anytls_rs::session::Stream>,
    _session: Arc<anytls_rs::session::Session>,
    target: SocketAddr,
    rx: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<(Vec<u8>, SocketAddr)>>,
}

#[async_trait]
impl ProxyUdpIo for AnyTlsUdp {
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize> {
        let _ = target;
        let mut frame = Vec::with_capacity(2 + buf.len());
        frame.extend_from_slice(&(buf.len() as u16).to_be_bytes());
        frame.extend_from_slice(buf);
        self.stream
            .send_data(Bytes::from(frame))
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "anytls udp send"))?;
        Ok(buf.len())
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let mut rx = self.rx.lock().await;
        let (payload, addr) = rx
            .recv()
            .await
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "anytls udp closed"))?;
        let n = payload.len().min(buf.len());
        buf[..n].copy_from_slice(&payload[..n]);
        Ok((n, addr))
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        Ok(self.target)
    }
}

async fn open_udp_stream(
    client: &Client,
    target: SocketAddr,
) -> Result<(Arc<anytls_rs::session::Stream>, Arc<anytls_rs::session::Session>)> {
    use anytls_rs::client::UDP_OVER_TCP_MAGIC_ADDR;
    use bytes::BufMut;

    let (stream, session) = client
        .create_proxy_stream((UDP_OVER_TCP_MAGIC_ADDR.to_string(), 0))
        .await
        .map_err(|e| anyhow::anyhow!("anytls udp tunnel: {e}"))?;

    let mut req = bytes::BytesMut::new();
    req.put_u8(1);
    match target {
        SocketAddr::V4(v4) => {
            req.put_u8(0x01);
            req.put_slice(&v4.ip().octets());
            req.put_u16(v4.port());
        }
        SocketAddr::V6(v6) => {
            req.put_u8(0x04);
            req.put_slice(&v6.ip().octets());
            req.put_u16(v6.port());
        }
    }
    stream
        .send_data(req.freeze())
        .map_err(|_| anyhow::anyhow!("anytls udp initial request failed"))?;

    Ok((stream, session))
}

#[async_trait]
impl Outbound for AnyTlsOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_ANYTLS
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        let dest = Self::destination_host(destination, domain);
        let (stream, session) = self
            .client
            .create_proxy_stream(dest)
            .await
            .map_err(|e| anyhow::anyhow!("anytls connect: {e}"))?;
        Ok(proxy_box(AnyTlsStream {
            stream,
            _session: session,
        }))
    }
    async fn dial_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let (stream, session) = open_udp_stream(&self.client, destination).await?;
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let stream_reader = stream.clone();
        let target = destination;
        tokio::spawn(async move {
            let reader = stream_reader.reader();
            loop {
                let packet = {
                    let mut guard = reader.lock().await;
                    match read_udp_packet(&mut guard).await {
                        Ok(p) => p,
                        Err(_) => break,
                    }
                };
                if tx.send((packet, target)).is_err() {
                    break;
                }
            }
        });
        Ok(ProxyUdpSocket::from_io(Arc::new(AnyTlsUdp {
            stream,
            _session: session,
            target: destination,
            rx: tokio::sync::Mutex::new(rx),
        })))
    }
    async fn close(&self) -> Result<(), BoxError> {
        self.client.stop_session_pool_cleanup().await;
        Ok(())
    }
}

async fn read_udp_packet(
    reader: &mut anytls_rs::session::StreamReader,
) -> std::io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 2];
    reader.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).await?;
    Ok(payload)
}
