//! VMess AEAD header (sing-vmess / v2fly compatible).

use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes128;
use aes_gcm::aead::{Aead, KeyInit as AeadKeyInit, Payload};
use aes_gcm::{Aes128Gcm, Nonce};
use anyhow::{Context, Result};
use crc::{Crc, CRC_32_ISO_HDLC};
use rand::RngCore;
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

use crate::transport::encode_port_first_socksaddr;

const KDF_AUTH_ID: &str = "AES Auth ID Encryption";
const KDF_HDR_LEN_KEY: &str = "VMess Header AEAD Key_Length";
const KDF_HDR_LEN_IV: &str = "VMess Header AEAD Nonce_Length";
const KDF_HDR_KEY: &str = "VMess Header AEAD Key";
const KDF_HDR_IV: &str = "VMess Header AEAD Nonce";
const KDF_RESP_LEN_KEY: &str = "AEAD Resp Header Len Key";
const KDF_RESP_LEN_IV: &str = "AEAD Resp Header Len IV";
const KDF_RESP_KEY: &str = "AEAD Resp Header Key";
const KDF_RESP_IV: &str = "AEAD Resp Header IV";

const SECURITY_NONE: u8 = 5;
const CMD_TCP: u8 = 1;

trait HashFactory: Send + Sync {
    fn new_hasher(&self) -> Box<dyn HashState>;
}

trait HashState: Send {
    fn reset(&mut self);
    fn update(&mut self, data: &[u8]);
    fn finalize(&mut self) -> [u8; 32];
}

struct Sha256Factory;

impl HashFactory for Sha256Factory {
    fn new_hasher(&self) -> Box<dyn HashState> {
        Box::new(Sha256State(Sha256::new()))
    }
}

struct Sha256State(Sha256);

impl HashState for Sha256State {
    fn reset(&mut self) {
        self.0 = Sha256::new();
    }
    fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }
    fn finalize(&mut self) -> [u8; 32] {
        std::mem::take(&mut self.0).finalize().into()
    }
}

struct GoHmac {
    factory: Arc<dyn HashFactory>,
    key: Vec<u8>,
    ipad: [u8; 64],
    opad: [u8; 64],
    inner: Box<dyn HashState>,
    outer: Box<dyn HashState>,
}

impl GoHmac {
    fn new(factory: Arc<dyn HashFactory>, key: Vec<u8>) -> Self {
        let (ipad, opad) = ipad_opad(&key);
        let mut inner = factory.new_hasher();
        inner.reset();
        inner.update(&ipad);
        let mut outer = factory.new_hasher();
        outer.reset();
        Self {
            factory,
            key,
            ipad,
            opad,
            inner,
            outer,
        }
    }

    fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    fn sum(mut self) -> [u8; 32] {
        let inner_digest = self.inner.finalize();
        self.outer.reset();
        self.outer.update(&self.opad);
        self.outer.update(&inner_digest);
        self.outer.finalize()
    }
}

fn ipad_opad(key: &[u8]) -> ([u8; 64], [u8; 64]) {
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    let key_block = if key.len() > 64 {
        let mut h = Sha256State(Sha256::new());
        h.update(key);
        h.finalize().to_vec()
    } else {
        key.to_vec()
    };
    for (i, b) in key_block.iter().enumerate() {
        ipad[i] ^= b;
        opad[i] ^= b;
    }
    (ipad, opad)
}

struct HmacFactory {
    key: Vec<u8>,
    parent: Arc<dyn HashFactory>,
}

impl HashFactory for HmacFactory {
    fn new_hasher(&self) -> Box<dyn HashState> {
        Box::new(HmacState {
            mac: GoHmac::new(self.parent.clone(), self.key.clone()),
        })
    }
}

struct HmacState {
    mac: GoHmac,
}

impl HashState for HmacState {
    fn reset(&mut self) {
        self.mac = GoHmac::new(self.mac.factory.clone(), self.mac.key.clone());
    }
    fn update(&mut self, data: &[u8]) {
        self.mac.update(data);
    }
    fn finalize(&mut self) -> [u8; 32] {
        let factory = self.mac.factory.clone();
        let key = self.mac.key.clone();
        let mac = std::mem::replace(&mut self.mac, GoHmac::new(factory, key));
        mac.sum()
    }
}

fn build_kdf_chain(path: &[&[u8]]) -> Arc<dyn HashFactory> {
    let mut current: Arc<dyn HashFactory> = Arc::new(HmacFactory {
        key: b"VMess AEAD KDF".to_vec(),
        parent: Arc::new(Sha256Factory),
    });
    for p in path {
        current = Arc::new(HmacFactory {
            key: p.to_vec(),
            parent: current,
        });
    }
    current
}

pub fn vmess_aead_kdf(key: &[u8], path: &[&[u8]]) -> [u8; 32] {
    let factory = build_kdf_chain(path);
    let mut h = factory.new_hasher();
    h.reset();
    h.update(key);
    h.finalize()
}

pub fn vmess_cmd_key(uuid: &Uuid) -> [u8; 16] {
    let mut data = Vec::with_capacity(16 + 36);
    data.extend_from_slice(uuid.as_bytes());
    data.extend_from_slice(b"c48619fe-8f02-49e0-b9e9-edf763e17e21");
    md5::compute(data).0
}

fn auth_id(key: [u8; 16], ts: u64) -> [u8; 16] {
    let mut buf = [0u8; 16];
    buf[..8].copy_from_slice(&ts.to_be_bytes());
    rand::rng().fill_bytes(&mut buf[8..12]);
    let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    let checksum = crc.checksum(&buf[..12]);
    buf[12..].copy_from_slice(&checksum.to_be_bytes());
    let aes_key: [u8; 16] = vmess_aead_kdf(&key, &[KDF_AUTH_ID.as_bytes()])[..16]
        .try_into()
        .unwrap();
    let cipher = Aes128::new_from_slice(&aes_key).expect("aes key");
    let mut block = aes::Block::from(buf);
    cipher.encrypt_block(&mut block);
    block.into()
}

fn seal_aead(key: &[u8], nonce: &[u8], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let cipher = Aes128Gcm::new_from_slice(key).expect("gcm key");
    cipher
        .encrypt(Nonce::from_slice(nonce), Payload { msg: plaintext, aad })
        .expect("gcm seal")
}

fn open_aead(key: &[u8], nonce: &[u8], ciphertext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let cipher = Aes128Gcm::new_from_slice(key).expect("gcm key");
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|e| anyhow::anyhow!("gcm open: {e}"))
}

fn encode_header(
    request_key: &[u8; 16],
    request_nonce: &[u8; 16],
    response_header: u8,
    dest: SocketAddr,
    command: u8,
) -> Vec<u8> {
    let padding_len = (rand::random::<u8>() % 16) as usize;
    let mut header = Vec::new();
    header.push(1);
    header.extend_from_slice(request_nonce);
    header.extend_from_slice(request_key);
    header.push(response_header);
    header.push(0);
    header.push((padding_len as u8) << 4 | SECURITY_NONE);
    header.push(0);
    header.push(command);
    header.extend_from_slice(&encode_port_first_socksaddr(dest));
    header.resize(header.len() + padding_len, 0);
    let checksum = fnv1a32(&header);
    header.extend_from_slice(&checksum.to_be_bytes());
    header
}

fn fnv1a32(data: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for b in data {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

pub async fn write_handshake(
    stream: &mut (impl AsyncWriteExt + Unpin),
    uuid: Uuid,
    dest: SocketAddr,
    command: u8,
) -> Result<([u8; 16], [u8; 16])> {
    let cmd_key = vmess_cmd_key(&uuid);
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("time")?
        .as_secs();
    let auth = auth_id(cmd_key, ts);
    let mut request_key = [0u8; 16];
    let mut request_nonce = [0u8; 16];
    rand::rng().fill_bytes(&mut request_key);
    rand::rng().fill_bytes(&mut request_nonce);
    let response_header = rand::random::<u8>();
    let header = encode_header(
        &request_key,
        &request_nonce,
        response_header,
        dest,
        command,
    );
    let mut connection_nonce = [0u8; 8];
    rand::rng().fill_bytes(&mut connection_nonce);

    let header_len = (header.len() as u16).to_be_bytes();
    let len_key: [u8; 16] = vmess_aead_kdf(
        &cmd_key,
        &[
            KDF_HDR_LEN_KEY.as_bytes(),
            &auth,
            &connection_nonce,
        ],
    )[..16]
        .try_into()
        .unwrap();
    let len_nonce: [u8; 12] = vmess_aead_kdf(
        &cmd_key,
        &[
            KDF_HDR_LEN_IV.as_bytes(),
            &auth,
            &connection_nonce,
        ],
    )[..12]
        .try_into()
        .unwrap();
    let len_cipher = seal_aead(&len_key, &len_nonce, &header_len, &auth);

    let hdr_key: [u8; 16] = vmess_aead_kdf(
        &cmd_key,
        &[
            KDF_HDR_KEY.as_bytes(),
            &auth,
            &connection_nonce,
        ],
    )[..16]
        .try_into()
        .unwrap();
    let hdr_nonce: [u8; 12] = vmess_aead_kdf(
        &cmd_key,
        &[
            KDF_HDR_IV.as_bytes(),
            &auth,
            &connection_nonce,
        ],
    )[..12]
        .try_into()
        .unwrap();
    let hdr_cipher = seal_aead(&hdr_key, &hdr_nonce, &header, &auth);

    let mut packet = Vec::with_capacity(16 + len_cipher.len() + 8 + hdr_cipher.len());
    packet.extend_from_slice(&auth);
    packet.extend_from_slice(&len_cipher);
    packet.extend_from_slice(&connection_nonce);
    packet.extend_from_slice(&hdr_cipher);
    stream.write_all(&packet).await?;
    Ok((request_key, request_nonce))
}

pub async fn read_response(
    stream: &mut (impl AsyncReadExt + Unpin),
    request_key: &[u8; 16],
    request_nonce: &[u8; 16],
) -> Result<()> {
    let response_key: [u8; 16] = Sha256::digest(request_key)[..16].try_into().unwrap();
    let response_nonce: [u8; 16] = Sha256::digest(request_nonce)[..16].try_into().unwrap();

    let len_key: [u8; 16] = vmess_aead_kdf(&response_key, &[KDF_RESP_LEN_KEY.as_bytes()])[..16]
        .try_into()
        .unwrap();
    let len_nonce: [u8; 12] =
        vmess_aead_kdf(&response_nonce, &[KDF_RESP_LEN_IV.as_bytes()])[..12]
            .try_into()
            .unwrap();
    let mut len_buf = [0u8; 18];
    stream.read_exact(&mut len_buf).await?;
    let len_plain = open_aead(&len_key, &len_nonce, &len_buf, &[])?;
    if len_plain.len() < 2 {
        anyhow::bail!("truncated vmess response length");
    }
    let header_len = u16::from_be_bytes([len_plain[0], len_plain[1]]) as usize;

    let hdr_key: [u8; 16] = vmess_aead_kdf(&response_key, &[KDF_RESP_KEY.as_bytes()])[..16]
        .try_into()
        .unwrap();
    let hdr_nonce: [u8; 12] = vmess_aead_kdf(&response_nonce, &[KDF_RESP_IV.as_bytes()])[..12]
        .try_into()
        .unwrap();
    let mut hdr_buf = vec![0u8; header_len + 16];
    stream.read_exact(&mut hdr_buf).await?;
    let _ = open_aead(&hdr_key, &hdr_nonce, &hdr_buf, &[])?;
    Ok(())
}

pub struct VmessAcceptResult {
    pub user: Uuid,
    pub dest: SocketAddr,
    pub request_key: [u8; 16],
    pub request_nonce: [u8; 16],
    pub response_header: u8,
    pub option: u8,
}

fn decode_auth_id(cmd_key: [u8; 16], auth: &[u8; 16]) -> Option<u64> {
    use aes::cipher::{BlockDecrypt, KeyInit};
    let aes_key: [u8; 16] = vmess_aead_kdf(&cmd_key, &[KDF_AUTH_ID.as_bytes()])[..16]
        .try_into()
        .ok()?;
    let cipher = Aes128::new_from_slice(&aes_key).ok()?;
    let mut block = aes::Block::from(*auth);
    cipher.decrypt_block(&mut block);
    let decoded: [u8; 16] = block.into();
    let crc = Crc::<u32>::new(&CRC_32_ISO_HDLC);
    if crc.checksum(&decoded[..12]) != u32::from_be_bytes(decoded[12..16].try_into().ok()?) {
        return None;
    }
    Some(u64::from_be_bytes(decoded[..8].try_into().ok()?))
}

fn auth_timestamp_valid(ts: u64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    ts.abs_diff(now) <= 120
}

fn parse_request_header(header: &[u8]) -> Result<(String, u16, [u8; 16], [u8; 16], u8, u8)> {
    if header.len() < 38 {
        anyhow::bail!("truncated vmess request header");
    }
    if header[0] != 1 {
        anyhow::bail!("unsupported vmess version {}", header[0]);
    }
    let mut request_nonce = [0u8; 16];
    request_nonce.copy_from_slice(&header[1..17]);
    let mut request_key = [0u8; 16];
    request_key.copy_from_slice(&header[17..33]);
    let response_header = header[33];
    let option = header[34];
    let padding_len = (header[35] >> 4) as usize;
    let command = header[37];
    if command != CMD_TCP {
        anyhow::bail!("unsupported vmess command {command}");
    }
    let body_end = header.len().checked_sub(4).context("vmess header fnv")?;
    let body = &header[..body_end];
    let checksum = fnv1a32(body);
    let expected = u32::from_be_bytes(header[body_end..body_end + 4].try_into()?);
    if checksum != expected {
        anyhow::bail!("vmess header checksum mismatch");
    }
    let addr_end = header.len().checked_sub(4 + padding_len).context("vmess padding")?;
    if addr_end <= 38 {
        anyhow::bail!("truncated vmess address");
    }
    let port = u16::from_be_bytes([header[38], header[39]]);
    let atyp = header[40];
    let (host, _) = crate::vless::read_address(&header[41..addr_end], atyp)?;
    Ok((host, port, request_key, request_nonce, response_header, option))
}

pub async fn accept_handshake(
    stream: &mut (impl AsyncReadExt + AsyncWriteExt + Unpin),
    users: &[Uuid],
) -> Result<VmessAcceptResult> {
    let mut auth = [0u8; 16];
    stream.read_exact(&mut auth).await?;
    let mut cmd_key = None;
    let mut matched_user = None;
    let mut ts = None;
    for uid in users {
        let key = vmess_cmd_key(uid);
        if let Some(decoded_ts) = decode_auth_id(key, &auth) {
            if auth_timestamp_valid(decoded_ts) {
                cmd_key = Some(key);
                matched_user = Some(*uid);
                ts = Some(decoded_ts);
                break;
            }
        }
    }
    let cmd_key = cmd_key.context("vmess auth failed")?;
    let user = matched_user.context("vmess auth failed")?;

    let mut len_cipher = [0u8; 18];
    stream.read_exact(&mut len_cipher).await?;
    let mut connection_nonce = [0u8; 8];
    stream.read_exact(&mut connection_nonce).await?;

    let len_key: [u8; 16] = vmess_aead_kdf(
        &cmd_key,
        &[
            KDF_HDR_LEN_KEY.as_bytes(),
            &auth,
            &connection_nonce,
        ],
    )[..16]
        .try_into()
        .unwrap();
    let len_nonce: [u8; 12] = vmess_aead_kdf(
        &cmd_key,
        &[
            KDF_HDR_LEN_IV.as_bytes(),
            &auth,
            &connection_nonce,
        ],
    )[..12]
        .try_into()
        .unwrap();
    let len_plain = open_aead(&len_key, &len_nonce, &len_cipher, &auth)?;
    if len_plain.len() < 2 {
        anyhow::bail!("truncated vmess header length");
    }
    let header_len = u16::from_be_bytes([len_plain[0], len_plain[1]]) as usize;

    let hdr_key: [u8; 16] = vmess_aead_kdf(
        &cmd_key,
        &[
            KDF_HDR_KEY.as_bytes(),
            &auth,
            &connection_nonce,
        ],
    )[..16]
        .try_into()
        .unwrap();
    let hdr_nonce: [u8; 12] = vmess_aead_kdf(
        &cmd_key,
        &[
            KDF_HDR_IV.as_bytes(),
            &auth,
            &connection_nonce,
        ],
    )[..12]
        .try_into()
        .unwrap();
    let mut hdr_cipher = vec![0u8; header_len + 16];
    stream.read_exact(&mut hdr_cipher).await?;
    let header = open_aead(&hdr_key, &hdr_nonce, &hdr_cipher, &auth)?;
    let (host, port, request_key, request_nonce, response_header, option) =
        parse_request_header(&header)?;
    let dest = tokio::net::lookup_host(format!("{host}:{port}"))
        .await?
        .next()
        .with_context(|| format!("resolve {host}"))?;
    Ok(VmessAcceptResult {
        user,
        dest,
        request_key,
        request_nonce,
        response_header,
        option,
    })
}

pub async fn write_server_response(
    stream: &mut (impl AsyncWriteExt + Unpin),
    request_key: &[u8; 16],
    request_nonce: &[u8; 16],
    response_header: u8,
    option: u8,
) -> Result<()> {
    let response_key: [u8; 16] = Sha256::digest(request_key)[..16].try_into().unwrap();
    let response_nonce: [u8; 16] = Sha256::digest(request_nonce)[..16].try_into().unwrap();

    let len_key: [u8; 16] = vmess_aead_kdf(&response_key, &[KDF_RESP_LEN_KEY.as_bytes()])[..16]
        .try_into()
        .unwrap();
    let len_nonce: [u8; 12] =
        vmess_aead_kdf(&response_nonce, &[KDF_RESP_LEN_IV.as_bytes()])[..12]
            .try_into()
            .unwrap();
    let len_plain = (4u16).to_be_bytes();
    let len_cipher = seal_aead(&len_key, &len_nonce, &len_plain, &[]);

    let hdr_key: [u8; 16] = vmess_aead_kdf(&response_key, &[KDF_RESP_KEY.as_bytes()])[..16]
        .try_into()
        .unwrap();
    let hdr_nonce: [u8; 12] = vmess_aead_kdf(&response_nonce, &[KDF_RESP_IV.as_bytes()])[..12]
        .try_into()
        .unwrap();
    let header_plain = [response_header, option, 0u8, 0u8];
    let hdr_cipher = seal_aead(&hdr_key, &hdr_nonce, &header_plain, &[]);

    stream.write_all(&len_cipher).await?;
    stream.write_all(&hdr_cipher).await?;
    Ok(())
}

pub async fn connect(
    stream: &mut (impl AsyncReadExt + AsyncWriteExt + Unpin),
    uuid: Uuid,
    dest: SocketAddr,
) -> Result<([u8; 16], [u8; 16])> {
    write_handshake(stream, uuid, dest, CMD_TCP).await
}

enum VmessResponseState {
    Len { buf: [u8; 18], filled: usize },
    Header { len: usize, buf: Vec<u8>, filled: usize },
    Done,
}

struct VmessResponseStream {
    inner: rsb_core::ProxyConn,
    request_key: [u8; 16],
    request_nonce: [u8; 16],
    state: VmessResponseState,
}

impl VmessResponseStream {
    fn new(
        inner: rsb_core::ProxyConn,
        request_key: [u8; 16],
        request_nonce: [u8; 16],
    ) -> rsb_core::ProxyConn {
        rsb_core::proxy_box(Self {
            inner,
            request_key,
            request_nonce,
            state: VmessResponseState::Len {
                buf: [0u8; 18],
                filled: 0,
            },
        })
    }

    fn advance_response(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        loop {
            match std::mem::replace(
                &mut self.state,
                VmessResponseState::Done,
            ) {
                VmessResponseState::Done => {
                    self.state = VmessResponseState::Done;
                    return std::task::Poll::Ready(Ok(()));
                }
                VmessResponseState::Len { mut buf, mut filled } => {
                    let mut read_buf = tokio::io::ReadBuf::new(&mut buf[filled..]);
                    match std::pin::Pin::new(&mut self.inner).poll_read(cx, &mut read_buf) {
                        std::task::Poll::Ready(Ok(())) => {
                            filled += read_buf.filled().len();
                            if filled < 18 {
                                self.state = VmessResponseState::Len { buf, filled };
                                return std::task::Poll::Pending;
                            }
                            let response_key: [u8; 16] =
                                Sha256::digest(self.request_key)[..16].try_into().unwrap();
                            let response_nonce: [u8; 16] =
                                Sha256::digest(self.request_nonce)[..16].try_into().unwrap();
                            let len_key: [u8; 16] = vmess_aead_kdf(
                                &response_key,
                                &[KDF_RESP_LEN_KEY.as_bytes()],
                            )[..16]
                                .try_into()
                                .unwrap();
                            let len_nonce: [u8; 12] = vmess_aead_kdf(
                                &response_nonce,
                                &[KDF_RESP_LEN_IV.as_bytes()],
                            )[..12]
                                .try_into()
                                .unwrap();
                            let len_plain = open_aead(&len_key, &len_nonce, &buf, &[]).map_err(
                                |e| {
                                    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                                },
                            )?;
                            if len_plain.len() < 2 {
                                return std::task::Poll::Ready(Err(std::io::Error::new(
                                    std::io::ErrorKind::InvalidData,
                                    "truncated vmess response length",
                                )));
                            }
                            let header_len =
                                u16::from_be_bytes([len_plain[0], len_plain[1]]) as usize;
                            self.state = VmessResponseState::Header {
                                len: header_len,
                                buf: vec![0u8; header_len + 16],
                                filled: 0,
                            };
                        }
                        std::task::Poll::Ready(Err(e)) => return std::task::Poll::Ready(Err(e)),
                        std::task::Poll::Pending => {
                            self.state = VmessResponseState::Len { buf, filled };
                            return std::task::Poll::Pending;
                        }
                    }
                }
                VmessResponseState::Header { len, mut buf, mut filled } => {
                    let need = buf.len();
                    let mut read_buf = tokio::io::ReadBuf::new(&mut buf[filled..]);
                    match std::pin::Pin::new(&mut self.inner).poll_read(cx, &mut read_buf) {
                        std::task::Poll::Ready(Ok(())) => {
                            filled += read_buf.filled().len();
                            if filled < need {
                                self.state = VmessResponseState::Header { len, buf, filled };
                                return std::task::Poll::Pending;
                            }
                            let response_key: [u8; 16] =
                                Sha256::digest(self.request_key)[..16].try_into().unwrap();
                            let response_nonce: [u8; 16] =
                                Sha256::digest(self.request_nonce)[..16].try_into().unwrap();
                            let hdr_key: [u8; 16] = vmess_aead_kdf(
                                &response_key,
                                &[KDF_RESP_KEY.as_bytes()],
                            )[..16]
                                .try_into()
                                .unwrap();
                            let hdr_nonce: [u8; 12] = vmess_aead_kdf(
                                &response_nonce,
                                &[KDF_RESP_IV.as_bytes()],
                            )[..12]
                                .try_into()
                                .unwrap();
                            let _ = open_aead(&hdr_key, &hdr_nonce, &buf, &[]).map_err(|e| {
                                std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                            })?;
                            let _ = len;
                            self.state = VmessResponseState::Done;
                        }
                        std::task::Poll::Ready(Err(e)) => return std::task::Poll::Ready(Err(e)),
                        std::task::Poll::Pending => {
                            self.state = VmessResponseState::Header { len, buf, filled };
                            return std::task::Poll::Pending;
                        }
                    }
                }
            }
        }
    }
}

impl tokio::io::AsyncRead for VmessResponseStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if !matches!(self.state, VmessResponseState::Done) {
            match self.as_mut().advance_response(cx) {
                std::task::Poll::Ready(Ok(())) => {}
                std::task::Poll::Ready(Err(e)) => return std::task::Poll::Ready(Err(e)),
                std::task::Poll::Pending => return std::task::Poll::Pending,
            }
        }
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl tokio::io::AsyncWrite for VmessResponseStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

pub fn wrap_stream(
    stream: rsb_core::ProxyConn,
    request_key: [u8; 16],
    request_nonce: [u8; 16],
) -> rsb_core::ProxyConn {
    VmessResponseStream::new(stream, request_key, request_nonce)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kdf_vectors_match_go() {
        let key: [u8; 16] = core::array::from_fn(|i| i as u8);
        let out1 = vmess_aead_kdf(&key, &[KDF_AUTH_ID.as_bytes()]);
        assert_eq!(
            hex_encode(out1),
            "9fa4289c41650861a45b34aeab3879fe4785dce57ab3f68cfb0cc60fca69460a"
        );
        let out2 = vmess_aead_kdf(
            &key,
            &[
                KDF_HDR_LEN_KEY.as_bytes(),
                b"auth",
                b"nonce",
            ],
        );
        assert_eq!(
            hex_encode(out2),
            "c1d54a327d9fb7d2d58cfbbbed92c0d9351cc6a93a3461cd74d2aad7e7ef2778"
        );
    }

    fn hex_encode(bytes: [u8; 32]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
