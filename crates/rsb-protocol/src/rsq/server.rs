use super::udp_fragment::{self, UdpReassembler};
use super::{auth, control, obfs, obfs_socket, protocol, quic, relay};
use anyhow::{Context, Result};
use bytes::BytesMut;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use quinn::Endpoint;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::UdpSocket;

#[derive(Clone)]
pub struct RsqServerConfig {
    pub listen: SocketAddr,
    pub inbound_tag: String,
    pub cert_path: String,
    pub key_path: String,
    pub passwords: Vec<String>,
    pub up_mbps: u32,
    pub down_mbps: u32,
    pub udp: bool,
    pub obfs: Option<Arc<obfs::RsqObfs>>,
    pub connections: rsb_core::SharedConnectionManager,
}

struct AppState {
    password_list: Arc<Vec<String>>,
    up_mbps: u32,
    down_mbps: u32,
    udp: bool,
    inbound_tag: String,
    connections: rsb_core::SharedConnectionManager,
    replay: Arc<auth::AuthReplayCache>,
}

#[derive(Clone)]
struct UdpSession {
    socket: Arc<UdpSocket>,
    return_addr: String,
    relay_started: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    last_active: Arc<Mutex<Instant>>,
}

pub async fn run(config: Arc<RsqServerConfig>) -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let password_list: Vec<String> = config.passwords.iter().cloned().collect();
    let state = Arc::new(AppState {
        password_list: Arc::new(password_list),
        up_mbps: config.up_mbps,
        down_mbps: config.down_mbps,
        udp: config.udp,
        inbound_tag: config.inbound_tag.clone(),
        connections: config.connections.clone(),
        replay: Arc::new(auth::AuthReplayCache::default()),
    });

    let server_config = quic::build_server_config(
        &config.cert_path,
        &config.key_path,
        config.up_mbps,
        config.down_mbps,
    )?;
    let endpoint = if let Some(ref obfs) = config.obfs {
        obfs_socket::endpoint_with_obfs_server(config.listen, server_config, obfs.clone())?
    } else {
        Endpoint::server(server_config, config.listen).context("create quinn endpoint")?
    };

    tracing::info!(addr = %config.listen, "rsq inbound listening");

    while let Some(incoming) = endpoint.accept().await {
        let state = state.clone();
        tokio::spawn(async move {
            match incoming.await {
                Ok(connection) => {
                    if let Err(err) = serve_connection(state, connection).await {
                        tracing::debug!(error = %err, "rsq connection ended");
                    }
                },
                Err(err) => tracing::warn!(error = %err, "rsq accept failed"),
            }
        });
    }
    Ok(())
}

async fn serve_connection(state: Arc<AppState>, connection: quinn::Connection) -> Result<()> {
    let (auth_password, client_caps) = match authenticate_control(&state, &connection).await? {
        Some(v) => v,
        None => return Ok(()),
    };
    let downlink_bps = client_caps.client_rx_bps;
    let uplink_bps = match (state.up_mbps, client_caps.client_up_bps) {
        (0, 0) => 0,
        (0, up) => up,
        (up, 0) => up as u64 * auth::MBPS_TO_BPS,
        (up, client) => (up as u64 * auth::MBPS_TO_BPS).min(client),
    };
    let user_name = state
        .connections
        .users()
        .lookup_password(&auth_password)
        .map(|r| r.name.clone())
        .unwrap_or_else(|| auth_password.chars().take(8).collect());
    let limits = state
        .connections
        .users()
        .lookup_password(&auth_password)
        .map(|r| r.limits.clone())
        .unwrap_or_default();
    // One QUIC session = one panel connection slot (same as hysteria2).
    let _session_guard = state
        .connections
        .acquire_user(&user_name, &limits)
        .map_err(|err| {
            tracing::warn!(user = %user_name, error = %err, "rsq session connection limit");
            err
        })?;
    let relay_ctx = relay::RsqRelayCtx {
        connections: state.connections.clone(),
        inbound_tag: state.inbound_tag.clone(),
        password: auth_password,
        server_down_mbps: state.down_mbps,
        client_rx_bps: downlink_bps,
        client_up_bps: uplink_bps,
        profile: client_caps.profile,
    };
    let udp_sessions: Arc<DashMap<u32, UdpSession>> = Arc::new(DashMap::new());
    let udp_reassembler = Arc::new(tokio::sync::Mutex::new(UdpReassembler::new()));
    let udp_enabled = state.udp;

    let prune_sessions = udp_sessions.clone();
    let prune_connection = connection.clone();
    let prune_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            if prune_connection.close_reason().is_some() {
                break;
            }
            let now = Instant::now();
            prune_sessions.retain(|_, session| {
                let active = session
                    .last_active
                    .lock()
                    .map(|t| now.duration_since(*t) < udp_fragment::UDP_SESSION_IDLE)
                    .unwrap_or(true);
                if !active {
                    session.closed.store(true, Ordering::SeqCst);
                }
                active
            });
        }
    });

    // NOTE: Do not kill sessions on low throughput after bulk.
    // Keepalives + QUIC idle timeout already reclaim dead links; the old
    // "stagnant" closer caused reconnect loops that felt like slow network.

    loop {
        tokio::select! {
            _ = connection.closed() => break,
            incoming = connection.accept_bi() => {
                match incoming {
                    Ok((send, recv)) => {
                        let ctx = relay_ctx.clone();
                        tokio::spawn(async move {
                            if let Err(err) = relay::handle_tcp_stream(ctx, send, recv).await {
                                tracing::debug!(error = %err, "rsq tcp relay failed");
                            }
                        });
                    }
                    Err(quinn::ConnectionError::ApplicationClosed(_)) | Err(quinn::ConnectionError::LocallyClosed) => break,
                    Err(err) => {
                        tracing::debug!(error = %err, "rsq accept bi stream");
                        break;
                    }
                }
            }
            datagram = connection.read_datagram(), if udp_enabled => {
                match datagram {
                    Ok(data) => {
                        let connection = connection.clone();
                        let sessions = udp_sessions.clone();
                        let reassembler = udp_reassembler.clone();
                        if let Err(err) =
                            handle_udp_datagram(connection, sessions, reassembler, data).await
                        {
                            tracing::debug!(error = %err, "rsq udp relay failed");
                        }
                    }
                    Err(quinn::ConnectionError::ApplicationClosed(_)) | Err(quinn::ConnectionError::LocallyClosed) => break,
                    Err(err) => {
                        tracing::debug!(error = %err, "rsq read datagram");
                        break;
                    }
                }
            }
            else => break,
        }
    }
    prune_task.abort();
    for entry in udp_sessions.iter_mut() {
        entry.value().closed.store(true, Ordering::SeqCst);
    }
    udp_sessions.clear();
    Ok(())
}

async fn authenticate_control(
    state: &AppState,
    connection: &quinn::Connection,
) -> Result<Option<(String, auth::AuthClientCaps)>> {
    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .context("accept control stream")?;
    let mut buf = BytesMut::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match recv.read(&mut chunk).await {
            Ok(Some(n)) => n,
            Ok(None) => break,
            Err(err) => {
                tracing::debug!(error = %err, "rsq auth read failed");
                connection.close(0u32.into(), b"");
                return Ok(None);
            }
        };
        buf.extend_from_slice(&chunk[..n]);
        let Some(frame) = protocol::try_decode_frame(&buf)? else {
            if buf.len() > 16384 {
                connection.close(0u32.into(), b"");
                return Ok(None);
            }
            continue;
        };
        match auth::verify_auth_req(&frame, state.password_list.as_slice(), Some(&state.replay)) {
            Ok((pass, caps)) => {
                let session_id = rand::random::<u32>();
                let server_rx = if state.up_mbps == 0 {
                    0
                } else {
                    state.up_mbps as u64 * auth::MBPS_TO_BPS
                };
                let ok = auth::encode_auth_ok(session_id, server_rx, state.udp);
                send.write_all(&ok).await.ok();
                let consumed = protocol::frame_consumed_len(&buf)?;
                let prefix = buf.split_off(consumed);
                control::spawn_server_control_with_prefix(send, recv, prefix);
                tracing::info!(
                    client_rx_bps = caps.client_rx_bps,
                    server_rx_bps = server_rx,
                    "rsq session authenticated"
                );
                return Ok(Some((pass, caps)));
            },
            Err(err) => {
                tracing::debug!(error = %err, "rsq auth rejected");
                connection.close(0u32.into(), b"");
                return Ok(None);
            }
        }
    }
    connection.close(0u32.into(), b"");
    Ok(None)
}

async fn handle_udp_datagram(
    connection: quinn::Connection,
    sessions: Arc<DashMap<u32, UdpSession>>,
    reassembler: Arc<tokio::sync::Mutex<UdpReassembler>>,
    data: bytes::Bytes,
) -> Result<()> {
    let mut cursor = &data[..];
    let msg = protocol::UdpMessage::decode(&mut cursor).context("decode udp message")?;
    if udp_fragment::is_udp_session_close(&msg) {
        if let Some((_, session)) = sessions.remove(&msg.session_id) {
            session.closed.store(true, Ordering::SeqCst);
        }
        return Ok(());
    }
    udp_fragment::ensure_fragment_ready(&msg)?;
    let msg = {
        let mut guard = reassembler.lock().await;
        guard.prune_expired();
        match guard.ingest(msg)? {
            Some(m) => m,
            None => return Ok(()),
        }
    };
    let target = relay::parse_udp_target(&msg.addr)
        .await
        .context("parse udp target")?;

    let session = match sessions.entry(msg.session_id) {
        Entry::Occupied(entry) => entry.get().clone(),
        Entry::Vacant(entry) => {
            let socket = UdpSocket::bind("0.0.0.0:0")
                .await
                .context("bind udp session")?;
            let socket = Arc::new(socket);
            let session = UdpSession {
                socket: socket.clone(),
                return_addr: msg.addr.clone(),
                relay_started: Arc::new(AtomicBool::new(false)),
                closed: Arc::new(AtomicBool::new(false)),
                last_active: Arc::new(Mutex::new(Instant::now())),
            };
            entry.insert(session.clone());
            session
        }
    };

    if let Ok(mut last) = session.last_active.lock() {
        *last = Instant::now();
    }

    if session
        .relay_started
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let conn = connection.clone();
        let session_id = msg.session_id;
        let socket = session.socket.clone();
        let return_addr = session.return_addr.clone();
        let closed = session.closed.clone();
        tokio::spawn(async move {
            if let Err(err) = relay_udp_back(conn, session_id, socket, return_addr, closed).await {
                tracing::debug!(error = %err, "rsq udp back relay ended");
            }
        });
    }
    relay::forward_udp_payload(&session.socket, target, &msg.payload).await?;
    Ok(())
}

async fn relay_udp_back(
    connection: quinn::Connection,
    session_id: u32,
    socket: Arc<UdpSocket>,
    return_addr: String,
    closed: Arc<AtomicBool>,
) -> Result<()> {
    let mut buf = vec![0u8; 65535];
    let mut packet_id: u16 = 0;
    loop {
        if closed.load(Ordering::SeqCst) {
            break;
        }
        tokio::select! {
            recv = socket.recv_from(&mut buf) => {
                let (n, _) = recv.context("udp recv back")?;
                let frames = udp_fragment::fragment_payload(session_id, packet_id, &return_addr, &buf[..n])?;
                packet_id = packet_id.wrapping_add(1);
                for (i, frame) in frames.iter().enumerate() {
                    if connection.send_datagram(frame.clone().freeze()).is_err() {
                        return Ok(());
                    }
                    if i + 1 < frames.len() {
                        tokio::task::yield_now().await;
                    }
                }
            }
            _ = connection.closed() => break,
        }
    }
    Ok(())
}
