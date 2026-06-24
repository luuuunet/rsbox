//! Runtime handles passed into supplementary services.

use crate::OutboundController;
use rsb_config::Options;
use rsb_core::SharedConnectionManager;
use rsb_dns::DnsRouter;
use std::sync::Arc;

#[derive(Clone)]
pub struct ServiceContext {
    pub options: Arc<Options>,
    pub controller: Arc<OutboundController>,
    pub connections: SharedConnectionManager,
    pub dns: Arc<DnsRouter>,
}
