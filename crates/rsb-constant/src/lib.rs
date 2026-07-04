//! sing-box compatible type constants.

pub const TYPE_TUN: &str = "tun";
pub const TYPE_REDIRECT: &str = "redirect";
pub const TYPE_TPROXY: &str = "tproxy";
pub const TYPE_DIRECT: &str = "direct";
pub const TYPE_BLOCK: &str = "block";
pub const TYPE_DNS: &str = "dns";
pub const TYPE_SOCKS: &str = "socks";
pub const TYPE_HTTP: &str = "http";
pub const TYPE_MIXED: &str = "mixed";
pub const TYPE_SHADOWSOCKS: &str = "shadowsocks";
pub const TYPE_VMESS: &str = "vmess";
pub const TYPE_TROJAN: &str = "trojan";
pub const TYPE_NAIVE: &str = "naive";
pub const TYPE_WIREGUARD: &str = "wireguard";
pub const TYPE_HYSTERIA: &str = "hysteria";
pub const TYPE_TOR: &str = "tor";
pub const TYPE_SSH: &str = "ssh";
pub const TYPE_SHADOWTLS: &str = "shadowtls";
pub const TYPE_ANYTLS: &str = "anytls";
pub const TYPE_VLESS: &str = "vless";
pub const TYPE_TUIC: &str = "tuic";
pub const TYPE_HYSTERIA2: &str = "hysteria2";
pub const TYPE_RSQ: &str = "rsq";
pub const TYPE_TAILSCALE: &str = "tailscale";
pub const TYPE_SELECTOR: &str = "selector";
pub const TYPE_URLTEST: &str = "urltest";
pub const TYPE_CHAIN: &str = "chain";

pub const TYPE_SERVICE_API: &str = "api";
pub const TYPE_SERVICE_DERP: &str = "derp";
pub const TYPE_SERVICE_CCM: &str = "ccm";
pub const TYPE_SERVICE_OCM: &str = "ocm";
pub const TYPE_SERVICE_RESOLVED: &str = "resolved";
pub const TYPE_SERVICE_SSM_API: &str = "ssm-api";
pub const TYPE_SERVICE_HYSTERIA_REALM: &str = "hysteria-realm";
pub const TYPE_SERVICE_USBIP_SERVER: &str = "usbip-server";
pub const TYPE_SERVICE_USBIP_CLIENT: &str = "usbip-client";

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const ALL_INBOUND_TYPES: &[&str] = &[
    TYPE_TUN,
    TYPE_REDIRECT,
    TYPE_TPROXY,
    TYPE_DIRECT,
    TYPE_SOCKS,
    TYPE_HTTP,
    TYPE_MIXED,
    TYPE_SHADOWSOCKS,
    TYPE_VMESS,
    TYPE_TROJAN,
    TYPE_NAIVE,
    TYPE_SHADOWTLS,
    TYPE_VLESS,
    TYPE_ANYTLS,
    TYPE_HYSTERIA,
    TYPE_HYSTERIA2,
    TYPE_RSQ,
    TYPE_TUIC,
    TYPE_DNS,
];

pub const ALL_OUTBOUND_TYPES: &[&str] = &[
    TYPE_DIRECT,
    TYPE_BLOCK,
    TYPE_SELECTOR,
    TYPE_URLTEST,
    TYPE_CHAIN,
    TYPE_SOCKS,
    TYPE_HTTP,
    TYPE_SHADOWSOCKS,
    TYPE_VMESS,
    TYPE_TROJAN,
    TYPE_NAIVE,
    TYPE_TOR,
    TYPE_SSH,
    TYPE_SHADOWTLS,
    TYPE_VLESS,
    TYPE_ANYTLS,
    TYPE_HYSTERIA,
    TYPE_HYSTERIA2,
    TYPE_RSQ,
    TYPE_TUIC,
    TYPE_WIREGUARD,
    TYPE_DNS,
];

pub const ALL_ENDPOINT_TYPES: &[&str] = &[TYPE_WIREGUARD, TYPE_TAILSCALE];

pub const ALL_SERVICE_TYPES: &[&str] = &[
    TYPE_SERVICE_API,
    TYPE_SERVICE_DERP,
    TYPE_SERVICE_CCM,
    TYPE_SERVICE_OCM,
    TYPE_SERVICE_RESOLVED,
    TYPE_SERVICE_SSM_API,
    TYPE_SERVICE_HYSTERIA_REALM,
    TYPE_SERVICE_USBIP_SERVER,
    TYPE_SERVICE_USBIP_CLIENT,
];
