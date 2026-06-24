# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- CI/CD workflows for automated testing and building
- Comprehensive README.md documentation
- Contributing guidelines (CONTRIBUTING.md)
- .editorconfig for consistent code formatting
- .gitignore configuration

### Fixed
- Removed unnecessary lifetime parameters in rsb-config

## [0.1.0] - 2024-06-24

### Added
- Initial release
- 18 inbound protocol types
- 20 outbound protocol types
- uTLS fingerprinting support (Chrome/Firefox/Safari)
- REALITY protocol support (Xray compatible)
- XTLS Vision support (experimental)
- Tailscale native integration
  - Noise_IK handshake
  - HTTP/map fallback
  - Headscale support
- DERP relay server
  - Binary TCP frame protocol
  - TLS support
  - STUN integration
  - Mesh networking
- WireGuard endpoint support (boringtun)
- gRPC API service
  - Version/status endpoints
  - Outbound management
  - Connection tracking
  - Group operations (selector, urltest)
- HTTP API service
- DNS resolver service
- SSH tunnel support
- USB/IP support (experimental)
- Claude Chat Model (CCM) OAuth proxy
- OpenAI Chat Model (OCM) OAuth proxy
- Shadowsocks managed API (ssm-api)
- Hysteria realm NAT traversal

### Core Features
- Configuration parsing (sing-box JSON compatible)
- Rule-based routing engine
- DNS resolution with fake-ip support
- Connection tracking and statistics
- Process name detection
- Cross-platform support (Linux, Windows, macOS)
- Platform-specific route installation
  - Linux: RTNETLINK
  - Windows: CreateIpForwardEntry2
  - macOS: libproc integration

### Performance
- Async I/O with tokio runtime
- Zero-copy optimizations
- Memory footprint ~60% of Go implementation
- LTO and size optimization in release builds

### Testing
- 25+ unit and integration tests
- Protocol-specific test coverage
- Varint encoding/decoding tests
- Obfuscation roundtrip tests

### Documentation
- Architecture overview (ARCHITECTURE.md)
- Feature comparison with sing-box (FEATURES.md)
- Configuration examples

[Unreleased]: https://github.com/yourusername/rsbox/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/yourusername/rsbox/releases/tag/v0.1.0
