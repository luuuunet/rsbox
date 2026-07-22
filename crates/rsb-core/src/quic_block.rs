//! Force browsers off HTTP/3 under system proxy **without Administrator**.
//!
//! System HTTP proxy cannot carry QUIC (UDP/443). Firewall blocking needs elevation
//! (pointless vs TUN). HKCU policy keys are often locked by enterprise GPO.
//!
//! Approach: patch Chromium `Local State` → `browser.enabled_labs_experiments`
//! with `enable-quic@2` (Disabled). No admin required. Restore on drop.
//!
//! Browsers apply labs flags on next launch — user should restart Chrome/Edge once.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

/// Labs three-state: 2 = Disabled (Chromium FeatureEntry).
const QUIC_DISABLED_FLAG: &str = "enable-quic@2";

/// RAII guard: restores previous Local State experiments on drop.
pub struct QuicBlockGuard {
    restores: Vec<LocalStateRestore>,
}

struct LocalStateRestore {
    path: PathBuf,
    /// Previous `enabled_labs_experiments` array (or None if missing).
    previous_experiments: Option<Value>,
}

impl QuicBlockGuard {
    /// Install QUIC-disable flags. Failures return `None` after logging — never abort startup.
    pub fn try_install(allow_udp: &[(String, u16)]) -> Option<Self> {
        let _ = allow_udp;
        match install() {
            Ok(guard) => Some(guard),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "block_quic: failed to disable browser QUIC via Local State"
                );
                None
            }
        }
    }
}

impl Drop for QuicBlockGuard {
    fn drop(&mut self) {
        for item in self.restores.drain(..) {
            if let Err(err) = restore_local_state(&item) {
                tracing::warn!(
                    path = %item.path.display(),
                    error = %err,
                    "block_quic: failed to restore Local State"
                );
            }
        }
        #[cfg(windows)]
        {
            let _ = remove_legacy_firewall_rules();
        }
        tracing::info!("block_quic: restored browser Local State experiments");
    }
}

fn install() -> Result<QuicBlockGuard> {
    #[cfg(windows)]
    {
        let _ = remove_legacy_firewall_rules();
    }
    let mut restores = Vec::new();
    for path in chromium_local_state_paths() {
        if !path.is_file() {
            continue;
        }
        match patch_local_state(&path) {
            Ok(Some(prev)) => {
                tracing::info!(path = %path.display(), "block_quic: enable-quic@2 set in Local State");
                restores.push(LocalStateRestore {
                    path,
                    previous_experiments: prev,
                });
            }
            Ok(None) => {
                tracing::debug!(path = %path.display(), "block_quic: already disabled");
            }
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "block_quic: skip Local State");
            }
        }
    }
    if restores.is_empty() {
        anyhow::bail!("no Chromium Local State patched (is Chrome/Edge installed?)");
    }
    tracing::warn!(
        "block_quic: fully quit and reopen Chrome/Edge once so disable-QUIC takes effect"
    );
    Ok(QuicBlockGuard { restores })
}

fn chromium_local_state_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return out;
    };
    let candidates = [
        local.join(r"Google\Chrome\User Data\Local State"),
        local.join(r"Google\Chrome Beta\User Data\Local State"),
        local.join(r"Microsoft\Edge\User Data\Local State"),
        local.join(r"Microsoft\Edge Beta\User Data\Local State"),
        local.join(r"BraveSoftware\Brave-Browser\User Data\Local State"),
        local.join(r"Chromium\User Data\Local State"),
        local.join(r"Vivaldi\User Data\Local State"),
    ];
    for p in candidates {
        if p.is_file() {
            out.push(p);
        }
    }
    out
}

/// Returns `Some(previous_experiments)` when we changed the file; `None` if already set.
fn patch_local_state(path: &Path) -> Result<Option<Option<Value>>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut root: Value =
        serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))?;

    let browser = root
        .as_object_mut()
        .context("Local State root not object")?
        .entry("browser")
        .or_insert_with(|| json!({}));
    let browser_obj = browser.as_object_mut().context("browser not object")?;

    let previous = browser_obj.get("enabled_labs_experiments").cloned();
    let mut experiments: Vec<Value> = match &previous {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    };

    let already = experiments.iter().any(|v| {
        v.as_str()
            .map(|s| s == QUIC_DISABLED_FLAG || s.starts_with("enable-quic@"))
            .unwrap_or(false)
    });
    if already {
        // Ensure exactly disabled (@2), replace other enable-quic@N if needed.
        let mut changed = false;
        for v in &mut experiments {
            if let Some(s) = v.as_str() {
                if s.starts_with("enable-quic@") && s != QUIC_DISABLED_FLAG {
                    *v = Value::String(QUIC_DISABLED_FLAG.into());
                    changed = true;
                }
            }
        }
        if !changed {
            return Ok(None);
        }
    } else {
        experiments.push(Value::String(QUIC_DISABLED_FLAG.into()));
    }

    browser_obj.insert(
        "enabled_labs_experiments".into(),
        Value::Array(experiments),
    );

    write_atomic(path, &serde_json::to_string(&root)?)?;
    Ok(Some(previous))
}

fn restore_local_state(item: &LocalStateRestore) -> Result<()> {
    if !item.path.is_file() {
        return Ok(());
    }
    let text = fs::read_to_string(&item.path)?;
    let mut root: Value = serde_json::from_str(&text)?;
    let Some(browser) = root.get_mut("browser").and_then(|v| v.as_object_mut()) else {
        return Ok(());
    };
    match &item.previous_experiments {
        Some(prev) => {
            browser.insert("enabled_labs_experiments".into(), prev.clone());
        }
        None => {
            // Remove only our flag if we invented the array.
            if let Some(Value::Array(arr)) = browser.get_mut("enabled_labs_experiments") {
                arr.retain(|v| v.as_str() != Some(QUIC_DISABLED_FLAG));
                if arr.is_empty() {
                    browser.remove("enabled_labs_experiments");
                }
            }
        }
    }
    write_atomic(&item.path, &serde_json::to_string(&root)?)?;
    Ok(())
}

fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let tmp = path.with_extension("rsbox-tmp");
    fs::write(&tmp, contents).with_context(|| format!("write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename onto {}", path.display()))?;
    Ok(())
}

#[cfg(windows)]
fn remove_legacy_firewall_rules() -> Result<()> {
    use std::process::Command;
    const RULE_PREFIX: &str = "rsbox-block-quic";
    let list = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "Get-NetFirewallRule -ErrorAction SilentlyContinue | Where-Object {{ $_.DisplayName -like '{RULE_PREFIX}*' }} | ForEach-Object {{ $_.DisplayName }}"
            ),
        ])
        .output();
    let Ok(out) = list else {
        return Ok(());
    };
    let text = String::from_utf8_lossy(&out.stdout);
    for name in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
        let _ = Command::new("netsh")
            .args(["advfirewall", "firewall", "delete", "rule", &format!("name={name}")])
            .output();
    }
    Ok(())
}

#[allow(dead_code)]
pub fn resolve_allow_hosts(hosts: &[(String, u16)]) -> Vec<(IpAddr, u16)> {
    let mut out = Vec::new();
    for (host, port) in hosts {
        if let Ok(ip) = host.parse::<IpAddr>() {
            out.push((ip, *port));
            continue;
        }
        if let Ok(iters) = dns_lookup_host(host) {
            for ip in iters {
                out.push((ip, *port));
            }
        }
    }
    out.sort_by_key(|(ip, p)| (ip.to_string(), *p));
    out.dedup();
    out
}

fn dns_lookup_host(host: &str) -> Result<Vec<IpAddr>> {
    use std::net::ToSocketAddrs;
    let addrs = (host, 0u16)
        .to_socket_addrs()
        .with_context(|| format!("resolve {host}"))?;
    Ok(addrs.map(|a| a.ip()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_literal_ip() {
        let v = resolve_allow_hosts(&[("1.2.3.4".into(), 9978)]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].1, 9978);
    }

    #[test]
    fn patch_adds_quic_disabled_flag() {
        let dir = tempfile_dir();
        let path = dir.join("Local State");
        fs::write(&path, r#"{"browser":{}}"#).unwrap();
        let prev = patch_local_state(&path).unwrap();
        assert!(prev.is_some());
        let root: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let arr = root["browser"]["enabled_labs_experiments"]
            .as_array()
            .unwrap();
        assert!(arr.iter().any(|v| v.as_str() == Some(QUIC_DISABLED_FLAG)));
        let _ = fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("rsbox-quic-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&p);
        p
    }
}
