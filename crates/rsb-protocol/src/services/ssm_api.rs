//! Shadowsocks Server Manager API (managed inbound CRUD).

use super::context::ServiceContext;
use super::listen::{auth_token, parse_listen, parse_user_tokens};
use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::get,
    Json, Router,
};
use rsb_constant::TYPE_SHADOWSOCKS;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

pub struct SsmApiService {
    tag: String,
    listen: SocketAddr,
    users: Vec<(String, String)>,
    ctx: ServiceContext,
    servers: Arc<Mutex<Value>>,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[derive(Clone)]
struct SsmState {
    ctx: ServiceContext,
    users: Arc<Vec<(String, String)>>,
    servers: Arc<Mutex<Value>>,
}

impl SsmApiService {
    pub fn new(tag: String, raw: Value, ctx: ServiceContext) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let servers = raw.get("servers").cloned().unwrap_or(json!({}));
        Ok(Self {
            tag,
            listen: parse_listen(&raw)?,
            users: parse_user_tokens(&raw),
            ctx,
            servers: Arc::new(Mutex::new(servers)),
            shutdown,
            handle: tokio::sync::Mutex::new(None),
        })
    }

    pub async fn start(&self) -> Result<()> {
        let state = SsmState {
            ctx: self.ctx.clone(),
            users: Arc::new(self.users.clone()),
            servers: self.servers.clone(),
        };
        let app = Router::new()
            .route("/", get(list_servers))
            .route("/servers", get(list_servers).post(create_server))
            .route(
                "/servers/{path}",
                get(get_server).put(update_server).delete(delete_server),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, "ssm-api service listening");
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

fn authorized(headers: &HeaderMap, users: &[(String, String)]) -> bool {
    users.is_empty() || auth_token(headers, users).is_some()
}

fn managed_inbounds(state: &SsmState) -> Vec<Value> {
    let opts = state.ctx.options_snapshot();
    opts.inbounds
        .iter()
        .enumerate()
        .filter(|(_, ib)| {
            ib.kind == TYPE_SHADOWSOCKS
                && ib
                    .raw
                    .get("managed")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
        })
        .map(|(i, ib)| {
            let tag = opts.inbound_tag(ib, i);
            serde_json::json!({
                "tag": tag,
                "type": ib.kind,
                "listen": ib.raw.get("listen"),
                "listen_port": ib.raw.get("listen_port"),
                "method": ib.raw.get("method"),
            })
        })
        .collect()
}

async fn list_servers(
    State(state): State<SsmState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let managed = managed_inbounds(&state);
    let servers = state.servers.lock().unwrap().clone();
    Ok(Json(json!({
        "servers": servers,
        "managed_inbounds": managed,
    })))
}

async fn get_server(
    State(state): State<SsmState>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let servers = state.servers.lock().unwrap();
    if let Some(v) = servers.get(&path) {
        return Ok(Json(v.clone()));
    }
    let managed = managed_inbounds(&state);
    if let Some(found) = managed
        .iter()
        .find(|v| v.get("tag").and_then(|t| t.as_str()) == Some(path.as_str()))
    {
        return Ok(Json(found.clone()));
    }
    Err(StatusCode::NOT_FOUND)
}

async fn create_server(
    State(state): State<SsmState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let tag = body
        .get("tag")
        .and_then(|v| v.as_str())
        .unwrap_or("ssm-server")
        .to_string();
    let mut servers = state.servers.lock().unwrap();
    if !servers.is_object() {
        *servers = json!({});
    }
    if servers.get(&tag).is_some() {
        return Err(StatusCode::CONFLICT);
    }
    servers.as_object_mut().unwrap().insert(tag.clone(), body);
    Ok(Json(json!({ "created": tag })))
}

async fn update_server(
    State(state): State<SsmState>,
    Path(path): Path<String>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut servers = state.servers.lock().unwrap();
    if servers.get(&path).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    servers.as_object_mut().unwrap().insert(path.clone(), body);
    Ok(Json(json!({ "updated": path })))
}

async fn delete_server(
    State(state): State<SsmState>,
    Path(path): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !authorized(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let mut servers = state.servers.lock().unwrap();
    if servers.as_object_mut().unwrap().remove(&path).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(Json(json!({ "deleted": path })))
}
