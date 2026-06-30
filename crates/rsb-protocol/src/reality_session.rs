//! REALITY server-side SessionId verification (Xray-compatible).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use hkdf::Hkdf;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::utls::{
    client_hello_random, hello_layout, parse_client_hello_key_share, pick_client_tls13_cipher,
};

const DEFAULT_MAX_TIME_DIFF: u64 = 120;

pub struct VerifiedSession {
    pub auth_key: [u8; 32],
    pub client_hello: Vec<u8>,
    pub client_pub: [u8; 32],
    pub cipher: u16,
}

pub fn verify_reality_session(
    record: &[u8],
    private_key: &StaticSecret,
    short_ids: &[[u8; 8]],
    max_time_diff: u64,
) -> Result<VerifiedSession> {
    let layout = hello_layout(record).context("reality: client hello layout")?;
    let random = client_hello_random(record).context("reality: client random")?;
    let client_pub = parse_client_hello_key_share(record).context("reality: key share")?;
    let cipher = pick_client_tls13_cipher(record).context("reality: tls1.3 cipher")?;

    let shared = private_key.diffie_hellman(&PublicKey::from(client_pub));
    let mut auth_key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&random[..20]), shared.as_bytes())
        .expand(b"REALITY", &mut auth_key)
        .map_err(|_| anyhow::anyhow!("reality hkdf"))?;

    let sid_len = record[layout.session_id_offset - 1] as usize;
    anyhow::ensure!(sid_len == 32, "reality: unexpected session id length {sid_len}");
    let ciphertext = &record[layout.session_id_offset..layout.session_id_offset + sid_len];

    let mut aad = record[5..].to_vec();
    let sid_off = layout.session_id_offset - 5;
    aad[sid_off..sid_off + sid_len].fill(0);

    let cipher_aead = Aes256Gcm::new_from_slice(&auth_key).context("reality aead")?;
    let nonce = Nonce::from_slice(&random[20..32]);
    let plain = cipher_aead
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad: &aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("reality session decrypt failed"))?;

    anyhow::ensure!(plain.len() >= 16, "reality session plaintext too short");
    validate_session_plain(&plain, short_ids, max_time_diff)?;

        Ok(VerifiedSession {
        auth_key,
        client_hello: record.to_vec(),
        client_pub,
        cipher,
    })
}

fn validate_session_plain(plain: &[u8], short_ids: &[[u8; 8]], max_time_diff: u64) -> Result<()> {
    let max_diff = if max_time_diff == 0 {
        DEFAULT_MAX_TIME_DIFF
    } else {
        max_time_diff
    };
    let ts = u32::from_be_bytes(plain[4..8].try_into().unwrap()) as u64;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let diff = now.abs_diff(ts);
    anyhow::ensure!(diff <= max_diff, "reality timestamp out of range");

    let sid = &plain[8..16];
    let matched = short_ids.iter().any(|allowed| {
        allowed
            .iter()
            .zip(sid.iter())
            .all(|(a, b)| *a == 0 || a == b)
    });
    anyhow::ensure!(matched, "reality short_id mismatch");
    Ok(())
}

pub fn decode_reality_private_key(b64: &str) -> Result<StaticSecret> {
    use base64::Engine;
    let trimmed = b64.trim();
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(trimmed))?;
    anyhow::ensure!(bytes.len() == 32, "reality private_key length");
    let arr: [u8; 32] = bytes.try_into().map_err(|_| anyhow::anyhow!("bad key len"))?;
    Ok(StaticSecret::from(arr))
}

pub fn decode_short_ids(raw: &serde_json::Value) -> Result<Vec<[u8; 8]>> {
    let reality = raw
        .get("reality")
        .context("reality block")?;
    let mut out = Vec::new();
    if let Some(arr) = reality.get("short_id").and_then(|v| v.as_array()) {
        for v in arr {
            if let Some(hex) = v.as_str() {
                out.push(decode_short_id_hex(hex));
            }
        }
    } else if let Some(hex) = reality.get("short_id").and_then(|v| v.as_str()) {
        out.push(decode_short_id_hex(hex));
    }
    anyhow::ensure!(!out.is_empty(), "reality short_id required");
    Ok(out)
}

fn decode_short_id_hex(s: &str) -> [u8; 8] {
    let mut out = [0u8; 8];
    let bytes = decode_hex(s);
    out[..bytes.len().min(8)].copy_from_slice(&bytes[..bytes.len().min(8)]);
    out
}

fn decode_hex(s: &str) -> Vec<u8> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    (0..s.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&s[i..i + 2.min(s.len() - i)], 16).ok())
        .collect()
}
