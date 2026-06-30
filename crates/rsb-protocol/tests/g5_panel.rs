//! G5 panel integration: user traffic stats via v2ray HTTP API.

use rsb_config::Options;
use rsb_protocol::RsBox;
use shadowsocks::config::ServerConfig;
use shadowsocks::config::ServerType;
use shadowsocks::context::Context as SsContext;
use shadowsocks::crypto::CipherKind;
use shadowsocks::relay::socks5::Address;
use shadowsocks::relay::tcprelay::proxy_stream::ProxyClientStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const USER_NAME: &str = "g5user@test.com";
const SS_PASSWORD: &str = "g5-test-password";
const SS_PORT: u16 = 18443;
const V2RAY_PORT: u16 = 10086;

fn test_config() -> String {
    format!(
        r#"{{
  "log": {{ "level": "warn" }},
  "inbounds": [{{
    "type": "shadowsocks",
    "tag": "ss-g5",
    "listen": "127.0.0.1",
    "listen_port": {SS_PORT},
    "method": "chacha20-ietf-poly1305",
    "name": "{USER_NAME}",
    "password": "{SS_PASSWORD}",
    "users": [{{
      "name": "{USER_NAME}",
      "password": "{SS_PASSWORD}",
      "conn_limit": 2,
      "speed_mbps": 100,
      "traffic_limit_gb": 10
    }}]
  }}],
  "outbounds": [{{ "type": "direct", "tag": "direct" }}],
  "route": {{ "final": "direct" }},
  "experimental": {{
    "v2ray_api": {{ "listen": "127.0.0.1:{V2RAY_PORT}" }}
  }}
}}"#
    )
}

async fn spawn_echo_server() -> std::net::SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind echo");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                loop {
                    let Ok(n) = stream.read(&mut buf).await else {
                        break;
                    };
                    if n == 0 {
                        break;
                    }
                    if stream.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    addr
}

async fn ss_roundtrip(echo_addr: std::net::SocketAddr, payload: &[u8]) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let ctx = SsContext::new_shared(ServerType::Local);
    let cfg = ServerConfig::new(
        ("127.0.0.1", SS_PORT),
        SS_PASSWORD,
        CipherKind::CHACHA20_POLY1305,
    )
    .expect("ss server config");
    let mut stream = ProxyClientStream::connect(
        ctx,
        &cfg,
        Address::from(echo_addr),
    )
    .await
    .expect("ss connect");
    for chunk in payload.chunks(1024) {
        stream.write_all(chunk).await.expect("write chunk");
    }
    stream.flush().await.expect("flush");
    let mut resp = Vec::with_capacity(payload.len());
    let mut tmp = [0u8; 1024];
    while resp.len() < payload.len() {
        let n = stream.read(&mut tmp).await.expect("read");
        if n == 0 {
            break;
        }
        resp.extend_from_slice(&tmp[..n]);
    }
    assert_eq!(resp.len(), payload.len());
    assert_eq!(resp, payload);
}

fn stat_value(body: &serde_json::Value, suffix: &str) -> u64 {
    let pattern = format!("user>>>{USER_NAME}>>>traffic>>>{suffix}");
    body.get("stat")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some(pattern.as_str()))
        .and_then(|e| e.get("value"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

#[tokio::test]
async fn g5_v2ray_stats_after_shadowsocks_traffic() {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let echo_addr = spawn_echo_server().await;
    let options = Options::from_json(&test_config()).expect("parse config");
    let instance = RsBox::new(options).await.expect("rsbox new");
    let mut v2ray = rsb_api::V2RayApiServer::start(
        &serde_json::json!({ "listen": format!("127.0.0.1:{V2RAY_PORT}") }),
        instance.connections(),
    )
    .await
    .expect("v2ray api");

    instance.start().await.expect("rsbox start");
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    let payload = vec![0xABu8; 8192];
    ss_roundtrip(echo_addr, &payload).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let url = format!(
        "http://127.0.0.1:{V2RAY_PORT}/stats?pattern=user>>>{USER_NAME}&reset=false"
    );
    let resp = reqwest::get(&url).await.expect("stats http");
    let text = resp.text().await.expect("stats body");
    let body: serde_json::Value = serde_json::from_str(&text).expect("stats json");

    let uplink = stat_value(&body, "uplink");
    let downlink = stat_value(&body, "downlink");
    assert!(
        uplink >= payload.len() as u64,
        "expected uplink >= {}, got {uplink}; body={body}",
        payload.len()
    );
    assert!(
        downlink >= payload.len() as u64,
        "expected downlink >= {}, got {downlink}; body={body}",
        payload.len()
    );

    v2ray.stop();
    instance.close().await.ok();
}
