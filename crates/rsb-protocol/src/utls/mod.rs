//! uTLS-style byte-level ClientHello + TLS 1.3 completion.

mod hello;
mod tls13;

pub use hello::{
    build_client_hello, client_hello_random, generate_client_hello,
    generate_shadowtls_client_hello, hello_layout, parse_client_hello_key_share,
    pick_client_tls13_cipher, ClientHelloKeys, HelloLayout, Profile,
};
pub use tls13::{complete_tls13_camouflage, server, TlsIo, UtlsTlsStream};

use anyhow::{Context, Result};
use rustls::ClientConfig;
use serde_json::Value;
use tokio_rustls::TlsConnector;

pub fn fingerprint_name(tls: Option<&Value>) -> Option<&str> {
    tls.and_then(|t| {
        t.get("utls")
            .and_then(|u| u.get("fingerprint"))
            .or_else(|| t.get("fingerprint"))
            .and_then(|v| v.as_str())
    })
    .filter(|s| !s.is_empty())
}

pub fn utls_enabled(tls: Option<&Value>) -> bool {
    if tls
        .and_then(|t| t.get("utls"))
        .and_then(|u| u.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    fingerprint_name(tls).is_some()
}

pub fn apply_fingerprint(cfg: &mut ClientConfig, tls: Option<&Value>) {
    let Some(tls) = tls else {
        return;
    };
    let fp = fingerprint_name(Some(tls)).unwrap_or("");
    if let Some(profile) = Profile::parse(fp) {
        let alpn = match profile {
            Profile::Safari | Profile::Ios => vec![b"h2".to_vec(), b"http/1.1".to_vec()],
            _ => vec![b"h2".to_vec(), b"http/1.1".to_vec()],
        };
        cfg.alpn_protocols = alpn;
        cfg.enable_sni = true;
    }
}

pub async fn connect(
    server: &str,
    port: u16,
    tls: Option<&Value>,
    sni: Option<&str>,
) -> Result<TlsIo> {
    let insecure = tls
        .and_then(|t| t.get("insecure"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let server_name = sni
        .map(str::to_string)
        .or_else(|| {
            tls.and_then(|t| t.get("server_name"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| server.to_string());
    let profile = fingerprint_name(tls)
        .and_then(Profile::parse)
        .unwrap_or(Profile::Chrome);
    let keys = generate_client_hello(profile, &server_name);
    let tcp = crate::transport::tcp_connect(server, port).await?;
    match UtlsTlsStream::connect(tcp, &keys.hello, keys.secret, &server_name, insecure).await {
        Ok(utls) => Ok(TlsIo::Utls(utls)),
        Err(err) => {
            tracing::warn!(error = %err, "utls handshake failed, falling back to rustls");
            utls_rustls_fallback(server, port, tls, sni, insecure).await
        },
    }
}

async fn utls_rustls_fallback(
    server: &str,
    port: u16,
    tls: Option<&Value>,
    sni: Option<&str>,
    insecure: bool,
) -> Result<TlsIo> {
    let server_name = sni
        .map(str::to_string)
        .or_else(|| {
            tls.and_then(|t| t.get("server_name"))
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| server.to_string());
    let cfg = crate::transport::build_tls_config(tls, insecure)?;
    let tcp = crate::transport::tcp_connect(server, port).await?;
    let name = rustls::pki_types::ServerName::try_from(server_name.as_str())
        .map_err(|_| anyhow::anyhow!("invalid sni: {server_name}"))?
        .to_owned();
    let stream = TlsConnector::from(cfg)
        .connect(name, tcp)
        .await
        .context("rustls fallback")?;
    Ok(TlsIo::Rustls(stream))
}
