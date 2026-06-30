use super::auth::MBPS_TO_BPS;
use super::protocol::{decode_tcp_request, encode_tcp_response};
use anyhow::{Context, Result};
use quinn::RecvStream;
use rsb_core::{SharedConnectionManager, UserLimits};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Clone)]
pub struct Hy2RelayCtx {
    pub connections: SharedConnectionManager,
    pub inbound_tag: String,
    pub password: String,
    pub server_down_mbps: u32,
}

impl Hy2RelayCtx {
    fn user_limits(&self) -> UserLimits {
        self.connections
            .users()
            .lookup_password(&self.password)
            .map(|r| r.limits.clone())
            .unwrap_or_default()
    }

    fn user_name(&self) -> String {
        self.connections
            .users()
            .lookup_password(&self.password)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| self.password.chars().take(8).collect())
    }

    fn speed_bps(&self, limits: &UserLimits) -> Option<u64> {
        limits.speed_bps.filter(|v| *v > 0).or_else(|| {
            if self.server_down_mbps > 0 {
                Some(self.server_down_mbps as u64 * MBPS_TO_BPS)
            } else {
                None
            }
        })
    }
}

pub async fn handle_tcp_stream(
    ctx: Hy2RelayCtx,
    mut send: quinn::SendStream,
    mut recv: RecvStream,
) -> Result<()> {
    let mut header = vec![0u8; 4096];
    let n = recv
        .read(&mut header)
        .await
        .context("read tcp request")?
        .ok_or_else(|| anyhow::anyhow!("stream closed before request"))?;
    let mut cursor = &header[..n];
    let target = decode_tcp_request(&mut cursor).context("decode tcp request")?;
    let addr = resolve_target(&target).await?;

    let remote = TcpStream::connect(addr)
        .await
        .with_context(|| format!("connect {target}"))?;

    let ok_buf = encode_tcp_response(true, "ok", 0);
    send.write_all(&ok_buf).await.context("write tcp ok")?;

    let limits = ctx.user_limits();
    let user_name = ctx.user_name();
    let relay_session = crate::inbound_proxy::UserRelaySession::begin(
        ctx.connections.clone(),
        &ctx.inbound_tag,
        &user_name,
        limits.clone(),
        Some(addr),
        None,
    )?;
    let limiter = ctx.connections.user_limiter(&user_name, ctx.speed_bps(&limits));

    let (mut remote_read, mut remote_write) = remote.into_split();
    let inner = Arc::new(Hy2RelayInner {
        inbound_tag: ctx.inbound_tag,
        user_name,
        connections: ctx.connections,
        limits,
        limiter,
    });

    let c2r = hy2_copy_recv_to_tcp(&inner, &mut recv, &mut remote_write, true);
    let r2c = hy2_copy_tcp_to_send(&inner, &mut remote_read, &mut send, false);
    tokio::pin!(c2r);
    tokio::pin!(r2c);
    tokio::select! {
        r = &mut c2r => { r?; let _ = r2c.await; }
        r = &mut r2c => { r?; let _ = c2r.await; }
    }
    drop(relay_session);
    Ok(())
}

struct Hy2RelayInner {
    inbound_tag: String,
    user_name: String,
    connections: SharedConnectionManager,
    limits: UserLimits,
    limiter: Option<Arc<rsb_core::RateLimiter>>,
}

async fn hy2_copy_recv_to_tcp(
    inner: &Arc<Hy2RelayInner>,
    recv: &mut RecvStream,
    remote: &mut tokio::net::tcp::OwnedWriteHalf,
    uplink: bool,
) -> Result<()> {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = recv
            .read(&mut buf)
            .await
            .context("read client stream")?
            .unwrap_or(0);
        if n == 0 {
            break;
        }
        if !inner
            .connections
            .user_quota_ok(&inner.user_name, &inner.limits)
        {
            break;
        }
        if let Some(ref lim) = inner.limiter {
            lim.throttle(n as u64).await;
        }
        remote.write_all(&buf[..n]).await?;
        record_hy2_traffic(inner, n as u64, uplink);
    }
    Ok(())
}

async fn hy2_copy_tcp_to_send(
    inner: &Arc<Hy2RelayInner>,
    remote: &mut tokio::net::tcp::OwnedReadHalf,
    send: &mut quinn::SendStream,
    uplink: bool,
) -> Result<()> {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        let n = remote.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        if !inner
            .connections
            .user_quota_ok(&inner.user_name, &inner.limits)
        {
            break;
        }
        if let Some(ref lim) = inner.limiter {
            lim.throttle(n as u64).await;
        }
        send.write_all(&buf[..n]).await?;
        record_hy2_traffic(inner, n as u64, uplink);
    }
    Ok(())
}

fn record_hy2_traffic(inner: &Hy2RelayInner, n: u64, uplink: bool) {
    if uplink {
        inner.connections.record_traffic(
            &inner.inbound_tag,
            "direct",
            n,
            0,
            Some(&inner.user_name),
        );
    } else {
        inner.connections.record_traffic(
            &inner.inbound_tag,
            "direct",
            0,
            n,
            Some(&inner.user_name),
        );
    }
}

async fn resolve_target(target: &str) -> Result<SocketAddr> {
    if let Ok(addr) = target.parse::<SocketAddr>() {
        return Ok(addr);
    }
    tokio::net::lookup_host(target)
        .await
        .context("resolve hysteria2 target")?
        .next()
        .with_context(|| format!("no addresses for {target}"))
}

pub async fn parse_udp_target(addr: &str) -> Result<SocketAddr> {
    resolve_target(addr).await
}

pub async fn forward_udp_payload(
    socket: &tokio::net::UdpSocket,
    target: SocketAddr,
    payload: &[u8],
) -> Result<usize> {
    socket.send_to(payload, target).await.context("udp send")
}

pub fn ensure_fragment_ready(msg: &super::protocol::UdpMessage) -> Result<()> {
    if msg.fragment_count != 1 {
        anyhow::bail!("udp fragmentation is not supported yet");
    }
    Ok(())
}
