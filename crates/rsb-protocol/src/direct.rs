use anyhow::Result;
use async_trait::async_trait;
use rsb_core::{tcp_stream, BoxError, Inbound, Network, Outbound, ProxyConn, ProxyUdpSocket};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub struct DirectOutbound {
    tag: String,
    bind_interface: Option<String>,
}

impl DirectOutbound {
    pub fn new(tag: String, bind_interface: Option<String>) -> Self {
        Self {
            tag,
            bind_interface,
        }
    }
}

#[async_trait]
impl Outbound for DirectOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_DIRECT
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        let candidates: Vec<SocketAddr> = if destination.ip().is_unspecified() {
            let host = domain.ok_or_else(|| {
                anyhow::anyhow!("direct outbound needs a domain when destination IP is unspecified")
            })?;
            // 必须限时：Windows 上 lookup_host 在高压/异常 DNS 下可能长时间不返回，
            // 会拖死 inbound 任务 → 端口仍 LISTEN 但经代理全站超时（假连接）。
            const DNS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
            let addrs: Vec<SocketAddr> = match tokio::time::timeout(
                DNS_TIMEOUT,
                tokio::net::lookup_host((host, destination.port())),
            )
            .await
            {
                Ok(Ok(iter)) => iter.collect(),
                Ok(Err(e)) => {
                    return Err(anyhow::anyhow!("direct dns lookup for `{host}` failed: {e}").into());
                }
                Err(_) => {
                    return Err(anyhow::anyhow!(
                        "direct dns lookup for `{host}` timed out after {DNS_TIMEOUT:?}"
                    )
                    .into());
                }
            };
            let mut v4: Vec<_> = addrs.iter().copied().filter(|a| a.is_ipv4()).collect();
            if v4.is_empty() {
                v4 = addrs;
            }
            if v4.is_empty() {
                return Err(anyhow::anyhow!(
                    "direct dns lookup for `{host}` returned no addresses"
                )
                .into());
            }
            v4
        } else {
            vec![destination]
        };

        let mut last_err: Option<anyhow::Error> = None;
        // 每个候选 IP 限时拨号。不宜 ≥ 客户端假死探针超时（约 2.5~4s），
        // 否则百度等 CDN 首个黑洞 IP 会把整次请求拖死，被误判成「代理假死」。
        const PER_CANDIDATE: std::time::Duration = std::time::Duration::from_millis(800);
        for addr in candidates {
            match tokio::time::timeout(
                PER_CANDIDATE,
                rsb_core::tcp_connect_via(addr, self.bind_interface.as_deref()),
            )
            .await
            {
                Ok(Ok(stream)) => return Ok(tcp_stream(stream)),
                Ok(Err(err)) => {
                    tracing::debug!(%addr, error = %err, "direct dial candidate failed");
                    last_err = Some(err);
                }
                Err(_) => {
                    tracing::debug!(%addr, "direct dial candidate timed out");
                    last_err = Some(anyhow::anyhow!("direct dial {addr} timed out"));
                }
            }
        }
        Err(last_err
            .unwrap_or_else(|| anyhow::anyhow!("direct dial failed"))
            .into())
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        let socket = rsb_core::udp_bind_via(self.bind_interface.as_deref()).await?;
        Ok(ProxyUdpSocket::from_tokio(socket))
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

pub struct BlockOutbound {
    tag: String,
}

impl BlockOutbound {
    pub fn new(tag: String) -> Self {
        Self { tag }
    }
}

#[async_trait]
impl Outbound for BlockOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_BLOCK
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(
        &self,
        _destination: SocketAddr,
        _domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        anyhow::bail!("connection blocked by outbound `{}`", self.tag)
    }
    async fn dial_udp(&self, _destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        anyhow::bail!("connection blocked by outbound `{}`", self.tag)
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

pub struct DirectInbound {
    tag: String,
    listen: SocketAddr,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl DirectInbound {
    pub fn new(tag: String, raw: Value) -> Result<Self> {
        let listen = parse_listen(&raw)?;
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for DirectInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_DIRECT
    }
    async fn start(&self) -> Result<(), BoxError> {
        let listener = TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, "direct inbound listening");
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    accept = listener.accept() => {
                        if accept.is_err() { break; }
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

pub fn parse_listen(raw: &Value) -> Result<SocketAddr> {
    let listen = raw
        .get("listen")
        .and_then(|v| v.as_str())
        .unwrap_or("127.0.0.1");
    let port = raw
        .get("listen_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(1080) as u16;
    let host = if listen.contains(':') && !listen.starts_with('[') {
        format!("[{listen}]")
    } else {
        listen.to_string()
    };
    Ok(format!("{host}:{port}").parse()?)
}
