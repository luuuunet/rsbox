use anyhow::Result;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, put},
    Json, Router,
};
use rsb_config::ClashApiOptions;
use rsb_core::SharedConnectionManager;
use rsb_protocol::OutboundController;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::sync::{Arc, RwLock};
use tokio::task::JoinHandle;

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
    handle: Option<JoinHandle<()>>,
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
        let mut server = Self { handle: None };
        if options.is_null() {
            return Ok(server);
        }
        let listen = options
            .get("listen")
            .and_then(|v| v.as_str())
            .unwrap_or("127.0.0.1:8080");
        let addr: SocketAddr = listen.parse()?;
        let state = V2RayApiState { connections };
        let app = Router::new()
            .route("/stats", get(v2ray_stats))
            .route("/debug/vars", get(v2ray_stats))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        tracing::info!(%addr, "v2ray api listening");
        server.handle = Some(tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        }));
        Ok(server)
    }

    pub fn stop(&mut self) {
        if let Some(h) = self.handle.take() {
            h.abort();
        }
    }
}

async fn v2ray_stats(State(state): State<V2RayApiState>) -> Json<serde_json::Value> {
    let stat: Vec<_> = state
        .connections
        .v2ray_stat_entries()
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
