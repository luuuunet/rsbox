//! Destination policy — block private / link-local / metadata targets by default.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Returns true if connecting to `addr` should be refused (SSRF / open-proxy guard).
pub fn is_forbidden_dest(addr: SocketAddr, allow_private: bool) -> bool {
    if allow_private {
        return false;
    }
    !is_public_global(addr.ip())
}

fn is_public_global(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    if ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
    {
        return false;
    }
    let o = ip.octets();
    // CGNAT 100.64.0.0/10
    if o[0] == 100 && (o[1] & 0xc0) == 64 {
        return false;
    }
    // IETF protocol assignments 192.0.0.0/24 (includes some special-use)
    if o[0] == 192 && o[1] == 0 && o[2] == 0 {
        return false;
    }
    // TEST-NET / documentation
    if o[0] == 192 && o[1] == 0 && o[2] == 2 {
        return false;
    }
    if o[0] == 198 && (o[1] == 51) && o[2] == 100 {
        return false;
    }
    if o[0] == 203 && o[1] == 0 && o[2] == 113 {
        return false;
    }
    // Benchmarking 198.18.0.0/15
    if o[0] == 198 && (o[1] & 0xfe) == 18 {
        return false;
    }
    true
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return false;
    }
    if ip.to_ipv4_mapped().is_some_and(|v4| !is_public_v4(v4)) {
        return false;
    }
    // Unique local fc00::/7
    let s = ip.segments();
    if (s[0] & 0xfe00) == 0xfc00 {
        return false;
    }
    // Link-local fe80::/10
    if (s[0] & 0xffc0) == 0xfe80 {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_private_by_default() {
        assert!(is_forbidden_dest("127.0.0.1:80".parse().unwrap(), false));
        assert!(is_forbidden_dest("10.0.0.1:443".parse().unwrap(), false));
        assert!(is_forbidden_dest("169.254.169.254:80".parse().unwrap(), false));
        assert!(is_forbidden_dest("100.64.1.1:80".parse().unwrap(), false));
        assert!(!is_forbidden_dest("1.1.1.1:443".parse().unwrap(), false));
    }

    #[test]
    fn allow_private_bypasses() {
        assert!(!is_forbidden_dest("127.0.0.1:80".parse().unwrap(), true));
    }
}
