//! sing-box API service (HTTP JSON control plane).

use super::context::ServiceContext;
use super::listen::{auth_token, parse_listen, parse_user_tokens};
use anyhow::Result;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde_json::Value;
use std::net::SocketAddr;
use std::sync::Arc;

pub struct ApiService {
    tag: String,
    listen: SocketAddr,
    grpc_listen: Option<SocketAddr>,
    users: Vec<(String, String)>,
    ctx: ServiceContext,
    shutdown: tokio::sync::watch::Sender<bool>,
    handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
    grpc_handle: tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[derive(Clone)]
struct ApiState {
    ctx: ServiceContext,
    users: Arc<Vec<(String, String)>>,
}

impl ApiService {
    pub fn new(tag: String, raw: Value, ctx: ServiceContext) -> Result<Self> {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        let secret = raw
            .get("secret")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let mut users = parse_user_tokens(&raw);
        if users.is_empty() {
            if let Some(s) = &secret {
                users.push(("api".into(), s.clone()));
            }
        }
        Ok(Self {
            tag,
            listen: parse_listen(&raw)?,
            grpc_listen: parse_grpc_listen(&raw),
            users,
            ctx,
            shutdown,
            handle: tokio::sync::Mutex::new(None),
            grpc_handle: tokio::sync::Mutex::new(None),
        })
    }

    pub async fn start(&self) -> Result<()> {
        let state = ApiState {
            ctx: self.ctx.clone(),
            users: Arc::new(self.users.clone()),
        };
        let app = Router::new()
            .route("/", get(status))
            .route("/version", get(version))
            .route("/outbounds", get(outbounds))
            .route("/connections", get(connections))
            .route("/connections/close", post(close_connection))
            .route("/connections/close/all", post(close_all_connections))
            .route("/stats", get(stats))
            .route("/selector/select", post(select_outbound))
            .route("/urltest", post(url_test))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(self.listen).await?;
        tracing::info!(tag = %self.tag, %self.listen, "api service listening");
        let mut shutdown = self.shutdown.subscribe();
        let handle = tokio::spawn(async move {
            let server = axum::serve(listener, app);
            tokio::select! {
                _ = shutdown.changed() => {}
                r = server => { let _ = r; }
            }
        });
        *self.handle.lock().await = Some(handle);

        if let Some(grpc_addr) = self.grpc_listen {
            tracing::info!(tag = %self.tag, %grpc_addr, "api gRPC listening");
            let ctx = self.ctx.clone();
            let mut grpc_shutdown = self.shutdown.subscribe();
            let grpc_task = tokio::spawn(async move {
                super::api_grpc::spawn_grpc(ctx, grpc_addr, grpc_shutdown).await;
            });
            *self.grpc_handle.lock().await = Some(grpc_task);
        }
        Ok(())
    }

    pub async fn close(&self) -> Result<()> {
        let _ = self.shutdown.send(true);
        if let Some(h) = self.handle.lock().await.take() {
            h.abort();
        }
        if let Some(h) = self.grpc_handle.lock().await.take() {
            h.abort();
        }
        Ok(())
    }
}

fn parse_grpc_listen(raw: &Value) -> Option<SocketAddr> {
    if raw.get("grpc_listen_port").is_none()
        && raw.get("grpc_listen").and_then(|v| v.as_str()).is_none()
    {
        return None;
    }
    let mut grpc_raw = raw.clone();
    if let Some(obj) = grpc_raw.as_object_mut() {
        if let Some(port) = obj.remove("grpc_listen_port") {
            obj.insert("listen_port".into(), port);
        }
        if let Some(host) = obj.get("grpc_listen").cloned() {
            obj.insert("listen".into(), host);
        }
    }
    parse_listen(&grpc_raw).ok()
}

fn auth(headers: &HeaderMap, users: &[(String, String)]) -> bool {
    users.is_empty() || auth_token(headers, users).is_some()
}

async fn status(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(serde_json::json!({
        "status": "running",
        "version": env!("CARGO_PKG_VERSION"),
    })))
}

async fn version(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "experimental": true,
    })))
}

async fn outbounds(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let list: Vec<_> = state
        .ctx
        .options
        .outbounds
        .iter()
        .enumerate()
        .map(|(i, ob)| {
            serde_json::json!({
                "tag": state.ctx.options.outbound_tag(ob, i),
                "type": ob.kind,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "outbounds": list })))
}

async fn connections(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let conns: Vec<_> = state
        .ctx
        .connections
        .list()
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id.to_string(),
                "inbound": c.inbound_tag,
                "outbound": c.outbound_tag,
                "network": c.network,
                "source": c.source.map(|a| a.to_string()),
                "destination": c.destination.map(|a| a.to_string()),
                "domain": c.domain,
                "started_at": c.started_at,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "connections": conns })))
}

#[derive(serde::Deserialize)]
struct CloseConnBody {
    id: u64,
}

async fn close_connection(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(body): Json<CloseConnBody>,
) -> Result<Json<Value>, StatusCode> {
    if !auth(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    state.ctx.connections.untrack(body.id);
    Ok(Json(serde_json::json!({ "closed": body.id })))
}

#[derive(serde::Deserialize)]
struct SelectBody {
    group_tag: String,
    outbound_tag: String,
}

async fn select_outbound(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(body): Json<SelectBody>,
) -> Result<Json<Value>, StatusCode> {
    if !auth(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    state
        .ctx
        .controller
        .select(&body.group_tag, &body.outbound_tag)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok(Json(serde_json::json!({
        "group": body.group_tag,
        "selected": body.outbound_tag,
    })))
}

async fn close_all_connections(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let ids: Vec<_> = state
        .ctx
        .connections
        .list()
        .into_iter()
        .map(|c| c.id)
        .collect();
    for id in &ids {
        state.ctx.connections.untrack(*id);
    }
    Ok(Json(serde_json::json!({ "closed": ids.len() })))
}

async fn stats(
    State(state): State<ApiState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    if !auth(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(Json(serde_json::json!({
        "connections": state.ctx.connections.list().len(),
        "outbounds": state.ctx.options.outbounds.len(),
        "inbounds": state.ctx.options.inbounds.len(),
    })))
}

#[derive(serde::Deserialize)]
struct UrlTestBody {
    group_tag: String,
}

async fn url_test(
    State(state): State<ApiState>,
    headers: HeaderMap,
    Json(body): Json<UrlTestBody>,
) -> Result<Json<Value>, StatusCode> {
    if !auth(&headers, &state.users) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let (selected, delays) = state
        .ctx
        .controller
        .run_url_test(&body.group_tag)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let delay_list: Vec<_> = delays
        .into_iter()
        .map(|(tag, ms)| serde_json::json!({ "tag": tag, "delay_ms": ms }))
        .collect();
    Ok(Json(serde_json::json!({
        "group": body.group_tag,
        "selected": selected,
        "delays": delay_list,
    })))
}
