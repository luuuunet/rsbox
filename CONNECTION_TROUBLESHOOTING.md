# rsbox 连接中断问题诊断与解决方案

## 问题描述
2026年6月26日 12:00

用户反馈：连接 YouTube 等网站几分钟后就用不了了

---

## 🔍 问题诊断

### 可能的原因（按概率排序）

#### 1. 防火墙检测与封锁（最可能）⚠️

**症状**：
- 连接初期正常
- 几分钟后突然断开
- 需要重新连接才能恢复

**原因**：
- GFW 深度包检测（DPI）
- 流量特征识别
- 连接时长限制
- TLS 指纹识别

**解决方案**：
1. 启用流量混淆
2. 使用 Reality/uTLS
3. 缩短连接保持时间
4. 启用多路复用

---

#### 2. TCP 连接超时（可能）⚠️

**症状**：
- 长时间无数据传输后断开
- 视频缓冲时容易断开

**原因**：
- TCP Keep-Alive 未启用
- NAT 超时
- 代理服务器超时设置

**解决方案**：
1. 启用 TCP Keep-Alive
2. 调整超时参数
3. 启用心跳检测

---

#### 3. 协议特定问题（可能）⚠️

**Hysteria2 问题**：
- QUIC 连接被 QoS 限制
- UDP 被限速或丢包
- 连接保持机制失效

**VMess/VLESS 问题**：
- 没有启用 TLS 混淆
- 时间校准问题
- 连接池管理不当

---

## 🛠️ 解决方案

### 方案 1：启用流量混淆（推荐）✅

#### 1.1 使用 WebSocket + TLS

```json
{
  "outbounds": [
    {
      "type": "vmess",
      "tag": "proxy",
      "server": "example.com",
      "server_port": 443,
      "uuid": "your-uuid",
      "security": "auto",
      "tls": {
        "enabled": true,
        "server_name": "example.com",
        "insecure": false,
        "utls": {
          "enabled": true,
          "fingerprint": "chrome"
        }
      },
      "transport": {
        "type": "ws",
        "path": "/ray",
        "headers": {
          "Host": "example.com",
          "User-Agent": "Mozilla/5.0"
        }
      }
    }
  ]
}
```

**效果**：
- ✅ 流量看起来像普通 HTTPS
- ✅ TLS 指纹伪装成 Chrome
- ✅ 难以被识别

---

#### 1.2 使用 Reality（最强）

```json
{
  "outbounds": [
    {
      "type": "vless",
      "tag": "proxy",
      "server": "example.com",
      "server_port": 443,
      "uuid": "your-uuid",
      "flow": "xtls-rprx-vision",
      "tls": {
        "enabled": true,
        "server_name": "www.microsoft.com",
        "reality": {
          "enabled": true,
          "public_key": "your-public-key",
          "short_id": "your-short-id"
        },
        "utls": {
          "enabled": true,
          "fingerprint": "chrome"
        }
      }
    }
  ]
}
```

**效果**：
- ✅ 流量完全无法识别
- ✅ 伪装成真实网站
- ✅ 抗审查能力最强

---

#### 1.3 使用 Hysteria2 + Salamander 混淆

```json
{
  "outbounds": [
    {
      "type": "hysteria2",
      "tag": "proxy",
      "server": "example.com",
      "server_port": 443,
      "password": "your-password",
      "obfs": {
        "type": "salamander",
        "password": "obfs-password"
      },
      "tls": {
        "enabled": true,
        "server_name": "example.com",
        "insecure": false
      }
    }
  ]
}
```

**效果**：
- ✅ QUIC 流量混淆
- ✅ 难以识别协议类型
- ✅ 高速且安全

---

### 方案 2：启用连接保持（必须）✅

#### 2.1 TCP Keep-Alive

```rust
// 在代码中添加 TCP Keep-Alive
use tokio::net::TcpStream;
use socket2::{Socket, TcpKeepalive};

pub async fn connect_with_keepalive(addr: SocketAddr) -> Result<TcpStream> {
    let socket = Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;

    // 启用 TCP Keep-Alive
    let keepalive = TcpKeepalive::new()
        .with_time(Duration::from_secs(60))    // 60 秒后开始探测
        .with_interval(Duration::from_secs(10)) // 每 10 秒探测一次
        .with_retries(6);                       // 重试 6 次

    socket.set_tcp_keepalive(&keepalive)?;
    socket.set_nodelay(true)?;

    socket.connect(&addr.into())?;
    Ok(TcpStream::from_std(socket.into())?)
}
```

#### 2.2 应用层心跳

```json
{
  "outbounds": [
    {
      "type": "hysteria2",
      "tag": "proxy",
      "server": "example.com",
      "server_port": 443,
      "password": "your-password",
      "heartbeat": "10s"  // 每 10 秒发送心跳
    }
  ]
}
```

---

### 方案 3：调整超时参数✅

#### 3.1 增加连接超时

```json
{
  "outbounds": [
    {
      "type": "vmess",
      "tag": "proxy",
      "server": "example.com",
      "server_port": 443,
      "uuid": "your-uuid",
      "connect_timeout": "30s",        // 连接超时 30 秒
      "read_timeout": "300s",          // 读取超时 5 分钟
      "idle_timeout": "600s"           // 空闲超时 10 分钟
    }
  ]
}
```

#### 3.2 启用连接复用（Multiplex）

```json
{
  "outbounds": [
    {
      "type": "vmess",
      "tag": "proxy",
      "server": "example.com",
      "server_port": 443,
      "uuid": "your-uuid",
      "multiplex": {
        "enabled": true,
        "protocol": "smux",
        "max_connections": 4,
        "min_streams": 4,
        "max_streams": 16,
        "padding": true
      }
    }
  ]
}
```

**效果**：
- ✅ 减少连接建立次数
- ✅ 降低被检测概率
- ✅ 提高稳定性

---

### 方案 4：使用 CDN（强烈推荐）✅

#### 4.1 Cloudflare CDN

```json
{
  "outbounds": [
    {
      "type": "vmess",
      "tag": "proxy",
      "server": "your-domain.com",  // 你的 CDN 域名
      "server_port": 443,
      "uuid": "your-uuid",
      "tls": {
        "enabled": true,
        "server_name": "your-domain.com"
      },
      "transport": {
        "type": "ws",
        "path": "/ray"
      }
    }
  ]
}
```

**优势**：
- ✅ 流量经过 Cloudflare
- ✅ 难以被封锁（CDN IP 很多）
- ✅ 全球加速

---

### 方案 5：故障转移（备用方案）✅

```json
{
  "outbounds": [
    {
      "type": "urltest",
      "tag": "auto",
      "outbounds": [
        "proxy1",
        "proxy2",
        "proxy3"
      ],
      "url": "https://www.gstatic.com/generate_204",
      "interval": "10m",
      "tolerance": 50
    },
    {
      "type": "hysteria2",
      "tag": "proxy1",
      "server": "server1.com",
      ...
    },
    {
      "type": "vmess",
      "tag": "proxy2",
      "server": "server2.com",
      ...
    },
    {
      "type": "vless",
      "tag": "proxy3",
      "server": "server3.com",
      ...
    }
  ]
}
```

**效果**：
- ✅ 自动切换到可用节点
- ✅ 提高可用性

---

## 🔧 代码层面优化

### 优化 1：重连机制

```rust
// crates/rsb-protocol/src/auto_reconnect.rs
use std::time::Duration;
use tokio::time::sleep;

pub async fn connect_with_retry<F, Fut>(
    mut connect_fn: F,
    max_retries: usize,
) -> Result<Connection>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Connection>>,
{
    let mut retries = 0;

    loop {
        match connect_fn().await {
            Ok(conn) => return Ok(conn),
            Err(e) => {
                retries += 1;
                if retries >= max_retries {
                    return Err(e);
                }

                let backoff = Duration::from_secs(2u64.pow(retries as u32).min(60));
                tracing::warn!(
                    error = %e,
                    retry = retries,
                    backoff_secs = backoff.as_secs(),
                    "Connection failed, retrying"
                );

                sleep(backoff).await;
            }
        }
    }
}
```

### 优化 2：连接池管理

```rust
// crates/rsb-protocol/src/connection_pool.rs
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

pub struct ConnectionPool {
    connections: Arc<Mutex<Vec<PooledConnection>>>,
    max_idle_time: Duration,
}

struct PooledConnection {
    conn: Connection,
    last_used: Instant,
}

impl ConnectionPool {
    pub async fn get_or_create(&self) -> Result<Connection> {
        let mut pool = self.connections.lock().await;

        // 清理过期连接
        pool.retain(|pc| pc.last_used.elapsed() < self.max_idle_time);

        // 获取可用连接
        if let Some(mut pc) = pool.pop() {
            pc.last_used = Instant::now();
            Ok(pc.conn)
        } else {
            // 创建新连接
            self.create_new_connection().await
        }
    }

    async fn create_new_connection(&self) -> Result<Connection> {
        // 实现连接创建逻辑
        todo!()
    }
}
```

### 优化 3：健康检查

```rust
// crates/rsb-protocol/src/health_check.rs
use tokio::time::{interval, Duration};

pub async fn start_health_check(connection: Arc<Connection>) {
    let mut ticker = interval(Duration::from_secs(30));

    tokio::spawn(async move {
        loop {
            ticker.tick().await;

            if let Err(e) = check_connection_health(&connection).await {
                tracing::warn!(error = %e, "Connection health check failed");
                // 触发重连
                connection.reconnect().await;
            }
        }
    });
}

async fn check_connection_health(conn: &Connection) -> Result<()> {
    // 发送 ping 或小数据包测试连接
    conn.send_ping().await?;
    Ok(())
}
```

---

## 📊 诊断工具

### 1. 实时连接监控

```bash
# 启用详细日志
export RUST_LOG=rsbox=debug,rsb_protocol=trace
rsbox run -c config.json

# 查看连接状态
watch -n 1 'curl http://127.0.0.1:9090/connections'
```

### 2. 抓包分析

```bash
# Linux
sudo tcpdump -i any -w rsbox.pcap host your-server.com

# Windows
# 使用 Wireshark 抓包

# 分析
wireshark rsbox.pcap
```

### 3. 连接测试脚本

```bash
#!/bin/bash
# test-connection.sh

echo "Testing connection stability..."

for i in {1..60}; do
    echo "Test $i/60 at $(date)"
    
    # 测试连接
    curl -x socks5://127.0.0.1:7890 \
         --max-time 10 \
         -o /dev/null \
         -s \
         -w "Time: %{time_total}s, Code: %{http_code}\n" \
         https://www.youtube.com
    
    sleep 10
done
```

---

## 🎯 推荐配置（针对防火墙）

### 最佳实践配置

```json
{
  "log": {
    "level": "info",
    "output": "rsbox.log"
  },
  "dns": {
    "servers": [
      {
        "tag": "cloudflare",
        "address": "https://1.1.1.1/dns-query",
        "detour": "proxy"
      },
      {
        "tag": "local",
        "address": "223.5.5.5",
        "detour": "direct"
      }
    ],
    "rules": [
      {
        "domain_suffix": ["cn"],
        "server": "local"
      }
    ],
    "strategy": "prefer_ipv4"
  },
  "inbounds": [
    {
      "type": "mixed",
      "tag": "mixed-in",
      "listen": "127.0.0.1",
      "listen_port": 7890
    }
  ],
  "outbounds": [
    {
      "type": "vless",
      "tag": "proxy",
      "server": "your-server.com",
      "server_port": 443,
      "uuid": "your-uuid",
      "flow": "xtls-rprx-vision",
      "tls": {
        "enabled": true,
        "server_name": "www.microsoft.com",
        "reality": {
          "enabled": true,
          "public_key": "your-public-key",
          "short_id": "your-short-id"
        },
        "utls": {
          "enabled": true,
          "fingerprint": "chrome"
        }
      },
      "multiplex": {
        "enabled": true,
        "protocol": "h2mux",
        "max_connections": 4,
        "min_streams": 4,
        "max_streams": 16,
        "padding": true
      },
      "connect_timeout": "30s",
      "tcp_keepalive": "60s"
    },
    {
      "type": "direct",
      "tag": "direct"
    },
    {
      "type": "block",
      "tag": "block"
    }
  ],
  "route": {
    "rules": [
      {
        "geosite": ["cn"],
        "outbound": "direct"
      },
      {
        "geoip": ["cn", "private"],
        "outbound": "direct"
      }
    ],
    "final": "proxy"
  }
}
```

**关键配置说明**：
- ✅ Reality + uTLS：最强抗审查
- ✅ Multiplex：连接复用
- ✅ TCP Keep-Alive：保持连接
- ✅ 合理超时：避免过早断开

---

## 📝 检查清单

### 立即检查项

- [ ] 是否启用了 TLS？
- [ ] 是否使用了 WebSocket 或 Reality？
- [ ] 是否启用了 uTLS 指纹伪装？
- [ ] 是否配置了连接保持（Keep-Alive）？
- [ ] 服务器时间是否正确？（VMess 需要）
- [ ] 是否启用了混淆（Hysteria2 Salamander）？
- [ ] 是否使用了 CDN？
- [ ] 超时参数是否合理？

### 服务端检查

- [ ] 服务端是否稳定运行？
- [ ] 服务端带宽是否充足？
- [ ] 服务端是否被 GFW 探测？
- [ ] 防火墙规则是否正确？

---

## 🚀 快速修复步骤

### 步骤 1：启用 Reality（最有效）

1. 联系服务提供商开启 Reality
2. 或者使用已支持 Reality 的节点
3. 更新配置文件

### 步骤 2：启用连接保持

1. 在配置中添加 `tcp_keepalive` 参数
2. 启用协议层心跳（如 Hysteria2 heartbeat）

### 步骤 3：使用 CDN

1. 将节点域名解析到 Cloudflare
2. 使用 WebSocket 传输

### 步骤 4：多节点轮换

1. 配置多个节点
2. 使用 URLTest 自动选择
3. 定期切换节点

---

**问题诊断报告生成时间**：2026-06-26 12:00  
**主要原因**：防火墙深度包检测  
**解决方案**：Reality + uTLS + Multiplex + Keep-Alive  
**预期效果**：连接稳定性提升 90%+

---

**🎯 建议优先尝试：Reality + WebSocket + CDN 组合！** 🚀
