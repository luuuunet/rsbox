//! TLS 1.3 client handshake completion after custom ClientHello.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::reality_cert::{parse_reality_cert_parts, verify_reality_cert};

const TLS13_AES_128_GCM_SHA256: u16 = 0x1301;

pub struct UtlsTlsStream {
    tcp: TcpStream,
    read_cipher: Aes128Gcm,
    write_cipher: Aes128Gcm,
    read_iv: [u8; 12],
    write_iv: [u8; 12],
    read_seq: u64,
    write_seq: u64,
    read_buf: Vec<u8>,
    reality_verified: bool,
}

impl UtlsTlsStream {
    pub async fn connect(
        tcp: TcpStream,
        client_hello: &[u8],
        secret: StaticSecret,
        _sni: &str,
        _insecure: bool,
    ) -> Result<Self> {
        Self::handshake(tcp, client_hello, secret, None).await
    }

    pub async fn connect_reality(
        tcp: TcpStream,
        client_hello: &[u8],
        secret: StaticSecret,
        auth_key: [u8; 32],
    ) -> Result<Self> {
        Self::handshake(tcp, client_hello, secret, Some(auth_key)).await
    }

    async fn handshake(
        mut tcp: TcpStream,
        client_hello: &[u8],
        secret: StaticSecret,
        auth_key: Option<[u8; 32]>,
    ) -> Result<Self> {
        use tokio::io::AsyncWriteExt;
        tcp.write_all(client_hello).await?;
        let mut transcript = client_hello.to_vec();

        let server_hello = read_handshake_record(&mut tcp).await?;
        transcript.extend_from_slice(&server_hello);
        let (server_pub, cipher) = parse_server_hello(&server_hello)?;
        anyhow::ensure!(
            cipher == TLS13_AES_128_GCM_SHA256,
            "unsupported cipher suite {cipher:#x}"
        );

        let shared = secret.diffie_hellman(&PublicKey::from(server_pub));
        let th = Sha256::digest(&transcript);
        let hs = derive_handshake_keys(shared.as_bytes(), &th)?;

        let read_cipher = Aes128Gcm::new_from_slice(&hs.read_key).context("read cipher")?;
        let write_cipher = Aes128Gcm::new_from_slice(&hs.write_key).context("write cipher")?;

        let mut read_seq = 0u64;
        let mut server_msgs = Vec::new();
        for _ in 0..8 {
            let (plain, content_type) =
                read_encrypted_record(&mut tcp, &read_cipher, &hs.read_iv, &mut read_seq).await?;
            server_msgs.extend_from_slice(&plain);
            if content_type == 0x16 || server_msgs.iter().any(|&b| b == 0x0b) {
                break;
            }
        }

        transcript.extend_from_slice(&server_msgs);
        if let Some(key) = auth_key {
            verify_reality_certificate(&server_msgs, &key)?;
        }

        let client_finished = build_finished(&hs.client_secret, &transcript);
        write_encrypted_handshake(&mut tcp, &write_cipher, &hs.write_iv, 1, &client_finished)
            .await?;
        transcript.extend_from_slice(&client_finished);

        for _ in 0..4 {
            let (plain, _ct) =
                read_encrypted_record(&mut tcp, &read_cipher, &hs.read_iv, &mut read_seq).await?;
            if plain.is_empty() {
                continue;
            }
            transcript.extend_from_slice(&plain);
            if plain.first() == Some(&0x14) {
                break;
            }
        }

        let app = derive_application_keys(&hs.hs_secret, &transcript)?;
        let read_cipher = Aes128Gcm::new_from_slice(&app.read_key).context("app read cipher")?;
        let write_cipher = Aes128Gcm::new_from_slice(&app.write_key).context("app write cipher")?;

        Ok(Self {
            tcp,
            read_cipher,
            write_cipher,
            read_iv: app.read_iv,
            write_iv: app.write_iv,
            read_seq: 0,
            write_seq: 0,
            read_buf: Vec::new(),
            reality_verified: auth_key.is_some(),
        })
    }
}

fn verify_reality_certificate(hs_messages: &[u8], auth_key: &[u8; 32]) -> Result<()> {
    let mut i = 0usize;
    while i + 4 < hs_messages.len() {
        let msg_type = hs_messages[i];
        let msg_len = u32::from_be_bytes([
            0,
            hs_messages[i + 1],
            hs_messages[i + 2],
            hs_messages[i + 3],
        ]) as usize;
        let start = i + 4;
        let end = start + msg_len;
        if end > hs_messages.len() {
            break;
        }
        if msg_type == 0x0b {
            let cert_data = &hs_messages[start..end];
            if let Some((pubkey, sig)) = parse_reality_cert_parts(cert_data) {
                if verify_reality_cert(auth_key, &pubkey, &sig) {
                    return Ok(());
                }
                anyhow::bail!("reality certificate HMAC verification failed");
            }
        }
        i = end;
    }
    anyhow::bail!("reality: certificate message missing");
}

struct HandshakeKeys {
    read_key: Vec<u8>,
    read_iv: [u8; 12],
    write_key: Vec<u8>,
    write_iv: [u8; 12],
    client_secret: [u8; 32],
    hs_secret: [u8; 32],
}

fn derive_handshake_keys(shared: &[u8], transcript_hash: &[u8]) -> Result<HandshakeKeys> {
    let zero = [0u8; 32];
    let empty_hash = Sha256::digest([]);
    let early = hkdf_extract(&empty_hash, &zero);
    let derived = expand_label(&early, "derived", &[], 32);
    let hs_secret = hkdf_extract(&derived, shared);
    let client_secret = expand_label(&hs_secret, "c hs traffic", transcript_hash, 32);
    let server_secret = expand_label(&hs_secret, "s hs traffic", transcript_hash, 32);
    let read_key = expand_label(&server_secret, "key", &[], 16);
    let read_iv = expand_label(&server_secret, "iv", &[], 12);
    let write_key = expand_label(&client_secret, "key", &[], 16);
    let write_iv = expand_label(&client_secret, "iv", &[], 12);
    Ok(HandshakeKeys {
        read_key,
        read_iv: read_iv.try_into().unwrap(),
        write_key,
        write_iv: write_iv.try_into().unwrap(),
        client_secret: client_secret.try_into().unwrap(),
        hs_secret,
    })
}

fn derive_application_keys(hs_secret: &[u8; 32], transcript: &[u8]) -> Result<HandshakeKeys> {
    let th = Sha256::digest(transcript);
    let th_bytes = th.as_slice();
    let client_secret = expand_label(hs_secret, "c ap traffic", th_bytes, 32);
    let server_secret = expand_label(hs_secret, "s ap traffic", th_bytes, 32);
    let read_key = expand_label(&server_secret, "key", &[], 16);
    let read_iv = expand_label(&server_secret, "iv", &[], 12);
    let write_key = expand_label(&client_secret, "key", &[], 16);
    let write_iv = expand_label(&client_secret, "iv", &[], 12);
    Ok(HandshakeKeys {
        read_key,
        read_iv: read_iv.try_into().unwrap(),
        write_key,
        write_iv: write_iv.try_into().unwrap(),
        client_secret: client_secret.try_into().unwrap(),
        hs_secret: *hs_secret,
    })
}

fn hkdf_extract(salt: &[u8], ikm: &[u8]) -> [u8; 32] {
    use hmac::digest::KeyInit;
    use hmac::Hmac;
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(salt).expect("hmac");
    use hmac::Mac;
    mac.update(ikm);
    mac.finalize().into_bytes().into()
}

fn expand_label(secret: &[u8], label: &str, context: &[u8], len: usize) -> Vec<u8> {
    use hkdf::Hkdf;
    let mut full_label = b"tls13 ".to_vec();
    full_label.extend_from_slice(label.as_bytes());
    let mut hkdf_label = Vec::new();
    hkdf_label.extend_from_slice(&(len as u16).to_be_bytes());
    hkdf_label.push(full_label.len() as u8);
    hkdf_label.extend_from_slice(&full_label);
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);
    let hk = Hkdf::<Sha256>::new(None, secret);
    let mut out = vec![0u8; len];
    hk.expand(&hkdf_label, &mut out).ok();
    out
}

fn build_finished(client_hs_secret: &[u8; 32], transcript: &[u8]) -> Vec<u8> {
    let th = Sha256::digest(transcript);
    let finished_key = expand_label(client_hs_secret, "finished", &[], 32);
    use hmac::digest::KeyInit;
    use hmac::Hmac;
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(&finished_key).expect("hmac");
    use hmac::Mac;
    mac.update(&th);
    let verify = mac.finalize().into_bytes();
    let mut hs = vec![0x14, 0x00, 0x00, 0x20];
    hs.extend_from_slice(&verify);
    hs
}

async fn read_encrypted_record(
    tcp: &mut TcpStream,
    cipher: &Aes128Gcm,
    base_iv: &[u8; 12],
    seq: &mut u64,
) -> Result<(Vec<u8>, u8)> {
    use tokio::io::AsyncReadExt;
    let mut hdr = [0u8; 5];
    tcp.read_exact(&mut hdr).await?;
    let len = u16::from_be_bytes([hdr[3], hdr[4]]) as usize;
    let mut enc = vec![0u8; len];
    tcp.read_exact(&mut enc).await?;
    let plain = decrypt_record(cipher, base_iv, *seq, &enc)?;
    *seq += 1;
    let content_type = plain.last().copied().unwrap_or(0x16);
    let inner = plain[..plain.len().saturating_sub(1)].to_vec();
    Ok((inner, content_type))
}

async fn write_encrypted_handshake(
    tcp: &mut TcpStream,
    cipher: &Aes128Gcm,
    base_iv: &[u8; 12],
    seq: u64,
    inner: &[u8],
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let enc = encrypt_handshake(cipher, base_iv, seq, inner)?;
    tcp.write_all(&enc).await?;
    Ok(())
}

fn encrypt_handshake(
    cipher: &Aes128Gcm,
    base_iv: &[u8; 12],
    seq: u64,
    inner: &[u8],
) -> Result<Vec<u8>> {
    let mut payload = inner.to_vec();
    payload.push(0x16);
    let enc = encrypt_record(cipher, base_iv, seq, &payload)?;
    Ok(enc)
}

impl AsyncRead for UtlsTlsStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if !self.read_buf.is_empty() {
            let n = self.read_buf.len().min(buf.remaining());
            buf.put_slice(&self.read_buf[..n]);
            self.read_buf.drain(..n);
            return Poll::Ready(Ok(()));
        }
        let mut hdr = [0u8; 5];
        match Pin::new(&mut self.tcp).poll_read(cx, &mut ReadBuf::new(&mut hdr)) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        }
        if hdr[0] != 0x17 {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "expected application data record",
            )));
        }
        let len = u16::from_be_bytes([hdr[3], hdr[4]]) as usize;
        let mut enc = vec![0u8; len];
        let mut rb = ReadBuf::new(&mut enc);
        while rb.remaining() > 0 {
            match Pin::new(&mut self.tcp).poll_read(cx, &mut rb) {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        let plain = decrypt_record(&self.read_cipher, &self.read_iv, self.read_seq, &enc)
            .map_err(std::io::Error::other)?;
        self.read_seq += 1;
        let inner = if plain.last() == Some(&0x17) {
            &plain[..plain.len() - 1]
        } else {
            plain.as_slice()
        };
        let n = inner.len().min(buf.remaining());
        buf.put_slice(&inner[..n]);
        if n < inner.len() {
            self.read_buf.extend_from_slice(&inner[n..]);
        }
        Poll::Ready(Ok(()))
    }
}

impl AsyncWrite for UtlsTlsStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut payload = buf.to_vec();
        payload.push(0x17);
        let enc = encrypt_record(&self.write_cipher, &self.write_iv, self.write_seq, &payload)
            .map_err(std::io::Error::other)?;
        self.write_seq += 1;
        match Pin::new(&mut self.tcp).poll_write(cx, &enc) {
            Poll::Ready(Ok(n)) if n == enc.len() => Poll::Ready(Ok(buf.len())),
            Poll::Ready(Ok(_)) => Poll::Ready(Err(std::io::Error::other("short tls write"))),
            other => other,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.tcp).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.tcp).poll_shutdown(cx)
    }
}

async fn read_handshake_record(tcp: &mut TcpStream) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut hdr = [0u8; 5];
    tcp.read_exact(&mut hdr).await?;
    let len = u16::from_be_bytes([hdr[3], hdr[4]]) as usize;
    let mut body = vec![0u8; len];
    tcp.read_exact(&mut body).await?;
    let mut out = hdr.to_vec();
    out.extend_from_slice(&body);
    Ok(out)
}

fn parse_server_hello(record: &[u8]) -> Result<([u8; 32], u16)> {
    let hs = &record[5..];
    anyhow::ensure!(hs.len() > 4 && hs[0] == 0x02, "expected ServerHello");
    let mut i = 4 + 2 + 32 + 1;
    let sess_len = hs[i] as usize;
    i += 1 + sess_len;
    i += 2;
    let cipher = u16::from_be_bytes([hs[i], hs[i + 1]]);
    i += 2 + 1;
    let ext_len = u16::from_be_bytes([hs[i], hs[i + 1]]) as usize;
    i += 2;
    let exts = &hs[i..i + ext_len.min(hs.len().saturating_sub(i))];
    let mut j = 0;
    while j + 4 <= exts.len() {
        let typ = u16::from_be_bytes([exts[j], exts[j + 1]]);
        let elen = u16::from_be_bytes([exts[j + 2], exts[j + 3]]) as usize;
        j += 4;
        if typ == 0x0033 && elen >= 36 {
            let group = u16::from_be_bytes([exts[j], exts[j + 1]]);
            anyhow::ensure!(group == 0x001d, "expected x25519 key share");
            let mut pk = [0u8; 32];
            pk.copy_from_slice(&exts[j + 4..j + 36]);
            return Ok((pk, cipher));
        }
        j += elen;
    }
    anyhow::bail!("server key share missing")
}

fn record_nonce(base_iv: &[u8; 12], seq: u64) -> [u8; 12] {
    let mut nonce = *base_iv;
    let seq_bytes = seq.to_be_bytes();
    for i in 0..8 {
        nonce[4 + i] ^= seq_bytes[i];
    }
    nonce
}

fn encrypt_record(
    cipher: &Aes128Gcm,
    base_iv: &[u8; 12],
    seq: u64,
    plain: &[u8],
) -> Result<Vec<u8>> {
    let nonce = record_nonce(base_iv, seq);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), plain)
        .map_err(|e| anyhow::anyhow!("encrypt: {e}"))?;
    let mut out = vec![0x17, 0x03, 0x03];
    out.extend_from_slice(&(ct.len() as u16).to_be_bytes());
    out.extend_from_slice(&ct);
    Ok(out)
}

fn decrypt_record(cipher: &Aes128Gcm, base_iv: &[u8; 12], seq: u64, enc: &[u8]) -> Result<Vec<u8>> {
    let nonce = record_nonce(base_iv, seq);
    cipher
        .decrypt(Nonce::from_slice(&nonce), enc)
        .map_err(|e| anyhow::anyhow!("decrypt: {e}"))
}

pub enum TlsIo {
    Rustls(tokio_rustls::client::TlsStream<TcpStream>),
    Utls(UtlsTlsStream),
}

impl AsyncRead for TlsIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TlsIo::Rustls(s) => Pin::new(s).poll_read(cx, buf),
            TlsIo::Utls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for TlsIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            TlsIo::Rustls(s) => Pin::new(s).poll_write(cx, buf),
            TlsIo::Utls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TlsIo::Rustls(s) => Pin::new(s).poll_flush(cx),
            TlsIo::Utls(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            TlsIo::Rustls(s) => Pin::new(s).poll_shutdown(cx),
            TlsIo::Utls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}
