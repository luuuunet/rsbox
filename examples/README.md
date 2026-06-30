# 配置示例和最佳实践

本目录包含了 rsbox 的各种配置示例，涵盖常见使用场景。

## 📁 文件说明

| 文件 | 场景 | 说明 |
|------|------|------|
| [config-basic.json](../config.example.json) | 基础代理 | 简单的 HTTP/SOCKS5 代理 |
| [config-advanced.json](config-advanced.json) | 高级配置 | 完整功能展示（DNS、路由、API、选择器） |
| [config-tun.json](config-tun.json) | TUN 模式 | 透明代理（需要 root 权限） |
| [config-server.json](config-server.json) | 服务端 | Hysteria2 服务器配置 |
| [config-tailscale-derp.json](config-tailscale-derp.json) | Tailscale/DERP | 私有网络配置 |
| [config-reality.json](config-reality.json) | REALITY | VLESS + REALITY 配置 |
| [config-shadowsocks.json](config-shadowsocks.json) | Shadowsocks | SS 客户端/服务端 |
| [config-shadowtls-ss.json](config-shadowtls-ss.json) | ShadowTLS+SS | ST v3 + SS2022 客户端（G5 端口 17890） |
| [config-anytls.json](config-anytls.json) | AnyTLS | AnyTLS 客户端 |
| [config-test-shadowtls-ss-anytls.json](config-test-shadowtls-ss-anytls.json) | 协议联调 | 选择器切换 ST+SS / AnyTLS |
| [TEST-PROTOCOLS.md](TEST-PROTOCOLS.md) | 联调说明 | 占位符、服务端、测试脚本 |
| [config-routing.json](config-routing.json) | 智能路由 | 基于规则的流量分流 |

## 🚀 快速开始

### 1. 基础 HTTP/SOCKS5 代理

```bash
# 使用基础配置
cp config.example.json my-config.json
rsbox run -c my-config.json

# 测试代理
curl -x http://127.0.0.1:7890 https://www.google.com
```

### 2. TUN 透明代理（Linux/macOS）

```bash
# 需要 root 权限
sudo rsbox run -c examples/config-tun.json

# 所有流量将自动通过代理
curl https://www.google.com
```

### 3. 服务器部署

```bash
# 编辑服务器配置
vim examples/config-server.json

# 生成 TLS 证书
openssl req -x509 -newkey rsa:4096 -keyout key.pem -out cert.pem -days 365 -nodes

# 启动服务
rsbox run -c examples/config-server.json
```

### 4. 高级配置（选择器 + 自动测速）

```bash
# 使用高级配置（包含多节点和自动选择）
rsbox run -c examples/config-advanced.json

# 通过 API 查看节点状态
curl http://127.0.0.1:9090/outbounds
```

## 📝 配置结构说明

### 基本结构

```json
{
  "log": {},           // 日志配置
  "dns": {},           // DNS 配置
  "inbounds": [],      // 入站配置
  "outbounds": [],     // 出站配置
  "route": {},         // 路由规则
  "services": [],      // 服务配置
  "endpoints": [],     // 端点配置
  "experimental": {}   // 实验性功能
}
```

### 日志配置

```json
{
  "log": {
    "level": "info",           // 日志级别: debug, info, warn, error
    "output": "rsbox.log",     // 日志文件路径
    "timestamp": true          // 包含时间戳
  }
}
```

### DNS 配置

```json
{
  "dns": {
    "servers": [
      {
        "tag": "cloudflare",
        "address": "https://1.1.1.1/dns-query",  // DoH
        "detour": "proxy"                        // 通过哪个出站
      },
      {
        "tag": "local",
        "address": "223.5.5.5",                  // UDP DNS
        "detour": "direct"
      }
    ],
    "rules": [
      {
        "domain_suffix": [".cn"],
        "server": "local"
      }
    ],
    "final": "cloudflare",
    "strategy": "prefer_ipv4"
  }
}
```

### 入站类型

| 类型 | 说明 | 用途 |
|------|------|------|
| `mixed` | HTTP + SOCKS5 | 通用本地代理 |
| `http` | 仅 HTTP | HTTP 代理 |
| `socks` | 仅 SOCKS5 | SOCKS5 代理 |
| `tun` | TUN 设备 | 透明代理 |
| `shadowsocks` | Shadowsocks | SS 服务器 |
| `hysteria2` | Hysteria2 | HY2 服务器 |
| `trojan` | Trojan | Trojan 服务器 |
| `vless` | VLESS | VLESS 服务器 |

### 出站类型

| 类型 | 说明 | 用途 |
|------|------|------|
| `direct` | 直连 | 直接连接目标 |
| `block` | 阻断 | 拦截连接 |
| `dns` | DNS | DNS 查询 |
| `shadowsocks` | Shadowsocks | SS 客户端 |
| `hysteria2` | Hysteria2 | HY2 客户端 |
| `trojan` | Trojan | Trojan 客户端 |
| `vless` | VLESS | VLESS 客户端 |
| `vmess` | VMess | VMess 客户端 |
| `selector` | 手动选择 | 用户选择节点 |
| `urltest` | 自动测速 | 自动选最快节点 |

### 路由规则

```json
{
  "route": {
    "rules": [
      // DNS 流量
      {
        "protocol": "dns",
        "outbound": "dns-out"
      },
      
      // 私有 IP 直连
      {
        "geoip": ["private"],
        "outbound": "direct"
      },
      
      // 中国域名直连
      {
        "domain_suffix": [".cn"],
        "outbound": "direct"
      },
      
      // 中国 IP 直连
      {
        "geoip": ["cn"],
        "outbound": "direct"
      },
      
      // 特定端口阻断
      {
        "network": "udp",
        "port": 443,
        "outbound": "block"
      }
    ],
    "final": "proxy"  // 默认出站
  }
}
```

### 选择器和自动测速

```json
{
  "outbounds": [
    // 手动选择
    {
      "type": "selector",
      "tag": "proxy",
      "outbounds": ["auto", "node1", "node2"],
      "default": "auto"
    },
    
    // 自动测速
    {
      "type": "urltest",
      "tag": "auto",
      "outbounds": ["node1", "node2", "node3"],
      "url": "https://www.gstatic.com/generate_204",
      "interval": "5m",
      "tolerance": 50
    }
  ]
}
```

## 🔐 TLS 配置

### uTLS 指纹伪装

```json
{
  "tls": {
    "enabled": true,
    "server_name": "www.cloudflare.com",
    "utls": {
      "enabled": true,
      "fingerprint": "chrome"  // chrome, firefox, safari, ios, android, edge
    }
  }
}
```

### REALITY

```json
{
  "tls": {
    "enabled": true,
    "server_name": "www.microsoft.com",
    "utls": {
      "enabled": true,
      "fingerprint": "chrome"
    },
    "reality": {
      "enabled": true,
      "public_key": "your-server-public-key",
      "short_id": "0123456789abcdef"
    }
  }
}
```

## 🛠️ 服务和 API

### API 服务

```json
{
  "services": [
    {
      "type": "api",
      "listen": "127.0.0.1",
      "listen_port": 9090,
      "secret": "your-token"
    }
  ]
}
```

API 端点：
- `GET /version` - 版本信息
- `GET /outbounds` - 出站列表
- `GET /connections` - 当前连接
- `POST /connections/close` - 关闭所有连接
- `GET /stats` - 统计信息

### Clash API

```json
{
  "experimental": {
    "clash_api": {
      "external_controller": "127.0.0.1:9091",
      "secret": "your-secret"
    }
  }
}
```

兼容 Clash Dashboard 和第三方客户端。

## 💡 最佳实践

### 1. DNS 配置

- 使用 DoH (DNS over HTTPS) 防止 DNS 泄露
- 国内域名使用国内 DNS，国外域名使用国外 DNS
- 启用 DNS 分流避免污染

### 2. 路由规则

- 私有 IP 必须直连
- DNS 查询单独处理
- 中国 IP/域名直连节省流量
- 广告域名可以阻断

### 3. TUN 模式

- 需要 root/管理员权限
- 设置正确的路由表
- 避免路由循环
- 使用 `auto_route` 自动配置

### 4. 安全性

- 不要在配置文件中硬编码密码
- 使用环境变量或密钥管理
- TLS 证书定期更新
- 限制 API 访问地址

### 5. 性能优化

- 调整 MTU 值（TUN 模式）
- 使用 `urltest` 自动选择最快节点
- 启用连接复用
- 合理设置超时时间

## 🐛 故障排查

### 检查配置

```bash
rsbox check -c config.json
```

### 查看日志

```json
{
  "log": {
    "level": "debug",
    "output": "debug.log"
  }
}
```

### 测试连接

```bash
# HTTP 代理
curl -v -x http://127.0.0.1:7890 https://www.google.com

# SOCKS5 代理
curl -v -x socks5://127.0.0.1:7890 https://www.google.com
```

### 常见问题

1. **端口被占用**: 更换 `listen_port`
2. **TUN 模式权限不足**: 使用 `sudo` 或设置 capabilities
3. **DNS 解析失败**: 检查 DNS 服务器配置
4. **连接超时**: 检查防火墙和路由规则

## 📚 更多资源

- [完整文档](../README.md)
- [架构设计](../ARCHITECTURE.md)
- [功能特性](../FEATURES.md)
- [贡献指南](../CONTRIBUTING.md)

## 🙋 需要帮助？

- [GitHub Issues](https://github.com/yourusername/rsbox/issues)
- [GitHub Discussions](https://github.com/yourusername/rsbox/discussions)
