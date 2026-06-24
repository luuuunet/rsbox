//! DNS inbound — UDP/TCP DNS server forwarding to DnsRouter.

use crate::direct::parse_listen;
use anyhow::{Context, Result};
use async_trait::async_trait;
use rsb_core::{BoxError, Inbound};
use rsb_dns::DnsRouter;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;

pub struct DnsInbound {
    tag: String,
    listen: SocketAddr,
    dns: Arc<DnsRouter>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handles: tokio::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl DnsInbound {
    pub fn new(tag: String, raw: Value, dns: Arc<DnsRouter>) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen: parse_listen(&raw)?,
            dns,
            shutdown,
            handles: tokio::sync::Mutex::new(Vec::new()),
        })
    }
}

#[async_trait]
impl Inbound for DnsInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_DNS
    }
    async fn start(&self) -> Result<(), BoxError> {
        let udp = Arc::new(UdpSocket::bind(self.listen).await?);
        tracing::info!(tag = %self.tag, %self.listen, "dns inbound listening (udp)");
        let dns = self.dns.clone();
        let mut shutdown = self.shutdown.subscribe();
        let udp_sock = udp.clone();
        let udp_handle = tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                tokio::select! {
                    _ = shutdown.changed() => { if *shutdown.borrow() { break; } }
                    recv = udp_sock.recv_from(&mut buf) => {
                        let Ok((n, peer)) = recv else { break };
                        let query = buf[..n].to_vec();
                        let dns = dns.clone();
                        let reply = udp.clone();
                        tokio::spawn(async move {
                            if let Ok(resp) = handle_dns_query(&dns, &query).await {
                                let _ = reply.send_to(&resp, peer).await;
                            }
                        });
                    }
                }
            }
        });
        let tcp_listen = self.listen;
        let dns_tcp = self.dns.clone();
        let mut shutdown_tcp = self.shutdown.subscribe();
        let tcp_handle = tokio::spawn(async move {
            let Ok(listener) = tokio::net::TcpListener::bind(tcp_listen).await else {
                return;
            };
            loop {
                tokio::select! {
                    _ = shutdown_tcp.changed() => { if *shutdown_tcp.borrow() { break; } }
                    accept = listener.accept() => {
                        let Ok((mut stream, _)) = accept else { break };
                        let dns = dns_tcp.clone();
                        tokio::spawn(async move {
                            let _ = serve_dns_tcp(&mut stream, &dns).await;
                        });
                    }
                }
            }
        });
        self.handles.lock().await.extend([udp_handle, tcp_handle]);
        Ok(())
    }
    async fn close(&self) -> Result<(), BoxError> {
        let _ = self.shutdown.send(true);
        for h in self.handles.lock().await.drain(..) {
            h.abort();
        }
        Ok(())
    }
}

async fn handle_dns_query(dns: &DnsRouter, query: &[u8]) -> Result<Vec<u8>> {
    if let Ok(addrs) = dns.exchange(query).await {
        if !addrs.is_empty() {
            return Ok(addrs);
        }
    }
    let host = extract_query_name(query).context("dns query name")?;
    let addrs = dns.lookup(&host).await?;
    rsb_dns::build_response(query, &addrs)
}

async fn serve_dns_tcp(stream: &mut tokio::net::TcpStream, dns: &DnsRouter) -> Result<()> {
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;
    let mut query = vec![0u8; len];
    stream.read_exact(&mut query).await?;
    let resp = handle_dns_query(dns, &query).await?;
    let out_len = (resp.len() as u16).to_be_bytes();
    stream.write_all(&out_len).await?;
    stream.write_all(&resp).await?;
    Ok(())
}

fn extract_query_name(query: &[u8]) -> Result<String> {
    if query.len() < 13 {
        anyhow::bail!("short query");
    }
    let mut offset = 12;
    let mut labels = Vec::new();
    loop {
        if offset >= query.len() {
            break;
        }
        let len = query[offset];
        if len == 0 {
            break;
        }
        offset += 1;
        if offset + len as usize > query.len() {
            break;
        }
        labels.push(String::from_utf8_lossy(&query[offset..offset + len as usize]).into_owned());
        offset += len as usize;
    }
    if labels.is_empty() {
        anyhow::bail!("empty qname");
    }
    Ok(labels.join("."))
}
