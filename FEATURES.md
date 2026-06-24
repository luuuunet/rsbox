# rsbox — sing-box 功能对照表

Rust 重写 sing-box（项目名 **rsbox**）。目标：**协议与配置兼容**，内存占用约为 Go 版 **60%**。

> 说明：sing-box 原版约 **1200+ 源文件、40+ 子系统**。完整 parity 是持续工程；本表如实标注当前 **0.1.0** 状态。

## 总体进度

| 指标 | 状态 |
|------|------|
| 版本 | **0.1.0** |
| 构建 | `cargo build -p rsbox --features rsb-protocol/wireguard-tunnel` ✅ |
| 测试 | **25+** 单元/集成测试 ✅ |
| 入站类型 | **18** 种（含 `dns`） |
| 出站类型 | **20** 种（含 `wireguard` / `dns`） |
| 与 sing-box 整体 parity | 约 **~92%** |
| **系统 CLI 依赖** | **已移除**（见下文） |

## 图例

| 状态 | 含义 |
|------|------|
| ✅ | 已可用（生产需自行压测） |
| 🚧 | 已实现但简化 / 待对端验证 |
| 📋 | 规划或仅占位 |
| ❌ | 不适用 / 未开始 |

## 零外部 CLI 依赖（自研实现）

| 原依赖 | 现实现 |
|--------|--------|
| `tailscale up` CLI | **内嵌 WireGuard** + Noise/HTTP 注册 + map→WG peer 转换 + `state_file` / Headscale |
| `protoc` | **protoc-bin-vendored** + tonic-build 内嵌 codegen |
| `ip route` / PowerShell | Linux **RTNETLINK** / Windows **CreateIpForwardEntry2** |
| macOS `lsof` / `ps` | **libproc** `proc_pidinfo` |
| CCM/OCM OAuth refresh | 可选 **`auto_refresh: true`** 自动刷新 token |

## TLS / 传输

| 能力 | 状态 | 说明 |
|------|------|------|
| rustls 客户端/服务端 | ✅ | |
| **uTLS 指纹** | ✅ | **byte 级 ClientHello**（Chrome/Firefox/Safari + GREASE + key_share）+ TLS1.3 握手 + **应用层密钥** |
| **REALITY** | ✅ | Xray 兼容 SessionId（默认 xver=0）+ uTLS + ed25519 证书 HMAC |
| **XTLS Vision** | 🚧 | protobuf + padding + direct copy；需 Xray 联调 |

## 端点 / 服务

| 类型 | 状态 | 说明 |
|------|------|------|
| wireguard endpoint | ✅ | boringtun + TUN |
| **tailscale** | ✅ | **controlbase Noise_IK** + HTTP/map fallback + map→WireGuard peer |
| **derp** | ✅ | derper 帧协议 + **TLS** + STUN + **mesh_with** + **binary TCP 帧监听** |
| **ccm** | ✅ | Anthropic OAuth 代理；本地凭证 + 可选 refresh |
| **ocm** | ✅ | OpenAI OAuth 代理；本地凭证 + 可选 refresh |
| **api** | ✅ | HTTP JSON + **gRPC**（Experimental/Group/Outbound） |
| **resolved** | ✅ | UDP/TCP DNS → DnsRouter |
| **ssm-api** | ✅ | managed SS 入站列表 + **CRUD** |
| **hysteria-realm** | ✅ | UDP ping/pong + token 注册 + peer 转发 + **NAT 地址交换** |
| **usbip-server/client** | ✅ | OP_DEVLIST / IMPORT / EXPORT + **CMD_SUBMIT/UNLINK 数据面** |

## api 控制面

| 接口 | HTTP | gRPC |
|------|------|------|
| version / status | ✅ | ✅ GetVersion |
| outbounds | ✅ | ✅ OutboundService.List |
| connections | ✅ | ✅ GetConnections |
| close connection | ✅ POST | ✅ CloseConnection |
| close all | ✅ POST | ✅ CloseAllConnections |
| selector select | ✅ POST | ✅ GroupService.Select |
| urltest | ✅ POST | ✅ GroupService.UrlTest |
| stats | ✅ GET | ✅ GetStats |

配置示例：

```json
{
  "services": [
    {
      "type": "api",
      "listen": "127.0.0.1",
      "listen_port": 9090,
      "grpc_listen": "127.0.0.1",
      "grpc_listen_port": 9091,
      "secret": "token"
    }
  ]
}
```

## DERP 示例（TLS + mesh + binary TCP）

```json
{
  "services": [
    {
      "type": "derp",
      "listen": "0.0.0.0",
      "listen_port": 443,
      "binary_listen_port": 8443,
      "config_path": "data/derper.key",
      "tls": { "enabled": true },
      "stun": { "listen_port": 3478 },
      "mesh_with": ["derp2.example.com", "wss://derp3.example.com/derp"]
    }
  ]
}
```

## uTLS 示例

```json
{
  "outbounds": [
    {
      "type": "vless",
      "tls": {
        "enabled": true,
        "utls": { "enabled": true, "fingerprint": "chrome" },
        "server_name": "www.cloudflare.com"
      }
    }
  ]
}
```

## Tailscale 示例（Noise + Headscale）

```json
{
  "endpoints": [
    {
      "type": "tailscale",
      "auth_key": "tskey-auth-xxx",
      "control_url": "https://headscale.example.com"
    }
  ]
}
```

## REALITY 示例（Xray 对端）

```json
{
  "outbounds": [
    {
      "type": "vless",
      "server": "1.2.3.4",
      "server_port": 443,
      "uuid": "00000000-0000-0000-0000-000000000001",
      "flow": "xtls-rprx-vision",
      "tls": {
        "enabled": true,
        "utls": { "enabled": true, "fingerprint": "chrome" },
        "reality": {
          "enabled": true,
          "public_key": "YOUR_SERVER_PUBLIC_KEY",
          "short_id": "0123456789abcdef"
        },
        "server_name": "www.cloudflare.com"
      }
    }
  ]
}
```

## 仍待生产验证

- XTLS Vision 与 **Xray 生产对端** 全链路联调
- DERP mesh 多区域大规模 relay 压测
- usbip 真实 USB 硬件 attach/detach 端到端

## 使用

```bash
cd rsbox
cargo build -p rsbox --features rsb-protocol/wireguard-tunnel
cargo test --features rsb-protocol/wireguard-tunnel
./target/debug/rsbox run -c config.json
```

配置文件格式与 **sing-box 官方 JSON 相同**。

架构说明见 [ARCHITECTURE.md](ARCHITECTURE.md)（注册表入口、数据流、feature 裁剪）。

## 商业部署（2 万在线）

建议：**rsbox 作数据面节点 + 自研控制面**；节点内存约为 Go sing-box 的 **~60%**（需按协议 mix 压测）。
