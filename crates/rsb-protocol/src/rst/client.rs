//! RST outbound — QUIC ALPN h3, HTTP/3 auth, Brutal CC, optional UDP obfs.

use super::{auth, obfs, obfs_socket, protocol, quic, udp_client};
use anyhow::{Context, Result};
use async_trait::async_trait;
use h3_quinn::Connection as H3QuinnConnection;
use http::{Request, StatusCode};
use quinn::Endpoint;
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket, SplitProxy};
use serde_json::Value;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::task::{Context as TaskContext, Poll};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, ReadBuf};

static SESSION_COUNTER: AtomicU32 = AtomicU32::new(1);

const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_KEEP_ALIVE: Duration = Duration::from_secs(10);
const DEFAULT_STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_SESSION_AGE: Duration = Duration::from_secs(30 * 60);

struct RstSession {
    _endpoint: Endpoint,
    connection: Arc<quinn::Connection>,
    generation: u32,
    created_at: Instant,
    _h3_keep_alive: tokio::task::JoinHandle<()>,
}

struct RstShared {
    session: tokio::sync::Mutex<Option<RstSession>>,
    connect_inflight: AtomicBool,
    connect_notify: tokio::sync::Notify,
    generation: AtomicU32,
}

pub struct RstOutbound {
    tag: String,
    server: String,
    port: u16,
    password: String,
    up_mbps: u32,
    down_mbps: u32,
    sni: Option<String>,
    insecure: bool,
    obfs: Option<Arc<obfs::RstObfs>>,
    idle_timeout: Duration,
    keep_alive_period: Duration,
    stream_open_timeout: Duration,
    max_session_age: Duration,
    use_brutal: bool,
    shared: Arc<RstShared>,
}

impl RstOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let tls = raw.get("tls");
        let password = raw
            .get("password")
            .and_then(|v| v.as_str())
            .context("rst: password required")?
            .to_string();
        let obfs = raw.get("obfs").and_then(|o| {
            if o.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
                return None;
            }
            let version = obfs::ObfsVersion::parse(o.get("version").and_then(|v| v.as_u64()));
            o.get("password")
                .and_then(|v| v.as_str())
                .or(Some(password.as_str()))
                .map(|p| Arc::new(obfs::RstObfs::with_version(p, version)))
        });
        let use_brutal = raw
            .get("brutal")
            .or_else(|| raw.get("use_brutal"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Ok(Self {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("rst: server required")?
                .to_string(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("rst: server_port required")? as u16,
            password,
            up_mbps: raw.get("up_mbps").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            down_mbps: raw.get("down_mbps").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            sni: tls
                .and_then(|t| t.get("server_name"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            insecure: tls
                .and_then(|t| t.get("insecure"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            obfs,
            idle_timeout: raw
                .get("idle_timeout")
                .and_then(|v| v.as_str())
                .and_then(crate::duration::parse_duration_str)
                .unwrap_or(DEFAULT_IDLE_TIMEOUT),
            keep_alive_period: raw
                .get("keep_alive_period")
                .and_then(|v| v.as_str())
                .and_then(crate::duration::parse_duration_str)
                .unwrap_or(DEFAULT_KEEP_ALIVE),
            stream_open_timeout: raw
                .get("stream_open_timeout")
                .and_then(|v| v.as_str())
                .and_then(crate::duration::parse_duration_str)
                .unwrap_or(DEFAULT_STREAM_OPEN_TIMEOUT),
            max_session_age: raw
                .get("max_session_age")
                .and_then(|v| v.as_str())
                .and_then(crate::duration::parse_duration_str)
                .unwrap_or(DEFAULT_MAX_SESSION_AGE),
            use_brutal,
            shared: Arc::new(RstShared {
                session: tokio::sync::Mutex::new(None),
                connect_inflight: AtomicBool::new(false),
                connect_notify: tokio::sync::Notify::new(),
                generation: AtomicU32::new(0),
            }),
        })
    }

    async fn get_connection(&self) -> Result<Arc<quinn::Connection>> {
        {
            let guard = self.shared.session.lock().await;
            if let Some(s) = guard.as_ref() {
                if s.created_at.elapsed() < self.max_session_age
                    && s.connection.close_reason().is_none()
                {
                    return Ok(s.connection.clone());
                }
            }
        }
        self.establish().await?;
        let guard = self.shared.session.lock().await;
        guard
            .as_ref()
            .map(|s| s.connection.clone())
            .ok_or_else(|| anyhow::anyhow!("rst: session lost after establish"))
    }

    async fn establish(&self) -> Result<()> {
        loop {
            if self
                .shared
                .connect_inflight
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
            self.shared.connect_notify.notified().await;
            let guard = self.shared.session.lock().await;
            if let Some(s) = guard.as_ref() {
                if s.created_at.elapsed() < self.max_session_age
                    && s.connection.close_reason().is_none()
                {
                    return Ok(());
                }
            }
        }
        struct Guard<'a>(&'a RstShared);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.connect_inflight.store(false, Ordering::SeqCst);
                self.0.connect_notify.notify_waiters();
            }
        }
        let _g = Guard(&self.shared);

        {
            let guard = self.shared.session.lock().await;
            if let Some(s) = guard.as_ref() {
                if s.created_at.elapsed() < self.max_session_age
                    && s.connection.close_reason().is_none()
                {
                    return Ok(());
                }
            }
        }

        let addr = tokio::net::lookup_host((self.server.as_str(), self.port))
            .await
            .context("resolve rst server")?
            .next()
            .context("no rst server address")?;

        let tls = quic::client_tls(self.insecure);
        let client_cfg = quic::build_client_config(
            tls,
            self.up_mbps,
            self.down_mbps,
            self.idle_timeout,
            self.keep_alive_period,
            self.use_brutal,
        )?;

        let endpoint = if let Some(obfs) = self.obfs.clone() {
            let (mut ep, _) = obfs_socket::endpoint_with_obfs("0.0.0.0:0".parse()?, obfs)?;
            ep.set_default_client_config(client_cfg);
            ep
        } else {
            let mut ep = Endpoint::client("0.0.0.0:0".parse()?)?;
            ep.set_default_client_config(client_cfg);
            ep
        };

        let sni = self.sni.clone().unwrap_or_else(|| self.server.clone());
        let connect_fut = endpoint.connect(addr, &sni)?;
        let connection = Arc::new(
            tokio::time::timeout(self.stream_open_timeout, connect_fut)
                .await
                .context("rst: quic connect timeout")?
                .context("quic connect")?,
        );

        let h3_keep_alive = self.authenticate(&connection).await?;
        let generation = self.shared.generation.fetch_add(1, Ordering::Relaxed) + 1;

        tracing::info!(
            tag = %self.tag,
            server = %self.server,
            port = self.port,
            generation,
            brutal = self.use_brutal,
            obfs = self.obfs.is_some(),
            "rst: session established (h3/quic)"
        );

        let conn_watch = connection.clone();
        let shared = self.shared.clone();
        let gen = generation;
        tokio::spawn(async move {
            let _ = conn_watch.closed().await;
            let mut guard = shared.session.lock().await;
            if guard.as_ref().map(|s| s.generation) == Some(gen) {
                *guard = None;
            }
        });

        *self.shared.session.lock().await = Some(RstSession {
            _endpoint: endpoint,
            connection,
            generation,
            created_at: Instant::now(),
            _h3_keep_alive: h3_keep_alive,
        });
        Ok(())
    }

    async fn authenticate(
        &self,
        connection: &quinn::Connection,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let h3_conn = H3QuinnConnection::new(connection.clone());
        let (mut driver, mut send_request) = h3::client::new(h3_conn).await.context("h3 client")?;

        let mut req = Request::builder()
            .method("POST")
            .uri(format!("https://{}/auth", auth::AUTH_AUTHORITY))
            .header("rst-auth", &self.password);
        if self.down_mbps > 0 {
            req = req.header(
                "rst-cc-rx",
                (self.down_mbps as u64 * auth::MBPS_TO_BPS).to_string(),
            );
        }
        let padding = auth::random_padding(64, 512);
        req = req.header("rst-padding", padding.as_str());

        let auth_fut = async {
            let mut stream = send_request.send_request(req.body(())?).await?;
            stream.finish().await?;
            let resp = stream.recv_response().await?;
            if resp.status() != StatusCode::from_u16(233).unwrap() {
                anyhow::bail!("rst auth failed: {}", resp.status());
            }
            Ok::<(), anyhow::Error>(())
        };

        tokio::pin!(auth_fut);
        loop {
            tokio::select! {
                result = &mut auth_fut => {
                    result?;
                    std::mem::forget(driver);
                    return Ok(tokio::spawn(async {
                        std::future::pending::<()>().await
                    }));
                }
                closed = std::future::poll_fn(|cx| driver.poll_close(cx)) => {
                    anyhow::bail!("rst: h3 connection closed during auth: {closed:?}");
                }
            }
        }
    }

    async fn open_bi_with_timeout(
        &self,
        conn: &quinn::Connection,
    ) -> Result<(quinn::SendStream, quinn::RecvStream)> {
        match tokio::time::timeout(self.stream_open_timeout, conn.open_bi()).await {
            Ok(Ok(streams)) => Ok(streams),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => anyhow::bail!("rst: open stream timeout"),
        }
    }

    async fn dial_tcp_inner(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        let conn = self.get_connection().await?;
        let (mut send, mut recv) = self
            .open_bi_with_timeout(&conn)
            .await
            .context("open rst stream")?;

        let target = if !destination.ip().is_unspecified() {
            format_address(destination)
        } else if let Some(domain) = domain {
            format!("{}:{}", domain, destination.port())
        } else {
            format_address(destination)
        };

        let padding_len = auth::random_padding_len(64, 512);
        let req = protocol::encode_tcp_request(&target, padding_len);
        send.write_all(&req).await?;

        let (ok, prefix) = read_tcp_response(&mut recv).await?;
        if !ok {
            anyhow::bail!("rst tcp request rejected");
        }
        let reader: Box<dyn AsyncRead + Send + Unpin> = if prefix.is_empty() {
            Box::new(recv)
        } else {
            Box::new(PrefixedReader::new(recv, prefix))
        };
        Ok(Box::new(SplitProxy::new(reader, send)))
    }

    async fn reset_session(&self) {
        let mut guard = self.shared.session.lock().await;
        if let Some(s) = guard.take() {
            s.connection.close(0u32.into(), b"rst reset");
        }
        self.shared.generation.fetch_add(1, Ordering::Relaxed);
    }
}

async fn read_tcp_response(recv: &mut quinn::RecvStream) -> Result<(bool, Vec<u8>)> {
    let mut buf = bytes::BytesMut::with_capacity(256);
    let mut chunk = [0u8; 512];
    loop {
        if !buf.is_empty() {
            if let Some((ok, _msg)) = protocol::try_decode_tcp_response(&mut buf)? {
                return Ok((ok, buf.to_vec()));
            }
        }
        let n = match tokio::time::timeout(Duration::from_secs(10), recv.read(&mut chunk)).await {
            Ok(Ok(Some(n))) => n,
            Ok(Ok(None)) => anyhow::bail!("rst: stream closed before tcp response"),
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => anyhow::bail!("rst: read tcp response timeout"),
        };
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > 8192 {
            anyhow::bail!("rst: tcp response too large");
        }
    }
}

struct PrefixedReader {
    inner: quinn::RecvStream,
    prefix: Vec<u8>,
    pos: usize,
}

impl PrefixedReader {
    fn new(inner: quinn::RecvStream, prefix: Vec<u8>) -> Self {
        Self {
            inner,
            prefix,
            pos: 0,
        }
    }
}

impl AsyncRead for PrefixedReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.pos < self.prefix.len() {
            let n = buf.remaining().min(self.prefix.len() - self.pos);
            buf.put_slice(&self.prefix[self.pos..self.pos + n]);
            self.pos += n;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

fn format_address(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v4) => format!("{}:{}", v4.ip(), v4.port()),
        SocketAddr::V6(v6) => format!("[{}]:{}", v6.ip(), v6.port()),
    }
}

#[async_trait]
impl Outbound for RstOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        rsb_constant::TYPE_RST
    }

    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }

    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        match self.dial_tcp_inner(destination, domain).await {
            Ok(c) => Ok(c),
            Err(err) => {
                let msg = err.to_string().to_lowercase();
                if msg.contains("auth")
                    || msg.contains("quic")
                    || msg.contains("h3")
                    || msg.contains("closed")
                    || msg.contains("reset")
                {
                    self.reset_session().await;
                    Ok(self.dial_tcp_inner(destination, domain).await?)
                } else {
                    Err(err)
                }
            }
        }
    }

    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let conn = self.get_connection().await?;
        let session_id = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        Ok(udp_client::rst_udp_socket(conn, session_id))
    }

    async fn close(&self) -> Result<(), BoxError> {
        self.reset_session().await;
        Ok(())
    }
}
