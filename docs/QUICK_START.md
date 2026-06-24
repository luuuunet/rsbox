# rsbox 快速开始指南

本文档帮助您快速上手 rsbox。

## 📦 安装

### 从源码构建

**前置要求**:
- Rust 1.93+ ([安装 Rust](https://rustup.rs/))

```bash
# 克隆仓库
git clone https://github.com/yourusername/rsbox.git
cd rsbox

# 完整构建（包含所有功能）
cargo build --release -p rsbox --features rsb-protocol/wireguard-tunnel

# 或使用 Makefile
make build-release

# 二进制文件位于
./target/release/rsbox
```

### 最小化构建

如果不需要 WireGuard 功能，可以构建更小的二进制文件：

```bash
cargo build --release -p rsbox --no-default-features
make build-minimal
```

## ⚙️ 基础配置

### 1. HTTP/SOCKS5 代理（本地）

创建 `config.json`:

```json
{
  "log": {
    "level": "info"
  },
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
      "type": "direct",
      "tag": "direct"
    }
  ],
  "route": {
    "final": "direct"
  }
}
```

启动：
```bash
./target/release/rsbox run -c config.json
```

测试：
```bash
# HTTP 代理
curl -x http://127.0.0.1:7890 https://www.google.com

# SOCKS5 代理
curl -x socks5://127.0.0.1:7890 https://www.google.com
```

### 2. Shadowsocks 客户端

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
    {
      "type": "shadowsocks",
      "tag": "ss-out",
      "server": "your-server.com",
      "server_port": 8388,
      "method": "chacha20-ietf-poly1305",
      "password": "your-password"
    }
  ],
  "route": {
    "final": "ss-out"
  }
}
```

### 3. 智能路由（国内直连）

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
    { "type": "shadowsocks", "tag": "proxy", "server": "...", "server_port": 8388, "method": "chacha20-ietf-poly1305", "password": "..." },
    { "type": "direct", "tag": "direct" },
    { "type": "block", "tag": "block" }
  ],
  "route": {
    "rules": [
      { "domain_suffix": [".cn", ".中国"], "outbound": "direct" },
      { "geoip": ["cn", "private"], "outbound": "direct" },
      { "domain_keyword": ["baidu", "taobao", "qq"], "outbound": "direct" },
      { "domain": ["lan", "localhost"], "outbound": "direct" }
    ],
    "final": "proxy"
  }
}
```

### 4. VLESS + REALITY（高级）

```json
{
  "outbounds": [
    {
      "type": "vless",
      "tag": "reality-out",
      "server": "1.2.3.4",
      "server_port": 443,
      "uuid": "00000000-0000-0000-0000-000000000001",
      "flow": "xtls-rprx-vision",
      "tls": {
        "enabled": true,
        "utls": {
          "enabled": true,
          "fingerprint": "chrome"
        },
        "reality": {
          "enabled": true,
          "public_key": "YOUR_PUBLIC_KEY",
          "short_id": "0123456789abcdef"
        },
        "server_name": "www.cloudflare.com"
      }
    }
  ]
}
```

## 🔧 常用命令

```bash
# 启动（前台）
rsbox run -c config.json

# 检查配置
rsbox check -c config.json

# 查看版本
rsbox version

# 查看帮助
rsbox --help
```

## 📊 API 控制

启用 API 服务：

```json
{
  "services": [
    {
      "type": "api",
      "listen": "127.0.0.1",
      "listen_port": 9090,
      "secret": "your-secret-token"
    }
  ]
}
```

使用 API：

```bash
# 获取版本
curl http://127.0.0.1:9090/version

# 查看出站列表
curl http://127.0.0.1:9090/outbounds

# 查看连接
curl http://127.0.0.1:9090/connections

# 关闭所有连接
curl -X POST http://127.0.0.1:9090/connections/close
```

## 🐛 故障排查

### 1. 查看日志

```json
{
  "log": {
    "level": "debug",
    "output": "/var/log/rsbox.log"
  }
}
```

### 2. 测试连接

```bash
# 测试代理是否工作
curl -v -x http://127.0.0.1:7890 https://www.google.com

# 测试 DNS
dig @127.0.0.1 -p 53 google.com
```

### 3. 常见问题

**端口被占用**:
```bash
# Linux/macOS
lsof -i :7890

# Windows
netstat -ano | findstr :7890
```

**权限问题**（TUN 模式）:
```bash
# Linux
sudo setcap cap_net_admin+ep ./target/release/rsbox

# 或使用 sudo 运行
sudo ./target/release/rsbox run -c config.json
```

## 📚 下一步

- 查看 [完整配置示例](../config.example.json)
- 阅读 [功能对照表](../FEATURES.md) 了解所有支持的协议
- 查看 [架构文档](../ARCHITECTURE.md) 了解内部结构

## 💡 提示

1. **生产环境**: 建议先在测试环境充分压测
2. **配置备份**: 定期备份配置文件
3. **日志监控**: 启用日志并监控异常
4. **版本更新**: 关注 [Releases](https://github.com/yourusername/rsbox/releases)

需要帮助？查看 [Issues](https://github.com/yourusername/rsbox/issues) 或提问！
