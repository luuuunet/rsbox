use anyhow::{bail, Context, Result};
use bytes::{Buf, BufMut, BytesMut};

pub const TCP_REQUEST_ID: u64 = 0x401;
pub const TCP_STATUS_OK: u8 = 0x00;
pub const TCP_STATUS_ERROR: u8 = 0x01;

pub fn write_varint(buf: &mut BytesMut, value: u64) {
    let mut encoded = [0u8; 8];
    let len = {
        if value <= 63 {
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
        }
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

pub fn encode_tcp_request(addr: &str, padding_len: usize) -> BytesMut {
    let mut buf = BytesMut::new();
    write_varint(&mut buf, TCP_REQUEST_ID);
    write_varint(&mut buf, addr.len() as u64);
    buf.put_slice(addr.as_bytes());
    write_varint(&mut buf, padding_len as u64);
    if padding_len > 0 {
        let mut pad = vec![0u8; padding_len];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut pad);
        buf.put_slice(&pad);
    }
    buf
}

pub fn decode_tcp_request(buf: &mut impl Buf) -> Result<String> {
    let id = read_varint(buf)?;
    if id != TCP_REQUEST_ID {
        bail!("unexpected message id: {id}");
    }
    let addr_len = read_varint(buf)? as usize;
    if buf.remaining() < addr_len {
        bail!("truncated address");
    }
    let addr_bytes = buf.copy_to_bytes(addr_len);
    let addr = std::str::from_utf8(&addr_bytes).context("address utf8")?;
    let pad_len = read_varint(buf)? as usize;
    if buf.remaining() < pad_len {
        bail!("truncated padding");
    }
    buf.advance(pad_len);
    Ok(addr.to_string())
}

pub fn encode_tcp_response(ok: bool, message: &str, padding_len: usize) -> BytesMut {
    let mut buf = BytesMut::new();
    buf.put_u8(if ok { TCP_STATUS_OK } else { TCP_STATUS_ERROR });
    write_varint(&mut buf, message.len() as u64);
    buf.put_slice(message.as_bytes());
    write_varint(&mut buf, padding_len as u64);
    if padding_len > 0 {
        let mut pad = vec![0u8; padding_len];
        rand::RngCore::fill_bytes(&mut rand::rng(), &mut pad);
        buf.put_slice(&pad);
    }
    buf
}

/// Try to decode a TCP response; returns `None` if more bytes are needed.
pub fn try_decode_tcp_response(buf: &mut bytes::BytesMut) -> Result<Option<(bool, String)>> {
    if buf.is_empty() {
        return Ok(None);
    }
    let mut slice: &[u8] = buf;
    match decode_tcp_response(&mut slice) {
        Ok(result) => {
            let consumed = buf.len() - slice.len();
            buf.advance(consumed);
            Ok(Some(result))
        }
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("truncated") {
                Ok(None)
            } else {
                Err(err)
            }
        }
    }
}

pub fn decode_tcp_response(buf: &mut impl Buf) -> Result<(bool, String)> {
    if buf.remaining() < 1 {
        bail!("truncated tcp response");
    }
    let status = buf.get_u8();
    let msg_len = read_varint(buf)? as usize;
    if buf.remaining() < msg_len {
        bail!("truncated message");
    }
    let msg_bytes = buf.copy_to_bytes(msg_len);
    let message = std::str::from_utf8(&msg_bytes)
        .context("message utf8")?
        .to_string();
    let pad_len = read_varint(buf)? as usize;
    if buf.remaining() < pad_len {
        bail!("truncated padding");
    }
    buf.advance(pad_len);
    Ok((status == TCP_STATUS_OK, message))
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
    fn varint_roundtrip() {
        for value in [0u64, 1, 63, 64, 16383, 16384, 1_073_741_823] {
            let mut buf = BytesMut::new();
            write_varint(&mut buf, value);
            let mut cursor = &buf[..];
            assert_eq!(read_varint(&mut cursor).unwrap(), value);
        }
    }
}
