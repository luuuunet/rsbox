use super::protocol::{decode_tcp_request, encode_tcp_response};
use anyhow::{Context, Result};
use quinn::RecvStream;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub async fn handle_tcp_stream(mut send: quinn::SendStream, mut recv: RecvStream) -> Result<()> {
    let mut header = vec![0u8; 4096];
    let n = recv
        .read(&mut header)
        .await
        .context("read tcp request")?
        .ok_or_else(|| anyhow::anyhow!("stream closed before request"))?;
    let mut cursor = &header[..n];
    let target = decode_tcp_request(&mut cursor).context("decode tcp request")?;
    let addr = resolve_target(&target).await?;

    let remote = TcpStream::connect(addr)
        .await
        .with_context(|| format!("connect {target}"))?;

    let ok_buf = encode_tcp_response(true, "ok", 0);
    send.write_all(&ok_buf).await.context("write tcp ok")?;

    let (mut remote_read, mut remote_write) = remote.into_split();

    let client_to_remote = async {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = recv
                .read(&mut buf)
                .await
                .context("read client stream")?
                .unwrap_or(0);
            if n == 0 {
                break;
            }
            remote_write.write_all(&buf[..n]).await?;
        }
        Ok::<_, anyhow::Error>(())
    };

    let remote_to_client = async {
        let mut buf = vec![0u8; 16 * 1024];
        loop {
            let n = remote_read.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            send.write_all(&buf[..n]).await?;
        }
        Ok::<_, anyhow::Error>(())
    };

    tokio::try_join!(client_to_remote, remote_to_client)?;
    Ok(())
}

async fn resolve_target(target: &str) -> Result<SocketAddr> {
    if let Ok(addr) = target.parse::<SocketAddr>() {
        return Ok(addr);
    }
    tokio::net::lookup_host(target)
        .await
        .context("resolve hysteria2 target")?
        .next()
        .with_context(|| format!("no addresses for {target}"))
}

pub async fn parse_udp_target(addr: &str) -> Result<SocketAddr> {
    resolve_target(addr).await
}

pub async fn forward_udp_payload(
    socket: &tokio::net::UdpSocket,
    target: SocketAddr,
    payload: &[u8],
) -> Result<usize> {
    socket.send_to(payload, target).await.context("udp send")
}

pub fn ensure_fragment_ready(msg: &super::protocol::UdpMessage) -> Result<()> {
    if msg.fragment_count != 1 {
        anyhow::bail!("udp fragmentation is not supported yet");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_ip_literal() {
        let addr = resolve_target("127.0.0.1:65535").await.unwrap();
        assert_eq!(addr.port(), 65535);
    }
}
