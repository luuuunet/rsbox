//! Traffic realism profiles for RSQ.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrafficProfile {
    Raw = 0,
    Video = 1,
    Browse = 2,
    Balanced = 3,
}

impl TrafficProfile {
    pub fn parse(raw: Option<&Value>) -> Self {
        let Some(v) = raw.and_then(|x| x.as_str()) else {
            return Self::Video;
        };
        Self::from_name(v)
    }

    pub fn from_name(v: &str) -> Self {
        match v.to_ascii_lowercase().as_str() {
            "raw" => Self::Raw,
            "browse" => Self::Browse,
            "balanced" => Self::Balanced,
            _ => Self::Video,
        }
    }

    pub fn from_id(id: u8) -> Self {
        match id {
            0 => Self::Raw,
            1 => Self::Video,
            2 => Self::Browse,
            3 => Self::Balanced,
            _ => Self::Video,
        }
    }

    pub fn default_up_bps(self) -> u64 {
        match self {
            Self::Raw => 0,
            Self::Video => 50 * super::auth::MBPS_TO_BPS,
            Self::Browse => 10 * super::auth::MBPS_TO_BPS,
            Self::Balanced => 80 * super::auth::MBPS_TO_BPS,
        }
    }

    pub fn keepalive_jitter_base_secs(self) -> u64 {
        match self {
            Self::Browse => 25,
            _ => 20,
        }
    }

    /// Preferred read/copy chunk size — shapes burstiness on the wire.
    pub fn read_chunk_size(self) -> usize {
        match self {
            Self::Raw => 64 * 1024,
            Self::Video => 32 * 1024,
            Self::Browse => 8 * 1024,
            Self::Balanced => 16 * 1024,
        }
    }

    /// Whether relay copy loops should insert artificial inter-chunk delays.
    /// Only browse mimics human click-scroll pauses; throughput profiles stay unpaced.
    pub fn pace_relay_copy(self) -> bool {
        matches!(self, Self::Browse)
    }

    /// Optional idle gap between chunks (browse = bursty).
    pub fn inter_chunk_delay(self) -> std::time::Duration {
        let max_ms = match self {
            Self::Raw | Self::Video | Self::Balanced => 0,
            Self::Browse => 12,
        };
        if max_ms == 0 {
            return std::time::Duration::ZERO;
        }
        let jitter = rand::random::<u64>() % (max_ms + 1);
        std::time::Duration::from_millis(jitter)
    }
}

pub fn jitter_duration(base_secs: u64) -> std::time::Duration {
    let jitter = (rand::random::<i64>() % ((base_secs as i64 * 30) / 100 + 1)).unsigned_abs();
    std::time::Duration::from_secs(base_secs + jitter.max(1))
}

pub async fn paced_copy_chunk(profile: TrafficProfile) {
    let delay = profile.inter_chunk_delay();
    if !delay.is_zero() {
        tokio::time::sleep(delay).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_chunk_sizes_ordered() {
        assert!(TrafficProfile::Browse.read_chunk_size() < TrafficProfile::Video.read_chunk_size());
        assert!(TrafficProfile::Video.read_chunk_size() <= TrafficProfile::Raw.read_chunk_size());
    }

    #[test]
    fn only_browse_paces_relay() {
        assert!(!TrafficProfile::Raw.pace_relay_copy());
        assert!(!TrafficProfile::Video.pace_relay_copy());
        assert!(!TrafficProfile::Balanced.pace_relay_copy());
        assert!(TrafficProfile::Browse.pace_relay_copy());
    }
}
