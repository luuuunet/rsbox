//! RSQ control stream: PING/PONG keepalive after AUTH.

use super::protocol::{self, FRAME_PING, FRAME_PONG};
use super::traffic::{self, TrafficProfile};
use anyhow::Result;
use bytes::BytesMut;
use bytes::Buf;
use quinn::{Connection, RecvStream, SendStream};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub fn spawn_client_ping(
    connection: Arc<Connection>,
    mut send: SendStream,
    mut recv: RecvStream,
    profile: TrafficProfile,
) {
    tokio::spawn(async move {
        if let Err(err) = client_ping_loop(&mut send, &mut recv, profile).await {
            tracing::debug!(error = %err, "rsq client control loop ended");
            connection.close(0u32.into(), b"rsq control failed");
        }
    });
}

pub fn spawn_server_control(send: SendStream, recv: RecvStream) {
    spawn_server_control_with_prefix(send, recv, BytesMut::new());
}

pub fn spawn_server_control_with_prefix(
    mut send: SendStream,
    mut recv: RecvStream,
    initial: BytesMut,
) {
    tokio::spawn(async move {
        if let Err(err) = server_control_loop(&mut send, &mut recv, initial).await {
            tracing::debug!(error = %err, "rsq server control loop ended");
        }
    });
}

async fn client_ping_loop(
    send: &mut SendStream,
    recv: &mut RecvStream,
    profile: TrafficProfile,
) -> Result<()> {
    let base = profile.keepalive_jitter_base_secs();
    loop {
        tokio::time::sleep(traffic::jitter_duration(base)).await;
        let ping = protocol::encode_frame(FRAME_PING, 0, b"", protocol::random_pad_len(8, 32));
        send.write_all(&ping).await?;
        let mut buf = BytesMut::new();
        let mut chunk = [0u8; 512];
        'pong_wait: loop {
            while let Some(frame) = protocol::try_decode_frame(&buf)? {
                let total = protocol::frame_consumed_len(&buf)?;
                if frame.frame_type == FRAME_PONG {
                    buf.advance(total);
                    break 'pong_wait;
                }
                buf.advance(total);
            }
            let n = match recv.read(&mut chunk).await? {
                Some(n) => n,
                None => return Ok(()),
            };
            buf.extend_from_slice(&chunk[..n]);
            if buf.len() > 4096 {
                anyhow::bail!("rsq control response too large");
            }
        }
    }
}

async fn server_control_loop(
    send: &mut SendStream,
    recv: &mut RecvStream,
    mut buf: BytesMut,
) -> Result<()> {
    let mut chunk = [0u8; 512];
    loop {
        while let Some(frame) = protocol::try_decode_frame(&buf)? {
            let total = protocol::frame_consumed_len(&buf)?;
            match frame.frame_type {
                FRAME_PING => {
                    let pong =
                        protocol::encode_frame(FRAME_PONG, 0, b"", protocol::random_pad_len(8, 32));
                    send.write_all(&pong).await?;
                }
                _ => {}
            }
            buf.advance(total);
        }
        if buf.len() > 8192 {
            anyhow::bail!("rsq control buffer overflow");
        }
        let n = match recv.read(&mut chunk).await? {
            Some(n) => n,
            None => return Ok(()),
        };
        buf.extend_from_slice(&chunk[..n]);
    }
}
