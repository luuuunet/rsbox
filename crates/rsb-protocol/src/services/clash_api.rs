// Clash API 兼容实现
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::{delete, get, patch, put},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct ClashApi {
    ctx: Arc<ClashContext>,
}

pub struct ClashContext {
    // 保存运行时上下文
}

impl ClashApi {
    pub fn new() -> Self {
        Self {
            ctx: Arc::new(ClashContext {}),
        }
    }

    pub fn router(self) -> Router {
        Router::new()
            // 基础端点
            .route("/", get(root))
            .route("/version", get(version))
            .route("/configs", get(get_configs).patch(update_configs))
            // 代理相关
            .route("/proxies", get(get_proxies))
            .route("/proxies/:name", get(get_proxy).put(select_proxy))
            .route("/proxies/:name/delay", get(proxy_delay))
            // 规则相关
            .route("/rules", get(get_rules))
            // 连接相关
            .route("/connections", get(get_connections))
            .route("/connections/:id", delete(close_connection))
            // 流量相关
            .route("/traffic", get(get_traffic))
            .with_state(self)
    }
}

async fn root() -> Json<Value> {
    Json(json!({
        "hello": "rsbox clash api"
    }))
}

async fn version() -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "meta": false,
        "premium": false
    }))
}

#[derive(Debug, Serialize)]
struct ClashConfig {
    port: u16,
    socks_port: u16,
    mixed_port: Option<u16>,
    allow_lan: bool,
    mode: String,
    log_level: String,
}

async fn get_configs(State(_api): State<ClashApi>) -> Json<ClashConfig> {
    Json(ClashConfig {
        port: 7890,
        socks_port: 7891,
        mixed_port: Some(7890),
        allow_lan: false,
        mode: "rule".to_string(),
        log_level: "info".to_string(),
    })
}

async fn update_configs(
    State(_api): State<ClashApi>,
    Json(payload): Json<Value>,
) -> StatusCode {
    tracing::info!("Update config: {:?}", payload);
    StatusCode::NO_CONTENT
}

async fn get_proxies(State(_api): State<ClashApi>) -> Json<Value> {
    Json(json!({
        "proxies": {
            "DIRECT": {
                "type": "Direct",
                "name": "DIRECT"
            },
            "REJECT": {
                "type": "Reject",
                "name": "REJECT"
            }
        }
    }))
}

async fn get_proxy(
    State(_api): State<ClashApi>,
    Path(name): Path<String>,
) -> Json<Value> {
    Json(json!({
        "name": name,
        "type": "Shadowsocks",
        "history": []
    }))
}

#[derive(Debug, Deserialize)]
struct SelectProxyRequest {
    name: String,
}

async fn select_proxy(
    State(_api): State<ClashApi>,
    Path(proxy_name): Path<String>,
    Json(payload): Json<SelectProxyRequest>,
) -> StatusCode {
    tracing::info!(
        proxy = %proxy_name,
        selected = %payload.name,
        "Select proxy"
    );
    StatusCode::NO_CONTENT
}

async fn proxy_delay(
    State(_api): State<ClashApi>,
    Path(name): Path<String>,
) -> Json<Value> {
    Json(json!({
        "delay": 100,
        "meanDelay": 100
    }))
}

async fn get_rules(State(_api): State<ClashApi>) -> Json<Value> {
    Json(json!({
        "rules": []
    }))
}

async fn get_connections(State(_api): State<ClashApi>) -> Json<Value> {
    Json(json!({
        "downloadTotal": 0,
        "uploadTotal": 0,
        "connections": []
    }))
}

async fn close_connection(
    State(_api): State<ClashApi>,
    Path(id): Path<String>,
) -> StatusCode {
    tracing::info!(connection_id = %id, "Close connection");
    StatusCode::NO_CONTENT
}

async fn get_traffic(State(_api): State<ClashApi>) -> Json<Value> {
    Json(json!({
        "up": 0,
        "down": 0
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_clash_api_version() {
        let response = version().await;
        let value = response.0;
        assert!(value.get("version").is_some());
    }
}
