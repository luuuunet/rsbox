//! Force browsers off HTTP/3 under system proxy **without Administrator**.
//!
//! System HTTP proxy cannot carry QUIC (UDP/443). Blocking that with Windows Firewall
//! needs elevation — pointless vs TUN. Instead we set per-user Chromium policies
//! (`QuicAllowed=0` under HKCU), which Chrome/Edge/Brave honor without admin.
//!
//! Note: browsers typically apply policy on next process start; log asks user to
//! restart the browser once after connect.

use anyhow::{Context, Result};
use std::net::IpAddr;

/// RAII guard: restores previous HKCU QuicAllowed policies on drop.
pub struct QuicBlockGuard {
    #[cfg(windows)]
    restores: Vec<PolicyRestore>,
    #[cfg(not(windows))]
    _unused: (),
}

#[cfg(windows)]
struct PolicyRestore {
    subkey: &'static str,
    /// Previous DWORD if the value existed before we wrote; `None` means delete on restore.
    previous: Option<u32>,
}

impl QuicBlockGuard {
    /// Install QUIC-disable policies. Failures return `None` after logging — never abort startup.
    pub fn try_install(allow_udp: &[(String, u16)]) -> Option<Self> {
        let _ = allow_udp; // allowlist kept for API stability / future WFP use
        match install() {
            Ok(guard) => Some(guard),
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "block_quic: failed to set browser QuicAllowed=0 under HKCU"
                );
                None
            }
        }
    }
}

impl Drop for QuicBlockGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            for item in self.restores.drain(..) {
                if let Err(err) = restore_policy(&item) {
                    tracing::warn!(
                        subkey = item.subkey,
                        error = %err,
                        "block_quic: failed to restore QuicAllowed"
                    );
                }
            }
            // Best-effort cleanup of leftover firewall rules from older rsbox builds.
            let _ = remove_legacy_firewall_rules();
            tracing::info!("block_quic: restored browser QuicAllowed policies");
        }
    }
}

fn install() -> Result<QuicBlockGuard> {
    #[cfg(windows)]
    {
        let _ = remove_legacy_firewall_rules();
        let mut restores = Vec::new();
        for subkey in POLICY_SUBKEYS {
            let previous = read_quic_allowed(subkey)?;
            set_quic_allowed(subkey, 0)?;
            restores.push(PolicyRestore { subkey, previous });
            tracing::info!(%subkey, "block_quic: QuicAllowed=0 (HKCU, no admin)");
        }
        if restores.is_empty() {
            anyhow::bail!("no browser policy keys written");
        }
        tracing::warn!(
            "block_quic: restart Chrome/Edge/Brave once so QuicAllowed policy takes effect"
        );
        Ok(QuicBlockGuard { restores })
    }
    #[cfg(not(windows))]
    {
        tracing::debug!("block_quic: unsupported on this platform (no-op)");
        anyhow::bail!("block_quic unsupported on this platform")
    }
}

#[cfg(windows)]
const POLICY_SUBKEYS: &[&str] = &[
    r"Software\Policies\Google\Chrome",
    r"Software\Policies\Microsoft\Edge",
    r"Software\Policies\BraveSoftware\Brave",
    r"Software\Policies\Chromium",
];

#[cfg(windows)]
const VALUE_NAME: &str = "QuicAllowed";

#[cfg(windows)]
fn read_quic_allowed(subkey: &str) -> Result<Option<u32>> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = match hkcu.open_subkey(subkey) {
        Ok(k) => k,
        Err(_) => return Ok(None),
    };
    match key.get_value::<u32, _>(VALUE_NAME) {
        Ok(v) => Ok(Some(v)),
        Err(_) => Ok(None),
    }
}

#[cfg(windows)]
fn set_quic_allowed(subkey: &str, value: u32) -> Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(subkey)
        .with_context(|| format!("create {subkey}"))?;
    key.set_value(VALUE_NAME, &value)
        .with_context(|| format!("set {subkey}\\{VALUE_NAME}"))?;
    Ok(())
}

#[cfg(windows)]
fn restore_policy(item: &PolicyRestore) -> Result<()> {
    use winreg::enums::*;
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    match item.previous {
        Some(v) => {
            let (key, _) = hkcu.create_subkey(item.subkey)?;
            key.set_value(VALUE_NAME, &v)?;
        }
        None => {
            if let Ok(key) = hkcu.open_subkey_with_flags(item.subkey, KEY_SET_VALUE) {
                let _ = key.delete_value(VALUE_NAME);
            }
        }
    }
    Ok(())
}

/// Older rsbox versions installed `rsbox-block-quic*` firewall rules (needed admin).
/// Clear them when possible so we do not leave orphans after upgrading.
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

/// Resolve hostnames in allowlist for logging / future use.
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
}
