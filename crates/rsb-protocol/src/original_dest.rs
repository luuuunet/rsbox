//! Original destination lookup for transparent proxy (redirect / tproxy).

use anyhow::{Context, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio::net::TcpStream;

const LINUX_SO_ORIGINAL_DST: libc::c_int = 80;

/// Returns the original destination for a socket captured by iptables REDIRECT / TPROXY.
pub async fn get_original_destination(stream: &TcpStream) -> Result<SocketAddr> {
    #[cfg(target_os = "linux")]
    {
        return linux_original_dest(stream);
    }
    #[cfg(target_os = "macos")]
    {
        return macos_original_dest(stream).await;
    }
    #[cfg(target_os = "windows")]
    {
        return windows_original_dest(stream).await;
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        let _ = stream;
        anyhow::bail!("original destination lookup unsupported on this platform")
    }
}

#[cfg(target_os = "linux")]
fn linux_original_dest(stream: &TcpStream) -> Result<SocketAddr> {
    use std::os::unix::io::AsRawFd;
    let fd = stream.as_raw_fd();
    let remote = stream.peer_addr().context("redirect socket peer address")?;
    if remote.ip().is_ipv4() {
        let mut addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        let ret = unsafe {
            libc::getsockopt(
                fd,
                libc::IPPROTO_IP,
                LINUX_SO_ORIGINAL_DST,
                &mut addr as *mut _ as *mut libc::c_void,
                &mut len,
            )
        };
        if ret != 0 {
            anyhow::bail!(
                "SO_ORIGINAL_DST failed: {}",
                std::io::Error::last_os_error()
            );
        }
        let ip = Ipv4Addr::from(u32::from_be(addr.sin_addr.s_addr));
        let port = u16::from_be(addr.sin_port);
        return Ok(SocketAddr::from((ip, port)));
    }
    let mut addr: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t;
    let ret = unsafe {
        libc::getsockopt(
            fd,
            libc::IPPROTO_IPV6,
            LINUX_SO_ORIGINAL_DST,
            &mut addr as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    if ret != 0 {
        anyhow::bail!(
            "IPV6 SO_ORIGINAL_DST failed: {}",
            std::io::Error::last_os_error()
        );
    }
    let ip = Ipv6Addr::from(addr.sin6_addr.s6_addr);
    let port = u16::from_be(addr.sin6_port);
    Ok(SocketAddr::from((ip, port)))
}

#[cfg(target_os = "macos")]
async fn macos_original_dest(stream: &TcpStream) -> Result<SocketAddr> {
    use std::os::unix::io::AsRawFd;
    const PF_OUT: u8 = 0x2;
    const DIOCNATLOOK: libc::c_ulong = 0xc0544417;
    #[repr(C)]
    struct PfNatLook {
        saddr: [u8; 16],
        daddr: [u8; 16],
        rsaddr: [u8; 16],
        rdaddr: [u8; 16],
        sxport: [u8; 4],
        dxport: [u8; 4],
        rsxport: [u8; 4],
        rdxport: [u8; 4],
        af: u8,
        proto: u8,
        proto_variant: u8,
        direction: u8,
    }
    let fd = unsafe { libc::open(b"/dev/pf\0".as_ptr() as *const _, libc::O_RDONLY) };
    if fd < 0 {
        anyhow::bail!("open /dev/pf: {}", std::io::Error::last_os_error());
    }
    let _guard = PfFd(fd);
    let la = stream.local_addr()?;
    let ra = stream.peer_addr()?;
    let mut nl: PfNatLook = unsafe { std::mem::zeroed() };
    nl.proto = libc::IPPROTO_TCP as u8;
    nl.direction = PF_OUT;
    match (ra.ip(), la.ip()) {
        (IpAddr::V4(ra_ip), IpAddr::V4(la_ip)) => {
            nl.af = libc::AF_INET as u8;
            nl.saddr[..4].copy_from_slice(&ra_ip.octets());
            nl.daddr[..4].copy_from_slice(&la_ip.octets());
        },
        (IpAddr::V6(ra_ip), IpAddr::V6(la_ip)) => {
            nl.af = libc::AF_INET6 as u8;
            nl.saddr.copy_from_slice(&ra_ip.octets());
            nl.daddr.copy_from_slice(&la_ip.octets());
        },
        _ => anyhow::bail!("address family mismatch on redirect socket"),
    }
    let ra_port = ra.port();
    let la_port = la.port();
    nl.sxport[0] = (ra_port >> 8) as u8;
    nl.sxport[1] = (ra_port & 0xff) as u8;
    nl.dxport[0] = (la_port >> 8) as u8;
    nl.dxport[1] = (la_port & 0xff) as u8;
    let sock_fd = stream.as_raw_fd();
    let ret = unsafe {
        libc::ioctl(
            fd,
            DIOCNATLOOK,
            &mut nl as *mut PfNatLook as *mut libc::c_void,
        )
    };
    let _ = sock_fd;
    if ret < 0 {
        anyhow::bail!("DIOCNATLOOK failed: {}", std::io::Error::last_os_error());
    }
    let (ip, port) = if nl.af == libc::AF_INET as u8 {
        (
            IpAddr::V4(Ipv4Addr::new(
                nl.rdaddr[0],
                nl.rdaddr[1],
                nl.rdaddr[2],
                nl.rdaddr[3],
            )),
            u16::from_be_bytes([nl.rdxport[0], nl.rdxport[1]]),
        )
    } else {
        (
            IpAddr::V6(Ipv6Addr::from(nl.rdaddr)),
            u16::from_be_bytes([nl.rdxport[0], nl.rdxport[1]]),
        )
    };
    Ok(SocketAddr::new(ip, port))
}

#[cfg(target_os = "macos")]
struct PfFd(libc::c_int);

#[cfg(target_os = "macos")]
impl Drop for PfFd {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.0);
        }
    }
}

#[cfg(target_os = "windows")]
async fn windows_original_dest(stream: &TcpStream) -> Result<SocketAddr> {
    crate::original_dest_windows::get_original_destination(stream).await
}
