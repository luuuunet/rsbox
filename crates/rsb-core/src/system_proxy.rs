// 系统代理设置实现
use anyhow::Result;

pub struct SystemProxy;

impl SystemProxy {
    /// 启用系统代理
    pub fn enable(http_port: u16, socks_port: u16) -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            Self::enable_windows(http_port)?;
        }

        #[cfg(target_os = "macos")]
        {
            Self::enable_macos(http_port, socks_port)?;
        }

        #[cfg(target_os = "linux")]
        {
            Self::enable_linux(http_port)?;
        }

        tracing::info!(
            http_port = http_port,
            socks_port = socks_port,
            "System proxy enabled"
        );

        Ok(())
    }

    /// 禁用系统代理
    pub fn disable() -> Result<()> {
        #[cfg(target_os = "windows")]
        {
            Self::disable_windows()?;
        }

        #[cfg(target_os = "macos")]
        {
            Self::disable_macos()?;
        }

        #[cfg(target_os = "linux")]
        {
            Self::disable_linux()?;
        }

        tracing::info!("System proxy disabled");

        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn enable_windows(port: u16) -> Result<()> {
        use winreg::RegKey;
        use winreg::enums::*;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let settings = hkcu.open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            KEY_WRITE,
        )?;

        let proxy_addr = format!("127.0.0.1:{}", port);
        settings.set_value("ProxyEnable", &1u32)?;
        settings.set_value("ProxyServer", &proxy_addr)?;
        settings.set_value("ProxyOverride", &"localhost;127.*;10.*;172.16.*;172.31.*;192.168.*")?;

        // 通知系统代理设置已更改
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                HWND_BROADCAST, WM_SETTINGCHANGE,
            };
            use windows_sys::Win32::Foundation::LPARAM;

            windows_sys::Win32::UI::WindowsAndMessaging::SendNotifyMessageW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                0 as LPARAM,
            );
        }

        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn disable_windows() -> Result<()> {
        use winreg::RegKey;
        use winreg::enums::*;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let settings = hkcu.open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            KEY_WRITE,
        )?;

        settings.set_value("ProxyEnable", &0u32)?;

        // 通知系统
        unsafe {
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                HWND_BROADCAST, WM_SETTINGCHANGE,
            };
            use windows_sys::Win32::Foundation::LPARAM;

            windows_sys::Win32::UI::WindowsAndMessaging::SendNotifyMessageW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                0 as LPARAM,
            );
        }

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn enable_macos(http_port: u16, socks_port: u16) -> Result<()> {
        use std::process::Command;

        // 获取所有网络服务
        let output = Command::new("networksetup")
            .args(&["-listallnetworkservices"])
            .output()?;

        let services = String::from_utf8(output.stdout)?;

        for service in services.lines().skip(1) {
            let service = service.trim();
            if service.starts_with('*') || service.is_empty() {
                continue;
            }

            // 设置 HTTP 代理
            Command::new("networksetup")
                .args(&[
                    "-setwebproxy",
                    service,
                    "127.0.0.1",
                    &http_port.to_string(),
                ])
                .output()?;

            // 设置 HTTPS 代理
            Command::new("networksetup")
                .args(&[
                    "-setsecurewebproxy",
                    service,
                    "127.0.0.1",
                    &http_port.to_string(),
                ])
                .output()?;

            // 设置 SOCKS 代理
            Command::new("networksetup")
                .args(&[
                    "-setsocksfirewallproxy",
                    service,
                    "127.0.0.1",
                    &socks_port.to_string(),
                ])
                .output()?;

            // 设置绕过列表
            Command::new("networksetup")
                .args(&[
                    "-setproxybypassdomains",
                    service,
                    "localhost",
                    "127.0.0.1",
                    "*.local",
                ])
                .output()?;

            tracing::debug!(service = %service, "Configured proxy for network service");
        }

        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn disable_macos() -> Result<()> {
        use std::process::Command;

        let output = Command::new("networksetup")
            .args(&["-listallnetworkservices"])
            .output()?;

        let services = String::from_utf8(output.stdout)?;

        for service in services.lines().skip(1) {
            let service = service.trim();
            if service.starts_with('*') || service.is_empty() {
                continue;
            }

            Command::new("networksetup")
                .args(&["-setwebproxystate", service, "off"])
                .output()?;

            Command::new("networksetup")
                .args(&["-setsecurewebproxystate", service, "off"])
                .output()?;

            Command::new("networksetup")
                .args(&["-setsocksfirewallproxystate", service, "off"])
                .output()?;

            tracing::debug!(service = %service, "Disabled proxy for network service");
        }

        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn enable_linux(port: u16) -> Result<()> {
        // Linux 通过环境变量设置代理
        // 注意：这只对当前进程和子进程有效

        let http_proxy = format!("http://127.0.0.1:{}", port);
        std::env::set_var("http_proxy", &http_proxy);
        std::env::set_var("https_proxy", &http_proxy);
        std::env::set_var("HTTP_PROXY", &http_proxy);
        std::env::set_var("HTTPS_PROXY", &http_proxy);

        std::env::set_var("no_proxy", "localhost,127.0.0.1,::1");
        std::env::set_var("NO_PROXY", "localhost,127.0.0.1,::1");

        // 对于 GNOME 桌面环境
        if let Ok(_) = std::process::Command::new("gsettings")
            .args(&["set", "org.gnome.system.proxy", "mode", "manual"])
            .output()
        {
            std::process::Command::new("gsettings")
                .args(&[
                    "set",
                    "org.gnome.system.proxy.http",
                    "host",
                    "127.0.0.1",
                ])
                .output()?;

            std::process::Command::new("gsettings")
                .args(&[
                    "set",
                    "org.gnome.system.proxy.http",
                    "port",
                    &port.to_string(),
                ])
                .output()?;

            std::process::Command::new("gsettings")
                .args(&[
                    "set",
                    "org.gnome.system.proxy.https",
                    "host",
                    "127.0.0.1",
                ])
                .output()?;

            std::process::Command::new("gsettings")
                .args(&[
                    "set",
                    "org.gnome.system.proxy.https",
                    "port",
                    &port.to_string(),
                ])
                .output()?;
        }

        Ok(())
    }

    #[cfg(target_os = "linux")]
    fn disable_linux() -> Result<()> {
        std::env::remove_var("http_proxy");
        std::env::remove_var("https_proxy");
        std::env::remove_var("HTTP_PROXY");
        std::env::remove_var("HTTPS_PROXY");
        std::env::remove_var("no_proxy");
        std::env::remove_var("NO_PROXY");

        // GNOME
        if let Ok(_) = std::process::Command::new("gsettings")
            .args(&["set", "org.gnome.system.proxy", "mode", "none"])
            .output()
        {
            // 成功
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // 需要管理员权限
    fn test_system_proxy() {
        SystemProxy::enable(7890, 7891).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
        SystemProxy::disable().unwrap();
    }
}
