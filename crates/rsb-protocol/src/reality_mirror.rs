//! REALITY dest mirror: fetch ServerHello from handshake target (Xray/sing-box compatible).

use anyhow::{Context, Result};
use rand::RngCore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::utls::{hello_layout, parse_client_hello_key_share};

pub struct MirroredHello {
    pub server_hello: Vec<u8>,
    pub change_cipher_spec: Vec<u8>,
    pub cipher: u16,
    pub eph_secret: StaticSecret,
}

/// Dial `dest`, send `client_hello`, read ServerHello + CCS, patch for REALITY.
pub async fn mirror_dest_handshake(
    client_hello: &[u8],
    dest_host: &str,
    dest_port: u16,
) -> Result<MirroredHello> {
    let mut dest = TcpStream::connect(format!("{dest_host}:{dest_port}"))
        .await
        .with_context(|| format!("reality mirror: connect {dest_host}:{dest_port}"))?;
    let _ = dest.set_nodelay(true);
    dest.write_all(client_hello)
        .await
        .context("reality mirror: write client hello")?;
    dest.flush().await?;

    let server_hello = read_tls_record(&mut dest).await?;
    anyhow::ensure!(
        server_hello.len() > 5 && server_hello[0] == 0x16 && server_hello[5] == 0x02,
        "reality mirror: expected ServerHello record, got type {:?}",
        server_hello.first()
    );
    let change_cipher_spec = read_tls_record(&mut dest).await?;
    anyhow::ensure!(
        change_cipher_spec.first() == Some(&0x14),
        "reality mirror: expected ChangeCipherSpec after ServerHello"
    );

    let mut eph_sk = [0u8; 32];
    rand::rng().fill_bytes(&mut eph_sk);
    let eph_secret = StaticSecret::from(eph_sk);
    let eph_pub = PublicKey::from(&eph_secret);

    let layout = hello_layout(client_hello).context("client hello layout")?;
    let sid_len = client_hello[layout.session_id_offset - 1] as usize;
    anyhow::ensure!(sid_len == 32, "reality mirror: expected 32-byte client session id");
    let client_sid =
        &client_hello[layout.session_id_offset..layout.session_id_offset + sid_len];

    let (server_hello, cipher) =
        rebuild_server_hello(&server_hello, client_sid, eph_pub.as_bytes())?;
    let _ = parse_client_hello_key_share(client_hello);

    Ok(MirroredHello {
        server_hello,
        change_cipher_spec,
        cipher,
        eph_secret,
    })
}

struct ParsedServerHello {
    random: [u8; 32],
    cipher: u16,
    compression: u8,
    extensions: Vec<u8>,
}

fn parse_server_hello_body(hs: &[u8]) -> Result<ParsedServerHello> {
    anyhow::ensure!(hs.first() == Some(&0x02), "not ServerHello handshake");
    let mut i = 4 + 2 + 32;
    let sid_len = hs[i] as usize;
    i += 1 + sid_len;
    anyhow::ensure!(i + 3 <= hs.len(), "server hello truncated before cipher");
    let cipher = u16::from_be_bytes([hs[i], hs[i + 1]]);
    i += 2;
    let compression = hs[i];
    i += 1;
    anyhow::ensure!(i + 2 <= hs.len(), "server hello missing extensions");
    let ext_len = u16::from_be_bytes([hs[i], hs[i + 1]]) as usize;
    i += 2;
    anyhow::ensure!(i + ext_len <= hs.len(), "server hello extensions truncated");
    let extensions = hs[i..i + ext_len].to_vec();
    let mut random = [0u8; 32];
    random.copy_from_slice(&hs[4 + 2..4 + 2 + 32]);
    Ok(ParsedServerHello {
        random,
        cipher,
        compression,
        extensions,
    })
}

fn patch_key_share(extensions: &mut [u8], eph_pub: &[u8; 32]) -> Result<()> {
    let mut i = 0usize;
    while i + 4 <= extensions.len() {
        let ext_type = u16::from_be_bytes([extensions[i], extensions[i + 1]]);
        let ext_val_len = u16::from_be_bytes([extensions[i + 2], extensions[i + 3]]) as usize;
        i += 4;
        if ext_type == 0x0033 && ext_val_len >= 4 && i + ext_val_len <= extensions.len() {
            let data = &mut extensions[i..i + ext_val_len];
            if patch_key_share_entries(data, eph_pub)? {
                return Ok(());
            }
        }
        i += ext_val_len;
    }
    anyhow::bail!("reality mirror: X25519 key_share not found in dest ServerHello")
}

/// Patch X25519 (0x001d) in key_share bytes (client list or server single entry).
fn patch_key_share_entries(data: &mut [u8], eph_pub: &[u8; 32]) -> Result<bool> {
    // ServerHello: single KeyShareEntry (group, len, key) with no leading list length.
    if data.len() >= 36 {
        let group = u16::from_be_bytes([data[0], data[1]]);
        let key_len = u16::from_be_bytes([data[2], data[3]]) as usize;
        if group == 0x001d && key_len == 32 && data.len() >= 4 + key_len {
            data[4..36].copy_from_slice(eph_pub);
            return Ok(true);
        }
    }
    // ClientHello-style list: uint16 length + entries.
    if data.len() < 2 {
        return Ok(false);
    }
    let list_len = u16::from_be_bytes([data[0], data[1]]) as usize;
    let mut k = 2usize;
    let list_end = (2 + list_len).min(data.len());
    while k + 4 <= list_end {
        let group = u16::from_be_bytes([data[k], data[k + 1]]);
        let key_len = u16::from_be_bytes([data[k + 2], data[k + 3]]) as usize;
        k += 4;
        if k + key_len > data.len() {
            break;
        }
        if group == 0x001d && key_len == 32 {
            data[k..k + 32].copy_from_slice(eph_pub);
            return Ok(true);
        }
        k += key_len;
    }
    Ok(false)
}

fn rebuild_server_hello(
    dest_record: &[u8],
    client_sid: &[u8],
    eph_pub: &[u8; 32],
) -> Result<(Vec<u8>, u16)> {
    let parsed = parse_server_hello_body(&dest_record[5..])?;
    anyhow::ensure!(
        parsed.cipher == 0x1301 || parsed.cipher == 0x1302 || parsed.cipher == 0x1303,
        "reality mirror: dest cipher {:#x} not TLS1.3",
        parsed.cipher
    );
    let mut extensions = parsed.extensions;
    patch_key_share(&mut extensions, eph_pub)?;

    let mut body = Vec::new();
    body.push(0x02);
    body.extend_from_slice(&[0, 0, 0]); // placeholder len
    body.extend_from_slice(&[0x03, 0x03]);
    body.extend_from_slice(&parsed.random);
    body.push(client_sid.len() as u8);
    body.extend_from_slice(client_sid);
    body.extend_from_slice(&parsed.cipher.to_be_bytes());
    body.push(parsed.compression);
    body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
    body.extend_from_slice(&extensions);

    let body_len = body.len() - 4;
    body[1] = ((body_len >> 16) & 0xff) as u8;
    body[2] = ((body_len >> 8) & 0xff) as u8;
    body[3] = (body_len & 0xff) as u8;

    let mut record = vec![0x16, 0x03, 0x03];
    let rec_len = body.len() as u16;
    record.extend_from_slice(&rec_len.to_be_bytes());
    record.extend_from_slice(&body);
    Ok((record, parsed.cipher))
}

async fn read_tls_record(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut hdr = [0u8; 5];
    stream.read_exact(&mut hdr).await?;
    let len = u16::from_be_bytes([hdr[3], hdr[4]]) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    let mut out = hdr.to_vec();
    out.extend_from_slice(&body);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_server_key_share_single_entry() {
        let mut exts = vec![0x00, 0x33, 0x00, 0x24, 0x00, 0x1d, 0x00, 0x20];
        exts.extend_from_slice(&[0xbb; 32]);
        let eph = [0x22u8; 32];
        assert!(patch_key_share_entries(&mut exts[4..], &eph).unwrap());
        assert_eq!(&exts[8..40], &eph);
    }

    #[test]
    fn rebuild_inserts_client_session_id() {
        // minimal dest SH: empty session id
        let mut dest_body = vec![
            0x02, 0x00, 0x00, 0x4a, 0x03, 0x03,
        ];
        dest_body.extend_from_slice(&[0xaa; 32]); // random
        dest_body.push(0); // empty session id
        dest_body.extend_from_slice(&0x1301u16.to_be_bytes());
        dest_body.push(0); // compression
        // extensions: supported_versions + key_share
        let mut exts = vec![
            0x00, 0x2b, 0x00, 0x02, 0x03, 0x04,
            0x00, 0x33, 0x00, 0x24, 0x00, 0x1d, 0x00, 0x20,
        ];
        exts.extend_from_slice(&[0xbb; 32]);
        dest_body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
        dest_body.extend_from_slice(&exts);
        let body_len = dest_body.len() - 4;
        dest_body[1] = ((body_len >> 16) & 0xff) as u8;
        dest_body[2] = ((body_len >> 8) & 0xff) as u8;
        dest_body[3] = (body_len & 0xff) as u8;

        let mut dest_rec = vec![0x16, 0x03, 0x03];
        let rl = dest_body.len() as u16;
        dest_rec.extend_from_slice(&rl.to_be_bytes());
        dest_rec.extend_from_slice(&dest_body);

        let client_sid = [0x11u8; 32];
        let eph = [0x22u8; 32];
        let (out, cipher) = rebuild_server_hello(&dest_rec, &client_sid, &eph).unwrap();
        assert_eq!(cipher, 0x1301);
        assert_eq!(out[5], 0x02);
        assert_eq!(out[43], 32); // session id len
        assert_eq!(&out[44..76], &client_sid);
    }
}
