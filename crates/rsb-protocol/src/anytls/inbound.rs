//! sing-box compatible AnyTLS inbound (via anytls-rs server).

use crate::anytls::UserRelayHandler;
use crate::direct::parse_listen;
use crate::trojan::build_tls_acceptor;
use anyhow::{Context, Result};
use anytls_rs::padding::PaddingFactory;
use anytls_rs::server::Server;
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound, SharedConnectionManager};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct AnyTlsInbound {
    tag: String,
    listen: SocketAddr,
    password: String,
    acceptor: tokio_rustls::TlsAcceptor,
    connections: SharedConnectionManager,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AnyTlsInbound {
    pub fn new(
        tag: String,
        raw: Value,
        connections: SharedConnectionManager,
    ) -> Result<Self> {
        let listen = parse_listen(&raw)?;
        let password = extract_password(&raw)?;
        let tls = raw.get("tls").context("anytls inbound: tls required")?;
        if !tls.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true) {
            anyhow::bail!("anytls inbound: tls.enabled is required");
        }
        let cert = tls
            .get("certificate_path")
            .or_else(|| tls.get("certificate"))
            .and_then(|v| v.as_str())
            .context("anytls inbound: certificate_path")?
            .to_string();
        let key = tls
            .get("key_path")
            .or_else(|| tls.get("key"))
            .and_then(|v| v.as_str())
            .context("anytls inbound: key_path")?
            .to_string();
        let acceptor = build_tls_acceptor(&cert, &key)?;
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen,
            password,
            acceptor,
            connections,
            shutdown,
            handle: Mutex::new(None),
        })
    }
}

fn extract_password(raw: &Value) -> Result<String> {
    if let Some(arr) = raw.get("users").and_then(|v| v.as_array()) {
        for u in arr {
            if let Some(p) = u.get("password").and_then(|v| v.as_str()) {
                return Ok(p.to_string());
            }
        }
    }
    raw.get("password")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .context("anytls inbound: users/password required")
}

#[async_trait]
impl Inbound for AnyTlsInbound {
    fn tag(&self) -> &str {
        &self.tag
    }

    fn kind(&self) -> &str {
        rsb_constant::TYPE_ANYTLS
    }

    async fn start(&self) -> Result<(), BoxError> {
        let padding = PaddingFactory::default();
        let acceptor = Arc::new(self.acceptor.clone());
        let handler = Arc::new(UserRelayHandler::new(
            self.connections.clone(),
            self.tag.clone(),
            self.password.clone(),
        ));
        let server = Server::new(&self.password, acceptor, padding, None).with_handler(handler);
        let listen = self.listen.to_string();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::debug!("anytls inbound shutdown");
                    }
                }
                res = server.listen(&listen) => {
                    if let Err(err) = res {
                        tracing::error!(error = %err, "anytls server exited");
                    }
                }
            }
        });
        tracing::info!(tag = %self.tag, listen = %self.listen, "anytls inbound listening");
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
