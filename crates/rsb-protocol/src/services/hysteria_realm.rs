//! Hysteria Realm UDP service (NAT traversal + token auth).

use super::listen::parse_listen;
use anyhow::Result;
use blake3::Hasher;
use rand::RngCore;
use serde_json::Value;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tokio::net::UdpSocket;
use tracing::info;

const REALM_MAGIC: &[u8; 4] = b"HYR\x01";
const REALM_CMD_PING: u8 = 0x01;
const REALM_CMD_PONG: u8 = 0x02;
const REALM_CMD_REGISTER: u8 = 0x03;
const REALM_CMD_PEER: u8 = 0x04;
const REALM_CMD_NAT: u8 = 0x05;

pub struct HysteriaRealmService {
    tag: String,
    listen: SocketAddr,
    realm: String,
    secret: Vec<u8>,
    peers: Arc<Mutex<HashMap<SocketAddr, Vec<u8>>>>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl HysteriaRealmService {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let realm = raw
            .get("realm")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();
        let secret = raw
            .get("secret")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .as_bytes()
            .to_vec();
        Ok(Self {
            tag,
            listen: parse_listen(&raw)?,
            realm,
            secret,
            peers: Arc::new(Mutex::new(HashMap::new())),
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }

    pub async fn start(&self) -> Result<()> {
        let socket = UdpSocket::bind(self.listen).await?;
        info!(tag = %self.tag, %self.listen, realm = %self.realm, "hysteria-realm listening");
        let realm = self.realm.clone();
        let secret = self.secret.clone();
        let peers = self.peers.clone();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            let mut buf = [0u8; 2048];
            loop {
                tokio::select! {
                    _ = shutdown.changed() => { if *shutdown.borrow() { break; } }
                    recv = socket.recv_from(&mut buf) => {
                        let Ok((n, peer)) = recv else { break };
                        if n < 5 || &buf[..4] != REALM_MAGIC {
                            continue;
                        }
                        let cmd = buf[4];
                        let payload = &buf[5..n];
                        match cmd {
                            REALM_CMD_PING => {
                                let mut resp = REALM_MAGIC.to_vec();
                                resp.push(REALM_CMD_PONG);
                                resp.extend_from_slice(realm.as_bytes());
                                let _ = socket.send_to(&resp, peer).await;
                            }
                            REALM_CMD_REGISTER if verify_token(payload, &secret, &realm) => {
                                let id = payload.get(32..).unwrap_or(payload).to_vec();
                                peers.lock().unwrap().insert(peer, id);
                                let mut resp = REALM_MAGIC.to_vec();
                                resp.push(REALM_CMD_PONG);
                                resp.extend_from_slice(b"ok");
                                let _ = socket.send_to(&resp, peer).await;
                            }
                            REALM_CMD_PEER if payload.len() >= 32 => {
                                let target_id = &payload[..32];
                                let forward = {
                                    peers.lock().unwrap().iter().find_map(|(addr, id)| {
                                        if id.as_slice() == target_id {
                                            Some(*addr)
                                        } else {
                                            None
                                        }
                                    })
                                };
                                if let Some(target) = forward {
                                    let mut out = REALM_MAGIC.to_vec();
                                    out.push(REALM_CMD_PEER);
                                    out.extend_from_slice(payload);
                                    let _ = socket.send_to(&out, target).await;
                                }
                            }
                            REALM_CMD_NAT if payload.len() >= 32 && verify_token(payload, &secret, &realm) => {
                                let target_id = &payload[..32];
                                let peer_addr = {
                                    peers.lock().unwrap().iter().find_map(|(addr, id)| {
                                        if id.starts_with(target_id) {
                                            Some(*addr)
                                        } else {
                                            None
                                        }
                                    })
                                };
                                let mut resp = REALM_MAGIC.to_vec();
                                resp.push(REALM_CMD_NAT);
                                if let Some(addr) = peer_addr {
                                    resp.extend_from_slice(&addr.ip().to_string().into_bytes());
                                    resp.push(b':');
                                    resp.extend_from_slice(addr.port().to_string().as_bytes());
                                }
                                let _ = socket.send_to(&resp, peer).await;
                            }
                            _ => {}
                        }
                    }
                }
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}

fn verify_token(payload: &[u8], secret: &[u8], realm: &str) -> bool {
    if payload.len() < 32 {
        return secret.is_empty();
    }
    let mut h = Hasher::new();
    h.update(secret);
    h.update(realm.as_bytes());
    h.update(&payload[32..]);
    h.finalize().as_bytes()[..32] == payload[..32]
}

pub fn build_register_token(secret: &[u8], realm: &str, id: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; 16];
    rand::rng().fill_bytes(&mut nonce);
    let mut h = Hasher::new();
    h.update(secret);
    h.update(realm.as_bytes());
    h.update(id);
    h.update(&nonce);
    let mac = h.finalize();
    let mut out = mac.as_bytes()[..32].to_vec();
    out.extend_from_slice(id);
    out.extend_from_slice(&nonce);
    out
}
