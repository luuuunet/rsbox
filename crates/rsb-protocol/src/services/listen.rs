//! Shared listen address parsing for services.

use anyhow::{Context, Result};
use serde_json::Value;
use std::net::SocketAddr;

pub fn parse_listen(raw: &Value) -> Result<SocketAddr> {
    let host = raw
        .get("listen")
        .and_then(|v| v.as_str())
        .unwrap_or("127.0.0.1");
    let port = raw
        .get("listen_port")
        .and_then(|v| v.as_u64())
        .unwrap_or(8080) as u16;
    format!("{host}:{port}")
        .parse()
        .with_context(|| format!("parse service listen {host}:{port}"))
}

pub fn parse_user_tokens(raw: &Value) -> Vec<(String, String)> {
    raw.get("users")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|u| {
                    let name = u.get("name")?.as_str()?.to_string();
                    let token = u.get("token")?.as_str()?.to_string();
                    Some((name, token))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn auth_token(headers: &http::HeaderMap, users: &[(String, String)]) -> Option<String> {
    if users.is_empty() {
        return Some(String::new());
    }
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim_start_matches("Bearer ").trim().to_string())
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
        .or_else(|| {
            headers
                .get("anthropic-auth-token")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        });
    bearer.filter(|t| users.iter().any(|(_, tok)| tok == t))
}
