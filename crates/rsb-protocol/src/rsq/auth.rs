//! RSQ binary authentication (control stream).

use super::protocol::{self, write_varint, DecodedFrame, FRAME_AUTH_OK, FRAME_AUTH_REQ};
use anyhow::{bail, Result};
use bytes::{Buf, BufMut, BytesMut};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

pub const MBPS_TO_BPS: u64 = 125_000;
pub const AUTH_MAX_SKEW_SECS: u64 = 120;
const REPLAY_TTL_SECS: u64 = AUTH_MAX_SKEW_SECS;

/// Tracks recent AUTH client_random values to reject replays.
#[derive(Default)]
pub struct AuthReplayCache {
    seen: Mutex<HashMap<[u8; 32], Instant>>,
}

impl AuthReplayCache {
    pub fn check_and_insert(&self, nonce: [u8; 32]) -> Result<()> {
        let now = Instant::now();
        let mut guard = self.seen.lock().expect("auth replay lock");
        guard.retain(|_, at| now.duration_since(*at).as_secs() <= REPLAY_TTL_SECS);
        if guard.contains_key(&nonce) {
            bail!("auth replay detected");
        }
        guard.insert(nonce, now);
        Ok(())
    }
}

pub fn auth_key(password: &str) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(password.as_bytes()).expect("hmac key");
    mac.update(b"rsq-auth-v1");
    let out = mac.finalize().into_bytes();
    let mut key = [0u8; 32];
    key.copy_from_slice(&out[..32]);
    key
}

pub fn encode_auth_req(
    password: &str,
    down_mbps: u32,
    up_mbps: u32,
    profile: super::traffic::TrafficProfile,
) -> BytesMut {
    let key = auth_key(password);
    let mut client_random = [0u8; 32];
    rand::RngCore::fill_bytes(&mut rand::rng(), &mut client_random);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let rx_bps = if down_mbps == 0 {
        0u64
    } else {
        down_mbps as u64 * MBPS_TO_BPS
    };
    let up_bps = if up_mbps == 0 {
        profile.default_up_bps()
    } else {
        up_mbps as u64 * MBPS_TO_BPS
    };
    let profile_id = profile as u8;

    let mut body = BytesMut::new();
    body.put_u8(1);
    body.put_slice(&client_random);
    body.put_u64(timestamp);
    write_varint(&mut body, rx_bps);
    write_varint(&mut body, up_bps);
    body.put_u8(profile_id);

    let proof = compute_proof(&key, &body);
    body.put_slice(&proof);

    protocol::encode_frame(FRAME_AUTH_REQ, 0, &body, protocol::random_pad_len(32, 128))
}

pub struct AuthOk {
    pub session_id: u32,
    pub server_rx_bps: u64,
    pub udp_enabled: bool,
}

pub struct AuthClientCaps {
    pub client_rx_bps: u64,
    pub client_up_bps: u64,
    pub profile: super::traffic::TrafficProfile,
}

pub fn verify_auth_req(
    frame: &DecodedFrame,
    passwords: &[String],
    replay: Option<&AuthReplayCache>,
) -> Result<(String, AuthClientCaps)> {
    if frame.frame_type != FRAME_AUTH_REQ {
        bail!("expected AUTH_REQ");
    }
    let body = &frame.payload;
    if body.len() < 32 + 8 + 1 + 32 + 3 {
        bail!("auth req too short");
    }
    let mut cursor = &body[..];
    let version = cursor.get_u8();
    if version != 1 {
        bail!("unsupported auth version");
    }
    let mut client_random = [0u8; 32];
    cursor.copy_to_slice(&mut client_random);
    let timestamp = cursor.get_u64();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if timestamp.abs_diff(now) > AUTH_MAX_SKEW_SECS {
        bail!("auth timestamp skew");
    }
    let rx_bps = protocol::read_varint(&mut cursor)?;
    let up_bps = protocol::read_varint(&mut cursor)?;
    let profile_id = cursor.get_u8();
    let profile = super::traffic::TrafficProfile::from_id(profile_id);
    if cursor.remaining() < 32 {
        bail!("missing auth proof");
    }
    let proof_start = body.len() - 32;
    let signed = &body[..proof_start];
    let proof = &body[proof_start..];

    for password in passwords {
        let key = auth_key(password);
        let expected = compute_proof(&key, signed);
        if expected == proof {
            if let Some(cache) = replay {
                cache.check_and_insert(client_random)?;
            }
            return Ok((
                password.clone(),
                AuthClientCaps {
                    client_rx_bps: rx_bps,
                    client_up_bps: up_bps,
                    profile,
                },
            ));
        }
    }
    bail!("auth failed")
}

pub fn encode_auth_ok(session_id: u32, server_rx_bps: u64, udp_enabled: bool) -> BytesMut {
    let mut body = BytesMut::new();
    body.put_u8(1);
    body.put_u32(session_id);
    write_varint(&mut body, server_rx_bps);
    body.put_u8(u8::from(udp_enabled));
    protocol::encode_frame(FRAME_AUTH_OK, 0, &body, protocol::random_pad_len(16, 64))
}

pub fn decode_auth_ok(frame: &DecodedFrame) -> Result<AuthOk> {
    if frame.frame_type != FRAME_AUTH_OK {
        bail!("expected AUTH_OK");
    }
    let body = &frame.payload;
    if body.first().copied() != Some(1) {
        bail!("unsupported auth ok version");
    }
    let mut cursor = &body[1..];
    if cursor.remaining() < 4 {
        bail!("auth ok too short");
    }
    let session_id = cursor.get_u32();
    let server_rx_bps = protocol::read_varint(&mut cursor)?;
    let udp_enabled = cursor.first().copied().unwrap_or(0) != 0;
    Ok(AuthOk {
        session_id,
        server_rx_bps,
        udp_enabled,
    })
}

fn compute_proof(key: &[u8; 32], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("hmac key");
    mac.update(data);
    let out = mac.finalize().into_bytes();
    let mut proof = [0u8; 32];
    proof.copy_from_slice(&out[..32]);
    proof
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rsq::traffic::TrafficProfile;

    #[test]
    fn auth_multi_user() {
        let passwords = vec!["user-a".into(), "user-b".into()];
        for expected in ["user-a", "user-b"] {
            let req = encode_auth_req(expected, 100, 50, TrafficProfile::Video);
            let frame = protocol::try_decode_frame(&req).unwrap().unwrap();
            let (pass, caps) = verify_auth_req(&frame, &passwords, None).unwrap();
            assert_eq!(pass, expected);
            assert_eq!(caps.client_rx_bps, 100 * MBPS_TO_BPS);
        }
        let bad = encode_auth_req("wrong-pass", 100, 50, TrafficProfile::Video);
        let frame = protocol::try_decode_frame(&bad).unwrap().unwrap();
        assert!(verify_auth_req(&frame, &passwords, None).is_err());
    }

    #[test]
    fn auth_replay_rejected() {
        let passwords = vec!["user-a".into()];
        let cache = AuthReplayCache::default();
        let req = encode_auth_req("user-a", 100, 50, TrafficProfile::Video);
        let frame = protocol::try_decode_frame(&req).unwrap().unwrap();
        assert!(verify_auth_req(&frame, &passwords, Some(&cache)).is_ok());
        assert!(verify_auth_req(&frame, &passwords, Some(&cache)).is_err());
    }

    #[test]
    fn auth_failed_does_not_block_replay_for_later_success() {
        let passwords = vec!["good".into()];
        let cache = AuthReplayCache::default();
        let bad = encode_auth_req("wrong", 100, 50, TrafficProfile::Video);
        let bad_frame = protocol::try_decode_frame(&bad).unwrap().unwrap();
        assert!(verify_auth_req(&bad_frame, &passwords, Some(&cache)).is_err());
        let ok = encode_auth_req("good", 100, 50, TrafficProfile::Video);
        let ok_frame = protocol::try_decode_frame(&ok).unwrap().unwrap();
        assert!(verify_auth_req(&ok_frame, &passwords, Some(&cache)).is_ok());
    }

    #[test]
    fn auth_ok_roundtrip() {
        let ok = encode_auth_ok(42, 100 * MBPS_TO_BPS, true);
        let frame = protocol::try_decode_frame(&ok).unwrap().unwrap();
        let parsed = decode_auth_ok(&frame).unwrap();
        assert_eq!(parsed.session_id, 42);
        assert_eq!(parsed.server_rx_bps, 100 * MBPS_TO_BPS);
        assert!(parsed.udp_enabled);
    }
}
