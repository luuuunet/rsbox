//! Dial an outbound's server address through another outbound (sing-box `detour`).

use crate::transport::{self, TlsIo};
use rsb_core::{proxy_box, BoxError, ProxyConn, SharedOutboundManager};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;

pub async fn dial_tcp_via_detour(
    shared: &SharedOutboundManager,
    detour_tag: &str,
    server: SocketAddr,
    domain: Option<&str>,
) -> Result<ProxyConn, BoxError> {
    shared
        .get()?
        .get(detour_tag)?
        .dial_tcp(server, domain)
        .await
}

pub fn detour_tag(raw: &serde_json::Value) -> Option<String> {
    raw.get("detour")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

pub async fn resolve_server_addr(server: &str, port: u16) -> anyhow::Result<SocketAddr> {
    tokio::net::lookup_host(format!("{server}:{port}"))
        .await?
        .next()
        .ok_or_else(|| anyhow::anyhow!("no address for {server}:{port}"))
}

fn tls_enabled(tls: Option<&Value>) -> bool {
    tls.map(|t| t.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true))
        .unwrap_or(false)
}

/// Open a connection to `server:port`, optionally via detour and/or TLS.
pub async fn dial_server_link(
    shared: &SharedOutboundManager,
    detour: Option<&str>,
    server: &str,
    port: u16,
    tls: Option<&Value>,
    sni: Option<&str>,
) -> Result<ProxyConn, BoxError> {
    if let Some(tag) = detour {
        let addr = resolve_server_addr(server, port)
            .await
            .map_err(Into::<BoxError>::into)?;
        let stream = dial_tcp_via_detour(shared, tag, addr, sni).await?;
        if tls_enabled(tls) {
            transport::tls_over_stream(stream, tls, server, sni)
                .await
                .map_err(Into::<BoxError>::into)
        } else {
            Ok(stream)
        }
    } else if tls_enabled(tls) {
        Ok(proxy_box(
            transport::tls_connect(server, port, tls, sni)
                .await
                .map_err(Into::<BoxError>::into)?,
        ))
    } else {
        Ok(proxy_box(
            transport::tcp_connect(server, port)
                .await
                .map_err(Into::<BoxError>::into)?,
        ))
    }
}

/// Same as [`dial_server_link`] but returns [`TlsIo`] when TLS is enabled without detour.
pub async fn dial_server_tls(
    shared: &SharedOutboundManager,
    detour: Option<&str>,
    server: &str,
    port: u16,
    tls: Option<&Value>,
    sni: Option<&str>,
) -> Result<TlsIo, BoxError> {
    if detour.is_some() {
        let conn = dial_server_link(shared, detour, server, port, tls, sni).await?;
        // Already wrapped TlsIo inside ProxyConn when tls+detour
        anyhow::bail!("use dial_server_link for detour+tls");
    }
    transport::tls_connect(server, port, tls, sni)
        .await
        .map_err(Into::into)
}
