//! Tailscale controlbase Noise_IK client (compatible with control/controlbase).

use anyhow::{bail, Context, Result};
use blake2::digest::{KeyInit as MacKeyInit, Mac};
use blake2::{Blake2s256, Blake2sMac256, Digest};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::ChaCha20Poly1305;
use rand::RngCore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use x25519_dalek::{PublicKey, StaticSecret};

fn hkdf_blake2s(salt: Option<&[u8]>, ikm: &[u8], len: usize) -> Vec<u8> {
    let salt_bytes = match salt {
        Some(s) => s.to_vec(),
        None => vec![0u8; 32],
    };
    let mut extract =
        <Blake2sMac256 as MacKeyInit>::new_from_slice(&salt_bytes).expect("hkdf extract");
    extract.update(ikm);
    let prk = extract.finalize().into_bytes();

    let mut out = Vec::with_capacity(len);
    let mut prev = Vec::new();
    let mut counter = 0u8;
    while out.len() < len {
        counter += 1;
        let mut expand = <Blake2sMac256 as MacKeyInit>::new_from_slice(&prk).expect("hkdf expand");
        if !prev.is_empty() {
            expand.update(&prev);
        }
        expand.update(&[]);
        expand.update(&[counter]);
        prev = expand.finalize().into_bytes().to_vec();
        out.extend_from_slice(&prev);
    }
    out.truncate(len);
    out
}

const PROTOCOL_NAME: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
pub const DEFAULT_PROTOCOL_VERSION: u16 = 115;
const MSG_INIT: u8 = 1;
const MSG_RESP: u8 = 2;
const MSG_RECORD: u8 = 4;
const INIT_MSG_LEN: usize = 101;
const RESP_MSG_LEN: usize = 51;
const MAX_PLAINTEXT: usize = 4096 - 3 - 16;

struct SymmetricState {
    h: [u8; 32],
    ck: [u8; 32],
    finished: bool,
}

impl SymmetricState {
    fn initialize() -> Self {
        let h = Blake2s256::digest(PROTOCOL_NAME.as_bytes());
        Self {
            h: h.into(),
            ck: h.into(),
            finished: false,
        }
    }

    fn mix_hash(&mut self, data: &[u8]) {
        assert!(!self.finished);
        let mut hasher = Blake2s256::new();
        hasher.update(self.h);
        hasher.update(data);
        self.h = hasher.finalize().into();
    }

    fn mix_dh(&mut self, priv_key: &StaticSecret, pub_key: &PublicKey) -> Result<[u8; 32]> {
        assert!(!self.finished);
        let shared = priv_key.diffie_hellman(pub_key);
        let okm = hkdf_blake2s(Some(&self.ck), shared.as_bytes(), 64);
        self.ck.copy_from_slice(&okm[..32]);
        Ok(okm[32..64].try_into().unwrap())
    }

    fn encrypt_and_hash(&mut self, key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new_from_slice(key).context("noise encrypt cipher")?;
        let ct = cipher
            .encrypt(
                &[0u8; 12].into(),
                Payload {
                    msg: plaintext,
                    aad: &self.h,
                },
            )
            .map_err(|e| anyhow::anyhow!("noise encrypt: {e}"))?;
        self.mix_hash(&ct);
        Ok(ct)
    }

    fn decrypt_and_hash(&mut self, key: &[u8; 32], ciphertext: &[u8]) -> Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new_from_slice(key).context("noise decrypt cipher")?;
        let pt = cipher
            .decrypt(
                &[0u8; 12].into(),
                Payload {
                    msg: ciphertext,
                    aad: &self.h,
                },
            )
            .map_err(|e| anyhow::anyhow!("noise decrypt: {e}"))?;
        self.mix_hash(ciphertext);
        Ok(pt)
    }

    fn split(&mut self) -> Result<([u8; 32], [u8; 32])> {
        assert!(!self.finished);
        self.finished = true;
        let okm = hkdf_blake2s(None, &self.ck, 64);
        Ok((okm[..32].try_into()?, okm[32..64].try_into()?))
    }
}

fn protocol_prologue(version: u16) -> Vec<u8> {
    format!("Tailscale Control Protocol v{version}").into_bytes()
}

pub struct NoiseConn {
    tx_key: [u8; 32],
    rx_key: [u8; 32],
    tx_nonce: u64,
    rx_nonce: u64,
}

impl NoiseConn {
    pub async fn write_all(
        &mut self,
        stream: &mut (impl AsyncWriteExt + Unpin),
        data: &[u8],
    ) -> Result<()> {
        for chunk in data.chunks(MAX_PLAINTEXT) {
            let mut hdr = [0u8; 3];
            hdr[0] = MSG_RECORD;
            let cipher = ChaCha20Poly1305::new_from_slice(&self.tx_key).context("tx cipher")?;
            let ct = cipher
                .encrypt(
                    &record_nonce(self.tx_nonce).into(),
                    Payload {
                        msg: chunk,
                        aad: &[],
                    },
                )
                .map_err(|e| anyhow::anyhow!("noise record encrypt: {e}"))?;
            hdr[1..3].copy_from_slice(&(ct.len() as u16).to_be_bytes());
            stream.write_all(&hdr).await?;
            stream.write_all(&ct).await?;
            self.tx_nonce += 1;
        }
        Ok(())
    }

    pub async fn read_record(
        &mut self,
        stream: &mut (impl AsyncReadExt + Unpin),
    ) -> Result<Vec<u8>> {
        let mut hdr = [0u8; 3];
        stream.read_exact(&mut hdr).await?;
        if hdr[0] != MSG_RECORD {
            bail!("unexpected noise message type {}", hdr[0]);
        }
        let len = u16::from_be_bytes([hdr[1], hdr[2]]) as usize;
        let mut ct = vec![0u8; len];
        stream.read_exact(&mut ct).await?;
        let cipher = ChaCha20Poly1305::new_from_slice(&self.rx_key).context("rx cipher")?;
        let pt = cipher
            .decrypt(
                &record_nonce(self.rx_nonce).into(),
                Payload { msg: &ct, aad: &[] },
            )
            .map_err(|e| anyhow::anyhow!("noise record decrypt: {e}"))?;
        self.rx_nonce += 1;
        Ok(pt)
    }
}

fn record_nonce(seq: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[4..12].copy_from_slice(&seq.to_be_bytes());
    n
}

/// Perform Noise IK client handshake; returns transport ciphers.
pub fn client_initiation(
    machine_key: &StaticSecret,
    control_key: &[u8; 32],
    protocol_version: u16,
) -> Result<(Vec<u8>, StaticSecret)> {
    let mut s = SymmetricState::initialize();
    s.mix_hash(&protocol_prologue(protocol_version));
    s.mix_hash(control_key);

    let mut eph_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut eph_bytes);
    let eph_sk = StaticSecret::from(eph_bytes);
    let eph_pk = PublicKey::from(&eph_sk);
    s.mix_hash(eph_pk.as_bytes());

    let k_es = s.mix_dh(&eph_sk, &PublicKey::from(*control_key))?;
    let machine_pub = PublicKey::from(machine_key);
    let enc_machine = s.encrypt_and_hash(&k_es, machine_pub.as_bytes())?;
    anyhow::ensure!(enc_machine.len() == 48, "bad encrypted machine key size");

    let k_ss = s.mix_dh(machine_key, &PublicKey::from(*control_key))?;
    let tag = s.encrypt_and_hash(&k_ss, b"")?;
    anyhow::ensure!(tag.len() == 16, "bad initiation tag size");

    let mut msg = vec![0u8; INIT_MSG_LEN];
    msg[0..2].copy_from_slice(&protocol_version.to_be_bytes());
    msg[2] = MSG_INIT;
    msg[3..5].copy_from_slice(&96u16.to_be_bytes());
    msg[5..37].copy_from_slice(eph_pk.as_bytes());
    msg[37..85].copy_from_slice(&enc_machine);
    msg[85..101].copy_from_slice(&tag);
    Ok((msg, eph_sk))
}

pub fn client_finish(
    mut s: SymmetricState,
    eph_sk: &StaticSecret,
    machine_key: &StaticSecret,
    control_key: &[u8; 32],
    response: &[u8],
) -> Result<NoiseConn> {
    anyhow::ensure!(response.len() == RESP_MSG_LEN, "bad noise response size");
    if response[0] != MSG_RESP {
        if response[0] == 3 {
            bail!("control server noise error");
        }
        bail!("unexpected noise response type {}", response[0]);
    }
    let server_eph =
        PublicKey::from(<[u8; 32]>::try_from(&response[3..35]).context("server ephemeral")?);
    s.mix_hash(server_eph.as_bytes());
    s.mix_dh(eph_sk, &server_eph)?;
    let k_se = s.mix_dh(machine_key, &server_eph)?;
    let tag = &response[35..51];
    s.decrypt_and_hash(&k_se, tag)?;

    let (tx_key, rx_key) = s.split()?;
    let _ = control_key;
    Ok(NoiseConn {
        tx_key,
        rx_key,
        tx_nonce: 0,
        rx_nonce: 0,
    })
}

/// Full client handshake state for step 2 after sending initiation.
pub fn client_state_for_finish(
    machine_key: &StaticSecret,
    control_key: &[u8; 32],
    protocol_version: u16,
    eph_sk: &StaticSecret,
    eph_pk: &PublicKey,
) -> SymmetricState {
    let mut s = SymmetricState::initialize();
    s.mix_hash(&protocol_prologue(protocol_version));
    s.mix_hash(control_key);
    s.mix_hash(eph_pk.as_bytes());
    let k_es = s
        .mix_dh(eph_sk, &PublicKey::from(*control_key))
        .expect("es");
    let machine_pub = PublicKey::from(machine_key);
    let _ = s.encrypt_and_hash(&k_es, machine_pub.as_bytes());
    let k_ss = s
        .mix_dh(machine_key, &PublicKey::from(*control_key))
        .expect("ss");
    let _ = s.encrypt_and_hash(&k_ss, b"");
    s
}

pub async fn client_handshake<S>(
    stream: &mut S,
    machine_key: &StaticSecret,
    control_key: &[u8; 32],
    protocol_version: u16,
) -> Result<NoiseConn>
where
    S: AsyncReadExt + AsyncWriteExt + Unpin,
{
    let (init, eph_sk) = client_initiation(machine_key, control_key, protocol_version)?;
    let eph_pk = PublicKey::from(&eph_sk);
    stream.write_all(&init).await?;

    let mut resp = vec![0u8; RESP_MSG_LEN];
    stream.read_exact(&mut resp).await?;

    let s = client_state_for_finish(machine_key, control_key, protocol_version, &eph_sk, &eph_pk);
    client_finish(s, &eph_sk, machine_key, control_key, &resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initiation_message_layout() {
        let mut sk = [0u8; 32];
        rand::rng().fill_bytes(&mut sk);
        let machine = StaticSecret::from(sk);
        let control = [7u8; 32];
        let (msg, _) = client_initiation(&machine, &control, DEFAULT_PROTOCOL_VERSION).unwrap();
        assert_eq!(msg.len(), INIT_MSG_LEN);
        assert_eq!(msg[2], MSG_INIT);
        assert_eq!(
            u16::from_be_bytes([msg[0], msg[1]]),
            DEFAULT_PROTOCOL_VERSION
        );
    }
}
