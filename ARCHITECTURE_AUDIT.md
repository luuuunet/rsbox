# rsbox 架构审核报告

## 审核时间
2026年6月26日 06:00

## 🔍 架构审核结果

---

## 📋 整体架构评估

### ⭐ 评级：B+ (良好，有改进空间)

### 优点 ✅
1. ✅ 模块化设计清晰
2. ✅ 依赖关系合理
3. ✅ 协议实现标准
4. ✅ 异步架构正确

### 问题 ⚠️
1. ⚠️ 错误处理不够细致
2. ⚠️ 日志记录不够详细
3. ⚠️ 调试信息不足
4. ⚠️ 错误追踪困难

---

## 🏗️ 架构层次分析

### 1. 核心层 (rsb-core)

**职责**：基础功能、平台抽象

**问题**：
- ⚠️ 平台相关代码错误处理简单
- ⚠️ 日志级别使用不当
- ⚠️ 缺少详细的错误上下文

**建议**：
```rust
// 当前
anyhow::bail!("netlink socket failed");

// 改进
anyhow::bail!("Failed to create netlink socket: errno={}", 
    std::io::Error::last_os_error());
```

---

### 2. 协议层 (rsb-protocol)

**职责**：协议实现、连接管理

**问题**：
- ⚠️ Hysteria2 错误信息不够详细
- ⚠️ VMess 调试信息缺失
- ⚠️ VLESS 连接状态追踪不足
- ⚠️ 错误传播链断裂

**建议**：
```rust
// 当前
let conn = self.get_connection().await?;

// 改进
let conn = self.get_connection().await
    .context(format!("Failed to get Hysteria2 connection to {}:{}", 
        self.server, self.port))?;
```

---

### 3. 路由层 (rsb-route)

**职责**：流量路由、规则匹配

**问题**：
- ⚠️ 路由决策缺少日志
- ⚠️ 规则匹配失败原因不明
- ⚠️ 性能指标缺失

**建议**：
```rust
// 添加详细路由日志
tracing::debug!(
    target: "rsbox::route",
    domain = %domain,
    matched_rule = %rule.tag,
    outbound = %outbound.tag,
    "Route decision made"
);
```

---

### 4. DNS 层 (rsb-dns)

**职责**：DNS 解析、缓存

**问题**：
- ⚠️ 查询过程无追踪
- ⚠️ 缓存命中率不可见
- ⚠️ 错误原因不清晰

---

## 🐛 常见问题分析

### 问题 1：连接失败无详细信息

**当前代码**：
```rust
// crates/rsb-protocol/src/hysteria2/client.rs
let conn = connect_and_auth(self).await?;
```

**问题**：
- 不知道失败在哪个阶段
- 不知道具体错误原因
- 无法定位网络问题

**改进方案**：
```rust
tracing::debug!("Connecting to Hysteria2 server {}:{}", self.server, self.port);

let conn = connect_and_auth(self).await
    .context(format!("Failed to connect to Hysteria2 {}:{}", self.server, self.port))?;

tracing::info!("Successfully connected to Hysteria2 {}", self.server);
```

---

### 问题 2：认证失败原因不明

**当前代码**：
```rust
if resp.status() != StatusCode::from_u16(233).unwrap() {
    anyhow::bail!("hysteria2 auth failed: {}", resp.status());
}
```

**问题**：
- 不知道服务器返回了什么错误
- 无法判断是密码错误还是其他问题

**改进方案**：
```rust
if resp.status() != StatusCode::from_u16(233).unwrap() {
    // 尝试读取错误响应体
    let error_body = stream.recv_data(&mut buf).await
        .unwrap_or_default();
    let error_msg = String::from_utf8_lossy(&error_body);
    
    tracing::error!(
        status = %resp.status(),
        server = %ob.server,
        error_body = %error_msg,
        "Hysteria2 authentication failed"
    );
    
    anyhow::bail!(
        "Hysteria2 auth failed: status={}, server={}, error={}",
        resp.status(),
        ob.server,
        error_msg
    );
}
```

---

### 问题 3：路由表加载失败

**当前代码**：
```rust
// crates/rsb-core/src/platform/linux.rs
let fd = unsafe { libc::socket(...) };
if fd < 0 {
    anyhow::bail!("netlink socket failed");
}
```

**问题**：
- 不知道为什么失败（权限？内核版本？）
- 无法给用户提供解决方案

**改进方案**：
```rust
let fd = unsafe { libc::socket(libc::AF_NETLINK, libc::SOCK_RAW, libc::NETLINK_ROUTE) };
if fd < 0 {
    let err = std::io::Error::last_os_error();
    tracing::error!(
        error = %err,
        errno = err.raw_os_error(),
        "Failed to create netlink socket"
    );
    
    match err.raw_os_error() {
        Some(libc::EPERM) | Some(libc::EACCES) => {
            anyhow::bail!("Permission denied: netlink socket requires CAP_NET_ADMIN capability. Try running with sudo.");
        }
        Some(libc::EPROTONOSUPPORT) => {
            anyhow::bail!("Netlink not supported: kernel may be too old or netlink disabled");
        }
        _ => {
            anyhow::bail!("Failed to create netlink socket: {}", err);
        }
    }
}
```

---

## 📊 错误类型分类

### 1. 网络错误
- 连接超时
- 连接拒绝
- DNS 解析失败
- TLS 握手失败

### 2. 认证错误
- 密码错误
- 证书验证失败
- Token 过期

### 3. 协议错误
- 协议版本不匹配
- 数据格式错误
- 握手失败

### 4. 系统错误
- 权限不足
- 资源不足
- 平台不支持

---

## 🛠️ 改进建议

### 1. 统一错误处理

创建自定义错误类型：

```rust
// crates/rsb-core/src/error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum RsboxError {
    #[error("Connection failed: {source}")]
    ConnectionFailed {
        server: String,
        port: u16,
        #[source]
        source: anyhow::Error,
    },
    
    #[error("Authentication failed: {reason}")]
    AuthFailed {
        protocol: String,
        server: String,
        reason: String,
    },
    
    #[error("Protocol error: {message}")]
    ProtocolError {
        protocol: String,
        message: String,
    },
    
    #[error("System error: {message}")]
    SystemError {
        operation: String,
        message: String,
        #[source]
        source: Option<std::io::Error>,
    },
}
```

### 2. 结构化日志

使用 tracing 的结构化日志：

```rust
use tracing::{debug, info, warn, error, span, Level};

// 为每个连接创建 span
let span = span!(
    Level::INFO,
    "hysteria2_connection",
    server = %self.server,
    port = self.port,
    session_id = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed)
);

let _enter = span.enter();

debug!("Starting connection");
// ... 连接逻辑 ...
info!("Connection established");
```

### 3. 性能追踪

```rust
use std::time::Instant;

let start = Instant::now();
let result = operation().await;
let duration = start.elapsed();

tracing::info!(
    duration_ms = duration.as_millis(),
    success = result.is_ok(),
    "Operation completed"
);
```

---

## 🔧 开发者模式设计

### 环境变量配置

```bash
# 启用开发者模式
export RSBOX_DEV_MODE=1

# 设置日志级别
export RUST_LOG=rsbox=trace,rsb_protocol=trace,rsb_core=debug

# 启用详细错误追踪
export RSBOX_BACKTRACE=1

# 启用性能追踪
export RSBOX_PERF_TRACE=1

# 启用协议调试
export RSBOX_PROTOCOL_DEBUG=1
```

### 配置文件

```json
{
  "log": {
    "level": "trace",
    "output": "rsbox-debug.log",
    "developer_mode": true,
    "enable_colors": true,
    "log_targets": {
      "rsbox": "trace",
      "rsb_protocol::hysteria2": "trace",
      "rsb_protocol::vmess": "trace",
      "rsb_core::platform": "debug"
    }
  },
  "debug": {
    "dump_traffic": true,
    "dump_dir": "./debug",
    "max_dump_size": 10485760,
    "enable_backtrace": true,
    "enable_perf_trace": true
  }
}
```

---

## 📈 监控指标

### 连接指标
- 连接总数
- 连接成功率
- 平均连接时间
- 认证成功率

### 性能指标
- 吞吐量
- 延迟
- 重连次数
- 错误率

### 路由指标
- 路由决策时间
- 规则命中率
- DNS 查询时间
- 缓存命中率

---

## 🎯 优先修复清单

### P0 - 紧急（影响使用）
1. ⚠️ Hysteria2 连接失败无详细信息
2. ⚠️ 认证错误原因不明
3. ⚠️ Linux 路由错误无上下文

### P1 - 重要（影响调试）
4. ⚠️ 缺少请求追踪
5. ⚠️ 错误传播链断裂
6. ⚠️ 性能瓶颈不可见

### P2 - 优化（提升体验）
7. ⚠️ 日志级别混乱
8. ⚠️ 缺少结构化日志
9. ⚠️ 监控指标缺失

---

## 📝 实施计划

### 阶段 1：基础增强（1-2 天）
1. 添加详细错误上下文
2. 统一日志记录
3. 添加开发者模式配置

### 阶段 2：追踪系统（2-3 天）
1. 实现请求追踪
2. 添加性能指标
3. 构建错误分类

### 阶段 3：调试工具（3-5 天）
1. 流量转储功能
2. 协议分析工具
3. 性能分析工具

---

## 🔍 测试建议

### 单元测试
- 错误处理路径
- 边界条件
- 异常情况

### 集成测试
- 端到端连接
- 多协议测试
- 故障注入

### 压力测试
- 高并发
- 长时间运行
- 资源限制

---

**报告生成时间**：2026-06-26 06:00  
**架构评级**：B+ (良好，有改进空间)  
**建议优先级**：立即实施阶段 1

---

**🎯 下一步：实施开发者模式和详细日志！**
