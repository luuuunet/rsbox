//! TCP/UDP relay for RSQ (Hy2-class data path, RSQ framing).

use super::auth::MBPS_TO_BPS;
use super::bandwidth::{brutal_pacer_from_bps, BrutalPacer};
use super::protocol::{encode_tcp_err, encode_tcp_ok, try_decode_frame};
use super::stream::PrefixedRecvStream;
use super::brutal::brutal_bps_from_mbps;
use super::traffic::{self, TrafficProfile};
use anyhow::{Context, Result};
use quinn::RecvStream;
use rsb_core::{SharedConnectionManager, UserLimits};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Clone)]
pub struct RsqRelayCtx {
    pub connections: SharedConnectionManager,
    pub inbound_tag: String,
    pub password: String,
    pub server_down_mbps: u32,
    /// Client-advertised download capacity (Brutal downlink pacing).
    pub client_rx_bps: u64,
    /// Client-advertised upload capacity (uplink pacing).
    pub client_up_bps: u64,
    pub profile: TrafficProfile,
}

impl RsqRelayCtx {
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

    fn downlink_target_bps(&self, limits: &UserLimits) -> u64 {
        limits.speed_bps.filter(|v| *v > 0).or_else(|| {
            if self.client_rx_bps > 0 {
                Some(self.client_rx_bps)
            } else if self.server_down_mbps > 0 {
                Some(self.server_down_mbps as u64 * MBPS_TO_BPS)
            } else {
                None
            }
        }).unwrap_or_else(|| brutal_bps_from_mbps(0))
    }

    fn uplink_target_bps(&self, limits: &UserLimits) -> u64 {
        limits.speed_bps.filter(|v| *v > 0).or_else(|| {
            if self.client_up_bps > 0 {
                Some(self.client_up_bps)
            } else {
                None
            }
        }).unwrap_or_else(|| brutal_bps_from_mbps(0))
    }
}

pub async fn handle_tcp_stream(
    ctx: RsqRelayCtx,
    mut send: quinn::SendStream,
    mut recv: RecvStream,
) -> Result<()> {
    let mut header = Vec::new();
    let mut chunk = [0u8; 4096];
    let (target, prefix) = loop {
        if let Some(frame) = try_decode_frame(&header)? {
            if frame.frame_type != super::protocol::FRAME_TCP_OPEN {
                write_tcp_err(&mut send, "expected TCP_OPEN").await?;
                return Ok(());
            }
            let target = std::str::from_utf8(&frame.payload)
                .map_err(|e| anyhow::anyhow!("tcp open target utf8: {e}"))?
                .to_string();
            let consumed = super::protocol::frame_consumed_len(&header)?;
            let prefix = header[consumed..].to_vec();
            break (target, prefix);
        }
        let n = recv
            .read(&mut chunk)
            .await
            .context("read tcp open")?
            .ok_or_else(|| anyhow::anyhow!("stream closed before tcp open"))?;
        header.extend_from_slice(&chunk[..n]);
        if header.len() > 16384 {
            write_tcp_err(&mut send, "tcp open frame too large").await?;
            return Ok(());
        }
    };

    let addr = match resolve_target(&target).await {
        Ok(a) => a,
        Err(err) => {
            write_tcp_err(&mut send, &format!("resolve {target}: {err}")).await?;
            return Ok(());
        }
    };

    let remote = match TcpStream::connect(addr).await {
        Ok(r) => r,
        Err(err) => {
            write_tcp_err(&mut send, &format!("connect {target}: {err}")).await?;
            return Ok(());
        }
    };

    let ok_buf = encode_tcp_ok(0);
    send.write_all(&ok_buf).await.context("write tcp ok")?;

    let limits = ctx.user_limits();
    let user_name = ctx.user_name();
    // QUIC session already holds the panel connection slot; streams are muxed only.
    let relay_session = crate::inbound_proxy::UserRelaySession::begin_muxed(
        ctx.connections.clone(),
        &ctx.inbound_tag,
        &user_name,
        limits.clone(),
        Some(addr),
        None,
    );
    let pacer_down = brutal_pacer_from_bps(ctx.downlink_target_bps(&limits));
    let pacer_up = brutal_pacer_from_bps(ctx.uplink_target_bps(&limits));
    let inner = Arc::new(RsqRelayInner {
        inbound_tag: ctx.inbound_tag,
        user_name,
        connections: ctx.connections,
        limits,
        pacer_down,
        pacer_up,
        profile: ctx.profile,
    });
    let (mut remote_read, mut remote_write) = remote.into_split();

    let mut recv_reader = PrefixedRecvStream::new(recv, prefix);
    let c2r = rsq_copy_reader_to_tcp(&inner, &mut recv_reader, &mut remote_write, true);
    let r2c = rsq_copy_tcp_to_send(&inner, &mut remote_read, &mut send, false);
    tokio::pin!(c2r);
    tokio::pin!(r2c);
    tokio::select! {
        r = &mut c2r => { r?; let _ = r2c.await; }
        r = &mut r2c => { r?; let _ = c2r.await; }
    }
    drop(relay_session);
    Ok(())
}

async fn write_tcp_err(send: &mut quinn::SendStream, message: &str) -> Result<()> {
    let err_buf = encode_tcp_err(message, 0);
    send.write_all(&err_buf).await.context("write tcp err")?;
    Ok(())
}

struct RsqRelayInner {
    inbound_tag: String,
    user_name: String,
    connections: SharedConnectionManager,
    limits: UserLimits,
    pacer_down: Arc<BrutalPacer>,
    pacer_up: Arc<BrutalPacer>,
    profile: TrafficProfile,
}

async fn rsq_copy_reader_to_tcp<R: AsyncRead + Unpin>(
    inner: &Arc<RsqRelayInner>,
    reader: &mut R,
    remote: &mut tokio::net::tcp::OwnedWriteHalf,
    uplink: bool,
) -> Result<()> {
    let mut buf = vec![0u8; inner.profile.read_chunk_size()];
    let mut paced = false;
    loop {
        if paced && inner.profile.pace_relay_copy() {
            traffic::paced_copy_chunk(inner.profile).await;
        }
        paced = true;
        let n = reader
            .read(&mut buf)
            .await
            .context("read client stream")?;
        if n == 0 {
            break;
        }
        if !inner
            .connections
            .user_quota_ok(&inner.user_name, &inner.limits)
        {
            tracing::debug!(user = %inner.user_name, "rsq relay quota exceeded (uplink)");
            let _ = remote.shutdown().await;
            break;
        }
        inner.pacer_up.write_all(remote, &buf[..n]).await?;
        record_rsq_traffic(inner, n as u64, uplink);
    }
    Ok(())
}

async fn rsq_copy_tcp_to_send(
    inner: &Arc<RsqRelayInner>,
    remote: &mut tokio::net::tcp::OwnedReadHalf,
    send: &mut quinn::SendStream,
    uplink: bool,
) -> Result<()> {
    let mut buf = vec![0u8; inner.profile.read_chunk_size()];
    let mut paced = false;
    loop {
        if paced && inner.profile.pace_relay_copy() {
            traffic::paced_copy_chunk(inner.profile).await;
        }
        paced = true;
        let n = remote.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        if !inner
            .connections
            .user_quota_ok(&inner.user_name, &inner.limits)
        {
            tracing::debug!(user = %inner.user_name, "rsq relay quota exceeded (downlink)");
            let _ = send.finish();
            break;
        }
        inner.pacer_down.write_all(send, &buf[..n]).await?;
        record_rsq_traffic(inner, n as u64, uplink);
    }
    Ok(())
}

fn record_rsq_traffic(inner: &RsqRelayInner, n: u64, uplink: bool) {
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
        .context("resolve rsq target")?
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
