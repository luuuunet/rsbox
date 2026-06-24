//! Integration smoke tests (no live network).

use rsb_protocol::{
    is_known_inbound, is_known_outbound, is_known_service, ALL_INBOUND_TYPES, ALL_OUTBOUND_TYPES,
};

#[test]
fn registry_covers_declared_types() {
    assert!(ALL_INBOUND_TYPES.iter().all(|t| is_known_inbound(t)));
    assert!(ALL_OUTBOUND_TYPES.iter().all(|t| is_known_outbound(t)));
    assert!(is_known_service("api"));
    assert!(is_known_outbound("wireguard"));
}

#[test]
fn urltest_probe_url_parsing() {
    let parsed =
        rsb_protocol::urltest::parse_probe_url_for_test("https://www.gstatic.com/generate_204")
            .unwrap();
    assert_eq!(parsed.0, "www.gstatic.com");
    assert_eq!(parsed.1, 443);
}
