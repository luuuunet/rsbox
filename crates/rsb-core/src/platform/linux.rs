use anyhow::{Context, Result};
use std::ffi::CString;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::os::fd::{AsRawFd, FromRawFd};

pub fn detect_default_interface() -> Result<String> {
    let text = std::fs::read_to_string("/proc/net/route").context("read /proc/net/route")?;
    for line in text.lines().skip(1) {
        let cols: Vec<_> = line.split_whitespace().collect();
        if cols.len() >= 2 && cols[1] == "00000000" {
            return Ok(cols[0].to_string());
        }
    }
    anyhow::bail!("default route interface not found")
}

pub fn route_add(cidr: &str, iface: &str) -> Result<()> {
    let (dest, prefix) = parse_cidr(cidr)?;
    let ifindex = if_nametoindex(iface)?;
    let mut req = RtRequest::new(RTM_NEWROUTE, RTM_F_REPLACE);
    req.set_family(match dest {
        IpAddr::V4(_) => AF_INET as u8,
        IpAddr::V6(_) => AF_INET6 as u8,
    });
    req.set_dst_prefix(dest, prefix);
    req.set_oif(ifindex);
    req.send()?;
    Ok(())
}

fn if_nametoindex(name: &str) -> Result<u32> {
    let cname = CString::new(name).context("interface name")?;
    let idx = unsafe { libc::if_nametoindex(cname.as_ptr()) };
    if idx == 0 {
        anyhow::bail!("interface `{name}` not found");
    }
    Ok(idx)
}

fn parse_cidr(cidr: &str) -> Result<(IpAddr, u8)> {
    if let Some((ip, prefix)) = cidr.split_once('/') {
        let addr: IpAddr = ip.parse().context("cidr ip")?;
        let prefix = prefix.parse().context("cidr prefix")?;
        return Ok((addr, prefix));
    }
    Ok((cidr.parse().context("cidr ip")?, 32))
}

const AF_NETLINK: i32 = 16;
const AF_INET: i32 = 2;
const AF_INET6: i32 = 10;
const NETLINK_ROUTE: i32 = 0;
const RTM_NEWROUTE: u16 = 24;
const RTM_F_REPLACE: u32 = 0x100;
const NLA_DST: u16 = 1;
const NLA_OIF: u16 = 4;

struct RtRequest {
    buf: Vec<u8>,
}

impl RtRequest {
    fn new(kind: u16, flags: u32) -> Self {
        let mut buf = vec![0u8; libc::NLMSG_HDRLEN + libc::RTMSG_HDRLEN];
        let nl = buf.as_mut_ptr() as *mut libc::nlmsghdr;
        unsafe {
            (*nl).nlmsg_len = buf.len() as u32;
            (*nl).nlmsg_type = kind;
            (*nl).nlmsg_flags = (libc::NLM_F_REQUEST | libc::NLM_F_ACK) as u16;
            (*nl).nlmsg_seq = 1;
            let rt = (nl as *mut u8).add(libc::NLMSG_HDRLEN) as *mut libc::rtmsg;
            (*rt).rtm_family = AF_INET as u8;
            (*rt).rtm_table = libc::RT_TABLE_MAIN as u8;
            (*rt).rtm_protocol = libc::RTPROT_STATIC as u8;
            (*rt).rtm_scope = libc::RT_SCOPE_UNIVERSE as u8;
            (*rt).rtm_type = libc::RTN_UNICAST as u8;
            (*rt).rtm_flags = flags;
        }
        Self { buf }
    }

    fn set_family(&mut self, family: u8) {
        let nl = self.buf.as_mut_ptr() as *mut libc::nlmsghdr;
        unsafe {
            let rt = (nl as *mut u8).add(libc::NLMSG_HDRLEN) as *mut libc::rtmsg;
            (*rt).rtm_family = family;
        }
    }

    fn set_dst_prefix(&mut self, dest: IpAddr, prefix: u8) {
        let nl = self.buf.as_mut_ptr() as *mut libc::nlmsghdr;
        unsafe {
            let rt = (nl as *mut u8).add(libc::NLMSG_HDRLEN) as *mut libc::rtmsg;
            (*rt).rtm_dst_len = prefix;
        }
        let bytes = match dest {
            IpAddr::V4(v4) => v4.octets().to_vec(),
            IpAddr::V6(v6) => v6.octets().to_vec(),
        };
        self.push_attr(NLA_DST, &bytes);
    }

    fn set_oif(&mut self, ifindex: u32) {
        self.push_attr(NLA_OIF, &ifindex.to_ne_bytes());
    }

    fn push_attr(&mut self, kind: u16, payload: &[u8]) {
        let len = libc::NLA_HDRLEN + payload.len();
        let pad = (4 - (len % 4)) % 4;
        let total = len + pad;
        let start = self.buf.len();
        self.buf.resize(start + total, 0);
        let attr = self.buf[start..].as_mut_ptr() as *mut libc::nlattr;
        unsafe {
            (*attr).nla_len = (libc::NLA_HDRLEN + payload.len()) as u16;
            (*attr).nla_type = kind;
            std::ptr::copy_nonoverlapping(
                payload.as_ptr(),
                (attr as *mut u8).add(libc::NLA_HDRLEN),
                payload.len(),
            );
        }
        let nl = self.buf.as_mut_ptr() as *mut libc::nlmsghdr;
        unsafe {
            (*nl).nlmsg_len = self.buf.len() as u32;
        }
    }

    fn send(&self) -> Result<()> {
        let fd = unsafe { libc::socket(AF_NETLINK, libc::SOCK_RAW, NETLINK_ROUTE) };
        if fd < 0 {
            anyhow::bail!("netlink socket failed");
        }
        let sock = unsafe { socket2::Socket::from_raw_fd(fd) };
        let addr = libc::sockaddr_nl {
            nl_family: AF_NETLINK as u16,
            nl_pad: Default::default(),
            nl_pid: 0,
            nl_groups: 0,
        };
        let ret = unsafe {
            libc::send(
                sock.as_raw_fd(),
                self.buf.as_ptr() as *const _,
                self.buf.len(),
                0,
            )
        };
        if ret < 0 {
            anyhow::bail!("netlink send failed");
        }
        Ok(())
    }
}
