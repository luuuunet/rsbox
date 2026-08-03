//! Brutal helpers for RST (re-export RSQ Brutal pacing).

pub use crate::rsq::brutal::{brutal_bps_from_mbps, BrutalPacer, DEFAULT_BRUTAL_MBPS};

use std::sync::Arc;

pub fn brutal_pacer_from_bps(bps: u64) -> Arc<BrutalPacer> {
    Arc::new(BrutalPacer::new(bps))
}

pub fn brutal_pacer_from_mbps(mbps: u32) -> Arc<BrutalPacer> {
    BrutalPacer::from_mbps(mbps)
}
