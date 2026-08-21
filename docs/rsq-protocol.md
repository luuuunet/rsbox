# RSQ Protocol v1

rsbox-only QUIC proxy protocol. **Not compatible** with Hysteria2 or sing-box.

## Goals

1. **Speed** — QUIC multi-stream, Hy2-class windows, Brutal-style bandwidth negotiation.
2. **Unlike Hy2** — no HTTP/3 `/auth`, no `hysteria-*` headers, no message id `0x401`, ALPN `rsq/1`.
3. **Traffic realism** — default `traffic_profile: video` (high download, smoothed upload).

## Stack

```
UDP
 └─ optional RSQ-Obfs v1
     └─ QUIC + TLS 1.3 (ALPN: rsq/1)
         └─ control bidi stream: binary AUTH
             └─ data bidi streams: TCP relay
             └─ QUIC datagrams: UDP relay
```

Default port: **443/udp**.

## Wire frame

All RSQ payloads (control + stream headers) start with:

| Field | Size | Value |
|-------|------|--------|
| magic | 4 | `RSQ\x01` |
| type | 1 | see below |
| flags | 1 | bit0: padding present |
| length | varint | payload length |
| payload | N | type-specific |
| pad_len | 1 | present when flags bit0; 0–128 |
| pad | N | random bytes (length = pad_len) |

### Frame types

| Type | Name | Direction |
|------|------|-----------|
| `0x01` | AUTH_REQ | client → server (control stream) |
| `0x02` | AUTH_OK | server → client |
| `0x10` | TCP_OPEN | client → server (data stream, first write) |
| `0x11` | TCP_OK | server → client |
| `0x12` | TCP_ERR | server → client (connect/resolve failed) |
| `0x20` | PING | either (control stream) |
| `0x21` | PONG | response |

Auth failure: server **closes QUIC** with generic code (no AUTH_FAIL frame on wire).

## Authentication (PSK + HMAC)

```
auth_key = HMAC-SHA256(key=password_bytes, data="rsq-auth-v1")
```

### AUTH_REQ payload

| Field | Type |
|-------|------|
| version | u8 = 1 |
| client_random | 32 bytes |
| timestamp | u64 BE Unix seconds |
| rx_bps | varint (client receive / download cap, 0 = unlimited) |
| up_bps | varint (client upload cap from traffic profile) |
| profile | u8 (0=raw, 1=video, 2=browse, 3=balanced) |
| proof | 32 bytes HMAC-SHA256(auth_key, version\|\|random\|\|timestamp\|\|rx_bps_varint\|\|up_bps_varint\|\|profile) |

Server checks: timestamp within ±120s, proof valid, password in user list.

### AUTH_OK payload

| Field | Type |
|-------|------|
| version | u8 = 1 |
| session_id | u32 |
| server_rx_bps | varint (0 = unlimited) |
| udp_enabled | u8 (0/1) |

## TCP relay

After AUTH, each new **bidirectional stream**:

1. Client sends one `TCP_OPEN` frame:
   - payload: `host:port` UTF-8 target string
   - optional random padding (64–512 B, flags bit0)
2. Server connects target, replies `TCP_OK` (payload may be `ok`), or `TCP_ERR` with UTF-8 error message.
3. Raw bytes follow after `TCP_OK` (same as Hy2 data phase, different header).

## UDP relay

QUIC datagram body **without** RSQ frame magic (binary layout):

| Field | Type |
|-------|------|
| session_id | u32 |
| packet_id | u16 |
| fragment_id | u8 |
| fragment_count | u8 |
| addr_len | varint |
| addr | UTF-8 `host:port` |
| payload | rest |

## RSQ-Obfs v1

Optional UDP obfuscation (default **on** in rsbox configs).

```
packet = salt[8] || (payload XOR stream_key)
stream_key = BLAKE2b-512(password || "rsq-obfs-v1" || salt)[0..32] repeated XOR
```

Disable with `"obfs": { "enabled": false }`.

## Traffic profiles

| profile | down | up | Use |
|---------|------|-----|-----|
| `video` (default) | unlimited | capped (config `up_mbps`) | Long high download |
| `browse` | moderate | low burst | Web-like |
| `balanced` | high | medium | Sync/cloud |
| `raw` | unlimited | unlimited | Lab / max speed |

Profiles set `up_bps` / pacing hints in AUTH_REQ; Brutal negotiation uses `rx_bps`.

## QUIC / TLS

- ALPN: `rsq/1` only (no HTTP/3 stack).
- Windows: stream recv 8 MiB, conn recv 20 MiB (Hy2-class).
- `keep_alive_interval`: base 20s ±30% jitter.
- `max_session_age`: default 3h, graceful reconnect.

## rsbox configuration

### Server inbound

```json
{
  "type": "rsq",
  "tag": "rsq-in",
  "listen": "::",
  "listen_port": 443,
  "users": [{ "password": "secret" }],
  "tls": {
    "enabled": true,
    "certificate_path": "/path/fullchain.pem",
    "key_path": "/path/privkey.pem"
  },
  "obfs": { "enabled": true, "password": "obfs-secret" },
  "udp": true,
  "down_mbps": 0,
  "up_mbps": 0
}
```

### Client outbound

```json
{
  "type": "rsq",
  "tag": "proxy",
  "server": "example.com",
  "server_port": 443,
  "password": "secret",
  "tls": {
    "enabled": true,
    "server_name": "example.com",
    "insecure": false
  },
  "obfs": { "enabled": true, "password": "obfs-secret" },
  "traffic_profile": "video",
  "warm_up": true,
  "down_mbps": 0,
  "up_mbps": 50
}
```

### Share URL

```
rsq://PASSWORD@host:443?sni=example.com&profile=video#name
```

## Versioning

- Magic byte 4 of frame = protocol version (`0x01` today).
- Incompatible changes bump magic to `RSQ\x02`.

## Security notes

- Rotate passwords per node; obfs password may equal auth password but separate is recommended.
- Use valid TLS certificates (Let's Encrypt) on port 443.
- Place a normal TCP 443 website on the same IP when possible.
