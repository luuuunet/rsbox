use blake2::{Blake2b512, Digest};
use rand::RngCore;

const SALT_LEN: usize = 8;
const HASH_LEN: usize = 32;

pub struct Salamander {
    key: Vec<u8>,
}

impl Salamander {
    pub fn new(password: &str) -> Self {
        Self {
            key: password.as_bytes().to_vec(),
        }
    }

    pub fn encode(&self, payload: &[u8], out: &mut Vec<u8>) {
        out.clear();
        let mut salt = [0u8; SALT_LEN];
        rand::rng().fill_bytes(&mut salt);
        out.extend_from_slice(&salt);
        let xored = self.xor_payload(&salt, payload);
        out.extend_from_slice(&xored);
    }

    pub fn decode_owned(&self, packet: &[u8]) -> Option<Vec<u8>> {
        if packet.len() <= SALT_LEN {
            return None;
        }
        let (salt, payload) = packet.split_at(SALT_LEN);
        Some(self.xor_payload(salt, payload))
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
        let mut hasher = Blake2b512::new();
        hasher.update(&self.key);
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
    fn roundtrip() {
        let obfs = Salamander::new("test-key");
        let mut encoded = Vec::new();
        obfs.encode(b"hello quic", &mut encoded);
        let decoded = obfs.decode_owned(&encoded).unwrap();
        assert_eq!(decoded, b"hello quic");
    }
}
