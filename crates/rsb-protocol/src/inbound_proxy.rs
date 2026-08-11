use crate::direct::parse_listen;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Dialer, Inbound, Metadata, Network, ProxyConn};
use rsb_dns::DnsRouter;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

/// 仅限制「握手中」并发（读 CONNECT/SOCKS 头）。拨号/转发不再占此许可。
const MAX_CONCURRENT_INBOUND: usize = 256;
/// 含 dial+relay 的总活跃连接硬顶。允许大量并发，但禁止无界堆积导致假死。
const MAX_ACTIVE_CONNECTIONS: usize = 512;
/// outbound dial 全局限流（仅代理链路）。直连绝不能进此队列，
/// 否则节点卡死时百度等国内站 CONNECT 也会被拖成「假死」。
const MAX_CONCURRENT_DIALS: usize = 48;
const INBOUND_ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
/// 读完代理请求头的上限；超时必须释许可，否则 CLOSE_WAIT/假死。
const HANDSHAKE_IO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const INBOUND_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);
/// Absolute ceiling for one relay; idle/EOF should finish much sooner.
const RELAY_TOTAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
/// 对齐 Clash/clash-rs 常见策略：双向约 60s 无字节则拆隧道。
/// 视频/直播持续有流量会不断刷新；闲置连接不长期占用（减轻 CLOSE_WAIT）。
const RELAY_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// 写方向半死远端更快失败，避免卡在 write 上堆 CLOSE_WAIT。
const RELAY_WRITE_IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const OUTBOUND_DIAL_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const PROXY_DIAL_ACQUIRE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
const RESOLVE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
const RELAY_BUF_SIZE: usize = 16 * 1024;

/// 连接生命周期计数：accept→close 全程持有，Drop 时自动 -1。
struct ActiveConnGuard {
    counter: Arc<AtomicUsize>,
}

impl Drop for ActiveConnGuard {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

fn try_acquire_active(counter: &Arc<AtomicUsize>) -> Option<ActiveConnGuard> {
    loop {
        let cur = counter.load(Ordering::Relaxed);
        if cur >= MAX_ACTIVE_CONNECTIONS {
            return None;
        }
        if counter
            .compare_exchange(cur, cur + 1, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            return Some(ActiveConnGuard {
                counter: Arc::clone(counter),
            });
        }
    }
}

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
        let concurrency = Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_INBOUND));
        let active_conns = Arc::new(AtomicUsize::new(0));
        let accept_count = Arc::new(AtomicU64::new(0));
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    accept = listener.accept() => {
                        let (mut stream, peer) = match accept {
                            Ok(v) => v,
                            Err(err) => {
                                // 偶发 accept 失败不应毁掉整个 inbound（旧逻辑 break → 端口假死）。
                                tracing::warn!(error = %err, "inbound accept failed, retrying");
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                continue;
                            }
                        };
                        tune_accepted_stream(&stream);

                        // 总活跃连接硬顶：超额立刻 RST，避免 ESTABLISHED/CLOSE_WAIT 无界。
                        let Some(active_guard) = try_acquire_active(&active_conns) else {
                            let n = active_conns.load(Ordering::Relaxed);
                            tracing::debug!(active = n, "inbound active cap hit, RST");
                            abort_inbound_socket(&stream);
                            drop(stream);
                            continue;
                        };

                        let n = accept_count.fetch_add(1, Ordering::Relaxed) + 1;
                        if n % 64 == 0 {
                            tracing::debug!(
                                accepts = n,
                                active = active_conns.load(Ordering::Relaxed),
                                "inbound connection stats"
                            );
                        }

                        let dialer = dialer.clone();
                        let dns = dns.clone();
                        let tag = tag.clone();
                        let kind = kind.clone();
                        let concurrency = concurrency.clone();

                        tokio::spawn(async move {
                            // 持有至任务结束，Drop 时释放活跃计数。
                            let _active_guard = active_guard;

                            let Ok(permit) = tokio::time::timeout(
                                INBOUND_ACQUIRE_TIMEOUT,
                                concurrency.acquire_owned(),
                            )
                            .await
                            else {
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

                            // 不再对整段 relay 套 120s 总超时：长视频会被误杀，
                            // 且取消路径易留下 CLOSE_WAIT。握手/空闲由内部超时负责。
                            let result = handle_client(
                                &mut stream,
                                peer,
                                &tag,
                                &kind,
                                mode,
                                dialer,
                                dns,
                                &mut handshake_permit,
                            )
                            .await;

                            // 确保握手许可一定释放（错误路径也可能未 take）。
                            drop(handshake_permit);

                            match result {
                                Ok(()) => {
                                    tracing::trace!("Connection completed successfully");
                                }
                                Err(err) => {
                                    tracing::debug!(error = ?err, "proxy client failed");
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
    tokio::time::timeout(HANDSHAKE_IO_TIMEOUT, stream.read_exact(&mut header))
        .await
        .map_err(|_| anyhow::anyhow!("socks header timeout"))??;
    if header[0] != 0x05 {
        anyhow::bail!("invalid socks version");
    }
    let mut methods = vec![0u8; header[1] as usize];
    tokio::time::timeout(HANDSHAKE_IO_TIMEOUT, stream.read_exact(&mut methods))
        .await
        .map_err(|_| anyhow::anyhow!("socks methods timeout"))??;
    stream.write_all(&[0x05, 0x00]).await?;
    let mut req = [0u8; 4];
    tokio::time::timeout(HANDSHAKE_IO_TIMEOUT, stream.read_exact(&mut req))
        .await
        .map_err(|_| anyhow::anyhow!("socks request timeout"))??;
    let (dest, domain) = tokio::time::timeout(HANDSHAKE_IO_TIMEOUT, read_socks_addr(stream, req[3]))
        .await
        .map_err(|_| anyhow::anyhow!("socks addr timeout"))??;
    // 先拨号再回成功：节点/直连失败时不要让浏览器以为隧道已通。
    release_handshake_permit(handshake_permit);
    let Some(remote) = dial_tcp_only(
        stream,
        peer,
        inbound_tag,
        inbound_type,
        dialer,
        dns,
        dest,
        domain,
    )
    .await?
    else {
        return Ok(());
    };
    stream
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await?;
    relay_proxy(stream, remote).await
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
    let mut reader = BufReader::new(stream);

    let mut request_line = String::new();
    tokio::time::timeout(HANDSHAKE_IO_TIMEOUT, reader.read_line(&mut request_line))
        .await
        .map_err(|_| anyhow::anyhow!("http request line timeout"))??;
    let mut full_request = request_line.clone();

    let mut parts = request_line.trim().split_whitespace();
    let method = parts.next().context("no method")?;
    let target = parts.next().context("no target")?;

    tracing::debug!(method = %method, target = %target, "HTTP proxy request");

    // 读取所有头部直到空行（必须限时，否则占满握手许可 → 端口假死）
    loop {
        let mut line = String::new();
        tokio::time::timeout(HANDSHAKE_IO_TIMEOUT, reader.read_line(&mut line))
            .await
            .map_err(|_| anyhow::anyhow!("http header timeout"))??;
        if line == "\r\n" || line == "\n" || line.is_empty() {
            full_request.push_str("\r\n");
            break;
        }
        full_request.push_str(&line);
    }

    if method == "CONNECT" {
        let (dest, domain) = parse_connect_target(target)?;
        let buffered = reader.buffer().to_vec();
        let mut stream = reader.into_inner();
        release_handshake_permit(handshake_permit);

        // 先拨号再回 200：旧逻辑「先 200 再拨号」在 outbound/RSQ 卡住时，
        // 浏览器会认为隧道已通并狂发连接，最终 CLOSE_WAIT 堆积、新 CONNECT 无响应。
        let remote = match dial_tcp_only(
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
        {
            Ok(Some(r)) => r,
            Ok(None) => return Ok(()),
            Err(err) => {
                let _ = send_http_error(
                    &mut stream,
                    502,
                    "Bad Gateway",
                    "outbound dial failed",
                )
                .await;
                return Err(err);
            }
        };
        let mut remote = remote;

        if let Err(err) = stream
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
        {
            let _ = remote.as_mut().shutdown().await;
            return Err(err.into());
        }

        if !buffered.is_empty() {
            if let Err(err) = remote.as_mut().write_all(&buffered).await {
                let _ = remote.as_mut().shutdown().await;
                return Err(err.into());
            }
        }

        relay_proxy(&mut stream, remote).await
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
    domain: Option<String>,
) -> Result<()> {
    let Some(remote) = dial_tcp_only(
        client,
        peer,
        inbound_tag,
        inbound_type,
        dialer,
        dns,
        dest,
        domain,
    )
    .await?
    else {
        return Ok(());
    };
    relay_proxy(client, remote).await
}

/// 只拨号不转发。用于 CONNECT/SOCKS「先拨通再应答」。
async fn dial_tcp_only(
    client: &mut TcpStream,
    peer: SocketAddr,
    inbound_tag: &str,
    inbound_type: &str,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    dest: SocketAddr,
    domain: Option<String>,
) -> Result<Option<ProxyConn>> {
    if client_is_closed(client).await {
        abort_if_client_gone(client);
        return Ok(None);
    }

    // 系统代理 / mixed 来自 127.0.0.1：同步 GetExtendedTcpTable 会阻塞 Tokio worker，
    // 高并发时整机表现为「端口在听、CONNECT 已 200、随后全站超时」。
    let process = if peer.ip().is_loopback() {
        rsb_core::ProcessInfo::default()
    } else {
        rsb_core::lookup_process_for_tcp_stream(client)
    };

    let (dest, metadata, is_direct) = {
        let resolve = resolve_for_connect(
            &dialer,
            &dns,
            peer,
            inbound_tag,
            inbound_type,
            dest,
            domain,
            process.name,
            process.path,
        );
        tokio::pin!(resolve);
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let deadline = tokio::time::sleep(RESOLVE_TIMEOUT);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                result = &mut resolve => break result?,
                _ = &mut deadline => {
                    anyhow::bail!("CONNECT resolve timed out after {RESOLVE_TIMEOUT:?}");
                }
                _ = tick.tick() => {
                    if client_is_closed(client).await {
                        abort_if_client_gone(client);
                        return Ok(None);
                    }
                }
            }
        }
    };

    tracing::debug!(
        is_direct,
        "dial_tcp_only: connecting to {:?}, domain: {:?}",
        dest,
        metadata.domain
    );

    match dial_tcp_with_client_watch(client, dialer, metadata, dest, is_direct).await {
        Ok(None) => {
            abort_if_client_gone(client);
            Ok(None)
        }
        other => other,
    }
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
    let Some(mut remote) = dial_tcp_only(
        client,
        peer,
        inbound_tag,
        inbound_type,
        dialer,
        dns,
        dest,
        domain,
    )
    .await?
    else {
        return Ok(());
    };

    remote.as_mut().write_all(&initial_data).await?;
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

/// CONNECT：先路由，再按 outbound 选 DNS。
/// - direct：系统 DNS（国内站避免 remote-dns 境外 GeoDNS 拿到不可用 IP）
/// - 代理：DnsRouter（remote-dns + detour，避免污染）
async fn resolve_for_connect(
    dialer: &Dialer,
    dns: &DnsRouter,
    peer: SocketAddr,
    inbound_tag: &str,
    inbound_type: &str,
    dest: SocketAddr,
    domain: Option<String>,
    process_name: Option<String>,
    process_path: Option<String>,
) -> Result<(SocketAddr, Metadata, bool)> {
    let port = dest.port();
    let route_dest = if dest.ip().is_unspecified() {
        SocketAddr::from(([0, 0, 0, 0], port))
    } else {
        dest
    };
    let metadata_for_route = Metadata {
        network: Network::Tcp,
        source: Some(peer),
        destination: Some(route_dest),
        domain: domain.clone(),
        protocol: Some("https".to_string()),
        process_name,
        process_path,
        inbound_tag: inbound_tag.to_string(),
        inbound_type: inbound_type.to_string(),
        user: None,
    };

    let tag = dialer.route_tag(&metadata_for_route).await?;
    let use_system_dns = dialer.is_direct_outbound(&tag);

    // direct：不在此预解析成单一 IP。DirectOutbound 会对全部 A 记录逐个拨号
    // （百度等多 CDN 站第一个 IP 偶发不可达时，旧逻辑会 15s 超时假死）。
    let resolved = if domain.is_none() {
        dest
    } else if use_system_dns {
        SocketAddr::from(([0, 0, 0, 0], port))
    } else {
        resolve_destination(dns, dest, domain.as_deref()).await?
    };

    tracing::debug!(
        outbound = %tag,
        system_dns = use_system_dns,
        resolved = %resolved,
        domain = ?domain,
        "CONNECT resolve"
    );

    let mut metadata = metadata_for_route;
    metadata.destination = Some(resolved);
    Ok((resolved, metadata, use_system_dns))
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
    // Resolve CONNECT hostnames via DnsRouter (remote-dns + detour when configured).
    // CN system DNS pollutes www.google.com to FB/Twitter IPs; RSQ server-side resolve
    // also often picks unreachable anycast. Clean A over 8.8.8.8-via-RSQ + dial by IP.
    let addrs = dns.lookup(host).await?;
    let ip = addrs
        .into_iter()
        .next()
        .context("dns lookup returned no addresses")?;
    let _ = placeholder; // port already taken; IP replaced by DNS
    Ok(SocketAddr::new(ip, port))
}

pub async fn relay_bidirectional(
    a: &mut TcpStream,
    mut b: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
) -> Result<()> {
    let copy = tokio::time::timeout(
        RELAY_TOTAL_TIMEOUT,
        relay_until_either_eof_sized(a, &mut b),
    )
    .await;
    let _ = tokio::io::AsyncWriteExt::shutdown(&mut b).await;
    close_inbound_stream(a).await;
    match copy {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => {
            tracing::debug!(error = %err, "relay_bidirectional ended");
            Ok(())
        }
        Err(_) => Ok(()),
    }
}

pub async fn relay_streams(
    a: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
    b: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin),
) -> Result<()> {
    let copy = tokio::time::timeout(
        RELAY_TOTAL_TIMEOUT,
        relay_until_either_eof_sized(a, b),
    )
    .await;
    shutdown_io(a, b).await;
    match copy {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => {
            tracing::debug!(error = %err, "relay_streams ended");
            Ok(())
        }
        Err(_) => Ok(()),
    }
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
    // Do NOT use copy_bidirectional: it waits for BOTH EOFs. Client FIN first
    // leaves the inbound TCP in CLOSE_WAIT until the outbound also EOFs (or
    // total timeout). Tear down as soon as either side ends or goes idle.
    //
    // 智能策略：有流量则保持（视频/直播可持续）；双向数秒无字节则拆掉，
    // 需要时由浏览器再建新连接。write 也必须带超时，否则远端半死会卡死
    // select → 读不到客户端 FIN → CLOSE_WAIT 堆积。
    let copy = tokio::time::timeout(RELAY_TOTAL_TIMEOUT, async {
        let mut a_buf = vec![0u8; RELAY_BUF_SIZE];
        let mut b_buf = vec![0u8; RELAY_BUF_SIZE];
        loop {
            tokio::select! {
                r = read_with_idle(a, &mut a_buf) => {
                    match r {
                        Ok(0) => break,
                        Ok(n) => write_with_idle(b.as_mut(), &a_buf[..n]).await?,
                        Err(err) => {
                            tracing::trace!(error = %err, "client->remote read end");
                            break;
                        }
                    }
                }
                r = read_with_idle(b.as_mut(), &mut b_buf) => {
                    match r {
                        Ok(0) => break,
                        Ok(n) => write_with_idle(a, &b_buf[..n]).await?,
                        Err(err) => {
                            tracing::trace!(error = %err, "remote->client read end");
                            break;
                        }
                    }
                }
            }
        }
        let _ = a.shutdown().await;
        let _ = b.as_mut().shutdown().await;
        Ok::<(), anyhow::Error>(())
    })
    .await;

    // Always release both ends immediately — this is what clears CLOSE_WAIT.
    let _ = b.as_mut().shutdown().await;
    drop(b);
    close_inbound_stream(a).await;

    match copy {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => {
            tracing::debug!(error = %err, "relay ended");
            Ok(())
        }
        Err(_) => {
            tracing::debug!("relay hit total timeout");
            Ok(())
        }
    }
}

/// Sized variant for callers that own concrete stream types.
async fn relay_until_either_eof_sized<A, B>(a: &mut A, b: &mut B) -> Result<()>
where
    A: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    B: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut a_buf = vec![0u8; RELAY_BUF_SIZE];
    let mut b_buf = vec![0u8; RELAY_BUF_SIZE];
    loop {
        tokio::select! {
            r = read_with_idle(a, &mut a_buf) => {
                match r {
                    Ok(0) => break,
                    Ok(n) => write_with_idle(b, &a_buf[..n]).await?,
                    Err(err) => {
                        tracing::trace!(error = %err, "a->b read end");
                        break;
                    }
                }
            }
            r = read_with_idle(b, &mut b_buf) => {
                match r {
                    Ok(0) => break,
                    Ok(n) => write_with_idle(a, &b_buf[..n]).await?,
                    Err(err) => {
                        tracing::trace!(error = %err, "b->a read end");
                        break;
                    }
                }
            }
        }
    }
    let _ = a.shutdown().await;
    let _ = b.shutdown().await;
    Ok(())
}

async fn read_with_idle<R>(reader: &mut R, buf: &mut [u8]) -> std::io::Result<usize>
where
    R: tokio::io::AsyncRead + Unpin + ?Sized,
{
    match tokio::time::timeout(RELAY_IDLE_TIMEOUT, AsyncReadExt::read(reader, buf)).await {
        Ok(r) => r,
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "relay idle timeout",
        )),
    }
}

async fn write_with_idle<W>(writer: &mut W, buf: &[u8]) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin + ?Sized,
{
    match tokio::time::timeout(RELAY_WRITE_IDLE_TIMEOUT, AsyncWriteExt::write_all(writer, buf)).await
    {
        Ok(r) => r,
        Err(_) => Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "relay write idle timeout",
        )),
    }
}

/// Keepalive + nodelay on accepted client sockets (listen-socket keepalive is not enough).
fn tune_accepted_stream(stream: &TcpStream) {
    let _ = stream.set_nodelay(true);
    let sock = socket2::SockRef::from(stream);
    let mut ka = socket2::TcpKeepalive::new().with_time(std::time::Duration::from_secs(30));
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        ka = ka.with_interval(std::time::Duration::from_secs(10));
    }
    #[cfg(target_os = "linux")]
    {
        ka = ka.with_retries(3);
    }
    if let Err(err) = sock.set_tcp_keepalive(&ka) {
        tracing::trace!(error = %err, "set_tcp_keepalive on accepted stream failed");
    }
}

/// Fully close an inbound client socket (avoids CLOSE_WAIT accumulation on Windows).
async fn close_inbound_stream(stream: &mut TcpStream) {
    // Peer may already have FINed (CLOSE_WAIT). Drain any leftover, then FIN our
    // write half; if drain stalls, RST so the fd does not linger in CLOSE_WAIT.
    let drain = async {
        let mut discard = [0u8; 4096];
        loop {
            match stream.read(&mut discard).await {
                Ok(0) | Err(_) => break,
                Ok(_) => continue,
            }
        }
    };
    let drained = tokio::time::timeout(INBOUND_DRAIN_TIMEOUT, drain).await;
    let _ = stream.shutdown().await;
    // Always arm linger-0 before drop so Windows clears CLOSE_WAIT on drop even
    // if the peer is half-closed and a graceful FIN handshake would stall.
    abort_inbound_socket(stream);
    if drained.is_err() {
        tracing::trace!("inbound drain timed out; socket armed for RST on drop");
    }
}

/// RST the socket when graceful drain times out (prevents CLOSE_WAIT pile-up).
fn abort_inbound_socket(stream: &TcpStream) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        let raw = stream.as_raw_socket();
        unsafe {
            let mut linger = windows_sys::Win32::Networking::WinSock::LINGER {
                l_onoff: 1,
                l_linger: 0,
            };
            windows_sys::Win32::Networking::WinSock::setsockopt(
                raw as usize,
                windows_sys::Win32::Networking::WinSock::SOL_SOCKET,
                windows_sys::Win32::Networking::WinSock::SO_LINGER,
                &linger as *const _ as *const u8,
                std::mem::size_of::<windows_sys::Win32::Networking::WinSock::LINGER>() as i32,
            );
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = stream.as_raw_fd();
        unsafe {
            let linger = libc::linger {
                l_onoff: 1,
                l_linger: 0,
            };
            libc::setsockopt(
                fd,
                libc::SOL_SOCKET,
                libc::SO_LINGER,
                &linger as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::linger>() as libc::socklen_t,
            );
        }
    }
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
    let _ = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        stream.write_all(response.as_bytes()),
    )
    .await;
    let _ = tokio::time::timeout(std::time::Duration::from_millis(200), stream.flush()).await;
    // 错误响应后立刻 RST，避免半关闭卡在 CLOSE_WAIT。
    abort_inbound_socket(stream);
    Ok(())
}

/// 客户端已 FIN：立刻 linger-0，禁止继续等 outbound。
fn abort_if_client_gone(client: &TcpStream) {
    abort_inbound_socket(client);
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
///
/// `is_direct`：直连不走全局限流。智能分流下大量境外 CONNECT 卡在 RSQ 时，
/// 若直连也排队，会出现「端口在听、百度 CONNECT 也超时」的假死。
async fn dial_tcp_with_client_watch(
    client: &mut TcpStream,
    dialer: Arc<Dialer>,
    metadata: Metadata,
    dest: SocketAddr,
    is_direct: bool,
) -> Result<Option<ProxyConn>> {
    use std::sync::OnceLock;

    let _dial_permit = if is_direct {
        None
    } else {
        static DIAL_LIMITER: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
        let limiter = DIAL_LIMITER
            .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_DIALS)));
        // 排队期间也要看客户端是否已断开，否则 CLOSE_WAIT 堆积。
        let acquire = limiter.acquire();
        tokio::pin!(acquire);
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let deadline = tokio::time::sleep(PROXY_DIAL_ACQUIRE_TIMEOUT);
        tokio::pin!(deadline);
        loop {
            tokio::select! {
                permit = &mut acquire => {
                    break Some(
                        permit.map_err(|_| anyhow::anyhow!("outbound dial semaphore closed"))?,
                    );
                }
                _ = &mut deadline => {
                    anyhow::bail!("outbound dial concurrency saturated");
                }
                _ = tick.tick() => {
                    if client_is_closed(client).await {
                        tracing::debug!("inbound client closed while waiting dial permit");
                        abort_if_client_gone(client);
                        return Ok(None);
                    }
                }
            }
        }
    };

    let dial_timeout = if is_direct {
        // 直连本身有 per-IP 800ms；总预算略宽即可
        std::time::Duration::from_secs(3)
    } else {
        OUTBOUND_DIAL_TIMEOUT
    };
    let dial = tokio::time::timeout(dial_timeout, dialer.dial_tcp(&metadata, dest));
    tokio::pin!(dial);

    let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            result = &mut dial => {
                return match result {
                    Ok(Ok(conn)) => Ok(Some(conn)),
                    Ok(Err(e)) => Err(e),
                    Err(_) => anyhow::bail!("outbound dial timeout after {dial_timeout:?}"),
                };
            }
            _ = tick.tick() => {
                if client_is_closed(client).await {
                    tracing::debug!("inbound client closed during outbound dial");
                    abort_if_client_gone(client);
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
