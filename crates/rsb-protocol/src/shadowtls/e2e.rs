//! Local ShadowTLS v3 end-to-end: mock TLS1.3 handshake server + echo detour.

use crate::shadowtls::client::connect;
use crate::shadowtls::server::{serve_v3, V3ServerConfig};
use anyhow::{Context, Result};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

const SNI: &str = "mock.cf.test";
const PASSWORD: &str = "test-password-e2e";

fn mock_cf_acceptor() -> Result<TlsAcceptor> {
    let cert = rcgen::generate_simple_self_signed(vec![SNI.into()]).context("rcgen")?;
    let cert_der = CertificateDer::from(cert.cert.der().to_vec());
    let key_der = PrivateKeyDer::Pkcs8(cert.key_pair.serialize_der().into());
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .context("tls server config")?;
    Ok(TlsAcceptor::from(Arc::new(cfg)))
}

async fn spawn_mock_cf() -> Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let acceptor = mock_cf_acceptor()?;
    tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let mut buf = [0u8; 4096];
                loop {
                    match tls.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if tls.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
    });
    Ok(addr)
}

async fn spawn_echo_detour() -> Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
    });
    Ok(addr)
}

async fn spawn_shadowtls_inbound(cf: SocketAddr, detour: SocketAddr) -> Result<SocketAddr> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let cfg = V3ServerConfig {
        users: vec![("u1".into(), PASSWORD.into())],
        handshake_server: cf.ip().to_string(),
        handshake_port: cf.port(),
        strict_mode: false,
        detour,
    };
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let cfg = cfg.clone();
            tokio::spawn(async move {
                if let Err(err) = serve_v3(
                    stream,
                    cfg,
                    std::sync::Arc::new(rsb_core::ConnectionManager::new()),
                    "e2e".into(),
                )
                .await {
                    tracing::warn!(error = %err, "local e2e serve_v3 failed");
                }
            });
        }
    });
    Ok(addr)
}

fn install_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[tokio::test]
async fn shadowtls_v3_local_e2e_handshake_and_relay() {
    install_crypto();

    let cf = spawn_mock_cf().await.expect("mock cf");
    let detour = spawn_echo_detour().await.expect("echo detour");
    let st = spawn_shadowtls_inbound(cf, detour)
        .await
        .expect("shadowtls inbound");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let tls = serde_json::json!({
        "enabled": true,
        "server_name": SNI,
        "insecure": true
    });

    let mut conn = connect(
        3,
        "127.0.0.1",
        st.port(),
        PASSWORD,
        Some(&tls),
        Some(SNI),
        false,
    )
    .await
    .expect("shadowtls v3 connect through local stack");

    conn.write_all(b"hello-e2e")
        .await
        .expect("write payload");
    let mut buf = vec![0u8; 64];
    let n = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        conn.read(&mut buf),
    )
    .await
    .expect("read timeout")
    .expect("read echo");
    assert_eq!(&buf[..n], b"hello-e2e");
}

/// Full local stack with real Cloudflare as TLS handshake server (needs internet).
#[tokio::test]
#[ignore = "needs internet"]
async fn shadowtls_v3_local_e2e_real_cloudflare() {
    install_crypto();

    let detour = spawn_echo_detour().await.expect("echo detour");
    let cf_host = "www.cloudflare.com";
    let cf_port = 443u16;
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let st_addr = listener.local_addr().expect("addr");
    let cfg = V3ServerConfig {
        users: vec![("u1".into(), PASSWORD.into())],
        handshake_server: cf_host.into(),
        handshake_port: cf_port,
        strict_mode: false,
        detour,
    };
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let cfg = cfg.clone();
            tokio::spawn(async move {
                if let Err(err) = serve_v3(
                    stream,
                    cfg,
                    std::sync::Arc::new(rsb_core::ConnectionManager::new()),
                    "e2e".into(),
                )
                .await {
                    tracing::warn!(error = %err, "real-cf e2e serve_v3 failed");
                }
            });
        }
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let tls = serde_json::json!({
        "enabled": true,
        "server_name": cf_host,
        "insecure": false
    });

    let mut conn = connect(
        3,
        "127.0.0.1",
        st_addr.port(),
        PASSWORD,
        Some(&tls),
        Some(cf_host),
        false,
    )
    .await
    .expect("shadowtls v3 connect through local stack + real CF");

    conn.write_all(b"hello-cf-e2e")
        .await
        .expect("write payload");
    let mut buf = vec![0u8; 64];
    let n = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        conn.read(&mut buf),
    )
    .await
    .expect("read timeout")
    .expect("read echo");
    assert_eq!(&buf[..n], b"hello-cf-e2e");
}

/// Dial mock CF directly with the same rustls client settings (control test).
#[tokio::test]
async fn shadowtls_v3_local_control_rustls_to_mock_cf() {
    install_crypto();

    let cf = spawn_mock_cf().await.expect("mock cf");
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let tls = serde_json::json!({
        "enabled": true,
        "server_name": SNI,
        "insecure": true
    });
    let mut stream =
        crate::transport::tls_connect("127.0.0.1", cf.port(), Some(&tls), Some(SNI))
            .await
            .expect("direct tls to mock cf");
    stream
        .write_all(b"direct")
        .await
        .expect("write");
    let mut buf = [0u8; 8];
    let n = stream.read(&mut buf).await.expect("read");
    assert_eq!(&buf[..n], b"direct");
}
