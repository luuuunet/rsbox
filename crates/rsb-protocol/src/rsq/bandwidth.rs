//! RSQ bandwidth pacing — delegates to [`super::brutal::BrutalPacer`].

pub use super::brutal::{brutal_bps_from_mbps, BrutalPacer, BrutalWriter, DEFAULT_BRUTAL_MBPS};

use std::sync::Arc;

pub fn brutal_pacer_from_bps(bps: u64) -> Arc<BrutalPacer> {
    Arc::new(BrutalPacer::new(bps))
}

pub fn brutal_pacer_from_mbps(mbps: u32) -> Arc<BrutalPacer> {
    BrutalPacer::from_mbps(mbps)
}
