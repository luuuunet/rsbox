# rsbox

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/luuuunet/rsbox)](https://github.com/luuuunet/rsbox/releases)
[![Rust](https://img.shields.io/badge/rust-1.93%2B-orange.svg)](https://www.rust-lang.org)

**rsbox** is a high-performance proxy platform written in Rust. It uses a **sing-box–compatible JSON config**, with lower memory use than typical Go proxies, plus first-party protocols **RSQ** and **RST**.

> Independent project. Not affiliated with the sing-box / SagerNet maintainers.

## Features

- **Rust core** — async I/O, modest binary size, lower RAM than many Go stacks
- **Broad protocol coverage** — Shadowsocks, VMess, VLESS, Trojan, Hysteria1/2, TUIC, ShadowTLS, AnyTLS, WireGuard, and more
- **Native protocols** — [RSQ](docs/rsq-protocol.md) and RST (QUIC + TLS 1.3)
- **TLS tooling** — uTLS fingerprints, REALITY, optional XTLS Vision flow
- **Routing** — rule sets, geosite/geoip, China-direct preset
- **Platforms** — Windows / Linux / macOS desktop binaries; Android / iOS libbox builds via Releases

## Quick start

### Install

Download a binary from [Releases](https://github.com/luuuunet/rsbox/releases), or build from source:

```bash
# Rust 1.93+
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

cargo build --release -p rsbox
```

### Run

```bash
./target/release/rsbox run -c config.json
# or: rsbox.exe run -c config.json
```

### Minimal config

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
    { "type": "direct", "tag": "direct" }
  ],
  "route": { "final": "direct" }
}
```

Point your system / app proxy to `127.0.0.1:7890` (HTTP + SOCKS mixed).

More samples: [`examples/`](examples/) · overview: [`examples/README.md`](examples/README.md)

## Documentation

| Doc | Description |
|-----|-------------|
| **[Protocols & encryption](docs/PROTOCOLS.md)** | Supported inbound/outbound types, ciphers, TLS, transports |
| **[RSQ protocol](docs/rsq-protocol.md)** | Wire format, auth, obfuscation for RSQ |
| **[Examples](examples/README.md)** | Config recipes (SS, REALITY, China-direct, TUN, …) |

## Supported protocols (summary)

### Inbounds

`mixed`, `http`, `socks`, `direct`, `shadowsocks`, `vmess`, `vless`, `trojan`, `naive`, `shadowtls`, `anytls`, `hysteria`, `hysteria2`, `rsq`, `rst`, `tuic`, `tun`, `redirect`, `tproxy`, `dns`

### Outbounds

`direct`, `block`, `dns`, `socks`, `http`, `shadowsocks`, `vmess`, `vless`, `trojan`, `naive`, `shadowtls`, `anytls`, `hysteria`, `hysteria2`, `rsq`, `rst`, `tuic`, `wireguard`, `ssh`, `tor`, `selector`, `urltest`, `chain`

### Encryption & security (high level)

| Area | Supported |
|------|-----------|
| **Shadowsocks** | `aes-128-gcm`, `aes-256-gcm`, `chacha20-ietf-poly1305`, `2022-blake3-aes-128-gcm`, `2022-blake3-aes-256-gcm`, `2022-blake3-chacha20-poly1305` |
| **VMess** | `auto`, `aes-128-gcm`, `chacha20-poly1305`, `none` (AEAD) |
| **VLESS / Trojan** | UUID / password over TLS; VLESS `flow`: `xtls-rprx-vision` |
| **Hysteria2 / RSQ / RST / TUIC** | TLS 1.3 (+ QUIC); password / UUID auth; optional UDP obfuscation on RSQ/RST |
| **uTLS fingerprints** | `chrome`, `firefox`, `edge`, `safari`, `ios`, `random` |
| **REALITY** | VLESS + REALITY (`public_key` / `short_id`) |

Full tables, notes, and examples: **[docs/PROTOCOLS.md](docs/PROTOCOLS.md)**.

## Project layout

```
rsbox/
├── crates/
│   ├── rsb-constant/   # protocol type names & version
│   ├── rsb-config/     # sing-box–style JSON
│   ├── rsb-core/       # inbound / outbound / router traits
│   ├── rsb-protocol/   # protocol implementations
│   ├── rsb-route/      # routing & geosite/geoip
│   ├── rsb-dns/        # DNS
│   ├── rsb-api/        # control API
│   ├── rsb-wireguard/  # WireGuard / Tailscale pieces
│   └── rsb-libbox/     # mobile FFI (libbox)
├── rsbox/              # CLI binary
├── docs/               # protocol documentation
└── examples/           # sample configs
```

## Build & test

```bash
cargo build --release -p rsbox
cargo test --workspace
```

Optional WireGuard-related features may require extra system libs depending on target; see crate `Cargo.toml` files.

## Releases

GitHub Actions builds desktop and mobile artifacts on version tags (`v*`):

https://github.com/luuuunet/rsbox/releases

## Contributing

1. Fork and branch from `main`
2. `cargo fmt` / `cargo clippy` / add tests where useful
3. Open a pull request

## License

[GPL-3.0-or-later](LICENSE)

## Acknowledgements

- [sing-box](https://github.com/SagerNet/sing-box) — config shape and ecosystem reference
- [Xray-core](https://github.com/XTLS/Xray-core) — REALITY / XTLS concepts
- Shadowsocks, Hysteria, TUIC, and other open protocol communities

## Disclaimer

Use only where you are allowed to. You are responsible for complying with local law and service terms.
