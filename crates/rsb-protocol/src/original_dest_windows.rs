//! Windows original destination via GetExtendedTcpTable (client-side row lookup).

use anyhow::{Context, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio::net::TcpStream;

const MIB_TCP_STATE_ESTAB: u32 = 5;
const MIB_TCP_STATE_SYN_RCVD: u32 = 3;

pub async fn get_original_destination(stream: &TcpStream) -> Result<SocketAddr> {
    let peer = stream.peer_addr()?;
    match peer.ip() {
        IpAddr::V4(v4) => tcp_table_lookup_v4(v4, peer.port()),
        IpAddr::V6(v6) => tcp_table_lookup_v6(v6, peer.port()),
    }
    .context("windows GetExtendedTcpTable original dest")
}

fn tcp_table_lookup_v4(peer_ip: Ipv4Addr, peer_port: u16) -> Result<SocketAddr> {
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
        anyhow::bail!("GetExtendedTcpTable failed: {status}");
    }
    let table = unsafe { &*(buf.as_ptr() as *const MibTcpTableOwnerPid) };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dw_num_entries as usize) };
    for row in rows {
        if row.dw_state != MIB_TCP_STATE_ESTAB && row.dw_state != MIB_TCP_STATE_SYN_RCVD {
            continue;
        }
        let local_ip = Ipv4Addr::from(u32::from_be(row.dw_local_addr));
        let local_port = ntohs(row.dw_local_port);
        let remote_ip = Ipv4Addr::from(u32::from_be(row.dw_remote_addr));
        let remote_port = ntohs(row.dw_remote_port);
        if local_ip == peer_ip && local_port == peer_port {
            return Ok(SocketAddr::new(IpAddr::V4(remote_ip), remote_port));
        }
    }
    anyhow::bail!("no TCP row for client {peer_ip}:{peer_port}")
}

fn tcp_table_lookup_v6(peer_ip: Ipv6Addr, peer_port: u16) -> Result<SocketAddr> {
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
        anyhow::bail!("GetExtendedTcpTable v6 failed: {status}");
    }
    let table = unsafe { &*(buf.as_ptr() as *const MibTcp6TableOwnerPid) };
    let rows =
        unsafe { std::slice::from_raw_parts(table.table.as_ptr(), table.dw_num_entries as usize) };
    for row in rows {
        if row.dw_state != MIB_TCP_STATE_ESTAB && row.dw_state != MIB_TCP_STATE_SYN_RCVD {
            continue;
        }
        let mut local = [0u8; 16];
        local.copy_from_slice(&row.local_addr);
        let local_port = ntohs(row.dw_local_port);
        let mut remote = [0u8; 16];
        remote.copy_from_slice(&row.remote_addr);
        let remote_port = ntohs(row.dw_remote_port);
        if Ipv6Addr::from(local) == peer_ip && local_port == peer_port {
            return Ok(SocketAddr::new(
                IpAddr::V6(Ipv6Addr::from(remote)),
                remote_port,
            ));
        }
    }
    anyhow::bail!("no TCP6 row for client [{peer_ip}]:{peer_port}")
}

#[repr(C)]
struct MibTcpTableOwnerPid {
    dw_num_entries: u32,
    table: [MibTcpRowOwnerPid; 1],
}

#[repr(C)]
struct MibTcpRowOwnerPid {
    dw_state: u32,
    dw_local_addr: u32,
    dw_local_port: u32,
    dw_remote_addr: u32,
    dw_remote_port: u32,
    dw_owning_pid: u32,
}

#[repr(C)]
struct MibTcp6TableOwnerPid {
    dw_num_entries: u32,
    table: [MibTcp6RowOwnerPid; 1],
}

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

fn ntohs(v: u32) -> u16 {
    (v >> 8) as u16 | ((v & 0xff) as u16) << 8
}
