//! Force browsers off HTTP/3 under system proxy by blocking UDP/443 at the OS layer.
//!
//! Windows Firewall evaluates outbound **block before allow**, so a global UDP/443 block
//! would also drop node tunnels that listen on 443. Strategy:
//! 1. Always block UDP/443 for common browser executables (Chrome/Edge/Firefox/…).
//! 2. If no UDP tunnel outbound uses remote port 443, also install a global UDP/443 block.
//! 3. Otherwise skip the global block and rely on browser rules (rsbox.exe stays unrestricted).

use anyhow::{Context, Result};
use std::net::IpAddr;
use std::process::Command;

const RULE_PREFIX: &str = "rsbox-block-quic";

/// RAII guard: removes firewall rules on drop.
pub struct QuicBlockGuard {
    installed: bool,
}

impl QuicBlockGuard {
    /// Install QUIC blocks. On non-Windows this is a no-op success.
    /// Failures (e.g. not admin) return `Ok(None)` after logging — never abort startup.
    pub fn try_install(allow_udp: &[(String, u16)]) -> Option<Self> {
        match install(allow_udp) {
            Ok(true) => Some(Self { installed: true }),
            Ok(false) => None,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    "block_quic: failed to install firewall rules (need Administrator?). Browser HTTP/3 may bypass system proxy"
                );
                None
            }
        }
    }
}

impl Drop for QuicBlockGuard {
    fn drop(&mut self) {
        if self.installed {
            if let Err(err) = remove_all_rules() {
                tracing::warn!(error = %err, "block_quic: failed to remove firewall rules");
            } else {
                tracing::info!("block_quic: firewall rules removed");
            }
        }
    }
}

fn install(allow_udp: &[(String, u16)]) -> Result<bool> {
    #[cfg(windows)]
    {
        remove_all_rules().ok();
        let mut any = false;
        let mut last_err: Option<anyhow::Error> = None;
        for (i, program) in browser_programs().into_iter().enumerate() {
            let name = format!("{RULE_PREFIX}-browser-{i}");
            match add_block_udp443_program(&name, &program) {
                Ok(()) => {
                    any = true;
                    tracing::info!(%program, rule = %name, "block_quic: browser UDP/443 blocked");
                }
                Err(err) => {
                    tracing::debug!(%program, error = %err, "block_quic: skip browser rule");
                    last_err = Some(err);
                }
            }
        }
        let has_udp443_tunnel = allow_udp.iter().any(|(_, p)| *p == 443);
        if has_udp443_tunnel {
            tracing::info!(
                "block_quic: UDP tunnel on :443 detected — skipping global UDP/443 block (browser rules only)"
            );
        } else {
            match add_block_udp443_global(&format!("{RULE_PREFIX}-global")) {
                Ok(()) => {
                    any = true;
                    tracing::info!("block_quic: global outbound UDP/443 blocked");
                }
                Err(err) => {
                    tracing::debug!(error = %err, "block_quic: global rule failed");
                    last_err = Some(err);
                }
            }
        }
        for (host, port) in allow_udp {
            tracing::debug!(%host, %port, "block_quic: udp tunnel allowlisted (not blocked)");
        }
        if !any {
            if let Some(err) = last_err {
                return Err(err).context("no block_quic firewall rules installed");
            }
            return Ok(false);
        }
        Ok(true)
    }
    #[cfg(not(windows))]
    {
        let _ = allow_udp;
        tracing::debug!("block_quic: unsupported on this platform (no-op)");
        Ok(false)
    }
}

fn remove_all_rules() -> Result<()> {
    #[cfg(windows)]
    {
        // netsh cannot glob-delete; enumerate via PowerShell then delete by name.
        let list = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "Get-NetFirewallRule -ErrorAction SilentlyContinue | Where-Object {{ $_.DisplayName -like '{RULE_PREFIX}*' }} | ForEach-Object {{ $_.DisplayName }}"
                ),
            ])
            .output()
            .context("list firewall rules")?;
        let text = String::from_utf8_lossy(&list.stdout);
        for name in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
            let _ = Command::new("netsh")
                .args(["advfirewall", "firewall", "delete", "rule", &format!("name={name}")])
                .output();
        }
        // Also try fixed names in case Get-NetFirewallRule unavailable.
        for suffix in ["global", "browser-0", "browser-1", "browser-2", "browser-3", "browser-4", "browser-5", "browser-6", "browser-7"] {
            let _ = Command::new("netsh")
                .args([
                    "advfirewall",
                    "firewall",
                    "delete",
                    "rule",
                    &format!("name={RULE_PREFIX}-{suffix}"),
                ])
                .output();
        }
        Ok(())
    }
    #[cfg(not(windows))]
    Ok(())
}

#[cfg(windows)]
fn add_block_udp443_global(name: &str) -> Result<()> {
    let out = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={name}"),
            "dir=out",
            "action=block",
            "protocol=UDP",
            "remoteport=443",
            "enable=yes",
            "profile=any",
        ])
        .output()
        .context("netsh add global block")?;
    if !out.status.success() {
        anyhow::bail!(
            "netsh add failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn add_block_udp443_program(name: &str, program: &str) -> Result<()> {
    let out = Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={name}"),
            "dir=out",
            "action=block",
            "protocol=UDP",
            "remoteport=443",
            &format!("program={program}"),
            "enable=yes",
            "profile=any",
        ])
        .output()
        .context("netsh add browser block")?;
    if !out.status.success() {
        anyhow::bail!(
            "netsh add failed for {program}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(windows)]
fn browser_programs() -> Vec<String> {
    use std::path::PathBuf;
    let mut paths = Vec::new();
    let mut push_if = |p: PathBuf| {
        if p.is_file() {
            paths.push(p.to_string_lossy().to_string());
        }
    };
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from);
    let pf86 = std::env::var_os("ProgramFiles(x86)").map(PathBuf::from);
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    if let Some(ref base) = pf {
        push_if(base.join(r"Google\Chrome\Application\chrome.exe"));
        push_if(base.join(r"Microsoft\Edge\Application\msedge.exe"));
        push_if(base.join(r"Mozilla Firefox\firefox.exe"));
        push_if(base.join(r"BraveSoftware\Brave-Browser\Application\brave.exe"));
        push_if(base.join(r"Vivaldi\Application\vivaldi.exe"));
    }
    if let Some(ref base) = pf86 {
        push_if(base.join(r"Google\Chrome\Application\chrome.exe"));
        push_if(base.join(r"Microsoft\Edge\Application\msedge.exe"));
        push_if(base.join(r"Mozilla Firefox\firefox.exe"));
    }
    if let Some(ref base) = local {
        push_if(base.join(r"Google\Chrome\Application\chrome.exe"));
        push_if(base.join(r"Microsoft\Edge\Application\msedge.exe"));
        push_if(base.join(r"Programs\Opera\opera.exe"));
        push_if(base.join(r"Thorium\Application\thorium.exe"));
    }
    paths.sort();
    paths.dedup();
    paths
}

/// Resolve hostnames in allowlist for logging / future WFP use.
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
