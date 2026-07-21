//! RSQ control stream: PING/PONG keepalive after AUTH.

use super::protocol::{self, FRAME_PING, FRAME_PONG};
use super::traffic::{self, TrafficProfile};
use anyhow::Result;
use bytes::BytesMut;
use bytes::Buf;
use quinn::{Connection, RecvStream, SendStream};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 与 PING 间隔（~20s）对齐，避免一次抖动误杀。
const PONG_TIMEOUT: Duration = Duration::from_secs(30);
/// 连续 PONG 超时达到阈值才关闭整条连接。
const PONG_FAIL_CLOSE_THRESHOLD: u32 = 3;

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
    let mut consecutive_pong_timeouts: u32 = 0;
    loop {
        tokio::time::sleep(traffic::jitter_duration(base)).await;
        let ping = protocol::encode_frame(FRAME_PING, 0, b"", protocol::random_pad_len(8, 32));
        send.write_all(&ping).await?;
        match wait_for_pong(recv).await {
            Ok(()) => {
                consecutive_pong_timeouts = 0;
            }
            Err(WaitPongError::Timeout) => {
                consecutive_pong_timeouts = consecutive_pong_timeouts.saturating_add(1);
                tracing::debug!(
                    consecutive_pong_timeouts,
                    threshold = PONG_FAIL_CLOSE_THRESHOLD,
                    "rsq pong timeout (soft retry)"
                );
                if consecutive_pong_timeouts >= PONG_FAIL_CLOSE_THRESHOLD {
                    anyhow::bail!("rsq pong timeout x{consecutive_pong_timeouts}");
                }
                // soft retry：不立刻 close，下一轮再 PING。
            }
            Err(WaitPongError::Other(e)) => return Err(e),
        }
    }
}

enum WaitPongError {
    Timeout,
    Other(anyhow::Error),
}

async fn wait_for_pong(recv: &mut RecvStream) -> Result<(), WaitPongError> {
    let mut buf = BytesMut::new();
    let mut chunk = [0u8; 512];
    loop {
        while let Some(frame) = protocol::try_decode_frame(&buf).map_err(WaitPongError::Other)? {
            let total = protocol::frame_consumed_len(&buf).map_err(WaitPongError::Other)?;
            if frame.frame_type == FRAME_PONG {
                buf.advance(total);
                return Ok(());
            }
            buf.advance(total);
        }
        let n = match tokio::time::timeout(PONG_TIMEOUT, recv.read(&mut chunk)).await {
            Ok(Ok(Some(n))) => n,
            Ok(Ok(None)) => {
                return Err(WaitPongError::Other(anyhow::anyhow!(
                    "rsq control stream closed"
                )));
            }
            Ok(Err(e)) => return Err(WaitPongError::Other(e.into())),
            Err(_) => return Err(WaitPongError::Timeout),
        };
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > 4096 {
            return Err(WaitPongError::Other(anyhow::anyhow!(
                "rsq control response too large"
            )));
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
