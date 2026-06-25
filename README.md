# rsbox

[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)
[![Rust Version](https://img.shields.io/badge/rust-1.93%2B-orange.svg)](https://www.rust-lang.org)

**rsbox** 是 [sing-box](https://github.com/SagerNet/sing-box) 的 Rust 重写版本，专注于高性能、低内存占用的网络代理解决方案。

## ✨ 特性

- 🦀 **纯 Rust 实现** - 内存占用约为 Go 版本的 60%
- 🔌 **协议丰富** - 支持 18 种入站协议 + 20 种出站协议
- 🔐 **安全传输** - 内置 uTLS、REALITY、XTLS Vision 支持
- 🌐 **高级功能** - Tailscale、WireGuard、DERP、gRPC API
- ⚡ **高性能** - 零拷贝、异步 I/O、内存高效
- 📦 **模块化设计** - Workspace 架构，可按需裁剪

## 🚀 快速开始

### 安装依赖

**Rust 工具链**（需要 1.93+）：
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### 构建

```bash
# 完整功能构建（包含 WireGuard）
cargo build --release -p rsbox --features rsb-protocol/wireguard-tunnel

# 最小化构建（不含 WireGuard）
cargo build --release -p rsbox --no-default-features
```

### 运行

```bash
./target/release/rsbox run -c config.json
```

### 配置示例

创建 `config.json`：
```json
{
  "log": { "level": "info" },
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
  "route": {
    "final": "direct"
  }
}
```

配置格式与 **sing-box 官方 JSON 完全兼容**。

## 📚 文档

- [架构设计](ARCHITECTURE.md) - 核心架构、注册表、数据流
- [功能对照表](FEATURES.md) - 与 sing-box 的完整对照
- [配置示例](config.example.json) - 基础配置

## 🔧 支持的协议

### 入站协议 (18 种)
`direct`, `mixed`, `http`, `socks`, `shadowsocks`, `vmess`, `trojan`, `naive`, `hysteria`, `hysteria2`, `tuic`, `shadowtls`, `vless`, `tun`, `redirect`, `tproxy`, `wireguard`, `dns`

### 出站协议 (20 种)
`direct`, `block`, `dns`, `shadowsocks`, `vmess`, `trojan`, `wireguard`, `hysteria`, `hysteria2`, `shadowsocksr`, `vless`, `tuic`, `hysteria-realm`, `ssh`, `http`, `socks`, `selector`, `urltest`, `tailscale`, `usbip`

### 传输层特性
- ✅ **uTLS 指纹伪装** - Chrome/Firefox/Safari
- ✅ **REALITY** - Xray 兼容的流量伪装
- 🚧 **XTLS Vision** - 零拷贝直连

## 🏗️ 项目结构

```
rsbox/
├── crates/
│   ├── rsb-constant/    # 常量定义
│   ├── rsb-config/      # 配置解析（sing-box JSON）
│   ├── rsb-core/        # 核心抽象（Inbound/Outbound/Router traits）
│   ├── rsb-protocol/    # 协议实现（入站/出站/服务）
│   ├── rsb-route/       # 路由规则引擎
│   ├── rsb-dns/         # DNS 解析器
│   ├── rsb-wireguard/   # WireGuard/Tailscale 实现
│   ├── rsb-api/         # API 服务（Clash/V2Ray/Cache）
│   └── rsb-libbox/      # C FFI 绑定（libbox 兼容）
└── rsbox/               # CLI 入口
```

## 🧪 测试

```bash
# 运行所有测试
cargo test --workspace

# 运行特定模块测试
cargo test -p rsb-protocol
```

## 🎯 性能对比

| 指标 | rsbox (Rust) | sing-box (Go) |
|------|--------------|---------------|
| 内存占用 | ~60% | 100% (基线) |
| 启动时间 | 更快 | 基线 |
| 二进制体积 | ~30MB | ~50MB |

*实际性能取决于协议组合和工作负载，建议自行压测。*

## 🔬 实验性功能

### Tailscale 原生支持
```json
{
  "endpoints": [
    {
      "type": "tailscale",
      "auth_key": "tskey-auth-xxx",
      "control_url": "https://headscale.example.com"
    }
  ]
}
```

### DERP 中继服务器
```json
{
  "services": [
    {
      "type": "derp",
      "listen": "0.0.0.0",
      "listen_port": 443,
      "tls": { "enabled": true }
    }
  ]
}
```

### gRPC 控制 API
```json
{
  "services": [
    {
      "type": "api",
      "listen": "127.0.0.1",
      "listen_port": 9090,
      "grpc_listen_port": 9091
    }
  ]
}
```

## ⚠️ 生产环境注意事项

**当前状态**: 版本 0.1.0，核心功能可用，但建议在生产环境部署前：

1. **充分压测** - 针对你的协议组合进行负载测试
2. **联调验证** - XTLS Vision 需要与 Xray 对端联调
3. **监控观察** - 建议先小规模部署并监控
4. **备份方案** - 保持 Go 版本作为回退选项

## 🤝 贡献

欢迎贡献！请遵循以下步骤：

1. Fork 本仓库
2. 创建特性分支 (`git checkout -b feature/amazing-feature`)
3. 提交更改 (`git commit -m 'Add amazing feature'`)
4. 推送到分支 (`git push origin feature/amazing-feature`)
5. 开启 Pull Request

### 代码规范

- 运行 `cargo fmt` 格式化代码
- 运行 `cargo clippy` 检查代码质量
- 添加测试覆盖新功能

## 📄 许可证

本项目采用 [GPL-3.0-or-later](LICENSE) 许可证。

## 🙏 致谢

- [sing-box](https://github.com/SagerNet/sing-box) - 原始项目和设计灵感
- [Xray-core](https://github.com/XTLS/Xray-core) - REALITY 和 XTLS 协议
- [Tailscale](https://tailscale.com) - WireGuard 和 DERP 协议参考

## 📞 联系

- Issues: [GitHub Issues](https://github.com/yourusername/rsbox/issues)
- Discussions: [GitHub Discussions](https://github.com/yourusername/rsbox/discussions)

---

**注意**: rsbox 是一个独立的重写项目，与 sing-box 官方无关联关系。
