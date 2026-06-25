//! Boringtun WireGuard userspace tunnel with TUN inject/eject.

use anyhow::{Context, Result};
use base64::Engine;
use boringtun::noise::{Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use serde_json::Value;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tracing::info;

pub struct WireGuardTunnel {
    tag: String,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl WireGuardTunnel {
    pub fn new(tag: String) -> Self {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Self {
            tag,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn start(&self, raw: Value) -> Result<()> {
        let cfg = parse_config(&raw)?;
        let udp = Arc::new(
            UdpSocket::bind(format!("0.0.0.0:{}", cfg.listen_port))
                .await
                .context("wireguard bind")?,
        );
        let tun_dev = create_tun(&cfg)?;
        let tun = Arc::new(Mutex::new(tun_dev));

        let mut peer_map: HashMap<[u8; 32], PeerState> = HashMap::new();
        for (i, peer) in cfg.peers.iter().enumerate() {
            let tunn = Tunn::new(
                cfg.private_key.clone(),
                peer.public_key,
                None,
                Some(peer.keepalive),
                i as u32,
                None,
            )
            .map_err(|e| anyhow::anyhow!("wireguard tunn init: {e}"))?;
            peer_map.insert(
                peer.public_key.to_bytes(),
                PeerState {
                    tunn,
                    allowed: peer.allowed.clone(),
                    endpoint: peer.endpoint,
                },
            );
        }
        let peers: Arc<Mutex<HashMap<[u8; 32], PeerState>>> = Arc::new(Mutex::new(peer_map));

        let tag = self.tag.clone();
        let listen_port = cfg.listen_port;
        let interface_name = cfg.interface_name.clone();
        let address = cfg.address.clone();
        let peer_count = peers.lock().await.len();
        info!(
            tag = %tag,
            port = listen_port,
            interface = %interface_name,
            address = %address,
            peers = peer_count,
            "wireguard boringtun + TUN started"
        );

        let sock = udp.clone();
        let tun_io = tun.clone();
        let peers_udp = peers.clone();
        let peers_tun = peers.clone();
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            let mut udp_buf = vec![0u8; 65535];
            let mut wg_out = vec![0u8; 65535];
            let mut tun_buf = vec![0u8; 65535];
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
            loop {
                tokio::select! {
                    _ = shutdown.changed() => {
                        if *shutdown.borrow() { break; }
                    }
                    _ = tick.tick() => {
                        let mut guard = peers_udp.lock().await;
                        for state in guard.values_mut() {
                            if let Some(ep) = state.endpoint {
                                drain_tunn(&sock, &mut state.tunn, ep, &mut wg_out).await;
                            }
                        }
                    }
                    recv = sock.recv_from(&mut udp_buf) => {
                        let Ok((n, src)) = recv else { break };
                        let mut guard = peers_udp.lock().await;

                        // Try to match packet to correct peer
                        // Only update endpoint for the matched peer
                        let mut found = false;
                        for state in guard.values_mut() {
                            if handle_datagram(
                                &sock,
                                &tun_io,
                                &mut state.tunn,
                                Some(src.ip()),
                                &udp_buf[..n],
                                src,
                                &mut wg_out,
                            ).await {
                                // Update endpoint only for matched peer
                                state.endpoint = Some(src);
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            tracing::debug!("WireGuard: unmatched packet from {}", src);
                        }
                    }
                    read = async {
                        let mut dev = tun_io.lock().await;
                        dev.read(&mut tun_buf).await
                    } => {
                        let Ok(n) = read else { continue };
                        if n == 0 { continue; }
                        let dest = match parse_ip_dest(&tun_buf[..n]) {
                            Some(d) => d,
                            None => continue,
                        };
                        let packet = tun_buf[..n].to_vec();
                        let mut guard = peers_tun.lock().await;
                        let key = guard
                            .iter()
                            .find(|(_, p)| p.matches(dest))
                            .map(|(k, _)| *k);
                        let Some(key) = key else {
                            continue;
                        };
                        let state = guard.get_mut(&key).unwrap();
                        let Some(ep) = state.endpoint else {
                            continue;
                        };
                        encapsulate_to_peer(&sock, &mut state.tunn, ep, &packet, &mut wg_out).await;
                    }
                }
            }
        });
        *self.handle.lock().await = Some(handle);
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}

struct PeerState {
    tunn: Tunn,
    allowed: Vec<IpCidr>,
    endpoint: Option<SocketAddr>,
}

impl PeerState {
    fn matches(&self, ip: IpAddr) -> bool {
        self.allowed.iter().any(|c| c.contains(ip))
    }
}

#[derive(Clone)]
struct IpCidr {
    addr: IpAddr,
    prefix: u8,
}

impl IpCidr {
    fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(addr)) => ipv4_match(net, addr, self.prefix),
            (IpAddr::V6(net), IpAddr::V6(addr)) => ipv6_match(net, addr, self.prefix),
            _ => false,
        }
    }
}

fn ipv4_match(net: Ipv4Addr, addr: Ipv4Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask = if prefix >= 32 {
        u32::MAX
    } else {
        u32::MAX << (32 - prefix)
    };
    u32::from_be_bytes(net.octets()) & mask == u32::from_be_bytes(addr.octets()) & mask
}

fn ipv6_match(net: Ipv6Addr, addr: Ipv6Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let net_bits = net.octets();
    let addr_bits = addr.octets();
    let full_bytes = (prefix / 8) as usize;
    let rem_bits = prefix % 8;
    if net_bits[..full_bytes] != addr_bits[..full_bytes] {
        return false;
    }
    if rem_bits == 0 {
        return true;
    }
    let mask = 0xff << (8 - rem_bits);
    (net_bits[full_bytes] & mask) == (addr_bits[full_bytes] & mask)
}

struct WgConfig {
    private_key: StaticSecret,
    listen_port: u16,
    interface_name: String,
    address: String,
    mtu: u16,
    peers: Vec<WgPeer>,
}

struct WgPeer {
    public_key: PublicKey,
    allowed: Vec<IpCidr>,
    endpoint: Option<SocketAddr>,
    keepalive: u16,
}

fn create_tun(cfg: &WgConfig) -> Result<tun::AsyncDevice> {
    let mut tun_cfg = tun::Configuration::default();
    tun_cfg.tun_name(&cfg.interface_name).mtu(cfg.mtu).up();
    #[cfg(unix)]
    {
        tun_cfg.address(&cfg.address);
    }
    #[cfg(windows)]
    {
        let gw = cfg
            .address
            .split('/')
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(Ipv4Addr::new(10, 0, 0, 1));
        tun_cfg.destination(gw);
    }
    tun::create_as_async(&tun_cfg).map_err(|e| anyhow::anyhow!("wireguard tun: {e}"))
}

async fn handle_datagram(
    sock: &UdpSocket,
    tun: &Arc<Mutex<tun::AsyncDevice>>,
    tunn: &mut Tunn,
    src_ip: Option<IpAddr>,
    datagram: &[u8],
    reply_to: SocketAddr,
    out: &mut [u8],
) -> bool {
    match tunn.decapsulate(src_ip, datagram, out) {
        TunnResult::WriteToNetwork(packet) => {
            let _ = sock.send_to(packet, reply_to).await;
            drain_tunn(sock, tunn, reply_to, out).await;
            true
        },
        TunnResult::WriteToTunnelV4(pkt, _) | TunnResult::WriteToTunnelV6(pkt, _) => {
            let mut dev = tun.lock().await;
            let _ = dev.write_all(pkt).await;
            drain_tunn(sock, tunn, reply_to, out).await;
            true
        },
        TunnResult::Done => {
            drain_tunn(sock, tunn, reply_to, out).await;
            true
        },
        TunnResult::Err(_) => false,
    }
}

async fn encapsulate_to_peer(
    sock: &UdpSocket,
    tunn: &mut Tunn,
    endpoint: SocketAddr,
    packet: &[u8],
    out: &mut [u8],
) {
    if let TunnResult::WriteToNetwork(wg_pkt) = tunn.encapsulate(packet, out) {
        let _ = sock.send_to(wg_pkt, endpoint).await;
        drain_tunn(sock, tunn, endpoint, out).await;
    }
}

async fn drain_tunn(sock: &UdpSocket, tunn: &mut Tunn, endpoint: SocketAddr, out: &mut [u8]) {
    loop {
        match tunn.decapsulate(None, &[], out) {
            TunnResult::WriteToNetwork(packet) => {
                let _ = sock.send_to(packet, endpoint).await;
            },
            TunnResult::Done => break,
            TunnResult::Err(_) => break,
            _ => break,
        }
    }
    loop {
        match tunn.update_timers(out) {
            TunnResult::WriteToNetwork(packet) => {
                let _ = sock.send_to(packet, endpoint).await;
            },
            TunnResult::Done => break,
            _ => break,
        }
    }
}

fn parse_ip_dest(packet: &[u8]) -> Option<IpAddr> {
    if packet.is_empty() {
        return None;
    }
    let version = packet[0] >> 4;
    match version {
        4 if packet.len() >= 20 => {
            let dst = Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]);
            Some(IpAddr::V4(dst))
        },
        6 if packet.len() >= 40 => {
            let mut oct = [0u8; 16];
            oct.copy_from_slice(&packet[24..40]);
            Some(IpAddr::V6(Ipv6Addr::from(oct)))
        },
        _ => None,
    }
}

fn parse_config(raw: &Value) -> Result<WgConfig> {
    let private_key = decode_secret(
        raw.get("private_key")
            .and_then(|v| v.as_str())
            .context("private_key")?,
    )?;
    let listen_port = raw
        .get("listen_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(51820) as u16;
    let interface_name = raw
        .get("interface_name")
        .and_then(|v| v.as_str())
        .unwrap_or("wg0")
        .to_string();
    let address = raw
        .get("address")
        .and_then(|v| v.as_str())
        .or_else(|| {
            raw.get("addresses")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_str())
        })
        .unwrap_or("10.0.0.1/32")
        .to_string();
    let mtu = raw.get("mtu").and_then(|v| v.as_u64()).unwrap_or(1420) as u16;
    let mut peers = Vec::new();
    if let Some(list) = raw.get("peers").and_then(|v| v.as_array()) {
        for (i, p) in list.iter().enumerate() {
            let pk = decode_pubkey(
                p.get("public_key")
                    .and_then(|v| v.as_str())
                    .with_context(|| format!("peer[{i}] public_key"))?,
            )?;
            let allowed = parse_allowed_ips(p, i)?;
            let endpoint = parse_peer_endpoint(p);
            let keepalive = p
                .get("persistent_keepalive_interval")
                .and_then(|v| v.as_u64())
                .unwrap_or(25) as u16;
            peers.push(WgPeer {
                public_key: pk,
                allowed,
                endpoint,
                keepalive,
            });
        }
    }
    Ok(WgConfig {
        private_key,
        listen_port,
        interface_name,
        address,
        mtu,
        peers,
    })
}

fn parse_allowed_ips(peer: &Value, idx: usize) -> Result<Vec<IpCidr>> {
    let mut out = Vec::new();
    if let Some(list) = peer.get("allowed_ips").and_then(|v| v.as_array()) {
        for cidr in list {
            if let Some(s) = cidr.as_str() {
                out.push(parse_cidr(s)?);
            }
        }
    }
    if out.is_empty() {
        anyhow::bail!("peer[{idx}] allowed_ips required");
    }
    Ok(out)
}

fn parse_cidr(s: &str) -> Result<IpCidr> {
    let (ip_s, prefix_s) = s.split_once('/').map(|(a, b)| (a, b)).unwrap_or((s, "32"));
    let addr: IpAddr = ip_s.parse().context("cidr ip")?;
    let prefix: u8 = prefix_s
        .parse()
        .unwrap_or(if addr.is_ipv4() { 32 } else { 128 });
    Ok(IpCidr { addr, prefix })
}

fn parse_peer_endpoint(peer: &Value) -> Option<SocketAddr> {
    if let Some(ep) = peer.get("endpoint").and_then(|v| v.as_str()) {
        if let Ok(addr) = ep.parse::<SocketAddr>() {
            return Some(addr);
        }
        if let Ok(mut addrs) = std::net::ToSocketAddrs::to_socket_addrs(ep) {
            return addrs.next();
        }
    }
    let host = peer
        .get("address")
        .or_else(|| peer.get("server"))
        .and_then(|v| v.as_str())?;
    let port = peer
        .get("port")
        .or_else(|| peer.get("server_port"))
        .and_then(|v| v.as_u64())
        .unwrap_or(51820) as u16;
    let target = format!("{host}:{port}");
    std::net::ToSocketAddrs::to_socket_addrs(&target)
        .ok()
        .and_then(|mut a| a.next())
}

fn decode_secret(b64: &str) -> Result<StaticSecret> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim())?;
    anyhow::ensure!(bytes.len() == 32);
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("bad key len"))?;
    Ok(StaticSecret::from(arr))
}

fn decode_pubkey(b64: &str) -> Result<PublicKey> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim())?;
    anyhow::ensure!(bytes.len() == 32);
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("bad pubkey len"))?;
    Ok(PublicKey::from(arr))
}

/// Install OS routes for peer allowed_ips (netlink / IP Helper API).
pub async fn install_routes(raw: &Value) -> Result<()> {
    let raw = raw.clone();
    tokio::task::spawn_blocking(move || rsb_core::install_routes(&raw))
        .await
        .context("join route install")??;
    Ok(())
}
