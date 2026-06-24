//! Tailscale / Headscale control plane registration (Noise_IK + HTTP fallback).

use anyhow::{Context, Result};
use base64::Engine;
use rand::RngCore;
use serde_json::{json, Value};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::tailscale_noise::{client_handshake, DEFAULT_PROTOCOL_VERSION};

const TAILSCALE_DEFAULT: &str = "https://controlplane.tailscale.com";

pub struct ControlRegistration {
    pub node_private: StaticSecret,
    pub node_public: PublicKey,
    pub machine_private: StaticSecret,
    pub machine_public: PublicKey,
}

impl ControlRegistration {
    pub fn generate() -> Self {
        let mut nb = [0u8; 32];
        let mut mb = [0u8; 32];
        rand::rng().fill_bytes(&mut nb);
        rand::rng().fill_bytes(&mut mb);
        let node_private = StaticSecret::from(nb);
        let machine_private = StaticSecret::from(mb);
        Self {
            node_public: PublicKey::from(&node_private),
            machine_public: PublicKey::from(&machine_private),
            node_private,
            machine_private,
        }
    }
}

pub async fn register_peers(
    control_url: Option<&str>,
    auth_key: &str,
    hostname: Option<&str>,
) -> Result<Value> {
    let base = control_url.unwrap_or(TAILSCALE_DEFAULT);
    let reg = ControlRegistration::generate();
    let (server_key, protocol_version) = fetch_control_key(base).await?;
    let body = json!({
        "Version": 35,
        "AuthKey": auth_key,
        "NodeKey": base64::engine::general_purpose::STANDARD.encode(reg.node_public.as_bytes()),
        "MachineKey": base64::engine::general_purpose::STANDARD.encode(reg.machine_public.as_bytes()),
        "Hostinfo": {
            "OS": std::env::consts::OS,
            "Hostname": hostname.unwrap_or("rsbox"),
        },
        "Ephemeral": false,
    });
    if let Ok(resp) = noise_register(base, &server_key, protocol_version, &reg, &body).await {
        let mut wg = parse_register_response(&resp, &reg)?;
        if let Ok(map_peers) = fetch_map_peers(base, &reg).await {
            let wg_peers = map_to_wg_peers(&map_peers);
            if !wg_peers.as_array().map(|a| a.is_empty()).unwrap_or(true) {
                wg["peers"] = wg_peers;
            }
        }
        return Ok(wg);
    }
    http_register(base, &body)
        .await
        .or_else(|_| parse_local_wg(&reg, auth_key))
}

async fn fetch_control_key(base: &str) -> Result<([u8; 32], u16)> {
    let url = format!("{}/key?v=115", base.trim_end_matches('/'));
    let text = reqwest::get(&url)
        .await
        .context("fetch control key")?
        .text()
        .await?;
    let json: Value = serde_json::from_str(&text)?;
    let key_b64 = json
        .get("PublicKey")
        .and_then(|v| v.as_str())
        .context("control PublicKey")?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(key_b64.trim())?;
    anyhow::ensure!(bytes.len() == 32);
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&bytes);
    let version = json
        .get("Version")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_PROTOCOL_VERSION as u64) as u16;
    Ok((pk, version))
}

async fn noise_register(
    base: &str,
    server_key: &[u8; 32],
    protocol_version: u16,
    reg: &ControlRegistration,
    body: &Value,
) -> Result<Value> {
    let host = base
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .context("control url")?;
    let mut tls = crate::transport::tls_connect_plain(host, 443, None, Some(host)).await?;
    use tokio::io::AsyncWriteExt;

    let mut conn =
        client_handshake(&mut tls, &reg.machine_private, server_key, protocol_version).await?;

    let req_body = serde_json::to_vec(body)?;
    conn.write_all(&mut tls, &req_body).await?;

    let payload = conn.read_record(&mut tls).await?;
    parse_noise_register_payload(&payload)
}

fn parse_noise_register_payload(payload: &[u8]) -> Result<Value> {
    if payload.len() >= 4 {
        let len = u32::from_be_bytes(payload[..4].try_into()?) as usize;
        if len > 0 && payload.len() >= 4 + len {
            return Ok(serde_json::from_slice(&payload[4..4 + len])?);
        }
    }
    serde_json::from_slice(payload).context("parse noise register response")
}

async fn fetch_map_peers(base: &str, reg: &ControlRegistration) -> Result<Value> {
    let url = format!("{}/machine/map", base.trim_end_matches('/'));
    let body = json!({
        "Version": 35,
        "NodeKey": base64::engine::general_purpose::STANDARD.encode(reg.node_public.as_bytes()),
        "MachineKey": base64::engine::general_purpose::STANDARD.encode(reg.machine_public.as_bytes()),
    });
    let resp = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(&body)?)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("map http {}", resp.status());
    }
    let text = resp.text().await?;
    Ok(serde_json::from_str(&text)?)
}

async fn http_register(base: &str, body: &Value) -> Result<Value> {
    let url = format!("{}/machine/register", base.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&url)
        .header("Content-Type", "application/json")
        .body(serde_json::to_string(body)?)
        .send()
        .await?;
    if !resp.status().is_success() {
        anyhow::bail!("register http {}", resp.status());
    }
    let text = resp.text().await?;
    Ok(serde_json::from_str(&text)?)
}

fn parse_local_wg(reg: &ControlRegistration, auth_key: &str) -> Result<Value> {
    Ok(json!({
        "private_key": base64::engine::general_purpose::STANDARD.encode(reg.node_private.to_bytes()),
        "listen_port": 41641,
        "interface_name": "tailscale0",
        "addresses": ["100.64.0.2/32"],
        "peers": [],
        "rsbox_auth_key_hint": &auth_key[..auth_key.len().min(8)],
    }))
}

fn parse_register_response(resp: &Value, reg: &ControlRegistration) -> Result<Value> {
    let peers = resp
        .get("Peers")
        .cloned()
        .filter(|p| p.as_array().map(|a| !a.is_empty()).unwrap_or(false))
        .unwrap_or_else(|| json!([]));
    Ok(json!({
        "private_key": base64::engine::general_purpose::STANDARD.encode(reg.node_private.to_bytes()),
        "listen_port": 41641,
        "interface_name": "tailscale0",
        "addresses": resp.get("Addresses").cloned().unwrap_or(json!(["100.64.0.2/32"])),
        "peers": peers,
        "control_response": resp,
    }))
}

/// Convert Tailscale /machine/map peer entries to WireGuard peer JSON.
fn map_to_wg_peers(map: &Value) -> Value {
    let peers = map
        .get("Peers")
        .or_else(|| map.get("peers"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let converted: Vec<Value> = peers.iter().filter_map(convert_tailscale_peer).collect();
    json!(converted)
}

fn convert_tailscale_peer(p: &Value) -> Option<Value> {
    let node_key_b64 = p
        .get("NodeKey")
        .or_else(|| p.get("node_key"))
        .and_then(|v| v.as_str())?;
    let endpoint = p
        .get("Endpoints")
        .or_else(|| p.get("endpoints"))
        .and_then(|v| v.as_array())
        .and_then(|a| a.first())
        .and_then(|v| v.as_str())
        .or_else(|| {
            p.get("Hostinfo")
                .and_then(|h| h.get("HostName"))
                .and_then(|v| v.as_str())
        });
    let allowed_ips: Vec<String> = p
        .get("AllowedIPs")
        .or_else(|| p.get("allowed_ips"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_else(|| {
            p.get("Addresses")
                .or_else(|| p.get("addresses"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        });
    let mut peer = json!({
        "public_key": node_key_b64,
        "allowed_ips": allowed_ips,
        "persistent_keepalive": 25,
    });
    if let Some(ep) = endpoint {
        peer["endpoint"] = json!(ep);
    }
    Some(peer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_peer_conversion() {
        let map = json!({
            "Peers": [{
                "NodeKey": "abc123=",
                "AllowedIPs": ["100.64.0.3/32"],
                "Endpoints": ["1.2.3.4:41641"]
            }]
        });
        let peers = map_to_wg_peers(&map);
        assert_eq!(peers.as_array().unwrap().len(), 1);
        assert_eq!(peers[0]["public_key"], "abc123=");
    }

    #[test]
    fn parse_length_prefixed_register_response() {
        let inner = br#"{"Addresses":["100.64.0.2/32"]}"#;
        let mut payload = (inner.len() as u32).to_be_bytes().to_vec();
        payload.extend_from_slice(inner);
        let v = parse_noise_register_payload(&payload).unwrap();
        assert!(v.get("Addresses").is_some());
    }
}
