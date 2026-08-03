//! RST-Obfs — UDP obfuscation (Salamander-style, same as RSQ).
//!
//! Domain tags are `rst-obfs-v*` so keys do not collide with RSQ/Hy2.

use blake2::{Blake2b512, Digest};
use rand::RngCore;

const SALT_LEN: usize = 8;
const HASH_LEN: usize = 32;
const DOMAIN_V1: &[u8] = b"rst-obfs-v1";
const DOMAIN_V2: &[u8] = b"rst-obfs-v2";
const MAX_V2_PAD: usize = 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObfsVersion {
    V1 = 1,
    V2 = 2,
}

impl ObfsVersion {
    pub fn parse(raw: Option<u64>) -> Self {
        match raw.unwrap_or(1) {
            2 => Self::V2,
            _ => Self::V1,
        }
    }
}

pub struct RstObfs {
    key: Vec<u8>,
    version: ObfsVersion,
}

impl RstObfs {
    pub fn new(password: &str) -> Self {
        Self::with_version(password, ObfsVersion::V1)
    }

    pub fn with_version(password: &str, version: ObfsVersion) -> Self {
        Self {
            key: password.as_bytes().to_vec(),
            version,
        }
    }

    pub fn version(&self) -> ObfsVersion {
        self.version
    }

    /// Encode plaintext into `salt || xor(inner)`.
    pub fn encode(&self, payload: &[u8], out: &mut Vec<u8>) {
        out.clear();
        let mut salt = [0u8; SALT_LEN];
        rand::rng().fill_bytes(&mut salt);
        out.extend_from_slice(&salt);
        let inner = match self.version {
            ObfsVersion::V1 => payload.to_vec(),
            ObfsVersion::V2 => {
                let pad_len = (rand::random::<u8>() as usize) % (MAX_V2_PAD + 1);
                let mut inner = Vec::with_capacity(payload.len() + 1 + pad_len);
                inner.extend_from_slice(payload);
                if pad_len > 0 {
                    let mut pad = vec![0u8; pad_len];
                    rand::rng().fill_bytes(&mut pad);
                    inner.extend_from_slice(&pad);
                }
                inner.push(pad_len as u8);
                inner
            }
        };
        out.extend_from_slice(&self.xor_payload(&salt, &inner));
    }

    pub fn decode_owned(&self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.len() <= SALT_LEN {
            return None;
        }
        let (salt, payload) = packet.split_at(SALT_LEN);
        let inner = self.xor_payload(salt, payload);
        match self.version {
            ObfsVersion::V1 => Some(inner),
            ObfsVersion::V2 => {
                if inner.is_empty() {
                    return None;
                }
                let pad_len = *inner.last()? as usize;
                if inner.len() < 1 + pad_len {
                    return None;
                }
                let payload_len = inner.len() - 1 - pad_len;
                Some(inner[..payload_len].to_vec())
            }
        }
    }

    fn xor_payload(&self, salt: &[u8], payload: &[u8]) -> Vec<u8> {
        let hash = self.salted_hash(salt);
        payload
            .iter()
            .enumerate()
            .map(|(i, b)| b ^ hash[i % HASH_LEN])
            .collect()
    }

    fn salted_hash(&self, salt: &[u8]) -> [u8; HASH_LEN] {
        let domain = match self.version {
            ObfsVersion::V1 => DOMAIN_V1,
            ObfsVersion::V2 => DOMAIN_V2,
        };
        let mut hasher = Blake2b512::new();
        hasher.update(&self.key);
        hasher.update(domain);
        hasher.update(salt);
        let digest = hasher.finalize();
        let mut out = [0u8; HASH_LEN];
        out.copy_from_slice(&digest[..HASH_LEN]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_v1() {
        let obfs = RstObfs::new("node-secret");
        let mut encoded = Vec::new();
        obfs.encode(b"tls payload", &mut encoded);
        let decoded = obfs.decode_owned(&encoded).unwrap();
        assert_eq!(decoded, b"tls payload");
    }

    #[test]
    fn roundtrip_v2() {
        let obfs = RstObfs::with_version("node-secret", ObfsVersion::V2);
        let mut encoded = Vec::new();
        obfs.encode(b"tls payload v2", &mut encoded);
        let decoded = obfs.decode_owned(&encoded).unwrap();
        assert_eq!(decoded, b"tls payload v2");
    }

    #[test]
    fn bad_packet_rejected() {
        let obfs = RstObfs::new("node-secret");
        assert!(obfs.decode_owned(&[1, 2, 3]).is_none());
        let mut encoded = Vec::new();
        obfs.encode(b"x", &mut encoded);
        encoded[8] ^= 0xff;
        // May still decode to garbage for v1; for wrong password:
        let other = RstObfs::new("other");
        let d = other.decode_owned(&encoded).unwrap();
        assert_ne!(d, b"x");
    }
}
