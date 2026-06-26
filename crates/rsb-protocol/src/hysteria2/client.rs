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
use std::sync::atomic::{AtomicU16, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

static SESSION_COUNTER: AtomicU32 = AtomicU32::new(1);

pub struct Hysteria2Outbound {
    tag: String,
    server: String,
    port: u16,                      // 保留用于单端口模式
    port_start: Option<u16>,        // 🔧 端口跳跃：范围起始
    port_end: Option<u16>,          // 🔧 端口跳跃：范围结束
    hop_interval: Option<Duration>, // 🔧 端口跳跃：跳跃间隔
    current_port: Arc<AtomicU16>,   // 🔧 端口跳跃：当前使用的端口
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

        // 🔧 端口跳跃：解析 server_ports
        let (port, port_start, port_end) = if let Some(server_ports) = raw.get("server_ports") {
            if let Some(ports_str) = server_ports.as_str() {
                // 解析端口范围：666-766
                if let Some((start, end)) = ports_str.split_once('-') {
                    let start_port = start
                        .trim()
                        .parse::<u16>()
                        .context("invalid port range start")?;
                    let end_port = end
                        .trim()
                        .parse::<u16>()
                        .context("invalid port range end")?;
                    (start_port, Some(start_port), Some(end_port))
                } else {
                    // 单端口：666
                    let single_port = ports_str.parse::<u16>().context("invalid port")?;
                    (single_port, None, None)
                }
            } else {
                return Err(anyhow::anyhow!("server_ports must be string"));
            }
        } else {
            // 兼容旧配置：server_port
            let port = raw
                .get("server_port")
                .and_then(|v| v.as_u64())
                .context("hysteria2: server_port required")? as u16;
            (port, None, None)
        };

        // 🔧 端口跳跃：解析 hop_interval
        let hop_interval = raw
            .get("hop_interval")
            .and_then(|v| v.as_str())
            .and_then(|s| {
                // 解析 "30s" 格式
                if let Some(secs) = s.strip_suffix('s') {
                    secs.parse::<u64>().ok().map(Duration::from_secs)
                } else {
                    None
                }
            });

        Ok(Self {
            tag,
            server: raw
                .get("server")
                .and_then(|v| v.as_str())
                .context("hysteria2: server required")?
                .to_string(),
            port,
            port_start,
            port_end,
            hop_interval,
            current_port: Arc::new(AtomicU16::new(port)), // 🔧 初始化当前端口
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

    // 🔧 端口跳跃：选择下一个端口
    fn next_port(&self) -> u16 {
        if let (Some(start), Some(end)) = (self.port_start, self.port_end) {
            if start == end {
                return start;
            }
            // 随机选择端口范围内的端口
            use rand::Rng;
            let mut rng = rand::thread_rng();
            rng.gen_range(start..=end)
        } else {
            self.port
        }
    }

    // 🔧 端口跳跃：启动定时切换任务
    fn start_port_hopping(self: Arc<Self>) {
        if let Some(interval) = self.hop_interval {
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(interval).await;

                    let new_port = self.next_port();
                    self.current_port.store(new_port, Ordering::Relaxed);

                    tracing::debug!("hysteria2: hopping to port {}", new_port);

                    // 重建连接
                    let mut guard = self.connection.lock().await;
                    if let Some(conn) = guard.take() {
                        conn.close(0u32.into(), b"port hopping");
                    }
                    // 下次 get_connection 会自动建立新连接
                }
            });
        }
    }

    async fn get_connection(&self) -> Result<Arc<quinn::Connection>> {
        // 🔧 端口跳跃：首次连接时启动任务
        use std::sync::atomic::AtomicBool;
        static HOPPING_STARTED: AtomicBool = AtomicBool::new(false);

        if self.hop_interval.is_some() && !HOPPING_STARTED.swap(true, Ordering::Relaxed) {
            if let (Some(start), Some(end)) = (self.port_start, self.port_end) {
                let interval = self.hop_interval.unwrap();
                let current_port = self.current_port.clone();
                let tag = self.tag.clone();

                tokio::spawn(async move {
                    tracing::info!("hysteria2: port hopping started for {}", tag);
                    loop {
                        tokio::time::sleep(interval).await;

                        // 随机选择新端口
                        use rand::Rng;
                        let mut rng = rand::thread_rng();
                        let new_port = if start == end {
                            start
                        } else {
                            rng.gen_range(start..=end)
                        };

                        current_port.store(new_port, Ordering::Relaxed);
                        tracing::info!("hysteria2: hopped to port {}", new_port);
                    }
                });
            }
        }

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

    // 🔧 端口跳跃：使用 current_port
    let current_port = ob.current_port.load(Ordering::Relaxed);

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
    // 🔧 优化：增加 max_idle_timeout 防止长时间连接断开
    transport_cfg.max_idle_timeout(Some(Duration::from_secs(300).try_into().unwrap()));
    client_cfg.transport_config(Arc::new(transport_cfg));

    let addr = tokio::net::lookup_host(format!("{}:{}", ob.server, current_port))
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

    // Keep H3 driver alive - store handle to prevent premature drop
    let driver_handle = tokio::spawn(async move {
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
    let padding = auth::random_padding(64, 512);
    req = req.header("hysteria-padding", padding.as_str());
    let mut stream = send_request.send_request(req.body(())?).await?;
    stream.finish().await?;
    let resp = stream.recv_response().await?;

    tracing::debug!("hysteria2: auth response status = {}", resp.status());

    if resp.status() != StatusCode::from_u16(233).unwrap() {
        anyhow::bail!("hysteria2 auth failed: {}", resp.status());
    }

    tracing::debug!("hysteria2: authentication completed");

    // Keep driver and request alive
    std::mem::forget(driver_handle);
    std::mem::forget(send_request);

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
    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        let conn = self.get_connection().await?;
        let (mut send, mut recv) = conn.open_bi().await.context("open hy2 stream")?;

        // ✅ 修复：优先使用域名，如果没有域名才使用 IP
        let target = if let Some(domain) = domain {
            format!("{}:{}", domain, destination.port())
        } else {
            format_address(destination)
        };

        let padding_len = auth::random_padding_len(64, 512);
        tracing::debug!(
            "hysteria2: target = {}, padding_len = {}, using_domain = {}",
            target,
            padding_len,
            domain.is_some()
        );

        let req = protocol::encode_tcp_request(&target, padding_len);

        send.write_all(&req).await?;
        tracing::debug!("hysteria2: sent tcp request, waiting for response");
        let mut resp_buf = vec![0u8; 2048];

        // ✅ 添加超时和详细错误日志
        tracing::debug!("hysteria2: attempting to read response...");
        let read_result =
            tokio::time::timeout(std::time::Duration::from_secs(10), recv.read(&mut resp_buf))
                .await;

        tracing::debug!("hysteria2: read_result = {:?}", read_result.is_ok());

        let n = match read_result {
            Ok(Ok(Some(n))) => n,
            Ok(Ok(None)) => anyhow::bail!("hysteria2: stream closed by server"),
            Ok(Err(e)) => return Err(e.into()),
            Err(_) => anyhow::bail!("hysteria2: read timeout"),
        };

        tracing::debug!("hysteria2: received {} bytes response", n);

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
