# rsbox 功能测试计划

## 测试目标
验证 README 中宣传的所有核心特性

---

## 🦀 测试 1：纯 Rust 实现 + 内存占用

### 测试方法
1. 启动 rsbox 服务
2. 使用 `tasklist` (Windows) 或 `ps` (Linux) 监控内存
3. 进行代理连接测试
4. 对比 Go 版本 sing-box（如有）

### 测试脚本
```bash
# 启动服务并记录内存
./target/release/rsbox run -c test_config.json &
PID=$!
sleep 5
# Windows
tasklist /FI "PID eq $PID" /FO TABLE
# Linux
ps -p $PID -o pid,vsz,rss,comm
```

### 预期结果
- ✅ 程序成功启动
- ✅ 内存占用合理（基础配置 < 50MB）
- ⚠️ 需要实际对比 Go 版本来验证"60%"的说法

---

## 🔌 测试 2：协议丰富性

### 入站协议测试 (18种)

#### 基础协议
- [ ] `direct` - 直连入站
- [ ] `mixed` - 混合代理（HTTP+SOCKS）
- [ ] `http` - HTTP 代理
- [ ] `socks` - SOCKS5 代理

#### 加密协议
- [ ] `shadowsocks` - SS 协议
- [ ] `vmess` - VMess 协议
- [ ] `vless` - VLESS 协议
- [ ] `trojan` - Trojan 协议

#### 现代协议
- [ ] `hysteria` - Hysteria v1
- [ ] `hysteria2` - Hysteria v2
- [ ] `tuic` - TUIC 协议

#### 特殊协议
- [ ] `shadowtls` - ShadowTLS
- [ ] `naive` - NaïveProxy
- [ ] `tun` - TUN 模式
- [ ] `redirect` - 透明代理
- [ ] `tproxy` - TPROXY
- [ ] `wireguard` - WireGuard
- [ ] `dns` - DNS 入站

### 出站协议测试 (20种)

#### 基础
- [ ] `direct` - 直连
- [ ] `block` - 阻断
- [ ] `dns` - DNS 查询

#### 代理协议（同入站）
- [ ] `shadowsocks`, `vmess`, `vless`, `trojan`
- [ ] `hysteria`, `hysteria2`, `tuic`
- [ ] `http`, `socks`

#### 高级
- [ ] `selector` - 手动选择
- [ ] `urltest` - 自动测速选择
- [ ] `wireguard` - WireGuard 出站
- [ ] `tailscale` - Tailscale 集成
- [ ] `ssh` - SSH 隧道

### 测试配置模板
```json
{
  "inbounds": [
    {"type": "mixed", "listen": "127.0.0.1", "listen_port": 17890}
  ],
  "outbounds": [
    {"type": "direct", "tag": "direct"},
    {"type": "block", "tag": "block"}
  ]
}
```

---

## 🔐 测试 3：安全传输特性

### uTLS 测试
```json
{
  "outbounds": [{
    "type": "vless",
    "server": "example.com",
    "server_port": 443,
    "uuid": "xxx",
    "tls": {
      "enabled": true,
      "utls": {
        "enabled": true,
        "fingerprint": "chrome"
      }
    }
  }]
}
```

**测试项**：
- [ ] Chrome 指纹
- [ ] Firefox 指纹
- [ ] Safari 指纹
- [ ] 随机指纹

### REALITY 测试
```json
{
  "outbounds": [{
    "type": "vless",
    "tls": {
      "enabled": true,
      "reality": {
        "enabled": true,
        "public_key": "xxx",
        "short_id": "xxx"
      }
    }
  }]
}
```

**测试项**：
- [ ] REALITY 握手成功
- [ ] 与 Xray 服务端互通

### XTLS Vision 测试
```json
{
  "outbounds": [{
    "type": "vless",
    "flow": "xtls-rprx-vision",
    "tls": {"enabled": true}
  }]
}
```

**测试项**：
- [ ] Vision 流控启用
- [ ] 零拷贝优化生效
- ⚠️ 需要 Xray 服务端联调

---

## 🌐 测试 4：高级功能

### Tailscale 测试
```json
{
  "endpoints": [{
    "type": "tailscale",
    "auth_key": "tskey-auth-xxx",
    "control_url": "https://controlplane.tailscale.com"
  }]
}
```

**测试项**：
- [ ] Noise 协议握手
- [ ] 控制面注册成功
- [ ] 节点发现
- ⚠️ 需要真实 Tailscale auth key

### WireGuard 测试
```json
{
  "outbounds": [{
    "type": "wireguard",
    "local_address": ["10.0.0.2/32"],
    "private_key": "xxx",
    "peer_public_key": "xxx",
    "server": "xxx",
    "server_port": 51820
  }]
}
```

**测试项**：
- [ ] WireGuard 握手
- [ ] 隧道建立
- [ ] 流量转发
- ⚠️ 需要 WireGuard 服务端

### DERP 中继测试
```json
{
  "services": [{
    "type": "derp",
    "listen": "127.0.0.1",
    "listen_port": 3478
  }]
}
```

**测试项**：
- [ ] DERP 服务启动
- [ ] 客户端连接
- [ ] 流量中继

### gRPC API 测试
```json
{
  "services": [{
    "type": "api",
    "listen": "127.0.0.1",
    "listen_port": 9090,
    "grpc_listen_port": 9091
  }]
}
```

**测试项**：
- [ ] gRPC 服务启动
- [ ] GetVersion 调用
- [ ] ListOutbounds 调用
- [ ] GetConnections 调用

---

## ⚡ 测试 5：高性能特性

### 零拷贝测试
- [ ] XTLS Vision 零拷贝
- [ ] Splice 系统调用（Linux）
- ⚠️ 需要性能分析工具

### 异步 I/O 测试
- [ ] Tokio 运行时正常
- [ ] 并发连接处理
- [ ] 无阻塞操作

### 内存效率测试
```bash
# 基准测试
cargo bench
# 或使用压测工具
ab -n 10000 -c 100 http://127.0.0.1:17890/
```

---

## 📦 测试 6：模块化设计

### 按需编译测试
```bash
# 最小化构建（无 WireGuard）
cargo build --release -p rsbox --no-default-features

# 完整构建
cargo build --release -p rsbox --features rsb-protocol/wireguard-tunnel

# 检查二进制大小差异
ls -lh target/release/rsbox
```

### Workspace 结构验证
```bash
# 独立编译各 crate
cargo build -p rsb-config
cargo build -p rsb-core
cargo build -p rsb-protocol
# ... 等
```

---

## 📊 测试结果记录

### 可以立即测试的项目
1. ✅ 基础功能（版本、配置检查）
2. ✅ 简单协议（direct, mixed, block）
3. ✅ 模块化编译
4. ✅ 内存占用基准

### 需要外部服务的项目
1. ⚠️ 完整协议测试（需要各协议服务端）
2. ⚠️ Tailscale（需要 auth key）
3. ⚠️ WireGuard（需要服务端）
4. ⚠️ REALITY/XTLS（需要 Xray 服务端）

### 需要压测工具的项目
1. ⚠️ 性能对比
2. ⚠️ 内存占用对比
3. ⚠️ 并发连接测试

---

## 🔧 自动化测试脚本

### 基础功能测试
```bash
#!/bin/bash
echo "=== rsbox 功能测试 ==="

# 1. 版本检查
echo "1. 版本检查..."
./target/release/rsbox version || exit 1

# 2. 配置检查
echo "2. 配置检查..."
./target/release/rsbox check -c test_config.json || exit 1

# 3. 启动测试（5秒后停止）
echo "3. 启动测试..."
timeout 5 ./target/release/rsbox run -c test_config.json &
PID=$!
sleep 2
if ps -p $PID > /dev/null; then
    echo "✅ 服务启动成功"
    kill $PID
else
    echo "❌ 服务启动失败"
    exit 1
fi

# 4. 内存占用测试
echo "4. 内存占用测试..."
./target/release/rsbox run -c test_config.json &
PID=$!
sleep 2
MEM=$(ps -p $PID -o rss= | awk '{print $1/1024 " MB"}')
echo "内存占用: $MEM"
kill $PID

echo "=== 基础测试完成 ==="
```

---

## 下一步行动

1. **立即执行**：基础功能测试
2. **准备环境**：设置各协议测试服务端
3. **性能测试**：使用 criterion 或 ab 工具
4. **完善功能**：根据测试结果修复问题

**准备好开始测试了吗？**
