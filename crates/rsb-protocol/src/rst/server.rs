//! RST inbound — QUIC + HTTP/3 auth (ALPN h3) + Brutal + optional UDP obfs.

use super::{auth, obfs_socket, protocol, quic, relay};
use anyhow::{Context, Result};
use dashmap::DashMap;
use h3_quinn::Connection;
use http::{Request, Response, StatusCode};
use quinn::Endpoint;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;

#[derive(Clone)]
pub struct RstServerConfig {
    pub listen: SocketAddr,
    pub inbound_tag: String,
    pub cert_path: String,
    pub key_path: String,
    pub passwords: Vec<String>,
    pub up_mbps: u32,
    pub down_mbps: u32,
    pub udp: bool,
    pub allow_private: bool,
    pub obfs: Option<Arc<super::obfs::RstObfs>>,
    pub connections: rsb_core::SharedConnectionManager,
}

struct AppState {
    passwords: Arc<HashSet<String>>,
    down_mbps: u32,
    udp: bool,
    allow_private: bool,
    inbound_tag: String,
    connections: rsb_core::SharedConnectionManager,
}

#[derive(Clone)]
struct UdpSession {
    socket: Arc<UdpSocket>,
    return_addr: String,
    relay_started: Arc<AtomicBool>,
}

pub async fn run(config: Arc<RstServerConfig>) -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let passwords: HashSet<String> = config.passwords.iter().cloned().collect();
    let state = Arc::new(AppState {
        passwords: Arc::new(passwords),
        down_mbps: config.down_mbps,
        udp: config.udp,
        allow_private: config.allow_private,
        inbound_tag: config.inbound_tag.clone(),
        connections: config.connections.clone(),
    });

    let server_config =
        quic::build_server_config(&config.cert_path, &config.key_path, config.up_mbps, config.down_mbps)?;
    let endpoint = if let Some(ref obfs) = config.obfs {
        obfs_socket::endpoint_with_obfs_server(config.listen, server_config, obfs.clone())?
    } else {
        Endpoint::server(server_config, config.listen).context("create quinn endpoint")?
    };

    tracing::info!(addr = %config.listen, udp = config.udp, "rst (h3/quic) inbound listening");

    while let Some(incoming) = endpoint.accept().await {
        let state = state.clone();
        tokio::spawn(async move {
            match incoming.await {
                Ok(connection) => {
                    if let Err(err) = serve_connection(state, connection).await {
                        tracing::debug!(error = %err, "rst connection ended");
                    }
                }
                Err(err) => tracing::warn!(error = %err, "rst accept failed"),
            }
        });
    }
    Ok(())
}

async fn serve_connection(state: Arc<AppState>, connection: quinn::Connection) -> Result<()> {
    let auth_password = match authenticate_via_h3(&state, &connection).await? {
        Some(pass) => pass,
        None => return Ok(()),
    };
    let relay_ctx = relay::RstRelayCtx {
        connections: state.connections.clone(),
        inbound_tag: state.inbound_tag.clone(),
        password: auth_password.clone(),
        server_down_mbps: state.down_mbps,
        allow_private: state.allow_private,
    };
    let limits = relay_ctx.user_limits();
    let user_name = relay_ctx.user_name();
    let _session_guard = state
        .connections
        .acquire_user(&user_name, &limits)
        .with_context(|| format!("rst session limit for user `{user_name}`"))?;

    tracing::info!(
        user = %user_name,
        peer = %connection.remote_address(),
        udp = state.udp,
        "rst session authenticated (h3)"
    );

    let udp_sessions: Arc<DashMap<u32, UdpSession>> = Arc::new(DashMap::new());
    let udp_enabled = state.udp;
    loop {
        tokio::select! {
            incoming = connection.accept_bi() => {
                match incoming {
                    Ok((send, recv)) => {
                        let ctx = relay_ctx.clone();
                        tokio::spawn(async move {
                            if let Err(err) = relay::handle_tcp_stream(ctx, send, recv).await {
                                tracing::debug!(error = %err, "rst tcp relay failed");
                            }
                        });
                    }
                    Err(quinn::ConnectionError::ApplicationClosed(_))
                    | Err(quinn::ConnectionError::LocallyClosed) => break,
                    Err(err) => {
                        tracing::debug!(error = %err, "rst accept bi stream");
                        break;
                    }
                }
            }
            datagram = connection.read_datagram(), if udp_enabled => {
                match datagram {
                    Ok(data) => {
                        let connection = connection.clone();
                        let sessions = udp_sessions.clone();
                        tokio::spawn(async move {
                            if let Err(err) = handle_udp_datagram(connection, sessions, data).await {
                                tracing::debug!(error = %err, "rst udp relay failed");
                            }
                        });
                    }
                    Err(quinn::ConnectionError::ApplicationClosed(_))
                    | Err(quinn::ConnectionError::LocallyClosed) => break,
                    Err(err) => {
                        tracing::debug!(error = %err, "rst read datagram");
                        break;
                    }
                }
            }
            else => break,
        }
    }
    Ok(())
}

async fn authenticate_via_h3(
    state: &AppState,
    connection: &quinn::Connection,
) -> Result<Option<String>> {
    let h3_conn = Connection::new(connection.clone());
    let mut h3: h3::server::Connection<Connection, bytes::Bytes> = h3::server::builder()
        .build(h3_conn)
        .await
        .context("build h3 server")?;
    let Some(resolver) = h3.accept().await.context("h3 accept")? else {
        return Ok(None);
    };
    let (req, mut stream) = resolver
        .resolve_request()
        .await
        .context("resolve h3 request")?;
    if let Some(password) = try_authenticate(state, &req) {
        let (status, headers) = auth::build_auth_response(state.udp, state.down_mbps);
        let mut response = Response::builder().status(status);
        for (k, v) in headers.iter() {
            response = response.header(k, v);
        }
        stream
            .send_response(response.body(()).unwrap())
            .await
            .context("send auth response")?;
        stream.finish().await.ok();
        std::mem::forget(h3);
        return Ok(Some(password));
    }
    let response = Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(())
        .unwrap();
    stream.send_response(response).await.ok();
    stream.finish().await.ok();
    Ok(None)
}

fn try_authenticate(state: &AppState, req: &Request<()>) -> Option<String> {
    let path = req.uri().path();
    if !auth::is_auth_request(req.method(), path) {
        return None;
    }
    let auth_req = auth::parse_auth_request(req.headers())?;
    if state.passwords.contains(&auth_req.password) {
        Some(auth_req.password)
    } else {
        None
    }
}

async fn handle_udp_datagram(
    connection: quinn::Connection,
    sessions: Arc<DashMap<u32, UdpSession>>,
    data: bytes::Bytes,
) -> Result<()> {
    let mut cursor = &data[..];
    let msg = protocol::UdpMessage::decode(&mut cursor).context("decode udp message")?;
    relay::ensure_fragment_ready(&msg)?;
    let target = relay::parse_udp_target(&msg.addr)
        .await
        .context("parse udp target")?;
    let session = if let Some(entry) = sessions.get(&msg.session_id) {
        entry.clone()
    } else {
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .context("bind udp session")?;
        let socket = Arc::new(socket);
        let session = UdpSession {
            socket: socket.clone(),
            return_addr: msg.addr.clone(),
            relay_started: Arc::new(AtomicBool::new(false)),
        };
        sessions.insert(msg.session_id, session.clone());
        session
    };
    if session
        .relay_started
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        let conn = connection.clone();
        let session_id = msg.session_id;
        let socket = session.socket.clone();
        let return_addr = session.return_addr.clone();
        tokio::spawn(async move {
            if let Err(err) = relay_udp_back(conn, session_id, socket, return_addr).await {
                tracing::debug!(error = %err, "rst udp back relay ended");
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
) -> Result<()> {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let (n, _) = socket.recv_from(&mut buf).await?;
        let mut out = bytes::BytesMut::new();
        protocol::UdpMessage {
            session_id,
            packet_id: 0,
            fragment_id: 0,
            fragment_count: 1,
            addr: return_addr.clone(),
            payload: buf[..n].to_vec(),
        }
        .encode(&mut out);
        connection.send_datagram(out.freeze())?;
    }
}
