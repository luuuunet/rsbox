//! Runtime handles passed into supplementary services.

use crate::OutboundController;
use crate::reload::ConfigReload;
use rsb_config::Options;
use rsb_core::SharedConnectionManager;
use rsb_dns::DnsRouter;
use std::sync::{Arc, RwLock};

#[derive(Clone)]
pub struct ServiceContext {
    pub options: Arc<RwLock<Options>>,
    pub controller: Arc<OutboundController>,
    pub connections: SharedConnectionManager,
    pub dns: Arc<DnsRouter>,
    pub reload: Option<Arc<ConfigReload>>,
}

impl ServiceContext {
    pub fn options_snapshot(&self) -> Options {
        self.options
            .read()
            .map(|o| o.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }
}
