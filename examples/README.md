# Configuration examples

Sample JSON configs for **rsbox**. Field names follow the sing-box–compatible style.

For ciphers and protocol details see **[docs/PROTOCOLS.md](../docs/PROTOCOLS.md)**.

## Files

| File | Scenario |
|------|----------|
| [`../config.example.json`](../config.example.json) | Minimal mixed inbound |
| [`config-advanced.json`](config-advanced.json) | DNS, routing, API, selector |
| [`config-tun.json`](config-tun.json) | TUN (needs privileges) |
| [`config-server.json`](config-server.json) | Server-oriented sample |
| [`config-shadowsocks.json`](config-shadowsocks.json) | Shadowsocks AEAD / 2022 |
| [`config-shadowtls-ss.json`](config-shadowtls-ss.json) | ShadowTLS + SS2022 client |
| [`config-anytls.json`](config-anytls.json) | AnyTLS client |
| [`config-reality.json`](config-reality.json) | VLESS + REALITY + uTLS |
| [`config-routing.json`](config-routing.json) | Rule-based routing |
| [`config-china-direct.json`](config-china-direct.json) | `route.preset: china-direct` |
| [`config-tailscale-derp.json`](config-tailscale-derp.json) | Tailscale / DERP-related |
| [`rsq-client.json`](rsq-client.json) | RSQ client |
| [`RSQ-LOCAL.md`](RSQ-LOCAL.md) | Local RSQ lab notes |

Server-side samples may live under [`server/`](server/).

## Quick start

```bash
# From repo root
cp config.example.json my-config.json
# edit my-config.json — set outbound server / password / UUID

cargo run -p rsbox --release -- run -c my-config.json
# or: rsbox run -c my-config.json

curl -x http://127.0.0.1:7890 https://www.example.com
```

### TUN (Linux / macOS)

```bash
sudo rsbox run -c examples/config-tun.json
```

### China direct + proxy final

```bash
rsbox run -c examples/config-china-direct.json
```

## Config skeleton

```json
{
  "log": {},
  "dns": {},
  "inbounds": [],
  "outbounds": [],
  "route": {},
  "services": [],
  "endpoints": [],
  "experimental": {}
}
```

### Log

```json
{
  "log": {
    "level": "info",
    "timestamp": true
  }
}
```

Levels: `trace`, `debug`, `info`, `warn`, `error`.

### Shadowsocks outbound

```json
{
  "type": "shadowsocks",
  "tag": "ss-out",
  "server": "example.com",
  "server_port": 8388,
  "method": "aes-256-gcm",
  "password": "your-password"
}
```

Supported methods: see [PROTOCOLS.md § Shadowsocks](../docs/PROTOCOLS.md#2-shadowsocks-encryption-methods).

### VLESS + REALITY

See [`config-reality.json`](config-reality.json). Use a real `uuid`, `public_key`, and `short_id` from your server.

### RSQ

See [`rsq-client.json`](rsq-client.json) and [docs/rsq-protocol.md](../docs/rsq-protocol.md). RSQ is **not** compatible with Hysteria2 peers.

## Tips

1. Start with `mixed` on `127.0.0.1` only.
2. Put secrets in env / private files; do not commit real passwords.
3. After editing geo / route rules, restart rsbox (hot reload does not always rebuild the full router).
4. Prefer AEAD / SS2022 / REALITY / Hy2 / RSQ over obsolete stream ciphers.

## License

Same as the main project: GPL-3.0-or-later.
