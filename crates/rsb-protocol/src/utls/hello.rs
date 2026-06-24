//! Byte-accurate TLS ClientHello builders (Chrome / Firefox / Safari profiles).

use rand::RngCore;
use x25519_dalek::{PublicKey, StaticSecret};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    Chrome,
    Edge,
    Firefox,
    Safari,
    Ios,
    Random,
}

impl Profile {
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "chrome" => Self::Chrome,
            "edge" => Self::Edge,
            "firefox" => Self::Firefox,
            "safari" => Self::Safari,
            "ios" => Self::Ios,
            "random" => Self::Random,
            _ => return None,
        })
    }
}

pub struct ClientHelloKeys {
    pub secret: StaticSecret,
    pub hello: Vec<u8>,
}

/// Offsets into `hello` (full TLS record) for REALITY patching.
pub struct HelloLayout {
    pub random_offset: usize,
    pub session_id_offset: usize,
}

pub fn hello_layout(hello: &[u8]) -> Option<HelloLayout> {
    if hello.len() < 50 || hello[0] != 0x16 {
        return None;
    }
    let hs_start = 5usize;
    if hello.get(hs_start)? != &0x01 {
        return None;
    }
    let body = hs_start + 4;
    Some(HelloLayout {
        random_offset: body + 2,
        session_id_offset: body + 2 + 32 + 1,
    })
}

pub fn client_hello_random<'a>(hello: &'a [u8]) -> Option<&'a [u8; 32]> {
    let layout = hello_layout(hello)?;
    hello
        .get(layout.random_offset..layout.random_offset + 32)?
        .try_into()
        .ok()
}

pub fn generate_client_hello(profile: Profile, sni: &str) -> ClientHelloKeys {
    let mut sk = [0u8; 32];
    rand::rng().fill_bytes(&mut sk);
    let secret = StaticSecret::from(sk);
    let pubkey = PublicKey::from(&secret);
    let hello = build_client_hello(profile, sni, pubkey.as_bytes());
    ClientHelloKeys { secret, hello }
}

fn grease16(rng: &mut impl RngCore) -> u16 {
    let b = (rng.next_u32() & 0x0f) as u8;
    u16::from_be_bytes([0x0a | b, 0x0a | b])
}

pub fn build_client_hello(profile: Profile, sni: &str, key_share: &[u8; 32]) -> Vec<u8> {
    let mut rng = rand::rng();
    let g1 = grease16(&mut rng);
    let g2 = grease16(&mut rng);

    let ciphers: &[u16] = match profile {
        Profile::Firefox => &[
            0x1301, 0x1303, 0x1302, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013, 0xc014,
            0x009c, 0x009d, 0x002f, 0x0035,
        ],
        Profile::Safari | Profile::Ios => &[
            0x1301, 0x1302, 0x1303, 0xc02c, 0xc02b, 0xcca9, 0xc030, 0xc02f, 0xcca8, 0xc024, 0xc023,
            0xc00a, 0xc009, 0x009d, 0x009c, 0x003d, 0x003c, 0x0035, 0x002f,
        ],
        _ => &[
            0x0a0a, 0x1301, 0x1302, 0x1303, 0xc02b, 0xc02f, 0xc02c, 0xc030, 0xcca9, 0xcca8, 0xc013,
            0xc014, 0x009c, 0x009d, 0x002f, 0x0035, 0x1a1a,
        ],
    };

    let mut exts = Vec::new();
    match profile {
        Profile::Firefox => {
            push_ext(&mut exts, 0x0000, &encode_sni(sni));
            push_ext(&mut exts, 0x0017, &[]);
            push_ext(&mut exts, 0xff01, &[0x00]);
            push_ext(&mut exts, 0x000a, &encode_groups());
            push_ext(&mut exts, 0x000b, &[0x00]);
            push_ext(&mut exts, 0x002b, &[0x03, 0x04, 0x03, 0x03]);
            push_ext(&mut exts, 0x002d, &[0x01, 0x01]);
            push_ext(&mut exts, 0x0033, &encode_key_share(key_share));
            push_ext(&mut exts, 0x0010, &encode_alpn(&["h2", "http/1.1"]));
            push_ext(&mut exts, 0x000d, &encode_sig_algs());
        }
        Profile::Safari | Profile::Ios => {
            push_ext(&mut exts, 0x0000, &encode_sni(sni));
            push_ext(&mut exts, 0x0017, &[]);
            push_ext(&mut exts, 0xff01, &[0x00]);
            push_ext(&mut exts, 0x000a, &encode_groups());
            push_ext(&mut exts, 0x000b, &[0x00]);
            push_ext(&mut exts, 0x002b, &[0x03, 0x04, 0x03, 0x03]);
            push_ext(&mut exts, 0x0033, &encode_key_share(key_share));
            push_ext(&mut exts, 0x0010, &encode_alpn(&["h2", "http/1.1"]));
            push_ext(&mut exts, 0x000d, &encode_sig_algs());
            push_ext(&mut exts, 0x002d, &[0x01, 0x01]);
        }
        _ => {
            push_ext_u16(&mut exts, g1, &[]);
            push_ext(&mut exts, 0x0000, &encode_sni(sni));
            push_ext(&mut exts, 0x0017, &[]);
            push_ext(&mut exts, 0xff01, &[0x00]);
            push_ext(&mut exts, 0x000a, &encode_groups());
            push_ext(&mut exts, 0x000b, &[0x00]);
            push_ext(&mut exts, 0x002b, &[0x03, 0x04, 0x03, 0x03]);
            push_ext(&mut exts, 0x0033, &encode_key_share(key_share));
            push_ext(&mut exts, 0x0010, &encode_alpn(&["h2", "http/1.1"]));
            push_ext(&mut exts, 0x000d, &encode_sig_algs());
            push_ext(&mut exts, 0x002d, &[0x01, 0x01]);
            push_ext_u16(&mut exts, g2, &[]);
        }
    }

    let mut body = Vec::new();
    body.extend_from_slice(&[0x03, 0x03]);
    let mut random = [0u8; 32];
    rng.fill_bytes(&mut random);
    body.extend_from_slice(&random);
    body.push(32);
    let mut session_id = [0u8; 32];
    rng.fill_bytes(&mut session_id);
    body.extend_from_slice(&session_id);
    for &c in ciphers {
        body.extend_from_slice(&c.to_be_bytes());
    }
    body.push(0x01);
    body.push(0x00);
    body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
    body.extend_from_slice(&exts);

    let mut hs = Vec::new();
    hs.push(0x01);
    hs.extend_from_slice(&(body.len() as u32).to_be_bytes()[1..]);
    hs.extend_from_slice(&body);

    let mut record = Vec::new();
    record.push(0x16);
    record.extend_from_slice(&[0x03, 0x01]);
    record.extend_from_slice(&(hs.len() as u16).to_be_bytes());
    record.extend_from_slice(&hs);
    record
}

fn push_ext(out: &mut Vec<u8>, typ: u16, data: &[u8]) {
    push_ext_u16(out, typ, data);
}

fn push_ext_u16(out: &mut Vec<u8>, typ: u16, data: &[u8]) {
    out.extend_from_slice(&typ.to_be_bytes());
    out.extend_from_slice(&(data.len() as u16).to_be_bytes());
    out.extend_from_slice(data);
}

fn encode_sni(host: &str) -> Vec<u8> {
    let host_bytes = host.as_bytes();
    let mut out = Vec::new();
    out.extend_from_slice(&((host_bytes.len() + 3) as u16).to_be_bytes());
    out.push(0x00);
    out.extend_from_slice(&(host_bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(host_bytes);
    out
}

fn encode_alpn(protos: &[&str]) -> Vec<u8> {
    let mut inner = Vec::new();
    for p in protos {
        inner.push(p.len() as u8);
        inner.extend_from_slice(p.as_bytes());
    }
    inner
}

fn encode_groups() -> Vec<u8> {
    let mut out = vec![0x00, 0x08];
    for g in [0x001d, 0x0017, 0x0018, 0x0019u16] {
        out.extend_from_slice(&g.to_be_bytes());
    }
    out
}

fn encode_key_share(pubkey: &[u8; 32]) -> Vec<u8> {
    let mut out = vec![0x00, 0x24];
    out.extend_from_slice(&0x001du16.to_be_bytes());
    out.extend_from_slice(&32u16.to_be_bytes());
    out.extend_from_slice(pubkey);
    out
}

fn encode_sig_algs() -> Vec<u8> {
    let algs: [u16; 8] = [
        0x0403, 0x0804, 0x0401, 0x0503, 0x0805, 0x0501, 0x0806, 0x0601,
    ];
    let mut out = vec![(algs.len() * 2) as u8];
    for a in algs {
        out.extend_from_slice(&a.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrome_hello_starts_with_record() {
        let keys = generate_client_hello(Profile::Chrome, "example.com");
        assert_eq!(keys.hello[0], 0x16);
        assert!(keys.hello.len() > 200);
    }
}
