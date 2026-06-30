//! Per-user policy registry for panel integration (G5 / sing-box user objects).

use dashmap::DashMap;
use rsb_config::Options;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use uuid::Uuid;

const MBPS_TO_BPS: u64 = 1_000_000;

#[derive(Debug, Clone, Default)]
pub struct UserLimits {
    pub max_connections: Option<usize>,
    pub max_traffic_bytes: Option<u64>,
    /// Combined up+down bytes per second (0 = unlimited).
    pub speed_bps: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct UserRecord {
    pub uuid: Uuid,
    pub name: String,
    pub inbound_tag: String,
    pub limits: UserLimits,
}

#[derive(Debug, Default)]
pub struct UserRuntime {
    pub uplink: AtomicU64,
    pub downlink: AtomicU64,
    pub active: AtomicUsize,
}

impl UserRuntime {
    pub fn total(&self) -> u64 {
        self.uplink.load(Ordering::Relaxed) + self.downlink.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Default)]
pub struct UserRegistry {
    by_uuid: DashMap<Uuid, Arc<UserRecord>>,
    by_name: DashMap<String, Arc<UserRecord>>,
    by_password: DashMap<String, Arc<UserRecord>>,
    by_trojan_hash: DashMap<String, Arc<UserRecord>>,
    by_inbound: DashMap<String, Vec<Arc<UserRecord>>>,
    runtime: DashMap<String, Arc<UserRuntime>>,
}

impl UserRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_options(options: &Options) -> Self {
        let registry = Self::new();
        for (i, ib) in options.inbounds.iter().enumerate() {
            let tag = options.inbound_tag(ib, i);
            registry.ingest_inbound(&ib.kind, &ib.raw, &tag);
        }
        registry
    }

    fn ingest_inbound(&self, kind: &str, raw: &Value, inbound_tag: &str) {
        match kind {
            "vless" | "vmess" | "tuic" => self.ingest_uuid_users(raw, inbound_tag),
            "trojan" | "anytls" | "hysteria2" | "shadowtls" => {
                self.ingest_password_users(raw, inbound_tag)
            }
            "shadowsocks" => self.ingest_shadowsocks_users(raw, inbound_tag),
            _ if raw.get("reality").is_some() => self.ingest_uuid_users(raw, inbound_tag),
            _ => {}
        }
    }

    fn ingest_uuid_users(&self, raw: &Value, inbound_tag: &str) {
        if let Some(arr) = raw.get("users").and_then(|v| v.as_array()) {
            for user in arr {
                if let Some(uuid_str) = user.get("uuid").and_then(|v| v.as_str()) {
                    if let Ok(uuid) = Uuid::parse_str(uuid_str) {
                        let name = user_display_name(user, &uuid);
                        let limits = parse_user_limits(user);
                        self.register(
                            UserRecord {
                                uuid,
                                name,
                                inbound_tag: inbound_tag.to_string(),
                                limits,
                            },
                            None,
                        );
                    }
                }
            }
        } else if let Some(uuid_str) = raw.get("uuid").and_then(|v| v.as_str()) {
            if let Ok(uuid) = Uuid::parse_str(uuid_str) {
                let name = user_display_name(raw, &uuid);
                let limits = parse_user_limits(raw);
                self.register(
                    UserRecord {
                        uuid,
                        name,
                        inbound_tag: inbound_tag.to_string(),
                        limits,
                    },
                    None,
                );
            }
        }
    }

    fn ingest_password_users(&self, raw: &Value, inbound_tag: &str) {
        if let Some(arr) = raw.get("users").and_then(|v| v.as_array()) {
            for user in arr {
                let password = user
                    .get("password")
                    .or_else(|| user.get("pass"))
                    .and_then(|v| v.as_str());
                if let Some(pass) = password {
                    let name = user_display_name(user, &Uuid::nil());
                    let uuid = uuid_from_password(pass);
                    let limits = parse_user_limits(user);
                    self.register(
                        UserRecord {
                            uuid,
                            name,
                            inbound_tag: inbound_tag.to_string(),
                            limits,
                        },
                        Some(pass),
                    );
                }
            }
        } else if let Some(pass) = raw.get("password").and_then(|v| v.as_str()) {
            let name = user_display_name(raw, &Uuid::nil());
            let uuid = uuid_from_password(pass);
            let limits = parse_user_limits(raw);
            self.register(
                UserRecord {
                    uuid,
                    name,
                    inbound_tag: inbound_tag.to_string(),
                    limits,
                },
                Some(pass),
            );
        }
    }

    fn ingest_shadowsocks_users(&self, raw: &Value, inbound_tag: &str) {
        if let Some(pass) = raw.get("password").and_then(|v| v.as_str()) {
            let name = user_display_name(raw, &Uuid::nil());
            let uuid = uuid_from_password(pass);
            let limits = parse_user_limits(raw);
            self.register(
                UserRecord {
                    uuid,
                    name,
                    inbound_tag: inbound_tag.to_string(),
                    limits,
                },
                Some(pass),
            );
        }
        self.ingest_password_users(raw, inbound_tag);
    }

    pub fn register(&self, record: UserRecord, password: Option<&str>) {
        let name = record.name.clone();
        let inbound_tag = record.inbound_tag.clone();
        let arc = Arc::new(record);
        self.by_uuid.insert(arc.uuid, arc.clone());
        self.by_name.insert(name, arc.clone());
        if let Some(pass) = password {
            self.by_password.insert(pass.to_string(), arc.clone());
            self.by_trojan_hash
                .insert(trojan_password_hash(pass), arc.clone());
        }
        self.by_inbound
            .entry(inbound_tag)
            .or_default()
            .push(arc);
    }

    pub fn reload_from_options(&self, options: &Options) {
        self.by_uuid.clear();
        self.by_name.clear();
        self.by_password.clear();
        self.by_trojan_hash.clear();
        self.by_inbound.clear();
        for (i, ib) in options.inbounds.iter().enumerate() {
            let tag = options.inbound_tag(ib, i);
            self.ingest_inbound(&ib.kind, &ib.raw, &tag);
        }
    }

    pub fn lookup_uuid(&self, uuid: &Uuid) -> Option<Arc<UserRecord>> {
        self.by_uuid.get(uuid).map(|e| e.clone())
    }

    pub fn lookup_password(&self, password: &str) -> Option<Arc<UserRecord>> {
        self.by_password.get(password).map(|e| e.clone())
    }

    pub fn lookup_trojan_hash(&self, hash: &str) -> Option<Arc<UserRecord>> {
        self.by_trojan_hash.get(hash).map(|e| e.clone())
    }

    pub fn first_for_inbound(&self, inbound_tag: &str) -> Option<Arc<UserRecord>> {
        self.by_inbound
            .get(inbound_tag)
            .and_then(|v| v.first().cloned())
    }

    pub fn runtime(&self, name: &str) -> Arc<UserRuntime> {
        self.runtime
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(UserRuntime::default()))
            .clone()
    }

    pub fn user_names(&self) -> Vec<String> {
        self.by_name.iter().map(|e| e.key().clone()).collect()
    }

    pub fn records(&self) -> Vec<Arc<UserRecord>> {
        self.by_name
            .iter()
            .map(|e| e.value().clone())
            .collect()
    }
}

pub fn trojan_password_hash(password: &str) -> String {
    use sha2::{Digest, Sha224};
    let mut hasher = Sha224::new();
    hasher.update(password.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn uuid_from_password(password: &str) -> Uuid {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    password.hash(&mut hasher);
    let h = hasher.finish();
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&h.to_le_bytes());
    bytes[8..].copy_from_slice(&h.rotate_left(17).to_le_bytes());
    Uuid::from_bytes(bytes)
}

fn user_display_name(user: &Value, uuid: &Uuid) -> String {
    user.get("name")
        .or_else(|| user.get("email"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| uuid.to_string())
}

fn parse_user_limits(user: &Value) -> UserLimits {
    let limit_obj = user
        .get("limit")
        .or_else(|| user.get("limits"))
        .and_then(|v| v.as_object());

    let max_connections = user
        .get("conn_limit")
        .or_else(|| user.get("connection_limit"))
        .or_else(|| user.get("max_connections"))
        .or_else(|| limit_obj.and_then(|o| o.get("connections")))
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);

    let max_traffic_bytes = user
        .get("traffic_limit_bytes")
        .or_else(|| user.get("traffic_limit"))
        .or_else(|| limit_obj.and_then(|o| o.get("traffic_bytes")))
        .and_then(|v| v.as_u64())
        .or_else(|| {
            user.get("traffic_limit_gb")
                .or_else(|| limit_obj.and_then(|o| o.get("traffic_gb")))
                .and_then(|v| v.as_f64())
                .map(|gb| (gb * 1024.0 * 1024.0 * 1024.0) as u64)
        });

    let speed_bps = user
        .get("speed_mbps")
        .or_else(|| user.get("download_speed_mbps"))
        .or_else(|| limit_obj.and_then(|o| o.get("speed_mbps")))
        .and_then(|v| v.as_u64())
        .map(|mbps| mbps * MBPS_TO_BPS)
        .or_else(|| {
            user.get("speed_bps")
                .or_else(|| limit_obj.and_then(|o| o.get("speed_bps")))
                .and_then(|v| v.as_u64())
        });

    UserLimits {
        max_connections,
        max_traffic_bytes,
        speed_bps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_user_limits_from_g5_fields() {
        let user = json!({
            "uuid": "550e8400-e29b-41d4-a716-446655440000",
            "name": "user1@example.com",
            "conn_limit": 3,
            "speed_mbps": 50,
            "traffic_limit_gb": 100.0
        });
        let limits = parse_user_limits(&user);
        assert_eq!(limits.max_connections, Some(3));
        assert_eq!(limits.speed_bps, Some(50 * MBPS_TO_BPS));
        assert_eq!(limits.max_traffic_bytes, Some(100 * 1024 * 1024 * 1024));
    }
}
