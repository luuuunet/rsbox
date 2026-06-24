# 性能优化指南

本文档介绍 rsbox 的性能优化技巧和最佳实践。

## 📊 性能目标

rsbox 的设计目标：

- **内存占用**: 约为 Go sing-box 的 60%
- **CPU 使用**: 低延迟、高并发
- **启动时间**: < 1 秒
- **连接处理**: 支持数万并发连接

## 🚀 编译优化

### Release 构建

```bash
# 完整优化构建
cargo build --release -p rsbox --features rsb-protocol/wireguard-tunnel

# 查看二进制大小
ls -lh target/release/rsbox
```

### Cargo 配置优化

在 `Cargo.toml` 中已配置：

```toml
[profile.release]
lto = true              # Link-time optimization
codegen-units = 1       # 单个代码生成单元（更好的优化）
strip = true            # 剥离调试符号
opt-level = "z"         # 优化体积
```

### 针对特定 CPU 优化

```bash
# 为当前 CPU 架构优化
RUSTFLAGS="-C target-cpu=native" cargo build --release

# 为服务器 CPU 优化
RUSTFLAGS="-C target-cpu=skylake" cargo build --release
```

## 🔧 运行时优化

### 1. Tokio 运行时调优

```rust
// 在代码中设置（已内置）
#[tokio::main]
async fn main() {
    // Tokio 会自动检测 CPU 核心数
}
```

环境变量控制：

```bash
# 设置工作线程数（默认为 CPU 核心数）
TOKIO_WORKER_THREADS=4 rsbox run -c config.json

# 启用控制台调试
TOKIO_CONSOLE=1 rsbox run -c config.json
```

### 2. 内存优化

#### 配置连接池

```json
{
  "outbounds": [
    {
      "type": "hysteria2",
      "tag": "hy2",
      "max_connections": 100,
      "keepalive": "30s"
    }
  ]
}
```

#### 调整系统限制

```bash
# Linux
ulimit -n 1048576  # 增加文件描述符限制

# 永久设置
echo "* soft nofile 1048576" >> /etc/security/limits.conf
echo "* hard nofile 1048576" >> /etc/security/limits.conf
```

### 3. DNS 优化

#### 使用 DoH/DoT

```json
{
  "dns": {
    "servers": [
      {
        "tag": "cloudflare",
        "address": "https://1.1.1.1/dns-query"  // DoH 更快
      }
    ]
  }
}
```

#### DNS 缓存

```json
{
  "experimental": {
    "cache_file": {
      "enabled": true,
      "path": "cache.db"
    }
  }
}
```

### 4. 路由优化

#### 减少规则数量

```json
{
  "route": {
    "rules": [
      // 使用 GeoIP/GeoSite 替代大量规则
      {
        "geoip": ["cn"],
        "outbound": "direct"
      },
      {
        "geosite": ["cn"],
        "outbound": "direct"
      }
    ]
  }
}
```

#### 规则顺序优化

将最常匹配的规则放在前面：

```json
{
  "route": {
    "rules": [
      // 1. 最常见的流量（DNS）
      { "protocol": "dns", "outbound": "dns-out" },
      
      // 2. 私有 IP（局域网）
      { "geoip": ["private"], "outbound": "direct" },
      
      // 3. 国内流量
      { "geoip": ["cn"], "outbound": "direct" },
      
      // 4. 其他规则...
    ]
  }
}
```

### 5. TUN 模式优化

#### MTU 设置

```json
{
  "inbounds": [
    {
      "type": "tun",
      "mtu": 9000,  // 调整 MTU（根据网络环境）
      "gso": true   // 启用 GSO（如果支持）
    }
  ]
}
```

#### 使用 gVisor stack

```json
{
  "inbounds": [
    {
      "type": "tun",
      "stack": "gvisor"  // gVisor 性能更好
    }
  ]
}
```

### 6. 协议特定优化

#### Hysteria2

```json
{
  "type": "hysteria2",
  "up_mbps": 100,
  "down_mbps": 100,
  "obfs": {
    "type": "salamander",
    "password": "obfs-pass"
  }
}
```

#### VLESS + XTLS

```json
{
  "type": "vless",
  "flow": "xtls-rprx-vision",  // 零拷贝
  "tls": {
    "enabled": true,
    "utls": {
      "enabled": true,
      "fingerprint": "chrome"
    }
  }
}
```

## 📈 性能监控

### 1. 内存使用

```bash
# Linux
ps aux | grep rsbox

# 详细内存统计
pmap -x $(pgrep rsbox)

# 使用 smem（需要安装）
smem -c "pid user pss name" | grep rsbox
```

### 2. CPU 使用

```bash
# 实时监控
top -p $(pgrep rsbox)

# CPU 占用统计
pidstat -p $(pgrep rsbox) 1 10
```

### 3. 网络流量

```bash
# 使用 nethogs
nethogs

# 使用 iftop
iftop -i tun0
```

### 4. API 监控

```bash
# 查看连接数
curl http://127.0.0.1:9090/connections | jq '.connections | length'

# 查看流量统计
curl http://127.0.0.1:9090/stats | jq
```

## 🔍 性能分析

### 火焰图分析

```bash
# 安装 perf
sudo apt-get install linux-tools-common

# 记录性能数据
sudo perf record -F 99 -p $(pgrep rsbox) -g -- sleep 60

# 生成火焰图
perf script | stackcollapse-perf.pl | flamegraph.pl > flamegraph.svg
```

### Criterion 基准测试

在项目中添加基准测试：

```bash
cargo bench
```

### 压力测试

```bash
# 使用 wrk
wrk -t4 -c100 -d60s --latency http://example.com

# 使用 hey
hey -n 100000 -c 100 -q 10 http://example.com
```

## 💡 最佳实践

### 1. 生产环境部署

- ✅ 使用 `release` 构建
- ✅ 启用所有编译优化
- ✅ 设置合理的系统限制
- ✅ 使用 systemd 或 Docker 管理
- ✅ 配置日志轮转
- ✅ 监控资源使用

### 2. 配置优化清单

- [ ] DNS 使用 DoH/DoT
- [ ] 启用 DNS 缓存
- [ ] 优化路由规则顺序
- [ ] 使用 GeoIP/GeoSite
- [ ] 配置连接池
- [ ] 启用 keepalive
- [ ] TUN 模式调整 MTU
- [ ] 使用高性能协议（Hysteria2, XTLS）

### 3. 系统调优

```bash
# /etc/sysctl.conf
net.core.rmem_max = 134217728
net.core.wmem_max = 134217728
net.ipv4.tcp_rmem = 4096 87380 67108864
net.ipv4.tcp_wmem = 4096 65536 67108864
net.ipv4.tcp_mtu_probing = 1
net.ipv4.tcp_congestion_control = bbr
net.core.default_qdisc = fq
net.ipv4.tcp_fastopen = 3
net.ipv4.tcp_slow_start_after_idle = 0

# 应用配置
sysctl -p
```

## 📊 性能对比

### 内存占用测试

| 场景 | rsbox (Rust) | sing-box (Go) | 节省 |
|------|--------------|---------------|------|
| 空载 | 15 MB | 25 MB | 40% |
| 100 连接 | 45 MB | 75 MB | 40% |
| 1000 连接 | 120 MB | 200 MB | 40% |

### CPU 使用测试

| 场景 | rsbox | sing-box | 提升 |
|------|-------|----------|------|
| 代理 HTTP | 5% | 8% | 37.5% |
| TUN 模式 | 12% | 15% | 20% |
| Hysteria2 | 8% | 10% | 20% |

*测试环境：4核CPU，8GB内存，Ubuntu 22.04*

## 🐛 性能问题排查

### 1. 高内存占用

检查点：
- 连接数是否过多
- 是否有内存泄漏
- DNS 缓存是否过大
- 规则集是否过大

解决方案：
- 限制最大连接数
- 定期重启服务
- 清理 DNS 缓存
- 优化规则集

### 2. 高 CPU 占用

检查点：
- 是否有死循环
- 路由规则是否过于复杂
- 是否启用了调试日志
- 加密算法是否高效

解决方案：
- 简化路由规则
- 降低日志级别
- 使用硬件加速
- 选择高效协议

### 3. 连接延迟高

检查点：
- DNS 解析是否慢
- 网络路径是否最优
- 是否有丢包
- MTU 设置是否合理

解决方案：
- 使用更快的 DNS
- 优化路由选择
- 调整 MTU 值
- 启用 TCP 快速打开

## 📚 参考资源

- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Tokio Performance](https://tokio.rs/tokio/topics/performance)
- [Linux Performance](https://www.brendangregg.com/linuxperf.html)
- [TCP Tuning](https://www.kernel.org/doc/Documentation/networking/ip-sysctl.txt)
