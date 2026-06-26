//! Process lookup for TCP connections (proxy accept socket or TUN tuple).

use std::net::SocketAddr;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessInfo {
    pub name: Option<String>,
    pub path: Option<String>,
}

pub fn lookup_process_for_tcp_stream(stream: &tokio::net::TcpStream) -> ProcessInfo {
    let Ok(local) = stream.local_addr() else {
        return ProcessInfo::default();
    };
    let remote = stream.peer_addr().ok();
    if let Some(remote) = remote {
        lookup_process_for_tuple(local, remote)
    } else {
        lookup_local_socket(local)
    }
}

pub fn lookup_process_for_tuple(local: SocketAddr, remote: SocketAddr) -> ProcessInfo {
    #[cfg(target_os = "linux")]
    {
        if let Some(inode) = find_tcp_inode("/proc/net/tcp", local, remote)
            .or_else(|| find_tcp_inode("/proc/net/tcp6", local, remote))
        {
            return find_process_by_inode(inode).unwrap_or_default();
        }
    }
    #[cfg(windows)]
    {
        let pid = match (local.ip(), remote.ip()) {
            (IpAddr::V4(l), IpAddr::V4(r)) => tcp_owner_pid_v4(l, local.port(), r, remote.port()),
            (IpAddr::V6(l), IpAddr::V6(r)) => tcp_owner_pid_v6(l, local.port(), r, remote.port()),
            _ => None,
        };
        if let Some(pid) = pid {
            return read_process_info_windows(pid);
        }
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (local, remote);
        #[cfg(target_os = "macos")]
        {
            return crate::platform::lookup_process_for_tcp_tuple(local, remote);
        }
    }
    ProcessInfo::default()
}

#[cfg(windows)]
fn lookup_local_socket(local: SocketAddr) -> ProcessInfo {
    let pid = match local.ip() {
        IpAddr::V4(v4) => tcp_owner_pid_v4(v4, local.port(), Ipv4Addr::UNSPECIFIED, 0),
        IpAddr::V6(v6) => tcp_owner_pid_v6(v6, local.port(), Ipv6Addr::UNSPECIFIED, 0),
    };
    pid.map(read_process_info_windows).unwrap_or_default()
}

#[cfg(not(windows))]
fn lookup_local_socket(_local: SocketAddr) -> ProcessInfo {
    ProcessInfo::default()
}

#[cfg(target_os = "linux")]
fn find_tcp_inode(path: &str, local: SocketAddr, remote: SocketAddr) -> Option<u64> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines().skip(1) {
        let cols: Vec<_> = line.split_whitespace().collect();
        if cols.len() < 10 {
            continue;
        }
        let Some(loc) = decode_proc_addr(cols[1]) else {
            continue;
        };
        let Some(rem) = decode_proc_addr(cols[2]) else {
            continue;
        };
        if loc == local && rem == remote {
            return cols[9].parse().ok();
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn decode_proc_addr(raw: &str) -> Option<SocketAddr> {
    let (ip_hex, port_hex) = raw.split_once(':')?;
    let port = u16::from_str_radix(port_hex, 16).ok()?;
    match ip_hex.len() {
        8 => {
            let n = u32::from_str_radix(ip_hex, 16).ok()?;
            Some(SocketAddr::from((Ipv4Addr::from(n.to_le_bytes()), port)))
        },
        32 => {
            let mut o = [0u8; 16];
            for (i, chunk) in ip_hex.as_bytes().chunks(2).enumerate().take(16) {
                let s = std::str::from_utf8(chunk).ok()?;
                o[i] = u8::from_str_radix(s, 16).ok()?;
            }
            Some(SocketAddr::from((Ipv6Addr::from(o), port)))
        },
        _ => None,
    }
}

#[cfg(target_os = "linux")]
fn find_process_by_inode(inode: u64) -> Option<ProcessInfo> {
    let proc = std::fs::read_dir("/proc").ok()?;
    for entry in proc.flatten() {
        let name = entry.file_name();
        let pid = name.to_string_lossy().parse::<u32>().ok()?;
        let fd_dir = entry.path().join("fd");
        let Ok(fds) = std::fs::read_dir(fd_dir) else {
            continue;
        };
        for fd in fds.flatten() {
            let Ok(target) = std::fs::read_link(fd.path()) else {
                continue;
            };
            if target.to_string_lossy() == format!("socket:[{inode}]") {
                return Some(read_process_info_linux(pid));
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn read_process_info_linux(pid: u32) -> ProcessInfo {
    let comm = std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let path = std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned());
    ProcessInfo { name: comm, path }
}

#[cfg(windows)]
fn tcp_owner_pid_v4(
    local_ip: Ipv4Addr,
    local_port: u16,
    remote_ip: Ipv4Addr,
    remote_port: u16,
) -> Option<u32> {
    with_tcp_table_v4(|rows| {
        for row in rows {
            let lip = Ipv4Addr::from(u32::from_be(row.dw_local_addr));
            let lport = ntohs(row.dw_local_port);
            let rip = Ipv4Addr::from(u32::from_be(row.dw_remote_addr));
            let rport = ntohs(row.dw_remote_port);
            if lip == local_ip
                && lport == local_port
                && (remote_port == 0 || (rip == remote_ip && rport == remote_port))
            {
                return Some(row.dw_owning_pid);
            }
        }
        None
    })
}

#[cfg(windows)]
fn tcp_owner_pid_v6(
    local_ip: Ipv6Addr,
    local_port: u16,
    remote_ip: Ipv6Addr,
    remote_port: u16,
) -> Option<u32> {
    with_tcp_table_v6(|rows| {
        for row in rows {
            let mut local = [0u8; 16];
            local.copy_from_slice(&row.local_addr);
            let lport = ntohs(row.dw_local_port);
            let mut remote = [0u8; 16];
            remote.copy_from_slice(&row.remote_addr);
            let rport = ntohs(row.dw_remote_port);
            if Ipv6Addr::from(local) == local_ip
                && lport == local_port
                && (remote_port == 0
                    || (Ipv6Addr::from(remote) == remote_ip && rport == remote_port))
            {
                return Some(row.dw_owning_pid);
            }
        }
        None
    })
}

#[cfg(windows)]
fn with_tcp_table_v4(f: impl FnOnce(&[MibTcpRowOwnerPid]) -> Option<u32>) -> Option<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET;

    let mut size: u32 = 0;
    unsafe {
        let _ = GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            false.into(),
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
    }
    let mut buf = vec![0u8; size as usize];
    let status = unsafe {
        GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut _,
            &mut size,
            false.into(),
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if status != 0 {
        return None;
    }
    let table = unsafe { &*(buf.as_ptr() as *const MibTcpTableOwnerPid) };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dw_num_entries as usize) };
    f(rows)
}

#[cfg(windows)]
fn with_tcp_table_v6(f: impl FnOnce(&[MibTcp6RowOwnerPid]) -> Option<u32>) -> Option<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, TCP_TABLE_OWNER_PID_ALL,
    };
    use windows_sys::Win32::Networking::WinSock::AF_INET6;

    let mut size: u32 = 0;
    unsafe {
        let _ = GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            false.into(),
            AF_INET6 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        );
    }
    let mut buf = vec![0u8; size as usize];
    let status = unsafe {
        GetExtendedTcpTable(
            buf.as_mut_ptr() as *mut _,
            &mut size,
            false.into(),
            AF_INET6 as u32,
            TCP_TABLE_OWNER_PID_ALL,
            0,
        )
    };
    if status != 0 {
        return None;
    }
    let table = unsafe { &*(buf.as_ptr() as *const MibTcp6TableOwnerPid) };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dw_num_entries as usize) };
    f(rows)
}

#[cfg(windows)]
fn read_process_info_windows(pid: u32) -> ProcessInfo {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return ProcessInfo::default();
    }
    let info = query_process_image(handle as *mut c_void);
    unsafe { CloseHandle(handle) };
    info.map(|path| {
        let name = std::path::Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string);
        ProcessInfo {
            name,
            path: Some(path),
        }
    })
    .unwrap_or_default()
}

#[cfg(windows)]
fn query_process_image(handle: *mut std::ffi::c_void) -> Option<String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Threading::{QueryFullProcessImageNameW, PROCESS_NAME_WIN32};

    let mut buf = vec![0u16; 32768];
    let mut size = buf.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut size)
    };
    if ok == 0 {
        return None;
    }
    Some(
        std::ffi::OsString::from_wide(&buf[..size as usize])
            .to_string_lossy()
            .into_owned(),
    )
}

#[cfg(windows)]
#[repr(C)]
struct MibTcpTableOwnerPid {
    dw_num_entries: u32,
    table: [MibTcpRowOwnerPid; 1],
}

#[cfg(windows)]
#[repr(C)]
struct MibTcpRowOwnerPid {
    dw_state: u32,
    dw_local_addr: u32,
    dw_local_port: u32,
    dw_remote_addr: u32,
    dw_remote_port: u32,
    dw_owning_pid: u32,
}

#[cfg(windows)]
#[repr(C)]
struct MibTcp6TableOwnerPid {
    dw_num_entries: u32,
    table: [MibTcp6RowOwnerPid; 1],
}

#[cfg(windows)]
#[repr(C)]
struct MibTcp6RowOwnerPid {
    local_addr: [u8; 16],
    dw_local_scope_id: u32,
    dw_local_port: u32,
    remote_addr: [u8; 16],
    dw_remote_scope_id: u32,
    dw_remote_port: u32,
    dw_state: u32,
    dw_owning_pid: u32,
}

#[cfg(windows)]
fn ntohs(v: u32) -> u16 {
    (v >> 8) as u16 | ((v & 0xff) as u16) << 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "linux")]
    fn decode_proc_ipv4_addr() {
        let addr = decode_proc_addr("0100007F:1F90").unwrap();
        assert_eq!(addr, SocketAddr::from(([127, 0, 0, 1], 8080)));
    }
}
