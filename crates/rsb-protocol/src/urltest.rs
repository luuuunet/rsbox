//! URLTest probe helpers (shared by outbound + API).

use anyhow::{Context, Result};
use rsb_core::Outbound;
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn probe_latency(ob: &dyn Outbound, url: &str) -> Result<Duration> {
    let start = Instant::now();
    let (host, port, path) = parse_probe_url(url)?;
    let addr: SocketAddr = tokio::net::lookup_host(format!("{host}:{port}"))
        .await?
        .next()
        .with_context(|| format!("probe resolve {host}:{port}"))?;
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nUser-Agent: rsbox-urltest/1.0\r\n\r\n"
    );
    let mut stream = ob.dial_tcp(addr).await?;
    stream.write_all(req.as_bytes()).await?;
    let mut buf = [0u8; 512];
    let n = stream.read(&mut buf).await.unwrap_or(0);
    if n == 0 {
        anyhow::bail!("empty probe response");
    }
    let status_ok = buf.starts_with(b"HTTP/1.1 204")
        || buf.starts_with(b"HTTP/1.0 204")
        || buf.starts_with(b"HTTP/1.1 200");
    anyhow::ensure!(status_ok, "probe bad status");
    Ok(start.elapsed())
}

fn parse_probe_url(url: &str) -> Result<(String, u16, String)> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, "/generate_204"));
    let path = if path.is_empty() {
        "/".to_string()
    } else {
        format!("/{path}")
    };
    let (host, port) = if let Some((h, p)) = host_port.split_once(':') {
        (h.to_string(), p.parse().context("probe port")?)
    } else if url.starts_with("https://") {
        (host_port.to_string(), 443)
    } else {
        (host_port.to_string(), 80)
    };
    Ok((host, port, path))
}

#[doc(hidden)]
pub fn parse_probe_url_for_test(url: &str) -> Result<(String, u16, String)> {
    parse_probe_url(url)
}

pub async fn probe_all(
    outbounds: &[String],
    url: &str,
    shared: &rsb_core::SharedOutboundManager,
) -> Vec<(String, Option<u32>)> {
    let mut results = Vec::new();
    let mgr = match shared.get() {
        Ok(m) => m,
        Err(_) => return results,
    };
    for tag in outbounds {
        let delay = match mgr.get(tag) {
            Ok(ob) => probe_latency(ob, url)
                .await
                .ok()
                .map(|d| d.as_millis() as u32),
            Err(_) => None,
        };
        results.push((tag.clone(), delay));
    }
    results
}

pub fn pick_best(delays: &[(String, Option<u32>)]) -> Option<String> {
    delays
        .iter()
        .filter_map(|(tag, d)| d.map(|ms| (tag.clone(), ms)))
        .min_by_key(|(_, ms)| *ms)
        .map(|(tag, _)| tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_probe_urls() {
        let (h, p, path) = parse_probe_url("https://www.gstatic.com/generate_204").unwrap();
        assert_eq!(h, "www.gstatic.com");
        assert_eq!(p, 443);
        assert!(path.contains("generate_204"));
    }
}
