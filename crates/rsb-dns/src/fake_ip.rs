use anyhow::Result;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;

pub struct FakeIpPool {
    base: u32,
    mask: u32,
    next: Mutex<u32>,
    domain_map: Mutex<HashMap<String, IpAddr>>,
}

impl FakeIpPool {
    pub fn new(cidr: &str) -> Result<Self> {
        let (net, prefix) = cidr.split_once('/').unwrap_or((cidr, "15"));
        let base: u32 = net.parse::<Ipv4Addr>()?.into();
        let prefix: u8 = prefix.parse().unwrap_or(15);
        let mask = if prefix >= 32 {
            u32::MAX
        } else {
            u32::MAX << (32 - prefix)
        };
        Ok(Self {
            base,
            mask,
            next: Mutex::new(1),
            domain_map: Mutex::new(HashMap::new()),
        })
    }

    pub fn allocate(&self, domain: &str) -> IpAddr {
        if let Ok(map) = self.domain_map.lock() {
            if let Some(ip) = map.get(domain) {
                return *ip;
            }
        }
        let mut cursor = self.next.lock().expect("fakeip lock");
        let offset = *cursor;
        *cursor = cursor.wrapping_add(1);
        let ip = IpAddr::V4(Ipv4Addr::from(
            (self.base & self.mask) | (offset & !self.mask),
        ));
        if let Ok(mut map) = self.domain_map.lock() {
            map.insert(domain.to_string(), ip);
        }
        ip
    }
}
