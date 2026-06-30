//! REALITY TLS client (Xray-compatible SessionId auth + uTLS ClientHello).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use base64::Engine;
use hkdf::Hkdf;
use serde_json::Value;
use sha2::{Digest, Sha256};
use x25519_dalek::PublicKey;

use crate::transport::TlsIo;
use crate::utls::{
    client_hello_random, generate_client_hello, hello_layout, ClientHelloKeys, Profile,
    UtlsTlsStream,
};

const XRAY_REALITY_VERSION: (u8, u8, u8) = (1, 8, 1);

fn reality_version(tls: &Value) -> (u8, u8, u8) {
    tls.get("reality")
        .and_then(|r| r.get("xver"))
        .and_then(|v| v.as_u64())
        .map(|v| (v as u8, 0, 0))
        .unwrap_or(XRAY_REALITY_VERSION)
}

pub fn is_reality(raw: Option<&Value>) -> bool {
    raw.and_then(|t| t.get("reality"))
        .and_then(|r| r.get("enabled"))
        .and_then(|v| v.as_bool())
        .unwrap_or(raw.and_then(|t| t.get("reality")).is_some())
}

pub async fn connect(
    server: &str,
    port: u16,
    tls: Option<&Value>,
    sni: Option<&str>,
) -> Result<TlsIo> {
    reality_connect(server, port, tls.context("reality tls")?, sni).await
}

pub(crate) struct RealityConfig {
    pub(crate) public_key: PublicKey,
    pub(crate) short_id: [u8; 8],
    pub(crate) server_name: String,
    pub(crate) fingerprint: Profile,
}

fn parse_reality(tls: &Value) -> Result<RealityConfig> {
    let reality = tls.get("reality").context("reality block")?;
    let pk_b64 = reality
        .get("public_key")
        .and_then(|v| v.as_str())
        .context("reality public_key")?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(pk_b64.trim())
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(pk_b64.trim()))?;
    anyhow::ensure!(bytes.len() == 32);
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&bytes);
    let short_id_hex = reality
        .get("short_id")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let server_name = tls
        .get("server_name")
        .and_then(|v| v.as_str())
        .or_else(|| {
            reality
                .get("server_name")
                .and_then(|v| v.as_str())
        })
        .or_else(|| {
            reality
                .get("server_names")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
        })
        .unwrap_or("www.microsoft.com")
        .to_string();
    let fp = reality
        .get("fingerprint")
        .and_then(|v| v.as_str())
        .or_else(|| {
            tls.get("utls")
                .and_then(|u| u.get("fingerprint"))
                .and_then(|v| v.as_str())
        })
        .and_then(Profile::parse)
        .unwrap_or(Profile::Chrome);
    Ok(RealityConfig {
        public_key: PublicKey::from(pk),
        short_id: decode_short_id(short_id_hex),
        server_name,
        fingerprint: fp,
    })
}

async fn reality_connect(server: &str, port: u16, tls: &Value, sni: Option<&str>) -> Result<TlsIo> {
    let cfg = parse_reality(tls)?;
    let server_name = sni.map(str::to_string).unwrap_or(cfg.server_name.clone());
    let keys = generate_client_hello(cfg.fingerprint, &server_name);
    let (hello, auth_key) = patch_reality_session(&keys, &cfg, tls)?;
    let tcp = crate::transport::tcp_connect(server, port).await?;
    let stream = UtlsTlsStream::connect_reality(tcp, &hello, keys.secret, auth_key).await?;
    Ok(TlsIo::Utls(stream))
}

fn decode_short_id(s: &str) -> [u8; 8] {
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

/// Xray REALITY SessionId auth (HKDF + AES-GCM over first 16 bytes).
pub(crate) fn patch_reality_session(
    keys: &ClientHelloKeys,
    cfg: &RealityConfig,
    tls: &Value,
) -> Result<(Vec<u8>, [u8; 32])> {
    let ver = reality_version(tls);
    let mut hello = keys.hello.clone();
    let layout = hello_layout(&hello).context("reality hello layout")?;
    let random = *client_hello_random(&hello).context("reality random")?;

    let mut session_plain = [0u8; 32];
    session_plain[0] = ver.0;
    session_plain[1] = ver.1;
    session_plain[2] = ver.2;
    session_plain[3] = 0;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as u32;
    session_plain[4..8].copy_from_slice(&ts.to_be_bytes());
    session_plain[8..16].copy_from_slice(&cfg.short_id);

    // Xray: AAD uses ClientHello with SessionId field zeroed (Raw[39:71] = 0 before Seal).
    hello[layout.session_id_offset..layout.session_id_offset + 32].fill(0);
    let aad = hello[5..].to_vec();

    let shared = keys.secret.diffie_hellman(&cfg.public_key);
    let mut auth_key = [0u8; 32];
    Hkdf::<Sha256>::new(Some(&random[..20]), shared.as_bytes())
        .expand(b"REALITY", &mut auth_key)
        .map_err(|_| anyhow::anyhow!("reality hkdf"))?;

    let cipher = Aes256Gcm::new_from_slice(&auth_key).context("reality aead")?;
    let nonce = Nonce::from_slice(&random[20..32]);
    let sealed = cipher
        .encrypt(
            nonce,
            Payload {
                msg: &session_plain[..16],
                aad: &aad,
            },
        )
        .map_err(|e| anyhow::anyhow!("reality seal: {e}"))?;
    anyhow::ensure!(sealed.len() == 32, "reality session id length");
    hello[layout.session_id_offset..layout.session_id_offset + 32].copy_from_slice(&sealed);
    Ok((hello, auth_key))
}

/// Verify REALITY ed25519 certificate signature (Xray `VerifyPeerCertificate`).
pub use crate::reality_cert::{parse_reality_cert_parts, verify_reality_cert};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utls::Profile;

    #[test]
    fn reality_session_id_sealed_to_32_bytes() {
        let cfg = RealityConfig {
            public_key: PublicKey::from([1u8; 32]),
            short_id: [0xab; 8],
            server_name: "example.com".into(),
            fingerprint: Profile::Chrome,
        };
        let keys = generate_client_hello(Profile::Chrome, "example.com");
        let (hello, auth_key) =
            patch_reality_session(&keys, &cfg, &serde_json::json!({"reality": {}})).unwrap();
        assert_eq!(auth_key.len(), 32);
        let layout = hello_layout(&hello).unwrap();
        assert_eq!(
            hello[layout.session_id_offset..layout.session_id_offset + 32].len(),
            32
        );
        assert_ne!(hello[layout.session_id_offset], cfg.short_id[0]);
    }

    #[test]
    fn reality_hello_has_session_id_field() {
        let keys = generate_client_hello(Profile::Chrome, "example.com");
        let layout = hello_layout(&keys.hello).unwrap();
        assert!(layout.session_id_offset + 32 <= keys.hello.len());
    }

    #[test]
    fn dump_reality_hello() {
        use base64::Engine;
        let mut pk = [0u8; 32];
        pk.copy_from_slice(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode("VdvA1In4Po7ugbQHYIm518Vw5u72SFokjyTY4XwByRw")
                .unwrap(),
        );
        let cfg = RealityConfig {
            public_key: PublicKey::from(pk),
            short_id: decode_short_id("a1b2c3d4"),
            server_name: "www.cloudflare.com".into(),
            fingerprint: Profile::Chrome,
        };
        let keys = generate_client_hello(Profile::Chrome, "www.cloudflare.com");
        let (hello, _) = patch_reality_session(
            &keys,
            &cfg,
            &serde_json::json!({"reality": {}}),
        )
        .unwrap();
        println!(
            "hello_b64={}",
            base64::engine::general_purpose::STANDARD.encode(&hello)
        );
        println!(
            "secret_b64={}",
            base64::engine::general_purpose::STANDARD.encode(keys.secret.as_bytes())
        );
    }
}
