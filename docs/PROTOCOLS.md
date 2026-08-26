# Protocols & encryption

rsbox ships **three proxy protocols** plus local routing infrastructure.

Legacy sing-box types (Shadowsocks, VMess, VLESS, Trojan, TUIC, WireGuard, …) are **not registered** and will fail config validation.

---

## 1. Proxy protocols

| Type | Role | Crypto / transport |
|------|------|---------------------|
| **rsq** | Native QUIC proxy | TLS 1.3, ALPN `rsq/1`; password + HMAC auth; optional UDP obfs (BLAKE2b stream) |
| **rst** | Native QUIC proxy | TLS 1.3 / HTTP/3 auth; password; optional Salamander-style UDP obfs |
| **hysteria2** | Hy2-compatible | TLS 1.3 over QUIC; password auth; Brutal bandwidth CC |

Wire details: [rsq-protocol.md](rsq-protocol.md).

### RSQ client outbound (example)

```json
{
  "type": "rsq",
  "tag": "jp",
  "server": "203.0.113.10",
  "server_port": 443,
  "password": "secret",
  "up_mbps": 100,
  "down_mbps": 500,
  "brutal": true,
  "tls": {
    "enabled": true,
    "server_name": "example.com",
    "insecure": false
  },
  "obfs": { "enabled": true, "password": "obfs-secret" }
}
```

### Hysteria2 client outbound (example)

```json
{
  "type": "hysteria2",
  "tag": "hy2",
  "server": "203.0.113.10",
  "server_port": 443,
  "password": "secret",
  "tls": {
    "enabled": true,
    "server_name": "example.com"
  }
}
```

### RST client outbound (example)

Same shape as RSQ/Hy2 sing-box JSON; see `examples/rst-local.json` if present in your tree.

---

## 2. Local inbounds (no remote encryption)

| Type | Use |
|------|-----|
| `mixed` | HTTP + SOCKS5 on one port (typical desktop client) |
| `http` | HTTP CONNECT only |
| `socks` | SOCKS5 only |
| `direct` | Accept and route by rules |
| `dns` | DNS inbound |
| `tun` | System-wide capture (mobile / desktop builds) |

Traffic is cleartext to `127.0.0.1`; encryption happens on the RSQ/RST/Hy2 outbound.

---

## 3. Routing outbounds

| Type | Use |
|------|-----|
| `direct` | Bypass proxy |
| `block` | Drop |
| `dns` | DNS routing action |
| `selector` | Pick one member outbound |
| `urltest` | Auto-pick lowest-latency member |

---

## 4. Encryption summary

| Layer | Algorithms |
|-------|------------|
| **RSQ auth** | HMAC-SHA256 over PSK-derived key |
| **RSQ/RST UDP obfs** | BLAKE2b-derived XOR stream (optional) |
| **QUIC / TLS** | TLS 1.3 (rustls); ALPN per protocol |
| **Hysteria2** | TLS 1.3 + QUIC; password auth |

There is **no** Shadowsocks AEAD, VMess body cipher, or VLESS/REALITY stack in this build.

---

## 5. Routing presets

`route.preset: "china-direct"` — China domains/IPs → `direct`, others → `route.final` proxy.

See [`examples/config-china-direct.json`](../examples/config-china-direct.json).

---

## 6. Removed types (will error)

These JSON `"type"` values are **not supported**:

`shadowsocks`, `vmess`, `vless`, `trojan`, `tuic`, `anytls`, `shadowtls`, `naive`, `hysteria`, `wireguard`, `ssh`, `tor`, `chain`, `socks`/`http` as **outbound**, `tailscale`, `redirect`, `tproxy`, …

If you need one of these, use upstream sing-box instead.

---

## 7. Services

Only `api` control service is recognized. Other service types are ignored or warned.

---

## 8. Security notes

1. Use strong unique passwords on RSQ/RST/Hy2.
2. Keep `tls.insecure: false` in production.
3. Open **UDP** to the server port for all three protocols.
4. RSQ/RST peers must run matching rsbox versions; RSQ is **not** Hysteria2-compatible.
