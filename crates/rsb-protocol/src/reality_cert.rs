//! REALITY ed25519 certificate verification helpers.

use hmac::digest::KeyInit;
use hmac::{Hmac, Mac};
use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::Sha512;

type HmacSha512 = Hmac<Sha512>;

const ED25519_OID: &[u8] = &[0x06, 0x03, 0x2b, 0x65, 0x70];

pub struct RealityCertMaterial {
    pub cert_message: Vec<u8>,
    key_pair: Ed25519KeyPair,
}

impl RealityCertMaterial {
    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.key_pair.sign(message).as_ref().to_vec()
    }
}

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

/// Build REALITY self-signed certificate DER (Xray-compatible HMAC signature).
pub fn build_reality_cert_der(auth_key: &[u8; 32], pubkey: &[u8; 32]) -> Vec<u8> {
    let mut mac = <HmacSha512 as KeyInit>::new_from_slice(auth_key).expect("hmac key");
    mac.update(pubkey);
    let signature = mac.finalize().into_bytes();

    let mut der = vec![0u8; 220];
    der[0] = 0x30;
    der[1] = 0x82;
    der[4] = 0x30;
    let oid_pos = 20usize;
    der[oid_pos..oid_pos + ED25519_OID.len()].copy_from_slice(ED25519_OID);
    let pk_marker = oid_pos + ED25519_OID.len();
    der[pk_marker] = 0x03;
    der[pk_marker + 1] = 0x21;
    der[pk_marker + 2] = 0x00;
    der[pk_marker + 3..pk_marker + 35].copy_from_slice(pubkey);
    let der_len = der.len();
    der[der_len - 70] = 0x03;
    der[der_len - 69] = 0x41;
    der[der_len - 68] = 0x00;
    der[der_len - 67..der_len - 3].copy_from_slice(&signature[..64]);
    der
}

fn wrap_tls_cert_handshake(cert_der: &[u8]) -> Vec<u8> {
    // TLS 1.3 Certificate: empty context + CertificateEntry list (sing-box / Go compatible).
    let mut entry = Vec::with_capacity(3 + cert_der.len() + 2);
    entry.extend_from_slice(&(cert_der.len() as u32).to_be_bytes()[1..]);
    entry.extend_from_slice(cert_der);
    entry.extend_from_slice(&[0, 0]); // empty CertificateEntry extensions

    let mut body = Vec::with_capacity(1 + 3 + entry.len());
    body.push(0); // certificate_request_context (empty)
    body.extend_from_slice(&(entry.len() as u32).to_be_bytes()[1..]);
    body.extend_from_slice(&entry);

    let mut hs = Vec::with_capacity(4 + body.len());
    hs.push(0x0b);
    hs.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    hs.extend_from_slice(&body);
    hs
}

/// Certificate handshake message (type 0x0b) for REALITY server.
pub fn build_reality_certificate_message(auth_key: &[u8; 32]) -> Vec<u8> {
    build_reality_cert_material(auth_key).cert_message
}

pub fn build_reality_cert_material(auth_key: &[u8; 32]) -> RealityCertMaterial {
    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).expect("ed25519 keygen");
    let key_pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).expect("ed25519 pkcs8");
    let pubkey: [u8; 32] = key_pair.public_key().as_ref().try_into().expect("ed25519 pub");
    RealityCertMaterial {
        cert_message: wrap_tls_cert_handshake(&build_reality_cert_der(auth_key, &pubkey)),
        key_pair,
    }
}

/// TLS 1.3 EncryptedExtensions (0x08) with optional ALPN.
pub fn build_encrypted_extensions(alpn: Option<&str>) -> Vec<u8> {
    let Some(alpn) = alpn else {
        return vec![0x08, 0x00, 0x00, 0x00];
    };
    let proto = alpn.as_bytes();
    let mut ext = Vec::new();
    ext.extend_from_slice(&[0x00, 0x10]); // alpn
    let ext_body_len = 2 + 1 + proto.len();
    ext.extend_from_slice(&(ext_body_len as u16).to_be_bytes());
    ext.extend_from_slice(&((1 + proto.len()) as u16).to_be_bytes());
    ext.push(proto.len() as u8);
    ext.extend_from_slice(proto);
    let mut hs = Vec::with_capacity(4 + ext.len());
    hs.push(0x08);
    hs.extend_from_slice(&(ext.len() as u32).to_be_bytes()[1..]);
    hs.extend_from_slice(&ext);
    hs
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
