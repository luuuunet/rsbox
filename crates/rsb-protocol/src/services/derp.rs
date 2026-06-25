//! Tailscale DERP relay (derper-compatible frame protocol + optional TLS + mesh).

use super::listen::parse_listen;
use super::tls_util::{load_server_config, tls_enabled, tls_paths, write_default_cert_dir};
use anyhow::{Context, Result};
use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    extract::State,
    response::{IntoResponse, Redirect, Response},
    routing::get,
    Router,
};
use axum_server::tls_rustls::RustlsConfig;
use base64::Engine;
use rand::RngCore;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;
use tracing::info;
use x25519_dalek::{PublicKey, StaticSecret};

const FRAME_SERVER_KEY: u8 = 0x01;
const FRAME_CLIENT_KEY: u8 = 0x02;
const FRAME_SERVER_INFO: u8 = 0x03;
const FRAME_SEND_PACKET: u8 = 0x04;
const FRAME_RECV_PACKET: u8 = 0x05;
const FRAME_KEEP_ALIVE: u8 = 0x06;
const FRAME_NOTE_PREFERRED: u8 = 0x07;
const FRAME_PEER_GONE: u8 = 0x08;
const FRAME_PEER_PRESENT: u8 = 0x09;

pub struct DerpService {
    tag: String,
    listen: SocketAddr,
    binary_listen: Option<SocketAddr>,
    home: Option<String>,
    config_path: String,
    stun_port: Option<u16>,
    tls: bool,
    cert_path: Option<String>,
    key_path: Option<String>,
    mesh_peers: Vec<String>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    stun_handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    mesh_handles: tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
    binary_handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    hub: DerpHub,
}

#[derive(Clone)]
struct DerpHub {
    server_public: Arc<[u8; 32]>,
    peers: Arc<Mutex<HashMap<[u8; 32], tokio::sync::mpsc::UnboundedSender<Vec<u8>>>>>,
    mesh_tx: Arc<tokio::sync::broadcast::Sender<Vec<u8>>>,
}

impl DerpService {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let config_path = raw
            .get("config_path")
            .and_then(|v| v.as_str())
            .unwrap_or("derper.key")
            .to_string();
        let server_public = Arc::new(load_derp_key(&config_path)?);
        let stun_port = raw.get("stun").and_then(|s| {
            if let Some(n) = s.as_u64() {
                return Some(n as u16);
            }
            s.get("listen_port")
                .and_then(|v| v.as_u64())
                .map(|p| p as u16)
        });
        let mut cert_path = None;
        let mut key_path = None;
        if tls_enabled(&raw) {
            let (mut cert, mut key) = tls_paths(&raw);
            if cert.is_none() || key.is_none() {
                let dir = std::path::Path::new(&config_path)
                    .parent()
                    .unwrap_or(std::path::Path::new("."));
                let (c, k) = write_default_cert_dir(dir)?;
                cert = Some(c);
                key = Some(k);
            }
            cert_path = cert;
            key_path = key;
        }
        let mesh_peers = raw
            .get("mesh_with")
            .or_else(|| raw.get("mesh_peers"))
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let binary_listen = raw
            .get("binary_listen")
            .and_then(|v| parse_listen(v).ok())
            .or_else(|| {
                raw.get("binary_listen_port").and_then(|v| {
                    let port = v.as_u64()? as u16;
                    let ip = raw
                        .get("listen")
                        .and_then(|l| l.as_str())
                        .unwrap_or("0.0.0.0");
                    format!("{ip}:{port}").parse().ok()
                })
            });
        let (mesh_tx, _) = tokio::sync::broadcast::channel(512);
        Ok(Self {
            tag,
            listen: parse_listen(&raw)?,
            binary_listen,
            home: raw.get("home").and_then(|v| v.as_str()).map(str::to_string),
            config_path,
            stun_port,
            tls: tls_enabled(&raw),
            cert_path,
            key_path,
            mesh_peers,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
            stun_handle: tokio::sync::Mutex::new(None),
            mesh_handles: tokio::sync::Mutex::new(Vec::new()),
            binary_handle: tokio::sync::Mutex::new(None),
            hub: DerpHub {
                server_public,
                peers: Arc::new(Mutex::new(HashMap::new())),
                mesh_tx: Arc::new(mesh_tx),
            },
        })
    }

    pub async fn start(&self) -> Result<()> {
        let hub = self.hub.clone();
        let home = self.home.clone();
        let app = Router::new()
            .route("/", get(move || async move { home_page(home.as_deref()) }))
            .route("/generate_204", get(|| async { "" }))
            .route("/derp", get(derp_ws))
            .route("/derp/probe", get(|| async { "OK" }))
            .with_state(hub.clone());

        info!(
            tag = %self.tag,
            %self.listen,
            tls = self.tls,
            mesh = self.mesh_peers.len(),
            "derp service listening (derper protocol)"
        );
        let tls = self.tls;
        let cert_path = self.cert_path.clone();
        let key_path = self.key_path.clone();
        let listen_addr = self.listen;
        let handle = if tls {
            let mut shutdown = self.shutdown.subscribe();
            tokio::spawn(async move {
                let cfg = load_server_config(cert_path.as_deref(), key_path.as_deref())
                    .expect("derp tls config");
                let rustls = RustlsConfig::from_config(cfg);
                let server =
                    axum_server::bind_rustls(listen_addr, rustls).serve(app.into_make_service());
                tokio::select! {
                    _ = shutdown.changed() => {}
                    r = server => { let _ = r; }
                }
            })
        } else {
            let listener = tokio::net::TcpListener::bind(listen_addr).await?;
            let mut shutdown = self.shutdown.subscribe();
            tokio::spawn(async move {
                let server = axum::serve(listener, app);
                tokio::select! {
                    _ = shutdown.changed() => {}
                    r = server => { let _ = r; }
                }
            })
        };
        *self.handle.lock().await = Some(handle);

        let mut mesh_tasks = Vec::new();
        for peer_url in &self.mesh_peers {
            let url = peer_url.clone();
            let hub = self.hub.clone();
            let mut mesh_shutdown = self.shutdown.subscribe();
            mesh_tasks.push(tokio::spawn(async move {
                mesh_client_loop(&url, hub, &mut mesh_shutdown).await;
            }));
        }
        *self.mesh_handles.lock().await = mesh_tasks;

        if let Some(addr) = self.binary_listen {
            let hub = self.hub.clone();
            let mut binary_shutdown = self.shutdown.subscribe();
            let binary = tokio::spawn(async move {
                if let Ok(listener) = tokio::net::TcpListener::bind(addr).await {
                    info!(%addr, "derp binary tcp listening");
                    loop {
                        tokio::select! {
                            _ = binary_shutdown.changed() => { if *binary_shutdown.borrow() { break; } }
                            accept = listener.accept() => {
                                let Ok((stream, _)) = accept else { break };
                                let hub = hub.clone();
                                tokio::spawn(async move {
                                    let _ = handle_derp_binary_tcp(stream, hub).await;
                                });
                            }
                        }
                    }
                }
            });
            *self.binary_handle.lock().await = Some(binary);
        }

        if let Some(port) = self.stun_port {
            let mut stun_shutdown = self.shutdown.subscribe();
            let stun = tokio::spawn(async move {
                if let Ok(sock) = UdpSocket::bind(format!("0.0.0.0:{port}")).await {
                    info!(%port, "derp stun listening");
                    let mut buf = [0u8; 1500];
                    loop {
                        tokio::select! {
                            _ = stun_shutdown.changed() => { if *stun_shutdown.borrow() { break; } }
                            recv = sock.recv_from(&mut buf) => {
                                let Ok((n, peer)) = recv else { break };
                                if n >= 20 && &buf[4..8] == [0x21, 0x12, 0xA4, 0x42] {
                                    let _ = sock.send_to(&buf[..n], peer).await;
                                }
                            }
                        }
                    }
                }
            });
            *self.stun_handle.lock().await = Some(stun);
        }
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        if let Some(h) = self.stun_handle.lock().await.take() {
            h.abort();
        }
        for h in self.mesh_handles.lock().await.drain(..) {
            h.abort();
        }
        if let Some(h) = self.binary_handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}

fn load_derp_key(path: &str) -> Result<[u8; 32]> {
    if !std::path::Path::new(path).exists() {
        let mut key = [0u8; 32];
        rand::rng().fill_bytes(&mut key);
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
        }
        std::fs::write(path, key).with_context(|| format!("write derp key `{path}`"))?;
        info!(path, "generated derp private key");
        return Ok(public_from_private(&key));
    }
    let data = std::fs::read(path).with_context(|| format!("read derp key `{path}`"))?;
    if data.len() == 32 {
        return Ok(public_from_private(data.as_slice().try_into()?));
    }
    if let Ok(text) = String::from_utf8(data) {
        if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(text.trim()) {
            if decoded.len() == 32 {
                return Ok(public_from_private(decoded.as_slice().try_into()?));
            }
        }
    }
    anyhow::bail!("invalid derp key format")
}

fn public_from_private(private: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*private);
    *PublicKey::from(&secret).as_bytes()
}

fn home_page(home: Option<&str>) -> Response {
    match home {
        Some("blank") => Response::builder()
            .body(String::new())
            .unwrap()
            .into_response(),
        Some(url) if url.starts_with("http://") || url.starts_with("https://") => {
            Redirect::temporary(url).into_response()
        }
        _ => Response::builder()
            .header("content-type", "text/html; charset=utf-8")
            .body(String::from(
                "<html><body><h1>rsbox DERP</h1></body></html>",
            ))
            .unwrap()
            .into_response(),
    }
}

async fn handle_derp_binary_tcp(mut stream: tokio::net::TcpStream, hub: DerpHub) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let server_key_frame = frame_server_key(&hub.server_public);
    stream
        .write_all(&(server_key_frame.len() as u32).to_be_bytes())
        .await?;
    stream.write_all(&server_key_frame).await?;
    let info = frame_server_info();
    stream.write_all(&(info.len() as u32).to_be_bytes()).await?;
    stream.write_all(&info).await?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut client_key = [0u8; 32];
    let mut registered = false;

    loop {
        tokio::select! {
            read = read_framed(&mut stream) => {
                let data = read?;
                if data.is_empty() {
                    break;
                }
                match data[0] {
                    FRAME_CLIENT_KEY if data.len() >= 33 => {
                        client_key.copy_from_slice(&data[1..33]);
                        hub.peers.lock().unwrap().insert(client_key, tx.clone());
                        registered = true;
                        let present = frame_peer_present(&client_key);
                        broadcast_except(&hub, &client_key, present).await;
                    }
                    FRAME_SEND_PACKET if data.len() > 33 => {
                        route_packet(&hub, &client_key, &data).await;
                    }
                    FRAME_KEEP_ALIVE => {
                        let ka = vec![FRAME_KEEP_ALIVE];
                        write_framed(&mut stream, &ka).await?;
                    }
                    _ => {}
                }
            }
            Some(out) = rx.recv() => {
                write_framed(&mut stream, &out).await?;
            }
        }
    }
    if registered {
        hub.peers.lock().unwrap().remove(&client_key);
        let gone = frame_peer_gone(&client_key);
        broadcast_except(&hub, &client_key, gone).await;
    }
    Ok(())
}

async fn read_framed(stream: &mut tokio::net::TcpStream) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    anyhow::ensure!(len <= 65536, "derp frame too large");
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn write_framed(stream: &mut tokio::net::TcpStream, data: &[u8]) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    stream.write_all(&(data.len() as u32).to_be_bytes()).await?;
    stream.write_all(data).await?;
    Ok(())
}

async fn derp_ws(ws: WebSocketUpgrade, State(hub): State<DerpHub>) -> Response {
    ws.on_upgrade(move |socket| handle_derp_client(socket, hub))
}

async fn handle_derp_client(mut socket: WebSocket, hub: DerpHub) {
    let server_key_frame = frame_server_key(&hub.server_public);
    let _ = socket.send(Message::Binary(server_key_frame.into())).await;
    let _ = socket
        .send(Message::Binary(frame_server_info().into()))
        .await;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut client_key = [0u8; 32];
    let mut registered = false;

    loop {
        tokio::select! {
            Some(msg) = socket.recv() => {
                match msg {
                    Ok(Message::Binary(data)) if !data.is_empty() => match data[0] {
                        FRAME_CLIENT_KEY if data.len() >= 33 => {
                            client_key.copy_from_slice(&data[1..33]);
                            hub.peers.lock().unwrap().insert(client_key, tx.clone());
                            registered = true;
                            let present = frame_peer_present(&client_key);
                            broadcast_except(&hub, &client_key, present).await;
                        }
                        FRAME_SEND_PACKET if data.len() > 33 => {
                            route_packet(&hub, &client_key, &data).await;
                        }
                        FRAME_KEEP_ALIVE => {
                            let _ = socket.send(Message::Binary(vec![FRAME_KEEP_ALIVE].into())).await;
                        }
                        _ => {}
                    },
                    Ok(Message::Close(_)) | Err(_) => break,
                    _ => {}
                }
            }
            Some(out) = rx.recv() => {
                if socket.send(Message::Binary(out.into())).await.is_err() {
                    break;
                }
            }
        }
    }
    if registered {
        hub.peers.lock().unwrap().remove(&client_key);
        let gone = frame_peer_gone(&client_key);
        broadcast_except(&hub, &client_key, gone).await;
    }
}

fn frame_server_key(public: &[u8; 32]) -> Vec<u8> {
    let mut f = vec![FRAME_SERVER_KEY];
    f.extend_from_slice(public);
    f
}

fn frame_server_info() -> Vec<u8> {
    let mut f = vec![FRAME_SERVER_INFO, 0x01];
    f.extend_from_slice(&1u32.to_be_bytes());
    f
}

fn frame_peer_present(key: &[u8; 32]) -> Vec<u8> {
    let mut f = vec![FRAME_PEER_PRESENT];
    f.extend_from_slice(key);
    f
}

fn frame_peer_gone(key: &[u8; 32]) -> Vec<u8> {
    let mut f = vec![FRAME_PEER_GONE];
    f.extend_from_slice(key);
    f
}

async fn route_packet(hub: &DerpHub, src: &[u8; 32], data: &[u8]) {
    if data.len() < 34 {
        return;
    }
    let mut dst = [0u8; 32];
    dst.copy_from_slice(&data[1..33]);
    let payload = data.to_vec();
    let local = {
        let peers = hub.peers.lock().unwrap();
        peers.get(&dst).cloned()
    };
    if let Some(tx) = local {
        let _ = tx.send(payload);
        return;
    }
    let _ = hub.mesh_tx.send(payload);
    let _ = src;
}

async fn mesh_client_loop(
    peer_url: &str,
    hub: DerpHub,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
) {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::connect_async;
    use tokio_tungstenite::tungstenite::Message as WsMessage;

    let url = if peer_url.starts_with("ws://") || peer_url.starts_with("wss://") {
        peer_url.to_string()
    } else {
        format!("wss://{peer_url}/derp")
    };
    loop {
        if *shutdown.borrow() {
            break;
        }
        let Ok((ws, _)) = connect_async(&url).await else {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            continue;
        };
        let (mut write, mut read) = ws.split();
        let mut mesh_rx = hub.mesh_tx.subscribe();
        loop {
            tokio::select! {
                _ = shutdown.changed() => { if *shutdown.borrow() { return; } }
                out = mesh_rx.recv() => {
                    let Ok(frame) = out else { continue };
                    if write.send(WsMessage::Binary(frame.into())).await.is_err() {
                        break;
                    }
                }
                msg = read.next() => {
                    match msg {
                        Some(Ok(WsMessage::Binary(data))) if data.len() > 33 && data[0] == FRAME_SEND_PACKET => {
                            let mut dst = [0u8; 32];
                            dst.copy_from_slice(&data[1..33]);
                            let peers = hub.peers.lock().unwrap();
                            if let Some(tx) = peers.get(&dst) {
                                let _ = tx.send(data.to_vec());
                            }
                        }
                        Some(Ok(WsMessage::Close(_))) | None | Some(Err(_)) => break,
                        _ => {}
                    }
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
}

async fn broadcast_except(hub: &DerpHub, except: &[u8; 32], msg: Vec<u8>) {
    let peers = hub.peers.lock().unwrap();
    for (key, tx) in peers.iter() {
        if key != except {
            let _ = tx.send(msg.clone());
        }
    }
}
