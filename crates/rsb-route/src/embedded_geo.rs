//! Bundled geosite/geoip rule-sets (no network required for china-direct).

/// Pre-extracted `geosite-cn` domain list (fast load). Generated from SagerNet `.srs`.
pub fn embedded_geosite_cn_text() -> &'static str {
    include_str!("../assets/geosite-cn.txt")
}

/// Embedded SagerNet `geoip-cn.srs` for offline China IP matching.
pub fn embedded_geo_srs(tag: &str) -> Option<&'static [u8]> {
    match tag {
        "geoip-cn" => Some(include_bytes!("../assets/geoip-cn.srs")),
        _ => None,
    }
}
