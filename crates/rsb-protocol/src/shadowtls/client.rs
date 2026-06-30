//! ShadowTLS client dial (v1/v2/v3).

use crate::shadowtls::handshake::{handshake, handshake_v3};
use crate::shadowtls::v2::{HashReadConn, V2ClientConn};
use crate::shadowtls::v3::{VerifiedConn, V3HandshakeConn};
use crate::transport::{self, build_tls_config};
use rustls::client::Resumption;
use anyhow::{Context, Result};
use rsb_core::ProxyConn;
use serde_json::Value;
use std::sync::Arc;

pub async fn connect(
    version: u8,
    server: &str,
    port: u16,
    password: &str,
    tls: Option<&Value>,
    sni: Option<&str>,
    strict_mode: bool,
) -> Result<ProxyConn> {
    let sni = sni.unwrap_or(server);
    let insecure = tls
        .and_then(|t| t.get("insecure"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let cfg = build_tls_config(tls, insecure)?;

    match version {
        1 => {
            let stream = transport::tls_connect(server, port, tls, Some(sni)).await?;
            Ok(rsb_core::proxy_box(stream))
        }
        2 => connect_v2(server, port, password, cfg, sni).await,
        3 => connect_v3(server, port, password, tls, sni, strict_mode).await,
        other => anyhow::bail!("shadowtls: unsupported version {other}"),
    }
}

async fn connect_v2(
    server: &str,
    port: u16,
    password: &str,
    cfg: Arc<rustls::ClientConfig>,
    sni: &str,
) -> Result<ProxyConn> {
    let tcp = transport::tcp_connect(server, port).await?;
    let hash = HashReadConn::new(tcp, password);
    let hash = handshake(hash, cfg, sni).await.context("shadowtls v2 tls handshake")?;
    let auth = hash.sum();
    let tcp = hash.into_inner();
    Ok(rsb_core::proxy_box(V2ClientConn::new(tcp, auth)))
}

async fn connect_v3(
    server: &str,
    port: u16,
    password: &str,
    tls: Option<&Value>,
    sni: &str,
    strict_mode: bool,
) -> Result<ProxyConn> {
    let insecure = tls
        .and_then(|t| t.get("insecure"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut cfg = (*build_tls_config(tls, insecure)?).clone();
    cfg.resumption = Resumption::disabled();
    let cfg = Arc::new(cfg);
    let tcp = transport::tcp_connect(server, port).await?;
    let hs = V3HandshakeConn::new(tcp, password.to_string());
    let hs = handshake_v3(hs, cfg, sni, password)
        .await
        .context("shadowtls v3 tls handshake")?;
    let (mut tcp, mut auth) = hs.into_parts();
    tracing::debug!(
        authorized = auth.authorized,
        tls13 = auth.is_tls13,
        random_len = auth.server_random.len(),
        "shadowtls v3 handshake complete"
    );
    if strict_mode && !auth.is_tls13 {
        anyhow::bail!("shadowtls v3 strict_mode requires TLS 1.3");
    }
    if !auth.authorized {
        anyhow::bail!(
            "shadowtls v3: traffic hijacked or auth failed (tls13={}, random={})",
            auth.is_tls13,
            auth.server_random.len()
        );
    }
    if auth.server_random.len() != crate::shadowtls::constants::TLS_RANDOM_SIZE {
        anyhow::bail!("shadowtls v3: server random not captured");
    }
    if auth.is_tls13 {
        crate::shadowtls::v3::send_client_auth_frame(&mut tcp, password, &auth.server_random)
            .await
            .context("shadowtls v3 client auth frame")?;
        crate::shadowtls::v3::stash_post_handshake_records(&mut tcp, &mut auth)
            .await
            .context("shadowtls v3 post-handshake sync")?;
    }
    let verified = VerifiedConn::from_auth(tcp, auth).context("shadowtls v3 verified conn")?;
    Ok(rsb_core::proxy_box(verified))
}

#[cfg(test)]
mod live_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "live VPS"]
    async fn shadowtls_v3_live_tunnel() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("rsb_protocol::shadowtls=warn")),
            )
            .with_test_writer()
            .try_init();
        rustls::crypto::ring::default_provider().install_default().ok();
        let tls = serde_json::json!({
            "enabled": true,
            "server_name": "www.cloudflare.com",
            "insecure": true
        });
        let _conn = connect(
            3,
            "s.lulunet.cc",
            443,
            "st_test_ioIxewpGpPE",
            Some(&tls),
            Some("www.cloudflare.com"),
            false,
        )
        .await
        .expect("shadowtls v3 connect");
        // Handshake + auth success is sufficient; VPS detour is shadowsocks (not echo).
    }

    #[tokio::test]
    #[ignore = "live VPS"]
    async fn shadowtls_ss2022_live_roundtrip() {
        use shadowsocks::config::ServerConfig;
        use shadowsocks::crypto::CipherKind;
        use shadowsocks::relay::socks5::Address;
        use shadowsocks::relay::tcprelay::proxy_stream::ProxyClientStream;
        use shadowsocks::config::ServerType;
        use shadowsocks::context::Context as SsContext;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
            )
            .with_test_writer()
            .try_init();
        rustls::crypto::ring::default_provider().install_default().ok();
        let tls = serde_json::json!({
            "enabled": true,
            "server_name": "www.cloudflare.com",
            "insecure": true
        });
        let tunnel = connect(
            3,
            "s.lulunet.cc",
            443,
            "st_test_ioIxewpGpPE",
            Some(&tls),
            Some("www.cloudflare.com"),
            false,
        )
        .await
        .expect("shadowtls v3 connect");
        let ctx = SsContext::new_shared(ServerType::Local);
        let cfg = ServerConfig::new(
            ("127.0.0.1", 8388),
            "b2IBYlv44b8OEbdY8DPp2A==",
            CipherKind::AEAD2022_BLAKE3_AES_128_GCM,
        )
        .expect("ss config");
        let mut stream = ProxyClientStream::from_stream(
            ctx,
            tunnel,
            &cfg,
            Address::DomainNameAddress("www.cloudflare.com".into(), 443),
        );
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: www.cloudflare.com\r\nConnection: close\r\n\r\n")
            .await
            .expect("ss write");
        let mut buf = [0u8; 512];
        let n = tokio::time::timeout(
            std::time::Duration::from_secs(20),
            stream.read(&mut buf),
        )
        .await
        .expect("read timeout")
        .expect("ss read");
        assert!(n > 0, "expected HTTP response bytes");
        let resp = std::str::from_utf8(&buf[..n]).unwrap_or("");
        assert!(resp.contains("HTTP/") || resp.contains("html"), "unexpected: {resp:?}");
    }
}
