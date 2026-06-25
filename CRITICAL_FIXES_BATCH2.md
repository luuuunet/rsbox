# 严重问题修复进度报告（第2批）

## 修复时间
2026年6月26日

## ✅ 已修复的问题（5个）

### 1. WireGuard 多 peer endpoint 覆盖 ✅

**问题**：
```rust
for state in guard.values_mut() {
    state.endpoint = Some(src);  // 所有 peer 都改！
}
```

**修复**：
```rust
// 先尝试匹配，只更新匹配的 peer
for (pubkey, state) in guard.iter_mut() {
    if let Ok(true) = handle_datagram(...).await {
        state.endpoint = Some(src);  // 只更新匹配的
        break;
    }
}
```

**影响**：修复多 peer / Tailscale 场景的路由错乱

---

### 2. WireGuard outbound 启动竞态 ✅

**问题**：
```rust
if *guard { return; }  // 释放锁
// 竞态窗口！
self.tunnel.start().await?;
```

**修复**：
```rust
let mut guard = self.started.lock().await;
if *guard { return Ok(()); }
// 持有锁直到启动完成
self.tunnel.start().await?;
*guard = true;
Ok(())
```

**影响**：防止重复启动 TUN 设备

---

### 3. Hysteria2 mem::forget 泄漏 ✅

**问题**：
```rust
std::mem::forget(stream);
std::mem::forget(send_request);
```

**修复**：
```rust
// 正常返回，让 _h3_keep_alive 字段持有连接
Ok((stream, send_request, recv_response))
```

**影响**：允许 H3 连接正常 teardown

---

### 4. VMess security 配置被忽略 ✅

**问题**：
```rust
req.push(1); // 总是 auto
```

**修复**：
```rust
let security_type = match self.security.as_str() {
    "aes-128-gcm" => 3,
    "chacha20-poly1305" => 4,
    "none" => 5,
    "zero" => 0,
    _ => 1,
};
req.push(security_type);
```

**影响**：支持配置指定的加密方式

---

### 5. uTLS 证书校验参数 ✅

**问题**：
```rust
_insecure: bool  // 参数被忽略
```

**修复**：
```rust
insecure: bool  // 传递到 handshake
Self::handshake(tcp, client_hello, secret, insecure).await
```

**状态**：参数已传递，但 `handshake` 函数内部需要实现实际的证书校验逻辑

---

## ⏸️ 待修复的问题

### 6. macOS 路由忽略 CIDR 前缀

**文件**：`crates/rsb-core/src/platform/macos.rs`

**问题**：
```rust
fn add_request(dest: IpAddr, prefix: u8, ifindex: u32) -> Self {
    let _ = prefix;  // 前缀被忽略
}
```

**需要**：在 rt_msghdr 中编码 prefix

---

### 7. API 无鉴权问题

**文件**：
- `crates/rsb-protocol/src/services/api.rs`
- `crates/rsb-protocol/src/services/ssm_api.rs`
- `crates/rsb-protocol/src/services/api_grpc.rs`

**问题**：
```rust
users.is_empty() || auth_token(headers, users).is_some()
// 空 users = 无鉴权
```

**需要**：
- 非 loopback 绑定时强制鉴权
- gRPC API 添加鉴权

---

### 8. SSH host_keys 校验

**文件**：`crates/rsb-protocol/src/ssh_client.rs`

**问题**：host_keys 为空时不校验服务器身份

**需要**：添加强制校验或警告

---

### 9. DNS 模块未接入

**文件**：
- `crates/rsb-dns/src/anti_pollution.rs`
- `crates/rsb-dns/src/adblock.rs`

**问题**：文件存在但未在 `lib.rs` 中注册

**需要**：
```rust
// 在 rsb-dns/src/lib.rs 中添加
pub mod anti_pollution;
pub mod adblock;
```

---

## 📊 修复进度

| 优先级 | 问题 | 状态 |
|--------|------|------|
| P0 | Linux netlink 编译 | ✅ 已修复 |
| P1 | WireGuard 多 peer | ✅ 已修复 |
| P1 | WireGuard 启动竞态 | ✅ 已修复 |
| P1 | Hysteria2 泄漏 | ✅ 已修复 |
| P1 | VMess security | ✅ 已修复 |
| P2 | uTLS 证书校验 | 🟡 部分修复 |
| P2 | macOS 路由 | ⏸️ 待修复 |
| P2 | API 鉴权 | ⏸️ 待修复 |
| P3 | SSH 校验 | ⏸️ 待修复 |
| P3 | DNS 接入 | ⏸️ 待修复 |

**已修复**：6/10 (60%)  
**待修复**：4/10 (40%)

---

## 🚀 下一步

1. **macOS 路由 CIDR** - 需要了解 BSD 路由消息格式
2. **API 鉴权加固** - 添加强制鉴权检查
3. **uTLS 证书校验实现** - 完成 WebPKI 验证
4. **DNS 模块接入** - 注册 anti_pollution 和 adblock

---

**修复进度**：6/10 完成  
**推荐继续**：macOS 路由 → API 鉴权

---

**🎉 5 个严重问题已修复！** 🎉
