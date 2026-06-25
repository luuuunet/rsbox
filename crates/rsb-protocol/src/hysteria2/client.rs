use super::{auth, obfs, obfs_socket, protocol, udp_client};
use crate::transport;
use anyhow::{Context, Result};
use async_trait::async_trait;
use h3_quinn::Connection;
use http::{Request, StatusCode};
use quinn::{ClientConfig, Endpoint, TransportConfig};
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket, SplitProxy};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static SESSION_COUNTER: AtomicU32 = AtomicU32::new(1);

pub struct Hysteria2Outbound {
    tag: String,
    server: String,
    port: u16,
    password: String,
    up_mbps: u32,
    down_mbps: u32,
    sni: Option<String>,
    insecure: bool,
    obfs: Option<Arc<obfs::Salamander>>,
    connection: tokio::sync::Mutex<Option<Arc<quinn::Connection>>>,
    // 保持 H3 driver 和 send_request 活着，防止连接关闭
    _h3_keep_alive:
        tokio::sync::Mutex<Option<(tokio::task::JoinHandle<()>, Box<dyn std::any::Any + Send>)>>,
}

impl Hysteria2Outbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let tls = raw.get("tls");
        let obfs = raw.get("obfs").and_then(|o| {
            o.get("password")
                .and_then(|v| v.as_str())
                .map(|p| Arc::new(obfs::Salamander::new(p)))
        });
        Ok(Self {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("hysteria2: server required")?
                .to_string(),
            port: raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("hysteria2: server_port required")? as u16,
            password: raw
                .get("password")
                .and_then(|v| v.as_str())
                .context("hysteria2: password required")?
                .to_string(),
            up_mbps: raw.get("up_mbps").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            down_mbps: raw.get("down_mbps").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            sni: tls
                .and_then(|t| t.get("server_name"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            insecure: tls
                .and_then(|t| t.get("insecure"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            obfs,
            connection: tokio::sync::Mutex::new(None),
            _h3_keep_alive: tokio::sync::Mutex::new(None),
        })
    }

    async fn get_connection(&self) -> Result<Arc<quinn::Connection>> {
        let mut guard = self.connection.lock().await;
        if let Some(conn) = guard.as_ref() {
            if conn.close_reason().is_none() {
                return Ok(conn.clone());
            }
        }
        let conn = Arc::new(connect_and_auth(self).await?);
        *guard = Some(conn.clone());
        Ok(conn)
    }
}

async fn connect_and_auth(ob: &Hysteria2Outbound) -> Result<quinn::Connection> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();
    let sni = ob.sni.clone().unwrap_or_else(|| ob.server.clone());
    let tls_cfg = if ob.insecure {
        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(transport::SkipVerifier))
            .with_no_client_auth()
    } else {
        rustls::ClientConfig::builder()
            .with_root_certificates({
                let mut roots = rustls::RootCertStore::empty();
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                roots
            })
            .with_no_client_auth()
    };
    let mut tls_cfg = tls_cfg;
    tls_cfg.alpn_protocols = vec![b"h3".to_vec()];
    let mut client_cfg = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls_cfg)?,
    ));
    let mut transport_cfg = TransportConfig::default();
    transport_cfg.keep_alive_interval(Some(Duration::from_secs(15)));
    client_cfg.transport_config(Arc::new(transport_cfg));

    let addr = tokio::net::lookup_host(format!("{}:{}", ob.server, ob.port))
        .await
        .context("resolve hysteria2 server")?
        .next()
        .context("no hysteria2 server address")?;

    let connection = if let Some(ref obfs) = ob.obfs {
        let (mut endpoint, _) =
            obfs_socket::endpoint_with_obfs("0.0.0.0:0".parse()?, obfs.clone())?;
        endpoint.set_default_client_config(client_cfg);
        endpoint
            .connect(addr, &sni)?
            .await
            .context("quic connect")?
    } else {
        let mut endpoint = Endpoint::client("0.0.0.0:0".parse()?)?;
        endpoint.set_default_client_config(client_cfg);
        endpoint
            .connect(addr, &sni)?
            .await
            .context("quic connect")?
    };
    authenticate(&connection, &ob.password, ob.up_mbps).await?;
    Ok(connection)
}

async fn authenticate(connection: &quinn::Connection, password: &str, up_mbps: u32) -> Result<()> {
    let h3_conn = Connection::new(connection.clone());
    let (mut driver, mut send_request) = h3::client::new(h3_conn).await.context("h3 client")?;

    // 在单独的任务中驱动 H3 连接 - 重要：不要 abort 这个任务！
    tokio::spawn(async move {
        let _ = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
    });

    let mut req = Request::builder()
        .method("POST")
        .uri("https://hysteria/auth")
        .header("hysteria-auth", password);
    if up_mbps > 0 {
        req = req.header(
            "hysteria-cc-rx",
            (up_mbps as u64 * auth::MBPS_TO_BPS).to_string(),
        );
    }
    let mut stream = send_request.send_request(req.body(())?).await?;
    stream.finish().await?;
    let resp = stream.recv_response().await?;

    tracing::debug!("hysteria2: auth response status = {}", resp.status());

    if resp.status() != StatusCode::from_u16(233).unwrap() {
        anyhow::bail!("hysteria2 auth failed: {}", resp.status());
    }

    // 关键修复：不要 drop stream 和 send_request
    // 使用 std::mem::forget 防止它们被 drop
    std::mem::forget(stream);
    std::mem::forget(send_request);

    tracing::debug!("hysteria2: authentication completed, kept H3 objects alive");

    Ok(())
}

#[async_trait]
impl Outbound for Hysteria2Outbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_HYSTERIA2
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(&self, destination: SocketAddr) -> Result<ProxyConn, BoxError> {
        let conn = self.get_connection().await?;
        let (mut send, mut recv) = conn.open_bi().await.context("open hy2 stream")?;
        let target = format_address(destination);
        let req = protocol::encode_tcp_request(&target, 0);
        send.write_all(&req).await?;
        tracing::debug!("hysteria2: sent tcp request, waiting for response");
        let mut resp_buf = vec![0u8; 2048]; // 增加缓冲区大小
        let n = recv
            .read(&mut resp_buf)
            .await?
            .context("hy2 tcp response")?;
        tracing::debug!("hysteria2: received {} bytes response", n);

        // ✅ 添加：打印响应内容用于调试
        if n > 0 {
            tracing::error!("🔴 Hysteria2 response content ({} bytes):", n);
            tracing::error!("🔴 Hex (first 256 bytes): {:02x?}", &resp_buf[..n.min(256)]);
            tracing::error!("🔴 String: {}", String::from_utf8_lossy(&resp_buf[..n.min(512)]));
        }

        let mut cursor = &resp_buf[..n];
        let (ok, _) = protocol::decode_tcp_response(&mut cursor)?;
        if !ok {
            anyhow::bail!("hysteria2 tcp request rejected");
        }
        Ok(Box::new(SplitProxy::new(recv, send)))
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let conn = self.get_connection().await?;
        let session_id = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
        Ok(udp_client::hy2_udp_socket(conn, session_id))
    }
    async fn close(&self) -> Result<(), BoxError> {
        if let Some(conn) = self.connection.lock().await.take() {
            conn.close(0u32.into(), b"close");
        }
        Ok(())
    }
}

fn format_address(addr: SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v4) => format!("{}:{}", v4.ip(), v4.port()),
        SocketAddr::V6(v6) => format!("[{}]:{}", v6.ip(), v6.port()),
    }
}
