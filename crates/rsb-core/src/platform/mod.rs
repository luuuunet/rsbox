//! OS integration without external shell commands.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub use route::install_routes;

mod route {
    use anyhow::Result;
    use serde_json::Value;

    pub fn install_routes(raw: &Value) -> Result<()> {
        let iface = raw
            .get("interface_name")
            .and_then(|v| v.as_str())
            .unwrap_or("wg0");
        let Some(peers) = raw.get("peers").and_then(|v| v.as_array()) else {
            return Ok(());
        };
        for peer in peers {
            let Some(list) = peer.get("allowed_ips").and_then(|v| v.as_array()) else {
                continue;
            };
            for cidr in list {
                let Some(c) = cidr.as_str() else {
                    continue;
                };
                if let Err(err) = route_add(c, iface) {
                    tracing::debug!(%c, %iface, error = %err, "route install skipped");
                }
            }
        }
        Ok(())
    }

    pub fn route_add(cidr: &str, iface: &str) -> Result<()> {
        #[cfg(target_os = "linux")]
        return super::linux::route_add(cidr, iface);
        #[cfg(windows)]
        return super::windows::route_add(cidr, iface);
        #[cfg(target_os = "macos")]
        return super::macos::route_add(cidr, iface);
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        {
            let _ = (cidr, iface);
            Ok(())
        }
    }

    pub fn route_delete(cidr: &str, iface: &str) -> Result<()> {
        #[cfg(target_os = "linux")]
        return super::linux::route_delete(cidr, iface);
        #[cfg(windows)]
        return super::windows::route_delete(cidr, iface);
        #[cfg(target_os = "macos")]
        return super::macos::route_delete(cidr, iface);
        #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
        {
            let _ = (cidr, iface);
            Ok(())
        }
    }
}

pub use route::{route_add, route_delete};

pub fn detect_default_interface() -> anyhow::Result<String> {
    #[cfg(target_os = "linux")]
    return linux::detect_default_interface();
    #[cfg(windows)]
    return windows::detect_default_interface();
    #[cfg(target_os = "macos")]
    return macos::detect_default_interface();
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    anyhow::bail!("auto_detect_interface unsupported on this platform")
}

#[cfg(target_os = "macos")]
pub fn lookup_process_for_tcp_tuple(
    local: std::net::SocketAddr,
    remote: std::net::SocketAddr,
) -> crate::ProcessInfo {
    macos::lookup_process_for_tcp_tuple(local, remote)
}
