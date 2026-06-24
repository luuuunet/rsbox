//! Registry mapping resolved-service tags to shared DnsRouter instances.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::DnsRouter;

static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<DnsRouter>>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Arc<DnsRouter>>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register_resolved_service(tag: &str, dns: Arc<DnsRouter>) {
    if let Ok(mut map) = registry().lock() {
        map.insert(tag.to_string(), dns);
    }
}

pub fn unregister_resolved_service(tag: &str) {
    if let Ok(mut map) = registry().lock() {
        map.remove(tag);
    }
}

pub fn resolved_dns(tag: &str) -> Option<Arc<DnsRouter>> {
    registry().lock().ok()?.get(tag).cloned()
}
