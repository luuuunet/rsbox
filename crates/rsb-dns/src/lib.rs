mod fake_ip;
mod resolved_registry;

pub use fake_ip::FakeIpPool;
pub use resolved_registry::{register_resolved_service, resolved_dns, unregister_resolved_service};

use anyhow::{Context, Result};
use base64::Engine;
use rsb_config::{DnsOptions, DnsServer};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::lookup_host;
use tokio_rustls::TlsConnector;
use tracing::debug;

#[derive(Clone)]
pub struct DnsRouter {
    options: Option<DnsOptions>,
    servers: Arc<Vec<DnsServerEntry>>,
    rules: Arc<Vec<DnsRule>>,
    fakeip: Option<Arc<FakeIpPool>>,
    reverse_fake: Arc<Mutex<HashMap<IpAddr, String>>>,
}

#[derive(Clone)]
enum DnsTransport {
    Udp(SocketAddr),
    Tcp(SocketAddr),
    Tls { addr: SocketAddr, sni: String },
    Https(String),
    FakeIp,
    Resolved(String),
}

#[derive(Clone)]
struct DnsServerEntry {
    tag: String,
    transport: DnsTransport,
}

#[derive(Clone)]
struct DnsRule {
    domain_suffix: Vec<String>,
    domain_keyword: Vec<String>,
    server_tag: String,
}

impl DnsRouter {
    pub fn new(options: Option<DnsOptions>) -> Self {
        let options = options.clone().unwrap_or_default();
        let mut servers: Vec<DnsServerEntry> = options
            .servers
            .iter()
            .enumerate()
            .filter_map(|(i, s)| parse_server(s, i).ok())
            .collect();
        let fakeip = options.fakeip.as_ref().and_then(|v| {
            let enabled = v.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true);
            if !enabled {
                return None;
            }
            let inet4 = v
                .get("inet4_range")
                .and_then(|r| r.as_str())
                .unwrap_or("198.18.0.0/15");
            FakeIpPool::new(inet4).ok().map(Arc::new)
        });
        if fakeip.is_some() {
            servers.push(DnsServerEntry {
                tag: "fakeip".into(),
                transport: DnsTransport::FakeIp,
            });
        }
        let rules = options.rules.iter().filter_map(parse_rule).collect();
        Self {
            options: Some(options),
            servers: Arc::new(servers),
            rules: Arc::new(rules),
            fakeip,
            reverse_fake: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn fakeip(&self) -> Option<Arc<FakeIpPool>> {
        self.fakeip.clone()
    }

    pub fn reverse_lookup(&self, ip: IpAddr) -> Option<String> {
        self.reverse_fake.lock().ok()?.get(&ip).cloned()
    }

    pub async fn lookup(&self, host: &str) -> Result<Vec<IpAddr>> {
        debug!(host, "dns lookup");
        if host.parse::<IpAddr>().is_ok() {
            return Ok(vec![host.parse()?]);
        }
        let mut pinned = None;
        let mut current = self;
        loop {
            let server = current.pick_server(host);
            if let Some(entry) = server {
                let addrs = match &entry.transport {
                    DnsTransport::FakeIp => {
                        if let Some(pool) = &current.fakeip {
                            let ip = pool.allocate(host);
                            if let Ok(mut map) = current.reverse_fake.lock() {
                                map.insert(ip, host.to_string());
                            }
                            return Ok(vec![ip]);
                        }
                        Vec::new()
                    }
                    DnsTransport::Udp(addr) => query_udp(*addr, host).await.unwrap_or_default(),
                    DnsTransport::Tcp(addr) => query_tcp(*addr, host).await.unwrap_or_default(),
                    DnsTransport::Tls { addr, sni } => {
                        query_dot(*addr, sni, host).await.unwrap_or_default()
                    }
                    DnsTransport::Https(url) => query_doh(url, host).await.unwrap_or_default(),
                    DnsTransport::Resolved(tag) => {
                        if let Some(router) = resolved_dns(tag) {
                            pinned = Some(router);
                            current = pinned.as_ref().expect("pinned").as_ref();
                            continue;
                        }
                        Vec::new()
                    }
                };
                if !addrs.is_empty() {
                    return Ok(addrs);
                }
            }
            break;
        }
        let mut addrs = Vec::new();
        for resolved in lookup_host(format!("{host}:0")).await? {
            addrs.push(resolved.ip());
        }
        Ok(addrs)
    }

    pub async fn exchange(&self, query: &[u8]) -> Result<Vec<u8>> {
        self.exchange_inner(query, false).await
    }

    pub async fn exchange_upstream(&self, query: &[u8]) -> Result<Vec<u8>> {
        self.exchange_inner(query, true).await
    }

    async fn exchange_inner(&self, query: &[u8], skip_resolved: bool) -> Result<Vec<u8>> {
        let mut pinned = None;
        let mut current = self;
        let mut skip = skip_resolved;
        'router: loop {
            for server in current.servers.iter() {
                if skip && matches!(server.transport, DnsTransport::Resolved(_)) {
                    continue;
                }
                let result = match &server.transport {
                    DnsTransport::Udp(addr) => query_udp_raw(*addr, query).await,
                    DnsTransport::Tcp(addr) => query_tcp_raw(*addr, query).await,
                    DnsTransport::Tls { addr, sni } => query_dot_raw(*addr, sni, query).await,
                    DnsTransport::Https(url) => query_doh_raw(url, query).await,
                    DnsTransport::FakeIp => {
                        anyhow::bail!("raw exchange not supported for fakeip server")
                    }
                    DnsTransport::Resolved(tag) => {
                        let router = resolved_dns(tag)
                            .with_context(|| format!("resolved service `{tag}` not running"))?;
                        pinned = Some(router);
                        current = pinned.as_ref().expect("pinned").as_ref();
                        skip = true;
                        continue 'router;
                    }
                };
                if result.is_ok() {
                    return result;
                }
            }
            anyhow::bail!("no upstream dns server available");
        }
    }

    fn pick_server(&self, host: &str) -> Option<&DnsServerEntry> {
        for rule in self.rules.iter() {
            if rule
                .domain_suffix
                .iter()
                .any(|s| host.ends_with(s.trim_start_matches('*').trim_start_matches('.')))
            {
                if let Some(entry) = self.servers.iter().find(|s| s.tag == rule.server_tag) {
                    return Some(entry);
                }
            }
            if rule.domain_keyword.iter().any(|k| host.contains(k)) {
                if let Some(entry) = self.servers.iter().find(|s| s.tag == rule.server_tag) {
                    return Some(entry);
                }
            }
        }
        self.servers.first()
    }

    pub fn options(&self) -> Option<&DnsOptions> {
        self.options.as_ref()
    }
}

fn parse_server(raw: &DnsServer, index: usize) -> Result<DnsServerEntry> {
    let tag = raw.tag.clone().unwrap_or_else(|| index.to_string());
    if raw.raw.get("type").and_then(|v| v.as_str()) == Some("resolved") {
        let service = raw
            .raw
            .get("service")
            .and_then(|v| v.as_str())
            .unwrap_or("resolved")
            .to_string();
        return Ok(DnsServerEntry {
            tag,
            transport: DnsTransport::Resolved(service),
        });
    }
    if raw.raw.get("type").and_then(|v| v.as_str()) == Some("local") {
        return Ok(DnsServerEntry {
            tag,
            transport: DnsTransport::Resolved("local".into()),
        });
    }
    let address = raw
        .address
        .as_deref()
        .or_else(|| raw.raw.get("address").and_then(|v| v.as_str()))
        .context("dns server address")?;
    if address == "fakeip" || address == "fake-ip" {
        return Ok(DnsServerEntry {
            tag,
            transport: DnsTransport::FakeIp,
        });
    }
    let transport = parse_dns_transport(address)?;
    Ok(DnsServerEntry { tag, transport })
}

fn parse_dns_transport(address: &str) -> Result<DnsTransport> {
    if address.starts_with("https://") {
        return Ok(DnsTransport::Https(address.to_string()));
    }
    if address.starts_with("tls://") {
        let host = address.trim_start_matches("tls://");
        let (server, sni) = split_host_sni(host);
        let addr = resolve_server_addr(&server, 853)?;
        return Ok(DnsTransport::Tls { addr, sni });
    }
    let host = address
        .trim_start_matches("udp://")
        .trim_start_matches("tcp://")
        .trim_start_matches("local://");
    if host == "local" {
        return Ok(DnsTransport::Udp(SocketAddr::from(([8, 8, 8, 8], 53))));
    }
    if address.starts_with("tcp://") {
        let addr = resolve_server_addr(host, 53)?;
        return Ok(DnsTransport::Tcp(addr));
    }
    if let Ok(addr) = host.parse::<SocketAddr>() {
        return Ok(DnsTransport::Udp(addr));
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(DnsTransport::Udp(SocketAddr::new(ip, 53)));
    }
    anyhow::bail!("unsupported dns server address: {address}")
}

fn split_host_sni(host: &str) -> (String, String) {
    if let Some((h, sni)) = host.split_once('#') {
        (h.to_string(), sni.to_string())
    } else {
        (host.to_string(), host.to_string())
    }
}

fn resolve_server_addr(host: &str, default_port: u16) -> Result<SocketAddr> {
    if let Ok(addr) = host.parse::<SocketAddr>() {
        return Ok(addr);
    }
    let target = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:{default_port}")
    };
    std::net::ToSocketAddrs::to_socket_addrs(&target)?
        .next()
        .context("resolve dns server")
}

fn parse_rule(raw: &serde_json::Value) -> Option<DnsRule> {
    let server_tag = raw
        .get("server")
        .and_then(|v| v.as_str())
        .map(str::to_string)?;
    let domain_suffix = raw
        .get("domain_suffix")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let domain_keyword = raw
        .get("domain_keyword")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Some(DnsRule {
        domain_suffix,
        domain_keyword,
        server_tag,
    })
}

async fn query_udp(server: SocketAddr, host: &str) -> Result<Vec<IpAddr>> {
    let query = build_query(host, false);
    let bytes = query_udp_raw(server, &query).await?;
    parse_response(&bytes)
}

async fn query_udp_raw(server: SocketAddr, query: &[u8]) -> Result<Vec<u8>> {
    let socket = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    socket.send_to(query, server).await?;
    let mut buf = vec![0u8; 4096];
    let (n, _) = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        socket.recv_from(&mut buf),
    )
    .await
    .context("dns udp timeout")??;
    Ok(buf[..n].to_vec())
}

async fn query_tcp(server: SocketAddr, host: &str) -> Result<Vec<IpAddr>> {
    let query = build_query(host, false);
    let bytes = query_tcp_raw(server, &query).await?;
    parse_response(&bytes)
}

async fn query_tcp_raw(server: SocketAddr, query: &[u8]) -> Result<Vec<u8>> {
    let mut stream = tokio::net::TcpStream::connect(server).await?;
    let len = (query.len() as u16).to_be_bytes();
    stream.write_all(&len).await?;
    stream.write_all(query).await?;
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let resp_len = u16::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; resp_len];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn query_dot(server: SocketAddr, sni: &str, host: &str) -> Result<Vec<IpAddr>> {
    let query = build_query(host, false);
    let bytes = query_dot_raw(server, sni, &query).await?;
    parse_response(&bytes)
}

async fn query_dot_raw(server: SocketAddr, sni: &str, query: &[u8]) -> Result<Vec<u8>> {
    use rustls::pki_types::ServerName;
    use rustls::{ClientConfig, RootCertStore};
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let tcp = tokio::net::TcpStream::connect(server).await?;
    let name = ServerName::try_from(sni.to_string()).map_err(|_| anyhow::anyhow!("bad sni"))?;
    let mut tls = TlsConnector::from(Arc::new(cfg)).connect(name, tcp).await?;
    let len = (query.len() as u16).to_be_bytes();
    tls.write_all(&len).await?;
    tls.write_all(query).await?;
    let mut len_buf = [0u8; 2];
    tls.read_exact(&mut len_buf).await?;
    let resp_len = u16::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; resp_len];
    tls.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn query_doh(url: &str, host: &str) -> Result<Vec<IpAddr>> {
    let query = build_query(host, false);
    let bytes = query_doh_raw(url, &query).await?;
    parse_response(&bytes)
}

async fn query_doh_raw(url: &str, query: &[u8]) -> Result<Vec<u8>> {
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(query);
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{url}?dns={encoded}"))
        .header("Accept", "application/dns-message")
        .send()
        .await
        .context("doh request")?;
    Ok(response.bytes().await?.to_vec())
}

fn build_query(host: &str, ipv6: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(64);
    out.extend_from_slice(&[0x12, 0x34, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    for label in host.split('.') {
        if label.is_empty() {
            continue;
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&if ipv6 { [0, 28] } else { [0, 1] });
    out.extend_from_slice(&[0, 1]);
    out
}

pub fn parse_response(buf: &[u8]) -> Result<Vec<IpAddr>> {
    if buf.len() < 12 {
        return Ok(Vec::new());
    }
    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    let mut offset = 12;
    for _ in 0..qd {
        offset = skip_name(buf, offset)?;
        offset += 4;
    }
    let mut addrs = Vec::new();
    for _ in 0..an {
        offset = skip_name(buf, offset)?;
        if offset + 10 > buf.len() {
            break;
        }
        let rtype = u16::from_be_bytes([buf[offset], buf[offset + 1]]);
        let rdlen = u16::from_be_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
        offset += 10;
        if offset + rdlen > buf.len() {
            break;
        }
        match rtype {
            1 if rdlen == 4 => {
                addrs.push(IpAddr::V4(Ipv4Addr::new(
                    buf[offset],
                    buf[offset + 1],
                    buf[offset + 2],
                    buf[offset + 3],
                )));
            }
            28 if rdlen == 16 => {
                let mut octets = [0u8; 16];
                octets.copy_from_slice(&buf[offset..offset + 16]);
                addrs.push(IpAddr::V6(Ipv6Addr::from(octets)));
            }
            _ => {}
        }
        offset += rdlen;
    }
    Ok(addrs)
}

pub fn build_response(query: &[u8], addrs: &[IpAddr]) -> Result<Vec<u8>> {
    if query.len() < 12 {
        anyhow::bail!("bad query");
    }
    let mut out = query[..12].to_vec();
    out[2] = 0x81;
    out[3] = 0x80;
    out[6] = 0;
    out[7] = addrs.len() as u8;
    for ip in addrs {
        out.extend_from_slice(&[0xc0, 0x0c]);
        match ip {
            IpAddr::V4(v4) => {
                out.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
                out.extend_from_slice(&v4.octets());
            }
            IpAddr::V6(v6) => {
                out.extend_from_slice(&[0, 28, 0, 1, 0, 0, 0, 60, 0, 16]);
                out.extend_from_slice(&v6.octets());
            }
        }
    }
    Ok(out)
}

fn skip_name(buf: &[u8], mut offset: usize) -> Result<usize> {
    loop {
        if offset >= buf.len() {
            anyhow::bail!("truncated dns name");
        }
        let len = buf[offset];
        if len & 0xC0 == 0xC0 {
            return Ok(offset + 2);
        }
        if len == 0 {
            return Ok(offset + 1);
        }
        offset += 1 + len as usize;
    }
}
