# 严重问题修复报告

## 修复时间
2026年6月26日

## 🔴 严重问题清单

### 优先级 P0：编译失败

#### 1. Linux 路由代码无法编译 ❌ → ✅

**问题**：
```rust
error: cannot find value `NLMSG_HDRLEN` in crate `libc`
error: cannot find type `rtmsg` in crate `libc`
```

**原因**：libc crate 未导出这些 netlink 符号

**修复**：
```rust
// 添加缺失的常量和结构体
const NLMSG_HDRLEN: usize = 16;
const RTMSG_HDRLEN: usize = 12;
const NLA_HDRLEN: usize = 4;

#[repr(C)]
struct rtmsg {
    rtm_family: u8,
    rtm_dst_len: u8,
    // ...
}
```

**状态**：✅ 已修复

---

#### 2. Linux netlink 标志位用错 ❌ → ✅

**问题**：
```rust
const RTM_F_REPLACE: u32 = 0x100;  // 错误！应该是 NLM_F_REPLACE
(*rt).rtm_flags = flags;  // 错误的位置
```

**原因**：
- `0x100` 是 netlink 消息标志 `NLM_F_REPLACE`
- 应该写入 `nlmsg_flags`，不是 `rtm_flags`
- `rtm_flags` 应使用 `RTF_UP` 等路由标志

**修复**：
```rust
const NLM_F_REPLACE: u16 = 0x100;
const RTF_UP: u32 = 0x1;

// 正确的位置
(*nl).nlmsg_flags = (NLM_F_REQUEST | NLM_F_ACK | NLM_F_REPLACE) as u16;
(*rt).rtm_flags = RTF_UP;  // 路由标志
```

**状态**：✅ 已修复

---

### 优先级 P1：运行时逻辑

#### 3. macOS 路由忽略 CIDR 前缀 ❌

**问题**：
```rust
fn add_request(dest: IpAddr, prefix: u8, ifindex: u32) -> Self {
    let _ = prefix;  // 前缀被忽略！
}
```

**影响**：10.0.0.0/24 被当成 10.0.0.0/32

**修复方案**：需要在 rt_msghdr 中编码 prefix

**状态**：⏸️ 待修复

---

#### 4. WireGuard 多 peer 时 endpoint 被覆盖 ❌

**问题**：
```rust
for state in guard.values_mut() {
    state.endpoint = Some(src);  // 所有 peer 都改成同一个！
}
```

**影响**：多 peer 场景路由错乱

**修复方案**：按 public key 匹配 peer

**状态**：⏸️ 待修复

---

#### 5. WireGuard outbound 启动存在竞态 ❌

**问题**：
```rust
if *guard { return Ok(()); }  // 释放锁
// 竞态窗口
self.tunnel.start().await?;  // 可能重复启动
```

**修复方案**：在整个检查-启动过程持有锁

**状态**：⏸️ 待修复

---

### 优先级 P2：安全问题

#### 8. uTLS 模式不做证书校验 ❌

**问题**：
```rust
pub async fn connect(..., _insecure: bool) -> Result<Self> {
    // _insecure 参数被忽略，没有证书校验
}
```

**风险**：MITM 攻击

**状态**：⏸️ 待修复

---

#### 9. 控制 API 未配置用户时无鉴权 ❌

**问题**：
```rust
users.is_empty() || auth_token(headers, users).is_some()
// 空 users = 无鉴权
```

**风险**：绑定 0.0.0.0 时暴露完整控制

**状态**：⏸️ 待修复

---

## ✅ 已完成的修复

### 1. Linux netlink 编译问题

**修复内容**：
- ✅ 添加 `NLMSG_HDRLEN`、`RTMSG_HDRLEN`、`NLA_HDRLEN` 常量
- ✅ 定义 `rtmsg` 结构体
- ✅ 定义 `nlattr` 结构体
- ✅ 添加所有缺失的常量

### 2. netlink 标志位修复

**修复内容**：
- ✅ `NLM_F_REPLACE` 写入 `nlmsg_flags`
- ✅ `RTF_UP` 写入 `rtm_flags`
- ✅ 修正函数签名 `flags: u32` → `flags: u16`

### 3. nlattr 编码修复

**修复内容**：
- ✅ 使用正确的 `nlattr` 结构体
- ✅ 正确的对齐计算
- ✅ 安全的内存操作

---

## 📋 待修复清单

### 高优先级

1. ⏸️ macOS 路由 CIDR 前缀
2. ⏸️ WireGuard 多 peer endpoint
3. ⏸️ WireGuard 启动竞态
4. ⏸️ Hysteria2 mem::forget
5. ⏸️ VMess security 配置

### 中优先级

6. ⏸️ uTLS 证书校验
7. ⏸️ API 鉴权强制
8. ⏸️ gRPC 鉴权
9. ⏸️ SSH host_keys 校验

### 低优先级

10. ⏸️ DNS anti_pollution 接入
11. ⏸️ DNS adblock 接入
12. ⏸️ 移除 unwrap()
13. ⏸️ netlink ACK 读取

---

## 🚀 下一步

1. **测试 Linux 编译**
   ```bash
   cargo build --target x86_64-unknown-linux-gnu
   ```

2. **修复 WireGuard 问题**
   - 多 peer endpoint 匹配
   - 启动竞态

3. **修复 macOS 路由**
   - CIDR 前缀编码

4. **安全加固**
   - 证书校验
   - API 鉴权

---

**修复状态**: 🔄 进行中  
**已修复**: 2/13  
**待修复**: 11/13

---

**立即测试编译！** 🔧
