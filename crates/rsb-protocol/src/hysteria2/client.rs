use super::{auth, obfs, obfs_socket, protocol, udp_client};
use crate::transport;
use anyhow::{Context, Result};
use async_trait::async_trait;
use h3_quinn::Connection as H3QuinnConnection;
use http::{Request, StatusCode};
use quinn::{ClientConfig, Endpoint, TransportConfig};
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket, SplitProxy};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static SESSION_COUNTER: AtomicU32 = AtomicU32::new(1);

const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(60);
const DEFAULT_KEEP_ALIVE: Duration = Duration::from_secs(5);
const DEFAULT_PROBE_INTERVAL: Duration = Duration::from_secs(20);
const DEFAULT_STREAM_OPEN_TIMEOUT: Duration = Duration::from_secs(8);
const DEFAULT_MAX_SESSION_AGE: Duration = Duration::from_secs(30 * 60);
const PROBE_STREAM_TIMEOUT: Duration = Duration::from_secs(4);

struct Hy2Session {
    endpoint: Endpoint,
    connection: Arc<quinn::Connection>,
    port: u16,
    generation: u32,
    created_at: Instant,
    /// Keeps the H3 driver alive without closing the underlying QUIC connection.
    _h3_keep_alive: tokio::task::JoinHandle<()>,
}

struct Hy2Shared {
    session: tokio::sync::Mutex<Option<Hy2Session>>,
    /// Ensures only one reconnect runs at a time (prevents orphan QUIC sessions).
    connect_lock: tokio::sync::Mutex<()>,
    current_port: AtomicU16,
    generation: AtomicU32,
    hop_task_started: AtomicBool,
    probe_task_started: AtomicBool,
    port: u16,
    port_start: Option<u16>,
    port_end: Option<u16>,
}

pub struct Hysteria2Outbound {
    tag: String,
    server: String,
    port: u16,
    port_start: Option<u16>,
    port_end: Option<u16>,
    hop_interval: Option<Duration>,
    password: String,
    up_mbps: u32,
    down_mbps: u32,
    sni: Option<String>,
    insecure: bool,
    obfs: Option<Arc<obfs::Salamander>>,
    idle_timeout: Duration,
    keep_alive_period: Duration,
    probe_interval: Duration,
    stream_open_timeout: Duration,
    max_session_age: Duration,
    disable_mtu_discovery: bool,
    shared: Arc<Hy2Shared>,
}

impl Hysteria2Outbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let tls = raw.get("tls");
        let obfs = raw.get("obfs").and_then(|o| {
            o.get("password")
                .and_then(|v| v.as_str())
                .map(|p| Arc::new(obfs::Salamander::new(p)))
        });

        let (port, port_start, port_end) = if let Some(server_ports) = raw.get("server_ports") {
            if let Some(ports_str) = server_ports.as_str() {
                if let Some((start, end)) = ports_str.split_once('-') {
                    let start_port = start
                        .trim()
                        .parse::<u16>()
                        .context("invalid port range start")?;
                    let end_port = end
                        .trim()
                        .parse::<u16>()
                        .context("invalid port range end")?;
                    (start_port, Some(start_port), Some(end_port))
                } else {
                    let single_port = ports_str.parse::<u16>().context("invalid port")?;
                    (single_port, None, None)
                }
            } else {
                return Err(anyhow::anyhow!("server_ports must be string"));
            }
        } else {
            let port = raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("hysteria2: server_port required")? as u16;
            (port, None, None)
        };

        let hop_interval = raw
            .get("hop_interval")
            .and_then(|v| v.as_str())
            .and_then(parse_duration_str);

        Ok(Self {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("hysteria2: server required")?
                .to_string(),
            port,
            port_start,
            port_end,
            hop_interval,
            password: raw
                .get("password")
                .and_then(|v| v.as_str())
                .context("hysteria2: password required")?
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
            idle_timeout: raw
                .get("idle_timeout")
                .and_then(|v| v.as_str())
                .and_then(parse_duration_str)
                .unwrap_or(DEFAULT_IDLE_TIMEOUT),
            keep_alive_period: raw
                .get("keep_alive_period")
                .and_then(|v| v.as_str())
                .and_then(parse_duration_str)
                .unwrap_or(DEFAULT_KEEP_ALIVE),
            probe_interval: raw
                .get("probe_interval")
                .and_then(|v| v.as_str())
                .and_then(parse_duration_str)
                .unwrap_or(DEFAULT_PROBE_INTERVAL),
            stream_open_timeout: raw
                .get("stream_open_timeout")
                .and_then(|v| v.as_str())
                .and_then(parse_duration_str)
                .unwrap_or(DEFAULT_STREAM_OPEN_TIMEOUT),
            max_session_age: raw
                .get("max_session_age")
                .and_then(|v| v.as_str())
                .and_then(parse_duration_str)
                .unwrap_or(DEFAULT_MAX_SESSION_AGE),
            disable_mtu_discovery: raw
                .get("disable_mtu_discovery")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            shared: Arc::new(Hy2Shared {
                session: tokio::sync::Mutex::new(None),
                connect_lock: tokio::sync::Mutex::new(()),
                current_port: AtomicU16::new(port),
                generation: AtomicU32::new(0),
                hop_task_started: AtomicBool::new(false),
                probe_task_started: AtomicBool::new(false),
                port,
                port_start,
                port_end,
            }),
        })
    }

    fn maybe_start_port_hopping(&self) {
        if self.hop_interval.is_none() || self.shared.hop_task_started.swap(true, Ordering::Relaxed)
        {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let tag = self.tag.clone();
        let interval = self.hop_interval.unwrap();
        tokio::spawn(async move {
            tracing::info!(tag = %tag, "hysteria2: port hopping started");
            loop {
                tokio::time::sleep(interval).await;
                let new_port = next_port_for(&shared);
                shared.current_port.store(new_port, Ordering::Relaxed);
                tracing::info!(tag = %tag, port = new_port, "hysteria2: hopping port");
                reset_session(&shared, "port hopping").await;
            }
        });
    }

    async fn reset_session(&self, reason: &str) {
        reset_session(&self.shared, reason).await;
    }

    fn session_is_usable(session: &Hy2Session, port: u16, max_age: Duration) -> bool {
        session.port == port
            && session.connection.close_reason().is_none()
            && session.created_at.elapsed() < max_age
    }

    async fn cached_connection(&self, port: u16) -> Option<Arc<quinn::Connection>> {
        let guard = self.shared.session.lock().await;
        guard.as_ref().and_then(|session| {
            if Self::session_is_usable(session, port, self.max_session_age) {
                Some(session.connection.clone())
            } else {
                None
            }
        })
    }

    /// Acquire a live QUIC connection, reusing the cached session when possible.
    async fn get_connection(&self) -> Result<Arc<quinn::Connection>> {
        self.maybe_start_port_hopping();
        self.maybe_start_probe_loop();

        let port = self.shared.current_port.load(Ordering::Relaxed);
        if let Some(conn) = self.cached_connection(port).await {
            return Ok(conn);
        }

        // Single-flight reconnect: concurrent callers wait on the same mutex.
        let _connect_guard = self.shared.connect_lock.lock().await;

        if let Some(conn) = self.cached_connection(port).await {
            return Ok(conn);
        }

        reset_session(&self.shared, "reconnect").await;

        let generation = self.shared.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let session = self.connect_session(port).await?;
        let conn = session.connection.clone();
        let conn_id = conn.stable_id();

        self.spawn_close_watcher(conn.clone(), generation, conn_id);
        *self.shared.session.lock().await = Some(session);

        Ok(conn)
    }

    fn maybe_start_probe_loop(&self) {
        if self.shared.probe_task_started.swap(true, Ordering::Relaxed) {
            return;
        }

        let shared = Arc::clone(&self.shared);
        let tag = self.tag.clone();
        let probe_interval = self.probe_interval;
        let max_session_age = self.max_session_age;

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
                    tracing::warn!(
                        tag = %tag,
                        conn_id,
                        generation,
                        "hysteria2: probe failed, resetting session"
                    );
                    reset_session(&shared, "probe failed").await;
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
        let shared = Arc::clone(&self.shared);
        let tag = self.tag.clone();
        tokio::spawn(async move {
            conn.closed().await;
            tracing::debug!(tag = %tag, conn_id, generation, "hysteria2: quic connection closed");
            let mut guard = shared.session.lock().await;
            if guard.as_ref().is_some_and(|s| {
                s.connection.stable_id() == conn_id && s.generation == generation
            }) {
                *guard = None;
            }
        });
    }

    async fn connect_session(&self, port: u16) -> Result<Hy2Session> {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let sni = self.sni.clone().unwrap_or_else(|| self.server.clone());

        let tls_cfg = if self.insecure {
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(transport::SkipVerifier))
                .with_no_client_auth()
        } else {
            rustls::ClientConfig::builder()
                .with_root_certificates({
                    let mut roots = rustls::RootCertStore::empty();
                    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                    roots
                })
                .with_no_client_auth()
        };
        let mut tls_cfg = tls_cfg;
        tls_cfg.alpn_protocols = vec![b"h3".to_vec()];
        let mut client_cfg = ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(tls_cfg)?,
        ));
        let mut transport_cfg = TransportConfig::default();
        transport_cfg.keep_alive_interval(Some(self.keep_alive_period));
        transport_cfg.max_idle_timeout(Some(
            self.idle_timeout
                .try_into()
                .map_err(|e| anyhow::anyhow!("invalid hysteria2 idle_timeout: {e}"))?,
        ));
        if !self.disable_mtu_discovery {
            transport_cfg.mtu_discovery_config(Some(quinn::MtuDiscoveryConfig::default()));
        }
        client_cfg.transport_config(Arc::new(transport_cfg));

        let addr = tokio::net::lookup_host(format!("{}:{}", self.server, port))
            .await
            .context("resolve hysteria2 server")?
            .next()
            .context("no hysteria2 server address")?;

        let endpoint = if let Some(ref obfs) = self.obfs {
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
            tokio::time::timeout(self.stream_open_timeout, connect_fut)
                .await
                .context("hysteria2: quic connect timeout")?
                .context("quic connect")?,
        );

        let h3_keep_alive = self.authenticate(&connection).await?;
        let generation = self.shared.generation.load(Ordering::Relaxed);

        tracing::info!(
            tag = %self.tag,
            server = %self.server,
            port,
            generation,
            idle_timeout_secs = self.idle_timeout.as_secs(),
            keep_alive_secs = self.keep_alive_period.as_secs(),
            probe_interval_secs = self.probe_interval.as_secs(),
            disable_mtu_discovery = self.disable_mtu_discovery,
            "hysteria2: session established"
        );

        Ok(Hy2Session {
            endpoint,
            connection,
            port,
            generation,
            created_at: Instant::now(),
            _h3_keep_alive: h3_keep_alive,
        })
    }

    async fn authenticate(
        &self,
        connection: &quinn::Connection,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let h3_conn = H3QuinnConnection::new(connection.clone());
        let (mut driver, mut send_request) = h3::client::new(h3_conn).await.context("h3 client")?;

        let mut req = Request::builder()
            .method("POST")
            .uri("https://hysteria/auth")
            .header("hysteria-auth", &self.password);
        if self.up_mbps > 0 {
            req = req.header(
                "hysteria-cc-rx",
                (self.up_mbps as u64 * auth::MBPS_TO_BPS).to_string(),
            );
        }
        let padding = auth::random_padding(64, 512);
        req = req.header("hysteria-padding", padding.as_str());

        let auth_fut = async {
            let mut stream = send_request.send_request(req.body(())?).await?;
            stream.finish().await?;
            let resp = stream.recv_response().await?;
            if resp.status() != StatusCode::from_u16(233).unwrap() {
                anyhow::bail!("hysteria2 auth failed: {}", resp.status());
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
                    anyhow::bail!("hysteria2: h3 connection closed during auth: {closed:?}");
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
            Err(_) => anyhow::bail!("hysteria2: open stream timeout"),
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
            .context("open hy2 stream")?;

        let target = if let Some(domain) = domain {
            format!("{}:{}", domain, destination.port())
        } else {
            format_address(destination)
        };

        let padding_len = auth::random_padding_len(64, 512);
        let req = protocol::encode_tcp_request(&target, padding_len);
        send.write_all(&req).await?;

        let mut resp_buf = vec![0u8; 2048];
        let read_result =
            tokio::time::timeout(Duration::from_secs(10), recv.read(&mut resp_buf)).await;

        let n = match read_result {
            Ok(Ok(Some(n))) => n,
            Ok(Ok(None)) => anyhow::bail!("hysteria2: stream closed by server"),
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => anyhow::bail!("hysteria2: read timeout"),
        };

        let mut cursor = &resp_buf[..n];
        let (ok, _) = protocol::decode_tcp_response(&mut cursor)?;
        if !ok {
            anyhow::bail!("hysteria2 tcp request rejected");
        }
        Ok(Box::new(SplitProxy::new(recv, send)))
    }
}

#[async_trait]
impl Outbound for Hysteria2Outbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_HYSTERIA2
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
            Err(err) => {
                tracing::warn!(
                    tag = %self.tag,
                    error = %err,
                    "hysteria2: dial failed, reconnecting once"
                );
                self.reset_session("dial retry").await;
                self.dial_tcp_inner(destination, domain).await
            }
        }
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let conn = match self.get_connection().await {
            Ok(conn) => conn,
            Err(_) => {
                self.reset_session("udp dial retry").await;
                self.get_connection().await?
            }
        };
        let session_id = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        Ok(udp_client::hy2_udp_socket(conn, session_id))
    }
    async fn close(&self) -> Result<(), BoxError> {
        self.reset_session("close").await;
        Ok(())
    }
}

/// Open a bidirectional stream to verify the QUIC path is still alive.
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
            tracing::debug!(error = %e, "hysteria2: probe open_bi failed");
            false
        }
        Err(_) => {
            tracing::debug!("hysteria2: probe open_bi timeout");
            false
        }
    }
}

async fn reset_session(shared: &Hy2Shared, reason: &str) {
    let mut guard = shared.session.lock().await;
    if let Some(session) = guard.take() {
        shared.generation.fetch_add(1, Ordering::Relaxed);
        session.connection.close(0u32.into(), reason.as_bytes());
        // Dropping session closes the Endpoint and releases the local UDP socket.
    }
}

fn next_port_for(shared: &Hy2Shared) -> u16 {
    if let (Some(start), Some(end)) = (shared.port_start, shared.port_end) {
        if start == end {
            return start;
        }
        use rand::Rng;
        rand::rng().random_range(start..=end)
    } else {
        shared.port
    }
}

fn format_address(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v4) => format!("{}:{}", v4.ip(), v4.port()),
        SocketAddr::V6(v6) => format!("[{}]:{}", v6.ip(), v6.port()),
    }
}

fn parse_duration_str(s: &str) -> Option<Duration> {
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
