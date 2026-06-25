//! XUDP (mux session 0) per-packet UDP addressing for VLESS / VMess.

use async_trait::async_trait;
use blake3::Hasher;
use rsb_core::{ProxyUdpIo, ProxyUdpSocket};
use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::Mutex;

pub const MUX_ADDR: &str = "v1.mux.cool";
pub const MUX_XUDP_PORT: u16 = 666;
const SESSION_ID: u16 = 0;
const STATUS_NEW: u8 = 0x01;
const STATUS_KEEP: u8 = 0x02;
const OPT_DATA: u8 = 0x01;
const NET_UDP: u8 = 0x02;

pub fn mux_xudp_target() -> (String, u16) {
    (MUX_ADDR.to_string(), MUX_XUDP_PORT)
}

pub fn global_id_for(source: Option<SocketAddr>) -> [u8; 8] {
    let mut h = Hasher::new();
    if let Some(src) = source {
        match src.ip() {
            std::net::IpAddr::V4(v4) => {
                h.update(&v4.octets());
            },
            std::net::IpAddr::V6(v6) => {
                h.update(&v6.octets());
            },
        }
        h.update(&src.port().to_be_bytes());
    } else {
        h.update(b"rsbox-xudp");
    }
    let hash = h.finalize();
    let mut out = [0u8; 8];
    out.copy_from_slice(&hash.as_bytes()[..8]);
    out
}

pub async fn xudp_over_stream<S>(stream: S, source: Option<SocketAddr>) -> ProxyUdpSocket
where
    S: AsyncRead + AsyncWrite + Send + Sync + Unpin + 'static,
{
    let (reader, writer) = tokio::io::split(stream);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let global_id = global_id_for(source);
    tokio::spawn(async move {
        let mut reader = reader;
        loop {
            match read_xudp_frame(&mut reader).await {
                Ok(Some((payload, addr))) => {
                    let _ = tx.send((payload, addr));
                },
                Ok(None) => break,
                Err(_) => break,
            }
        }
    });
    ProxyUdpSocket::from_io(Arc::new(XudpIo {
        writer: Mutex::new(XudpWriter {
            writer,
            global_id,
            seen: HashMap::new(),
        }),
        rx: Mutex::new(rx),
    }))
}

struct XudpIo<W> {
    writer: Mutex<XudpWriter<W>>,
    rx: Mutex<tokio::sync::mpsc::UnboundedReceiver<(Vec<u8>, SocketAddr)>>,
}

struct XudpWriter<W> {
    writer: W,
    global_id: [u8; 8],
    seen: HashMap<SocketAddr, bool>,
}

#[async_trait]
impl<W> ProxyUdpIo for XudpIo<W>
where
    W: AsyncWrite + Send + Sync + Unpin,
{
    async fn send_to(&self, buf: &[u8], target: SocketAddr) -> std::io::Result<usize> {
        let mut guard = self.writer.lock().await;
        let is_new = !guard.seen.contains_key(&target);
        if is_new {
            guard.seen.insert(target, true);
        }
        let frame = encode_xudp_frame(buf, target, guard.global_id, is_new);
        guard.writer.write_all(&frame).await?;
        Ok(buf.len())
    }

    async fn recv_from(&self, buf: &mut [u8]) -> std::io::Result<(usize, SocketAddr)> {
        let mut guard = self.rx.lock().await;
        let (payload, addr) = guard.recv().await.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "xudp stream closed")
        })?;
        let n = payload.len().min(buf.len());
        buf[..n].copy_from_slice(&payload[..n]);
        Ok((n, addr))
    }

    fn local_addr(&self) -> std::io::Result<SocketAddr> {
        Ok("0.0.0.0:0".parse().unwrap())
    }
}

fn encode_xudp_frame(
    payload: &[u8],
    dest: SocketAddr,
    global_id: [u8; 8],
    is_new: bool,
) -> Vec<u8> {
    let mut meta = Vec::new();
    meta.extend_from_slice(&SESSION_ID.to_be_bytes());
    meta.push(if is_new { STATUS_NEW } else { STATUS_KEEP });
    meta.push(OPT_DATA);
    meta.push(NET_UDP);
    meta.extend_from_slice(&dest.port().to_be_bytes());
    meta.extend_from_slice(&encode_address(dest));
    if is_new {
        meta.extend_from_slice(&global_id);
    }
    let mut out = Vec::with_capacity(2 + meta.len() + 2 + payload.len());
    out.extend_from_slice(&(meta.len() as u16).to_be_bytes());
    out.extend_from_slice(&meta);
    out.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn encode_address(addr: SocketAddr) -> Vec<u8> {
    let mut out = Vec::new();
    match addr {
        SocketAddr::V4(v4) => {
            out.push(0x01);
            out.extend_from_slice(&v4.ip().octets());
        },
        SocketAddr::V6(v6) => {
            out.push(0x03);
            out.extend_from_slice(&v6.ip().octets());
        },
    }
    out
}

async fn read_xudp_frame<R>(reader: &mut R) -> std::io::Result<Option<(Vec<u8>, SocketAddr)>>
where
    R: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 2];
    if reader.read_exact(&mut len_buf).await? == 0 && len_buf == [0, 0] {
        return Ok(None);
    }
    let meta_len = u16::from_be_bytes(len_buf) as usize;
    let mut meta = vec![0u8; meta_len];
    reader.read_exact(&mut meta).await?;
    if meta.len() < 6 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "short xudp meta",
        ));
    }
    let mut cursor = &meta[4..]; // skip session + status + option
    if cursor.is_empty() || cursor[0] != NET_UDP {
        // skip non-udp mux frames
        let mut skip = [0u8; 2];
        reader.read_exact(&mut skip).await?;
        let plen = u16::from_be_bytes(skip) as usize;
        let mut junk = vec![0u8; plen];
        reader.read_exact(&mut junk).await?;
        return Ok(None);
    }
    cursor = &cursor[1..];
    if cursor.len() < 3 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "short xudp header",
        ));
    }
    let port = u16::from_be_bytes([cursor[0], cursor[1]]);
    let (host, consumed) = decode_address(&cursor[2..])?;
    cursor = &cursor[2 + consumed..];
    let _status = meta[3];
    if meta[2] == STATUS_NEW && cursor.len() >= 8 {
        let _reserved = &cursor[..8]; // 读取但不使用
        cursor = &cursor[8..];
    }
    let mut plen_buf = [0u8; 2];
    reader.read_exact(&mut plen_buf).await?;
    let plen = u16::from_be_bytes(plen_buf) as usize;
    let mut payload = vec![0u8; plen];
    reader.read_exact(&mut payload).await?;
    let dest = resolve_host_port(&host, port).await?;
    Ok(Some((payload, dest)))
}

fn decode_address(buf: &[u8]) -> std::io::Result<(String, usize)> {
    if buf.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "empty address",
        ));
    }
    match buf[0] {
        0x01 if buf.len() >= 5 => {
            let ip = Ipv4Addr::new(buf[1], buf[2], buf[3], buf[4]);
            Ok((ip.to_string(), 5))
        },
        0x02 => {
            let len = buf[1] as usize;
            if buf.len() < 2 + len {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "short domain",
                ));
            }
            let host = std::str::from_utf8(&buf[2..2 + len])
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok((host.to_string(), 2 + len))
        },
        0x03 if buf.len() >= 17 => {
            let mut oct = [0u8; 16];
            oct.copy_from_slice(&buf[1..17]);
            Ok((Ipv6Addr::from(oct).to_string(), 17))
        },
        other => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("bad atyp {other}"),
        )),
    }
}

async fn resolve_host_port(host: &str, port: u16) -> std::io::Result<SocketAddr> {
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Ok(SocketAddr::new(ip, port));
    }
    let mut addrs = tokio::net::lookup_host(format!("{host}:{port}")).await?;
    addrs
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "resolve xudp dest"))
}

/// VLESS mux command (3) header target for XUDP mode.
pub fn vless_xudp_request_header(uuid: uuid::Uuid, flow: Option<&str>) -> Vec<u8> {
    let mut buf = Vec::new();
    buf.push(0);
    buf.extend_from_slice(uuid.as_bytes());
    if let Some(flow) = flow.filter(|f| !f.is_empty()) {
        buf.push(flow.len() as u8);
        buf.extend_from_slice(flow.as_bytes());
    } else {
        buf.push(0);
    }
    buf.push(3); // mux
    buf.extend_from_slice(&MUX_XUDP_PORT.to_be_bytes());
    buf.push(0x02); // domain
    buf.push(MUX_ADDR.len() as u8);
    buf.extend_from_slice(MUX_ADDR.as_bytes());
    buf
}
