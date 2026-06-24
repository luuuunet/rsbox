//! CCM / OCM credential multiplexing HTTP proxy.

use super::listen::{auth_token, parse_listen, parse_user_tokens};
use anyhow::{Context, Result};
use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    response::Response,
    routing::{any, get},
    Router,
};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Clone)]
struct MultiplexerState {
    kind: String,
    auto_refresh: bool,
    oauth_client_id: String,
    users: Arc<Vec<(String, String)>>,
    credential_path: Option<String>,
    upstream_base: String,
    auth_header: &'static str,
    oauth_field: &'static str,
}

pub struct MultiplexerService {
    tag: String,
    kind: String,
    listen: SocketAddr,
    state: MultiplexerState,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl MultiplexerService {
    pub fn ccm(tag: String, raw: Value) -> Result<Self> {
        Self::new(
            tag,
            "ccm",
            raw,
            "https://api.anthropic.com",
            "x-api-key",
            "accessToken",
        )
    }

    pub fn ocm(tag: String, raw: Value) -> Result<Self> {
        Self::new(
            tag,
            "ocm",
            raw,
            "https://api.openai.com",
            "authorization",
            "access_token",
        )
    }

    fn new(
        tag: String,
        kind: &'static str,
        raw: Value,
        upstream_base: &str,
        auth_header: &'static str,
        oauth_field: &'static str,
    ) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Ok(Self {
            tag,
            kind: kind.to_string(),
            listen: parse_listen(&raw)?,
            state: MultiplexerState {
                kind: kind.to_string(),
                auto_refresh: raw
                    .get("auto_refresh")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                oauth_client_id: raw
                    .get("oauth_client_id")
                    .or_else(|| raw.get("client_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(if kind == "ocm" {
                        "app_rsbox"
                    } else {
                        "claude-code"
                    })
                    .to_string(),
                users: Arc::new(parse_user_tokens(&raw)),
                credential_path: raw
                    .get("credential_path")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                upstream_base: upstream_base.to_string(),
                auth_header,
                oauth_field,
            },
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }

    pub async fn start(&self) -> Result<()> {
        let app = Router::new()
            .route("/health", get(|| async { "ok" }))
            .route("/*path", any(proxy_request))
            .with_state(self.state.clone());
        let listener = tokio::net::TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, kind = %self.kind, %self.listen, "multiplexer service listening");
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            let server = axum::serve(listener, app);
            tokio::select! {
                _ = shutdown.changed() => {}
                r = server => { let _ = r; }
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

async fn proxy_request(
    State(state): State<MultiplexerState>,
    req: Request<Body>,
) -> Result<Response, StatusCode> {
    if !state.users.is_empty() && auth_token(req.headers(), &state.users).is_none() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let token = load_oauth_token(&state)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let path = req.uri().path();
    let query = req
        .uri()
        .query()
        .map(|q| format!("?{q}"))
        .unwrap_or_default();
    let url = format!("{}{}{}", state.upstream_base, path, query);
    let (parts, body) = req.into_parts();
    let body_bytes = axum::body::to_bytes(body, 16 * 1024 * 1024)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let client = reqwest::Client::new();
    let mut upstream = client.request(parts.method, &url).body(body_bytes);
    for (k, v) in parts.headers.iter() {
        if k == "host" || k == "authorization" || k == "x-api-key" || k == "anthropic-auth-token" {
            continue;
        }
        upstream = upstream.header(k, v);
    }
    upstream = match state.auth_header {
        "authorization" => upstream.header("Authorization", format!("Bearer {token}")),
        _ => upstream.header(state.auth_header, &token),
    };
    let resp = upstream.send().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut builder = Response::builder().status(status);
    for (k, v) in resp.headers().iter() {
        builder = builder.header(k, v);
    }
    let bytes = resp.bytes().await.map_err(|_| StatusCode::BAD_GATEWAY)?;
    builder
        .body(Body::from(bytes))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn load_oauth_token(state: &MultiplexerState) -> Result<String> {
    let path = state
        .credential_path
        .clone()
        .or_else(default_credential_path);
    let Some(path) = path else {
        anyhow::bail!("credential_path not configured");
    };
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("read credential `{path}`"))?;
    let mut json: Value = serde_json::from_str(&text)?;
    if state.auto_refresh && token_expired(&json) {
        json = refresh_oauth_token(state, &json, &path).await?;
    }
    extract_oauth_token(state, &json)
}

fn extract_oauth_token(state: &MultiplexerState, json: &Value) -> Result<String> {
    if let Some(t) = json.get(state.oauth_field).and_then(|v| v.as_str()) {
        return Ok(t.to_string());
    }
    if let Some(t) = json
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|v| v.as_str())
    {
        return Ok(t.to_string());
    }
    if let Some(t) = json
        .pointer("/tokens/access_token")
        .and_then(|v| v.as_str())
    {
        return Ok(t.to_string());
    }
    anyhow::bail!("oauth token not found in credential file")
}

fn token_expired(json: &Value) -> bool {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if let Some(exp) = json.get("expires_at").and_then(|v| v.as_u64()) {
        return now.saturating_add(60) >= exp;
    }
    if let Some(exp) = json
        .pointer("/claudeAiOauth/expiresAt")
        .and_then(|v| v.as_u64())
    {
        let now_ms = now.saturating_mul(1000);
        return now_ms.saturating_add(60_000) >= exp;
    }
    if let Some(exp) = json.pointer("/tokens/expires_at").and_then(|v| v.as_u64()) {
        return now.saturating_add(60) >= exp;
    }
    false
}

async fn refresh_oauth_token(state: &MultiplexerState, json: &Value, path: &str) -> Result<Value> {
    let refresh = json
        .get("refresh_token")
        .or_else(|| json.pointer("/tokens/refresh_token"))
        .and_then(|v| v.as_str());
    let Some(refresh) = refresh else {
        anyhow::bail!("refresh_token missing");
    };
    let (token_url, client_id) = if state.kind == "ocm" {
        (
            "https://auth.openai.com/oauth/token",
            state.oauth_client_id.as_str(),
        )
    } else {
        (
            "https://console.anthropic.com/v1/oauth/token",
            state.oauth_client_id.as_str(),
        )
    };
    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh),
            ("client_id", client_id),
        ])
        .send()
        .await
        .context("oauth refresh request")?;
    if !resp.status().is_success() {
        anyhow::bail!("oauth refresh failed: {}", resp.status());
    }
    let body = resp.text().await.context("read oauth refresh body")?;
    let refreshed: Value = serde_json::from_str(&body).context("parse oauth refresh")?;
    let mut merged = json.clone();
    if let Some(obj) = merged.as_object_mut() {
        if let Some(token) = refreshed.get("access_token").and_then(|v| v.as_str()) {
            obj.insert(
                state.oauth_field.to_string(),
                Value::String(token.to_string()),
            );
            obj.insert("access_token".into(), Value::String(token.to_string()));
        }
        if let Some(expires) = refreshed.get("expires_in").and_then(|v| v.as_u64()) {
            let exp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_add(expires);
            obj.insert("expires_at".into(), Value::Number(exp.into()));
        }
        if let Some(rt) = refreshed.get("refresh_token").and_then(|v| v.as_str()) {
            obj.insert("refresh_token".into(), Value::String(rt.to_string()));
        }
    }
    std::fs::write(path, serde_json::to_string_pretty(&merged)?)
        .with_context(|| format!("write credential `{path}`"))?;
    tracing::info!(kind = %state.kind, "oauth token refreshed");
    Ok(merged)
}

fn default_credential_path() -> Option<String> {
    if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
        return Some(format!("{home}/.claude/.credentials.json"));
    }
    None
}
