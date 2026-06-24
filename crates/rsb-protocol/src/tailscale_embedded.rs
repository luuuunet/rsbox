//! Embedded Tailscale data plane (WireGuard + local state, no `tailscale` CLI).

use anyhow::{Context, Result};
use base64::Engine;
use rand::RngCore;
use serde_json::{json, Value};
use std::path::Path;
use x25519_dalek::{PublicKey, StaticSecret};

pub async fn resolve_wireguard_config(raw: &Value) -> Result<Value> {
    if let Some(path) = raw.get("state_file").and_then(|v| v.as_str()) {
        return load_state_file(path);
    }
    if raw.get("private_key").is_some() {
        return normalize_wireguard(raw);
    }
    if let Some(dir) = raw.get("state_directory").and_then(|v| v.as_str()) {
        let path = Path::new(dir).join("rsbox-tailscale.json");
        if path.exists() {
            return load_state_file(path.to_string_lossy().as_ref());
        }
    }
    if raw.get("auth_key").is_some() {
        return bootstrap_from_auth_key(raw).await;
    }
    anyhow::bail!("tailscale: set state_file, private_key, state_directory, or auth_key")
}

fn load_state_file(path: &str) -> Result<Value> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("read tailscale state `{path}`"))?;
    let json: Value = serde_json::from_str(&text)?;
    normalize_wireguard(&json)
}

fn normalize_wireguard(raw: &Value) -> Result<Value> {
    let mut out = raw.clone();
    if out.get("listen_port").is_none() {
        out["listen_port"] = json!(41641);
    }
    if out.get("interface_name").is_none() {
        out["interface_name"] = json!("tailscale0");
    }
    if out.get("addresses").is_none() {
        out["addresses"] = json!(["100.64.0.1/32"]);
    }
    Ok(out)
}

async fn bootstrap_from_auth_key(raw: &Value) -> Result<Value> {
    let auth_key = raw
        .get("auth_key")
        .and_then(|v| v.as_str())
        .context("auth_key")?;
    let mut secret_bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut secret_bytes);
    let secret = StaticSecret::from(secret_bytes);
    let public = PublicKey::from(&secret);
    let private_key = base64::engine::general_purpose::STANDARD.encode(secret.to_bytes());
    let public_key = base64::engine::general_purpose::STANDARD.encode(public.as_bytes());

    let mut wg = json!({
        "private_key": private_key,
        "listen_port": raw.get("listen_port").and_then(|v| v.as_u64()).unwrap_or(41641),
        "interface_name": raw.get("interface_name").and_then(|v| v.as_str()).unwrap_or("tailscale0"),
        "addresses": raw.get("addresses").cloned().unwrap_or(json!(["100.64.0.1/32"])),
        "peers": raw.get("peers").cloned().unwrap_or(json!([])),
        "rsbox_public_key": public_key,
        "rsbox_auth_key_hint": &auth_key[..auth_key.len().min(8)],
    });

    if let Some(control_url) = raw.get("control_url").and_then(|v| v.as_str()) {
        if let Ok(wg_config) = crate::tailscale_control::register_peers(
            Some(control_url),
            auth_key,
            raw.get("hostname").and_then(|v| v.as_str()),
        )
        .await
        {
            wg = wg_config;
            tracing::info!(%control_url, "tailscale: Noise control registration succeeded");
        } else if let Ok(peers) = headscale_register(control_url, auth_key, &public_key).await {
            wg["peers"] = peers;
            tracing::info!(%control_url, "tailscale embedded: registered with control server");
        } else {
            tracing::warn!(
                %control_url,
                "tailscale embedded: control register failed; using local peers only"
            );
        }
    } else {
        tracing::info!(
            public_key = %public_key,
            "tailscale embedded: generated wireguard keys (set control_url for headscale register)"
        );
    }

    if let Some(dir) = raw.get("state_directory").and_then(|v| v.as_str()) {
        std::fs::create_dir_all(dir).ok();
        let path = Path::new(dir).join("rsbox-tailscale.json");
        std::fs::write(&path, serde_json::to_string_pretty(&wg)?)
            .with_context(|| format!("write tailscale state `{}`", path.display()))?;
        tracing::info!(path = %path.display(), "tailscale state saved");
    }

    normalize_wireguard(&wg)
}

async fn headscale_register(control_url: &str, auth_key: &str, node_key: &str) -> Result<Value> {
    let base = control_url.trim_end_matches('/');
    let urls = [
        format!("{base}/machine/register"),
        format!("{base}/api/v1/register"),
    ];
    let client = reqwest::Client::new();
    let body = json!({
        "AuthKey": auth_key,
        "NodeKey": node_key,
        "Hostinfo": { "OS": std::env::consts::OS },
    });
    for url in urls {
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&body)?)
            .send()
            .await?;
        if !resp.status().is_success() {
            continue;
        }
        let text = resp.text().await.unwrap_or_default();
        if let Ok(json) = serde_json::from_str::<Value>(&text) {
            if let Some(peers) = json.get("peers").cloned() {
                return Ok(peers);
            }
            if let Some(peer) = parse_headscale_node(&json) {
                return Ok(json!([peer]));
            }
        }
    }
    anyhow::bail!("headscale register failed")
}

fn parse_headscale_node(json: &Value) -> Option<Value> {
    let pubkey = json
        .get("WireGuardPublicKey")
        .or_else(|| json.get("node_key"))
        .or_else(|| json.get("public_key"))
        .and_then(|v| v.as_str())?;
    let endpoint = json
        .get("Endpoint")
        .or_else(|| json.get("endpoint"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let allowed = json
        .get("AllowedIPs")
        .or_else(|| json.get("allowed_ips"))
        .cloned()
        .unwrap_or(json!(["0.0.0.0/0", "::/0"]));
    Some(json!({
        "public_key": pubkey,
        "endpoint": endpoint,
        "allowed_ips": allowed,
        "persistent_keepalive_interval": 25,
    }))
}
