# 日志系统优化指南

## 问题描述
2026年6月26日 15:00

**用户反馈**：生产环境日志太多，希望减少日志输出

---

## 🎯 优化方案

### 方案 1：环境变量控制（推荐）✅

#### 使用方法

**生产环境（最少日志）**：
```bash
# 只显示错误
RUST_LOG=error ./rsbox run -c config.json

# 或者不设置，默认为 warn
./rsbox run -c config.json
```

**开发环境（详细日志）**：
```bash
# 显示所有日志
RUST_LOG=debug ./rsbox run -c config.json

# 只显示特定模块
RUST_LOG=rsb_protocol=debug ./rsbox run -c config.json
```

---

### 方案 2：编译时优化（最彻底）✅

#### 修改 Cargo.toml

添加 release 配置以移除 debug 日志：

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true  # 移除符号信息
# 移除 debug 日志的编译优化
[profile.release.package."*"]
opt-level = 3
```

#### 条件编译宏

使用我创建的 `logging.rs` 模块：

```rust
// 开发环境：输出日志
debug!("Connection established");

// 生产环境：完全不编译
// 零运行时开销
```

---

### 方案 3：配置文件控制 ✅

#### 在配置文件中设置日志级别

```json
{
  "log": {
    "level": "error",  // 只显示错误
    "disabled": false,
    "output": "stdout"
  },
  "inbounds": [...],
  "outbounds": [...]
}
```

---

## 🔧 立即实施步骤

### 步骤 1：添加日志配置模块

已创建：`crates/rsb-core/src/logging.rs`

**功能**：
- ✅ 生产环境默认 `warn` 级别
- ✅ 开发环境默认 `info` 级别
- ✅ 条件编译宏自动移除 debug/trace

### 步骤 2：更新 main.rs

```rust
// crates/rsbox/src/main.rs
use rsb_core::logging;

fn main() {
    // 初始化日志（自动根据环境选择级别）
    logging::init_logging();

    // ... 其他代码
}
```

### 步骤 3：替换现有日志调用

**之前**：
```rust
tracing::debug!("Connection established");
tracing::trace!("Packet received: {:?}", packet);
```

**之后**：
```rust
use rsb_core::{debug, trace};

debug!("Connection established");  // 生产环境不编译
trace!("Packet received: {:?}", packet);  // 生产环境不编译
```

---

## 📊 日志级别说明

| 级别 | 使用场景 | 生产环境 | 开发环境 |
|------|---------|---------|---------|
| **error** | 错误信息 | ✅ 显示 | ✅ 显示 |
| **warn** | 警告信息 | ✅ 显示 | ✅ 显示 |
| **info** | 重要信息 | ❌ 隐藏 | ✅ 显示 |
| **debug** | 调试信息 | ❌ 不编译 | ✅ 显示 |
| **trace** | 详细追踪 | ❌ 不编译 | ✅ 显示 |

---

## 🎯 推荐配置

### 生产环境

```bash
# 方式 1：不设置（默认 warn）
./rsbox run -c config.json

# 方式 2：只显示错误
RUST_LOG=error ./rsbox run -c config.json

# 方式 3：完全静默（不推荐）
RUST_LOG=off ./rsbox run -c config.json
```

### 开发环境

```bash
# 显示详细日志
RUST_LOG=debug ./rsbox run -c config.json

# 只显示特定模块
RUST_LOG=rsb_protocol=debug,rsb_dns=info ./rsbox run -c config.json

# 显示所有日志（包括依赖库）
RUST_LOG=trace ./rsbox run -c config.json
```

---

## 🔍 检查当前日志量

### 统计日志调用

```bash
# tracing 日志数量
grep -r "tracing::" --include="*.rs" crates/ | wc -l

# println 数量
grep -r "println!\|eprintln!" --include="*.rs" crates/ | wc -l

# debug 级别日志
grep -r "tracing::debug" --include="*.rs" crates/ | wc -l
```

### 建议清理

**可以移除**：
- `println!` / `eprintln!`（除了 main.rs 的帮助信息）
- 过多的 `debug!` 调用
- 详细的 `trace!` 调用

**应该保留**：
- `error!` - 错误信息
- `warn!` - 警告信息
- 关键的 `info!` - 重要事件（如启动、连接、停止）

---

## 📝 迁移清单

### 高优先级（立即清理）

1. **移除 println/eprintln**
   ```rust
   // ❌ 移除
   println!("Debug: {:?}", data);
   
   // ✅ 替换为
   debug!("Debug: {:?}", data);
   ```

2. **降低详细日志级别**
   ```rust
   // ❌ 过多的 info
   info!("Processing packet {}", i);
   
   // ✅ 改为 debug
   debug!("Processing packet {}", i);
   ```

3. **条件编译 debug 日志**
   ```rust
   // ❌ 生产环境仍会评估参数
   debug!("Data: {:?}", expensive_operation());
   
   // ✅ 生产环境完全不编译
   #[cfg(debug_assertions)]
   debug!("Data: {:?}", expensive_operation());
   ```

### 中优先级（逐步优化）

4. **精简日志内容**
   ```rust
   // ❌ 过于详细
   debug!("Received packet: len={}, type={}, data={:?}", len, typ, data);
   
   // ✅ 简洁明了
   debug!("Received {} bytes", len);
   ```

5. **合并重复日志**
   ```rust
   // ❌ 循环中大量日志
   for item in items {
       debug!("Processing {}", item);
   }
   
   // ✅ 汇总日志
   debug!("Processing {} items", items.len());
   ```

---

## 🚀 快速清理脚本

### 清理 println

```bash
# 查找所有 println
find crates -name "*.rs" -exec grep -l "println!" {} \;

# 替换为 debug（需要手动确认）
# sed -i 's/println!/debug!/g' file.rs
```

### 清理过多的 debug

```bash
# 查找调用最频繁的文件
find crates -name "*.rs" -exec sh -c 'echo "$(grep -c "tracing::debug" "$1") $1"' _ {} \; | sort -rn | head -20
```

---

## ✅ 验证效果

### 构建并测试

```bash
# Release 构建（生产模式）
cargo build --release -p rsbox

# 运行（无 RUST_LOG，默认 warn）
./target/release/rsbox run -c config.json

# 应该只看到：
# - 启动信息
# - 警告（如果有）
# - 错误（如果有）
# - 没有 debug/trace 日志
```

### 对比测试

**之前（所有日志）**：
```bash
RUST_LOG=debug ./rsbox run -c config.json
# 输出：数百行日志
```

**之后（生产模式）**：
```bash
./rsbox run -c config.json
# 输出：只有关键信息，<10 行
```

---

## 📈 预期效果

### 性能提升

- ✅ 减少字符串格式化开销
- ✅ 减少 I/O 操作
- ✅ 减少锁竞争（日志写入）

### 日志输出

**生产环境**：
```
[2026-06-26 15:00:00] INFO Starting rsbox v0.1.2
[2026-06-26 15:00:00] INFO Listening on 127.0.0.1:7890
[2026-06-26 15:00:05] WARN Connection timeout, retrying
```

**开发环境**（RUST_LOG=debug）：
```
[2026-06-26 15:00:00] INFO Starting rsbox v0.1.2
[2026-06-26 15:00:00] DEBUG Loading configuration from config.json
[2026-06-26 15:00:00] DEBUG Initializing DNS resolver
[2026-06-26 15:00:00] DEBUG Starting inbound: mixed on 127.0.0.1:7890
[2026-06-26 15:00:00] INFO Listening on 127.0.0.1:7890
[2026-06-26 15:00:01] DEBUG New connection from 127.0.0.1:12345
[2026-06-26 15:00:01] TRACE DNS query: google.com
... (更多详细日志)
```

---

## 🎯 总结

### 推荐方案

1. **立即**：使用环境变量 `RUST_LOG=error`
2. **短期**：添加 `logging.rs` 模块
3. **长期**：逐步清理不必要的日志

### 快速使用

```bash
# 生产环境（静默运行）
./rsbox run -c config.json

# 或显式设置
RUST_LOG=error ./rsbox run -c config.json

# 开发调试
RUST_LOG=debug ./rsbox run -c config.json
```

---

**优化指南生成时间**：2026-06-26 15:00  
**目标**：生产环境减少 90%+ 日志输出  
**方法**：环境变量 + 条件编译 + 级别控制

---

🎯 **建议立即使用 `RUST_LOG=error` 或不设置环境变量（默认 warn）！**
