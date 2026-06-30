//! Legacy protocol inbounds (TLS fronted).

use crate::direct::parse_listen;
use crate::trojan::{build_tls_acceptor, serve_trojan};
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;

macro_rules! tls_inbound {
    ($name:ident, $kind:expr, $serve:path) => {
        pub struct $name {
            tag: String,
            listen: SocketAddr,
            password: String,
            cert: String,
            key: String,
            shutdown: tokio::sync::watch::Sender<bool>,
            handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
        }

        impl $name {
            pub fn new(tag: String, raw: Value) -> Result<Self> {
                let listen = parse_listen(&raw)?;
                let password = raw
                    .get("password")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let tls = raw.get("tls").context("tls required")?;
                let cert = tls
                    .get("certificate_path")
                    .or_else(|| tls.get("certificate"))
                    .and_then(|v| v.as_str())
                    .context("certificate")?
                    .to_string();
                let key = tls
                    .get("key_path")
                    .or_else(|| tls.get("key"))
                    .and_then(|v| v.as_str())
                    .context("key")?
                    .to_string();
                let (shutdown, _) = tokio::sync::watch::channel(false);
                Ok(Self {
                    tag,
                    listen,
                    password,
                    cert,
                    key,
                    shutdown,
                    handle: tokio::sync::Mutex::new(None),
                })
            }
        }

        #[async_trait]
        impl Inbound for $name {
            fn tag(&self) -> &str {
                &self.tag
            }
            fn kind(&self) -> &str {
                $kind
            }
            async fn start(&self) -> Result<(), BoxError> {
                let acceptor = build_tls_acceptor(&self.cert, &self.key)?;
                let listener = TcpListener::bind(self.listen).await?;
                tracing::info!(tag = %self.tag, %self.listen, kind = $kind, "legacy inbound listening");
                let password = self.password.clone();
                let mut shutdown = self.shutdown.subscribe();
                let handle = tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = shutdown.changed() => {
                                if *shutdown.borrow() { break; }
                            }
                            accept = listener.accept() => {
                                let Ok((stream, _)) = accept else { break };
                                let acceptor = acceptor.clone();
                                let password = password.clone();
                                tokio::spawn(async move {
                                    if let Err(err) = $serve(stream, acceptor, password).await {
                                        tracing::debug!(error = %err, "legacy client failed");
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
    };
}

async fn serve_legacy_tls(
    stream: tokio::net::TcpStream,
    acceptor: tokio_rustls::TlsAcceptor,
    password: String,
) -> Result<()> {
    let connections: rsb_core::SharedConnectionManager =
        Arc::new(rsb_core::ConnectionManager::new());
    serve_trojan(
        stream,
        acceptor,
        vec![crate::transport::sha224_hex(&password)],
        connections,
        "legacy".into(),
    )
    .await
}

tls_inbound!(
    HysteriaInbound,
    rsb_constant::TYPE_HYSTERIA,
    serve_legacy_tls
);
tls_inbound!(
    ShadowTlsInbound,
    rsb_constant::TYPE_SHADOWTLS,
    serve_legacy_tls
);
tls_inbound!(AnyTlsInbound, rsb_constant::TYPE_ANYTLS, serve_legacy_tls);
tls_inbound!(NaiveInbound, rsb_constant::TYPE_NAIVE, serve_legacy_tls);
