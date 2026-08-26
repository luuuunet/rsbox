# rsbox

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/luuuunet/rsbox)](https://github.com/luuuunet/rsbox/releases)
[![Rust](https://img.shields.io/badge/rust-1.93%2B-orange.svg)](https://www.rust-lang.org)

**rsbox** is a Rust proxy platform focused on three data-plane protocols:

| Protocol | Transport | Notes |
|----------|-----------|--------|
| **RSQ** | QUIC + TLS 1.3 | Native rsbox protocol — see [docs/rsq-protocol.md](docs/rsq-protocol.md) |
| **RST** | QUIC + TLS 1.3 | Native rsbox protocol (HTTP/3 auth plane) |
| **Hysteria2** | QUIC + TLS 1.3 | sing-box–compatible Hy2 client/server |

Local control uses sing-box–style JSON: `mixed` inbound, `direct` / `selector` / `urltest` routing, optional `tun`.

> Independent project. Not affiliated with sing-box / SagerNet.

## Features

- **Three proxy protocols only** — RSQ, RST, Hysteria2 (no SS/VMess/VLESS/Trojan/TUIC/…)
- **Low memory** — Rust async core
- **Smart routing** — geosite/geoip, China-direct preset
- **Cross-platform** — Windows / Linux / macOS CLI; Android / iOS libbox builds

## Quick start

```bash
cargo build --release -p rsbox
./target/release/rsbox run -c config.json
```

### Minimal client config

```json
{
  "log": { "level": "info", "timestamp": true },
  "inbounds": [
    {
      "type": "mixed",
      "tag": "mixed-in",
      "listen": "127.0.0.1",
      "listen_port": 7890
    }
  ],
  "outbounds": [
    {
      "type": "rsq",
      "tag": "proxy",
      "server": "example.com",
      "server_port": 443,
      "password": "change-me",
      "tls": { "enabled": true, "server_name": "example.com", "insecure": true }
    },
    { "type": "direct", "tag": "direct" }
  ],
  "route": { "final": "proxy" }
}
```

Swap `"type": "rsq"` for `"rst"` or `"hysteria2"` as needed.

## Supported types

### Proxy inbounds / outbounds

- `rsq`
- `rst`
- `hysteria2`

### Infrastructure (not encrypted tunnels)

| Inbound | Outbound |
|---------|----------|
| `mixed`, `http`, `socks` | `direct`, `block`, `dns` |
| `direct`, `dns`, `tun` | `selector`, `urltest` |

Anything else in config (`shadowsocks`, `vless`, `vmess`, `trojan`, `tuic`, `wireguard`, …) is **rejected** at build time.

## Documentation

| Doc | Description |
|-----|-------------|
| [Protocols & encryption](docs/PROTOCOLS.md) | Supported types and crypto |
| [RSQ wire format](docs/rsq-protocol.md) | RSQ auth, frames, obfs |
| [Examples](examples/README.md) | Sample configs |

## Project layout

```
rsbox/
├── crates/rsb-protocol/   # RSQ, RST, Hy2 + mixed inbound
├── crates/rsb-route/      # routing / geo
├── crates/rsb-dns/        # DNS
└── rsbox/                 # CLI
```

## Build & test

```bash
cargo build --release -p rsbox
cargo test --workspace
```

## Releases

https://github.com/luuuunet/rsbox/releases

## License

[GPL-3.0-or-later](LICENSE)

## Disclaimer

Use only where permitted. You are responsible for compliance with local law.
