//! Hot-reload user policy and inbound listeners without restarting the process.

use crate::build::BuildContext;
use crate::build_inbounds;
use anyhow::{Context, Result};
use rsb_config::Options;
use rsb_core::{Dialer, InboundManager, SharedConnectionManager};
use rsb_dns::DnsRouter;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct ConfigReload {
    inbound: Arc<tokio::sync::Mutex<InboundManager>>,
    dialer: Arc<Dialer>,
    connections: SharedConnectionManager,
    options: Arc<RwLock<Options>>,
    dns: Arc<DnsRouter>,
}

impl ConfigReload {
    pub fn new(
        inbound: Arc<tokio::sync::Mutex<InboundManager>>,
        dialer: Arc<Dialer>,
        connections: SharedConnectionManager,
        options: Arc<RwLock<Options>>,
        dns: Arc<DnsRouter>,
    ) -> Self {
        Self {
            inbound,
            dialer,
            connections,
            options,
            dns,
        }
    }

    /// Reload user limits/quotas from the current in-memory config.
    pub fn reload_users(&self) {
        if let Ok(opts) = self.options.read() {
            self.connections.reload_users(&opts);
        }
    }

    /// Apply a new config: refresh users, restart inbounds, update stored options.
    pub async fn reload(&self, options: Options) -> Result<()> {
        self.connections.reload_users(&options);
        let mut ctx = BuildContext::from_options(&options)?;
        ctx.dns = self.dns.clone();
        let mut inbound = self.inbound.lock().await;
        inbound.close_all().await.ok();
        let new_inbounds = build_inbounds(&options, ctx, self.dialer.clone())?;
        *inbound = InboundManager::new(new_inbounds);
        inbound.start_all().await.context("start inbounds after reload")?;
        if let Ok(mut opts) = self.options.write() {
            *opts = options;
        }
        tracing::info!("configuration reload completed");
        Ok(())
    }
}
