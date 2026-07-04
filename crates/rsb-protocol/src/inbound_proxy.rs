use crate::direct::parse_listen;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Dialer, Inbound, Metadata, Network, ProxyConn};
use rsb_dns::DnsRouter;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

const MAX_CONCURRENT_INBOUND: usize = 256;
const INBOUND_HANDLER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);
const INBOUND_ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const INBOUND_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

// ✅ 异步清理模块（内联）
mod async_cleanup {
    use std::sync::Arc;
    use tokio::sync::mpsc;
    use tokio::net::TcpStream;
    use tokio::io::AsyncWriteExt;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    enum CleanupRequest {
        TcpStream(TcpStream),
    }

    pub struct AsyncCleanup {
        sender: mpsc::UnboundedSender<CleanupRequest>,
    }

    impl AsyncCleanup {
        pub fn new() -> Arc<Self> {
            let (tx, mut rx) = mpsc::unbounded_channel();
            tokio::spawn(async move {
                tracing::info!("✅ AsyncCleanup 后台任务已启动");
                let mut count = 0u64;
                while let Some(request) = rx.recv().await {
                    match request {
                        CleanupRequest::TcpStream(mut stream) => {
                            match stream.shutdown().await {
                                Ok(_) => {
                                    count += 1;
                                    tracing::trace!("✅ TcpStream #{} 已清理", count);
                                }
                                Err(e) => {
                                    tracing::debug!("⚠️ shutdown 失败: {}", e);
                                }
                            }
                        }
                    }
                }
                tracing::info!("🛑 AsyncCleanup 已退出，共清理 {} 个连接", count);
            });
            Arc::new(Self { sender: tx })
        }

        pub fn cleanup_stream(&self, stream: TcpStream) {
            let _ = self.sender.send(CleanupRequest::TcpStream(stream));
        }
    }

    pub struct AutoCleanStream {
        stream: Option<TcpStream>,
        cleanup: Arc<AsyncCleanup>,
    }

    impl AutoCleanStream {
        pub fn new(stream: TcpStream, cleanup: Arc<AsyncCleanup>) -> Self {
            Self {
                stream: Some(stream),
                cleanup,
            }
        }

        pub fn get_mut(&mut self) -> &mut TcpStream {
            self.stream.as_mut().expect("stream is None")
        }
    }

    impl Drop for AutoCleanStream {
        fn drop(&mut self) {
            if let Some(stream) = self.stream.take() {
                self.cleanup.cleanup_stream(stream);
                tracing::trace!("📤 AutoCleanStream drop - 已发送清理请求");
            }
        }
    }

    impl tokio::io::AsyncRead for AutoCleanStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(self.stream.as_mut().expect("stream is None")).poll_read(cx, buf)
        }
    }

    impl tokio::io::AsyncWrite for AutoCleanStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Pin::new(self.stream.as_mut().expect("stream is None")).poll_write(cx, buf)
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(self.stream.as_mut().expect("stream is None")).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(self.stream.as_mut().expect("stream is None")).poll_shutdown(cx)
        }
    }

    impl Unpin for AutoCleanStream {}
}

use async_cleanup::{AsyncCleanup, AutoCleanStream};

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
    // ✅ 新增配置支持
    tcp_fast_open: bool,
    tcp_multi_path: bool,
    sniff: bool,
    sniff_override_destination: bool,
    // ✅ 异步清理器
    cleanup: Arc<AsyncCleanup>,
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

        // ✅ 解析新增配置
        let tcp_fast_open = raw.get("tcp_fast_open").and_then(|v| v.as_bool()).unwrap_or(false);
        let tcp_multi_path = raw.get("tcp_multi_path").and_then(|v| v.as_bool()).unwrap_or(false);
        let sniff = raw.get("sniff").and_then(|v| v.as_bool()).unwrap_or(false);
        let sniff_override_destination = raw.get("sniff_override_destination").and_then(|v| v.as_bool()).unwrap_or(false);

        Ok(Self {
            tag,
            kind,
            listen: parse_listen(&raw)?,
            mode,
            dialer,
            dns,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
            tcp_fast_open,
            tcp_multi_path,
            sniff,
            sniff_override_destination,
            cleanup: AsyncCleanup::new(),  // ✅ 初始化异步清理器
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
        // ✅ 使用 socket2 创建 socket 并应用配置
        use socket2::{Socket, Domain, Type, Protocol};

        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;

        // TCP Fast Open: enabled on Linux/macOS only (skip Windows — bad with system proxy).
        if self.tcp_fast_open {
            #[cfg(target_os = "linux")]
            {
                use std::os::unix::io::AsRawFd;
                // Linux: TCP_FASTOPEN = 23
                const TCP_FASTOPEN: i32 = 23;
                let value: i32 = 256;
                unsafe {
                    let ret = libc::setsockopt(
                        socket.as_raw_fd(),
                        libc::IPPROTO_TCP,
                        TCP_FASTOPEN,
                        &value as *const _ as *const libc::c_void,
                        std::mem::size_of::<i32>() as libc::socklen_t,
                    );
                    if ret != 0 {
                        tracing::warn!("Failed to set TCP_FASTOPEN on Linux");
                    }
                }
            }
            #[cfg(target_os = "macos")]
            {
                use std::os::unix::io::AsRawFd;
                // macOS: TCP_FASTOPEN = 0x105
                const TCP_FASTOPEN: i32 = 0x105;
                let value: i32 = 1;
                unsafe {
                    let ret = libc::setsockopt(
                        socket.as_raw_fd(),
                        libc::IPPROTO_TCP,
                        TCP_FASTOPEN,
                        &value as *const _ as *const libc::c_void,
                        std::mem::size_of::<i32>() as libc::socklen_t,
                    );
                    if ret != 0 {
                        tracing::warn!("Failed to set TCP_FASTOPEN on macOS");
                    }
                }
            }
        }

        // ✅ 应用 TCP Multi-Path (仅 Linux)
        #[cfg(target_os = "linux")]
        if !self.tcp_multi_path {
            use std::os::unix::io::AsRawFd;
            // MPTCP_ENABLED = 42
            const MPTCP_ENABLED: i32 = 42;
            let value: i32 = 0;
            unsafe {
                let _ = libc::setsockopt(
                    socket.as_raw_fd(),
                    libc::IPPROTO_TCP,
                    MPTCP_ENABLED,
                    &value as *const _ as *const libc::c_void,
                    std::mem::size_of::<i32>() as libc::socklen_t,
                );
            }
        }

        socket.set_reuse_address(true)?;

        // ✅ 启用 TCP Keep-Alive（保持长连接活跃，支持 WebSocket）
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let fd = socket.as_raw_fd();

            // SO_KEEPALIVE = 1
            let keepalive: libc::c_int = 1;
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_SOCKET,
                    libc::SO_KEEPALIVE,
                    &keepalive as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );

                // TCP_KEEPIDLE = 60 秒（开始发送 Keep-Alive 探测的空闲时间）
                #[cfg(target_os = "linux")]
                {
                    let idle: libc::c_int = 60;
                    libc::setsockopt(
                        fd,
                        libc::IPPROTO_TCP,
                        libc::TCP_KEEPIDLE,
                        &idle as *const _ as *const libc::c_void,
                        std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                    );

                    // TCP_KEEPINTVL = 10 秒（探测间隔）
                    let interval: libc::c_int = 10;
                    libc::setsockopt(
                        fd,
                        libc::IPPROTO_TCP,
                        libc::TCP_KEEPINTVL,
                        &interval as *const _ as *const libc::c_void,
                        std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                    );
                }
            }
        }

        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawSocket;
            let sock = socket.as_raw_socket();

            // SO_KEEPALIVE = 1
            let keepalive: u32 = 1;
            unsafe {
                windows_sys::Win32::Networking::WinSock::setsockopt(
                    sock as usize,
                    windows_sys::Win32::Networking::WinSock::SOL_SOCKET,
                    windows_sys::Win32::Networking::WinSock::SO_KEEPALIVE,
                    &keepalive as *const _ as *const u8,
                    std::mem::size_of::<u32>() as i32,
                );
            }
        }

        socket.bind(&self.listen.into())?;
        socket.listen(1024)?;
        socket.set_nonblocking(true)?;

        let listener: std::net::TcpListener = socket.into();
        let listener = TcpListener::from_std(listener)?;

        tracing::info!(
            tag = %self.tag,
            %self.listen,
            kind = %self.kind,
            tcp_fast_open = %self.tcp_fast_open,
            tcp_multi_path = %self.tcp_multi_path,
            sniff = %self.sniff,
            "inbound listening"
        );

        let dialer = self.dialer.clone();
        let dns = self.dns.clone();
        let tag = self.tag.clone();
        let kind = self.kind.clone();
        let mode = self.mode;
        let sniff = self.sniff;
        let sniff_override = self.sniff_override_destination;
        let concurrency = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_INBOUND));
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    accept = listener.accept() => {
                        let Ok((mut stream, peer)) = accept else { break };
                        let dialer = dialer.clone();
                        let dns = dns.clone();
                        let tag = tag.clone();
                        let kind = kind.clone();
                        let concurrency = concurrency.clone();

                        tokio::spawn(async move {
                            let Ok(permit) = tokio::time::timeout(
                                INBOUND_ACQUIRE_TIMEOUT,
                                concurrency.acquire_owned(),
                            )
                            .await
                            else {
                                let mut stream = stream;
                                let _ = send_http_error(
                                    &mut stream,
                                    503,
                                    "Service Unavailable",
                                    "proxy concurrency saturated",
                                )
                                .await;
                                close_inbound_stream(&mut stream).await;
                                return;
                            };
                            let Ok(permit) = permit else {
                                close_inbound_stream(&mut stream).await;
                                return;
                            };
                            let mut handshake_permit = Some(permit);

                            let result = tokio::time::timeout(
                                INBOUND_HANDLER_TIMEOUT,
                                handle_client(
                                    &mut stream,
                                    peer,
                                    &tag,
                                    &kind,
                                    mode,
                                    dialer,
                                    dns,
                                    &mut handshake_permit,
                                ),
                            )
                            .await;

                            match result {
                                Ok(Ok(())) => {
                                    tracing::trace!("Connection completed successfully");
                                }
                                Ok(Err(err)) => {
                                    tracing::debug!(error = ?err, "proxy client failed");
                                }
                                Err(_) => {
                                    tracing::debug!("Connection timeout after handler limit");
                                }
                            }

                            close_inbound_stream(&mut stream).await;
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
    handshake_permit: &mut Option<tokio::sync::OwnedSemaphorePermit>,
) -> Result<()> {
    let mut peek = [0u8; 1];
    let n = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.peek(&mut peek),
    )
    .await
    {
        Ok(Ok(n)) => n,
        Ok(Err(e)) => return Err(e.into()),
        Err(_) => {
            tracing::debug!("inbound peek timeout, closing idle client");
            return Ok(());
        }
    };
    if n == 0 {
        return Ok(());
    }
    match mode {
        ProxyMode::Http => {
            handle_http_connect(
                stream,
                peer,
                inbound_tag,
                inbound_type,
                dialer,
                dns,
                handshake_permit,
            )
            .await
        },
        ProxyMode::Socks => {
            handle_socks5(
                stream,
                peer,
                inbound_tag,
                inbound_type,
                dialer,
                dns,
                handshake_permit,
            )
            .await
        },
        ProxyMode::Mixed => {
            if peek[0] == 0x05 {
                handle_socks5(
                    stream,
                    peer,
                    inbound_tag,
                    inbound_type,
                    dialer,
                    dns,
                    handshake_permit,
                )
                .await
            } else {
                handle_http_connect(
                    stream,
                    peer,
                    inbound_tag,
                    inbound_type,
                    dialer,
                    dns,
                    handshake_permit,
                )
                .await
            }
        },
    }
}

fn release_handshake_permit(permit: &mut Option<tokio::sync::OwnedSemaphorePermit>) {
    permit.take();
}

async fn handle_socks5(
    stream: &mut TcpStream,
    peer: SocketAddr,
    inbound_tag: &str,
    inbound_type: &str,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    handshake_permit: &mut Option<tokio::sync::OwnedSemaphorePermit>,
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
    release_handshake_permit(handshake_permit);
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
    handshake_permit: &mut Option<tokio::sync::OwnedSemaphorePermit>,
) -> Result<()> {
    // ✅ 使用 BufReader 精确读取 HTTP 请求，完全模仿 sing-box
    let mut reader = BufReader::new(stream);

    // 读取请求行
    let mut request_line = String::new();
    reader.read_line(&mut request_line).await?;
    let mut full_request = request_line.clone();

    let mut parts = request_line.trim().split_whitespace();
    let method = parts.next().context("no method")?;
    let target = parts.next().context("no target")?;

    tracing::info!("🔍 HTTP request: method={}, target={}", method, target);

    // 读取所有头部直到空行
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).await?;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            full_request.push_str("\r\n");
            break;
        }
        full_request.push_str(&line);
    }

    tracing::info!(
        "🔍 HTTP headers parsed, reader.buffer().len()={}",
        reader.buffer().len()
    );

    // 支持 HTTP CONNECT 和普通 HTTP 方法
    if method == "CONNECT" {
        // CONNECT 方法：用于 HTTPS 隧道
        let (dest, domain) = parse_connect_target(target)?;

        tracing::info!(
            "🔍 CONNECT target parsed: dest={:?}, domain={:?}",
            dest,
            domain
        );

        // 发送 200 响应
        let stream_ref = reader.get_mut();
        stream_ref
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;

        tracing::info!("✅ Sent 200 Connection Established");
        release_handshake_permit(handshake_permit);

        // ✅ 检查 BufReader 是否有缓冲数据（模仿 sing-box）
        let buffered = reader.buffer().len();
        tracing::info!("🔍 BufReader has {} bytes buffered", buffered);

        if buffered > 0 {
            // 有缓冲数据，需要先发送
            tracing::info!(
                "🔍 Found {} bytes in buffer, will send before relay",
                buffered
            );
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
        let mut stream = reader.into_inner();
        release_handshake_permit(handshake_permit);
        if let Err(err) = handle_http_proxy(
            &mut stream,
            peer,
            inbound_tag,
            inbound_type,
            dialer,
            dns,
            method,
            target,
            &full_request,
            &[],
        )
        .await
        {
            tracing::debug!(error = ?err, "http proxy request failed");
            let _ = send_http_error(
                &mut stream,
                502,
                "Bad Gateway",
                "outbound dial failed",
            )
            .await;
            return Err(err);
        }
        Ok(())
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
    let process = rsb_core::lookup_process_for_tcp_stream(client);
    let dest = resolve_destination(&dns, dest, domain.as_deref()).await?;

    tracing::debug!(
        "dial_and_relay: connecting to {:?}, domain: {:?}",
        dest,
        domain
    );

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
        user: None,
    };

    let Some(mut remote) =
        dial_tcp_with_client_watch(client, dialer, metadata, dest).await?
    else {
        return Ok(());
    };
    relay_proxy(client, remote).await
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
    use std::time::Duration;

    tracing::debug!(
        "dial_and_relay_with_initial_data: initial_data.len() = {}",
        initial_data.len()
    );

    let dest = resolve_destination(&dns, dest, domain.as_deref()).await?;

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
        user: None,
    };

    let Some(mut remote) =
        dial_tcp_with_client_watch(client, dialer, metadata, dest).await?
    else {
        return Ok(());
    };

    remote.as_mut().write_all(&initial_data).await?;

    let mut first_chunk = vec![0u8; 1024];
    match tokio::time::timeout(
        Duration::from_secs(2),
        remote.as_mut().read(&mut first_chunk),
    )
    .await
    {
        Ok(Ok(n)) => {
            if n > 0 {
                client.write_all(&first_chunk[..n]).await?;
            } else {
                tracing::debug!("remote closed before relay");
                let _ = remote.as_mut().shutdown().await;
                return Ok(());
            }
        }
        Ok(Err(e)) => return Err(e.into()),
        Err(_) => {
            tracing::debug!("timeout waiting for first remote response");
            let _ = remote.as_mut().shutdown().await;
            anyhow::bail!("timeout waiting for first remote response");
        }
    }

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
    // HTTP CONNECT uses 0.0.0.0:port; QUIC outbounds (RSQ/Hy2) carry the hostname.
    // Skip local DNS here to avoid pollution before the remote side resolves.
    if placeholder.ip().is_unspecified() {
        return Ok(SocketAddr::new(placeholder.ip(), port));
    }
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
    let copy = tokio::io::copy_bidirectional(a, &mut b).await;
    close_inbound_stream(a).await;
    let _ = tokio::io::AsyncWriteExt::shutdown(&mut b).await;
    copy?;
    Ok(())
}

pub async fn relay_streams(
    a: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
    b: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
) -> Result<()> {
    let copy = tokio::io::copy_bidirectional(a, b).await;
    shutdown_io(a, b).await;
    copy?;
    Ok(())
}

/// Panel-aware relay: per-user traffic stats, quota, connection limits, and bandwidth cap.
pub struct UserRelaySession {
    pub(crate) inner: std::sync::Arc<UserRelayInner>,
    conn_id: u64,
    _user_guard: rsb_core::UserSessionGuard,
}

struct UserRelayInner {
    connections: rsb_core::SharedConnectionManager,
    inbound_tag: String,
    outbound_tag: String,
    user_name: String,
    limits: rsb_core::UserLimits,
    limiter: Option<std::sync::Arc<rsb_core::rate_limit::RateLimiter>>,
}

impl UserRelaySession {
    pub fn begin(
        connections: rsb_core::SharedConnectionManager,
        inbound_tag: &str,
        user_name: &str,
        limits: rsb_core::UserLimits,
        destination: Option<std::net::SocketAddr>,
        domain: Option<String>,
    ) -> Result<Self> {
        let guard = connections.acquire_user(user_name, &limits)?;
        Self::begin_tracked(
            connections,
            inbound_tag,
            user_name,
            limits,
            destination,
            domain,
            Some(guard),
        )
    }

    /// Track per-stream relay stats without consuming a panel connection slot.
    /// Used by QUIC-multiplexed inbounds (RSQ) where one session carries many TCP streams.
    pub fn begin_muxed(
        connections: rsb_core::SharedConnectionManager,
        inbound_tag: &str,
        user_name: &str,
        limits: rsb_core::UserLimits,
        destination: Option<std::net::SocketAddr>,
        domain: Option<String>,
    ) -> Self {
        Self::begin_tracked(
            connections,
            inbound_tag,
            user_name,
            limits,
            destination,
            domain,
            None,
        )
        .expect("muxed relay tracking must not fail")
    }

    fn begin_tracked(
        connections: rsb_core::SharedConnectionManager,
        inbound_tag: &str,
        user_name: &str,
        limits: rsb_core::UserLimits,
        destination: Option<std::net::SocketAddr>,
        domain: Option<String>,
        guard: Option<rsb_core::UserSessionGuard>,
    ) -> Result<Self> {
        let limiter = connections.user_limiter(user_name, limits.speed_bps);
        let conn_id = connections.track(
            inbound_tag,
            "direct",
            "tcp",
            None,
            destination,
            domain,
            Some(user_name.to_string()),
        );
        Ok(Self {
            inner: std::sync::Arc::new(UserRelayInner {
                connections,
                inbound_tag: inbound_tag.to_string(),
                outbound_tag: "direct".into(),
                user_name: user_name.to_string(),
                limits,
                limiter,
            }),
            conn_id,
            _user_guard: guard.unwrap_or_else(rsb_core::UserSessionGuard::detached),
        })
    }
}

impl Drop for UserRelaySession {
    fn drop(&mut self) {
        self.inner.connections.untrack(self.conn_id);
    }
}

pub async fn relay_streams_user(
    session: &UserRelaySession,
    client: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
    remote: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
) -> Result<()> {
    let session = session.inner.clone();
    let (mut client_r, mut client_w) = tokio::io::split(client);
    let (mut remote_r, mut remote_w) = tokio::io::split(remote);
    let s_up = session.clone();
    let s_down = session;
    let up = relay_user_half(&mut client_r, &mut remote_w, &s_up, true);
    let down = relay_user_half(&mut remote_r, &mut client_w, &s_down, false);
    tokio::pin!(up);
    tokio::pin!(down);
    tokio::select! {
        r = &mut up => { r?; down.await?; }
        r = &mut down => { r?; up.await?; }
    }
    Ok(())
}

/// Relay between an AnyTLS multiplexed stream and a plain TCP socket with user limits.
pub async fn relay_anytls_stream_user(
    session: &UserRelaySession,
    stream: std::sync::Arc<anytls_rs::session::Stream>,
    outbound: tokio::net::TcpStream,
) -> Result<()> {
    let inner = session.inner.clone();
    let stream_id = stream.id();
    let (mut outbound_read, mut outbound_write) = tokio::io::split(outbound);

    let stream_for_read = std::sync::Arc::clone(&stream);
    let inner_up = inner.clone();
    let up = async move {
        let reader_mutex = stream_for_read.reader();
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = {
                let mut reader = reader_mutex.lock().await;
                reader.read(&mut buf).await?
            };
            if n == 0 {
                break;
            }
            if !inner_up
                .connections
                .user_quota_ok(&inner_up.user_name, &inner_up.limits)
            {
                break;
            }
            if let Some(ref lim) = inner_up.limiter {
                lim.throttle(n as u64).await;
            }
            outbound_write.write_all(&buf[..n]).await?;
            inner_up.connections.record_traffic(
                &inner_up.inbound_tag,
                &inner_up.outbound_tag,
                n as u64,
                0,
                Some(&inner_up.user_name),
            );
        }
        Ok::<(), anyhow::Error>(())
    };

    let stream_for_write = std::sync::Arc::clone(&stream);
    let inner_down = inner;
    let down = async move {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = outbound_read.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            if !inner_down
                .connections
                .user_quota_ok(&inner_down.user_name, &inner_down.limits)
            {
                break;
            }
            if let Some(ref lim) = inner_down.limiter {
                lim.throttle(n as u64).await;
            }
            use bytes::Bytes;
            stream_for_write
                .send_data(Bytes::copy_from_slice(&buf[..n]))
                .map_err(|e| anyhow::anyhow!("anytls stream {stream_id} write: {e:?}"))?;
            inner_down.connections.record_traffic(
                &inner_down.inbound_tag,
                &inner_down.outbound_tag,
                0,
                n as u64,
                Some(&inner_down.user_name),
            );
        }
        Ok::<(), anyhow::Error>(())
    };

    tokio::pin!(up);
    tokio::pin!(down);
    tokio::select! {
        r = &mut up => { r?; let _ = down.await; }
        r = &mut down => { r?; let _ = up.await; }
    }
    Ok(())
}

pub(crate) async fn relay_user_half<R, W>(
    reader: &mut R,
    writer: &mut W,
    session: &std::sync::Arc<UserRelayInner>,
    uplink: bool,
) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        if !session
            .connections
            .user_quota_ok(&session.user_name, &session.limits)
        {
            tracing::info!(user = %session.user_name, "user traffic quota exceeded");
            break;
        }
        if let Some(ref lim) = session.limiter {
            lim.throttle(n as u64).await;
        }
        writer.write_all(&buf[..n]).await?;
        if uplink {
            session.connections.record_traffic(
                &session.inbound_tag,
                &session.outbound_tag,
                n as u64,
                0,
                Some(&session.user_name),
            );
        } else {
            session.connections.record_traffic(
                &session.inbound_tag,
                &session.outbound_tag,
                0,
                n as u64,
                Some(&session.user_name),
            );
        }
    }
    Ok(())
}

pub async fn relay_proxy(a: &mut TcpStream, mut b: ProxyConn) -> Result<()> {
    let copy = tokio::time::timeout(
        std::time::Duration::from_secs(90),
        tokio::io::copy_bidirectional(a, b.as_mut()),
    )
    .await;
    close_inbound_stream(a).await;
    let _ = b.as_mut().shutdown().await;
    drop(b);
    match copy {
        Ok(result) => {
            result?;
        }
        Err(_) => {
            tracing::debug!("relay timed out after 90s");
        }
    }
    Ok(())
}

/// Fully close an inbound client socket (avoids CLOSE_WAIT accumulation on Windows).
async fn close_inbound_stream(stream: &mut TcpStream) {
    let _ = stream.shutdown().await;
    let drain = async {
        let mut discard = [0u8; 4096];
        loop {
            match stream.read(&mut discard).await {
                Ok(0) | Err(_) => break,
                Ok(_) => continue,
            }
        }
    };
    let _ = tokio::time::timeout(INBOUND_DRAIN_TIMEOUT, drain).await;
}

async fn send_http_error(
    stream: &mut TcpStream,
    code: u16,
    reason: &str,
    body: &str,
) -> Result<()> {
    let response = format!(
        "HTTP/1.1 {code} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    Ok(())
}

async fn shutdown_io(
    a: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
    b: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
) {
    let _ = tokio::io::AsyncWriteExt::shutdown(a).await;
    let _ = tokio::io::AsyncWriteExt::shutdown(b).await;
}

async fn client_is_closed(stream: &mut TcpStream) -> bool {
    use std::time::Duration;

    // Must not consume bytes (CONNECT may send TLS ClientHello while we dial outbound).
    let mut buf = [0u8; 1];
    match tokio::time::timeout(Duration::from_millis(50), stream.peek(&mut buf)).await {
        Ok(Ok(0)) => true,
        Ok(Ok(_)) => false,
        Ok(Err(e)) => matches!(
            e.kind(),
            std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::UnexpectedEof
        ),
        Err(_) => false,
    }
}

/// Dial outbound while watching the inbound client; abort if the client disconnects first.
async fn dial_tcp_with_client_watch(
    client: &mut TcpStream,
    dialer: Arc<Dialer>,
    metadata: Metadata,
    dest: SocketAddr,
) -> Result<Option<ProxyConn>> {
    use std::time::Duration;

    let dial = tokio::time::timeout(
        Duration::from_secs(8),
        dialer.dial_tcp(&metadata, dest),
    );
    tokio::pin!(dial);

    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            result = &mut dial => {
                return match result {
                    Ok(Ok(conn)) => Ok(Some(conn)),
                    Ok(Err(e)) => Err(e),
                    Err(_) => anyhow::bail!("outbound dial timeout after 8s"),
                };
            }
            _ = tick.tick() => {
                if client_is_closed(client).await {
                    tracing::debug!("inbound client closed during outbound dial");
                    return Ok(None);
                }
            }
        }
    }
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
    let (host, port, path) = parse_http_url(target)?;
    let (dest, domain) = parse_connect_target(&format!("{host}:{port}"))?;

    let rewritten_request = rewrite_http_request(method, &host, port, &path, full_request)?;

    dial_and_relay_with_initial_data(
        client,
        rewritten_request.into_bytes(),
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
