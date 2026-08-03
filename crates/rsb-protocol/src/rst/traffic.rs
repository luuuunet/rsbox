//! Traffic profiles for RST (mirrors RSQ).

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
            // RST targets video/streaming over Brutal TCP.
            return Self::Video;
        };
        Self::from_name(v)
    }

    pub fn from_name(v: &str) -> Self {
        match v.to_ascii_lowercase().as_str() {
            "raw" => Self::Raw,
            "browse" | "web" => Self::Browse,
            "balanced" => Self::Balanced,
            "video" | "stream" | "streaming" => Self::Video,
            // Unknown → video (RST is Brutal streaming oriented).
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

    pub fn read_chunk_size(self) -> usize {
        match self {
            Self::Raw => 256 * 1024,
            Self::Video => 64 * 1024,
            Self::Browse => 16 * 1024,
            Self::Balanced => 32 * 1024,
        }
    }

    /// Base keepalive interval (seconds); control loop adds 0–4s jitter.
    pub fn keepalive_jitter_base_secs(self) -> u64 {
        match self {
            Self::Raw => 20,
            Self::Video => 15,
            Self::Browse => 25,
            Self::Balanced => 20,
        }
    }
}
