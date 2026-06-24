# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

We take security vulnerabilities seriously. If you discover a security issue, please follow these steps:

### 1. **Do Not** Open a Public Issue

Please do not open a public GitHub issue for security vulnerabilities, as this could put users at risk.

### 2. Report Privately

**Preferred method**: Use GitHub's private vulnerability reporting feature:
- Go to the Security tab
- Click "Report a vulnerability"
- Fill out the advisory form

**Alternative method**: Email security report to:
- Email: [your-email@example.com]
- Subject: "rsbox Security Vulnerability"

### 3. Include Details

Please include as much information as possible:

- **Type of vulnerability** (e.g., buffer overflow, injection, authentication bypass)
- **Affected component** (e.g., specific protocol implementation, TLS handling)
- **Steps to reproduce** with minimal example
- **Potential impact** (e.g., remote code execution, information disclosure)
- **Suggested fix** (if you have one)
- **Your environment** (OS, Rust version, rsbox version)

### 4. Response Timeline

- **Initial response**: Within 48 hours
- **Status update**: Within 7 days
- **Fix timeline**: Depends on severity
  - Critical: 1-7 days
  - High: 7-30 days
  - Medium: 30-90 days
  - Low: 90+ days or next release

### 5. Disclosure Policy

- We follow **coordinated disclosure**
- Security advisories will be published after a fix is available
- Credit will be given to reporters (unless anonymity is requested)

## Security Considerations

### Network Proxy Security

rsbox is a network proxy tool. Please be aware:

1. **Traffic Visibility**: Proxy servers can see unencrypted traffic
2. **Trust Model**: Only use trusted proxy servers
3. **Configuration Secrets**: Protect config files containing credentials
4. **TLS Verification**: Enable certificate verification in production

### Known Limitations

- **XTLS Vision**: Still in experimental validation phase
- **usbip**: Experimental, not recommended for sensitive devices
- **OAuth tokens**: Local storage without encryption (use OS-level protection)

### Best Practices

1. **Keep Updated**: Regularly update to the latest version
2. **Minimal Permissions**: Run with least privileges necessary
3. **Config Protection**: Secure config files (chmod 600)
4. **Audit Logs**: Enable logging for security monitoring
5. **Network Isolation**: Use firewall rules to restrict access

## Security Features

### Implemented

- ✅ TLS 1.3 support via rustls
- ✅ Certificate validation
- ✅ Modern cipher suites only
- ✅ Memory-safe Rust implementation
- ✅ Input validation on all parsers
- ✅ No unsafe blocks in critical paths

### Planned

- 🔄 Config file encryption
- 🔄 Token secure storage (OS keychain integration)
- 🔄 Audit logging framework
- 🔄 Rate limiting and DoS protection

## Dependencies

We monitor our dependencies for vulnerabilities:

- **cargo-audit**: Automated security audits in CI
- **RustSec Advisory Database**: Daily checks
- **Dependabot**: Automated dependency updates

Run security audit yourself:
```bash
cargo install cargo-audit
cargo audit
```

## Bug Bounty

We currently do not offer a bug bounty program, but we deeply appreciate security researchers' efforts and will:

- Give public credit (if desired)
- Prioritize fixes for reported issues
- Keep you informed of fix progress

---

Thank you for helping keep rsbox and its users safe! 🔒
