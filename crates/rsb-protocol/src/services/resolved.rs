//! systemd-resolved style DNS listener (forwards to DnsRouter).

use super::context::ServiceContext;
use super::listen::parse_listen;
use anyhow::Result;
use rsb_dns::{register_resolved_service, unregister_resolved_service};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tracing::info;

pub struct ResolvedService {
    tag: String,
    listen: SocketAddr,
    ctx: ServiceContext,
    shutdown: tokio::sync::watch::Sender<bool>,
    udp_handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    tcp_handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl ResolvedService {
    pub fn new(tag: String, raw: Value, ctx: ServiceContext) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen: parse_listen(&raw).unwrap_or_else(|_| "127.0.0.53:53".parse().unwrap()),
            ctx,
            shutdown,
            udp_handle: tokio::sync::Mutex::new(None),
            tcp_handle: tokio::sync::Mutex::new(None),
        })
    }

    pub async fn start(&self) -> Result<()> {
        register_resolved_service(&self.tag, self.ctx.dns.clone());
        let udp = UdpSocket::bind(self.listen).await?;
        info!(tag = %self.tag, %self.listen, "resolved dns service listening (udp)");
        let dns = self.ctx.dns.clone();
        let mut shutdown = self.shutdown.subscribe();
        let udp_task = tokio::spawn(async move {
            let mut buf = [0u8; 4096];
            loop {
                tokio::select! {
                    _ = shutdown.changed() => { if *shutdown.borrow() { break; } }
                    recv = udp.recv_from(&mut buf) => {
                        let Ok((n, peer)) = recv else { break };
                        if let Ok(resp) = dns.exchange_upstream(&buf[..n]).await {
                            let _ = udp.send_to(&resp, peer).await;
                        }
                    }
                }
            }
        });
        *self.udp_handle.lock().await = Some(udp_task);

        let listen = self.listen;
        let dns = self.ctx.dns.clone();
        let mut shutdown = self.shutdown.subscribe();
        let tcp_task = tokio::spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::bind(listen).await else {
                return;
            };
            loop {
                tokio::select! {
                    _ = shutdown.changed() => { if *shutdown.borrow() { break; } }
                    accept = listener.accept() => {
                        let Ok((mut stream, _)) = accept else { break };
                        let dns = dns.clone();
                        tokio::spawn(async move {
                            let mut len_buf = [0u8; 2];
                            if stream.read_exact(&mut len_buf).await.is_err() {
                                return;
                            }
                            let len = u16::from_be_bytes(len_buf) as usize;
                            if len == 0 || len > 4096 {
                                return;
                            }
                            let mut query = vec![0u8; len];
                            if stream.read_exact(&mut query).await.is_err() {
                                return;
                            }
                            if let Ok(resp) = dns.exchange_upstream(&query).await {
                                let out_len = (resp.len() as u16).to_be_bytes();
                                let _ = stream.write_all(&out_len).await;
                                let _ = stream.write_all(&resp).await;
                            }
                        });
                    }
                }
            }
        });
        *self.tcp_handle.lock().await = Some(tcp_task);
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        unregister_resolved_service(&self.tag);
        let _ = self.shutdown.send(true);
        if let Some(h) = self.udp_handle.lock().await.take() {
            h.abort();
        }
        if let Some(h) = self.tcp_handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}
