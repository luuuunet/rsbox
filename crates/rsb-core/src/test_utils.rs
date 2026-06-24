//! Test utilities and helpers for rsbox
//!
//! This module provides common testing infrastructure used across the project.

use std::net::SocketAddr;
use tokio::net::TcpListener;

/// Find an available port for testing
pub async fn find_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to a port");
    listener
        .local_addr()
        .expect("Failed to get local addr")
        .port()
}

/// Create a test TCP listener
pub async fn test_listener() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to create test listener");
    let addr = listener.local_addr().expect("Failed to get local addr");
    (listener, addr)
}

/// Test configuration helper
pub fn test_config_json() -> &'static str {
    r#"{
        "log": {"level": "debug"},
        "inbounds": [{
            "type": "mixed",
            "listen": "127.0.0.1",
            "listen_port": 0
        }],
        "outbounds": [{"type": "direct", "tag": "direct"}],
        "route": {"final": "direct"}
    }"#
}

/// Create a minimal test configuration
pub fn minimal_config() -> rsb_config::Options {
    serde_json::from_str(test_config_json()).expect("Failed to parse test config")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_find_free_port() {
        let port = find_free_port().await;
        assert!(port > 0);
    }

    #[tokio::test]
    async fn test_listener_creation() {
        let (_listener, addr) = test_listener().await;
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }

    #[test]
    fn test_config_parsing() {
        let config = minimal_config();
        assert_eq!(config.inbounds.len(), 1);
        assert_eq!(config.outbounds.len(), 1);
    }
}
