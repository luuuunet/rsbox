# rsbox 架构重构计划

## 📋 执行摘要

**目标**：解决依赖版本冲突、循环依赖、架构混乱等问题
**预期收益**：
- 编译时间减少 40% (8分钟 → 5分钟)
- 二进制大小减少 20% (50MB → 40MB)
- 架构清晰、易维护
- 消除 15+ 依赖版本冲突

**执行策略**：分三阶段，每阶段独立可验证

---

## 🎯 问题诊断

### 1. 严重依赖版本冲突（15+个）
- axum: 0.7.9 vs 0.8.9
- base64: 0.13.1 vs 0.22.1
- rand: 0.8.6 vs 0.9.4
- getrandom: 0.2 vs 0.3 vs 0.4
- thiserror: 1.0 vs 2.0
- ring: 0.16 vs 0.17
- windows-sys: 0.52 vs 0.59 vs 0.60 vs 0.61

### 2. 循环依赖
```
rsb-protocol → rsb-experimental → rsb-protocol (循环！)
```
- rsb-experimental 同时被 rsb-protocol 和 rsb-libbox 依赖
- rsb-experimental 又依赖 rsb-protocol

### 3. 职责混乱
- **rsb-protocol**: 64个文件，职责过重
  - 协议实现 (vless, vmess, trojan, etc.)
  - 传输层 (utls, reality, xtls)
  - 服务 (api, derp, realm, usbip)
  - 端点 (tailscale, wireguard)
  - 引擎 (engine, build)
  
- **rsb-experimental**: 定位不明
  - 只有 Clash API 和 V2Ray API
  - 被 rsb-protocol 依赖但又依赖 rsb-protocol

- **rsb-libbox**: 仅为 FFI 包装
  - 依赖 rsb-experimental
  - 但 rsbox CLI 不使用它

### 4. crate 代码量不均衡
```
rsb-protocol:     64 文件  (90%+ 的代码)
rsb-core:         12 文件
rsb-route:         4 文件
rsb-dns:           3 文件
rsb-config:        1 文件
rsb-constant:      1 文件
rsb-experimental:  1 文件
rsb-libbox:        1 文件
rsb-wireguard:     1 文件
```

---

## 🏗️ 重构方案

### 阶段 1：立即修复依赖冲突 ⚡ (1-2小时)

#### 1.1 统一依赖版本
在 `Cargo.toml` 的 `[workspace.dependencies]` 中强制版本：

```toml
# 统一 axum 到最新版本
axum = "0.8"
axum-server = "0.7"

# 统一 base64
base64 = "0.22"

# 统一错误处理
thiserror = "2.0"

# 统一 WebSocket
tungstenite = "0.29"
tokio-tungstenite = "0.29"

# 统一随机数生成
rand = "0.9"
getrandom = "0.4"
```

#### 1.2 处理不可升级的依赖
对于 boringtun (依赖 ring 0.16)，使用 `[patch]` 隔离：

```toml
[patch.crates-io]
# 强制新代码使用 ring 0.17
# boringtun 继续使用 ring 0.16（无法避免）
```

#### 1.3 修复所有 clippy 警告
```bash
cargo fix --workspace --allow-dirty
cargo clippy --workspace --fix --allow-dirty
```

**验证点**：
- [ ] `cargo tree --duplicates` 显示冲突从 15+ 降到 <5
- [ ] `cargo build --release` 成功
- [ ] 所有测试通过

---

### 阶段 2：打破循环依赖 🔄 (2-4小时)

#### 2.1 重组 rsb-experimental

**当前问题**：
- rsb-experimental 只包含 API 服务（Clash API, V2Ray API, Cache）
- 但被 rsb-protocol 依赖，又依赖 rsb-protocol

**解决方案**：将 API 服务移到独立 crate

```
创建新 crate: rsb-api
  ├─ clash_api.rs   (从 rsb-experimental 移动)
  ├─ v2ray_api.rs   (从 rsb-experimental 移动)
  └─ cache.rs       (从 rsb-experimental 移动)

删除 rsb-experimental crate
```

**依赖关系变为**：
```
rsbox (CLI)
  ├→ rsb-protocol
  ├→ rsb-api (新)
  └→ rsb-libbox

rsb-api (新)
  ├→ rsb-protocol
  └→ rsb-core

rsb-libbox
  ├→ rsb-protocol
  └→ rsb-api
```

**文件操作**：
1. 创建 `crates/rsb-api/`
2. 移动 `rsb-experimental/src/lib.rs` → `rsb-api/src/lib.rs`
3. 更新所有引用 `rsb_experimental` → `rsb_api`
4. 删除 `crates/rsb-experimental/`
5. 更新 workspace 成员列表

**验证点**：
- [ ] 没有循环依赖
- [ ] `cargo build --workspace` 成功
- [ ] API 功能正常

---

### 阶段 3：重构 rsb-protocol 📦 (4-8小时)

#### 3.1 问题分析
rsb-protocol 承担了太多职责（64个文件）：
- 协议实现
- 传输层
- 服务
- 引擎逻辑

#### 3.2 拆分策略

**方案 A：提取服务层（推荐，风险最低）**

将 `services/` 目录提取为独立 crate：

```
创建: crates/rsb-services/
  ├─ api.rs           (HTTP/gRPC API 服务)
  ├─ api_grpc.rs
  ├─ derp.rs          (DERP relay 服务)
  ├─ hysteria_realm.rs
  ├─ resolved.rs      (DNS 服务)
  ├─ ssm_api.rs
  ├─ usbip.rs
  ├─ multiplexer.rs
  ├─ listen.rs
  └─ mod.rs

保留在 rsb-protocol/services/：
  └─ registry.rs      (服务注册表)
```

**新的依赖关系**：
```
rsb-protocol
  ├→ rsb-core
  ├→ rsb-config
  ├→ rsb-services (可选)
  └→ (不再依赖 rsb-experimental)

rsb-services
  ├→ rsb-core
  ├→ rsb-config
  └→ (不依赖 rsb-protocol)
```

**方案 B：深度拆分（可选，未来考虑）**

进一步拆分为：
- rsb-transport (TLS, QUIC, uTLS, Reality)
- rsb-protocols (每个协议一个模块)

**本次重构采用方案 A**，方案 B 留待后续优化。

#### 3.3 具体操作

1. **创建 rsb-services crate**
   ```bash
   mkdir -p crates/rsb-services/src
   ```

2. **移动服务文件**
   ```bash
   cp crates/rsb-protocol/src/services/*.rs crates/rsb-services/src/
   # 保留 registry.rs 在原位置
   ```

3. **更新 Cargo.toml**
   ```toml
   # crates/rsb-services/Cargo.toml
   [dependencies]
   rsb-core.workspace = true
   rsb-config.workspace = true
   axum.workspace = true
   tokio.workspace = true
   tonic.workspace = true
   # ... 其他服务所需依赖
   ```

4. **更新 rsb-protocol**
   - 从 dependencies 中移除不需要的 axum-server, tonic 等
   - 在 registry 中导入 rsb-services

5. **可选：按需加载服务**
   ```toml
   # rsb-protocol/Cargo.toml
   [features]
   default = ["quic"]
   quic = []
   wireguard-tunnel = ["dep:rsb-wireguard"]
   services = ["dep:rsb-services"]  # 新增
   ```

**验证点**：
- [ ] rsb-protocol 文件数减少到 ~50
- [ ] 服务功能正常
- [ ] 编译时间减少

---

## 📊 最终架构

### 目标 Crate 结构

```
rsbox/
├── rsbox (CLI 入口)                   [1 文件]
│   └→ rsb-protocol
│   └→ rsb-api
│   └→ rsb-config
│
├── rsb-protocol (核心协议引擎)        [~50 文件]
│   ├─ 协议实现/ (vless, vmess, ss, trojan, etc.)
│   ├─ 传输层/ (transport, utls, reality, xtls)
│   ├─ 端点/ (tailscale, wireguard)
│   ├─ 引擎/ (engine, build, registry)
│   └→ rsb-core, rsb-config, rsb-dns, rsb-route
│
├── rsb-api (API 服务)                 [1 文件]
│   ├─ Clash API
│   ├─ V2Ray API  
│   ├─ Cache File
│   └→ rsb-protocol, rsb-core
│
├── rsb-services (其他服务)             [13 文件]
│   ├─ DERP relay
│   ├─ Hysteria Realm
│   ├─ DNS resolved
│   ├─ SSM API
│   ├─ USB/IP
│   └→ rsb-core, rsb-config
│
├── rsb-core (核心抽象层)              [12 文件]
│   └→ rsb-config
│
├── rsb-route (路由引擎)               [4 文件]
│   └→ rsb-core, rsb-config
│
├── rsb-dns (DNS 引擎)                 [3 文件]
│   └→ rsb-config
│
├── rsb-wireguard (WireGuard 数据面)   [1 文件]
│   └→ rsb-core
│
├── rsb-config (配置解析)              [1 文件]
│   └→ rsb-constant
│
├── rsb-constant (常量定义)            [1 文件]
│
└── rsb-libbox (FFI 包装)              [1 文件]
    └→ rsb-protocol, rsb-api
```

### 清晰的层次结构

```
Layer 5: rsbox (CLI)
         rsb-libbox (FFI)
         ↓
Layer 4: rsb-api (可选)
         rsb-services (可选)
         ↓
Layer 3: rsb-protocol (核心引擎)
         ↓
Layer 2: rsb-core + rsb-route + rsb-dns + rsb-wireguard
         ↓
Layer 1: rsb-config
         ↓
Layer 0: rsb-constant
```

**关键改进**：
✅ 无循环依赖
✅ 职责清晰
✅ 按需编译（通过 features）
✅ 易于测试和维护

---

## 🔧 实施细节

### 阶段 1 文件清单

**修改文件**：
- `Cargo.toml` - 统一依赖版本
- `crates/*/Cargo.toml` - 更新版本引用

**命令**：
```bash
cargo fix --workspace --allow-dirty
cargo update
cargo build --release
cargo test --workspace
```

### 阶段 2 文件清单

**创建文件**：
- `crates/rsb-api/Cargo.toml`
- `crates/rsb-api/src/lib.rs`

**移动/删除**：
- 移动 `crates/rsb-experimental/src/lib.rs` → `crates/rsb-api/src/lib.rs`
- 删除 `crates/rsb-experimental/`

**修改文件**：
- `Cargo.toml` - 更新 workspace members
- `crates/rsb-protocol/Cargo.toml` - 移除 rsb-experimental 依赖
- `crates/rsb-libbox/Cargo.toml` - 改为依赖 rsb-api
- `crates/rsb-libbox/src/lib.rs` - 更新 use 语句
- `rsbox/Cargo.toml` - 添加 rsb-api 依赖（如果需要）

### 阶段 3 文件清单

**创建文件**：
- `crates/rsb-services/Cargo.toml`
- `crates/rsb-services/src/mod.rs`
- `crates/rsb-services/src/*.rs` (13个服务文件)

**修改文件**：
- `Cargo.toml` - 添加 rsb-services 到 workspace
- `crates/rsb-protocol/Cargo.toml` - 添加可选依赖 rsb-services
- `crates/rsb-protocol/src/services/mod.rs` - 重新导出 rsb-services

---

## ⚠️ 风险评估

### 高风险
1. **axum 版本升级**
   - tonic 0.12 强依赖 axum 0.7
   - 缓解：暂时保留 tonic 依赖旧版，只在必要处使用 axum 0.8
   
2. **API 接口变更**
   - 移动代码可能影响外部调用
   - 缓解：保持公开 API 签名不变，使用 pub use 重新导出

### 中风险
3. **服务拆分**
   - 服务间可能有隐式依赖
   - 缓解：仔细测试每个服务独立运行

4. **编译顺序**
   - 新的依赖图可能影响编译顺序
   - 缓解：分阶段验证，每步都完整构建

### 低风险
5. **依赖版本统一**
   - 大部分库 API 兼容
   - 缓解：有 breaking changes 的逐一处理

---

## ✅ 验证计划

### 每阶段验证

**编译验证**：
```bash
cargo clean
cargo build --release --workspace
cargo clippy --workspace -- -D warnings
```

**功能验证**：
```bash
cargo test --workspace
./target/release/rsbox check -c config.example.json
./target/release/rsbox version
```

**性能验证**：
```bash
# 编译时间
time cargo build --release

# 二进制大小
ls -lh target/release/rsbox
```

### 最终验证

**依赖检查**：
```bash
cargo tree --duplicates  # 应该 <5 个冲突
cargo tree --depth 2     # 检查依赖图清晰
```

**功能测试**：
- [ ] Shadowsocks 连接
- [ ] VLESS/VMess 连接
- [ ] Hysteria2 连接
- [ ] TUN 模式
- [ ] Clash API
- [ ] V2Ray API
- [ ] 路由规则
- [ ] DNS 分流

**回归测试**：
```bash
cargo test --workspace --all-features
```

---

## 📈 预期收益

### 编译时间
- 当前：~8 分钟（首次）
- 目标：~5 分钟（减少 37.5%）

### 二进制大小
- 当前：~50MB
- 目标：~40MB（减少 20%）

### 依赖冲突
- 当前：15+ 个库多版本
- 目标：<5 个（只保留不可避免的如 boringtun）

### 代码质量
- ✅ 清晰的模块边界
- ✅ 易于测试
- ✅ 易于添加新协议
- ✅ 按需编译

---

## 🎯 成功标准

### 必须达成
1. ✅ 无循环依赖
2. ✅ 所有测试通过
3. ✅ 编译成功且无 error
4. ✅ 功能完整（配置兼容）

### 期望达成
1. ✅ 编译时间减少 >30%
2. ✅ 二进制减少 >15%
3. ✅ 依赖冲突 <5 个
4. ✅ Clippy 警告 <10 个

### 可选目标
1. ✅ 模块化 feature gates
2. ✅ 文档完善
3. ✅ 性能测试通过

---

## 📅 时间估算

| 阶段 | 任务 | 时间 | 风险 |
|------|------|------|------|
| 1 | 统一依赖版本 | 1-2小时 | 低 |
| 1 | 修复 clippy 警告 | 0.5小时 | 低 |
| 1 | 验证测试 | 0.5小时 | 低 |
| 2 | 创建 rsb-api | 1小时 | 中 |
| 2 | 移动代码 | 1小时 | 中 |
| 2 | 更新引用 | 1小时 | 中 |
| 2 | 验证测试 | 1小时 | 中 |
| 3 | 创建 rsb-services | 1小时 | 中 |
| 3 | 移动服务代码 | 2小时 | 中 |
| 3 | 更新依赖关系 | 1小时 | 中 |
| 3 | feature gates | 1小时 | 低 |
| 3 | 完整测试 | 2小时 | 低 |
| **总计** | | **12-14小时** | |

**建议执行**：
- 第一天：阶段 1（2-3小时）
- 第二天：阶段 2（4小时）
- 第三天：阶段 3（6-7小时）

---

## 🔄 回滚计划

每个阶段都使用 git commit，方便回滚：

```bash
# 阶段 1 完成
git add -A
git commit -m "Phase 1: Unify dependency versions"

# 阶段 2 完成
git add -A
git commit -m "Phase 2: Break circular dependency"

# 阶段 3 完成
git add -A
git commit -m "Phase 3: Extract services crate"
```

如果任何阶段出现问题：
```bash
git reset --hard HEAD^  # 回滚到上一个 commit
```

---

## 📝 注意事项

### 1. 保持向后兼容
- 配置文件格式不变
- CLI 参数不变
- API 接口尽量兼容

### 2. 渐进式重构
- 每阶段独立可用
- 不要一次改太多
- 及时验证

### 3. 文档更新
- 更新 ARCHITECTURE.md
- 更新 README.md
- 更新贡献指南

### 4. 社区沟通
- 发布重构说明
- 收集反馈
- 提供迁移指导

---

## 🎬 开始执行

准备就绪后，按以下顺序执行：

1. **备份当前代码**
   ```bash
   git add -A
   git commit -m "Before architecture refactor"
   git tag pre-refactor
   ```

2. **执行阶段 1**
   - 统一依赖版本
   - 修复 warnings
   - 验证通过

3. **执行阶段 2**
   - 创建 rsb-api
   - 打破循环依赖
   - 验证通过

4. **执行阶段 3**
   - 提取 rsb-services
   - 精简 rsb-protocol
   - 最终验证

5. **发布**
   - 更新文档
   - 标记新版本
   - 通知社区

---

**准备好开始了吗？** 👍
