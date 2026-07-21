use anyhow::{Context, Result};
use std::ffi::OsString;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::os::windows::ffi::OsStringExt;
use windows_sys::Win32::NetworkManagement::IpHelper::{
    ConvertInterfaceLuidToNameW, CreateIpForwardEntry2, DeleteIpForwardEntry2, FreeMibTable,
    GetAdaptersAddresses, GetIpForwardTable2, InitializeIpForwardEntry, GAA_FLAG_SKIP_ANYCAST,
    GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, MIB_IPFORWARD_ROW2, MIB_IPFORWARD_TABLE2,
};
use windows_sys::Win32::Networking::WinSock::{AF_INET, AF_INET6, AF_UNSPEC};

pub fn detect_default_interface() -> Result<String> {
    unsafe {
        let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
        if GetIpForwardTable2(AF_UNSPEC, &mut table) != 0 {
            anyhow::bail!("GetIpForwardTable2 failed");
        }
        let result = (|| {
            let rows =
                std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
            let mut best: Option<&MIB_IPFORWARD_ROW2> = None;
            for row in rows {
                if row.DestinationPrefix.Prefix.si_family != AF_INET {
                    continue;
                }
                let dst = row.DestinationPrefix.Prefix.Ipv4.sin_addr.S_un.S_addr;
                let prefix = row.DestinationPrefix.PrefixLength;
                if dst == 0 && prefix == 0 {
                    best = Some(match best {
                        None => row,
                        Some(prev) if row.Metric < prev.Metric => row,
                        Some(prev) => prev,
                    });
                }
            }
            let row = best.context("default route not found")?;
            let mut name = [0u16; 256];
            if ConvertInterfaceLuidToNameW(&row.InterfaceLuid, name.as_mut_ptr(), name.len()) != 0 {
                anyhow::bail!("ConvertInterfaceLuidToNameW failed");
            }
            let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
            Ok(OsString::from_wide(&name[..len])
                .to_string_lossy()
                .into_owned())
        })();
        FreeMibTable(table as _);
        result
    }
}

pub fn route_add(cidr: &str, iface: &str) -> Result<()> {
    let row = build_forward_row(cidr, iface)?;
    unsafe {
        let err = CreateIpForwardEntry2(&row);
        if err != 0 && err != windows_sys::Win32::Foundation::ERROR_OBJECT_ALREADY_EXISTS {
            anyhow::bail!("CreateIpForwardEntry2 failed for {cidr} (error {err})");
        }
    }
    Ok(())
}

pub fn route_delete(cidr: &str, iface: &str) -> Result<()> {
    let row = build_forward_row(cidr, iface)?;
    unsafe {
        let err = DeleteIpForwardEntry2(&row);
        if err != 0 && err != windows_sys::Win32::Foundation::ERROR_NOT_FOUND {
            anyhow::bail!("DeleteIpForwardEntry2 failed for {cidr} (error {err})");
        }
    }
    Ok(())
}

fn build_forward_row(cidr: &str, iface: &str) -> Result<MIB_IPFORWARD_ROW2> {
    let (dest, prefix) = parse_cidr(cidr)?;
    let ifindex = interface_index(iface)?;
    unsafe {
        let mut row: MIB_IPFORWARD_ROW2 = std::mem::zeroed();
        InitializeIpForwardEntry(&mut row);
        row.InterfaceIndex = ifindex;
        row.DestinationPrefix.PrefixLength = prefix;
        row.Metric = 256;
        match dest {
            IpAddr::V4(v4) => {
                row.DestinationPrefix.Prefix.si_family = AF_INET;
                row.DestinationPrefix.Prefix.Ipv4.sin_family = AF_INET;
                row.DestinationPrefix.Prefix.Ipv4.sin_addr.S_un.S_addr =
                    u32::from_ne_bytes(v4.octets());
            },
            IpAddr::V6(v6) => {
                row.DestinationPrefix.Prefix.si_family = AF_INET6;
                row.DestinationPrefix.Prefix.Ipv6.sin6_family = AF_INET6;
                row.DestinationPrefix.Prefix.Ipv6.sin6_addr = in6_addr(v6);
            },
        }
        Ok(row)
    }
}

fn interface_index(name: &str) -> Result<u32> {
    unsafe {
        let mut size: u32 = 0;
        let flags = GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER;
        let err = GetAdaptersAddresses(
            AF_UNSPEC as u32,
            flags,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        );
        if err != windows_sys::Win32::Foundation::ERROR_BUFFER_OVERFLOW {
            anyhow::bail!("GetAdaptersAddresses size query failed");
        }
        let mut buf = vec![0u8; size as usize];
        if GetAdaptersAddresses(
            AF_UNSPEC as u32,
            flags,
            std::ptr::null_mut(),
            buf.as_mut_ptr() as _,
            &mut size,
        ) != 0
        {
            anyhow::bail!("GetAdaptersAddresses failed");
        }
        let mut cur = buf.as_ptr()
            as *const windows_sys::Win32::NetworkManagement::IpHelper::IP_ADAPTER_ADDRESSES_LH;
        while !cur.is_null() {
            let friendly_wide =
                std::slice::from_raw_parts((*cur).FriendlyName, wide_len((*cur).FriendlyName));
            let friendly_os = OsString::from_wide(friendly_wide);
            let friendly = friendly_os.to_string_lossy();
            if friendly.eq_ignore_ascii_case(name) {
                return Ok((*cur).Anonymous1.Anonymous.IfIndex);
            }
            cur = (*cur).Next;
        }
        anyhow::bail!("interface `{name}` not found")
    }
}

fn wide_len(ptr: *const u16) -> usize {
    unsafe {
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
            if len > 512 {
                break;
            }
        }
        len
    }
}

fn parse_cidr(cidr: &str) -> Result<(IpAddr, u8)> {
    if let Some((ip, prefix)) = cidr.split_once('/') {
        return Ok((
            ip.parse().context("cidr ip")?,
            prefix.parse().context("cidr prefix")?,
        ));
    }
    Ok((cidr.parse().context("cidr ip")?, 32))
}

fn in6_addr(v6: Ipv6Addr) -> windows_sys::Win32::Networking::WinSock::IN6_ADDR {
    windows_sys::Win32::Networking::WinSock::IN6_ADDR {
        u: windows_sys::Win32::Networking::WinSock::IN6_ADDR_0 { Byte: v6.octets() },
    }
}

#[allow(dead_code)]
fn _unused() {
    let _ = Ipv4Addr::LOCALHOST;
}
