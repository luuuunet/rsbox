# Protocols & encryption

Reference for protocols, ciphers, and TLS options implemented in **rsbox**.  
Config field names follow the **sing-box–compatible** JSON style unless noted.

For RSQ wire details see [rsq-protocol.md](rsq-protocol.md).

---

## 1. Inbound / outbound types

### Inbounds (`inbounds[].type`)

| Type | Role |
|------|------|
| `mixed` | HTTP + SOCKS5 on one port (typical local client) |
| `http` | HTTP CONNECT proxy |
| `socks` | SOCKS5 (optional UDP associate) |
| `direct` | Accept raw TCP and forward by route |
| `shadowsocks` | Shadowsocks server |
| `vmess` | VMess server (AEAD) |
| `vless` | VLESS server |
| `trojan` | Trojan server |
| `naive` | NaiveProxy-style inbound |
| `shadowtls` | ShadowTLS (often chained with SS) |
| `anytls` | AnyTLS inbound |
| `hysteria` | Hysteria v1 |
| `hysteria2` | Hysteria2 |
| `rsq` | rsbox QUIC protocol (native) |
| `rst` | rsbox RST protocol (native) |
| `tuic` | TUIC v5 |
| `tun` | TUN device (system-wide) |
| `redirect` | Linux redirect |
| `tproxy` | Linux TPROXY |
| `dns` | DNS inbound |

### Outbounds (`outbounds[].type`)

| Type | Role |
|------|------|
| `direct` / `block` / `dns` | Local actions |
| `socks` / `http` | Classic proxies |
| `shadowsocks` / `vmess` / `vless` / `trojan` / `naive` | Encrypted proxies |
| `shadowtls` / `anytls` | TLS camouflage layers |
| `hysteria` / `hysteria2` / `tuic` | QUIC / UDP-oriented |
| `rsq` / `rst` | Native QUIC protocols |
| `wireguard` | WireGuard tunnel |
| `ssh` / `tor` | Special outbounds |
| `selector` / `urltest` / `chain` | Groups / chaining |

---

## 2. Shadowsocks encryption methods

Set with `"method"` on Shadowsocks inbound/outbound.

| Method | Family | Notes |
|--------|--------|--------|
| `aes-128-gcm` | AEAD | Widely supported |
| `aes-256-gcm` | AEAD | **Default** if `method` omitted |
| `chacha20-ietf-poly1305` | AEAD | Alias: `chacha20-poly1305` |
| `2022-blake3-aes-128-gcm` | SS2022 | Password is base64-encoded key material |
| `2022-blake3-aes-256-gcm` | SS2022 | Same |
| `2022-blake3-chacha20-poly1305` | SS2022 | Same |

**Not supported:** stream ciphers (`aes-128-cfb`, `chacha20-ietf`, `rc4-md5`, …). Prefer AEAD or SS2022.

### Example (client)

```json
{
  "type": "shadowsocks",
  "tag": "ss-out",
  "server": "example.com",
  "server_port": 8388,
  "method": "2022-blake3-aes-128-gcm",
  "password": "<base64-psk>"
}
```

See also: [`examples/config-shadowsocks.json`](../examples/config-shadowsocks.json), ShadowTLS+SS samples under `examples/`.

---

## 3. VMess security

Outbound field: `"security"` (default `auto`).

| Value | Meaning |
|-------|---------|
| `auto` | Prefer AEAD (implementation picks a strong suite) |
| `aes-128-gcm` | AES-128-GCM body encryption |
| `chacha20-poly1305` | ChaCha20-Poly1305 |
| `none` | No body cipher (still uses AEAD header framing where applicable) |

Use a UUID as `uuid`. Prefer TLS or a secure transport in production; plain VMess is detectable.

---

## 4. VLESS / Trojan

| Protocol | Auth | Encryption on the wire |
|----------|------|-------------------------|
| **VLESS** | UUID | Typically **TLS 1.3** (or REALITY). Optional `flow`: `xtls-rprx-vision` |
| **Trojan** | Password | TLS 1.2/1.3; looks like HTTPS |

REALITY example: [`examples/config-reality.json`](../examples/config-reality.json).

```json
{
  "type": "vless",
  "uuid": "00000000-0000-0000-0000-000000000001",
  "flow": "xtls-rprx-vision",
  "tls": {
    "enabled": true,
    "server_name": "www.microsoft.com",
    "utls": { "enabled": true, "fingerprint": "chrome" },
    "reality": {
      "enabled": true,
      "public_key": "<server-public-key>",
      "short_id": "<short-id>"
    }
  }
}
```

---

## 5. QUIC-family protocols

| Protocol | Auth | Crypto | Notes |
|----------|------|--------|--------|
| **Hysteria2** | Password | TLS 1.3 over QUIC | Bandwidth / Brutal CC options |
| **Hysteria (v1)** | Auth string | TLS + QUIC | Legacy |
| **TUIC v5** | UUID + password | TLS 1.3 over QUIC | |
| **RSQ** | Password (HMAC auth) | TLS 1.3, ALPN `rsq/1` | Optional UDP obfs (BLAKE2b XOR stream) |
| **RST** | Password | TLS 1.3 / QUIC | Native; optional Salamander-style UDP obfs |

RSQ obfuscation (when enabled) derives a stream key with BLAKE2b from the password; see [rsq-protocol.md](rsq-protocol.md).

---

## 6. TLS, uTLS, REALITY

### Common TLS fields

```json
"tls": {
  "enabled": true,
  "server_name": "example.com",
  "insecure": false,
  "alpn": ["h3", "h2", "http/1.1"]
}
```

### uTLS fingerprints (`tls.utls.fingerprint`)

| Fingerprint | Typical use |
|-------------|-------------|
| `chrome` | Default Chrome-like ClientHello |
| `firefox` | Firefox |
| `edge` | Edge |
| `safari` / `ios` | Apple stacks |
| `random` | Randomized among profiles |

```json
"utls": { "enabled": true, "fingerprint": "chrome" }
```

### REALITY

Used with VLESS (and related TLS paths). Requires `public_key` (client) / private key material (server) and optional `short_id`. Mimics a real HTTPS site handshake.

---

## 7. Camouflage layers

| Type | Role |
|------|------|
| **ShadowTLS** | TLS camouflage in front of another protocol (often Shadowsocks 2022) |
| **AnyTLS** | Session multiplexing over TLS |

Examples: `examples/config-shadowtls-ss.json`, `examples/config-anytls.json`.

---

## 8. WireGuard

Outbound / endpoint type `wireguard`: Noise IK, ChaCha20-Poly1305 (standard WireGuard crypto). Provide private key, peer public key, endpoint, and allowed IPs as in sing-box-style configs.

---

## 9. Local proxies (no payload cipher)

| Type | Notes |
|------|--------|
| `mixed` / `http` / `socks` | Cleartext to localhost; encrypt only on the selected outbound |
| `direct` | No tunnel encryption |
| `block` | Drop |

Always put encryption on the **remote** outbound (SS / VLESS / Hy2 / RSQ / …).

---

## 10. Routing presets

`route.preset: "china-direct"` (also exposed as smart split in clients):

- Domains / IPs classified as China → `direct`
- Everything else → proxy (`route.final`)

Embedded geosite/geoip assets ship in the binary for offline matching. See [`examples/config-china-direct.json`](../examples/config-china-direct.json).

---

## 11. Security recommendations

1. Prefer **SS2022**, **VLESS+REALITY**, **Hysteria2**, or **RSQ/RST** over legacy stream ciphers or plain VMess.
2. Keep `tls.insecure` **false** unless debugging.
3. Use strong unique passwords / UUIDs; rotate credentials after leaks.
4. For Shadowsocks 2022, generate keys with the standard base64 length for the chosen method.
5. Enable UDP only when you need it (DNS, QUIC, games).

---

## 12. Compatibility notes

- JSON is largely **sing-box compatible**; not every sing-box feature is implemented.
- **RSQ / RST** are **rsbox-native** and are **not** drop-in Hysteria2 / sing-box peers.
- Mobile builds use `rsb-libbox` (same protocol core as the desktop binary).

If a method or field is missing from this page, check the source under `crates/rsb-protocol/` or open an issue.
