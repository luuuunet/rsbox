//! TLS 1.3 client handshake completion after custom ClientHello.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes128Gcm, Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256, Sha384};
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::reality_cert::{parse_reality_cert_parts, verify_reality_cert};

const TLS13_AES_128_GCM_SHA256: u16 = 0x1301;
const TLS13_AES_256_GCM_SHA384: u16 = 0x1302;

enum Tls13Cipher {
    Aes128(Aes128Gcm),
    Aes256(Aes256Gcm),
}

impl Tls13Cipher {
    fn decrypt(&self, base_iv: &[u8; 12], seq: u64, aad: &[u8], enc: &[u8]) -> Result<Vec<u8>> {
        match self {
            Self::Aes128(c) => decrypt_record(c, base_iv, seq, aad, enc),
            Self::Aes256(c) => decrypt_record(c, base_iv, seq, aad, enc),
        }
    }

    fn encrypt_handshake(
        &self,
        base_iv: &[u8; 12],
        seq: u64,
        inner: &[u8],
    ) -> Result<Vec<u8>> {
        match self {
            Self::Aes128(c) => encrypt_handshake(c, base_iv, seq, inner),
            Self::Aes256(c) => encrypt_handshake(c, base_iv, seq, inner),
        }
    }

    fn encrypt_record(
        &self,
        base_iv: &[u8; 12],
        seq: u64,
        plain: &[u8],
    ) -> Result<Vec<u8>> {
        match self {
            Self::Aes128(c) => encrypt_record(c, base_iv, seq, plain),
            Self::Aes256(c) => encrypt_record(c, base_iv, seq, plain),
        }
    }
}

pub struct UtlsTlsStream {
    tcp: TcpStream,
    read_cipher: Aes128Gcm,
    write_cipher: Aes128Gcm,
    read_iv: [u8; 12],
    write_iv: [u8; 12],
    read_seq: u64,
    write_seq: u64,
    read_buf: Vec<u8>,
    read_hdr: [u8; 5],
    read_hdr_len: u8,
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
        let app = tls13_handshake_core(&mut tcp, client_hello, secret, auth_key).await?;
        Ok(Self {
            tcp,
            read_cipher: Aes128Gcm::new_from_slice(&app.read_key).context("app read cipher")?,
            write_cipher: Aes128Gcm::new_from_slice(&app.write_key).context("app write cipher")?,
            read_iv: app.read_iv,
            write_iv: app.write_iv,
            read_seq: 0,
            write_seq: 0,
            read_buf: Vec::new(),
            read_hdr: [0u8; 5],
            read_hdr_len: 0,
            reality_verified: auth_key.is_some(),
        })
    }
}

struct AppKeys {
    read_key: Vec<u8>,
    read_iv: [u8; 12],
    write_key: Vec<u8>,
    write_iv: [u8; 12],
}

/// Run TLS 1.3 client handshake to completion (camouflage only; no session kept).
pub async fn complete_tls13_camouflage<S>(
    stream: &mut S,
    client_hello: &[u8],
    secret: StaticSecret,
    auth_key: Option<[u8; 32]>,
) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    tls13_handshake_core(stream, client_hello, secret, auth_key)
        .await
        .map(|_| ())
}

async fn tls13_handshake_core<S>(
    stream: &mut S,
    client_hello: &[u8],
    secret: StaticSecret,
    auth_key: Option<[u8; 32]>,
) -> Result<AppKeys>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    stream
        .write_all(client_hello)
        .await
        .context("write client hello")?;
    stream.flush().await.context("flush client hello")?;
    let mut transcript = if auth_key.is_some() {
        client_hello_tls_transcript(client_hello)
    } else {
        client_hello[5..].to_vec()
    };

    let server_hello = read_tls_record(stream)
        .await
        .context("read server hello")?;
    transcript.extend_from_slice(&server_hello[5..]);
    let (server_pub, cipher) = parse_server_hello(&server_hello).context("parse server hello")?;

    let shared = secret.diffie_hellman(&PublicKey::from(server_pub));
    let hash = cipher_hash(cipher);
    let th = transcript_hash(hash, &transcript);
    let key_len = cipher_key_len(cipher);
    let hs = derive_handshake_keys(hash, shared.as_bytes(), &th, key_len)?;

    let read_cipher = match cipher {
        TLS13_AES_128_GCM_SHA256 => Tls13Cipher::Aes128(
            Aes128Gcm::new_from_slice(&hs.read_key).context("read cipher")?,
        ),
        TLS13_AES_256_GCM_SHA384 => Tls13Cipher::Aes256(
            Aes256Gcm::new_from_slice(&hs.read_key).context("read cipher")?,
        ),
        other => anyhow::bail!("unsupported cipher suite {other:#x}"),
    };
    let write_cipher = match cipher {
        TLS13_AES_128_GCM_SHA256 => Tls13Cipher::Aes128(
            Aes128Gcm::new_from_slice(&hs.write_key).context("write cipher")?,
        ),
        TLS13_AES_256_GCM_SHA384 => Tls13Cipher::Aes256(
            Aes256Gcm::new_from_slice(&hs.write_key).context("write cipher")?,
        ),
        other => anyhow::bail!("unsupported cipher suite {other:#x}"),
    };

    let mut read_seq = 0u64;
    let mut server_msgs = Vec::new();
    for _ in 0..16 {
        let (plain, _content_type) = read_encrypted_record(
            stream,
            &read_cipher,
            &hs.read_iv,
            &mut read_seq,
        )
        .await
        .context("read server encrypted handshake")?;
        server_msgs.extend_from_slice(&plain);
        if handshake_contains(&server_msgs, 0x14) {
            break;
        }
    }
    anyhow::ensure!(
        handshake_contains(&server_msgs, 0x14),
        "server Finished missing in handshake"
    );

    transcript.extend_from_slice(&server_msgs);
    if let Some(key) = auth_key {
        verify_reality_certificate(&server_msgs, &key)?;
    }

    let app = if auth_key.is_some() {
        // REALITY (Go/sing-box): app traffic keys omit client Finished in transcript.
        derive_application_keys(hash, &hs.hs_secret, &transcript, key_len)?
    } else {
        let client_finished = build_finished(hash, &hs.client_secret, &transcript);
        emit_middlebox_ccs(stream)
            .await
            .context("write client change cipher spec")?;
        write_encrypted_handshake(stream, &write_cipher, &hs.write_iv, 0, &client_finished)
            .await
            .context("write client finished")?;
        stream.flush().await.context("flush client finished")?;
        transcript.extend_from_slice(&client_finished);
        derive_application_keys(hash, &hs.hs_secret, &transcript, key_len)?
    };

    if auth_key.is_some() {
        let client_finished = build_finished(hash, &hs.client_secret, &transcript);
        emit_middlebox_ccs(stream)
            .await
            .context("write client change cipher spec")?;
        write_encrypted_handshake(stream, &write_cipher, &hs.write_iv, 0, &client_finished)
            .await
            .context("write client finished")?;
        stream.flush().await.context("flush client finished")?;
    }
    Ok(AppKeys {
        read_key: app.read_key,
        read_iv: app.read_iv,
        write_key: app.write_key,
        write_iv: app.write_iv,
    })
}

fn handshake_contains(msgs: &[u8], msg_type: u8) -> bool {
    let mut i = 0usize;
    while i + 4 <= msgs.len() {
        if msgs[i] == msg_type {
            return true;
        }
        let len = u32::from_be_bytes([0, msgs[i + 1], msgs[i + 2], msgs[i + 3]]) as usize;
        if i + 4 + len > msgs.len() {
            break;
        }
        i += 4 + len;
    }
    false
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
    hs_secret: Vec<u8>,
}

fn cipher_key_len(cipher: u16) -> usize {
    if cipher == TLS13_AES_256_GCM_SHA384 {
        32
    } else {
        16
    }
}

#[derive(Clone, Copy)]
enum Tls13Hash {
    Sha256,
    Sha384,
}

fn cipher_hash(cipher: u16) -> Tls13Hash {
    if cipher == TLS13_AES_256_GCM_SHA384 {
        Tls13Hash::Sha384
    } else {
        Tls13Hash::Sha256
    }
}

fn transcript_hash(hash: Tls13Hash, transcript: &[u8]) -> Vec<u8> {
    match hash {
        Tls13Hash::Sha256 => Sha256::digest(transcript).to_vec(),
        Tls13Hash::Sha384 => Sha384::digest(transcript).to_vec(),
    }
}

fn derive_handshake_keys(
    hash: Tls13Hash,
    shared: &[u8],
    transcript_hash: &[u8],
    key_len: usize,
) -> Result<HandshakeKeys> {
    let zero = vec![0u8; hash_len(hash)];
    let empty_hash = transcript_hash_for_empty(hash);
    let early = hkdf_extract(hash, &empty_hash, &zero);
    let derived = expand_label(hash, &early, "derived", &[], hash_len(hash));
    let hs_secret = hkdf_extract(hash, &derived, shared);
    let client_secret = expand_label(hash, &hs_secret, "c hs traffic", transcript_hash, 32);
    let server_secret = expand_label(hash, &hs_secret, "s hs traffic", transcript_hash, 32);
    let read_key = expand_label(hash, &server_secret, "key", &[], key_len);
    let read_iv = expand_label(hash, &server_secret, "iv", &[], 12);
    let write_key = expand_label(hash, &client_secret, "key", &[], key_len);
    let write_iv = expand_label(hash, &client_secret, "iv", &[], 12);
    Ok(HandshakeKeys {
        read_key,
        read_iv: read_iv.try_into().unwrap(),
        write_key,
        write_iv: write_iv.try_into().unwrap(),
        client_secret: client_secret.try_into().unwrap(),
        hs_secret,
    })
}

fn derive_application_keys(
    hash: Tls13Hash,
    hs_secret: &[u8],
    transcript: &[u8],
    key_len: usize,
) -> Result<HandshakeKeys> {
    let th = transcript_hash(hash, transcript);
    let client_secret = expand_label(hash, hs_secret, "c ap traffic", &th, 32);
    let server_secret = expand_label(hash, hs_secret, "s ap traffic", &th, 32);
    let read_key = expand_label(hash, &server_secret, "key", &[], key_len);
    let read_iv = expand_label(hash, &server_secret, "iv", &[], 12);
    let write_key = expand_label(hash, &client_secret, "key", &[], key_len);
    let write_iv = expand_label(hash, &client_secret, "iv", &[], 12);
    Ok(HandshakeKeys {
        read_key,
        read_iv: read_iv.try_into().unwrap(),
        write_key,
        write_iv: write_iv.try_into().unwrap(),
        client_secret: client_secret.try_into().unwrap(),
        hs_secret: hs_secret.to_vec(),
    })
}

fn hash_len(hash: Tls13Hash) -> usize {
    match hash {
        Tls13Hash::Sha256 => 32,
        Tls13Hash::Sha384 => 48,
    }
}

fn transcript_hash_for_empty(hash: Tls13Hash) -> Vec<u8> {
    transcript_hash(hash, &[])
}

fn hkdf_extract(hash: Tls13Hash, salt: &[u8], ikm: &[u8]) -> Vec<u8> {
    use hmac::Mac;
    match hash {
        Tls13Hash::Sha256 => {
            let mut mac = <hmac::Hmac<Sha256> as hmac::digest::KeyInit>::new_from_slice(salt)
                .expect("hmac");
            mac.update(ikm);
            mac.finalize().into_bytes().to_vec()
        }
        Tls13Hash::Sha384 => {
            let mut mac = <hmac::Hmac<Sha384> as hmac::digest::KeyInit>::new_from_slice(salt)
                .expect("hmac");
            mac.update(ikm);
            mac.finalize().into_bytes().to_vec()
        }
    }
}

fn expand_label(hash: Tls13Hash, secret: &[u8], label: &str, context: &[u8], len: usize) -> Vec<u8> {
    let mut full_label = b"tls13 ".to_vec();
    full_label.extend_from_slice(label.as_bytes());
    let mut hkdf_label = Vec::new();
    hkdf_label.extend_from_slice(&(len as u16).to_be_bytes());
    hkdf_label.push(full_label.len() as u8);
    hkdf_label.extend_from_slice(&full_label);
    hkdf_label.push(context.len() as u8);
    hkdf_label.extend_from_slice(context);
    hkdf_expand(hash, secret, &hkdf_label, len)
}

fn hkdf_expand(hash: Tls13Hash, prk: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    use hmac::Mac;
    let mut out = Vec::with_capacity(len);
    let mut t = Vec::new();
    let mut counter = 1u8;
    while out.len() < len {
        let block = match hash {
            Tls13Hash::Sha256 => {
                let mut mac =
                    <hmac::Hmac<Sha256> as hmac::digest::KeyInit>::new_from_slice(prk).expect("hmac");
                mac.update(&t);
                mac.update(info);
                mac.update(&[counter]);
                mac.finalize().into_bytes().to_vec()
            }
            Tls13Hash::Sha384 => {
                let mut mac =
                    <hmac::Hmac<Sha384> as hmac::digest::KeyInit>::new_from_slice(prk).expect("hmac");
                mac.update(&t);
                mac.update(info);
                mac.update(&[counter]);
                mac.finalize().into_bytes().to_vec()
            }
        };
        t = block.clone();
        out.extend_from_slice(&block);
        counter += 1;
    }
    out.truncate(len);
    out
}

fn build_finished(hash: Tls13Hash, client_hs_secret: &[u8; 32], transcript: &[u8]) -> Vec<u8> {
    let th = transcript_hash(hash, transcript);
    let finished_key = expand_label(hash, client_hs_secret, "finished", &[], 32);
    use hmac::Mac;
    let verify = match hash {
        Tls13Hash::Sha256 => {
            let mut mac = <hmac::Hmac<Sha256> as hmac::digest::KeyInit>::new_from_slice(&finished_key)
                .expect("hmac");
            mac.update(&th);
            mac.finalize().into_bytes().to_vec()
        }
        Tls13Hash::Sha384 => {
            let mut mac = <hmac::Hmac<Sha384> as hmac::digest::KeyInit>::new_from_slice(&finished_key)
                .expect("hmac");
            mac.update(&th);
            mac.finalize().into_bytes().to_vec()
        }
    };
    let mut hs = vec![0x14, 0x00, 0x00, 0x20];
    hs.extend_from_slice(&verify[..32]);
    hs
}

async fn emit_middlebox_ccs<S>(stream: &mut S) -> Result<()>
where
    S: AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    stream
        .write_all(&[0x14, 0x03, 0x03, 0x00, 0x01, 0x01])
        .await?;
    stream.flush().await?;
    Ok(())
}

async fn read_encrypted_record<S>(
    stream: &mut S,
    cipher: &Tls13Cipher,
    base_iv: &[u8; 12],
    seq: &mut u64,
) -> Result<(Vec<u8>, u8)>
where
    S: AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    loop {
        let mut hdr = [0u8; 5];
        stream.read_exact(&mut hdr).await?;
        if hdr[0] == 0x14 {
            let len = u16::from_be_bytes([hdr[3], hdr[4]]) as usize;
            let mut body = vec![0u8; len];
            stream.read_exact(&mut body).await?;
            continue;
        }
        let len = u16::from_be_bytes([hdr[3], hdr[4]]) as usize;
        let mut enc = vec![0u8; len];
        stream.read_exact(&mut enc).await?;
        let plain = cipher.decrypt(base_iv, *seq, &hdr, &enc)?;
        *seq += 1;
        let content_type = plain.last().copied().unwrap_or(0x16);
        let inner = plain[..plain.len().saturating_sub(1)].to_vec();
        return Ok((inner, content_type));
    }
}

async fn write_encrypted_handshake<S>(
    stream: &mut S,
    cipher: &Tls13Cipher,
    base_iv: &[u8; 12],
    mut seq: u64,
    inner: &[u8],
) -> Result<()>
where
    S: AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let mut i = 0usize;
    while i + 4 <= inner.len() {
        let len = u32::from_be_bytes([0, inner[i + 1], inner[i + 2], inner[i + 3]]) as usize;
        let end = i + 4 + len;
        if end > inner.len() {
            break;
        }
        let msg = &inner[i..end];
        let enc = cipher.encrypt_handshake(base_iv, seq, msg)?;
        stream.write_all(&enc).await?;
        seq += 1;
        i = end;
    }
    stream.flush().await?;
    Ok(())
}

/// REALITY/sing-box: coalesce encrypted handshake into one TLS record (seq stays 0).
async fn write_encrypted_handshake_combined<S>(
    stream: &mut S,
    cipher: &Tls13Cipher,
    base_iv: &[u8; 12],
    inner: &[u8],
) -> Result<()>
where
    S: AsyncWrite + Unpin,
{
    use tokio::io::AsyncWriteExt;
    let enc = cipher.encrypt_handshake(base_iv, 0, inner)?;
    stream.write_all(&enc).await?;
    stream.flush().await?;
    Ok(())
}

fn poll_read_app_plain<C: Aead>(
    tcp: &mut TcpStream,
    cx: &mut TaskContext<'_>,
    cipher: &C,
    base_iv: &[u8; 12],
    read_seq: &mut u64,
    read_hdr: &mut [u8; 5],
    read_hdr_len: &mut u8,
) -> Poll<std::io::Result<Vec<u8>>> {
    let mut tcp = Pin::new(tcp);
    loop {
        while *read_hdr_len < 5 {
            let mut rb = ReadBuf::new(&mut read_hdr[*read_hdr_len as usize..]);
            match tcp.as_mut().poll_read(cx, &mut rb) {
                Poll::Ready(Ok(())) => {
                    if rb.filled().is_empty() {
                        return Poll::Ready(Err(std::io::Error::from(
                            std::io::ErrorKind::UnexpectedEof,
                        )));
                    }
                    *read_hdr_len += rb.filled().len() as u8;
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        if read_hdr[0] == 0x14 {
            let len = u16::from_be_bytes([read_hdr[3], read_hdr[4]]) as usize;
            let mut body = vec![0u8; len];
            let mut filled = 0usize;
            while filled < len {
                let mut rb = ReadBuf::new(&mut body[filled..]);
                match tcp.as_mut().poll_read(cx, &mut rb) {
                    Poll::Ready(Ok(())) => {
                        if rb.filled().is_empty() {
                            return Poll::Ready(Err(std::io::Error::from(
                                std::io::ErrorKind::UnexpectedEof,
                            )));
                        }
                        filled += rb.filled().len();
                    }
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                    Poll::Pending => return Poll::Pending,
                }
            }
            *read_hdr_len = 0;
            continue;
        }
        if read_hdr[0] != 0x17 {
            return Poll::Ready(Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("expected application data record, got 0x{:02x}", read_hdr[0]),
            )));
        }
        let len = u16::from_be_bytes([read_hdr[3], read_hdr[4]]) as usize;
        let mut enc = vec![0u8; len];
        let mut filled = 0usize;
        while filled < len {
            let mut rb = ReadBuf::new(&mut enc[filled..]);
            match tcp.as_mut().poll_read(cx, &mut rb) {
                Poll::Ready(Ok(())) => {
                    if rb.filled().is_empty() {
                        return Poll::Ready(Err(std::io::Error::from(
                            std::io::ErrorKind::UnexpectedEof,
                        )));
                    }
                    filled += rb.filled().len();
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
        let hdr = *read_hdr;
        *read_hdr_len = 0;
        let plain = decrypt_record(cipher, base_iv, *read_seq, &hdr, &enc)
            .map_err(std::io::Error::other)?;
        *read_seq += 1;
        let inner = if plain.last() == Some(&0x17) {
            plain[..plain.len() - 1].to_vec()
        } else {
            plain
        };
        return Poll::Ready(Ok(inner));
    }
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
        let inner = match {
            let this = self.as_mut().get_mut();
            poll_read_app_plain(
                &mut this.tcp,
                cx,
                &this.read_cipher,
                &this.read_iv,
                &mut this.read_seq,
                &mut this.read_hdr,
                &mut this.read_hdr_len,
            )
        } {
            Poll::Ready(Ok(v)) => v,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
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

async fn read_tls_record<S>(stream: &mut S) -> Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    use tokio::io::AsyncReadExt;
    let mut hdr = [0u8; 5];
    stream.read_exact(&mut hdr).await?;
    let len = u16::from_be_bytes([hdr[3], hdr[4]]) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    let mut out = hdr.to_vec();
    out.extend_from_slice(&body);
    Ok(out)
}

fn parse_server_hello(record: &[u8]) -> Result<([u8; 32], u16)> {
    let hs = &record[5..];
    anyhow::ensure!(
        hs.len() > 4 && hs[0] == 0x02,
        "expected ServerHello, got record type {} hs type {}",
        record.first().copied().unwrap_or(0),
        hs.first().copied().unwrap_or(0)
    );
    let mut i = 4 + 2 + 32;
    let sess_len = hs[i] as usize;
    i += 1 + sess_len;
    anyhow::ensure!(i + 3 <= hs.len(), "server hello truncated");
    let cipher = u16::from_be_bytes([hs[i], hs[i + 1]]);
    i += 2 + 1; // cipher suite + compression
    if i + 2 > hs.len() {
        anyhow::bail!("server hello missing extensions");
    }
    let ext_len = u16::from_be_bytes([hs[i], hs[i + 1]]) as usize;
    i += 2;
    let end = i + ext_len;
    while i + 4 <= end && i + 4 <= hs.len() {
        let ext_type = u16::from_be_bytes([hs[i], hs[i + 1]]);
        let ext_data_len = u16::from_be_bytes([hs[i + 2], hs[i + 3]]) as usize;
        i += 4;
        let ext_data = hs
            .get(i..i + ext_data_len)
            .context("server hello extension truncated")?;
        if ext_type == 0x0033 && ext_data.len() >= 4 {
            let group = u16::from_be_bytes([ext_data[0], ext_data[1]]);
            let key_len = u16::from_be_bytes([ext_data[2], ext_data[3]]) as usize;
            if group == 0x001d && key_len == 32 && ext_data.len() >= 4 + key_len {
                let mut pk = [0u8; 32];
                pk.copy_from_slice(&ext_data[4..4 + key_len]);
                return Ok((pk, cipher));
            }
        }
        i += ext_data_len;
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

fn decrypt_record<C: Aead>(
    cipher: &C,
    base_iv: &[u8; 12],
    seq: u64,
    aad: &[u8],
    enc: &[u8],
) -> Result<Vec<u8>> {
    let nonce = record_nonce(base_iv, seq);
    cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: enc,
                aad,
            },
        )
        .map_err(|e| anyhow::anyhow!("decrypt: {e}"))
}

fn encrypt_handshake<C: Aead>(
    cipher: &C,
    base_iv: &[u8; 12],
    seq: u64,
    inner: &[u8],
) -> Result<Vec<u8>> {
    let mut payload = inner.to_vec();
    // TLS 1.3: inner content type is handshake (0x16); outer record type is 0x17.
    payload.push(0x17);
    encrypt_record(cipher, base_iv, seq, &payload)
}

/// TLS 1.3 transcript ClientHello bytes for key derivation.
///
/// REALITY uses zeroed legacy session id in the TLS transcript (same as session
/// decrypt AAD) while the wire record carries the sealed blob.
pub(crate) fn client_hello_tls_transcript(record: &[u8]) -> Vec<u8> {
    let mut hs = record[5..].to_vec();
    zero_legacy_session_id_transcript(&mut hs, record);
    hs
}

fn zero_legacy_session_id_transcript(hs: &mut [u8], record: &[u8]) {
    let Some(layout) = crate::utls::hello_layout(record) else {
        return;
    };
    let off = layout.session_id_offset.saturating_sub(5);
    if off == 0 || off >= hs.len() {
        return;
    }
    let sid_len = hs[off - 1] as usize;
    if sid_len == 32 && off + sid_len <= hs.len() {
        hs[off..off + sid_len].fill(0);
    }
}

fn encrypt_record<C: Aead>(
    cipher: &C,
    base_iv: &[u8; 12],
    seq: u64,
    plain: &[u8],
) -> Result<Vec<u8>> {
    let ct_len = plain.len() + 16;
    let hdr = [
        0x17,
        0x03,
        0x03,
        (ct_len >> 8) as u8,
        (ct_len & 0xff) as u8,
    ];
    let nonce = record_nonce(base_iv, seq);
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: &hdr,
            },
        )
        .map_err(|e| anyhow::anyhow!("encrypt: {e}"))?;
    let mut out = hdr.to_vec();
    out.extend_from_slice(&ct);
    Ok(out)
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

pub mod server {
    //! TLS 1.3 server handshake for REALITY inbound.

    use super::*;
    use crate::reality_cert::{build_encrypted_extensions, build_reality_cert_material};
    use crate::utls::hello::parse_client_hello_alpn;
    use crate::reality_session::VerifiedSession;
    use rand::RngCore;
    use std::pin::Pin;
    use std::task::{Context as TaskContext, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    use tokio::net::TcpStream;
    use x25519_dalek::{PublicKey, StaticSecret};

    pub struct RealityServerStream {
        tcp: TcpStream,
        read_cipher: Tls13Cipher,
        write_cipher: Tls13Cipher,
        read_iv: [u8; 12],
        write_iv: [u8; 12],
        read_seq: u64,
        write_seq: u64,
        read_buf: Vec<u8>,
        read_hdr: [u8; 5],
        read_hdr_len: u8,
    }

    pub async fn accept_reality(
        stream: TcpStream,
        session: &VerifiedSession,
        _server_secret: StaticSecret,
    ) -> Result<RealityServerStream> {
        let mut eph_sk = [0u8; 32];
        rand::rng().fill_bytes(&mut eph_sk);
        let eph_secret = StaticSecret::from(eph_sk);
        let eph_pub = PublicKey::from(&eph_secret);
        let server_hello =
            build_server_hello(&session.client_hello, session.cipher, eph_pub.as_bytes())?;
        accept_reality_core(
            stream,
            session,
            server_hello,
            session.cipher,
            eph_secret,
            false,
        )
        .await
    }

    /// REALITY with dest-mirrored ServerHello (sing-box / uTLS compatible).
    pub async fn accept_reality_mirror(
        stream: TcpStream,
        session: &VerifiedSession,
        mirror: &crate::reality_mirror::MirroredHello,
    ) -> Result<RealityServerStream> {
        accept_reality_core(
            stream,
            session,
            mirror.server_hello.clone(),
            mirror.cipher,
            mirror.eph_secret.clone(),
            true,
        )
        .await
    }

    async fn accept_reality_core(
        mut stream: TcpStream,
        session: &VerifiedSession,
        server_hello: Vec<u8>,
        cipher: u16,
        eph_secret: StaticSecret,
        mirrored: bool,
    ) -> Result<RealityServerStream> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        stream
            .write_all(&server_hello)
            .await
            .context("write server hello")?;
        stream.flush().await.context("flush server hello")?;
        super::emit_middlebox_ccs(&mut stream)
            .await
            .context("write server change cipher spec")?;

        let mut transcript = super::client_hello_tls_transcript(&session.client_hello);
        transcript.extend_from_slice(&server_hello[5..]);

        let shared = eph_secret.diffie_hellman(&PublicKey::from(session.client_pub));
        let hash = cipher_hash(cipher);
        let th = transcript_hash(hash, &transcript);
        let key_len = cipher_key_len(cipher);
        let hs = derive_handshake_keys(hash, shared.as_bytes(), &th, key_len)?;

        // `derive_handshake_keys` uses client perspective: read=server, write=client.
        let write_cipher = match cipher {
            TLS13_AES_128_GCM_SHA256 => Tls13Cipher::Aes128(
                Aes128Gcm::new_from_slice(&hs.read_key).context("server write cipher")?,
            ),
            TLS13_AES_256_GCM_SHA384 => Tls13Cipher::Aes256(
                Aes256Gcm::new_from_slice(&hs.read_key).context("server write cipher")?,
            ),
            other => anyhow::bail!("unsupported cipher suite {other:#x}"),
        };
        let read_cipher = match cipher {
            TLS13_AES_128_GCM_SHA256 => Tls13Cipher::Aes128(
                Aes128Gcm::new_from_slice(&hs.write_key).context("server read cipher")?,
            ),
            TLS13_AES_256_GCM_SHA384 => Tls13Cipher::Aes256(
                Aes256Gcm::new_from_slice(&hs.write_key).context("server read cipher")?,
            ),
            other => anyhow::bail!("unsupported cipher suite {other:#x}"),
        };

        let mut server_msgs =
            build_encrypted_extensions(parse_client_hello_alpn(&session.client_hello).as_deref());
        let cert_material = build_reality_cert_material(&session.auth_key);
        server_msgs.extend_from_slice(&cert_material.cert_message);
        let mut transcript_with_cert = transcript.clone();
        transcript_with_cert.extend_from_slice(&server_msgs);
        let cert_verify = build_certificate_verify(hash, &transcript_with_cert, &cert_material)?;
        server_msgs.extend_from_slice(&cert_verify);
        transcript_with_cert.extend_from_slice(&cert_verify);
        let server_traffic: [u8; 32] = expand_label(hash, &hs.hs_secret, "s hs traffic", &th, 32)
            .try_into()
            .unwrap();
        let server_finished = build_finished(hash, &server_traffic, &transcript_with_cert);
        server_msgs.extend_from_slice(&server_finished);
        let enc_flight = if mirrored && session.client_hello.len() > 400 {
            write_encrypted_handshake_combined(&mut stream, &write_cipher, &hs.read_iv, &server_msgs)
                .await
        } else {
            write_encrypted_handshake(&mut stream, &write_cipher, &hs.read_iv, 0, &server_msgs)
                .await
        };
        enc_flight.context("write server encrypted flight")?;
        transcript.extend_from_slice(&server_msgs);

        // REALITY server (Xray/sing-box): derive app keys after ServerFinished without
        // waiting for client Finished. Client Finished may still arrive encrypted; drain it.
        let app = derive_application_keys(hash, &hs.hs_secret, &transcript, key_len)?;
        let _ = drain_client_finished(&mut stream, &read_cipher, &hs.write_iv).await;
        let read_cipher = match cipher {
            TLS13_AES_128_GCM_SHA256 => Tls13Cipher::Aes128(
                Aes128Gcm::new_from_slice(&app.write_key).context("app read cipher")?,
            ),
            TLS13_AES_256_GCM_SHA384 => Tls13Cipher::Aes256(
                Aes256Gcm::new_from_slice(&app.write_key).context("app read cipher")?,
            ),
            other => anyhow::bail!("unsupported cipher suite {other:#x}"),
        };
        let write_cipher = match cipher {
            TLS13_AES_128_GCM_SHA256 => Tls13Cipher::Aes128(
                Aes128Gcm::new_from_slice(&app.read_key).context("app write cipher")?,
            ),
            TLS13_AES_256_GCM_SHA384 => Tls13Cipher::Aes256(
                Aes256Gcm::new_from_slice(&app.read_key).context("app write cipher")?,
            ),
            other => anyhow::bail!("unsupported cipher suite {other:#x}"),
        };
        Ok(RealityServerStream {
            tcp: stream,
            read_cipher,
            write_cipher,
            read_iv: app.write_iv,
            write_iv: app.read_iv,
            read_seq: 0,
            write_seq: 0,
            read_buf: Vec::new(),
            read_hdr: [0u8; 5],
            read_hdr_len: 0,
        })
    }

    fn verify_client_finished(
        hash: Tls13Hash,
        client_hs_secret: &[u8; 32],
        transcript: &[u8],
        finished: &[u8],
    ) -> Result<()> {
        let expected = build_finished(hash, client_hs_secret, transcript);
        anyhow::ensure!(
            finished == expected,
            "client Finished verification failed"
        );
        Ok(())
    }

    fn extract_handshake_message(msgs: &[u8], msg_type: u8) -> Option<&[u8]> {
        let mut i = 0usize;
        while i + 4 <= msgs.len() {
            let ty = msgs[i];
            let len = u32::from_be_bytes([0, msgs[i + 1], msgs[i + 2], msgs[i + 3]]) as usize;
            let end = i + 4 + len;
            if end > msgs.len() {
                break;
            }
            let msg = &msgs[i..end];
            if ty == msg_type {
                return Some(msg);
            }
            i = end;
        }
        None
    }

    async fn read_encrypted_handshake<S>(
        stream: &mut S,
        cipher: &Tls13Cipher,
        base_iv: &[u8; 12],
    ) -> Result<Vec<u8>>
    where
        S: AsyncRead + Unpin,
    {
        let mut seq = 0u64;
        let mut msgs = Vec::new();
        for _ in 0..8 {
            let (plain, _ct) = read_encrypted_record(stream, cipher, base_iv, &mut seq).await?;
            if plain.is_empty() {
                continue;
            }
            msgs.extend_from_slice(&plain);
            if handshake_contains(&msgs, 0x14) {
                break;
            }
        }
        anyhow::ensure!(
            handshake_contains(&msgs, 0x14),
            "client Finished missing"
        );
        Ok(msgs)
    }

    /// Best-effort drain of client Finished (+ optional middlebox CCS) after REALITY server flight.
    async fn drain_client_finished<S>(
        stream: &mut S,
        cipher: &Tls13Cipher,
        base_iv: &[u8; 12],
    ) -> Result<()>
    where
        S: AsyncRead + Unpin,
    {
        use tokio::time::{timeout, Duration};
        let _ = timeout(
            Duration::from_secs(2),
            read_encrypted_handshake(stream, cipher, base_iv),
        )
        .await;
        Ok(())
    }

    fn build_certificate_verify(
        hash: Tls13Hash,
        transcript: &[u8],
        cert_material: &crate::reality_cert::RealityCertMaterial,
    ) -> Result<Vec<u8>> {
        let th = transcript_hash(hash, transcript);
        let mut ctx = vec![0x20u8; 64];
        ctx.extend_from_slice(b"TLS 1.3, server CertificateVerify");
        ctx.push(0);
        ctx.extend_from_slice(&th);
        let sig_bytes = cert_material.sign(&ctx);
        let mut body = Vec::new();
        body.extend_from_slice(&[0x08, 0x07]); // Ed25519
        body.extend_from_slice(&(sig_bytes.len() as u16).to_be_bytes());
        body.extend_from_slice(&sig_bytes);
        let mut hs = Vec::with_capacity(4 + body.len());
        hs.push(0x0f);
        hs.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
        hs.extend_from_slice(&body);
        Ok(hs)
    }

    fn build_server_hello(
        client_hello: &[u8],
        cipher: u16,
        server_pub: &[u8; 32],
    ) -> Result<Vec<u8>> {
        let layout = crate::utls::hello_layout(client_hello).context("client hello layout")?;
        let sid_len = client_hello[layout.session_id_offset - 1] as usize;
        anyhow::ensure!(
            sid_len == 32,
            "reality server hello: expected 32-byte legacy session id, got {sid_len}"
        );
        let session_id =
            &client_hello[layout.session_id_offset..layout.session_id_offset + sid_len];

        let mut random = [0u8; 32];
        rand::rng().fill_bytes(&mut random);
        let mut hs = Vec::new();
        hs.push(0x02);
        hs.extend_from_slice(&[0x00, 0x00, 0x00]); // placeholder length
        hs.extend_from_slice(&[0x03, 0x03]);
        hs.extend_from_slice(&random);
        hs.push(sid_len as u8);
        hs.extend_from_slice(session_id);
        hs.extend_from_slice(&cipher.to_be_bytes());
        hs.push(0x00); // compression
        let mut exts = Vec::new();
        // Go crypto/tls / sing-box REALITY: supported_versions before key_share.
        exts.extend_from_slice(&[0x00, 0x2b, 0x00, 0x02, 0x03, 0x04]);
        exts.extend_from_slice(&[0x00, 0x33, 0x00, 0x24]);
        exts.extend_from_slice(&[0x00, 0x1d, 0x00, 0x20]);
        exts.extend_from_slice(server_pub);
        hs.extend_from_slice(&(exts.len() as u16).to_be_bytes());
        hs.extend_from_slice(&exts);
        let body_len = hs.len() - 4;
        hs[1] = ((body_len >> 16) & 0xff) as u8;
        hs[2] = ((body_len >> 8) & 0xff) as u8;
        hs[3] = (body_len & 0xff) as u8;
        let mut record = vec![0x16, 0x03, 0x03];
        let rec_len = hs.len() as u16;
        record.extend_from_slice(&rec_len.to_be_bytes());
        record.extend_from_slice(&hs);
        Ok(record)
    }

    impl AsyncRead for RealityServerStream {
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
        let inner = {
            let this = self.as_mut().get_mut();
            match &mut this.read_cipher {
                Tls13Cipher::Aes128(c) => super::poll_read_app_plain(
                    &mut this.tcp,
                    cx,
                    c,
                    &this.read_iv,
                    &mut this.read_seq,
                    &mut this.read_hdr,
                    &mut this.read_hdr_len,
                ),
                Tls13Cipher::Aes256(c) => super::poll_read_app_plain(
                    &mut this.tcp,
                    cx,
                    c,
                    &this.read_iv,
                    &mut this.read_seq,
                    &mut this.read_hdr,
                    &mut this.read_hdr_len,
                ),
            }
        };
        let inner = match inner {
                Poll::Ready(Ok(v)) => v,
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            };
            let n = inner.len().min(buf.remaining());
            buf.put_slice(&inner[..n]);
            if n < inner.len() {
                self.read_buf.extend_from_slice(&inner[n..]);
            }
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for RealityServerStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            let mut payload = buf.to_vec();
            payload.push(0x17);
            let enc = self
                .write_cipher
                .encrypt_record(&self.write_iv, self.write_seq, &payload)
                .map_err(std::io::Error::other)?;
            self.write_seq += 1;
            match Pin::new(&mut self.tcp).poll_write(cx, &enc) {
                Poll::Ready(Ok(n)) if n == enc.len() => Poll::Ready(Ok(buf.len())),
                Poll::Ready(Ok(_)) => Poll::Ready(Err(std::io::Error::other("short tls write"))),
                other => other,
            }
        }

        fn poll_flush(
            mut self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.tcp).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut TaskContext<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.tcp).poll_shutdown(cx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls13_key_schedule_matches_reference() {
        let shared: [u8; 32] = [
            0x89, 0xad, 0x06, 0xaf, 0xda, 0x83, 0x72, 0x83, 0x2d, 0x21, 0xcf, 0xa2, 0xbe, 0xb2,
            0x2b, 0xcf, 0x23, 0x9b, 0xd7, 0x65, 0x64, 0x48, 0x14, 0x9b, 0xba, 0xc8, 0x6e, 0x3f,
            0x54, 0xb4, 0x6d, 0x73,
        ];
        let th: [u8; 32] = [
            0xd5, 0xcf, 0xfe, 0x82, 0xe0, 0xc3, 0x42, 0x86, 0x75, 0xe0, 0xd9, 0x9a, 0x19, 0xab,
            0x3e, 0x6b, 0xe0, 0x1d, 0xea, 0xfb, 0x0b, 0x14, 0x6b, 0xdb, 0xde, 0x33, 0xef, 0xbe,
            0xee, 0x74, 0x9e, 0x21,
        ];
        let hs = derive_handshake_keys(Tls13Hash::Sha256, &shared, &th, 16).unwrap();
        assert_eq!(
            hs.read_key,
            [
                0xe8, 0xf5, 0x3f, 0x0e, 0x42, 0xc5, 0xe1, 0x25, 0x18, 0x03, 0x41, 0x31, 0x74, 0x68,
                0xf1, 0x25,
            ]
        );
        assert_eq!(
            hs.read_iv,
            [
                0x9e, 0x00, 0xd9, 0x8c, 0xd0, 0x56, 0x9e, 0x41, 0x54, 0xd0, 0x10, 0xfa,
            ]
        );
    }

    #[test]
    fn reality_transcript_zeroes_session_id() {
        let mut record = vec![0u8; 280];
        record[0] = 0x16;
        record[1] = 0x03;
        record[2] = 0x01;
        let hs_len = (record.len() - 5) as u16;
        record[3] = (hs_len >> 8) as u8;
        record[4] = (hs_len & 0xff) as u8;
        record[5] = 0x01;
        record[9] = 0x03;
        record[10] = 0x03;
        record[43] = 32;
        for i in 0..32 {
            record[44 + i] = (i + 1) as u8;
        }
        let t = client_hello_tls_transcript(&record);
        assert_eq!(t[38], 32);
        assert!(t[39..71].iter().all(|&b| b == 0));
    }

    #[tokio::test]
    async fn reality_handshake_roundtrip() {
        use crate::reality::{patch_reality_session, RealityConfig};
        use crate::reality_session::verify_reality_session;
        use crate::utls::hello::{generate_client_hello, Profile};
        use crate::utls::server::accept_reality;
        use tokio::net::{TcpListener, TcpStream};
        use x25519_dalek::{PublicKey, StaticSecret};

        let server_secret = StaticSecret::from([0x11u8; 32]);
        let server_pub = PublicKey::from(&server_secret);
        let cfg = RealityConfig {
            public_key: server_pub,
            short_id: [0xa1, 0xb2, 0xc3, 0xd4, 0, 0, 0, 0],
            server_name: "www.cloudflare.com".into(),
            fingerprint: Profile::Chrome,
        };
        let keys = generate_client_hello(Profile::Chrome, "www.cloudflare.com");
        let (hello, auth_key) =
            patch_reality_session(&keys, &cfg, &serde_json::json!({"reality": {}})).unwrap();
        let short_ids = [[0xa1, 0xb2, 0xc3, 0xd4, 0, 0, 0, 0]];
        let session =
            verify_reality_session(&hello, &server_secret, &short_ids, 120).expect("verify session");

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let expected_hello = session.client_hello.clone();
        let server_secret2 = server_secret.clone();
        let server_task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let on_wire = super::read_tls_record(&mut stream)
                .await
                .expect("read client hello");
            assert_eq!(on_wire, expected_hello);
            accept_reality(stream, &session, server_secret2)
                .await
                .expect("accept reality");
        });

        let mut client = TcpStream::connect(addr).await.unwrap();
        complete_tls13_camouflage(&mut client, &hello, keys.secret, Some(auth_key))
            .await
            .expect("client handshake");
        server_task.await.unwrap();
    }

    #[tokio::test]
    #[ignore = "live VPS"]
    async fn reality_live_vless_via_proxy_conn() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use uuid::Uuid;
        let tls = serde_json::json!({
            "enabled": true,
            "server_name": "www.cloudflare.com",
            "utls": { "enabled": true, "fingerprint": "chrome" },
            "reality": {
                "enabled": true,
                "public_key": "VdvA1In4Po7ugbQHYIm518Vw5u72SFokjyTY4XwByRw",
                "short_id": "a1b2c3d4"
            }
        });
        let mut stream = rsb_core::proxy_box(
            crate::reality::connect("s.lulunet.cc", 8447, Some(&tls), Some("www.cloudflare.com"))
                .await
                .expect("reality connect"),
        );
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let dest: std::net::SocketAddr = "1.1.1.1:443".parse().unwrap();
        let mut header = vec![0];
        header.extend_from_slice(uuid.as_bytes());
        header.push(0);
        header.push(1);
        header.extend_from_slice(&dest.port().to_be_bytes());
        header.push(1);
        if let std::net::IpAddr::V4(v4) = dest.ip() {
            header.extend_from_slice(&v4.octets());
        }
        stream.write_all(&header).await.expect("write vless");
        let mut resp = [0u8; 2];
        stream.read_exact(&mut resp).await.expect("read vless response");
        assert_eq!(resp, [0, 0]);
    }

    #[tokio::test]
    #[ignore = "live VPS"]
    async fn reality_live_vless_roundtrip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use uuid::Uuid;
        let tls = serde_json::json!({
            "enabled": true,
            "server_name": "www.cloudflare.com",
            "utls": { "enabled": true, "fingerprint": "chrome" },
            "reality": {
                "enabled": true,
                "public_key": "VdvA1In4Po7ugbQHYIm518Vw5u72SFokjyTY4XwByRw",
                "short_id": "a1b2c3d4"
            }
        });
        let mut stream = crate::reality::connect(
            "s.lulunet.cc",
            8447,
            Some(&tls),
            Some("www.cloudflare.com"),
        )
        .await
        .expect("reality connect");
        let uuid = Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        let dest: std::net::SocketAddr = "1.1.1.1:443".parse().unwrap();
        let mut header = vec![0];
        header.extend_from_slice(uuid.as_bytes());
        header.push(0); // addon len
        header.push(1); // tcp
        header.extend_from_slice(&dest.port().to_be_bytes());
        match dest.ip() {
            std::net::IpAddr::V4(v4) => {
                header.push(1);
                header.extend_from_slice(&v4.octets());
            }
            std::net::IpAddr::V6(v6) => {
                header.push(3);
                header.extend_from_slice(&v6.octets());
            }
        }
        stream.write_all(&header).await.expect("write vless");
        let mut resp = [0u8; 2];
        stream
            .read_exact(&mut resp)
            .await
            .expect("read vless response");
        assert_eq!(resp, [0, 0]);
    }

    #[tokio::test]
    #[ignore = "live VPS"]
    async fn reality_live_vps_handshake() {
        use std::time::Duration;
        let tls = serde_json::json!({
            "enabled": true,
            "server_name": "www.cloudflare.com",
            "utls": { "enabled": true, "fingerprint": "chrome" },
            "reality": {
                "enabled": true,
                "public_key": "VdvA1In4Po7ugbQHYIm518Vw5u72SFokjyTY4XwByRw",
                "short_id": "a1b2c3d4"
            }
        });
        let result = tokio::time::timeout(
            Duration::from_secs(20),
            crate::reality::connect("s.lulunet.cc", 8447, Some(&tls), Some("www.cloudflare.com")),
        )
        .await;
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => panic!("reality live handshake failed: {err:#}"),
            Err(_) => panic!("reality live handshake timed out after 20s"),
        }
    }
}
