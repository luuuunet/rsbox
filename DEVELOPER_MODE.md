# rsbox 开发者模式实现

## 版本
v1.0.0 - 2026年6月26日

## 🎯 开发者模式功能

开发者模式提供详细的调试信息，帮助快速定位问题。

---

## 📖 使用方法

### 1. 环境变量方式

```bash
# 启用开发者模式
export RSBOX_DEV_MODE=1

# 设置详细日志
export RUST_LOG=rsbox=trace,rsb_protocol=trace,rsb_core=debug

# 启用回溯
export RUST_BACKTRACE=full

# 启动 rsbox
rsbox run -c config.json
```

### 2. 配置文件方式

```json
{
  "log": {
    "level": "trace",
    "output": "rsbox-debug.log",
    "developer_mode": true
  },
  "debug": {
    "enable_backtrace": true,
    "dump_traffic": false,
    "trace_requests": true
  }
}
```

### 3. 命令行方式

```bash
# 使用 --dev 标志启动
rsbox run -c config.json --dev

# 指定日志级别
rsbox run -c config.json --log-level trace

# 启用流量转储
rsbox run -c config.json --dev --dump-traffic
```

---

## 🔍 日志级别说明

### TRACE
最详细的日志，包含每个步骤的详细信息。

**输出示例**：
```
TRACE rsbox::protocol::hysteria2: Sending authentication request
  server: example.com:443
  password_hash: sha256:abc123...
  up_mbps: 100
  
TRACE rsbox::protocol::hysteria2: Received authentication response
  status: 233
  duration_ms: 45
  udp_enabled: true
```

### DEBUG
调试信息，包含重要的中间状态。

**输出示例**：
```
DEBUG rsbox::route: Route decision
  domain: www.google.com
  rule: "proxy"
  outbound: "hy2-out"
  latency_ms: 2
```

### INFO
一般信息，记录重要事件。

**输出示例**：
```
INFO rsbox: Connection established
  protocol: hysteria2
  server: example.com:443
  session_id: 42
```

### WARN
警告信息，可能的问题。

**输出示例**：
```
WARN rsbox::protocol::hysteria2: Connection slow
  server: example.com
  rtt_ms: 500
  threshold_ms: 300
```

### ERROR
错误信息，操作失败。

**输出示例**：
```
ERROR rsbox::protocol::hysteria2: Authentication failed
  server: example.com:443
  status: 403
  error: "Invalid password"
  backtrace: ...
```

---

## 📊 日志输出格式

### 标准格式

```
2026-06-26T06:00:00.123Z INFO rsbox::protocol::hysteria2: Connection established
  server: example.com:443
  session_id: 42
  duration_ms: 123
```

### JSON 格式（机器可读）

```json
{
  "timestamp": "2026-06-26T06:00:00.123Z",
  "level": "INFO",
  "target": "rsbox::protocol::hysteria2",
  "message": "Connection established",
  "fields": {
    "server": "example.com:443",
    "session_id": 42,
    "duration_ms": 123
  }
}
```

---

## 🐛 调试场景

### 场景 1：连接失败

**启用详细日志**：
```bash
export RUST_LOG=rsb_protocol::hysteria2=trace
rsbox run -c config.json
```

**期望输出**：
```
TRACE rsbox::protocol::hysteria2: Starting connection
  server: example.com:443
  
TRACE rsbox::protocol::hysteria2: Resolving DNS
  hostname: example.com
  
DEBUG rsbox::protocol::hysteria2: DNS resolved
  addresses: ["1.2.3.4", "5.6.7.8"]
  duration_ms: 50
  
TRACE rsbox::protocol::hysteria2: Attempting QUIC connection
  address: 1.2.3.4:443
  
ERROR rsbox::protocol::hysteria2: QUIC connection failed
  address: 1.2.3.4:443
  error: "Connection timeout"
  errno: 110
  duration_ms: 5000
```

---

### 场景 2：认证失败

**启用详细日志**：
```bash
export RUST_LOG=rsb_protocol::hysteria2=trace
rsbox run -c config.json
```

**期望输出**：
```
INFO rsbox::protocol::hysteria2: QUIC connection established
  server: example.com:443
  duration_ms: 120
  
TRACE rsbox::protocol::hysteria2: Starting H3 handshake
  
DEBUG rsbox::protocol::hysteria2: H3 connection ready
  duration_ms: 30
  
TRACE rsbox::protocol::hysteria2: Sending auth request
  method: POST
  uri: https://hysteria/auth
  headers:
    hysteria-auth: [REDACTED]
    hysteria-cc-rx: 12500000
    
ERROR rsbox::protocol::hysteria2: Authentication failed
  status: 403
  error_body: "Invalid password"
  server: example.com:443
  
  Suggestion: Check your password in the configuration file
```

---

### 场景 3：路由问题

**启用详细日志**：
```bash
export RUST_LOG=rsbox::route=trace
rsbox run -c config.json
```

**期望输出**：
```
TRACE rsbox::route: Incoming request
  protocol: HTTP
  domain: www.google.com
  port: 443
  
TRACE rsbox::route: Evaluating rules
  total_rules: 5
  
DEBUG rsbox::route: Rule matched
  rule_index: 2
  rule_type: "domain_suffix"
  pattern: "google.com"
  outbound: "proxy"
  
DEBUG rsbox::route: Selecting outbound
  tag: "hy2-out"
  type: "hysteria2"
  
INFO rsbox::route: Route decision complete
  domain: www.google.com
  outbound: "hy2-out"
  duration_ms: 2
```

---

## 🔧 配置选项详解

### log.developer_mode

启用开发者模式，自动设置：
- 日志级别为 TRACE
- 启用颜色输出
- 显示文件和行号
- 启用回溯

### log.log_targets

精细控制各模块日志级别：

```json
{
  "log": {
    "log_targets": {
      "rsbox": "info",
      "rsb_protocol::hysteria2": "trace",
      "rsb_protocol::vmess": "debug",
      "rsb_core::platform": "debug",
      "rsb_route": "trace",
      "rsb_dns": "debug"
    }
  }
}
```

### debug.dump_traffic

启用流量转储（谨慎使用，会影响性能）：

```json
{
  "debug": {
    "dump_traffic": true,
    "dump_dir": "./debug",
    "dump_filters": {
      "protocols": ["hysteria2", "vmess"],
      "max_packet_size": 1024
    }
  }
}
```

### debug.trace_requests

为每个请求分配追踪 ID：

```
INFO rsbox: Request started [trace_id=abc123]
  domain: www.google.com
  
DEBUG rsbox::route: Route decision [trace_id=abc123]
  outbound: hy2-out
  
DEBUG rsbox::protocol::hysteria2: Opening stream [trace_id=abc123]
  
INFO rsbox: Request completed [trace_id=abc123]
  duration_ms: 234
  bytes_sent: 1234
  bytes_received: 5678
```

---

## 📈 性能追踪

### 启用性能追踪

```bash
export RSBOX_PERF_TRACE=1
rsbox run -c config.json
```

### 输出示例

```
PERF rsbox::protocol::hysteria2::connect: 123ms
  dns_resolve: 50ms
  quic_connect: 45ms
  h3_handshake: 20ms
  auth: 8ms
  
PERF rsbox::route::decide: 2ms
  rule_match: 1ms
  outbound_select: 1ms
  
PERF rsbox::request: 456ms
  route: 2ms
  connect: 123ms
  transfer: 331ms
```

---

## 🛠️ 调试工具

### 1. 连接测试

```bash
# 测试 Hysteria2 连接
rsbox test hysteria2 \
  --server example.com:443 \
  --password "your_password" \
  --log-level trace
```

### 2. 配置验证

```bash
# 验证配置文件
rsbox check config.json --dev
```

### 3. 协议分析

```bash
# 分析协议握手过程
rsbox analyze hysteria2 \
  --server example.com:443 \
  --dump-handshake
```

---

## 💡 最佳实践

### 1. 本地开发

```bash
# 使用详细日志和回溯
export RUST_LOG=trace
export RUST_BACKTRACE=full
cargo run -- run -c config.json
```

### 2. 问题排查

```bash
# 只关注特定协议
export RUST_LOG=rsb_protocol::hysteria2=trace
rsbox run -c config.json 2>&1 | tee debug.log
```

### 3. 性能分析

```bash
# 启用性能追踪
export RSBOX_PERF_TRACE=1
export RUST_LOG=info
rsbox run -c config.json
```

### 4. 生产环境调试

```bash
# 临时启用 DEBUG 级别
kill -USR1 <rsbox_pid>  # 提升日志级别
# ... 收集日志 ...
kill -USR2 <rsbox_pid>  # 恢复日志级别
```

---

## 🚨 注意事项

### 安全性

1. ⚠️ **不要在生产环境启用 TRACE 级别**
   - 可能泄露敏感信息（密码、证书）
   - 大量日志影响性能

2. ⚠️ **流量转储包含明文数据**
   - 仅用于开发和调试
   - 及时删除转储文件

3. ⚠️ **日志文件权限**
   - 设置适当的文件权限（600）
   - 定期清理日志

### 性能影响

| 功能 | 性能影响 | 建议 |
|------|---------|------|
| TRACE 日志 | 高 | 仅调试时使用 |
| 流量转储 | 极高 | 仅排查特定问题 |
| 性能追踪 | 中 | 可常开 |
| 请求追踪 | 低 | 可常开 |

---

## 📝 日志示例

### 完整的连接过程日志

```
2026-06-26T06:00:00.000Z INFO rsbox: Starting rsbox v0.1.0 (dev mode)
2026-06-26T06:00:00.001Z DEBUG rsbox::config: Loading configuration from config.json
2026-06-26T06:00:00.010Z INFO rsbox: Configuration loaded successfully
  inbounds: 1
  outbounds: 3
  routes: 10
  
2026-06-26T06:00:00.011Z INFO rsbox::inbound: Starting mixed inbound
  listen: 127.0.0.1:7890
  
2026-06-26T06:00:00.012Z INFO rsbox: rsbox started successfully
  
2026-06-26T06:00:05.123Z TRACE rsbox::inbound: New connection accepted
  client: 127.0.0.1:54321
  protocol: SOCKS5
  
2026-06-26T06:00:05.124Z TRACE rsbox::inbound::socks5: SOCKS5 handshake
  version: 5
  methods: [0]
  
2026-06-26T06:00:05.125Z DEBUG rsbox::inbound::socks5: SOCKS5 request
  command: CONNECT
  address: www.google.com:443
  
2026-06-26T06:00:05.126Z TRACE rsbox::route: Route decision started [trace_id=req_001]
  domain: www.google.com
  port: 443
  
2026-06-26T06:00:05.127Z DEBUG rsbox::route: Rule matched [trace_id=req_001]
  rule: "domain_suffix:google.com"
  outbound: "hy2-out"
  
2026-06-26T06:00:05.128Z TRACE rsbox::protocol::hysteria2: Getting connection [trace_id=req_001]
  server: example.com:443
  
2026-06-26T06:00:05.129Z DEBUG rsbox::protocol::hysteria2: Reusing existing connection [trace_id=req_001]
  session_id: 42
  age_ms: 30000
  
2026-06-26T06:00:05.130Z TRACE rsbox::protocol::hysteria2: Opening bidirectional stream [trace_id=req_001]
  
2026-06-26T06:00:05.135Z DEBUG rsbox::protocol::hysteria2: Stream opened [trace_id=req_001]
  stream_id: 7
  duration_ms: 5
  
2026-06-26T06:00:05.136Z TRACE rsbox::protocol::hysteria2: Sending target address [trace_id=req_001]
  target: www.google.com:443
  
2026-06-26T06:00:05.180Z DEBUG rsbox::protocol::hysteria2: Connection established [trace_id=req_001]
  duration_ms: 54
  
2026-06-26T06:00:05.181Z INFO rsbox: Request proxied successfully [trace_id=req_001]
  client: 127.0.0.1:54321
  target: www.google.com:443
  outbound: hy2-out
  duration_ms: 58
  
2026-06-26T06:00:10.456Z DEBUG rsbox: Connection closed [trace_id=req_001]
  bytes_sent: 1234
  bytes_received: 56789
  duration_s: 5.3
```

---

**文档版本**：v1.0.0  
**最后更新**：2026-06-26  
**状态**：待实施

---

**🎯 下一步：实施代码修改！**
