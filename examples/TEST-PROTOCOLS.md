# ShadowTLS+SS / AnyTLS 测试配置说明

本目录包含 rsbox 客户端测试配置，端口 **17890**（与 G5 默认 mixed 端口一致）。

## 文件一览

| 文件 | 用途 |
|------|------|
| [config-shadowtls-ss.json](config-shadowtls-ss.json) | ShadowTLS **v3** + Shadowsocks 2022（推荐） |
| [config-shadowtls-ss-v2.json](config-shadowtls-ss-v2.json) | ShadowTLS **v2** + SS chacha20 |
| [config-anytls.json](config-anytls.json) | AnyTLS 单节点 |
| [config-test-shadowtls-ss-anytls.json](config-test-shadowtls-ss-anytls.json) | 选择器切换 ST+SS / AnyTLS |
| [server/config-shadowtls-ss-server.json](server/config-shadowtls-ss-server.json) | 服务端参考（sing-box） |
| [server/config-anytls-server.json](server/config-anytls-server.json) | AnyTLS 服务端参考（sing-box） |

## 填写占位符

复制一份配置并替换以下字段：

| 占位符 | 说明 |
|--------|------|
| `YOUR_SERVER` | VPS IP 或域名 |
| `YOUR_ST_PASSWORD` | ShadowTLS 密码（v2/v3 必填） |
| `YOUR_SS_PASSWORD` | Shadowsocks 密码 |
| `YOUR_ANYTLS_PASSWORD` | AnyTLS 密码 |
| `YOUR_SNI` | TLS SNI，需与服务端 handshake / 证书一致 |
| `server_port` | 按服务端实际端口修改（示例 ST=443, SS=8388, AnyTLS=8443） |

### SS2022 密码生成

```bash
rsbox generate rand --base64 16
# 或 sing-box generate rand --base64 16
```

2022 系列 method 的 password 通常为 **base64 字符串**（16/32 字节随机数编码）。

### ShadowTLS+SS 拓扑

```
浏览器 → mixed:17890 → SS outbound → [detour: shadowtls-out] → VPS:443 (TLS 伪装)
                                                      ↓
                                              SS 127.0.0.1:8388 (服务端)
```

客户端 `shadowsocks.server` / `server_port` 填 **SS 服务地址**（通常与 VPS 同 IP，端口为 SS 监听端口）。  
`shadowtls.server` / `server_port` 填 **ShadowTLS 外层入口**（通常 443）。

## 快速测试（Windows）

```powershell
# 1. 编辑配置，填好占位符
notepad examples\config-shadowtls-ss.json

# 2. 校验 + 启动 + curl
.\scripts\test-protocol-config.ps1 -Config examples\config-shadowtls-ss.json
```

AnyTLS：

```powershell
.\scripts\test-protocol-config.ps1 -Config examples\config-anytls.json
```

## G5 客户端使用

1. 将填好参数的 JSON 复制为：
   `%APPDATA%\com.example\g5_client\config\runtime.json`
2. 确保 `%APPDATA%\com.example\g5_client\bin\rsbox.exe` 为最新构建
3. 在 G5 中连接 VPN（系统代理模式即可）

或使用合并配置 `config-test-shadowtls-ss-anytls.json`，在 selector 里切换 **ShadowTLS+SS** / **AnyTLS**。

## 服务端部署（sing-box）

rsbox 当前 **未实现** ShadowTLS / AnyTLS inbound，服务端请用官方 sing-box：

```bash
# ShadowTLS+SS
sing-box check -c examples/server/config-shadowtls-ss-server.json
sing-box run -c examples/server/config-shadowtls-ss-server.json

# AnyTLS（需自备 cert.pem / key.pem）
sing-box run -c examples/server/config-anytls-server.json
```

## 常见问题

- **TLS 握手失败**：检查 `tls.server_name` 是否与服务端 `handshake.server` 一致；自签证书需 `"insecure": true`
- **shadowtls v3 auth failed**：密码不匹配，或 `strict_mode` 与服务端不一致
- **SS 连接失败**：确认 `method` / `password` 与服务端一致；2022 密码格式为 base64
- **AnyTLS 超时**：确认端口、密码、TLS 证书；查看 `log.level: debug` 日志
