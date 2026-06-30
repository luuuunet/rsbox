//! ShadowTLS v3 server (sing-shadowtls compatible).

use crate::shadowtls::constants::*;
use crate::shadowtls::handshake::verify_v3_client_hello;
use crate::shadowtls::v3::{VerifiedConn, V3AuthState, kdf_public};
use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

type HmacSha1 = Hmac<Sha1>;

#[derive(Clone)]
pub struct V3ServerConfig {
    pub users: Vec<(String, String)>,
    pub handshake_server: String,
    pub handshake_port: u16,
    pub strict_mode: bool,
    pub detour: SocketAddr,
}

pub async fn serve_v3(
    client: TcpStream,
    cfg: V3ServerConfig,
    connections: rsb_core::SharedConnectionManager,
    inbound_tag: String,
) -> Result<()> {
    let mut client = client;
    let _ = client.set_nodelay(true);
    let client_hello = read_tls_frame(&mut client).await?;
    let mut hs = TcpStream::connect(format!(
        "{}:{}",
        cfg.handshake_server, cfg.handshake_port
    ))
    .await
    .context("shadowtls: connect handshake server")?;
    let _ = hs.set_nodelay(true);
    hs.write_all(&client_hello).await?;

    let password = match verify_v3_client_hello(&client_hello, &cfg.users) {
        Some(p) => {
            tracing::info!("shadowtls v3: client hello verified");
            p
        },
        None => {
            tracing::warn!("shadowtls v3: client hello verify failed, passthrough");
            return relay_bidirectional(client, hs).await;
        }
    };

    let server_hello = read_tls_frame(&mut hs).await?;
    client.write_all(&server_hello).await?;
    client.flush().await?;

    let server_random = extract_server_random(&server_hello)
        .ok_or_else(|| anyhow::anyhow!("shadowtls v3: server random missing"))?;

    if cfg.strict_mode && !is_server_hello_tls13(&server_hello[5..]) {
        tracing::debug!("shadowtls v3: strict_mode TLS1.3 required, passthrough");
        return relay_bidirectional(client, hs).await;
    }

    let (tcp, _client_first) =
        relay_handshake_auth(client, hs, &password, &server_random).await?;

    let auth = V3AuthState {
        password: password.clone(),
        server_random: server_random.to_vec(),
        is_tls13: is_server_hello_tls13(&server_hello[5..]),
        authorized: true,
        read_hmac: None,
        pending: Vec::new(),
    };
    let mut verified =
        VerifiedConn::from_auth_server(tcp, auth).context("shadowtls verified conn")?;
    let mut upstream = TcpStream::connect(cfg.detour).await.context("shadowtls detour")?;
    let session = crate::user_relay::begin_for_password(
        &connections,
        &inbound_tag,
        &password,
        Some(cfg.detour),
        None,
    )?;
    crate::inbound_proxy::relay_streams_user(&session, &mut verified, &mut upstream).await
}

async fn relay_bidirectional(mut a: TcpStream, mut b: TcpStream) -> Result<()> {
    let (mut ar, mut aw) = a.split();
    let (mut br, mut bw) = b.split();
    let c2s = tokio::io::copy(&mut ar, &mut bw);
    let s2c = tokio::io::copy(&mut br, &mut aw);
    tokio::try_join!(c2s, s2c)?;
    Ok(())
}

async fn read_tls_frame<S>(stream: &mut S) -> io::Result<Vec<u8>>
where
    S: tokio::io::AsyncRead + Unpin,
{
    let mut header = [0u8; TLS_HEADER_SIZE];
    stream.read_exact(&mut header).await?;
    let len = u16::from_be_bytes([header[3], header[4]]) as usize;
    let mut frame = vec![0u8; TLS_HEADER_SIZE + len];
    frame[..TLS_HEADER_SIZE].copy_from_slice(&header);
    stream.read_exact(&mut frame[TLS_HEADER_SIZE..]).await?;
    Ok(frame)
}

fn extract_server_random(frame: &[u8]) -> Option<[u8; TLS_RANDOM_SIZE]> {
    if frame.len() < SERVER_RANDOM_INDEX + TLS_RANDOM_SIZE || frame[5] != SERVER_HELLO {
        return None;
    }
    let mut out = [0u8; TLS_RANDOM_SIZE];
    out.copy_from_slice(&frame[SERVER_RANDOM_INDEX..SERVER_RANDOM_INDEX + TLS_RANDOM_SIZE]);
    Some(out)
}

fn is_server_hello_tls13(hs: &[u8]) -> bool {
    const SESSION_ID_LEN_IDX: usize = 1 + 3 + 2 + TLS_RANDOM_SIZE;
    if hs.len() <= SESSION_ID_LEN_IDX + 3 {
        return false;
    }
    let mut i = SESSION_ID_LEN_IDX;
    let session_id_len = hs[i] as usize;
    i += 1 + session_id_len + 2 + 1;
    if i + 2 > hs.len() {
        return false;
    }
    let ext_len = u16::from_be_bytes([hs[i], hs[i + 1]]) as usize;
    i += 2;
    if i + ext_len > hs.len() {
        return false;
    }
    let exts = &hs[i..i + ext_len];
    let mut j = 0usize;
    while j + 4 <= exts.len() {
        let ext_type = u16::from_be_bytes([exts[j], exts[j + 1]]);
        let ext_val_len = u16::from_be_bytes([exts[j + 2], exts[j + 3]]) as usize;
        j += 4;
        if ext_type == 43 {
            if ext_val_len == 2 && j + 2 <= exts.len() {
                return u16::from_be_bytes([exts[j], exts[j + 1]]) == 0x0304;
            }
            return false;
        }
        if j + ext_val_len > exts.len() {
            break;
        }
        j += ext_val_len;
    }
    false
}

fn client_auth_frame(
    frame: &[u8],
    server_random: &[u8],
    password: &str,
    hmac_verify: &mut HmacSha1,
) -> bool {
    // sing-shadowtls: APPLICATION_DATA with HMAC(ServerRandom+"C", payload) prefix.
    // Empty payload auth is exactly tlsHmacHeaderSize (9); Go clients may attach first app bytes (len > 9).
    if frame[0] != APPLICATION_DATA || frame.len() < TLS_HMAC_HEADER_SIZE_V3 {
        return false;
    }
    let body = &frame[TLS_HEADER_SIZE..];
    let Ok(mut mac) = HmacSha1::new_from_slice(password.as_bytes()) else {
        return false;
    };
    mac.update(server_random);
    mac.update(b"C");
    mac.update(&body[HMAC_SIZE_V3..]);
    let sum = mac.finalize().into_bytes();
    if sum[..HMAC_SIZE_V3] != body[..HMAC_SIZE_V3] {
        return false;
    }
    *hmac_verify = HmacSha1::new_from_slice(password.as_bytes())
        .map_err(|_| ())
        .expect("hmac key");
    hmac_verify.update(server_random);
    hmac_verify.update(b"C");
    hmac_verify.update(&body[HMAC_SIZE_V3..]);
    hmac_verify.update(&body[..HMAC_SIZE_V3]);
    true
}

async fn relay_handshake_auth(
    client: TcpStream,
    hs: TcpStream,
    password: &str,
    server_random: &[u8],
) -> Result<(TcpStream, Vec<u8>)> {
    let (mut c_read, mut c_write) = client.into_split();
    let (mut h_read, mut h_write) = hs.into_split();

    let password = password.to_string();
    let server_random = server_random.to_vec();
    let auth_done = Arc::new(AtomicBool::new(false));

    let done_c2s = auth_done.clone();
    let password_c2s = password.clone();
    let sr_verify = server_random.clone();
    let client_task = tokio::spawn(async move {
        let mut hmac_verify = HmacSha1::new_from_slice(password_c2s.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        loop {
            if done_c2s.load(Ordering::Relaxed) {
                break;
            }
            let frame = read_tls_frame(&mut c_read).await?;
            tracing::debug!(
                direction = "client_to_cf",
                record_type = frame[0],
                len = frame.len(),
                "shadowtls v3 relay frame"
            );
            if client_auth_frame(&frame, &sr_verify, &password_c2s, &mut hmac_verify) {
                tracing::debug!("shadowtls v3: client auth frame accepted");
                done_c2s.store(true, Ordering::Relaxed);
                break;
            }
            h_write.write_all(&frame).await?;
            h_write.flush().await?;
        }
        let _ = h_write.shutdown().await;
        Ok::<(tokio::net::tcp::OwnedReadHalf, ()), io::Error>((c_read, ()))
    });

    let done_s2c = auth_done.clone();
    let password_s2c = password;
    let sr_wrap = server_random;
    let server_task = tokio::spawn(async move {
        let mut hmac_write = HmacSha1::new_from_slice(password_s2c.as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        hmac_write.update(&sr_wrap);
        loop {
            if done_s2c.load(Ordering::Relaxed) {
                break;
            }
            let frame = match read_tls_frame(&mut h_read).await {
                Ok(frame) => frame,
                Err(e) if done_s2c.load(Ordering::Relaxed) => break,
                Err(e) => return Err(e),
            };
            tracing::debug!(
                direction = "cf_to_client",
                record_type = frame[0],
                len = frame.len(),
                "shadowtls v3 relay frame"
            );
            let out = if frame[0] == APPLICATION_DATA {
                wrap_server_app_data(&frame, &password_s2c, &sr_wrap, &mut hmac_write)?
            } else {
                frame
            };
            c_write.write_all(&out).await?;
            c_write.flush().await?;
        }
        Ok::<(tokio::net::tcp::OwnedWriteHalf, ()), io::Error>((c_write, ()))
    });

    let (c_read, _) = match client_task.await {
        Ok(Ok(parts)) => parts,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "shadowtls v3 client relay failed");
            return Err(e.into());
        }
        Err(e) => return Err(anyhow::anyhow!("shadowtls v3 client relay task: {e}")),
    };
    auth_done.store(true, Ordering::Relaxed);
    let (c_write, _) = match server_task.await {
        Ok(Ok(parts)) => parts,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "shadowtls v3 server relay failed");
            return Err(e.into());
        }
        Err(e) => return Err(anyhow::anyhow!("shadowtls v3 server relay task: {e}")),
    };

    let client = c_read
        .reunite(c_write)
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("reunite client: {e}")))?;
    Ok((client, Vec::new()))
}

fn wrap_server_app_data(
    frame: &[u8],
    password: &str,
    server_random: &[u8],
    hmac_write: &mut HmacSha1,
) -> io::Result<Vec<u8>> {
    let payload = &frame[TLS_HEADER_SIZE..];
    let key = kdf_public(password, server_random);
    let mut xored = payload.to_vec();
    for (i, b) in xored.iter_mut().enumerate() {
        *b ^= key[i % key.len()];
    }
    hmac_write.update(&xored);
    let hash = hmac_write.clone().finalize().into_bytes();

    let mut out = Vec::with_capacity(TLS_HMAC_HEADER_SIZE_V3 + xored.len());
    out.push(APPLICATION_DATA);
    out.extend_from_slice(&TLS_VERSION_12);
    out.extend_from_slice(&((HMAC_SIZE_V3 + xored.len()) as u16).to_be_bytes());
    out.extend_from_slice(&hash[..HMAC_SIZE_V3]);
    out.extend_from_slice(&xored);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shadowtls::v3::kdf_public;

    fn app_data_record(payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(TLS_HEADER_SIZE + payload.len());
        frame.push(APPLICATION_DATA);
        frame.extend_from_slice(&TLS_VERSION_12);
        frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    fn client_unwrap_handshake(record: &mut [u8], mac: &mut HmacSha1, key: &[u8]) -> bool {
        if record[0] != APPLICATION_DATA || record.len() <= TLS_HMAC_HEADER_SIZE_V3 {
            return false;
        }
        mac.update(&record[TLS_HMAC_HEADER_SIZE_V3..]);
        let sum = mac.clone().finalize().into_bytes();
        if sum[..HMAC_SIZE_V3] != record[TLS_HEADER_SIZE..TLS_HMAC_HEADER_SIZE_V3] {
            return false;
        }
        for (i, b) in record[TLS_HMAC_HEADER_SIZE_V3..].iter_mut().enumerate() {
            *b ^= key[i % key.len()];
        }
        true
    }

    #[test]
    fn client_auth_frame_accepts_exact_hmac_record() {
        let password = "st_test_ioIxewpGpPE";
        let server_random = [3u8; TLS_RANDOM_SIZE];
        let frame = crate::shadowtls::v3::build_client_auth_frame(password, &server_random).unwrap();
        assert_eq!(frame.len(), TLS_HMAC_HEADER_SIZE_V3);
        let mut mac = HmacSha1::new_from_slice(password.as_bytes()).unwrap();
        assert!(client_auth_frame(
            &frame,
            &server_random,
            password,
            &mut mac,
        ));
    }

    #[test]
    fn wrap_unwrap_chains_multiple_app_records() {
        let password = "st_test_ioIxewpGpPE";
        let server_random = [9u8; TLS_RANDOM_SIZE];
        let key = kdf_public(password, &server_random);
        let mut hmac_write = HmacSha1::new_from_slice(password.as_bytes()).unwrap();
        hmac_write.update(&server_random);
        let mut read_mac = HmacSha1::new_from_slice(password.as_bytes()).unwrap();
        read_mac.update(&server_random);

        for payload in [b"encrypted-finished".as_ref(), b"session-ticket".as_ref()] {
            let frame = app_data_record(payload);
            let wrapped = wrap_server_app_data(&frame, password, &server_random, &mut hmac_write).unwrap();
            let mut record = wrapped;
            assert!(client_unwrap_handshake(&mut record, &mut read_mac, &key));
            assert_eq!(
                &record[TLS_HMAC_HEADER_SIZE_V3..TLS_HMAC_HEADER_SIZE_V3 + payload.len()],
                payload
            );
        }
    }
}
