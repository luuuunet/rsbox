// 流量统计实现
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct TrafficStats {
    global_uplink: Arc<AtomicU64>,
    global_downlink: Arc<AtomicU64>,
    by_outbound: Arc<DashMap<String, OutboundStats>>,
    by_inbound: Arc<DashMap<String, InboundStats>>,
}

#[derive(Debug)]
struct OutboundStats {
    uplink: AtomicU64,
    downlink: AtomicU64,
    connections: AtomicU64,
}

#[derive(Debug)]
struct InboundStats {
    uplink: AtomicU64,
    downlink: AtomicU64,
    connections: AtomicU64,
}

impl TrafficStats {
    pub fn new() -> Self {
        Self {
            global_uplink: Arc::new(AtomicU64::new(0)),
            global_downlink: Arc::new(AtomicU64::new(0)),
            by_outbound: Arc::new(DashMap::new()),
            by_inbound: Arc::new(DashMap::new()),
        }
    }

    /// 记录上传流量
    pub fn record_uplink(&self, bytes: u64) {
        self.global_uplink.fetch_add(bytes, Ordering::Relaxed);
    }

    /// 记录下载流量
    pub fn record_downlink(&self, bytes: u64) {
        self.global_downlink.fetch_add(bytes, Ordering::Relaxed);
    }

    /// 记录出站流量
    pub fn record_outbound_uplink(&self, tag: &str, bytes: u64) {
        self.record_uplink(bytes);

        self.by_outbound
            .entry(tag.to_string())
            .or_insert_with(|| OutboundStats {
                uplink: AtomicU64::new(0),
                downlink: AtomicU64::new(0),
                connections: AtomicU64::new(0),
            })
            .uplink
            .fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_outbound_downlink(&self, tag: &str, bytes: u64) {
        self.record_downlink(bytes);

        self.by_outbound
            .entry(tag.to_string())
            .or_insert_with(|| OutboundStats {
                uplink: AtomicU64::new(0),
                downlink: AtomicU64::new(0),
                connections: AtomicU64::new(0),
            })
            .downlink
            .fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_outbound_connection(&self, tag: &str) {
        self.by_outbound
            .entry(tag.to_string())
            .or_insert_with(|| OutboundStats {
                uplink: AtomicU64::new(0),
                downlink: AtomicU64::new(0),
                connections: AtomicU64::new(0),
            })
            .connections
            .fetch_add(1, Ordering::Relaxed);
    }

    /// 记录入站流量
    pub fn record_inbound_uplink(&self, tag: &str, bytes: u64) {
        self.by_inbound
            .entry(tag.to_string())
            .or_insert_with(|| InboundStats {
                uplink: AtomicU64::new(0),
                downlink: AtomicU64::new(0),
                connections: AtomicU64::new(0),
            })
            .uplink
            .fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_inbound_downlink(&self, tag: &str, bytes: u64) {
        self.by_inbound
            .entry(tag.to_string())
            .or_insert_with(|| InboundStats {
                uplink: AtomicU64::new(0),
                downlink: AtomicU64::new(0),
                connections: AtomicU64::new(0),
            })
            .downlink
            .fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn record_inbound_connection(&self, tag: &str) {
        self.by_inbound
            .entry(tag.to_string())
            .or_insert_with(|| InboundStats {
                uplink: AtomicU64::new(0),
                downlink: AtomicU64::new(0),
                connections: AtomicU64::new(0),
            })
            .connections
            .fetch_add(1, Ordering::Relaxed);
    }

    /// 获取全局统计
    pub fn global_stats(&self) -> GlobalStats {
        GlobalStats {
            uplink: self.global_uplink.load(Ordering::Relaxed),
            downlink: self.global_downlink.load(Ordering::Relaxed),
        }
    }

    /// 获取出站统计
    pub fn outbound_stats(&self, tag: &str) -> Option<OutboundStatsSnapshot> {
        self.by_outbound.get(tag).map(|stats| OutboundStatsSnapshot {
            tag: tag.to_string(),
            uplink: stats.uplink.load(Ordering::Relaxed),
            downlink: stats.downlink.load(Ordering::Relaxed),
            connections: stats.connections.load(Ordering::Relaxed),
        })
    }

    /// 获取所有出站统计
    pub fn all_outbound_stats(&self) -> Vec<OutboundStatsSnapshot> {
        self.by_outbound
            .iter()
            .map(|entry| OutboundStatsSnapshot {
                tag: entry.key().clone(),
                uplink: entry.value().uplink.load(Ordering::Relaxed),
                downlink: entry.value().downlink.load(Ordering::Relaxed),
                connections: entry.value().connections.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// 获取入站统计
    pub fn inbound_stats(&self, tag: &str) -> Option<InboundStatsSnapshot> {
        self.by_inbound.get(tag).map(|stats| InboundStatsSnapshot {
            tag: tag.to_string(),
            uplink: stats.uplink.load(Ordering::Relaxed),
            downlink: stats.downlink.load(Ordering::Relaxed),
            connections: stats.connections.load(Ordering::Relaxed),
        })
    }

    /// 获取所有入站统计
    pub fn all_inbound_stats(&self) -> Vec<InboundStatsSnapshot> {
        self.by_inbound
            .iter()
            .map(|entry| InboundStatsSnapshot {
                tag: entry.key().clone(),
                uplink: entry.value().uplink.load(Ordering::Relaxed),
                downlink: entry.value().downlink.load(Ordering::Relaxed),
                connections: entry.value().connections.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// 重置统计
    pub fn reset(&self) {
        self.global_uplink.store(0, Ordering::Relaxed);
        self.global_downlink.store(0, Ordering::Relaxed);
        self.by_outbound.clear();
        self.by_inbound.clear();
    }
}

impl Default for TrafficStats {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GlobalStats {
    pub uplink: u64,
    pub downlink: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OutboundStatsSnapshot {
    pub tag: String,
    pub uplink: u64,
    pub downlink: u64,
    pub connections: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InboundStatsSnapshot {
    pub tag: String,
    pub uplink: u64,
    pub downlink: u64,
    pub connections: u64,
}

/// 流量统计包装器
pub struct StatsCounter<T> {
    inner: T,
    stats: Arc<TrafficStats>,
    tag: String,
    is_outbound: bool,
}

impl<T> StatsCounter<T> {
    pub fn new_outbound(inner: T, stats: Arc<TrafficStats>, tag: String) -> Self {
        stats.record_outbound_connection(&tag);
        Self {
            inner,
            stats,
            tag,
            is_outbound: true,
        }
    }

    pub fn new_inbound(inner: T, stats: Arc<TrafficStats>, tag: String) -> Self {
        stats.record_inbound_connection(&tag);
        Self {
            inner,
            stats,
            tag,
            is_outbound: false,
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for StatsCounter<T> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = std::pin::Pin::new(&mut self.inner).poll_read(cx, buf);
        let after = buf.filled().len();
        let bytes_read = (after - before) as u64;

        if bytes_read > 0 {
            if self.is_outbound {
                self.stats.record_outbound_downlink(&self.tag, bytes_read);
            } else {
                self.stats.record_inbound_downlink(&self.tag, bytes_read);
            }
        }

        result
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for StatsCounter<T> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let result = std::pin::Pin::new(&mut self.inner).poll_write(cx, buf);

        if let std::task::Poll::Ready(Ok(n)) = result {
            if self.is_outbound {
                self.stats.record_outbound_uplink(&self.tag, n as u64);
            } else {
                self.stats.record_inbound_uplink(&self.tag, n as u64);
            }
        }

        result
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

use tokio::io::{AsyncRead, AsyncWrite};
