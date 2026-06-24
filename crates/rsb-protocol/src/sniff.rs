//! Protocol sniffing for domain extraction and L7 protocol detection.

use anyhow::Result;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SniffResult {
    pub domain: Option<String>,
    pub protocol: Option<String>,
}

pub fn sniff_tcp(data: &[u8]) -> SniffResult {
    SniffResult {
        domain: sniff_domain(data),
        protocol: sniff_tcp_protocol(data).map(str::to_string),
    }
}

pub fn sniff_udp(data: &[u8], dest_port: u16) -> SniffResult {
    SniffResult {
        domain: sniff_dns_name(data, dest_port),
        protocol: sniff_udp_protocol(data, dest_port).map(str::to_string),
    }
}

pub fn sniff_tcp_protocol(data: &[u8]) -> Option<&'static str> {
    if data.is_empty() {
        return None;
    }
    if data.starts_with(b"SSH-") {
        return Some("ssh");
    }
    if data.len() >= 20 && data.starts_with(b"\x13BitTorrent protocol") {
        return Some("bittorrent");
    }
    if data.len() >= 2 && data[0] == 0x03 && data[1] == 0x00 {
        return Some("rdp");
    }
    if looks_like_tls(data) {
        return Some("tls");
    }
    if looks_like_http(data) {
        return Some("http");
    }
    None
}

pub fn sniff_udp_protocol(data: &[u8], dest_port: u16) -> Option<&'static str> {
    if data.len() >= 20 && data[4..8] == [0x21, 0x12, 0xA4, 0x42] {
        return Some("stun");
    }
    if looks_like_quic(data) {
        return Some("quic");
    }
    if looks_like_dtls(data) {
        return Some("dtls");
    }
    if dest_port == 123 && data.len() >= 48 {
        return Some("ntp");
    }
    if looks_like_dns(data, dest_port) {
        return Some("dns");
    }
    None
}

fn looks_like_tls(data: &[u8]) -> bool {
    if data.len() < 5 {
        return false;
    }
    matches!(data[0], 0x14..=0x17) && data[1] == 0x03 && data[2] <= 0x04
}

fn looks_like_dtls(data: &[u8]) -> bool {
    if data.len() < 3 {
        return false;
    }
    matches!(data[0], 0x14..=0x19) && data[1] >= 0xFE
}

fn looks_like_quic(data: &[u8]) -> bool {
    if data.is_empty() {
        return false;
    }
    if data[0] & 0x80 != 0 {
        return data.len() >= 5;
    }
    data.len() >= 1 && data[0] & 0x40 == 0
}

fn looks_like_http(data: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(data) else {
        return false;
    };
    for method in [
        "GET ", "POST ", "PUT ", "DELETE ", "HEAD ", "OPTIONS ", "PATCH ", "CONNECT ",
    ] {
        if text.starts_with(method) {
            return true;
        }
    }
    text.starts_with("HTTP/")
}

fn looks_like_dns(data: &[u8], dest_port: u16) -> bool {
    if dest_port != 53 && dest_port != 853 {
        return false;
    }
    if data.len() < 12 {
        return false;
    }
    let flags = u16::from_be_bytes([data[2], data[3]]);
    let opcode = (flags >> 11) & 0xF;
    opcode <= 2
}

fn sniff_dns_name(data: &[u8], dest_port: u16) -> Option<String> {
    if !looks_like_dns(data, dest_port) {
        return None;
    }
    parse_dns_query_name(data)
}

fn parse_dns_query_name(data: &[u8]) -> Option<String> {
    if data.len() < 13 {
        return None;
    }
    let qd_count = u16::from_be_bytes([data[4], data[5]]);
    if qd_count == 0 {
        return None;
    }
    let mut labels = Vec::new();
    let mut pos = 12usize;
    while pos < data.len() {
        let len = data[pos] as usize;
        if len == 0 {
            break;
        }
        if len > 63 || pos + 1 + len > data.len() {
            return None;
        }
        labels.push(
            std::str::from_utf8(&data[pos + 1..pos + 1 + len])
                .ok()?
                .to_string(),
        );
        pos += 1 + len;
    }
    if labels.is_empty() {
        None
    } else {
        Some(labels.join("."))
    }
}

pub fn sniff_tls_sni(data: &[u8]) -> Option<String> {
    if data.len() < 5 || data[0] != 0x16 {
        return None;
    }
    let mut pos = 5;
    if pos + 2 > data.len() {
        return None;
    }
    pos += 2;
    if pos + 1 > data.len() || data[pos] != 0x01 {
        return None;
    }
    pos += 1;
    if pos + 3 > data.len() {
        return None;
    }
    let hs_len = u32::from_be_bytes([0, data[pos], data[pos + 1], data[pos + 2]]) as usize;
    pos += 3;
    if pos + hs_len > data.len() {
        return None;
    }
    if pos + 2 + 32 > data.len() {
        return None;
    }
    pos += 2 + 32;
    if pos + 1 > data.len() {
        return None;
    }
    let sess_len = data[pos] as usize;
    pos += 1 + sess_len;
    if pos + 2 > data.len() {
        return None;
    }
    let cs_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2 + cs_len;
    if pos + 1 > data.len() {
        return None;
    }
    pos += 1 + data[pos] as usize;
    if pos + 2 > data.len() {
        return None;
    }
    let ext_len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
    pos += 2;
    let ext_end = pos + ext_len;
    while pos + 4 <= ext_end && pos + 4 <= data.len() {
        let ext_type = u16::from_be_bytes([data[pos], data[pos + 1]]);
        let ext_size = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;
        if ext_type == 0 && pos + ext_size <= data.len() {
            let mut p = pos + 2;
            if p + 1 > data.len() {
                return None;
            }
            let name_len = data[p] as usize;
            p += 1;
            if p + name_len <= data.len() {
                return std::str::from_utf8(&data[p..p + name_len])
                    .ok()
                    .map(str::to_string);
            }
        }
        pos += ext_size;
    }
    None
}

pub fn sniff_http_host(data: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(data).ok()?;
    for line in text.lines() {
        if let Some(rest) = line
            .strip_prefix("Host:")
            .or_else(|| line.strip_prefix("host:"))
        {
            return Some(rest.trim().split(':').next()?.to_string());
        }
    }
    None
}

pub fn sniff_domain(data: &[u8]) -> Option<String> {
    sniff_tls_sni(data).or_else(|| sniff_http_host(data))
}

pub async fn peek_sniff_tcp(stream: &mut TcpStream) -> Result<SniffResult> {
    let mut buf = [0u8; 512];
    let n = stream.peek(&mut buf).await?;
    if n == 0 {
        return Ok(SniffResult::default());
    }
    Ok(sniff_tcp(&buf[..n]))
}

pub async fn read_sniff_tcp<S: AsyncRead + Unpin>(
    stream: &mut S,
) -> Result<(SniffResult, Vec<u8>)> {
    let mut buf = vec![0u8; 512];
    let n = stream.read(&mut buf).await?;
    buf.truncate(n);
    Ok((sniff_tcp(&buf), buf))
}

/// Replays an initial read prefix before continuing with the inner stream.
pub struct PrefixedStream<S> {
    inner: S,
    prefix: Vec<u8>,
    pos: usize,
}

impl<S> PrefixedStream<S> {
    pub fn new(inner: S, prefix: Vec<u8>) -> Self {
        Self {
            inner,
            prefix,
            pos: 0,
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for PrefixedStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        if this.pos < this.prefix.len() {
            let remain = &this.prefix[this.pos..];
            let n = remain.len().min(buf.remaining());
            buf.put_slice(&remain[..n]);
            this.pos += n;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for PrefixedStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.as_mut().get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.as_mut().get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.as_mut().get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniff_tls_protocol() {
        let tls = [0x16, 0x03, 0x01, 0x00, 0x05, 0x01];
        assert_eq!(sniff_tcp_protocol(&tls), Some("tls"));
    }

    #[test]
    fn sniff_http_protocol() {
        let data = b"GET / HTTP/1.1\r\nHost: example.org\r\n\r\n";
        let result = sniff_tcp(data);
        assert_eq!(result.protocol.as_deref(), Some("http"));
        assert_eq!(result.domain.as_deref(), Some("example.org"));
    }

    #[test]
    fn sniff_udp_stun_and_quic() {
        let mut stun = vec![0u8; 20];
        stun[4..8].copy_from_slice(&[0x21, 0x12, 0xA4, 0x42]);
        assert_eq!(sniff_udp_protocol(&stun, 3478), Some("stun"));

        let quic = [0xC0, 0x00, 0x00, 0x01, 0x08];
        assert_eq!(sniff_udp_protocol(&quic, 443), Some("quic"));
    }
}
