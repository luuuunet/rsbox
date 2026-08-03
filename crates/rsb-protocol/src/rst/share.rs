//! RST `rst://` share link builder.

use std::fmt::Write;

fn pct_encode_userinfo(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                write!(out, "%{b:02X}").ok();
            }
        }
    }
    out
}

pub struct RstShareLink<'a> {
    pub password: &'a str,
    pub server: &'a str,
    pub port: u16,
    pub name: Option<&'a str>,
    pub sni: Option<&'a str>,
    pub up_mbps: Option<u32>,
    pub down_mbps: Option<u32>,
    pub profile: Option<&'a str>,
    pub warm_up: bool,
    pub udp: bool,
    pub obfs_password: Option<&'a str>,
    pub obfs_version: Option<u8>,
    pub insecure: bool,
    pub brutal: bool,
}

impl<'a> RstShareLink<'a> {
    pub fn encode(&self) -> String {
        let mut url = format!(
            "rst://{}@{}:{}",
            pct_encode_userinfo(self.password),
            self.server,
            self.port
        );
        let mut qs = String::new();
        let mut first = true;
        let mut push = |k: &str, v: &str| {
            if first {
                write!(qs, "?{k}={v}").ok();
                first = false;
            } else {
                write!(qs, "&{k}={v}").ok();
            }
        };
        if let Some(sni) = self.sni.filter(|s| !s.is_empty()) {
            push("sni", sni);
        }
        if let Some(up) = self.up_mbps.filter(|v| *v > 0) {
            push("up", &up.to_string());
        }
        if let Some(down) = self.down_mbps.filter(|v| *v > 0) {
            push("down", &down.to_string());
        }
        if let Some(profile) = self.profile.filter(|s| !s.is_empty()) {
            push("profile", profile);
        }
        if let Some(obfs) = self.obfs_password.filter(|s| !s.is_empty()) {
            push("obfs", "salamander");
            push("obfs-password", obfs);
        }
        if self.obfs_version == Some(2) {
            push("obfs-version", "2");
        }
        if !self.warm_up {
            push("warm_up", "0");
        }
        if !self.udp {
            push("udp", "0");
        }
        if self.insecure {
            push("insecure", "1");
        }
        push("brutal", "1");
        url.push_str(&qs);
        if let Some(name) = self.name.filter(|s| !s.is_empty()) {
            url.push('#');
            url.push_str(name);
        }
        url
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_special_password() {
        let link = RstShareLink {
            password: "p@ss:word",
            server: "example.com",
            port: 8443,
            name: Some("demo"),
            sni: Some("example.com"),
            up_mbps: Some(100),
            down_mbps: Some(200),
            profile: Some("raw"),
            warm_up: false,
            udp: true,
            obfs_password: Some("obfs-secret"),
            obfs_version: Some(2),
            insecure: true,
            brutal: false,
        };
        let s = link.encode();
        assert!(s.starts_with("rst://p%40ss%3Aword@example.com:8443?"));
        assert!(s.contains("sni=example.com"));
        assert!(s.contains("obfs=salamander"));
        assert!(s.contains("obfs-password=obfs-secret"));
        assert!(s.contains("obfs-version=2"));
        assert!(s.contains("warm_up=0"));
        assert!(s.contains("insecure=1"));
        assert!(s.contains("brutal=1"));
        assert!(s.ends_with("#demo"));
    }
}
