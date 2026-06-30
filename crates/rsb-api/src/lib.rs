use anyhow::Result;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
    Json, Router,
};
use rsb_config::ClashApiOptions;
use rsb_core::SharedConnectionManager;
use rsb_protocol::OutboundController;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::sync::watch;
use tokio::task::JoinHandle;

mod v2ray_grpc;

pub struct ClashApiServer {
    handle: Option<JoinHandle<()>>,
}

#[derive(Clone)]
pub struct ClashApiState {
    controller: Arc<OutboundController>,
    connections: SharedConnectionManager,
    secret: Option<String>,
    cache: Option<Arc<CacheFileService>>,
}

impl Default for ClashApiServer {
    fn default() -> Self {
        Self::new()
    }
}

impl ClashApiServer {
    pub fn new() -> Self {
        Self { handle: None }
    }

    pub async fn start(
        &mut self,
        options: &ClashApiOptions,
        controller: Arc<OutboundController>,
        connections: SharedConnectionManager,
        cache: Option<Arc<CacheFileService>>,
    ) -> Result<()> {
        let Some(controller_addr) = &options.external_controller else {
            return Ok(());
        };
        let addr: SocketAddr = controller_addr.parse()?;
        let state = ClashApiState {
            controller,
            connections,
            secret: options.secret.clone(),
            cache,
        };
        let app = Router::new()
            .route("/", get(version))
            .route("/version", get(version_json))
            .route("/proxies", get(list_proxies))
            .route("/proxies/{name}", get(get_proxy))
            .route("/proxies/{name}", put(select_proxy))
            .route("/connections", get(list_connections))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(%addr, "clash api listening");
        self.handle = Some(tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        }));
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}

async fn list_connections(
    State(state): State<ClashApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth_ok(&state, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let conns: Vec<_> = state
        .connections
        .list()
        .into_iter()
        .map(|c| {
            serde_json::json!({
                "id": c.id.to_string(),
                "metadata": {
                    "network": c.network,
                    "sourceIP": c.source.map(|s| s.ip().to_string()),
                    "destinationIP": c.destination.map(|d| d.ip().to_string()),
                    "destinationPort": c.destination.map(|d| d.port()),
                    "host": c.domain,
                },
                "chains": [c.outbound_tag],
                "rule": "",
                "start": c.started_at,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "connections": conns })))
}

async fn version() -> &'static str {
    concat!("rsbox/", env!("CARGO_PKG_VERSION"))
}

async fn version_json() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "premium": false,
        "meta": true,
    }))
}

async fn list_proxies(
    State(state): State<ClashApiState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth_ok(&state, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    state
        .controller
        .list_proxies()
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn get_proxy(
    State(state): State<ClashApiState>,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth_ok(&state, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let all = state
        .controller
        .list_proxies()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    all.get(&name)
        .cloned()
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

#[derive(serde::Deserialize)]
struct SelectBody {
    name: String,
}

async fn select_proxy(
    State(state): State<ClashApiState>,
    Path(selector): Path<String>,
    headers: HeaderMap,
    Json(body): Json<SelectBody>,
) -> Result<StatusCode, StatusCode> {
    if !auth_ok(&state, &headers) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    state
        .controller
        .select(&selector, &body.name)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    if let Some(cache) = &state.cache {
        cache.set_selector(&selector, &body.name);
        let _ = cache.flush().await;
    }
    Ok(StatusCode::NO_CONTENT)
}

fn auth_ok(state: &ClashApiState, headers: &HeaderMap) -> bool {
    let Some(secret) = &state.secret else {
        return true;
    };
    headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim_start_matches("Bearer ") == secret)
        .unwrap_or(false)
}

pub struct V2RayApiServer {
    http_handle: Option<JoinHandle<()>>,
    grpc_handle: Option<JoinHandle<()>>,
    grpc_shutdown: Option<watch::Sender<bool>>,
}

#[derive(Clone)]
struct V2RayApiState {
    connections: SharedConnectionManager,
}

impl V2RayApiServer {
    pub async fn start(
        options: &serde_json::Value,
        connections: SharedConnectionManager,
    ) -> Result<Self> {
        let mut server = Self {
            http_handle: None,
            grpc_handle: None,
            grpc_shutdown: None,
        };
        if options.is_null() {
            return Ok(server);
        }
        let listen = options
            .get("listen")
            .and_then(|v| v.as_str())
            .unwrap_or("127.0.0.1:8080");
        let addr: SocketAddr = listen.parse()?;
        let http_enabled = options
            .get("http")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        if http_enabled {
            let state = V2RayApiState {
                connections: connections.clone(),
            };
            let app = Router::new()
                .route("/stats", get(v2ray_stats).post(v2ray_stats_query))
                .route("/debug/vars", get(v2ray_stats))
                .with_state(state);
            let listener = tokio::net::TcpListener::bind(addr).await?;
            tracing::info!(%addr, "v2ray api http listening");
            server.http_handle = Some(tokio::spawn(async move {
                let _ = axum::serve(listener, app).await;
            }));
        }
        let grpc_listen: Option<SocketAddr> = options
            .get("grpc_listen")
            .and_then(|v| v.as_str())
            .map(str::parse)
            .transpose()?;
        let grpc_listen =
            grpc_listen.or_else(|| if http_enabled { None } else { Some(addr) });
        if let Some(grpc_addr) = grpc_listen {
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            server.grpc_shutdown = Some(shutdown_tx);
            let conns = connections.clone();
            tracing::info!(%grpc_addr, "v2ray api grpc listening");
            server.grpc_handle = Some(tokio::spawn(async move {
                v2ray_grpc::spawn_v2ray_stats_grpc(conns, grpc_addr, shutdown_rx).await;
            }));
        }
        Ok(server)
    }

    pub fn stop(&mut self) {
        if let Some(h) = self.http_handle.take() {
            h.abort();
        }
        if let Some(tx) = self.grpc_shutdown.take() {
            let _ = tx.send(true);
        }
        if let Some(h) = self.grpc_handle.take() {
            h.abort();
        }
    }
}

#[derive(serde::Deserialize, Default)]
struct V2RayStatsQuery {
    pattern: Option<String>,
    reset: Option<bool>,
}

#[derive(serde::Deserialize, Default)]
struct V2RayStatsBody {
    pattern: Option<String>,
    reset: Option<bool>,
}

async fn v2ray_stats(
    State(state): State<V2RayApiState>,
    Query(query): Query<V2RayStatsQuery>,
) -> Json<serde_json::Value> {
    v2ray_stats_json(&state, query.pattern.as_deref().unwrap_or(""), query.reset.unwrap_or(false))
}

async fn v2ray_stats_query(
    State(state): State<V2RayApiState>,
    Json(body): Json<V2RayStatsBody>,
) -> Json<serde_json::Value> {
    v2ray_stats_json(
        &state,
        body.pattern.as_deref().unwrap_or(""),
        body.reset.unwrap_or(false),
    )
}

fn v2ray_stats_json(state: &V2RayApiState, pattern: &str, reset: bool) -> Json<serde_json::Value> {
    let stat: Vec<_> = state
        .connections
        .query_v2ray_stats(pattern, reset)
        .into_iter()
        .map(|(name, value)| serde_json::json!({ "name": name, "value": value }))
        .collect();
    Json(serde_json::json!({ "stat": stat }))
}

#[derive(Default)]
struct CacheState {
    selectors: HashMap<String, String>,
}

pub struct CacheFileService {
    path: PathBuf,
    state: Arc<RwLock<CacheState>>,
}

impl Clone for CacheFileService {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            state: self.state.clone(),
        }
    }
}

impl CacheFileService {
    pub async fn start(options: &serde_json::Value) -> Result<Self> {
        let path: PathBuf = options
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("cache.json")
            .into();
        let svc = Self {
            path: path.clone(),
            state: Arc::new(RwLock::new(CacheState::default())),
        };
        if FsPath::new(&path).exists() {
            if let Ok(text) = tokio::fs::read_to_string(&path).await {
                if let Ok(st) = serde_json::from_str::<CacheState>(&text) {
                    *svc.state.write().unwrap() = st;
                }
            }
        }
        tracing::info!(path = %path.display(), "cache_file loaded");
        Ok(svc)
    }

    pub fn set_selector(&self, group: &str, selected: &str) {
        self.state
            .write()
            .unwrap()
            .selectors
            .insert(group.to_string(), selected.to_string());
    }

    pub fn get_selector(&self, group: &str) -> Option<String> {
        self.state.read().unwrap().selectors.get(group).cloned()
    }

    pub fn selectors(&self) -> HashMap<String, String> {
        self.state.read().unwrap().selectors.clone()
    }

    pub async fn flush(&self) -> Result<()> {
        let st = self.state.read().unwrap().clone();
        let text = serde_json::to_string_pretty(&st)?;
        tokio::fs::write(&self.path, text).await?;
        Ok(())
    }
}

impl Clone for CacheState {
    fn clone(&self) -> Self {
        Self {
            selectors: self.selectors.clone(),
        }
    }
}

impl serde::Serialize for CacheState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("CacheState", 1)?;
        st.serialize_field("selectors", &self.selectors)?;
        st.end()
    }
}

impl<'de> serde::Deserialize<'de> for CacheState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            #[serde(default)]
            selectors: HashMap<String, String>,
        }
        let raw = Raw::deserialize(d)?;
        Ok(CacheState {
            selectors: raw.selectors,
        })
    }
}
