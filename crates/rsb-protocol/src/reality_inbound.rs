//! VLESS + REALITY native inbound (TLS1.3 server + session auth).

use crate::reality_session::{
    decode_reality_private_key, decode_short_ids, verify_reality_session,
};
use crate::utls::server::RealityServerStream;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound, SharedConnectionManager, UserLimits};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;
use x25519_dalek::StaticSecret;

pub struct RealityVlessInbound {
    tag: String,
    listen: SocketAddr,
    users: Vec<Uuid>,
    connections: SharedConnectionManager,
    private_key: StaticSecret,
    short_ids: Vec<[u8; 8]>,
    max_time_diff: u64,
    handshake_server: String,
    handshake_port: u16,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl RealityVlessInbound {
    pub fn new(tag: String, raw: Value, connections: SharedConnectionManager) -> Result<Self> {
        let listen = crate::direct::parse_listen(&raw)?;
        let mut users = Vec::new();
        if let Some(arr) = raw.get("users").and_then(|v| v.as_array()) {
            for u in arr {
                if let Some(id) = u.get("uuid").and_then(|v| v.as_str()) {
                    users.push(Uuid::parse_str(id)?);
                }
            }
        }
        anyhow::ensure!(!users.is_empty(), "reality vless: uuid/users required");
        let tls = raw.get("tls").context("reality vless: tls required")?;
        let reality = tls.get("reality").context("reality block")?;
        let pk_b64 = reality
            .get("private_key")
            .and_then(|v| v.as_str())
            .context("reality private_key")?;
        let private_key = decode_reality_private_key(pk_b64)?;
        let short_ids = decode_short_ids(tls)?;
        let max_time_diff = reality
            .get("max_time_diff")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let (handshake_server, handshake_port) = parse_handshake_dest(reality)?;
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen,
            users,
            connections,
            private_key,
            short_ids,
            max_time_diff,
            handshake_server,
            handshake_port,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }
}

fn parse_handshake_dest(reality: &Value) -> Result<(String, u16)> {
    if let Some(hs) = reality.get("handshake") {
        let server = hs
            .get("server")
            .and_then(|v| v.as_str())
            .context("reality handshake.server")?
            .to_string();
        let port = hs
            .get("server_port")
            .and_then(|v| v.as_u64())
            .unwrap_or(443) as u16;
        return Ok((server, port));
    }
    let server = reality
        .get("dest")
        .and_then(|v| v.as_str())
        .context("reality handshake dest")?
        .to_string();
    Ok((server, 443))
}

#[async_trait]
impl Inbound for RealityVlessInbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        rsb_constant::TYPE_VLESS
    }

    async fn start(&self) -> Result<(), BoxError> {
        let listener = TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, "reality vless inbound listening");
        let users = self.users.clone();
        let connections = self.connections.clone();
        let inbound_tag = self.tag.clone();
        let private_key = self.private_key.clone();
        let short_ids = self.short_ids.clone();
        let max_time_diff = self.max_time_diff;
        let hs_server = self.handshake_server.clone();
        let hs_port = self.handshake_port;
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    accept = listener.accept() => {
                        let Ok((stream, _)) = accept else { break };
                        let users = users.clone();
                        let connections = connections.clone();
                        let inbound_tag = inbound_tag.clone();
                        let private_key = private_key.clone();
                        let short_ids = short_ids.clone();
                        let hs_server = hs_server.clone();
                        tokio::spawn(async move {
                            if let Err(err) = serve_connection(
                                stream,
                                private_key,
                                short_ids,
                                max_time_diff,
                                hs_server,
                                hs_port,
                                users,
                                connections,
                                inbound_tag,
                            ).await {
                                tracing::warn!(error = %err, "reality vless client failed");
                            }
                        });
                    }
                }
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }

    async fn close(&self) -> Result<(), BoxError> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}

async fn serve_connection(
    mut stream: TcpStream,
    private_key: StaticSecret,
    short_ids: Vec<[u8; 8]>,
    max_time_diff: u64,
    hs_server: String,
    hs_port: u16,
    users: Vec<Uuid>,
    connections: SharedConnectionManager,
    inbound_tag: String,
) -> Result<()> {
    let client_hello = read_tls_record(&mut stream).await?;
    let session = match verify_reality_session(
        &client_hello,
        &private_key,
        &short_ids,
        max_time_diff,
    ) {
        Ok(s) => {
            tracing::info!(cipher = format!("{:#x}", s.cipher), "reality session verified");
            if std::env::var_os("RSB_DUMP_REALITY_HELLO").is_some() {
                let _ = std::fs::write("/tmp/rsb_last_hello.bin", &client_hello);
            }
            s
        },
        Err(err) => {
            tracing::warn!(error = %err, "reality auth failed, passthrough");
            return relay_passthrough(stream, client_hello, hs_server, hs_port).await;
        }
    };

    let tls = match mirror_and_accept(stream, &session, hs_server, hs_port, private_key).await {
        Ok(s) => {
            tracing::info!("reality tls handshake complete (dest mirror)");
            s
        }
        Err(err) => {
            tracing::warn!(error = %err, "reality mirror handshake failed");
            return Err(err);
        }
    };
    serve_vless_over_tls(tls, users, connections, &inbound_tag).await
}

async fn mirror_and_accept(
    stream: TcpStream,
    session: &crate::reality_session::VerifiedSession,
    hs_server: String,
    hs_port: u16,
    private_key: StaticSecret,
) -> Result<RealityServerStream> {
    match crate::reality_mirror::mirror_dest_handshake(
        &session.client_hello,
        &hs_server,
        hs_port,
    )
    .await
    {
        Ok(mirror) => {
            tracing::info!(
                cipher = format!("{:#x}", mirror.cipher),
                sh_len = mirror.server_hello.len(),
                "reality mirrored dest ServerHello"
            );
            crate::utls::server::accept_reality_mirror(stream, session, &mirror).await
        }
        Err(mirror_err) => {
            tracing::warn!(
                error = %mirror_err,
                "reality dest mirror failed, using local ServerHello"
            );
            crate::utls::server::accept_reality(stream, session, private_key).await
        }
    }
}

async fn read_tls_record(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut hdr = [0u8; 5];
    stream.read_exact(&mut hdr).await?;
    let len = u16::from_be_bytes([hdr[3], hdr[4]]) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    let mut out = hdr.to_vec();
    out.extend_from_slice(&body);
    Ok(out)
}

async fn relay_passthrough(
    mut client: TcpStream,
    client_hello: Vec<u8>,
    hs_server: String,
    hs_port: u16,
) -> Result<()> {
    let mut upstream =
        TcpStream::connect(format!("{hs_server}:{hs_port}"))
            .await
            .context("reality passthrough connect")?;
    upstream.write_all(&client_hello).await?;
    let (mut cr, mut cw) = client.split();
    let (mut ur, mut uw) = upstream.split();
    let c2u = tokio::io::copy(&mut cr, &mut uw);
    let u2c = tokio::io::copy(&mut ur, &mut cw);
    tokio::try_join!(c2u, u2c)?;
    Ok(())
}

async fn serve_vless_over_tls(
    mut tls: RealityServerStream,
    users: Vec<Uuid>,
    connections: SharedConnectionManager,
    inbound_tag: &str,
) -> Result<()> {
    let header = crate::vless::read_vless_request(&mut tls).await?;
    let mut cursor = header.as_slice();
    if cursor.is_empty() || cursor[0] != 0 {
        anyhow::bail!("invalid vless version");
    }
    cursor = &cursor[1..];
    if cursor.len() < 16 {
        anyhow::bail!("truncated vless uuid");
    }
    let uid = Uuid::from_bytes(cursor[..16].try_into()?);
    if !users.contains(&uid) {
        anyhow::bail!("vless auth failed");
    }
    cursor = &cursor[16..];
    let addon_len = cursor[0] as usize;
    cursor = &cursor[1 + addon_len..];
    if cursor.len() < 4 {
        anyhow::bail!("truncated vless header");
    }
    if cursor[0] != 1 {
        anyhow::bail!("unsupported vless command {}", cursor[0]);
    }
    let port = u16::from_be_bytes([cursor[1], cursor[2]]);
    let atyp = cursor[3];
    cursor = &cursor[4..];
    let (host, consumed) = crate::vless::read_address(cursor, atyp)?;
    cursor = &cursor[consumed..];
    // Respond immediately after auth so the client can start relaying (Xray-compatible).
    tls.write_all(&[0, 0]).await?;
    tls.flush().await?;
    let dest: SocketAddr = tokio::net::lookup_host(format!("{host}:{port}"))
        .await?
        .next()
        .with_context(|| format!("resolve {host}"))?;
    let mut remote = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        TcpStream::connect(dest),
    )
    .await
    .context("upstream connect timeout")??;
    if !cursor.is_empty() {
        remote.write_all(cursor).await?;
    }
    let (user_name, limits) = if let Some(rec) = connections.resolve_user(&uid) {
        (rec.name.clone(), rec.limits.clone())
    } else {
        (uid.to_string(), UserLimits::default())
    };
    let session = crate::user_relay::begin_for_uuid(
        &connections,
        inbound_tag,
        &uid,
        Some(dest),
        Some(host.clone()),
    )?;
    crate::inbound_proxy::relay_streams_user(&session, &mut tls, &mut remote).await
}
