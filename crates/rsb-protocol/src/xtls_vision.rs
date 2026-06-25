//! XTLS Vision (xtls-rprx-vision) — Xray-compatible padding + direct copy.

use rsb_core::{proxy_box, ProxyConn};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

const CMD_PADDING_CONTINUE: u8 = 0x00;
const CMD_PADDING_END: u8 = 0x01;
const CMD_PADDING_DIRECT: u8 = 0x02;

const TLS_APP_DATA: u8 = 0x17;
const TLS_MAJOR: u8 = 0x03;
const TLS_V13: u8 = 0x03;

pub fn is_vision_flow(flow: Option<&str>) -> bool {
    matches!(
        flow,
        Some(f) if f == "xtls-rprx-vision" || f == "xtls-rprx-vision-udp443"
    )
}

/// Protobuf addons for XRV flow (Xray `vless.Addons{Flow}`).
pub fn encode_vision_addons(flow: &str) -> Vec<u8> {
    let flow_bytes = flow.as_bytes();
    let mut pb = Vec::with_capacity(2 + flow_bytes.len());
    pb.push(0x0a);
    pb.push(flow_bytes.len() as u8);
    pb.extend_from_slice(flow_bytes);
    pb
}

pub fn encode_vless_addons(flow: Option<&str>) -> Vec<u8> {
    match flow.filter(|f| !f.is_empty()) {
        Some(f) if is_vision_flow(Some(f)) => encode_vision_addons(f),
        Some(f) => {
            let mut out = Vec::with_capacity(1 + f.len());
            out.push(f.len() as u8);
            out.extend_from_slice(f.as_bytes());
            out
        },
        None => vec![0],
    }
}

/// Read VLESS response: version(1) + addon_len(1) + addons.
pub async fn read_vless_response<S>(tls: &mut S) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let mut ver = [0u8; 1];
    tls.read_exact(&mut ver).await?;
    let mut addon_len = [0u8; 1];
    tls.read_exact(&mut addon_len).await?;
    let n = addon_len[0] as usize;
    if n > 0 {
        let mut skip = vec![0u8; n];
        tls.read_exact(&mut skip).await?;
    }
    Ok(())
}

/// After VLESS request, wrap TLS stream with Vision read/write.
pub async fn vision_relay<S>(mut tls: S, uuid: Uuid) -> anyhow::Result<ProxyConn>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + Sync + 'static,
{
    read_vless_response(&mut tls).await?;
    let mut stream = VisionStream {
        inner: tls,
        uuid,
        user_uuid_sent: false,
        is_padding: true,
        direct: false,
        read_state: ReadPadState::default(),
        filter_packets: 8,
        is_tls13: false,
        write_buf: Vec::new(),
        read_buf: Vec::new(),
    };
    // Initial long padding block (hides VLESS header from traffic shape analysis).
    let pad = xtls_padding(None, CMD_PADDING_CONTINUE, Some(uuid.as_bytes()), true);
    stream.write_all(&pad).await?;
    Ok(proxy_box(stream))
}

#[derive(Default)]
struct ReadPadState {
    remaining_cmd: i32,
    remaining_content: i32,
    remaining_pad: i32,
    current_cmd: i32,
}

struct VisionStream<S> {
    inner: S,
    uuid: Uuid,
    user_uuid_sent: bool,
    is_padding: bool,
    direct: bool,
    read_state: ReadPadState,
    filter_packets: u32,
    is_tls13: bool,
    write_buf: Vec<u8>,
    read_buf: Vec<u8>,
}

impl<S> VisionStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn filter_tls(&mut self, data: &[u8]) {
        if self.filter_packets == 0 || data.len() < 6 {
            return;
        }
        self.filter_packets = self.filter_packets.saturating_sub(1);
        if data[0] == 0x16 && data[1] == TLS_MAJOR {
            if data.len() >= 6 && data[5] == 0x01 {
                self.is_tls13 = true;
            }
            if data.len() >= 6 && data[5] == 0x02 {
                self.is_tls13 = true;
            }
        }
        if data.len() >= 5
            && data[0] == TLS_APP_DATA
            && data[1] == TLS_MAJOR
            && data[2] == TLS_V13
            && is_complete_tls_record(data)
        {
            self.direct = true;
            self.is_padding = false;
        }
    }

    fn process_read_padding(&mut self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut cursor = input;
        let st = &mut self.read_state;
        if st.remaining_cmd == 0 && st.remaining_content == 0 && st.remaining_pad == 0 {
            st.remaining_cmd = -1;
            st.remaining_content = -1;
            st.remaining_pad = -1;
        }
        while !cursor.is_empty() {
            if st.remaining_cmd > 0 {
                let b = cursor[0];
                cursor = &cursor[1..];
                match st.remaining_cmd {
                    5 => st.current_cmd = b as i32,
                    4 => st.remaining_content = (b as i32) << 8,
                    3 => st.remaining_content |= b as i32,
                    2 => st.remaining_pad = (b as i32) << 8,
                    1 => st.remaining_pad |= b as i32,
                    _ => {},
                }
                st.remaining_cmd -= 1;
            } else if st.remaining_content > 0 {
                let n = (st.remaining_content as usize).min(cursor.len());
                out.extend_from_slice(&cursor[..n]);
                cursor = &cursor[n..];
                st.remaining_content -= n as i32;
            } else if st.remaining_pad > 0 {
                let n = (st.remaining_pad as usize).min(cursor.len());
                cursor = &cursor[n..];
                st.remaining_pad -= n as i32;
            } else if st.remaining_cmd == -1
                && cursor.len() >= 16
                && cursor[..16] == *self.uuid.as_bytes()
            {
                cursor = &cursor[16..];
                st.remaining_cmd = 5;
            } else {
                out.extend_from_slice(cursor);
                break;
            }
            if st.remaining_cmd <= 0
                && st.remaining_content <= 0
                && st.remaining_pad <= 0
                && st.remaining_cmd != -1
            {
                if st.current_cmd == CMD_PADDING_DIRECT as i32 {
                    self.direct = true;
                    self.is_padding = false;
                }
                if st.current_cmd == CMD_PADDING_CONTINUE as i32 {
                    st.remaining_cmd = 5;
                } else {
                    st.remaining_cmd = -1;
                    st.remaining_content = -1;
                    st.remaining_pad = -1;
                }
            }
        }
        out
    }

    fn wrap_write(&mut self, data: &[u8]) -> Vec<u8> {
        if self.direct {
            return data.to_vec();
        }
        self.filter_tls(data);
        if self.is_padding {
            let complete = is_complete_tls_record(data);
            let is_tls_app = data.len() >= 3
                && data[0] == TLS_APP_DATA
                && data[1] == TLS_MAJOR
                && (data[2] == TLS_V13 || data[2] == 0x01);
            let cmd = if is_tls_app && complete && self.is_tls13 {
                self.direct = true;
                self.is_padding = false;
                CMD_PADDING_DIRECT
            } else if self.filter_packets <= 1 {
                CMD_PADDING_END
            } else {
                CMD_PADDING_CONTINUE
            };
            let uuid_once = if !self.user_uuid_sent {
                self.user_uuid_sent = true;
                Some(self.uuid.as_bytes().as_slice())
            } else {
                None
            };
            return xtls_padding(Some(data), cmd, uuid_once, self.is_tls13);
        }
        data.to_vec()
    }
}

impl<S> AsyncRead for VisionStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.direct {
            return std::pin::Pin::new(&mut self.get_mut().inner).poll_read(cx, buf);
        }
        loop {
            if !self.read_buf.is_empty() {
                let n = self.read_buf.len().min(buf.remaining());
                buf.put_slice(&self.read_buf[..n]);
                self.read_buf.drain(..n);
                return std::task::Poll::Ready(Ok(()));
            }
            let mut tmp = [0u8; 8192];
            let mut tmp_buf = tokio::io::ReadBuf::new(&mut tmp);
            match std::pin::Pin::new(&mut self.inner).poll_read(cx, &mut tmp_buf) {
                std::task::Poll::Pending => return std::task::Poll::Pending,
                std::task::Poll::Ready(Err(e)) => return std::task::Poll::Ready(Err(e)),
                std::task::Poll::Ready(Ok(())) => {
                    let n = tmp_buf.filled().len();
                    if n == 0 {
                        return std::task::Poll::Ready(Ok(()));
                    }
                    let plain = self.process_read_padding(&tmp[..n]);
                    self.filter_tls(&plain);
                    if plain.is_empty() {
                        continue;
                    }
                    let put = plain.len().min(buf.remaining());
                    buf.put_slice(&plain[..put]);
                    if plain.len() > put {
                        self.read_buf.extend_from_slice(&plain[put..]);
                    }
                    return std::task::Poll::Ready(Ok(()));
                },
            }
        }
    }
}

impl<S> AsyncWrite for VisionStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let framed = self.wrap_write(buf);
        if self.direct {
            return std::pin::Pin::new(&mut self.get_mut().inner).poll_write(cx, &framed);
        }
        std::pin::Pin::new(&mut self.inner).poll_write(cx, &framed)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.direct {
            return std::pin::Pin::new(&mut self.get_mut().inner).poll_flush(cx);
        }
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        if self.direct {
            return std::pin::Pin::new(&mut self.get_mut().inner).poll_shutdown(cx);
        }
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

fn xtls_padding(
    content: Option<&[u8]>,
    command: u8,
    user_uuid: Option<&[u8]>,
    long_pad: bool,
) -> Vec<u8> {
    let content_len = content.map(|c| c.len()).unwrap_or(0) as i32;
    let padding_len = if content_len < 900 && long_pad {
        rand::random::<u32>() % 500 + 900 - content_len as u32
    } else {
        rand::random::<u32>() % 256
    } as i32;
    let padding_len = padding_len.min(65535 - 21 - content_len);
    let mut out = Vec::new();
    if let Some(uuid) = user_uuid {
        out.extend_from_slice(uuid);
    }
    out.push(command);
    out.push((content_len >> 8) as u8);
    out.push(content_len as u8);
    out.push((padding_len >> 8) as u8);
    out.push(padding_len as u8);
    if let Some(c) = content {
        out.extend_from_slice(c);
    }
    out.resize(out.len() + padding_len as usize, 0);
    out
}

fn is_complete_tls_record(data: &[u8]) -> bool {
    let mut i = 0;
    let total = data.len();
    while i < total {
        if total - i < 5 {
            return false;
        }
        if data[i] != TLS_APP_DATA || data[i + 1] != TLS_MAJOR || data[i + 2] != TLS_V13 {
            return false;
        }
        let record_len = ((data[i + 3] as usize) << 8) | data[i + 4] as usize;
        if total - i < 5 + record_len {
            return false;
        }
        i += 5 + record_len;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vision_protobuf_addon_roundtrip_len() {
        let pb = encode_vision_addons("xtls-rprx-vision");
        assert_eq!(pb[0], 0x0a);
        assert_eq!(pb[1], 16);
    }

    #[test]
    fn complete_tls_record_check() {
        let mut rec = vec![0x17, 0x03, 0x03, 0x00, 0x02, 0x01, 0x02];
        assert!(is_complete_tls_record(&rec));
        rec.pop();
        assert!(!is_complete_tls_record(&rec));
    }
}
