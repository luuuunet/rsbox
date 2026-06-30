//! ShadowTLS wire constants (sing-shadowtls).

pub const TLS_HEADER_SIZE: usize = 5;
pub const TLS_RANDOM_SIZE: usize = 32;
pub const TLS_SESSION_ID_SIZE: usize = 32;
pub const HMAC_SIZE_V2: usize = 8;
pub const HMAC_SIZE_V3: usize = 4;
pub const TLS_HMAC_HEADER_SIZE_V3: usize = TLS_HEADER_SIZE + HMAC_SIZE_V3;

pub const HANDSHAKE: u8 = 22;
pub const APPLICATION_DATA: u8 = 23;
pub const ALERT: u8 = 21;

pub const SERVER_HELLO: u8 = 2;
pub const SERVER_RANDOM_INDEX: usize = TLS_HEADER_SIZE + 1 + 3 + 2;

pub const TLS_VERSION_12: [u8; 2] = [0x03, 0x03];
