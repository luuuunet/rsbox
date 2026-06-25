use anyhow::{Context, Result};
use std::mem;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

const PROC_PIDLISTFDS: i32 = 1;
const PROC_PIDFDSOCKETINFO: i32 = 3;
const PROC_PIDFDSOCKETINFO_SIZE: i32 = 3;
const SOCKETINFO_TCP: i32 = 2;

#[repr(C)]
struct ProcFdInfo {
    proc_fd: i32,
    fd: i32,
    fdtype: u32,
}

#[repr(C)]
struct SocketInfo {
    si_stat: u64,
    si_type: i32,
    si_protocol: i32,
    si_family: i32,
    si_state: i32,
    si_pcb: [u8; 104],
}

#[repr(C)]
struct InSockInfo {
    insi_fport: u32,
    insi_lport: u32,
    insi_gencnt: u64,
    insi_flags: u32,
    insi_flow: u32,
    insi_vflag: u8,
    insi_ip_ttl: u8,
    insi_rflow: u8,
    insi_lflow: u8,
    insi_v4: InSockInfoV4,
    insi_v6: InSockInfoV6,
}

#[repr(C)]
struct InSockInfoV4 {
    insi_faddr: [u8; 4],
    insi_laddr: [u8; 4],
    insi_v4: [u8; 4],
}

#[repr(C)]
struct InSockInfoV6 {
    insi6_faddr: [u8; 16],
    insi6_laddr: [u8; 16],
    insi6_flow: u32,
    insi6_gencnt: u32,
    insi6_hops: u16,
}

#[repr(C)]
struct SocketFdInfo {
    socket_info: SocketInfo,
    tcp: TcpSocketInfo,
}

#[repr(C)]
struct TcpSocketInfo {
    tcpi_state: u32,
    tcpi_timer: u32,
    tcpi_flags: u32,
    tcpi_linger: u32,
    tcpi_backoff: u32,
    tcpi_options: u32,
    tcpi_snd_wscale: u8,
    tcpi_rcv_wscale: u8,
    rqi_band: u32,
    rqi_pid: u32,
    tcpi_state_time: u32,
    insi: InSockInfo,
}

pub fn detect_default_interface() -> Result<String> {
    let fd = unsafe { libc::socket(libc::AF_ROUTE, libc::SOCK_RAW, 0) };
    if fd < 0 {
        anyhow::bail!("routing socket failed");
    }
    let mut msg = RouteMsg::get_request();
    let ret = unsafe { libc::write(fd, msg.as_ptr() as *const _, msg.len()) };
    if ret <= 0 {
        unsafe { libc::close(fd) };
        anyhow::bail!("routing socket write failed");
    }
    let mut buf = [0u8; 4096];
    let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len()) };
    unsafe { libc::close(fd) };
    if n <= 0 {
        anyhow::bail!("routing socket read failed");
    }
    parse_route_ifname(&buf[..n as usize])
}

pub fn route_add(cidr: &str, iface: &str) -> Result<()> {
    let (dest, prefix) = parse_cidr(cidr)?;
    let ifindex = if_nametoindex(iface)?;
    let fd = unsafe { libc::socket(libc::AF_ROUTE, libc::SOCK_RAW, 0) };
    if fd < 0 {
        anyhow::bail!("routing socket failed");
    }
    let msg = RouteMsg::add_request(dest, prefix, ifindex);
    let ret = unsafe { libc::write(fd, msg.as_ptr() as *const _, msg.len()) };
    unsafe { libc::close(fd) };
    if ret <= 0 {
        anyhow::bail!("route add failed for {cidr}");
    }
    Ok(())
}

pub fn lookup_process_for_tcp_tuple(local: SocketAddr, remote: SocketAddr) -> crate::ProcessInfo {
    let mut pids = vec![0i32; 4096];
    let n = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr(),
            (pids.len() * mem::size_of::<i32>()) as i32,
        )
    };
    if n <= 0 {
        return crate::ProcessInfo::default();
    }
    let count = (n as usize / mem::size_of::<i32>()).min(pids.len());
    for pid in &pids[..count] {
        if *pid <= 0 {
            continue;
        }
        if let Some(info) = pid_tcp_match(*pid, local, remote) {
            return info;
        }
    }
    crate::ProcessInfo::default()
}

fn pid_tcp_match(pid: i32, local: SocketAddr, remote: SocketAddr) -> Option<crate::ProcessInfo> {
    let mut fds = vec![
        ProcFdInfo {
            proc_fd: 0,
            fd: 0,
            fdtype: 0
        };
        256
    ];
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr() as *mut _,
            (fds.len() * mem::size_of::<ProcFdInfo>()) as i32,
        )
    };
    if n <= 0 {
        return None;
    }
    let count = (n as usize / mem::size_of::<ProcFdInfo>()).min(fds.len());
    for fd in &fds[..count] {
        if fd.fdtype as i32 != SOCKETINFO_TCP {
            continue;
        }
        let mut info = SocketFdInfo {
            socket_info: unsafe { mem::zeroed() },
            tcp: unsafe { mem::zeroed() },
        };
        let got = unsafe {
            libc::proc_pidfdinfo(
                pid,
                fd.fd,
                PROC_PIDFDSOCKETINFO,
                &mut info as *mut _ as *mut _,
                mem::size_of::<SocketFdInfo>() as i32,
            )
        };
        if got <= 0 {
            continue;
        }
        let lport = u16::from_be((info.tcp.insi.insi_lport & 0xffff) as u16);
        let fport = u16::from_be((info.tcp.insi.insi_fport & 0xffff) as u16);
        let (laddr, raddr) = if info.tcp.insi.insi_vflag as i32 & 1 != 0 {
            (
                SocketAddr::from((Ipv6Addr::from(info.tcp.insi.insi_v6.insi6_laddr), lport)),
                SocketAddr::from((Ipv6Addr::from(info.tcp.insi.insi_v6.insi6_faddr), fport)),
            )
        } else {
            (
                SocketAddr::from((Ipv4Addr::from(info.tcp.insi.insi_v4.insi_laddr), lport)),
                SocketAddr::from((Ipv4Addr::from(info.tcp.insi.insi_v4.insi_faddr), fport)),
            )
        };
        if laddr == local && raddr == remote {
            return Some(read_process_info(pid));
        }
    }
    None
}

fn read_process_info(pid: i32) -> crate::ProcessInfo {
    let mut path_buf = [0u8; libc::MAXPATHLEN as usize];
    let n =
        unsafe { libc::proc_pidpath(pid, path_buf.as_mut_ptr() as *mut _, path_buf.len() as u32) };
    let path = if n > 0 {
        Some(
            std::ffi::CStr::from_bytes_with_nul(&path_buf[..=n as usize])
                .ok()
                .map(|c| c.to_string_lossy().into_owned())
                .unwrap_or_default(),
        )
    } else {
        None
    };
    let name = path.as_ref().and_then(|p| {
        std::path::Path::new(p)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
    });
    crate::ProcessInfo { name, path }
}

struct RouteMsg {
    bytes: Vec<u8>,
}

impl RouteMsg {
    fn get_request() -> Self {
        let mut bytes = vec![0u8; libc::RTM_HDRLEN + 32];
        let hdr = bytes.as_mut_ptr() as *mut libc::rt_msghdr;
        unsafe {
            (*hdr).rtm_msglen = bytes.len() as u16;
            (*hdr).rtm_version = libc::RTM_VERSION as u8;
            (*hdr).rtm_type = libc::RTM_GET as u8;
            (*hdr).rtm_addrs = libc::RTA_DST as i32;
            (*hdr).rtm_pid = libc::getpid();
            (*hdr).rtm_seq = 1;
        }
        append_sockaddr_in(&mut bytes, Ipv4Addr::new(0, 0, 0, 0));
        Self { bytes }
    }

    fn add_request(dest: IpAddr, prefix: u8, ifindex: u32) -> Self {
        let mut bytes = vec![0u8; libc::RTM_HDRLEN + 128];
        let hdr = bytes.as_mut_ptr() as *mut libc::rt_msghdr;
        unsafe {
            (*hdr).rtm_msglen = bytes.len() as u16;
            (*hdr).rtm_version = libc::RTM_VERSION as u8;
            (*hdr).rtm_type = libc::RTM_ADD as u8;
            (*hdr).rtm_flags = libc::RTF_UP | libc::RTF_STATIC;
            (*hdr).rtm_addrs = (libc::RTA_DST | libc::RTA_NETMASK | libc::RTA_IFP) as i32;
            (*hdr).rtm_pid = libc::getpid();
            (*hdr).rtm_seq = 1;
            (*hdr).rtm_index = ifindex as u16;
        }

        // Add destination
        match dest {
            IpAddr::V4(v4) => append_sockaddr_in(&mut bytes, v4),
            IpAddr::V6(v6) => append_sockaddr_in6(&mut bytes, v6),
        }

        // Add netmask (CIDR prefix)
        match dest {
            IpAddr::V4(_) => append_netmask_v4(&mut bytes, prefix),
            IpAddr::V6(_) => append_netmask_v6(&mut bytes, prefix),
        }

        // Add interface
        append_sockaddr_dl(&mut bytes, ifindex);

        Self { bytes }
    }

    fn as_ptr(&self) -> *const u8 {
        self.bytes.as_ptr()
    }

    fn len(&self) -> usize {
        self.bytes.len()
    }
}

fn append_sockaddr_in(buf: &mut Vec<u8>, ip: Ipv4Addr) {
    let mut sa: libc::sockaddr_in = unsafe { mem::zeroed() };
    sa.sin_len = mem::size_of::<libc::sockaddr_in>() as u8;
    sa.sin_family = libc::AF_INET as u8;
    sa.sin_port = 0;
    sa.sin_addr = libc::in_addr {
        s_addr: u32::from_ne_bytes(ip.octets()),
    };
    let bytes = unsafe {
        std::slice::from_raw_parts(
            &sa as *const _ as *const u8,
            mem::size_of::<libc::sockaddr_in>(),
        )
    };
    buf.extend_from_slice(bytes);
}

fn append_sockaddr_in6(buf: &mut Vec<u8>, ip: Ipv6Addr) {
    let mut sa: libc::sockaddr_in6 = unsafe { mem::zeroed() };
    sa.sin6_len = mem::size_of::<libc::sockaddr_in6>() as u8;
    sa.sin6_family = libc::AF_INET6 as u8;
    sa.sin6_port = 0;
    sa.sin6_addr = libc::in6_addr {
        s6_addr: ip.octets(),
    };
    let bytes = unsafe {
        std::slice::from_raw_parts(
            &sa as *const _ as *const u8,
            mem::size_of::<libc::sockaddr_in6>(),
        )
    };
    buf.extend_from_slice(bytes);
}

fn append_sockaddr_dl(buf: &mut Vec<u8>, ifindex: u32) {
    let mut sa: libc::sockaddr_dl = unsafe { mem::zeroed() };
    sa.sdl_len = mem::size_of::<libc::sockaddr_dl>() as u8;
    sa.sdl_family = libc::AF_LINK as u8;
    sa.sdl_index = ifindex as u16;
    let bytes = unsafe {
        std::slice::from_raw_parts(
            &sa as *const _ as *const u8,
            mem::size_of::<libc::sockaddr_dl>(),
        )
    };
    buf.extend_from_slice(bytes);
}

fn append_netmask_v4(buf: &mut Vec<u8>, prefix: u8) {
    let mut sa: libc::sockaddr_in = unsafe { mem::zeroed() };
    sa.sin_len = mem::size_of::<libc::sockaddr_in>() as u8;
    sa.sin_family = libc::AF_INET as u8;
    // Calculate netmask from prefix
    let mask = if prefix == 0 {
        0u32
    } else if prefix >= 32 {
        !0u32
    } else {
        !0u32 << (32 - prefix)
    };
    sa.sin_addr = libc::in_addr {
        s_addr: mask.to_be(),
    };
    let bytes = unsafe {
        std::slice::from_raw_parts(
            &sa as *const _ as *const u8,
            mem::size_of::<libc::sockaddr_in>(),
        )
    };
    buf.extend_from_slice(bytes);
}

fn append_netmask_v6(buf: &mut Vec<u8>, prefix: u8) {
    let mut sa: libc::sockaddr_in6 = unsafe { mem::zeroed() };
    sa.sin6_len = mem::size_of::<libc::sockaddr_in6>() as u8;
    sa.sin6_family = libc::AF_INET6 as u8;
    // Calculate IPv6 netmask from prefix
    let mut mask = [0u8; 16];
    let full_bytes = (prefix / 8) as usize;
    let remaining_bits = prefix % 8;
    for i in 0..full_bytes.min(16) {
        mask[i] = 0xff;
    }
    if full_bytes < 16 && remaining_bits > 0 {
        mask[full_bytes] = !0u8 << (8 - remaining_bits);
    }
    sa.sin6_addr = libc::in6_addr { s6_addr: mask };
    let bytes = unsafe {
        std::slice::from_raw_parts(
            &sa as *const _ as *const u8,
            mem::size_of::<libc::sockaddr_in6>(),
        )
    };
    buf.extend_from_slice(bytes);
}

fn parse_route_ifname(buf: &[u8]) -> Result<String> {
    if buf.len() < libc::RTM_HDRLEN {
        anyhow::bail!("short route message");
    }
    let hdr = buf.as_ptr() as *const libc::rt_msghdr;
    let addrs = unsafe { (*hdr).rtm_addrs };
    let mut offset = libc::RTM_HDRLEN;
    let mut ifname = None;
    for bit in 0..8 {
        if addrs & (1 << bit) == 0 {
            continue;
        }
        if offset + 2 > buf.len() {
            break;
        }
        let sa_len = buf[offset] as usize;
        if sa_len == 0 || offset + sa_len > buf.len() {
            break;
        }
        let family = buf[offset + 1];
        if family as i32 == libc::AF_LINK {
            let sa = &buf[offset..offset + sa_len];
            if let Some(name) = parse_sockaddr_dl_name(sa) {
                ifname = Some(name);
            }
        }
        offset += sa_len;
    }
    ifname.context("default interface not found in route message")
}

fn parse_sockaddr_dl_name(sa: &[u8]) -> Option<String> {
    if sa.len() < mem::size_of::<libc::sockaddr_dl>() {
        return None;
    }
    let sdl = sa.as_ptr() as *const libc::sockaddr_dl;
    unsafe {
        let nlen = (*sdl).sdl_nlen as usize;
        let data_offset = 8usize;
        if data_offset + nlen <= sa.len() {
            let name = &sa[data_offset..data_offset + nlen];
            return Some(
                String::from_utf8_lossy(name)
                    .trim_end_matches('\0')
                    .to_string(),
            );
        }
    }
    None
}

fn if_nametoindex(name: &str) -> Result<u32> {
    let cname = std::ffi::CString::new(name).context("interface name")?;
    let idx = unsafe { libc::if_nametoindex(cname.as_ptr()) };
    if idx == 0 {
        anyhow::bail!("interface `{name}` not found");
    }
    Ok(idx)
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

extern "C" {
    fn proc_listallpids(buffer: *mut i32, buffersize: i32) -> i32;
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut std::ffi::c_void,
        buffersize: i32,
    ) -> i32;
    fn proc_pidfdinfo(
        pid: i32,
        fd: i32,
        flavor: i32,
        buffer: *mut std::ffi::c_void,
        buffersize: i32,
    ) -> i32;
    fn proc_pidpath(pid: i32, buffer: *mut std::ffi::c_void, buffersize: u32) -> i32;
}

const _: () = assert!(PROC_PIDFDSOCKETINFO_SIZE >= 0);
