//! Cascade fallback outbound: top-to-bottom, sticky, never fail back.
//!
//! Dial starts at the active child index and only tries that index and below.
//! On success further down the list, stick there for the rest of the session.
//! Earlier (higher) nodes are never retried until the process restarts / config reload.

use crate::duration::parse_duration_str;
use anyhow::Result;
use async_trait::async_trait;
use rsb_core::{BoxError, Network, Outbound, ProxyConn, ProxyUdpSocket, SharedOutboundManager};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_DIAL_TIMEOUT: Duration = Duration::from_secs(5);

/// Pure policy used by dial path (unit-testable).
#[derive(Debug, Clone)]
pub struct FallbackPolicy {
    pub outbounds: Vec<String>,
    pub dial_timeout: Duration,
    pub warm_children: bool,
}

impl FallbackPolicy {
    pub fn from_raw(raw: &Value) -> Result<Self> {
        let outbounds: Vec<String> = raw
            .get("outbounds")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        anyhow::ensure!(!outbounds.is_empty(), "fallback: outbounds required");

        Ok(Self {
            outbounds,
            dial_timeout: raw
                .get("dial_timeout")
                .and_then(|v| v.as_str())
                .and_then(parse_duration_str)
                .unwrap_or(DEFAULT_DIAL_TIMEOUT),
            warm_children: raw
                .get("warm_children")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
        })
    }

    pub fn primary_tag(&self) -> &str {
        &self.outbounds[0]
    }
}

struct FallbackState {
    policy: FallbackPolicy,
    active_idx: AtomicUsize,
}

impl FallbackState {
    fn new(policy: FallbackPolicy) -> Self {
        Self {
            policy,
            active_idx: AtomicUsize::new(0),
        }
    }

    fn active_idx(&self) -> usize {
        self.active_idx.load(Ordering::SeqCst)
    }

    fn set_active(&self, idx: usize) {
        self.active_idx.store(idx, Ordering::SeqCst);
    }

    fn selected_tag(&self) -> &str {
        let idx = self.active_idx();
        self.policy
            .outbounds
            .get(idx)
            .map(String::as_str)
            .unwrap_or(self.policy.primary_tag())
    }

    /// Stick further down the cascade; never move upward.
    fn on_failover(&self, success_idx: usize) {
        let cur = self.active_idx();
        if success_idx <= cur {
            return;
        }
        self.set_active(success_idx);
        tracing::info!(
            active = %self.policy.outbounds[success_idx],
            from_idx = cur,
            to_idx = success_idx,
            "fallback: cascaded down (no failback)"
        );
    }

    /// Only active and nodes below it (top → bottom). Never retry earlier nodes.
    fn dial_order(&self) -> Vec<usize> {
        let n = self.policy.outbounds.len();
        if n == 0 {
            return Vec::new();
        }
        let active = self.active_idx().min(n - 1);
        (active..n).collect()
    }
}

/// Clash-api style control surface.
#[derive(Clone)]
pub struct FallbackControl {
    tag: String,
    state: Arc<FallbackState>,
}

impl FallbackControl {
    pub fn tag(&self) -> &str {
        &self.tag
    }

    pub fn selected(&self) -> String {
        self.state.selected_tag().to_string()
    }

    pub fn outbounds(&self) -> &[String] {
        &self.state.policy.outbounds
    }
}

pub struct FallbackOutbound {
    tag: String,
    state: Arc<FallbackState>,
    shared: Arc<SharedOutboundManager>,
}

impl FallbackOutbound {
    pub fn new(tag: String, raw: Value, shared: Arc<SharedOutboundManager>) -> Result<Self> {
        let policy = FallbackPolicy::from_raw(&raw)?;
        let state = Arc::new(FallbackState::new(policy));
        Ok(Self {
            tag,
            state,
            shared,
        })
    }

    pub fn control(&self) -> FallbackControl {
        FallbackControl {
            tag: self.tag.clone(),
            state: self.state.clone(),
        }
    }

    async fn dial_tcp_with_failover(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        let mgr = self.shared.get()?;
        let order = self.state.dial_order();
        let mut last_err: Option<BoxError> = None;
        let active_before = self.state.active_idx();

        for idx in order {
            let tag = &self.state.policy.outbounds[idx];
            let ob = match mgr.get(tag) {
                Ok(o) => o,
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            };
            let timed = tokio::time::timeout(
                self.state.policy.dial_timeout,
                ob.dial_tcp(destination, domain),
            )
            .await;
            match timed {
                Ok(Ok(conn)) => {
                    if idx > active_before {
                        self.state.on_failover(idx);
                    }
                    return Ok(conn);
                }
                Ok(Err(e)) => last_err = Some(e),
                Err(_) => {
                    last_err = Some(
                        anyhow::anyhow!(
                            "fallback: dial timeout {}ms on `{tag}`",
                            self.state.policy.dial_timeout.as_millis()
                        )
                        .into(),
                    );
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("fallback: all children failed").into()))
    }

    async fn dial_udp_with_failover(
        &self,
        destination: SocketAddr,
    ) -> Result<ProxyUdpSocket, BoxError> {
        let mgr = self.shared.get()?;
        let order = self.state.dial_order();
        let mut last_err: Option<BoxError> = None;
        let active_before = self.state.active_idx();

        for idx in order {
            let tag = &self.state.policy.outbounds[idx];
            let ob = match mgr.get(tag) {
                Ok(o) => o,
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            };
            let timed =
                tokio::time::timeout(self.state.policy.dial_timeout, ob.dial_udp(destination)).await;
            match timed {
                Ok(Ok(sock)) => {
                    if idx > active_before {
                        self.state.on_failover(idx);
                    }
                    return Ok(sock);
                }
                Ok(Err(e)) => last_err = Some(e),
                Err(_) => {
                    last_err = Some(
                        anyhow::anyhow!(
                            "fallback: udp dial timeout {}ms on `{tag}`",
                            self.state.policy.dial_timeout.as_millis()
                        )
                        .into(),
                    );
                }
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("fallback: all udp children failed").into()))
    }
}

#[async_trait]
impl Outbound for FallbackOutbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_FALLBACK
    }
    fn networks(&self) -> &[Network] {
        &[Network::Tcp, Network::Udp]
    }
    async fn dial_tcp(
        &self,
        destination: SocketAddr,
        domain: Option<&str>,
    ) -> Result<ProxyConn, BoxError> {
        self.dial_tcp_with_failover(destination, domain).await
    }
    async fn dial_udp(&self, destination: SocketAddr) -> Result<ProxyUdpSocket, BoxError> {
        self.dial_udp_with_failover(destination).await
    }
    async fn close(&self) -> Result<(), BoxError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_three() -> FallbackPolicy {
        FallbackPolicy {
            outbounds: vec!["a".into(), "b".into(), "c".into()],
            dial_timeout: Duration::from_millis(50),
            warm_children: true,
        }
    }

    #[test]
    fn dial_order_starts_at_active_and_goes_down() {
        let st = FallbackState::new(policy_three());
        assert_eq!(st.dial_order(), vec![0, 1, 2]);
        st.set_active(1);
        assert_eq!(st.dial_order(), vec![1, 2]);
        st.set_active(2);
        assert_eq!(st.dial_order(), vec![2]);
    }

    #[test]
    fn failover_only_moves_down() {
        let st = FallbackState::new(policy_three());
        st.on_failover(1);
        assert_eq!(st.selected_tag(), "b");
        st.on_failover(0); // must not go back
        assert_eq!(st.selected_tag(), "b");
        st.on_failover(2);
        assert_eq!(st.selected_tag(), "c");
        st.on_failover(1); // must not go back
        assert_eq!(st.selected_tag(), "c");
    }

    #[test]
    fn from_raw_parses_ms_and_ignores_legacy_failback() {
        let raw = serde_json::json!({
            "outbounds": ["a", "b"],
            "dial_timeout": "120ms",
            "cooldown": "90s",
            "failback_successes": 3,
            "failback_max_rtt_ms": 800
        });
        let p = FallbackPolicy::from_raw(&raw).unwrap();
        assert_eq!(p.dial_timeout, Duration::from_millis(120));
        assert_eq!(p.outbounds, vec!["a", "b"]);
    }
}
