//! UDP datagram fragmentation and reassembly for RSQ.

use super::protocol::UdpMessage;
use anyhow::{bail, Result};
use bytes::BytesMut;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Drop incomplete fragment assemblies after this duration.
pub const FRAGMENT_TTL: Duration = Duration::from_secs(5);

/// Max payload bytes per QUIC datagram fragment (conservative for MTU).
pub const MAX_FRAGMENT_PAYLOAD: usize = 1100;

/// Max UDP fragments per logical packet (fragment_count is u8).
pub const MAX_UDP_FRAGMENTS: usize = 255;

/// Max reassembled UDP payload size.
pub const MAX_UDP_PAYLOAD: usize = MAX_FRAGMENT_PAYLOAD * MAX_UDP_FRAGMENTS;

/// Idle UDP relay sessions are dropped after this duration.
pub const UDP_SESSION_IDLE: Duration = Duration::from_secs(60);

pub fn is_udp_session_close(msg: &UdpMessage) -> bool {
    msg.fragment_count == 0 && msg.addr.is_empty() && msg.payload.is_empty()
}

pub fn encode_udp_session_close(session_id: u32) -> BytesMut {
    let mut out = BytesMut::new();
    UdpMessage {
        session_id,
        packet_id: 0,
        fragment_id: 0,
        fragment_count: 0,
        addr: String::new(),
        payload: vec![],
    }
    .encode(&mut out);
    out
}

pub fn fragment_payload(
    session_id: u32,
    packet_id: u16,
    addr: &str,
    payload: &[u8],
) -> Result<Vec<BytesMut>> {
    if payload.len() <= MAX_FRAGMENT_PAYLOAD {
        let mut out = BytesMut::new();
        UdpMessage {
            session_id,
            packet_id,
            fragment_id: 0,
            fragment_count: 1,
            addr: addr.to_string(),
            payload: payload.to_vec(),
        }
        .encode(&mut out);
        return Ok(vec![out]);
    }
    let count = payload.len().div_ceil(MAX_FRAGMENT_PAYLOAD);
    if count > MAX_UDP_FRAGMENTS {
        bail!(
            "udp payload too large: {} bytes (max {})",
            payload.len(),
            MAX_UDP_PAYLOAD
        );
    }
    let count_u8 = count as u8;
    let mut frames = Vec::with_capacity(count);
    for (i, chunk) in payload.chunks(MAX_FRAGMENT_PAYLOAD).enumerate() {
        let mut out = BytesMut::new();
        UdpMessage {
            session_id,
            packet_id,
            fragment_id: i as u8,
            fragment_count: count_u8,
            addr: addr.to_string(),
            payload: chunk.to_vec(),
        }
        .encode(&mut out);
        frames.push(out);
    }
    Ok(frames)
}

#[derive(Default)]
struct PacketAssembly {
    fragments: HashMap<u8, Vec<u8>>,
    fragment_count: u8,
    addr: Option<String>,
    started: Option<Instant>,
}

pub struct UdpReassembler {
    packets: HashMap<(u32, u16), PacketAssembly>,
}

impl UdpReassembler {
    pub fn new() -> Self {
        Self {
            packets: HashMap::new(),
        }
    }

    pub fn ingest(&mut self, msg: UdpMessage) -> Result<Option<UdpMessage>> {
        if msg.fragment_count <= 1 {
            return Ok(Some(msg));
        }
        let key = (msg.session_id, msg.packet_id);
        let entry = self.packets.entry(key).or_insert_with(PacketAssembly::default);
        if entry.started.is_none() {
            entry.started = Some(Instant::now());
        }
        if entry.fragment_count == 0 {
            entry.fragment_count = msg.fragment_count;
            entry.addr = Some(msg.addr.clone());
        } else if entry.fragment_count != msg.fragment_count {
            self.packets.remove(&key);
            bail!("udp fragment count mismatch");
        } else if let Some(ref expected_addr) = entry.addr {
            if msg.addr != *expected_addr {
                self.packets.remove(&key);
                bail!("udp fragment addr mismatch");
            }
        }
        if msg.fragment_id >= msg.fragment_count {
            bail!("invalid fragment id");
        }
        entry.fragments.insert(msg.fragment_id, msg.payload);
        if entry.fragments.len() != msg.fragment_count as usize {
            return Ok(None);
        }
        let assembly = self.packets.remove(&key).expect("assembly");
        let mut payload = Vec::new();
        for id in 0..assembly.fragment_count {
            let part = assembly
                .fragments
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("missing udp fragment {id}"))?;
            payload.extend_from_slice(part);
        }
        Ok(Some(UdpMessage {
            session_id: msg.session_id,
            packet_id: msg.packet_id,
            fragment_id: 0,
            fragment_count: 1,
            addr: assembly.addr.unwrap_or(msg.addr),
            payload,
        }))
    }

    pub fn prune_expired(&mut self) {
        let now = Instant::now();
        self.packets.retain(|_, asm| {
            asm.started
                .map(|t| now.duration_since(t) < FRAGMENT_TTL)
                .unwrap_or(true)
        });
    }

    pub fn prune_stale(&mut self, max_packets: usize) {
        self.prune_expired();
        while self.packets.len() > max_packets {
            if let Some(key) = self.packets.keys().next().cloned() {
                self.packets.remove(&key);
            } else {
                break;
            }
        }
    }

    pub fn pending_count(&self) -> usize {
        self.packets.len()
    }
}

pub fn ensure_fragment_ready(msg: &UdpMessage) -> Result<()> {
    if msg.fragment_count == 0 {
        bail!("invalid fragment_count");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rsq::protocol::UdpMessage;

    #[test]
    fn roundtrip_fragments() {
        let payload: Vec<u8> = (0..2500).map(|i| (i % 251) as u8).collect();
        let frames = fragment_payload(1, 7, "1.2.3.4:53", &payload).unwrap();
        assert!(frames.len() > 1);
        let mut asm = UdpReassembler::new();
        let mut complete = None;
        for frame in frames {
            let mut cursor = &frame[..];
            let msg = UdpMessage::decode(&mut cursor).unwrap();
            if let Some(m) = asm.ingest(msg).unwrap() {
                complete = Some(m);
            }
        }
        let m = complete.expect("reassembled");
        assert_eq!(m.payload, payload);
    }

    #[test]
    fn fragment_addr_mismatch_rejected() {
        let mut asm = UdpReassembler::new();
        let frames = fragment_payload(1, 9, "1.2.3.4:53", &vec![0u8; 2500]).unwrap();
        let mut cursor = &frames[0][..];
        let msg0 = UdpMessage::decode(&mut cursor).unwrap();
        assert!(asm.ingest(msg0).unwrap().is_none());
        let mut cursor = &frames[1][..];
        let mut msg1 = UdpMessage::decode(&mut cursor).unwrap();
        msg1.addr = "9.9.9.9:53".to_string();
        assert!(asm.ingest(msg1).is_err());
    }

    #[test]
    fn oversized_payload_rejected() {
        let payload = vec![0u8; MAX_UDP_PAYLOAD + 1];
        assert!(fragment_payload(1, 1, "127.0.0.1:9", &payload).is_err());
    }

    #[test]
    fn fragment_ttl_expires() {
        let mut asm = UdpReassembler::new();
        let frames = fragment_payload(1, 1, "127.0.0.1:9", &vec![0u8; 2500]).unwrap();
        let mut cursor = &frames[0][..];
        let msg = UdpMessage::decode(&mut cursor).unwrap();
        assert!(asm.ingest(msg).unwrap().is_none());
        assert_eq!(asm.pending_count(), 1);
        std::thread::sleep(FRAGMENT_TTL + Duration::from_millis(50));
        asm.prune_expired();
        assert_eq!(asm.pending_count(), 0);
    }
}
