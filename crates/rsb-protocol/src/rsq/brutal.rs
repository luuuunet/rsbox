//! Hysteria-style Brutal congestion control for RSQ.
//!
//! QUIC layer: custom `quinn::congestion::Controller` that ignores loss backoff and
//! sizes the window from configured target bitrate × RTT (BDP).
//!
//! App layer: `BrutalPacer` / `BrutalWriter` pace `SendStream` writes at the target rate.

use super::auth::MBPS_TO_BPS;
use quinn::congestion::{Controller, ControllerFactory, ControllerMetrics};
use quinn_proto::RttEstimator;
use rsb_core::RateLimiter;
use std::any::Any;
use std::future::Future;
use std::io;
use std::time::Duration;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::io::{AsyncWrite, AsyncWriteExt};

/// Default Brutal target when config omits Mbps (matches G5 Hy2 default).
pub const DEFAULT_BRUTAL_MBPS: u32 = 200;

const BASE_DATAGRAM_SIZE: u64 = 1200;
const BRUTAL_WRITE_CHUNK: usize = 16 * 1024;

pub fn brutal_bps_from_mbps(mbps: u32) -> u64 {
    let mbps = if mbps == 0 { DEFAULT_BRUTAL_MBPS } else { mbps };
    mbps as u64 * MBPS_TO_BPS
}

pub fn brutal_bps_from_pair(up_mbps: u32, down_mbps: u32) -> u64 {
    brutal_bps_from_mbps(up_mbps.max(down_mbps))
}

// ── QUIC congestion controller ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct BrutalConfig {
    target_bps: u64,
    initial_window: u64,
    max_window: u64,
}

impl BrutalConfig {
    pub fn new(target_bps: u64) -> Self {
        let target_bps = target_bps.max(MBPS_TO_BPS);
        let initial_window = (target_bps / 10)
            .clamp(2 * BASE_DATAGRAM_SIZE, 2 * 1024 * 1024);
        let max_window = (target_bps * 2).clamp(initial_window, 20 * 1024 * 1024);
        Self {
            target_bps,
            initial_window,
            max_window,
        }
    }

    pub fn from_mbps(mbps: u32) -> Self {
        Self::new(brutal_bps_from_mbps(mbps))
    }

    pub fn from_mbps_pair(up_mbps: u32, down_mbps: u32) -> Self {
        Self::new(brutal_bps_from_pair(up_mbps, down_mbps))
    }
}

#[derive(Debug, Clone)]
struct Brutal {
    config: Arc<BrutalConfig>,
    current_mtu: u64,
    window: u64,
    smoothed_rtt: std::time::Duration,
}

impl Brutal {
    fn new(config: Arc<BrutalConfig>, _now: Instant, current_mtu: u16) -> Self {
        Self {
            window: config.initial_window,
            current_mtu: current_mtu as u64,
            smoothed_rtt: std::time::Duration::from_millis(100),
            config,
        }
    }

    fn compute_window(&self) -> u64 {
        let rtt_secs = self.smoothed_rtt.as_secs_f64().max(0.05);
        let bdp = (self.config.target_bps as f64 * rtt_secs) as u64;
        bdp.max(2 * self.current_mtu)
            .min(self.config.max_window)
    }
}

impl Controller for Brutal {
    fn on_ack(
        &mut self,
        _now: Instant,
        _sent: Instant,
        _bytes: u64,
        _app_limited: bool,
        rtt: &RttEstimator,
    ) {
        self.smoothed_rtt = rtt.get();
        self.window = self.compute_window();
    }

    fn on_congestion_event(
        &mut self,
        _now: Instant,
        _sent: Instant,
        _is_persistent_congestion: bool,
        _lost_bytes: u64,
    ) {
        // Brutal: do not back off on loss — keep target BDP window.
        self.window = self.compute_window();
    }

    fn on_mtu_update(&mut self, new_mtu: u16) {
        self.current_mtu = new_mtu as u64;
        self.window = self.window.max(2 * self.current_mtu);
    }

    fn window(&self) -> u64 {
        self.window.max(2 * self.current_mtu)
    }

    fn metrics(&self) -> ControllerMetrics {
        let mut m = ControllerMetrics::default();
        m.congestion_window = self.window();
        m.pacing_rate = Some(self.config.target_bps.saturating_mul(8));
        m
    }

    fn clone_box(&self) -> Box<dyn Controller> {
        Box::new(self.clone())
    }

    fn initial_window(&self) -> u64 {
        self.config.initial_window
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
}

impl ControllerFactory for BrutalConfig {
    fn build(self: Arc<Self>, now: Instant, current_mtu: u16) -> Box<dyn Controller> {
        Box::new(Brutal::new(self, now, current_mtu))
    }
}

// ── App-layer pacer (SendStream / relay writes) ────────────────────────────

#[derive(Debug)]
pub struct BrutalPacer {
    target_bps: u64,
    limiter: RateLimiter,
}

impl BrutalPacer {
    pub fn new(target_bps: u64) -> Self {
        let target_bps = if target_bps == 0 {
            brutal_bps_from_mbps(0)
        } else {
            target_bps
        };
        Self {
            target_bps,
            limiter: RateLimiter::new(target_bps),
        }
    }

    pub fn from_mbps(mbps: u32) -> Arc<Self> {
        Arc::new(Self::new(brutal_bps_from_mbps(mbps)))
    }

    pub fn target_bps(&self) -> u64 {
        self.target_bps
    }

    pub async fn pace(&self, nbytes: u64) {
        if nbytes == 0 {
            return;
        }
        self.limiter.throttle(nbytes).await;
    }

    pub fn acquire_wait(&self, nbytes: u64) -> Duration {
        self.limiter.acquire_wait(nbytes)
    }

    pub async fn write_all<W: AsyncWrite + Unpin>(
        &self,
        writer: &mut W,
        buf: &[u8],
    ) -> io::Result<()> {
        let mut offset = 0;
        while offset < buf.len() {
            let end = (offset + BRUTAL_WRITE_CHUNK).min(buf.len());
            let chunk = &buf[offset..end];
            self.pace(chunk.len() as u64).await;
            writer.write_all(chunk).await?;
            offset = end;
        }
        Ok(())
    }
}

pub struct BrutalWriter<W> {
    inner: W,
    pacer: Arc<BrutalPacer>,
    delay: Option<Pin<Box<tokio::time::Sleep>>>,
}

impl<W> BrutalWriter<W> {
    pub fn new(inner: W, pacer: Arc<BrutalPacer>) -> Self {
        Self {
            inner,
            pacer,
            delay: None,
        }
    }

    pub fn into_inner(self) -> W {
        self.inner
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for BrutalWriter<W> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if let Some(delay) = self.delay.as_mut() {
            if delay.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.delay = None;
        }
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let wait = self.pacer.acquire_wait(buf.len() as u64);
        if !wait.is_zero() {
            self.delay = Some(Box::pin(tokio::time::sleep(wait)));
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_target_is_200mbps() {
        assert_eq!(brutal_bps_from_mbps(0), 200 * MBPS_TO_BPS);
    }

    #[test]
    fn brutal_window_grows_with_rtt() {
        let cfg = Arc::new(BrutalConfig::new(25 * MBPS_TO_BPS));
        let mut ctrl = Brutal::new(cfg, Instant::now(), 1200);
        ctrl.smoothed_rtt = std::time::Duration::from_millis(200);
        ctrl.window = ctrl.compute_window();
        assert!(ctrl.window() >= 2 * BASE_DATAGRAM_SIZE);
        assert!(ctrl.window() <= 20 * 1024 * 1024);
    }

    #[test]
    fn congestion_event_does_not_shrink_window() {
        let cfg = Arc::new(BrutalConfig::new(100 * MBPS_TO_BPS));
        let mut ctrl = Brutal::new(cfg, Instant::now(), 1200);
        ctrl.smoothed_rtt = std::time::Duration::from_millis(100);
        ctrl.window = ctrl.compute_window();
        let before = ctrl.window();
        ctrl.on_congestion_event(Instant::now(), Instant::now(), true, 4096);
        assert!(ctrl.window() >= before / 2);
    }
}
