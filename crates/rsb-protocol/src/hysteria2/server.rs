use super::{auth, protocol, relay};
use anyhow::{Context, Result};
use dashmap::DashMap;
use h3_quinn::Connection;
use http::{Request, Response, StatusCode};
use quinn::{Endpoint, ServerConfig as QuinnServerConfig, TransportConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;

#[derive(Clone)]
pub struct Hy2ServerConfig {
    pub listen: SocketAddr,
    pub cert_path: String,
    pub key_path: String,
    pub passwords: Vec<String>,
    pub up_mbps: u32,
    pub down_mbps: u32,
    pub udp: bool,
    pub obfs_password: Option<String>,
}

struct AppState {
    passwords: Arc<HashSet<String>>,
    down_mbps: u32,
    udp: bool,
}

#[derive(Clone)]
struct UdpSession {
    socket: Arc<UdpSocket>,
    return_addr: String,
    relay_started: Arc<AtomicBool>,
}

pub async fn run(config: Arc<Hy2ServerConfig>) -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let passwords: HashSet<String> = config.passwords.iter().cloned().collect();
    let state = Arc::new(AppState {
        passwords: Arc::new(passwords),
        down_mbps: config.down_mbps,
        udp: config.udp,
    });

    let server_config = build_quinn_config(&config.cert_path, &config.key_path)?;
    let endpoint = if let Some(ref pass) = config.obfs_password {
        super::obfs_socket::endpoint_with_obfs_server(
            config.listen,
            server_config,
            Arc::new(super::obfs::Salamander::new(pass)),
        )?
    } else {
        Endpoint::server(server_config, config.listen).context("create quinn endpoint")?
    };

    tracing::info!(addr = %config.listen, "hysteria2 inbound listening");

    while let Some(incoming) = endpoint.accept().await {
        let state = state.clone();
        tokio::spawn(async move {
            match incoming.await {
                Ok(connection) => {
                    if let Err(err) = serve_connection(state, connection).await {
                        tracing::debug!(error = %err, "hysteria2 connection ended");
                    }
                },
                Err(err) => tracing::warn!(error = %err, "hysteria2 accept failed"),
            }
        });
    }
    Ok(())
}

fn build_quinn_config(cert_path: &str, key_path: &str) -> Result<QuinnServerConfig> {
    let cert_chain = load_certs(cert_path)?;
    let key = load_key(key_path)?;
    let mut server_crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .context("build tls config")?;
    server_crypto.alpn_protocols = vec![b"h3".to_vec()];

    let mut transport = TransportConfig::default();
    transport.max_concurrent_bidi_streams(64u32.into());
    transport.stream_receive_window((256u32 * 1024).into());
    transport.receive_window((512u32 * 1024).into());
    transport.max_idle_timeout(Some(
        Duration::from_secs(60).try_into().context("idle timeout")?,
    ));
    transport.keep_alive_interval(Some(Duration::from_secs(15)));

    let mut server_config = QuinnServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
            .context("quic server crypto")?,
    ));
    server_config.transport_config(Arc::new(transport));
    Ok(server_config)
}

fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let file = std::fs::File::open(path).with_context(|| format!("open cert {path}"))?;
    let mut reader = std::io::BufReader::new(file);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .context("read cert pem")
}

fn load_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let file = std::fs::File::open(path).with_context(|| format!("open key {path}"))?;
    let mut reader = std::io::BufReader::new(file);
    let keys = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .context("read key pem")?;
    keys.into_iter()
        .next()
        .map(PrivateKeyDer::Pkcs8)
        .ok_or_else(|| anyhow::anyhow!("no private key found in {path}"))
}

async fn serve_connection(state: Arc<AppState>, connection: quinn::Connection) -> Result<()> {
    if !authenticate_via_h3(&state, &connection).await? {
        return Ok(());
    }
    let udp_sessions: Arc<DashMap<u32, UdpSession>> = Arc::new(DashMap::new());
    let udp_enabled = state.udp;
    loop {
        tokio::select! {
            incoming = connection.accept_bi() => {
                match incoming {
                    Ok((send, recv)) => {
                        tokio::spawn(async move {
                            if let Err(err) = relay::handle_tcp_stream(send, recv).await {
                                tracing::debug!(error = %err, "hy2 tcp relay failed");
                            }
                        });
                    }
                    Err(quinn::ConnectionError::ApplicationClosed(_)) | Err(quinn::ConnectionError::LocallyClosed) => break,
                    Err(err) => {
                        tracing::debug!(error = %err, "hy2 accept bi stream");
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
                                tracing::debug!(error = %err, "hy2 udp relay failed");
                            }
                        });
                    }
                    Err(quinn::ConnectionError::ApplicationClosed(_)) | Err(quinn::ConnectionError::LocallyClosed) => break,
                    Err(err) => {
                        tracing::debug!(error = %err, "hy2 read datagram");
                        break;
                    }
                }
            }
            else => break,
        }
    }
    Ok(())
}

async fn authenticate_via_h3(state: &AppState, connection: &quinn::Connection) -> Result<bool> {
    let h3_conn = Connection::new(connection.clone());
    let mut h3: h3::server::Connection<Connection, bytes::Bytes> = h3::server::builder()
        .build(h3_conn)
        .await
        .context("build h3 server")?;
    let Some(resolver) = h3.accept().await.context("h3 accept")? else {
        return Ok(false);
    };
    let (req, mut stream) = resolver
        .resolve_request()
        .await
        .context("resolve h3 request")?;
    if !try_authenticate(state, &req) {
        let response = Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(())
            .unwrap();
        stream.send_response(response).await.ok();
        stream.finish().await.ok();
        return Ok(false);
    }
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
    drop(h3);
    Ok(true)
}

fn try_authenticate(state: &AppState, req: &Request<()>) -> bool {
    let path = req.uri().path();
    let authority = req.uri().authority().map(|a| a.as_str());
    if !auth::is_auth_request(req.method(), path, authority) {
        return false;
    }
    let Some(auth_req) = auth::parse_auth_request(req.headers()) else {
        return false;
    };
    state.passwords.contains(&auth_req.password)
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
                tracing::debug!(error = %err, "hy2 udp back relay ended");
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
    let mut buf = vec![0u8; 65535];
    loop {
        let (n, _) = socket.recv_from(&mut buf).await.context("udp recv back")?;
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
        connection
            .send_datagram(out.freeze())
            .context("send udp datagram back")?;
    }
}
