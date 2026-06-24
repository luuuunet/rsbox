# rsbox 架构（简版）

## 心智模型（5 步）

```
配置 JSON → RsBox 引擎 → 入站/出站 → 路由/DNS → 传输(TLS)
```

```
客户端 → Inbound → Router → Outbound → 远端
```

## Crate 分层

| 层 | Crate | 职责 |
|----|-------|------|
| 入口 | `rsbox` | CLI |
| 引擎 | `rsb-protocol` | RsBox 生命周期 + 协议实现 |
| 抽象 | `rsb-core` | Inbound/Outbound/Router trait |
| 路由 | `rsb-route` | 规则匹配 |
| DNS | `rsb-dns` | 解析 |
| 配置 | `rsb-config` | sing-box JSON |

## 统一注册表（加新协议看这里）

| 类型 | 注册文件 |
|------|----------|
| 入站 / 出站 | [`crates/rsb-protocol/src/registry.rs`](crates/rsb-protocol/src/registry.rs) |
| 服务 service | [`crates/rsb-protocol/src/services/registry.rs`](crates/rsb-protocol/src/services/registry.rs) |
| 端点 endpoint | [`crates/rsb-protocol/src/endpoints.rs`](crates/rsb-protocol/src/endpoints.rs) |
| 类型常量 | [`crates/rsb-constant/src/lib.rs`](crates/rsb-constant/src/lib.rs) |

**新增 vless 出站**：在 `registry.rs` 的 `build_outbound` 加一条 match 分支，并在 `rsb-constant` 的 `ALL_OUTBOUND_TYPES` 确认已列出。

## Feature 裁剪（编译体积）

```bash
# 默认（含 QUIC + WireGuard 隧道 feature 由 rsbox 二进制开启）
cargo build -p rsbox --features rsb-protocol/wireguard-tunnel

# 无 WireGuard 数据面（更小依赖树）
cargo build -p rsbox --no-default-features
```

| Feature | 作用 |
|---------|------|
| `quic` | hysteria2 / tuic QUIC（默认开） |
| `wireguard-tunnel` | boringtun 真实 WG/Tailscale 数据面 |

## 数据流

```mermaid
flowchart LR
    C[客户端] --> IB[Inbound]
    IB --> R[RuleRouter]
    R --> OB[Outbound]
    OB --> T[transport/utls/reality]
    T --> S[远端服务器]
```

功能对照见 [FEATURES.md](FEATURES.md)。
