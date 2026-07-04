//! Local RSQ end-to-end: QUIC server + outbound TCP/UDP relay.

use super::cert::write_dev_certs;
use super::client::RsqOutbound;
use super::server::{self, RsqServerConfig};
use anyhow::{Context, Result};
use rsb_core::{ConnectionManager, Outbound};
use serde_json::json;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tokio::task::JoinHandle;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

static E2E_ID: AtomicU32 = AtomicU32::new(0);

const PASSWORD: &str = "rsq-e2e-pass";
const OBFS_PASS: &str = "rsq-e2e-obfs";
const SNI: &str = "rsq.local";

fn install_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn pick_port() -> Result<u16> {
    let sock = std::net::UdpSocket::bind("127.0.0.1:0").context("bind ephemeral")?;
    Ok(sock.local_addr()?.port())
}

async fn spawn_tcp_echo() -> Result<SocketAddr> {
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

async fn spawn_udp_echo() -> Result<SocketAddr> {
    let socket = UdpSocket::bind("127.0.0.1:0").await?;
    let addr = socket.local_addr()?;
    tokio::spawn(async move {
        let mut buf = [0u8; 65535];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((n, peer)) => {
                    let _ = socket.send_to(&buf[..n], peer).await;
                }
                Err(_) => break,
            }
        }
    });
    Ok(addr)
}

async fn spawn_rsq_server_with_obfs(
    port: u16,
    cert_dir: &PathBuf,
    obfs_version: super::obfs::ObfsVersion,
    udp: bool,
) -> Result<JoinHandle<()>> {
    let (cert, key) = write_dev_certs(cert_dir, SNI)?;
    let cfg = Arc::new(RsqServerConfig {
        listen: format!("127.0.0.1:{port}").parse()?,
        inbound_tag: "rsq-e2e".into(),
        cert_path: cert.display().to_string(),
        key_path: key.display().to_string(),
        passwords: vec![PASSWORD.into()],
        up_mbps: 0,
        down_mbps: 0,
        udp,
        obfs: Some(Arc::new(super::obfs::RsqObfs::with_version(
            OBFS_PASS,
            obfs_version,
        ))),
        connections: Arc::new(ConnectionManager::new()),
    });
    Ok(tokio::spawn(async move {
        if let Err(err) = server::run(cfg).await {
            tracing::warn!(error = %err, "rsq e2e server exited");
        }
    }))
}

async fn spawn_rsq_server(port: u16, cert_dir: &PathBuf) -> Result<JoinHandle<()>> {
    spawn_rsq_server_with_obfs(port, cert_dir, super::obfs::ObfsVersion::V1, true).await
}

fn outbound_config(port: u16, password: &str) -> serde_json::Value {
    outbound_config_with_obfs(port, password, 1)
}

fn outbound_config_with_obfs(port: u16, password: &str, obfs_version: u64) -> serde_json::Value {
    json!({
        "server": "127.0.0.1",
        "server_port": port,
        "password": password,
        "warm_up": false,
        "tls": {
            "enabled": true,
            "server_name": SNI,
            "insecure": true
        },
        "obfs": { "enabled": true, "password": OBFS_PASS, "version": obfs_version }
    })
}

async fn setup_stack() -> Result<(u16, PathBuf, JoinHandle<()>)> {
    install_crypto();
    let port = pick_port()?;
    let id = E2E_ID.fetch_add(1, Ordering::Relaxed);
    let cert_dir = std::env::temp_dir().join(format!("rsq-e2e-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cert_dir);
    let server = spawn_rsq_server(port, &cert_dir).await?;
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    Ok((port, cert_dir, server))
}

async fn dial_tcp_with_retry(
    ob: &RsqOutbound,
    echo: SocketAddr,
) -> Result<rsb_core::ProxyConn> {
    let mut last = None;
    for _ in 0..12 {
        match ob.dial_tcp(echo, None).await {
            Ok(conn) => return Ok(conn),
            Err(err) => {
                last = Some(err);
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("rsq e2e dial failed").into()))
}

async fn dial_udp_with_retry(
    ob: &RsqOutbound,
    echo: SocketAddr,
) -> Result<rsb_core::ProxyUdpSocket> {
    let mut last = None;
    for _ in 0..12 {
        match ob.dial_udp(echo).await {
            Ok(sock) => return Ok(sock),
            Err(err) => {
                last = Some(err);
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("rsq e2e udp dial failed").into()))
}

#[tokio::test]
async fn rsq_local_tcp_echo() {
    let (port, cert_dir, _server) = setup_stack().await.expect("setup");
    let echo = spawn_tcp_echo().await.expect("tcp echo");
    let ob = RsqOutbound::new("e2e".into(), outbound_config(port, PASSWORD)).expect("outbound");
    let mut conn = dial_tcp_with_retry(&ob, echo).await.expect("dial tcp through rsq");
    conn.write_all(b"hello-rsq-tcp")
        .await
        .expect("write");
    let mut buf = vec![0u8; 64];
    let n = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        conn.read(&mut buf),
    )
    .await
    .expect("read timeout")
    .expect("read");
    assert_eq!(&buf[..n], b"hello-rsq-tcp");
    let _ = std::fs::remove_dir_all(cert_dir);
}

#[tokio::test]
async fn rsq_local_udp_echo() {
    let (port, cert_dir, _server) = setup_stack().await.expect("setup");
    let echo = spawn_udp_echo().await.expect("udp echo");
    let ob = RsqOutbound::new("e2e".into(), outbound_config(port, PASSWORD)).expect("outbound");
    let sock = dial_udp_with_retry(&ob, echo).await.expect("dial udp");
    sock.send_to(b"ping-udp", echo).await.expect("udp send");
    let mut buf = [0u8; 64];
    let (n, from) = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        sock.recv_from(&mut buf),
    )
    .await
    .expect("udp recv timeout")
    .expect("udp recv");
    assert_eq!(&buf[..n], b"ping-udp");
    assert_eq!(from, echo);
    let _ = std::fs::remove_dir_all(cert_dir);
}

#[tokio::test]
async fn rsq_local_udp_large_fragment() {
    let (port, cert_dir, _server) = setup_stack().await.expect("setup");
    let echo = spawn_udp_echo().await.expect("udp echo");
    let ob = RsqOutbound::new("e2e".into(), outbound_config(port, PASSWORD)).expect("outbound");
    let sock = dial_udp_with_retry(&ob, echo).await.expect("dial udp");
    let payload: Vec<u8> = (0..2500).map(|i| (i % 251) as u8).collect();
    sock.send_to(&payload, echo).await.expect("udp send large");
    let mut buf = vec![0u8; 4096];
    let (n, _) = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        sock.recv_from(&mut buf),
    )
    .await
    .expect("udp recv timeout")
    .expect("udp recv");
    assert_eq!(&buf[..n], &payload[..]);
    let _ = std::fs::remove_dir_all(cert_dir);
}

#[tokio::test]
async fn rsq_auth_rejects_bad_password() {
    let (port, cert_dir, _server) = setup_stack().await.expect("setup");
    let ob = RsqOutbound::new(
        "e2e-bad".into(),
        outbound_config(port, "wrong-password"),
    )
    .expect("outbound");
    let echo = spawn_tcp_echo().await.expect("tcp echo");
    let result = ob.dial_tcp(echo, None).await;
    assert!(result.is_err(), "bad password should be rejected");
    let _ = std::fs::remove_dir_all(cert_dir);
}

#[tokio::test]
async fn rsq_local_obfs_v2_tcp_echo() {
    install_crypto();
    let port = pick_port().expect("port");
    let id = E2E_ID.fetch_add(1, Ordering::Relaxed);
    let cert_dir = std::env::temp_dir().join(format!("rsq-e2e-v2-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cert_dir);
    let _server = spawn_rsq_server_with_obfs(port, &cert_dir, super::obfs::ObfsVersion::V2, true)
        .await
        .expect("server");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let echo = spawn_tcp_echo().await.expect("tcp echo");
    let ob = RsqOutbound::new(
        "e2e-v2".into(),
        outbound_config_with_obfs(port, PASSWORD, 2),
    )
    .expect("outbound");
    let mut conn = dial_tcp_with_retry(&ob, echo).await.expect("dial");
    conn.write_all(b"hello-rsq-obfs-v2").await.expect("write");
    let mut buf = vec![0u8; 64];
    let n = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        conn.read(&mut buf),
    )
    .await
    .expect("read timeout")
    .expect("read");
    assert_eq!(&buf[..n], b"hello-rsq-obfs-v2");
    let _ = std::fs::remove_dir_all(cert_dir);
}

#[tokio::test]
async fn rsq_udp_disabled_rejected() {
    install_crypto();
    let port = pick_port().expect("port");
    let id = E2E_ID.fetch_add(1, Ordering::Relaxed);
    let cert_dir = std::env::temp_dir().join(format!("rsq-e2e-udp-off-{}-{id}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cert_dir);
    let _server = spawn_rsq_server_with_obfs(port, &cert_dir, super::obfs::ObfsVersion::V1, false)
        .await
        .expect("server");
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    let echo = spawn_udp_echo().await.expect("udp echo");
    let ob = RsqOutbound::new("e2e-udp-off".into(), outbound_config(port, PASSWORD)).expect("outbound");
    match ob.dial_udp(echo).await {
        Err(err) => assert!(err.to_string().contains("udp disabled")),
        Ok(_) => panic!("udp should be disabled"),
    }
    let _ = std::fs::remove_dir_all(cert_dir);
}
