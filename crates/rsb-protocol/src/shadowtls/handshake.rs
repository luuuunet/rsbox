//! Plain rustls client handshake on an existing stream (session discarded after).

use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use rand::RngCore;
use rustls::pki_types::ServerName;
use rustls::ClientConnection;
use sha1::Sha1;
use std::io::Cursor;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::shadowtls::constants::*;

pub async fn handshake<S>(mut stream: S, cfg: Arc<rustls::ClientConfig>, sni: &str) -> Result<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let name = ServerName::try_from(sni)
        .map_err(|_| anyhow::anyhow!("invalid sni: {sni}"))?
        .to_owned();
    let mut conn = ClientConnection::new(cfg, name).context("tls client connection")?;
    run_handshake(&mut stream, &mut conn).await?;
    Ok(stream)
}

/// ShadowTLS v3: rustls handshake with in-band session id HMAC (transcript-safe).
pub async fn handshake_v3<S>(
    mut stream: S,
    cfg: Arc<rustls::ClientConfig>,
    sni: &str,
    password: &str,
) -> Result<S>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let name = ServerName::try_from(sni)
        .map_err(|_| anyhow::anyhow!("invalid sni: {sni}"))?
        .to_owned();
    let password = password.to_string();
    let mut conn = ClientConnection::new_with_session_id_generator(
        cfg,
        name,
        Arc::new(move |client_hello| generate_v3_session_id(&password, client_hello)),
    )
    .context("shadowtls v3 tls client connection")?;
    run_handshake(&mut stream, &mut conn).await?;
    Ok(stream)
}

async fn run_handshake<S>(stream: &mut S, conn: &mut ClientConnection) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;

    loop {
        while conn.wants_write() {
            let mut tls_out = Vec::new();
            while conn.wants_write() {
                conn.write_tls(&mut tls_out)
                    .context("encode tls handshake write")?;
            }
            if !tls_out.is_empty() {
                stream.write_all(&tls_out).await?;
                stream.flush().await?;
            }
        }

        if !conn.is_handshaking() {
            break;
        }

        if conn.wants_read() {
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                anyhow::bail!("tls handshake eof");
            }
            let mut cursor = Cursor::new(&buf[..n]);
            while cursor.position() < n as u64 {
                let read = conn.read_tls(&mut cursor).context("read tls handshake")?;
                if read == 0 {
                    break;
                }
            }
        }

        conn.process_new_packets()
            .map_err(|e| anyhow::anyhow!("process tls packets: {e}"))?;
    }
    Ok(())
}

/// Build a signed ShadowTLS v3 session id from a ClientHello handshake message.
fn generate_v3_session_id(password: &str, client_hello: &[u8]) -> [u8; 32] {
    const SESSION_ID_START: usize = 1 + 3 + 2 + TLS_RANDOM_SIZE + 1;
    let mut session_id = [0u8; TLS_SESSION_ID_SIZE];
    if client_hello.len() < SESSION_ID_START + TLS_SESSION_ID_SIZE {
        return session_id;
    }
    rand::rng().fill_bytes(&mut session_id[..TLS_SESSION_ID_SIZE - HMAC_SIZE_V3]);
    let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(password.as_bytes()) else {
        return session_id;
    };
    mac.update(&client_hello[..SESSION_ID_START]);
    mac.update(&session_id);
    mac.update(&client_hello[SESSION_ID_START + TLS_SESSION_ID_SIZE..]);
    let sum = mac.finalize().into_bytes();
    session_id[TLS_SESSION_ID_SIZE - HMAC_SIZE_V3..].copy_from_slice(&sum[..HMAC_SIZE_V3]);
    session_id
}

/// Capture the first TLS ClientHello record emitted by rustls.
pub fn capture_rustls_client_hello(cfg: &Arc<rustls::ClientConfig>, sni: &str) -> Result<Vec<u8>> {
    let name = ServerName::try_from(sni)
        .map_err(|_| anyhow::anyhow!("invalid sni: {sni}"))?
        .to_owned();
    let mut conn = ClientConnection::new(cfg.clone(), name).context("tls client connection")?;
    let mut tls_out = Vec::new();
    while conn.wants_write() {
        conn.write_tls(&mut tls_out)
            .context("encode rustls client hello")?;
        break;
    }
    anyhow::ensure!(!tls_out.is_empty(), "rustls produced empty ClientHello");
    Ok(tls_out)
}

/// Verify ShadowTLS v3 ClientHello session id HMAC against configured users.
pub fn verify_v3_client_hello(record: &[u8], users: &[(String, String)]) -> Option<String> {
    use crate::shadowtls::constants::{HANDSHAKE, TLS_HEADER_SIZE, TLS_SESSION_ID_SIZE};
    if record.len() < TLS_HEADER_SIZE + 1 || record[0] != HANDSHAKE {
        return None;
    }
    let hs = &record[TLS_HEADER_SIZE..];
    if hs.first()? != &1 {
        return None;
    }
    const SESSION_ID_START: usize = 1 + 3 + 2 + TLS_RANDOM_SIZE + 1;
    if hs.len() < SESSION_ID_START + TLS_SESSION_ID_SIZE {
        return None;
    }
    if hs[SESSION_ID_START - 1] != TLS_SESSION_ID_SIZE as u8 {
        return None;
    }
    let hmac_index = SESSION_ID_START + TLS_SESSION_ID_SIZE - HMAC_SIZE_V3;
    for (_, password) in users {
        let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(password.as_bytes()) else {
            continue;
        };
        mac.update(&hs[..hmac_index]);
        mac.update(&[0u8; HMAC_SIZE_V3]);
        mac.update(&hs[hmac_index + HMAC_SIZE_V3..]);
        let sum = mac.finalize().into_bytes();
        if sum[..HMAC_SIZE_V3] == hs[hmac_index..hmac_index + HMAC_SIZE_V3] {
            return Some(password.clone());
        }
    }
    None
}

/// Replace the x25519 key share in a ClientHello TLS record.
pub fn replace_client_hello_key_share(record: &mut [u8], public_key: &[u8; 32]) -> Result<()> {
    const MARKER: [u8; 4] = [0x00, 0x1d, 0x00, 0x20];
    for i in 0..record.len().saturating_sub(MARKER.len() + 32) {
        if record[i..i + 4] == MARKER {
            record[i + 4..i + 36].copy_from_slice(public_key);
            return Ok(());
        }
    }
    anyhow::bail!("client hello key share extension missing")
}

/// Patch the first ClientHello record found in a TLS write buffer.
pub fn patch_client_hello_buffer(buf: &mut Vec<u8>, password: &str) -> Result<bool> {
    use crate::shadowtls::constants::*;
    for offset in 0..buf.len().saturating_sub(TLS_HEADER_SIZE + 1) {
        if buf[offset] != HANDSHAKE {
            continue;
        }
        let len = u16::from_be_bytes([buf[offset + 3], buf[offset + 4]]) as usize;
        let end = offset + TLS_HEADER_SIZE + len;
        if end > buf.len() || len < 4 {
            continue;
        }
        if buf[offset + TLS_HEADER_SIZE] != 0x01 {
            continue;
        }
        let mut record = buf[offset..end].to_vec();
        patch_v3_session_id(&mut record, password)?;
        buf.splice(offset..end, record);
        return Ok(true);
    }
    Ok(false)
}

fn patch_v3_session_id(record: &mut Vec<u8>, password: &str) -> Result<()> {
    use crate::shadowtls::constants::*;
    use hmac::{Hmac, Mac};
    use rand::RngCore;
    use sha1::Sha1;

    if record.len() < 5 || record[0] != HANDSHAKE {
        return Ok(());
    }
    let hs = &record[5..];
    if hs.is_empty() || hs[0] != 0x01 {
        return Ok(());
    }
    // handshake type(1) + length(3) + version(2) + random(32) + session_id_len(1)
    const SESSION_ID_LEN_INDEX: usize = 1 + 3 + 2 + TLS_RANDOM_SIZE;
    const SESSION_ID_START: usize = SESSION_ID_LEN_INDEX + 1;
    if hs.len() <= SESSION_ID_LEN_INDEX {
        return Ok(());
    }

    let session_id_len_idx = 5 + SESSION_ID_LEN_INDEX;
    let sid_len = record[session_id_len_idx] as usize;
    if sid_len == 0 {
        record[session_id_len_idx] = TLS_SESSION_ID_SIZE as u8;
        let insert_at = session_id_len_idx + 1;
        record.splice(
            insert_at..insert_at,
            std::iter::repeat_n(0u8, TLS_SESSION_ID_SIZE),
        );

        let hs_len = u32::from_be_bytes([0, record[6], record[7], record[8]]) + 32;
        record[6] = ((hs_len >> 16) & 0xff) as u8;
        record[7] = ((hs_len >> 8) & 0xff) as u8;
        record[8] = (hs_len & 0xff) as u8;

        let rec_len = u16::from_be_bytes([record[3], record[4]]) + 32;
        record[3..5].copy_from_slice(&rec_len.to_be_bytes());
    } else if sid_len != TLS_SESSION_ID_SIZE {
        return Ok(());
    }

    let hs = &record[5..];
    if hs.len() < SESSION_ID_START + TLS_SESSION_ID_SIZE {
        return Ok(());
    }
    let session_id_start_absolute = 5 + SESSION_ID_START;
    let suffix_start = session_id_start_absolute + TLS_SESSION_ID_SIZE;

    let mut session_id = [0u8; TLS_SESSION_ID_SIZE];
    rand::rng().fill_bytes(&mut session_id[..TLS_SESSION_ID_SIZE - HMAC_SIZE_V3]);
    let mut mac =
        Hmac::<Sha1>::new_from_slice(password.as_bytes()).context("shadowtls v3 hmac key")?;
    mac.update(&record[5..session_id_start_absolute]);
    mac.update(&session_id);
    mac.update(&record[suffix_start..]);
    let sum = mac.finalize().into_bytes();
    session_id[TLS_SESSION_ID_SIZE - HMAC_SIZE_V3..].copy_from_slice(&sum[..HMAC_SIZE_V3]);
    record[session_id_start_absolute..suffix_start].copy_from_slice(&session_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shadowtls::constants::*;
    use crate::utls::{generate_shadowtls_client_hello, Profile};
    use hmac::{Hmac, Mac};
    use sha1::Sha1;

    fn verify_v3_client_hello(frame: &[u8], password: &str) -> bool {
        const SESSION_ID_LEN_INDEX: usize = TLS_HEADER_SIZE + 1 + 3 + 2 + TLS_RANDOM_SIZE;
        const HMAC_INDEX: usize = SESSION_ID_LEN_INDEX + 1 + TLS_SESSION_ID_SIZE - HMAC_SIZE_V3;
        if frame.len() < HMAC_INDEX + HMAC_SIZE_V3 || frame[0] != HANDSHAKE || frame[5] != 0x01 {
            return false;
        }
        if frame[SESSION_ID_LEN_INDEX] != TLS_SESSION_ID_SIZE as u8 {
            return false;
        }
        let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(password.as_bytes()) else {
            return false;
        };
        mac.update(&frame[TLS_HEADER_SIZE..HMAC_INDEX]);
        mac.update(&[0u8; HMAC_SIZE_V3]);
        mac.update(&frame[HMAC_INDEX + HMAC_SIZE_V3..]);
        let sum = mac.finalize().into_bytes();
        sum[..HMAC_SIZE_V3] == frame[HMAC_INDEX..HMAC_INDEX + HMAC_SIZE_V3]
    }

    #[test]
    fn rustls_v3_session_id_passes_server_verify() {
        rustls::crypto::ring::default_provider().install_default().ok();
        let password = "st_test_ioIxewpGpPE";
        let cfg = Arc::new(
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(crate::transport::SkipVerifier))
                .with_no_client_auth(),
        );
        let password_owned = password.to_string();
        let mut conn = rustls::ClientConnection::new_with_session_id_generator(
            cfg,
            rustls::pki_types::ServerName::try_from("www.cloudflare.com")
                .unwrap()
                .to_owned(),
            Arc::new(move |client_hello| generate_v3_session_id(&password_owned, client_hello)),
        )
        .expect("client connection");
        let mut tls_out = Vec::new();
        while conn.wants_write() {
            conn.write_tls(&mut tls_out).expect("write hello");
            break;
        }
        assert!(super::verify_v3_client_hello(
            &tls_out,
            &[("u1".into(), password.into())],
        )
        .is_some());
    }

    #[test]
    fn rustls_hello_has_key_share_ext() {
        rustls::crypto::ring::default_provider().install_default().ok();
        let cfg = Arc::new(
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(
                    crate::transport::SkipVerifier,
                ))
                .with_no_client_auth(),
        );
        let mut hello =
            capture_rustls_client_hello(&cfg, "www.cloudflare.com").expect("capture hello");
        replace_client_hello_key_share(&mut hello, &[7u8; 32]).expect("replace key share");
    }

    #[test]
    fn patch_v3_client_hello_matches_server_verify() {
        let keys = generate_shadowtls_client_hello(Profile::Chrome, "www.cloudflare.com");
        let mut hello = keys.hello;
        patch_client_hello_buffer(&mut hello, "test-password").unwrap();
        assert!(verify_v3_client_hello(&hello, "test-password"));
    }
}
