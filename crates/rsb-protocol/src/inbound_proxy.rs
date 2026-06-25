use crate::direct::parse_listen;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Dialer, Inbound, Metadata, Network, ProxyConn};
use rsb_dns::DnsRouter;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt, AsyncBufReadExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProxyMode {
    Mixed,
    Http,
    Socks,
}

pub struct MixedInbound {
    tag: String,
    kind: String,
    listen: SocketAddr,
    mode: ProxyMode,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl MixedInbound {
    pub fn new(
        tag: String,
        kind: String,
        raw: Value,
        dialer: Arc<Dialer>,
        dns: Arc<DnsRouter>,
    ) -> Result<Self> {
        let mode = match kind.as_str() {
            rsb_constant::TYPE_HTTP => ProxyMode::Http,
            rsb_constant::TYPE_SOCKS => ProxyMode::Socks,
            _ => ProxyMode::Mixed,
        };
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            kind,
            listen: parse_listen(&raw)?,
            mode,
            dialer,
            dns,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for MixedInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        &self.kind
    }
    async fn start(&self) -> Result<(), BoxError> {
        let listener = TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, kind = %self.kind, "inbound listening");
        let dialer = self.dialer.clone();
        let dns = self.dns.clone();
        let tag = self.tag.clone();
        let kind = self.kind.clone();
        let mode = self.mode;
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    accept = listener.accept() => {
                        let Ok((stream, peer)) = accept else { break };
                        let dialer = dialer.clone();
                        let dns = dns.clone();
                        let tag = tag.clone();
                        let kind = kind.clone();
                        tokio::spawn(async move {
                            let mut stream = stream;
                            if let Err(err) = handle_client(&mut stream, peer, &tag, &kind, mode, dialer, dns).await {
                                tracing::debug!(error = %err, "proxy client failed");
                            }
                        });
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

async fn handle_client(
    stream: &mut TcpStream,
    peer: SocketAddr,
    inbound_tag: &str,
    inbound_type: &str,
    mode: ProxyMode,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
) -> Result<()> {
    let mut peek = [0u8; 1];
    let n = stream.peek(&mut peek).await?;
    if n == 0 {
        return Ok(());
    }
    match mode {
        ProxyMode::Http => {
            handle_http_connect(stream, peer, inbound_tag, inbound_type, dialer, dns).await
        },
        ProxyMode::Socks => {
            handle_socks5(stream, peer, inbound_tag, inbound_type, dialer, dns).await
        },
        ProxyMode::Mixed => {
            if peek[0] == 0x05 {
                handle_socks5(stream, peer, inbound_tag, inbound_type, dialer, dns).await
            } else {
                handle_http_connect(stream, peer, inbound_tag, inbound_type, dialer, dns).await
            }
        },
    }
}

async fn handle_socks5(
    stream: &mut TcpStream,
    peer: SocketAddr,
    inbound_tag: &str,
    inbound_type: &str,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
) -> Result<()> {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).await?;
    if header[0] != 0x05 {
        anyhow::bail!("invalid socks version");
    }
    let mut methods = vec![0u8; header[1] as usize];
    stream.read_exact(&mut methods).await?;
    stream.write_all(&[0x05, 0x00]).await?;
    let mut req = [0u8; 4];
    stream.read_exact(&mut req).await?;
    let (dest, domain) = read_socks_addr(stream, req[3]).await?;
    stream
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    dial_and_relay(
        stream,
        peer,
        inbound_tag,
        inbound_type,
        dialer,
        dns,
        dest,
        domain,
    )
    .await
}

async fn read_socks_addr(stream: &mut TcpStream, atyp: u8) -> Result<(SocketAddr, Option<String>)> {
    match atyp {
        0x01 => {
            let mut buf = [0u8; 6];
            stream.read_exact(&mut buf).await?;
            let ip: [u8; 4] = buf[..4].try_into()?;
            let port = u16::from_be_bytes([buf[4], buf[5]]);
            Ok((SocketAddr::from((std::net::Ipv4Addr::from(ip), port)), None))
        },
        0x03 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await?;
            let mut buf = vec![0u8; len[0] as usize + 2];
            stream.read_exact(&mut buf).await?;
            let host = std::str::from_utf8(&buf[..len[0] as usize])?.to_string();
            let port = u16::from_be_bytes([buf[len[0] as usize], buf[len[0] as usize + 1]]);
            Ok((SocketAddr::from(([0, 0, 0, 0], port)), Some(host)))
        },
        0x04 => {
            let mut buf = [0u8; 18];
            stream.read_exact(&mut buf).await?;
            let ip = std::net::Ipv6Addr::from([
                buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7], buf[8], buf[9],
                buf[10], buf[11], buf[12], buf[13], buf[14], buf[15],
            ]);
            let port = u16::from_be_bytes([buf[16], buf[17]]);
            Ok((SocketAddr::from((ip, port)), None))
        },
        _ => anyhow::bail!("unsupported socks address type {atyp}"),
    }
}

async fn handle_http_connect(
    stream: &mut TcpStream,
    peer: SocketAddr,
    inbound_tag: &str,
    inbound_type: &str,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
) -> Result<()> {
    // ✅ 使用 BufReader 精确读取 HTTP 请求，完全模仿 sing-box
    let mut reader = BufReader::new(stream);

    // 读取请求行
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;

    let mut parts = request_line.trim().split_whitespace();
    let method = parts.next().context("no method")?;
    let target = parts.next().context("no target")?;

    tracing::info!("🔍 HTTP request: method={}, target={}", method, target);

    // 读取所有头部直到空行
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            break;
        }
    }

    tracing::info!("🔍 HTTP headers parsed, reader.buffer().len()={}", reader.buffer().len());

    // 支持 HTTP CONNECT 和普通 HTTP 方法
    if method == "CONNECT" {
        // CONNECT 方法：用于 HTTPS 隧道
        let (dest, domain) = parse_connect_target(target)?;

        tracing::info!("🔍 CONNECT target parsed: dest={:?}, domain={:?}", dest, domain);

        // 发送 200 响应
        let stream_ref = reader.get_mut();
        stream_ref
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;

        tracing::info!("✅ Sent 200 Connection Established");

        // ✅ 检查 BufReader 是否有缓冲数据（模仿 sing-box）
        let buffered = reader.buffer().len();
        tracing::info!("🔍 BufReader has {} bytes buffered", buffered);

        if buffered > 0 {
            // 有缓冲数据，需要先发送
            tracing::info!("🔍 Found {} bytes in buffer, will send before relay", buffered);
            let buffered_data = reader.buffer().to_vec();

            // 提取底层 stream
            let mut stream = reader.into_inner();

            // 连接远程并发送缓冲数据
            dial_and_relay_with_initial_data(
                &mut stream,
                buffered_data,
                peer,
                inbound_tag,
                inbound_type,
                dialer,
                dns,
                dest,
                domain,
            )
            .await
        } else {
            // 没有缓冲数据，直接转发
            tracing::info!("🔍 No buffered data, using direct relay");
            let mut stream = reader.into_inner();
            dial_and_relay(
                &mut stream,
                peer,
                inbound_tag,
                inbound_type,
                dialer,
                dns,
                dest,
                domain,
            )
            .await
        }
    } else if method == "GET"
        || method == "POST"
        || method == "HEAD"
        || method == "PUT"
        || method == "DELETE"
        || method == "OPTIONS"
        || method == "PATCH"
    {
        // 普通 HTTP 方法：GET, POST 等
        // 对于非 CONNECT 请求，回退到原始实现
        // TODO: 未来可以也用 BufReader 重构这部分
        tracing::warn!("⚠️ Non-CONNECT HTTP method {} not fully refactored yet, using fallback", method);

        // 获取底层 stream 并重新读取（简化处理）
        let mut stream = reader.into_inner();
        anyhow::bail!("Non-CONNECT HTTP methods not supported in BufReader mode yet: {}", method)
    } else {
        anyhow::bail!("unsupported HTTP method: {}", method)
    }
}

async fn dial_and_relay(
    client: &mut TcpStream,
    peer: SocketAddr,
    inbound_tag: &str,
    inbound_type: &str,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    dest: SocketAddr,
    mut domain: Option<String>,
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use std::time::Duration;

    let process = rsb_core::lookup_process_for_tcp_stream(client);
    let dest = resolve_destination(&dns, dest, domain.as_deref()).await?;

    tracing::info!("🔍 dial_and_relay: connecting to {:?}, domain: {:?}", dest, domain);

    let metadata = Metadata {
        network: Network::Tcp,
        source: Some(peer),
        destination: Some(dest),
        domain,
        protocol: Some("https".to_string()),
        process_name: process.name,
        process_path: process.path,
        inbound_tag: inbound_tag.to_string(),
        inbound_type: inbound_type.to_string(),
    };

    let mut remote = dialer.dial_tcp(&metadata, dest).await?;
    tracing::info!("✅ dial_tcp succeeded");

    // ✅ 添加：读取第一批响应数据并打印
    tracing::info!("🔍 Attempting to read first response from remote...");
    let mut first_chunk = vec![0u8; 2048];
    match tokio::time::timeout(Duration::from_secs(3), remote.as_mut().read(&mut first_chunk)).await {
        Ok(Ok(n)) => {
            if n > 0 {
                tracing::error!("🔴 Received {} bytes from remote BEFORE client sends data!", n);
                tracing::error!("🔴 Response (first 512 bytes hex): {:02x?}", &first_chunk[..n.min(512)]);
                tracing::error!("🔴 Response (as string): {}", String::from_utf8_lossy(&first_chunk[..n.min(512)]));

                // 将这些数据发送给客户端
                if let Err(e) = client.write_all(&first_chunk[..n]).await {
                    tracing::error!("❌ Failed to write response to client: {}", e);
                    return Err(e.into());
                }
                tracing::info!("✅ Sent {} bytes to client", n);
            } else {
                tracing::warn!("⚠️ Remote closed connection immediately (0 bytes)");
                return Ok(());
            }
        }
        Ok(Err(e)) => {
            tracing::error!("❌ Error reading from remote: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            tracing::info!("✅ No immediate response (timeout), starting normal relay");
        }
    }

    // 正常双向转发
    tracing::info!("🔍 Starting relay_proxy");
    let result = relay_proxy(client, remote).await;
    match &result {
        Ok(_) => tracing::info!("✅ relay_proxy completed successfully"),
        Err(e) => tracing::error!("❌ relay_proxy failed: {}", e),
    }
    result
}

/// 带初始数据的转发（用于 CONNECT 隧道中已读取的 TLS ClientHello）
async fn dial_and_relay_with_initial_data(
    client: &mut TcpStream,
    initial_data: Vec<u8>,
    peer: SocketAddr,
    inbound_tag: &str,
    inbound_type: &str,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    dest: SocketAddr,
    domain: Option<String>,
) -> Result<()> {
    use tokio::io::{AsyncWriteExt, AsyncReadExt};
    use std::time::Duration;

    tracing::info!("🔍 dial_and_relay_with_initial_data: initial_data.len() = {}", initial_data.len());
    tracing::info!("🔍 initial_data (first 64 bytes): {:02x?}", &initial_data[..initial_data.len().min(64)]);

    // 不嗅探，直接解析和连接
    let dest = resolve_destination(&dns, dest, domain.as_deref()).await?;
    tracing::info!("🔍 resolved destination: {:?}, domain: {:?}", dest, domain);

    let metadata = Metadata {
        network: Network::Tcp,
        source: Some(peer),
        destination: Some(dest),
        domain,
        protocol: Some("https".to_string()),
        process_name: None,
        process_path: None,
        inbound_tag: inbound_tag.to_string(),
        inbound_type: inbound_type.to_string(),
    };

    let mut remote = dialer.dial_tcp(&metadata, dest).await?;
    tracing::info!("🔍 connected to remote server successfully");

    // 先发送初始数据到远程服务器
    tracing::info!("🔍 sending {} bytes of initial data...", initial_data.len());
    remote.as_mut().write_all(&initial_data).await
        .map_err(|e| {
            tracing::error!("❌ failed to write initial data: {}", e);
            e
        })?;
    tracing::info!("✅ initial data sent successfully");

    // 读取远程服务器的第一批响应数据
    tracing::info!("🔍 waiting for first response from remote...");
    let mut first_chunk = vec![0u8; 1024];
    match tokio::time::timeout(Duration::from_secs(2), remote.as_mut().read(&mut first_chunk)).await {
        Ok(Ok(n)) => {
            if n > 0 {
                tracing::info!("🔍 received {} bytes from remote server", n);
                tracing::info!("🔍 response (first 256 bytes hex): {:02x?}", &first_chunk[..n.min(256)]);
                tracing::info!("🔍 response (as string): {}", String::from_utf8_lossy(&first_chunk[..n.min(256)]));

                // 将读取的数据写回客户端
                client.write_all(&first_chunk[..n]).await?;
                tracing::info!("✅ first chunk relayed to client");
            } else {
                tracing::warn!("⚠️ remote server closed connection immediately");
                return Ok(());
            }
        }
        Ok(Err(e)) => {
            tracing::error!("❌ error reading from remote: {}", e);
            return Err(e.into());
        }
        Err(_) => {
            tracing::warn!("⚠️ timeout waiting for remote response");
        }
    }

    // 然后正常双向转发
    tracing::info!("🔍 starting bidirectional relay");
    relay_proxy(client, remote).await
}

fn parse_connect_target(target: &str) -> Result<(SocketAddr, Option<String>)> {
    if let Ok(addr) = target.parse::<SocketAddr>() {
        return Ok((addr, None));
    }
    if let Some((host, port)) = target.rsplit_once(':') {
        let port: u16 = port.parse().context("invalid connect port")?;
        return Ok((
            SocketAddr::from(([0, 0, 0, 0], port)),
            Some(host.to_string()),
        ));
    }
    anyhow::bail!("invalid connect target: {target}")
}

pub async fn resolve_destination(
    dns: &DnsRouter,
    placeholder: SocketAddr,
    domain: Option<&str>,
) -> Result<SocketAddr> {
    let Some(host) = domain else {
        return Ok(placeholder);
    };
    let port = placeholder.port();
    let addrs = dns.lookup(host).await?;
    let ip = addrs
        .into_iter()
        .next()
        .context("dns lookup returned no addresses")?;
    Ok(SocketAddr::new(ip, port))
}

pub async fn relay_bidirectional(
    a: &mut TcpStream,
    mut b: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
) -> Result<()> {
    relay_streams(a, &mut b).await
}

pub async fn relay_streams(
    a: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
    b: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
) -> Result<()> {
    tokio::io::copy_bidirectional(a, b).await?;
    Ok(())
}

pub async fn relay_proxy(a: &mut TcpStream, mut b: ProxyConn) -> Result<()> {
    tokio::io::copy_bidirectional(a, b.as_mut()).await?;
    Ok(())
}

pub async fn handle_redirect_stream(
    mut stream: tokio::net::TcpStream,
    peer: SocketAddr,
    inbound_tag: &str,
    inbound_type: &str,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    dest: SocketAddr,
) -> Result<()> {
    dial_and_relay(
        &mut stream,
        peer,
        inbound_tag,
        inbound_type,
        dialer,
        dns,
        dest,
        None,
    )
    .await
}
// 在 inbound_proxy.rs 末尾添加这个新函数
async fn handle_http_proxy(
    client: &mut TcpStream,
    peer: SocketAddr,
    inbound_tag: &str,
    inbound_type: &str,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    method: &str,
    target: &str,
    full_request: &str,
    _request_bytes: &[u8],
) -> Result<()> {
    // 解析目标 URL
    let (host, port, path) = parse_http_url(target)?;

    // 解析目标地址
    let dest = SocketAddr::from(([0, 0, 0, 0], port));
    let _domain = Some(host.clone());

    // 解析 DNS
    let dest = resolve_destination(&dns, dest, Some(&host)).await?;

    // 创建元数据
    let metadata = Metadata {
        network: Network::Tcp,
        source: Some(peer),
        destination: Some(dest),
        domain: Some(host.clone()),
        protocol: Some("http".to_string()),
        process_name: None,
        process_path: None,
        inbound_tag: inbound_tag.to_string(),
        inbound_type: inbound_type.to_string(),
    };

    // 连接到目标服务器
    let mut remote = dialer.dial_tcp(&metadata, dest).await?;

    // 重写请求（去掉代理格式，改为标准 HTTP 请求）
    let rewritten_request = rewrite_http_request(method, &host, port, &path, full_request)?;

    // 发送请求到目标服务器
    remote.write_all(rewritten_request.as_bytes()).await?;

    // 双向转发数据
    relay_bidirectional(client, remote).await
}

fn parse_http_url(url: &str) -> Result<(String, u16, String)> {
    // 处理完整 URL: http://example.com/path 或 http://example.com:8080/path
    if let Some(without_scheme) = url.strip_prefix("http://") {
        if let Some(slash_pos) = without_scheme.find('/') {
            let host_port = &without_scheme[..slash_pos];
            let path = &without_scheme[slash_pos..];
            if let Some(colon_pos) = host_port.find(':') {
                let host = host_port[..colon_pos].to_string();
                let port: u16 = host_port[colon_pos + 1..].parse()?;
                return Ok((host, port, path.to_string()));
            } else {
                return Ok((host_port.to_string(), 80, path.to_string()));
            }
        } else {
            // 没有路径
            if let Some(colon_pos) = without_scheme.find(':') {
                let host = without_scheme[..colon_pos].to_string();
                let port: u16 = without_scheme[colon_pos + 1..].parse()?;
                return Ok((host, port, "/".to_string()));
            } else {
                return Ok((without_scheme.to_string(), 80, "/".to_string()));
            }
        }
    }

    // 处理不带 scheme 的 URL: example.com/path
    if let Some(slash_pos) = url.find('/') {
        let host_port = &url[..slash_pos];
        let path = &url[slash_pos..];
        if let Some(colon_pos) = host_port.find(':') {
            let host = host_port[..colon_pos].to_string();
            let port: u16 = host_port[colon_pos + 1..].parse()?;
            return Ok((host, port, path.to_string()));
        } else {
            return Ok((host_port.to_string(), 80, path.to_string()));
        }
    }

    anyhow::bail!("invalid HTTP URL: {}", url)
}

fn rewrite_http_request(
    method: &str,
    host: &str,
    port: u16,
    path: &str,
    original_request: &str,
) -> Result<String> {
    let mut lines: Vec<&str> = original_request.lines().collect();

    if lines.is_empty() {
        anyhow::bail!("empty HTTP request");
    }

    // 重写请求行：GET http://example.com/path HTTP/1.1 -> GET /path HTTP/1.1
    let request_line_parts: Vec<&str> = lines[0].split_whitespace().collect();
    if request_line_parts.len() < 3 {
        anyhow::bail!("invalid HTTP request line");
    }

    let http_version = request_line_parts[2];
    let new_request_line = format!("{} {} {}", method, path, http_version);
    lines[0] = &new_request_line;

    // 构建新请求
    let mut new_request = String::new();
    new_request.push_str(&new_request_line);
    new_request.push_str("\r\n");

    // 检查是否已有 Host header
    let mut has_host = false;
    for (i, line) in lines.iter().enumerate().skip(1) {
        if line.is_empty() {
            break;
        }
        if line.to_lowercase().starts_with("host:") {
            has_host = true;
        }
        if i > 0 {
            new_request.push_str(line);
            new_request.push_str("\r\n");
        }
    }

    // 如果没有 Host header，添加一个
    if !has_host {
        if port == 80 {
            new_request.push_str(&format!("Host: {}\r\n", host));
        } else {
            new_request.push_str(&format!("Host: {}:{}\r\n", host, port));
        }
    }

    new_request.push_str("\r\n");

    Ok(new_request)
}
