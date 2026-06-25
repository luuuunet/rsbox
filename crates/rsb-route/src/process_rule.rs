// 进程路由规则实现
use anyhow::Result;
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ProcessRule {
    pub process_names: Vec<String>,
    pub process_paths: Vec<PathBuf>,
    pub outbound: String,
}

impl ProcessRule {
    pub fn new(outbound: String) -> Self {
        Self {
            process_names: Vec::new(),
            process_paths: Vec::new(),
            outbound,
        }
    }

    pub async fn match_process(&self, local_addr: &SocketAddr) -> bool {
        if let Some(info) = get_process_info(local_addr).await {
            // 检查进程名
            if !self.process_names.is_empty() {
                if self.process_names.iter().any(|name| info.name.contains(name)) {
                    return true;
                }
            }

            // 检查进程路径
            if !self.process_paths.is_empty() {
                if self.process_paths.iter().any(|path| info.path.starts_with(path)) {
                    return true;
                }
            }
        }

        false
    }
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub path: PathBuf,
}

#[cfg(target_os = "linux")]
async fn get_process_info(local_addr: &SocketAddr) -> Option<ProcessInfo> {
    use std::fs;

    // 1. 从 /proc/net/tcp 或 /proc/net/tcp6 找到 inode
    let inode = find_inode_from_proc_net(local_addr)?;

    // 2. 遍历 /proc/*/fd/* 找到匹配的 socket
    let pid = find_pid_by_inode(inode)?;

    // 3. 读取进程信息
    let name = fs::read_to_string(format!("/proc/{}/comm", pid))
        .ok()?
        .trim()
        .to_string();

    let path = fs::read_link(format!("/proc/{}/exe", pid))
        .ok()?;

    Some(ProcessInfo { pid, name, path })
}

#[cfg(target_os = "linux")]
fn find_inode_from_proc_net(addr: &SocketAddr) -> Option<u64> {
    use std::fs;

    let file_path = if addr.is_ipv4() {
        "/proc/net/tcp"
    } else {
        "/proc/net/tcp6"
    };

    let content = fs::read_to_string(file_path).ok()?;

    // 转换地址为 /proc/net/tcp 格式
    let local_hex = format_addr_as_proc_net(addr);

    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 10 {
            continue;
        }

        let local_address = parts[1];
        if local_address == local_hex {
            // parts[9] 是 inode
            return parts[9].parse().ok();
        }
    }

    None
}

#[cfg(target_os = "linux")]
fn format_addr_as_proc_net(addr: &SocketAddr) -> String {
    match addr {
        SocketAddr::V4(v4) => {
            let ip = v4.ip().octets();
            let port = v4.port();
            format!(
                "{:02X}{:02X}{:02X}{:02X}:{:04X}",
                ip[0], ip[1], ip[2], ip[3], port
            )
        }
        SocketAddr::V6(v6) => {
            let ip = v6.ip().octets();
            let port = v6.port();
            let ip_hex: String = ip.iter().map(|b| format!("{:02X}", b)).collect();
            format!("{}:{:04X}", ip_hex, port)
        }
    }
}

#[cfg(target_os = "linux")]
fn find_pid_by_inode(inode: u64) -> Option<u32> {
    use std::fs;

    for entry in fs::read_dir("/proc").ok()? {
        let entry = entry.ok()?;
        let file_name = entry.file_name();
        let pid_str = file_name.to_str()?;

        // 只处理数字目录（PID）
        if !pid_str.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let fd_dir = format!("/proc/{}/fd", pid_str);
        if let Ok(fd_entries) = fs::read_dir(&fd_dir) {
            for fd_entry in fd_entries.flatten() {
                if let Ok(link) = fs::read_link(fd_entry.path()) {
                    if let Some(link_str) = link.to_str() {
                        if link_str.contains(&format!("socket:[{}]", inode)) {
                            return pid_str.parse().ok();
                        }
                    }
                }
            }
        }
    }

    None
}

#[cfg(target_os = "macos")]
async fn get_process_info(local_addr: &SocketAddr) -> Option<ProcessInfo> {
    // macOS 使用 lsof 或 proc_pidinfo
    use std::process::Command;

    let output = Command::new("lsof")
        .args(&[
            "-nP",
            "-iTCP",
            &format!("@{}:{}", local_addr.ip(), local_addr.port()),
        ])
        .output()
        .ok()?;

    let stdout = String::from_utf8(output.stdout).ok()?;

    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }

        let name = parts[0].to_string();
        let pid: u32 = parts[1].parse().ok()?;

        // 获取完整路径
        let path_output = Command::new("ps")
            .args(&["-p", &pid.to_string(), "-o", "comm="])
            .output()
            .ok()?;

        let path = String::from_utf8(path_output.stdout)
            .ok()?
            .trim()
            .into();

        return Some(ProcessInfo { pid, name, path });
    }

    None
}

#[cfg(target_os = "windows")]
async fn get_process_info(local_addr: &SocketAddr) -> Option<ProcessInfo> {
    // Windows 使用 GetExtendedTcpTable
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID,
        TCP_TABLE_OWNER_PID_ALL,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;

    unsafe {
        let mut size: u32 = 0;

        // 获取需要的大小
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );

        if size == 0 {
            return None;
        }

        let mut buffer = vec![0u8; size as usize];
        let result = GetExtendedTcpTable(
            buffer.as_mut_ptr() as *mut _,
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );

        if result != 0 {
            return None;
        }

        let table = &*(buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID);

        // 转换地址
        let local_port = local_addr.port();
        let local_ip = match local_addr.ip() {
            std::net::IpAddr::V4(v4) => u32::from_ne_bytes(v4.octets()),
            _ => return None,
        };

        // 查找匹配的连接
        for i in 0..table.dwNumEntries {
            let row = &*((table.table.as_ptr() as *const MIB_TCPROW_OWNER_PID).add(i as usize));

            if row.dwLocalAddr == local_ip && row.dwLocalPort as u16 == local_port.to_be() {
                let pid = row.dwOwningPid;

                // 获取进程信息
                use std::process::Command;

                let output = Command::new("tasklist")
                    .args(&["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
                    .output()
                    .ok()?;

                let stdout = String::from_utf8(output.stdout).ok()?;
                let parts: Vec<&str> = stdout.split(',').collect();

                if parts.len() >= 2 {
                    let name = parts[0].trim_matches('"').to_string();

                    // 获取完整路径
                    let path_output = Command::new("wmic")
                        .args(&[
                            "process",
                            "where",
                            &format!("ProcessId={}", pid),
                            "get",
                            "ExecutablePath",
                            "/VALUE",
                        ])
                        .output()
                        .ok()?;

                    let path_str = String::from_utf8(path_output.stdout)
                        .ok()?
                        .lines()
                        .find(|l| l.starts_with("ExecutablePath="))?
                        .strip_prefix("ExecutablePath=")?
                        .trim()
                        .to_string();

                    return Some(ProcessInfo {
                        pid,
                        name,
                        path: PathBuf::from(path_str),
                    });
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore] // 需要权限
    async fn test_get_process_info() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        if let Some(info) = get_process_info(&addr).await {
            println!("Process: {} ({})", info.name, info.pid);
            println!("Path: {:?}", info.path);
        }
    }
}
