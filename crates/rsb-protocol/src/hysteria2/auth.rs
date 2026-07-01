/// Megabit/s → byte/s, matching sing-quic `hysteria.MbpsToBps` (125_000).
pub const MBPS_TO_BPS: u64 = 125_000;

use http::{HeaderMap, StatusCode};

pub const AUTH_PATH: &str = "/auth";
pub const AUTH_AUTHORITY: &str = "hysteria";

pub struct AuthRequest {
    pub password: String,
    pub client_rx_bps: u64,
}

pub struct AuthResponse {
    pub udp_enabled: bool,
    pub server_rx_bps: u64,
    pub server_rx_auto: bool,
}

pub fn parse_auth_request(headers: &HeaderMap) -> Option<AuthRequest> {
    let password = headers.get("hysteria-auth")?.to_str().ok()?.to_string();
    let client_rx = headers
        .get("hysteria-cc-rx")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    Some(AuthRequest {
        password,
        client_rx_bps: client_rx,
    })
}

pub fn build_auth_response(udp_enabled: bool, down_mbps: u32) -> (StatusCode, HeaderMap) {
    let mut headers = HeaderMap::new();
    headers.insert(
        "hysteria-udp",
        if udp_enabled { "true" } else { "false" }.parse().unwrap(),
    );
    if down_mbps == 0 {
        headers.insert("hysteria-cc-rx", "0".parse().unwrap());
    } else {
        let bps = down_mbps as u64 * MBPS_TO_BPS;
        headers.insert("hysteria-cc-rx", bps.to_string().parse().unwrap());
    }
    let padding = random_padding(64, 512);
    headers.insert("hysteria-padding", padding.parse().unwrap());
    (StatusCode::from_u16(233).unwrap(), headers)
}

pub fn random_padding(min: usize, max: usize) -> String {
    let span = max.saturating_sub(min) + 1;
    let len = min + (rand::random::<u32>() as usize % span);
    let mut s = String::with_capacity(len);
    for _ in 0..len {
        s.push(char::from(b'a' + (rand::random::<u8>() % 26)));
    }
    s
}

pub fn random_padding_len(min: usize, max: usize) -> usize {
    let span = max.saturating_sub(min) + 1;
    min + (rand::random::<u32>() as usize % span)
}

pub fn is_auth_request(method: &http::Method, path: &str, _authority: Option<&str>) -> bool {
    method == http::Method::POST && path == AUTH_PATH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_status_code() {
        let (status, _) = build_auth_response(true, 100);
        assert_eq!(status.as_u16(), 233);
    }
}
