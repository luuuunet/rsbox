use super::{auth, bandwidth, control, obfs, obfs_socket, protocol, quic, stream, traffic, udp_client, udp_demux};
use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::BytesMut;
use quinn::Endpoint;
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket, SplitProxy};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

fn next_udp_session_id() -> u32 {
    let id = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed) as u32;
    if id == 0 { 1 } else { id }
}

const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_KEEP_ALIVE: Duration = Duration::from_secs(10);
const DEFAULT_PROBE_INTERVAL: Duration = Duration::from_secs(20);
const PROBE_STREAM_TIMEOUT: Duration = Duration::from_secs(4);
const DEFAULT_STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_MAX_SESSION_AGE: Duration = Duration::from_secs(30 * 60);

struct RsqSession {
    endpoint: Endpoint,
    connection: Arc<quinn::Connection>,
    generation: u32,
    created_at: Instant,
    udp_demux: Arc<udp_demux::UdpDemux>,
    udp_enabled: bool,
}

struct RsqShared {
    session: tokio::sync::Mutex<Option<Arc<RsqSession>>>,
    connect_inflight: AtomicBool,
    connect_notify: tokio::sync::Notify,
    generation: AtomicU32,
    probe_task_started: AtomicBool,
}

struct RsqOutboundInner {
    tag: String,
    server: String,
    port: u16,
    password: String,
    up_mbps: u32,
    down_mbps: u32,
    sni: Option<String>,
    insecure: bool,
    obfs: Option<Arc<obfs::RsqObfs>>,
    profile: traffic::TrafficProfile,
    idle_timeout: Duration,
    keep_alive_period: Duration,
    probe_interval: Duration,
    stream_open_timeout: Duration,
    max_session_age: Duration,
    shared: Arc<RsqShared>,
}

pub struct RsqOutbound {
    inner: Arc<RsqOutboundInner>,
}

impl RsqOutbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let tls = raw.get("tls");
        let obfs = raw.get("obfs").and_then(|o| {
            if o.get("enabled").and_then(|v| v.as_bool()) == Some(false) {
                return None;
            }
            let version = obfs::ObfsVersion::parse(o.get("version").and_then(|v| v.as_u64()));
            o.get("password")
                .and_then(|v| v.as_str())
                .or_else(|| raw.get("password").and_then(|v| v.as_str()))
                .map(|p| Arc::new(obfs::RsqObfs::with_version(p, version)))
        });
        let port = raw
            .get("server_port")
            .and_then(|v| v.as_u64())
            .context("rsq: server_port required")? as u16;
        let warm_up = raw.get("warm_up").and_then(|v| v.as_bool()).unwrap_or(false);
        let inner = Arc::new(RsqOutboundInner {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("rsq: server required")?
                .to_string(),
            port,
            password: raw
                .get("password")
                .and_then(|v| v.as_str())
                .context("rsq: password required")?
                .to_string(),
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
            profile: traffic::TrafficProfile::parse(raw.get("traffic_profile")),
            idle_timeout: parse_duration_str(
                raw.get("idle_timeout").and_then(|v| v.as_str()),
            )
            .unwrap_or(DEFAULT_IDLE_TIMEOUT),
            keep_alive_period: parse_duration_str(
                raw.get("keep_alive_period").and_then(|v| v.as_str()),
            )
            .unwrap_or(DEFAULT_KEEP_ALIVE),
            probe_interval: parse_duration_str(
                raw.get("probe_interval").and_then(|v| v.as_str()),
            )
            .unwrap_or(DEFAULT_PROBE_INTERVAL),
            stream_open_timeout: parse_duration_str(
                raw.get("stream_open_timeout").and_then(|v| v.as_str()),
            )
            .unwrap_or(DEFAULT_STREAM_OPEN_TIMEOUT),
            max_session_age: parse_duration_str(
                raw.get("max_session_age").and_then(|v| v.as_str()),
            )
            .unwrap_or(DEFAULT_MAX_SESSION_AGE),
            shared: Arc::new(RsqShared {
                session: tokio::sync::Mutex::new(None),
                connect_inflight: AtomicBool::new(false),
                connect_notify: tokio::sync::Notify::new(),
                generation: AtomicU32::new(0),
                probe_task_started: AtomicBool::new(false),
            }),
        });
        if warm_up {
            spawn_warmup(inner.clone());
        }
        Ok(Self { inner })
    }

    async fn reset_session(&self, reason: &str) {
        reset_session(&self.inner.shared, reason).await;
    }

    fn session_is_usable(session: &RsqSession, max_age: Duration) -> bool {
        session.connection.close_reason().is_none() && session.created_at.elapsed() < max_age
    }

    async fn cached_session(&self) -> Option<Arc<RsqSession>> {
        let guard = self.inner.shared.session.lock().await;
        guard.as_ref().and_then(|session| {
            if Self::session_is_usable(session, self.inner.max_session_age) {
                Some(session.clone())
            } else {
                None
            }
        })
    }

    async fn get_connection(&self) -> Result<Arc<RsqSession>> {
        self.maybe_start_probe_loop();

        loop {
            if let Some(session) = self.cached_session().await {
                return Ok(session);
            }

            if self
                .inner
                .shared
                .connect_inflight
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                struct ConnectGuard<'a> {
                    shared: &'a RsqShared,
                }
                impl Drop for ConnectGuard<'_> {
                    fn drop(&mut self) {
                        self.shared
                            .connect_inflight
                            .store(false, Ordering::SeqCst);
                        self.shared.connect_notify.notify_waiters();
                    }
                }
                let _guard = ConnectGuard {
                    shared: &self.inner.shared,
                };
                return self.establish_session().await;
            }

            self.inner.shared.connect_notify.notified().await;
        }
    }

    async fn establish_session(&self) -> Result<Arc<RsqSession>> {
        if let Some(session) = self.cached_session().await {
            return Ok(session);
        }

        reset_session(&self.inner.shared, "reconnect").await;

        let generation = self.inner.shared.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let session = self.connect_session(generation).await?;
        let conn = session.connection.clone();
        let conn_id = conn.stable_id();

        self.spawn_close_watcher(conn, generation, conn_id);
        *self.inner.shared.session.lock().await = Some(Arc::new(session));

        if let Some(session) = self.cached_session().await {
            return Ok(session);
        }
        anyhow::bail!("rsq: session lost after establish");
    }

    fn maybe_start_probe_loop(&self) {
        if self
            .inner
            .shared
            .probe_task_started
            .swap(true, Ordering::Relaxed)
        {
            return;
        }

        let shared = Arc::clone(&self.inner.shared);
        let tag = self.inner.tag.clone();
        let probe_interval = self.inner.probe_interval;
        let max_session_age = self.inner.max_session_age;

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(probe_interval).await;

                let probe_target = {
                    let guard = shared.session.lock().await;
                    guard.as_ref().and_then(|session| {
                        if session.connection.close_reason().is_some()
                            || session.created_at.elapsed() >= max_session_age
                        {
                            return None;
                        }
                        Some((
                            session.connection.clone(),
                            session.generation,
                            session.connection.stable_id(),
                        ))
                    })
                };

                let Some((conn, generation, conn_id)) = probe_target else {
                    continue;
                };

                if !probe_connection(&conn).await {
                    // Keep session on transient probe failure (matches hysteria2).
                    // open_bi can time out under load; resetting here kills in-flight dials.
                    tracing::debug!(
                        tag = %tag,
                        conn_id,
                        generation,
                        "rsq: probe open_bi failed (session kept)"
                    );
                }
            }
        });
    }

    fn spawn_close_watcher(
        &self,
        conn: Arc<quinn::Connection>,
        generation: u32,
        conn_id: usize,
    ) {
        let shared = Arc::clone(&self.inner.shared);
        let tag = self.inner.tag.clone();
        tokio::spawn(async move {
            conn.closed().await;
            tracing::debug!(tag = %tag, conn_id, generation, "rsq: quic connection closed");
            let mut guard = shared.session.lock().await;
            if guard.as_ref().is_some_and(|s| {
                s.connection.stable_id() == conn_id && s.generation == generation
            }) {
                *guard = None;
            }
        });
    }

    async fn connect_session(&self, generation: u32) -> Result<RsqSession> {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let sni = self
            .inner
            .sni
            .clone()
            .unwrap_or_else(|| self.inner.server.clone());
        let tls_cfg = quic::client_tls(self.inner.insecure);
        let client_cfg = quic::build_client_config(
            tls_cfg,
            Some(self.inner.profile),
            self.inner.up_mbps,
            self.inner.down_mbps,
            Some(self.inner.idle_timeout),
            Some(self.inner.keep_alive_period),
        )?;

        let addr = tokio::net::lookup_host(format!("{}:{}", self.inner.server, self.inner.port))
            .await
            .context("resolve rsq server")?
            .next()
            .context("no rsq server address")?;

        let endpoint = if let Some(ref obfs) = self.inner.obfs {
            let (mut ep, _) = obfs_socket::endpoint_with_obfs("0.0.0.0:0".parse()?, obfs.clone())?;
            ep.set_default_client_config(client_cfg);
            ep
        } else {
            let mut ep = Endpoint::client("0.0.0.0:0".parse()?)?;
            ep.set_default_client_config(client_cfg);
            ep
        };

        let connect_fut = endpoint.connect(addr, &sni)?;
        let connection = Arc::new(
            tokio::time::timeout(self.inner.stream_open_timeout, connect_fut)
                .await
                .context("rsq: quic connect timeout")?
                .context("quic connect")?,
        );

        let (udp_demux, udp_enabled) = self.authenticate(connection.clone()).await?;

        tracing::info!(
            tag = %self.inner.tag,
            server = %self.inner.server,
            port = self.inner.port,
            generation,
            idle_timeout_secs = self.inner.idle_timeout.as_secs(),
            keep_alive_secs = self.inner.keep_alive_period.as_secs(),
            probe_interval_secs = self.inner.probe_interval.as_secs(),
            "rsq: session established"
        );

        Ok(RsqSession {
            endpoint,
            connection,
            generation,
            created_at: Instant::now(),
            udp_demux,
            udp_enabled,
        })
    }

    async fn authenticate(
        &self,
        connection: Arc<quinn::Connection>,
    ) -> Result<(Arc<udp_demux::UdpDemux>, bool)> {
        let (mut send, mut recv) = connection.open_bi().await.context("open control stream")?;
        let req = auth::encode_auth_req(
            &self.inner.password,
            self.inner.down_mbps,
            self.inner.up_mbps,
            self.inner.profile,
        );
        send.write_all(&req).await.context("write auth req")?;

        let mut buf = BytesMut::new();
        let mut chunk = [0u8; 4096];
        let auth_ok = loop {
            let n = recv
                .read(&mut chunk)
                .await
                .context("read auth resp")?
                .ok_or_else(|| anyhow::anyhow!("rsq: auth stream closed"))?;
            buf.extend_from_slice(&chunk[..n]);
            if let Some(frame) = protocol::try_decode_frame(&buf)? {
                break auth::decode_auth_ok(&frame)?;
            }
            if buf.len() > 8192 {
                anyhow::bail!("rsq: auth response too large");
            }
        };
        tracing::info!(
            server_rx_bps = auth_ok.server_rx_bps,
            session_id = auth_ok.session_id,
            udp = auth_ok.udp_enabled,
            "rsq auth ok"
        );
        let demux = udp_demux::UdpDemux::new();
        demux.ensure_reader(connection.clone());
        control::spawn_client_ping(connection, send, recv, self.inner.profile);
        Ok((demux, auth_ok.udp_enabled))
    }

    async fn open_bi_with_timeout(
        &self,
        conn: &quinn::Connection,
    ) -> Result<(quinn::SendStream, quinn::RecvStream)> {
        match tokio::time::timeout(self.inner.stream_open_timeout, conn.open_bi()).await {
            Ok(Ok(streams)) => Ok(streams),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => anyhow::bail!("rsq: open stream timeout"),
        }
    }

    async fn dial_tcp_inner(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        let session = self.get_connection().await?;
        let (mut send, mut recv) = self
            .open_bi_with_timeout(&session.connection)
            .await
            .context("open rsq stream")?;

        let target = if let Some(domain) = domain {
            format!("{}:{}", domain, destination.port())
        } else {
            format_address(destination)
        };
        let padding = protocol::random_pad_len(64, 512);
        let req = protocol::encode_tcp_open(&target, padding);
        send.write_all(&req).await.context("write tcp open")?;

        let mut buf = BytesMut::new();
        let mut chunk = [0u8; 512];
        loop {
            if let Some(reply) = protocol::try_decode_tcp_reply(&mut buf)? {
                match reply {
                    protocol::TcpOpenReply::Ok => break,
                    protocol::TcpOpenReply::Err(msg) => {
                        anyhow::bail!("rsq tcp open rejected: {msg}");
                    }
                }
            }
            let n = recv
                .read(&mut chunk)
                .await
                .context("read tcp reply")?
                .ok_or_else(|| anyhow::anyhow!("rsq: stream closed before tcp reply"))?;
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > 8192 {
                anyhow::bail!("rsq: tcp reply too large");
            }
        }

        let prefix = buf.to_vec();
        let reader: Box<dyn AsyncRead + Send + Unpin> = if prefix.is_empty() {
            Box::new(recv)
        } else {
            Box::new(stream::PrefixedRecvStream::new(recv, prefix))
        };
        let pacer = bandwidth::brutal_pacer_from_mbps(self.inner.up_mbps);
        let writer = bandwidth::BrutalWriter::new(send, pacer);
        Ok(Box::new(SplitProxy::new(reader, writer)))
    }
}

fn spawn_warmup(inner: Arc<RsqOutboundInner>) {
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return;
    };
    handle.spawn(async move {
        let ob = RsqOutbound { inner };
        if let Err(err) = ob.get_connection().await {
            tracing::debug!(
                error = %err,
                tag = %ob.inner.tag,
                "rsq warmup failed (will retry on first dial)"
            );
        }
    });
}

#[async_trait]
impl Outbound for RsqOutbound {
    fn tag(&self) -> &str {
        &self.inner.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_RSQ
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
            Ok(conn) => Ok(conn),
            Err(first) => {
                if should_reset_rsq_session(&first) {
                    tracing::warn!(
                        tag = %self.inner.tag,
                        error = %first,
                        "rsq: session error, reconnecting once"
                    );
                    self.reset_session("dial retry").await;
                    return self.dial_tcp_inner(destination, domain).await;
                }
                tracing::debug!(
                    tag = %self.inner.tag,
                    error = %first,
                    "rsq: stream dial retry on same session"
                );
                self.dial_tcp_inner(destination, domain).await
            }
        }
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let session = match self.get_connection().await {
            Ok(session) => session,
            Err(_) => {
                self.reset_session("udp dial retry").await;
                self.get_connection().await?
            }
        };
        if !session.udp_enabled {
            return Err(anyhow::anyhow!("rsq: server udp disabled").into());
        }
        let session_id = next_udp_session_id();
        Ok(udp_client::rsq_udp_socket(
            session.connection.clone(),
            session.udp_demux.clone(),
            session_id,
        ))
    }
    async fn close(&self) -> Result<(), BoxError> {
        self.reset_session("close").await;
        Ok(())
    }
}

async fn probe_connection(conn: &quinn::Connection) -> bool {
    if conn.close_reason().is_some() {
        return false;
    }

    match tokio::time::timeout(PROBE_STREAM_TIMEOUT, conn.open_bi()).await {
        Ok(Ok((mut send, recv))) => {
            let _ = send.reset(0u32.into());
            drop(recv);
            drop(send);
            true
        }
        Ok(Err(e)) => {
            tracing::debug!(error = %e, "rsq: probe open_bi failed");
            false
        }
        Err(_) => {
            tracing::debug!("rsq: probe open_bi timeout");
            false
        }
    }
}

async fn reset_session(shared: &RsqShared, reason: &str) {
    let mut guard = shared.session.lock().await;
    if let Some(session) = guard.take() {
        shared.generation.fetch_add(1, Ordering::Relaxed);
        session
            .connection
            .close(0u32.into(), reason.as_bytes());
        // Dropping session closes the Endpoint and releases the local UDP socket.
    }
}

fn should_reset_rsq_session(err: &BoxError) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("auth")
        || msg.contains("quic connect")
        || msg.contains("rsq: quic connect timeout")
        || msg.contains("connection lost")
        || msg.contains("connection closed")
        || msg.contains("application closed")
        || msg.contains("timed out waiting for connection")
}

fn format_address(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v4) => format!("{}:{}", v4.ip(), v4.port()),
        SocketAddr::V6(v6) => format!("[{}]:{}", v6.ip(), v6.port()),
    }
}

fn parse_duration_str(s: Option<&str>) -> Option<Duration> {
    let s = s?;
    if let Some(secs) = s.strip_suffix('s') {
        secs.parse::<u64>().ok().map(Duration::from_secs)
    } else if let Some(mins) = s.strip_suffix('m') {
        mins.parse::<u64>()
            .ok()
            .map(|m| Duration::from_secs(m * 60))
    } else if let Some(hours) = s.strip_suffix('h') {
        hours
            .parse::<u64>()
            .ok()
            .map(|h| Duration::from_secs(h * 3600))
    } else {
        None
    }
}
