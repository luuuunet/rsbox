//! Local RST end-to-end: QUIC/h3 server + outbound TCP echo.

use crate::rsq::write_dev_certs;
use rsb_core::{ConnectionManager, Inbound, Outbound};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const PASSWORD: &str = "rst-e2e-pass";
const SNI: &str = "rst.local";

#[tokio::test]
async fn rst_h3_local_tcp_echo() {
    let _ = tracing_subscriber::fmt::try_init();

    let echo = TcpListener::bind("127.0.0.1:0").await.expect("echo bind");
    let echo_port = echo.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = echo.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                loop {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if sock.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });

    let id = std::process::id();
    let cert_dir = std::env::temp_dir().join(format!("rst-e2e-{id}"));
    let (cert, key) = write_dev_certs(&cert_dir, SNI).expect("certs");
    let listen_port = {
        let l = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };

    let connections = Arc::new(ConnectionManager::new());
    let inbound = super::RstInbound::new(
        "rst-e2e".into(),
        serde_json::json!({
            "listen": "127.0.0.1",
            "listen_port": listen_port,
            "password": PASSWORD,
            "up_mbps": 100,
            "down_mbps": 100,
            "udp": true,
            "allow_private": true,
            "tls": {
                "enabled": true,
                "certificate_path": cert.to_string_lossy(),
                "key_path": key.to_string_lossy(),
            }
        }),
        connections,
    )
    .expect("inbound");
    inbound.start().await.expect("start");
    tokio::time::sleep(Duration::from_millis(400)).await;

    let outbound = super::RstOutbound::new(
        "rst".into(),
        serde_json::json!({
            "server": "127.0.0.1",
            "server_port": listen_port,
            "password": PASSWORD,
            "up_mbps": 100,
            "down_mbps": 100,
            "brutal": true,
            "tls": { "server_name": SNI, "insecure": true }
        }),
    )
    .expect("outbound");

    let dest: std::net::SocketAddr = format!("127.0.0.1:{echo_port}").parse().unwrap();
    let mut conn = outbound
        .dial_tcp(dest, None)
        .await
        .expect("dial tcp through rst");
    conn.write_all(b"hello-rst-h3").await.expect("write");
    let mut buf = [0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(8), conn.read(&mut buf))
        .await
        .expect("timeout")
        .expect("read");
    assert_eq!(&buf[..n], b"hello-rst-h3");

    let _ = inbound.close().await;
}
