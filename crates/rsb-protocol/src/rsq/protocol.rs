//! RSQ wire encoding (frame header + TCP/UDP relay).

use anyhow::{bail, Context, Result};
use bytes::{Buf, BufMut, BytesMut};

pub const MAGIC: &[u8; 4] = b"RSQ\x01";

pub const FRAME_AUTH_REQ: u8 = 0x01;
pub const FRAME_AUTH_OK: u8 = 0x02;
pub const FRAME_TCP_OPEN: u8 = 0x10;
pub const FRAME_TCP_OK: u8 = 0x11;
pub const FRAME_TCP_ERR: u8 = 0x12;
pub const FRAME_PING: u8 = 0x20;
pub const FRAME_PONG: u8 = 0x21;

pub const FLAG_PADDING: u8 = 0x01;

pub fn write_varint(buf: &mut BytesMut, value: u64) {
    let mut encoded = [0u8; 8];
    let len = if value <= 63 {
        encoded[0] = value as u8;
        1
    } else if value <= 16383 {
        encoded[0] = ((value >> 8) as u8) | 0x40;
        encoded[1] = value as u8;
        2
    } else if value <= 1_073_741_823 {
        encoded[0] = ((value >> 24) as u8) | 0x80;
        encoded[1] = (value >> 16) as u8;
        encoded[2] = (value >> 8) as u8;
        encoded[3] = value as u8;
        4
    } else {
        encoded[0] = ((value >> 56) as u8) | 0xc0;
        encoded[1] = (value >> 48) as u8;
        encoded[2] = (value >> 40) as u8;
        encoded[3] = (value >> 32) as u8;
        encoded[4] = (value >> 24) as u8;
        encoded[5] = (value >> 16) as u8;
        encoded[6] = (value >> 8) as u8;
        encoded[7] = value as u8;
        8
    };
    buf.put_slice(&encoded[..len]);
}

pub fn read_varint(buf: &mut impl Buf) -> Result<u64> {
    if !buf.has_remaining() {
        bail!("empty varint");
    }
    let first = buf.get_u8();
    let len = 1 << (first >> 6);
    let mut value = (first & 0x3f) as u64;
    if len > 1 {
        if buf.remaining() < len - 1 {
            bail!("truncated varint");
        }
        for _ in 1..len {
            value = (value << 8) | buf.get_u8() as u64;
        }
    }
    Ok(value)
}

pub fn encode_frame(frame_type: u8, flags: u8, payload: &[u8], pad_len: usize) -> BytesMut {
    let pad_len = pad_len.min(128);
    let mut buf = BytesMut::with_capacity(8 + payload.len() + pad_len + 1);
    buf.put_slice(MAGIC);
    buf.put_u8(frame_type);
    let flags = if pad_len > 0 {
        flags | FLAG_PADDING
    } else {
        flags
    };
    buf.put_u8(flags);
    write_varint(&mut buf, payload.len() as u64);
    buf.put_slice(payload);
    if pad_len > 0 {
        buf.put_u8(pad_len as u8);
        let mut pad = vec![0u8; pad_len];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut pad);
        buf.put_slice(&pad);
    }
    buf
}

pub struct DecodedFrame {
    pub frame_type: u8,
    pub flags: u8,
    pub payload: Vec<u8>,
}

pub fn frame_consumed_len(buf: &[u8]) -> Result<usize> {
    frame_wire_len(buf)?.context("incomplete frame")
}

pub fn try_decode_frame(buf: &[u8]) -> Result<Option<DecodedFrame>> {
    let Some(total) = frame_wire_len(buf)? else {
        return Ok(None);
    };
    if buf.len() < total {
        return Ok(None);
    }
    let frame_type = buf[4];
    let flags = buf[5];
    let mut cursor = &buf[6..];
    let payload_len = read_varint(&mut cursor)? as usize;
    let payload = cursor[..payload_len].to_vec();
    Ok(Some(DecodedFrame {
        frame_type,
        flags,
        payload,
    }))
}

fn frame_wire_len(buf: &[u8]) -> Result<Option<usize>> {
    if buf.len() < 6 {
        return Ok(None);
    }
    if &buf[..4] != MAGIC {
        bail!("bad rsq magic");
    }
    let flags = buf[5];
    let mut cursor = &buf[6..];
    let payload_len = match read_varint(&mut cursor) {
        Ok(n) => n as usize,
        Err(e) if e.to_string().contains("truncated") => return Ok(None),
        Err(e) => return Err(e),
    };
    let header_len = buf.len() - cursor.len();
    if cursor.len() < payload_len {
        return Ok(None);
    }
    let pad_extra = if flags & FLAG_PADDING != 0 {
        if cursor.len() < payload_len + 1 {
            return Ok(None);
        }
        let pad_len = cursor[payload_len] as usize;
        if pad_len > 128 {
            bail!("invalid padding length");
        }
        if cursor.len() < payload_len + 1 + pad_len {
            return Ok(None);
        }
        1 + pad_len
    } else {
        0
    };
    Ok(Some(header_len + payload_len + pad_extra))
}

pub fn encode_tcp_open(addr: &str, padding_len: usize) -> BytesMut {
    encode_frame(FRAME_TCP_OPEN, 0, addr.as_bytes(), padding_len)
}

pub fn encode_tcp_ok(padding_len: usize) -> BytesMut {
    encode_frame(FRAME_TCP_OK, 0, b"ok", padding_len)
}

pub fn encode_tcp_err(message: &str, padding_len: usize) -> BytesMut {
    encode_frame(FRAME_TCP_ERR, 0, message.as_bytes(), padding_len)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpOpenReply {
    Ok,
    Err(String),
}

pub fn decode_tcp_open(buf: &mut impl Buf) -> Result<String> {
    let frame = decode_frame(buf)?;
    if frame.frame_type != FRAME_TCP_OPEN {
        bail!("expected TCP_OPEN, got {}", frame.frame_type);
    }
    std::str::from_utf8(&frame.payload)
        .context("tcp open target utf8")
        .map(str::to_string)
}

pub fn try_decode_tcp_reply(buf: &mut bytes::BytesMut) -> Result<Option<TcpOpenReply>> {
    let Some(total) = frame_wire_len(buf)? else {
        return Ok(None);
    };
    if buf.len() < total {
        return Ok(None);
    }
    let frame = try_decode_frame(buf)?.context("partial tcp reply frame")?;
    let reply = match frame.frame_type {
        FRAME_TCP_OK => TcpOpenReply::Ok,
        FRAME_TCP_ERR => {
            let msg = std::str::from_utf8(&frame.payload)
                .unwrap_or("connect failed")
                .to_string();
            TcpOpenReply::Err(msg)
        }
        other => bail!("unexpected tcp reply frame: {other:#x}"),
    };
    buf.advance(total);
    Ok(Some(reply))
}

fn decode_frame(buf: &mut impl Buf) -> Result<DecodedFrame> {
    let snapshot = buf.chunk();
    let total = frame_wire_len(snapshot)?.context("incomplete frame")?;
    let decoded = try_decode_frame(snapshot)?.context("incomplete frame")?;
    buf.advance(total);
    Ok(decoded)
}

pub fn random_pad_len(min: usize, max: usize) -> usize {
    let span = max.saturating_sub(min) + 1;
    min + (rand::random::<u32>() as usize % span)
}

#[derive(Debug, Clone)]
pub struct UdpMessage {
    pub session_id: u32,
    pub packet_id: u16,
    pub fragment_id: u8,
    pub fragment_count: u8,
    pub addr: String,
    pub payload: Vec<u8>,
}

impl UdpMessage {
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u32(self.session_id);
        buf.put_u16(self.packet_id);
        buf.put_u8(self.fragment_id);
        buf.put_u8(self.fragment_count);
        write_varint(buf, self.addr.len() as u64);
        buf.put_slice(self.addr.as_bytes());
        buf.put_slice(&self.payload);
    }

    pub fn decode(buf: &mut impl Buf) -> Result<Self> {
        if buf.remaining() < 8 {
            bail!("truncated udp header");
        }
        let session_id = buf.get_u32();
        let packet_id = buf.get_u16();
        let fragment_id = buf.get_u8();
        let fragment_count = buf.get_u8();
        let addr_len = read_varint(buf)? as usize;
        if buf.remaining() < addr_len {
            bail!("truncated udp address");
        }
        let addr_bytes = buf.copy_to_bytes(addr_len);
        let addr = std::str::from_utf8(&addr_bytes)
            .context("udp addr utf8")?
            .to_string();
        let payload = buf.copy_to_bytes(buf.remaining()).to_vec();
        Ok(Self {
            session_id,
            packet_id,
            fragment_id,
            fragment_count,
            addr,
            payload,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip() {
        for _ in 0..50 {
            let frame = encode_frame(FRAME_AUTH_REQ, 0, b"hello", random_pad_len(0, 128));
            let decoded = try_decode_frame(&frame).unwrap().unwrap();
            assert_eq!(decoded.frame_type, FRAME_AUTH_REQ);
            assert_eq!(decoded.payload, b"hello");
        }
    }

    #[test]
    fn tcp_ok_decode_advances_buffer() {
        let mut buf = bytes::BytesMut::from(&encode_frame(FRAME_TCP_OK, 0, b"ok", 8)[..]);
        buf.extend_from_slice(b"leftover");
        assert_eq!(
            try_decode_tcp_reply(&mut buf).unwrap(),
            Some(TcpOpenReply::Ok)
        );
        assert_eq!(&buf[..], b"leftover");
    }

    #[test]
    fn tcp_err_reply() {
        let mut buf = bytes::BytesMut::from(&encode_tcp_err("refused", 4)[..]);
        assert_eq!(
            try_decode_tcp_reply(&mut buf).unwrap(),
            Some(TcpOpenReply::Err("refused".into()))
        );
    }
}
