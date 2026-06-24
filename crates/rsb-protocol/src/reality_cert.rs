//! REALITY ed25519 certificate verification helpers.

use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use sha2::Sha512;

type HmacSha512 = Hmac<Sha512>;

const ED25519_OID: &[u8] = &[0x06, 0x03, 0x2b, 0x65, 0x70];

pub fn verify_reality_cert(auth_key: &[u8; 32], ed25519_pub: &[u8], signature: &[u8]) -> bool {
    if signature.len() != 64 || ed25519_pub.len() != 32 {
        return false;
    }
    let mut mac = match <HmacSha512 as KeyInit>::new_from_slice(auth_key) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(ed25519_pub);
    mac.finalize().into_bytes().as_slice() == signature
}

/// Parse Ed25519 public key + signature from REALITY self-signed cert DER.
pub fn parse_reality_cert_parts(cert_der: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let pubkey = find_ed25519_pubkey(cert_der)?;
    let signature = find_certificate_signature(cert_der)?;
    Some((pubkey, signature))
}

fn find_ed25519_pubkey(der: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0usize;
    while i + ED25519_OID.len() < der.len() {
        if der[i..].starts_with(ED25519_OID) {
            let mut j = i + ED25519_OID.len();
            while j + 2 < der.len() {
                if der[j] == 0x03 {
                    let len = der[j + 1] as usize;
                    let start = j + 2;
                    if start + len <= der.len() && len >= 33 {
                        let unused = der[start];
                        let pk_start = start + 1 + unused as usize;
                        if pk_start + 32 <= der.len() {
                            return Some(der[pk_start..pk_start + 32].to_vec());
                        }
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    None
}

fn find_certificate_signature(der: &[u8]) -> Option<Vec<u8>> {
    if der.len() < 70 {
        return None;
    }
    let mut i = der.len().saturating_sub(80);
    while i + 3 < der.len() {
        if der[i] == 0x03 {
            let len = der[i + 1] as usize;
            let start = i + 2;
            if len >= 64 && start + len <= der.len() {
                let unused = der[start];
                let sig_start = start + 1 + unused as usize;
                if sig_start + 64 <= der.len() {
                    return Some(der[sig_start..sig_start + 64].to_vec());
                }
            }
        }
        i += 1;
    }
    if der.len() >= 64 {
        return Some(der[der.len() - 64..].to_vec());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::Mac;

    #[test]
    fn verify_hmac_length() {
        let key = [1u8; 32];
        let pub_key = [2u8; 32];
        let mut mac = <HmacSha512 as KeyInit>::new_from_slice(&key).unwrap();
        mac.update(&pub_key);
        let sig = mac.finalize().into_bytes();
        assert!(verify_reality_cert(&key, &pub_key, &sig));
    }
}
