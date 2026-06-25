//! TUN inbound via userspace IP stack (ipstack) with full TCP/UDP relay.

use crate::direct::parse_listen;
use anyhow::{Context, Result};
use async_trait::async_trait;
use ipstack::{IpStack, IpStackConfig, IpStackStream};
use rsb_core::{BoxError, Dialer, Inbound, Metadata, Network};
use rsb_dns::DnsRouter;
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct TunInbound {
    tag: String,
    name: String,
    mtu: u16,
    address: String,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl TunInbound {
    pub fn new(tag: String, raw: Value, dialer: Arc<Dialer>, dns: Arc<DnsRouter>) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            name: raw
                .get("interface_name")
                .and_then(|v| v.as_str())
                .unwrap_or("tun0")
                .to_string(),
            mtu: raw.get("mtu").and_then(|v| v.as_u64()).unwrap_or(1500) as u16,
            address: raw
                .get("address")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
                .unwrap_or("172.19.0.1/30")
                .to_string(),
            dialer,
            dns,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }
}

#[async_trait]
impl Inbound for TunInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_TUN
    }
    async fn start(&self) -> Result<(), BoxError> {
        let mut cfg = tun::Configuration::default();
        cfg.tun_name(&self.name).mtu(self.mtu).up();
        #[cfg(unix)]
        {
            cfg.address(&self.address);
        }
        #[cfg(windows)]
        {
            cfg.destination(parse_tun_gateway(&self.address));
        }
        let dev = tun::create_as_async(&cfg).map_err(|e| anyhow::anyhow!("tun create: {e}"))?;
        let mut stack_cfg = IpStackConfig::default();
        stack_cfg.mtu(self.mtu).context("ipstack mtu")?;
        let mut ip_stack = IpStack::new(stack_cfg, dev);
        tracing::info!(tag = %self.tag, name = %self.name, %self.address, "tun ipstack started");
        let dialer = self.dialer.clone();
        let dns = self.dns.clone();
        let tag = self.tag.clone();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    accepted = ip_stack.accept() => {
                        let Ok(stream) = accepted else { break };
                        let dialer = dialer.clone();
                        let dns = dns.clone();
                        let tag = tag.clone();
                        tokio::spawn(async move {
                            if let Err(err) = handle_ipstack_stream(stream, dialer, dns, tag).await {
                                tracing::debug!(error = %err, "tun flow");
                            }
                        });
                    }
                }
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }
    async fn close(&self) -> Result<(), BoxError> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}

#[cfg(windows)]
fn parse_tun_gateway(cidr: &str) -> std::net::Ipv4Addr {
    cidr.split('/')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(std::net::Ipv4Addr::new(172, 19, 0, 1))
}

async fn handle_ipstack_stream(
    stream: IpStackStream,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    inbound_tag: String,
) -> Result<()> {
    match stream {
        IpStackStream::Tcp(tcp) => handle_tun_tcp(tcp, dialer, dns, inbound_tag).await,
        IpStackStream::Udp(udp) => handle_tun_udp(udp, dialer, dns, inbound_tag).await,
        IpStackStream::UnknownTransport(_) | IpStackStream::UnknownNetwork(_) => Ok(()),
    }
}

async fn handle_tun_tcp(
    mut tcp: ipstack::IpStackTcpStream,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    inbound_tag: String,
) -> Result<()> {
    let src = tcp.local_addr();
    let dst = tcp.peer_addr();
    let (sniff, prefix) = crate::sniff::read_sniff_tcp(&mut tcp).await?;
    let process = rsb_core::lookup_process_for_tuple(src, dst);
    let resolved =
        crate::inbound_proxy::resolve_destination(&dns, dst, sniff.domain.as_deref()).await?;
    let metadata = Metadata {
        network: Network::Tcp,
        source: Some(src),
        destination: Some(resolved),
        domain: sniff.domain,
        protocol: sniff.protocol,
        process_name: process.name,
        process_path: process.path,
        inbound_tag: inbound_tag.clone(),
        inbound_type: rsb_constant::TYPE_TUN.to_string(),
    };
    let remote = dialer.dial_tcp(&metadata, resolved).await?;
    let local = crate::sniff::PrefixedStream::new(tcp, prefix);
    relay_ipstack_tcp(local, remote).await
}

async fn relay_ipstack_tcp(
    mut local: impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    mut remote: rsb_core::ProxyConn,
) -> Result<()> {
    tokio::io::copy_bidirectional(&mut local, remote.as_mut()).await?;
    Ok(())
}

async fn handle_tun_udp(
    udp: ipstack::IpStackUdpStream,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    inbound_tag: String,
) -> Result<()> {
    use rsb_core::ProxyUdpIo;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let src = udp.local_addr();
    let dst = udp.peer_addr();
    let (mut udp_reader, mut udp_writer) = tokio::io::split(udp);
    let mut first = [0u8; 65535];
    let first_n = match udp_reader.read(&mut first).await {
        Ok(0) | Err(_) => return Ok(()),
        Ok(n) => n,
    };
    let sniff = crate::sniff::sniff_udp(&first[..first_n], dst.port());
    let resolved =
        crate::inbound_proxy::resolve_destination(&dns, dst, sniff.domain.as_deref()).await?;
    let metadata = Metadata {
        network: Network::Udp,
        source: Some(src),
        destination: Some(resolved),
        domain: sniff.domain,
        protocol: sniff.protocol,
        process_name: None,
        process_path: None,
        inbound_tag: inbound_tag.clone(),
        inbound_type: rsb_constant::TYPE_TUN.to_string(),
    };
    let remote = dialer.dial_udp(&metadata, resolved).await?;
    let dest = resolved;
    let remote_send = remote.clone();
    let remote_recv = remote;
    if remote_send.send_to(&first[..first_n], dest).await.is_err() {
        return Ok(());
    }
    let up = tokio::spawn(async move {
        let mut buf = [0u8; 65535];
        loop {
            match udp_reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if remote_send.send_to(&buf[..n], dest).await.is_err() {
                        break;
                    }
                }
            }
        }
    });
    let mut buf = [0u8; 65535];
    loop {
        match remote_recv.recv_from(&mut buf).await {
            Ok((0, _)) => continue,
            Ok((n, _)) => {
                if udp_writer.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    up.abort();
    Ok(())
}

pub struct RedirectInbound {
    tag: String,
    listen: SocketAddr,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl RedirectInbound {
    pub fn new(tag: String, raw: Value, dialer: Arc<Dialer>, dns: Arc<DnsRouter>) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen: parse_listen(&raw)?,
            dialer,
            dns,
            shutdown,
            handle: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }
}

#[async_trait]
impl Inbound for RedirectInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_REDIRECT
    }
    async fn start(&self) -> Result<(), BoxError> {
        spawn_transparent_listener(
            self.tag.clone(),
            self.listen,
            rsb_constant::TYPE_REDIRECT,
            self.dialer.clone(),
            self.dns.clone(),
            self.shutdown.subscribe(),
            self.handle.clone(),
        )
        .await
    }
    async fn close(&self) -> Result<(), BoxError> {
        let _ = self.shutdown.send(true);
        Ok(())
    }
}

pub struct TproxyInbound {
    tag: String,
    listen: SocketAddr,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl TproxyInbound {
    pub fn new(tag: String, raw: Value, dialer: Arc<Dialer>, dns: Arc<DnsRouter>) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            listen: parse_listen(&raw)?,
            dialer,
            dns,
            shutdown,
            handle: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }
}

#[async_trait]
impl Inbound for TproxyInbound {
    fn tag(&self) -> &str {
        &self.tag
    }
    fn kind(&self) -> &str {
        rsb_constant::TYPE_TPROXY
    }
    async fn start(&self) -> Result<(), BoxError> {
        spawn_transparent_listener(
            self.tag.clone(),
            self.listen,
            rsb_constant::TYPE_TPROXY,
            self.dialer.clone(),
            self.dns.clone(),
            self.shutdown.subscribe(),
            self.handle.clone(),
        )
        .await
    }
    async fn close(&self) -> Result<(), BoxError> {
        let _ = self.shutdown.send(true);
        Ok(())
    }
}

async fn spawn_transparent_listener(
    tag: String,
    listen: SocketAddr,
    kind: &str,
    dialer: Arc<Dialer>,
    dns: Arc<DnsRouter>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
    handle_slot: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
) -> Result<(), BoxError> {
    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!(tag = %tag, %listen, kind, "transparent inbound listening");
    let kind = kind.to_string();
    let handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown.changed() => { if *shutdown.borrow() { break; } }
                accept = listener.accept() => {
                    let Ok((stream, peer)) = accept else { break };
                    let dialer = dialer.clone();
                    let dns = dns.clone();
                    let tag = tag.clone();
                    let kind = kind.clone();
                    tokio::spawn(async move {
                        let dest = match crate::original_dest::get_original_destination(&stream).await {
                            Ok(d) => d,
                            Err(err) => {
                                tracing::debug!(error = %err, %peer, "original dest lookup failed, using peer");
                                peer
                            }
                        };
                        let tcp_stream = stream;
                        let _ = crate::inbound_proxy::handle_redirect_stream(
                            tcp_stream, peer, &tag, &kind, dialer, dns, dest,
                        ).await;
                    });
                }
            }
        }
    });
    *handle_slot.lock().await = Some(handle);
    Ok(())
}
