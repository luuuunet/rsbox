# rsbox vs sing-box 功能对比报告

## 生成时间
2026年6月26日 07:00

## 📊 功能对比总览

---

## 1. 入站协议 (Inbound Protocols)

| 协议 | sing-box | rsbox | 状态 | 优先级 |
|------|----------|-------|------|--------|
| Mixed (HTTP+SOCKS) | ✅ | ✅ | 完整 | - |
| HTTP | ✅ | ✅ | 完整 | - |
| SOCKS5 | ✅ | ✅ | 完整 | - |
| SOCKS4/4a | ✅ | ❌ | **缺失** | P2 |
| Shadowsocks | ✅ | ✅ | 完整 | - |
| VMess | ✅ | ❌ | **缺失** | P3 |
| Trojan | ✅ | ❌ | **缺失** | P2 |
| Naive | ✅ | ❌ | **缺失** | P3 |
| Hysteria | ✅ | ❌ | **缺失** | P3 |
| Hysteria2 | ✅ | ❌ | **缺失** | P3 |
| TUIC | ✅ | ❌ | **缺失** | P3 |
| TUN | ✅ | ❌ | **缺失** | P1 |
| Redirect | ✅ | ❌ | **缺失** | P2 |
| TProxy | ✅ | ❌ | **缺失** | P2 |

**总结**：
- ✅ 已实现：4/14 (29%)
- ❌ 缺失：10/14 (71%)

---

## 2. 出站协议 (Outbound Protocols)

| 协议 | sing-box | rsbox | 状态 | 优先级 |
|------|----------|-------|------|--------|
| Direct | ✅ | ✅ | 完整 | - |
| Block | ✅ | ✅ | 完整 | - |
| DNS | ✅ | ✅ | 完整 | - |
| Shadowsocks | ✅ | ✅ | 完整 | - |
| VMess | ✅ | ✅ | 完整 | - |
| VLESS | ✅ | ✅ | 完整 | - |
| Trojan | ✅ | ✅ | 完整 | - |
| WireGuard | ✅ | ✅ | 完整 | - |
| Hysteria | ✅ | ❌ | **缺失** | P3 |
| Hysteria2 | ✅ | ✅ | 完整 | - |
| TUIC | ✅ | ❌ | **缺失** | P2 |
| SSH | ✅ | ✅ | 完整 | - |
| Tor | ✅ | ❌ | **缺失** | P3 |
| Selector | ✅ | ✅ | 完整 | - |
| URLTest | ✅ | ✅ | 完整 | - |

**总结**：
- ✅ 已实现：12/15 (80%)
- ❌ 缺失：3/15 (20%)

---

## 3. 路由功能 (Routing)

| 功能 | sing-box | rsbox | 状态 | 优先级 |
|------|----------|-------|------|--------|
| Domain 规则 | ✅ | ✅ | 完整 | - |
| Domain Suffix | ✅ | ✅ | 完整 | - |
| Domain Keyword | ✅ | ✅ | 完整 | - |
| Domain Regex | ✅ | ✅ | 完整 | - |
| GeoIP | ✅ | ✅ | 完整 | - |
| GeoSite | ✅ | ✅ | 完整 | - |
| IP CIDR | ✅ | ✅ | 完整 | - |
| Source IP | ✅ | ❌ | **缺失** | P2 |
| Source Port | ✅ | ❌ | **缺失** | P2 |
| Port | ✅ | ✅ | 完整 | - |
| Port Range | ✅ | ❌ | **缺失** | P2 |
| Process Name | ✅ | ❌ | **缺失** | P2 |
| Process Path | ✅ | ❌ | **缺失** | P2 |
| User ID | ✅ | ❌ | **缺失** | P3 |
| Network Type | ✅ | ✅ | 完整 | - |
| Protocol | ✅ | ✅ | 完整 | - |
| Inbound | ✅ | ✅ | 完整 | - |
| Rule Set | ✅ | ❌ | **缺失** | P1 |
| Clash Mode | ✅ | ❌ | **缺失** | P2 |

**总结**：
- ✅ 已实现：11/19 (58%)
- ❌ 缺失：8/19 (42%)

---

## 4. DNS 功能

| 功能 | sing-box | rsbox | 状态 | 优先级 |
|------|----------|-------|------|--------|
| 基础 DNS | ✅ | ✅ | 完整 | - |
| DNS over UDP | ✅ | ✅ | 完整 | - |
| DNS over TCP | ✅ | ❌ | **缺失** | P2 |
| DNS over TLS | ✅ | ❌ | **缺失** | P1 |
| DNS over HTTPS | ✅ | ❌ | **缺失** | P1 |
| DNS over QUIC | ✅ | ❌ | **缺失** | P2 |
| DNS over H3 | ✅ | ❌ | **缺失** | P3 |
| FakeIP | ✅ | ❌ | **缺失** | P1 |
| DNS 缓存 | ✅ | ✅ | 完整 | - |
| DNS 规则 | ✅ | ❌ | **缺失** | P1 |
| 反劫持 | ✅ | ⚠️ | 部分实现 | P1 |
| 广告过滤 | ✅ | ⚠️ | 部分实现 | P2 |

**总结**：
- ✅ 已实现：3/12 (25%)
- ⚠️ 部分实现：2/12 (17%)
- ❌ 缺失：7/12 (58%)

---

## 5. TLS/传输层

| 功能 | sing-box | rsbox | 状态 | 优先级 |
|------|----------|-------|------|--------|
| TLS | ✅ | ✅ | 完整 | - |
| XTLS | ✅ | ⚠️ | 部分实现 | P2 |
| Reality | ✅ | ⚠️ | 部分实现 | P2 |
| uTLS | ✅ | ✅ | 完整 | - |
| WebSocket | ✅ | ❌ | **缺失** | P1 |
| HTTP/2 | ✅ | ❌ | **缺失** | P2 |
| HTTP/3 | ✅ | ⚠️ | 部分实现 | P2 |
| gRPC | ✅ | ❌ | **缺失** | P2 |
| HTTPUpgrade | ✅ | ❌ | **缺失** | P2 |
| Multiplex | ✅ | ❌ | **缺失** | P1 |

**总结**：
- ✅ 已实现：3/10 (30%)
- ⚠️ 部分实现：3/10 (30%)
- ❌ 缺失：4/10 (40%)

---

## 6. 控制 API

| 功能 | sing-box | rsbox | 状态 | 优先级 |
|------|----------|-------|------|--------|
| RESTful API | ✅ | ✅ | 完整 | - |
| gRPC API | ✅ | ⚠️ | 无鉴权 | P1 |
| Clash API | ✅ | ❌ | **缺失** | P2 |
| 连接管理 | ✅ | ✅ | 完整 | - |
| 流量统计 | ✅ | ❌ | **缺失** | P1 |
| 日志查询 | ✅ | ❌ | **缺失** | P2 |
| 配置热重载 | ✅ | ❌ | **缺失** | P1 |
| 健康检查 | ✅ | ✅ | 完整 | - |
| 节点延迟测试 | ✅ | ✅ | 完整 | - |

**总结**：
- ✅ 已实现：4/9 (44%)
- ⚠️ 部分实现：1/9 (11%)
- ❌ 缺失：4/9 (44%)

---

## 7. 高级功能

| 功能 | sing-box | rsbox | 状态 | 优先级 |
|------|----------|-------|------|--------|
| TUN 模式 | ✅ | ❌ | **缺失** | P0 |
| 系统代理设置 | ✅ | ❌ | **缺失** | P1 |
| 进程规则 | ✅ | ❌ | **缺失** | P2 |
| 流量统计 | ✅ | ❌ | **缺失** | P1 |
| 规则集订阅 | ✅ | ❌ | **缺失** | P1 |
| 节点订阅 | ✅ | ❌ | **缺失** | P0 |
| 分流订阅 | ✅ | ❌ | **缺失** | P1 |
| 延迟测试 | ✅ | ✅ | 完整 | - |
| 自动选择 | ✅ | ✅ | 完整 | - |
| 负载均衡 | ✅ | ❌ | **缺失** | P2 |
| 故障转移 | ✅ | ❌ | **缺失** | P2 |
| 链式代理 | ✅ | ❌ | **缺失** | P2 |

**总结**：
- ✅ 已实现：2/12 (17%)
- ❌ 缺失：10/12 (83%)

---

## 🎯 优先级分类

### P0 - 关键缺失（严重影响使用）

1. **TUN 入站** ⚠️
   - 系统级代理
   - 透明代理
   - 全局流量接管

2. **节点订阅** ⚠️
   - 从 URL 导入节点
   - 自动更新
   - 多订阅源管理

### P1 - 重要缺失（显著影响体验）

1. **Rule Set（规则集）** ⚠️
   - 远程规则集
   - 规则集更新
   - 规则集编译

2. **DNS over TLS/HTTPS** ⚠️
   - DoT 支持
   - DoH 支持
   - 加密 DNS

3. **FakeIP** ⚠️
   - FakeIP 池管理
   - DNS 映射
   - 性能优化

4. **WebSocket 传输** ⚠️
   - CDN 友好
   - 防火墙穿透

5. **Multiplex（多路复用）** ⚠️
   - smux
   - yamux
   - 连接复用

6. **流量统计** ⚠️
   - 上传/下载统计
   - 按节点统计
   - 实时统计

7. **配置热重载** ⚠️
   - 无需重启
   - 平滑切换

8. **gRPC 鉴权** ⚠️
   - Token 认证
   - API 安全

### P2 - 一般缺失（有用但非必需）

1. **SOCKS4/4a 入站**
2. **Trojan 入站**
3. **TUIC 出站**
4. **Source IP/Port 路由**
5. **Process Name/Path 路由**
6. **XTLS 完善**
7. **Reality 完善**
8. **负载均衡**
9. **故障转移**

### P3 - 低优先级（可选功能）

1. **VMess/Hysteria/TUIC 入站**
2. **Tor 出站**
3. **Clash Mode**
4. **链式代理**

---

## 📈 总体完成度

| 类别 | 已实现 | 部分实现 | 缺失 | 完成度 |
|------|--------|---------|------|--------|
| **入站协议** | 4 | 0 | 10 | 29% |
| **出站协议** | 12 | 0 | 3 | 80% |
| **路由功能** | 11 | 0 | 8 | 58% |
| **DNS 功能** | 3 | 2 | 7 | 42% |
| **传输层** | 3 | 3 | 4 | 60% |
| **控制 API** | 4 | 1 | 4 | 56% |
| **高级功能** | 2 | 0 | 10 | 17% |
| **总体** | **39** | **6** | **46** | **49%** |

---

## 🛠️ 实施计划

### 阶段 1：关键功能（P0，1-2 周）

#### 1.1 TUN 入站
```rust
// crates/rsb-protocol/src/tun_inbound.rs
pub struct TunInbound {
    name: String,
    address: Vec<IpAddr>,
    auto_route: bool,
    mtu: u32,
}
```

#### 1.2 节点订阅
```rust
// crates/rsb-core/src/subscription.rs
pub struct Subscription {
    url: String,
    update_interval: Duration,
    user_agent: String,
}

impl Subscription {
    pub async fn fetch(&self) -> Result<Vec<Outbound>>;
    pub async fn parse_base64(&self, content: &str) -> Result<Vec<Outbound>>;
}
```

### 阶段 2：重要功能（P1，2-3 周）

#### 2.1 Rule Set
```rust
// crates/rsb-route/src/rule_set.rs
pub struct RuleSet {
    tag: String,
    type_: RuleSetType, // local, remote
    format: RuleSetFormat, // binary, json
    url: Option<String>,
    download_detour: Option<String>,
    update_interval: Duration,
}
```

#### 2.2 DNS over HTTPS
```rust
// crates/rsb-dns/src/doh.rs
pub struct DohClient {
    url: String,
    client: reqwest::Client,
}

impl DohClient {
    pub async fn query(&self, domain: &str) -> Result<Vec<IpAddr>>;
}
```

#### 2.3 FakeIP
```rust
// crates/rsb-dns/src/fakeip.rs
pub struct FakeIpPool {
    inet4_range: Ipv4Net,
    inet6_range: Ipv6Net,
    mapping: HashMap<String, IpAddr>,
}
```

#### 2.4 WebSocket 传输
```rust
// crates/rsb-protocol/src/transport/websocket.rs
pub struct WebSocketTransport {
    uri: String,
    headers: HeaderMap,
    max_early_data: usize,
    early_data_header_name: String,
}
```

#### 2.5 Multiplex
```rust
// crates/rsb-protocol/src/transport/multiplex.rs
pub enum MultiplexProtocol {
    Smux,
    Yamux,
    H2Mux,
}

pub struct MultiplexClient {
    protocol: MultiplexProtocol,
    max_connections: usize,
    max_streams: usize,
}
```

#### 2.6 流量统计
```rust
// crates/rsb-core/src/stats.rs
pub struct TrafficStats {
    uplink: AtomicU64,
    downlink: AtomicU64,
    by_outbound: DashMap<String, (AtomicU64, AtomicU64)>,
}
```

#### 2.7 配置热重载
```rust
// crates/rsb-core/src/runtime.rs
impl Runtime {
    pub async fn reload_config(&self, new_config: Options) -> Result<()>;
}
```

#### 2.8 gRPC 鉴权
```rust
// crates/rsb-protocol/src/services/api_grpc.rs
use tonic::{Request, Status};
use tonic::metadata::MetadataValue;

fn check_auth(req: Request<()>) -> Result<Request<()>, Status> {
    let token = req.metadata().get("authorization")?;
    // 验证 token
    Ok(req)
}
```

### 阶段 3：一般功能（P2，3-5 周）

- SOCKS4/4a 入站
- Trojan 入站
- TUIC 出站
- Source IP/Port 路由
- Process 路由
- 负载均衡
- 故障转移

---

## 📝 代码示例

### 1. TUN 入站实现

```rust
// crates/rsb-protocol/src/tun_inbound.rs
use tun::TunSocket;
use ipstack::stream::IpStackStream;

pub struct TunInbound {
    tag: String,
    name: String,
    address: Vec<IpAddr>,
    mtu: u32,
    auto_route: bool,
    stack: String, // system, gvisor, mixed
}

impl TunInbound {
    pub async fn start(&self, ctx: ServiceContext) -> Result<()> {
        let mut config = tun::Configuration::default();
        config.name(&self.name)
            .address_with_prefix(self.address[0], 32)
            .mtu(self.mtu as i32)
            .up();
        
        #[cfg(target_os = "linux")]
        config.platform(|config| {
            config.packet_information(true);
        });
        
        let device = tun::create_as_async(&config)?;
        
        // 设置路由
        if self.auto_route {
            self.setup_routes()?;
        }
        
        // 处理数据包
        let mut stack = ipstack::IpStack::new(device);
        loop {
            match stack.accept().await? {
                IpStackStream::Tcp(tcp) => {
                    let ctx = ctx.clone();
                    tokio::spawn(async move {
                        handle_tcp(tcp, ctx).await;
                    });
                }
                IpStackStream::Udp(udp) => {
                    let ctx = ctx.clone();
                    tokio::spawn(async move {
                        handle_udp(udp, ctx).await;
                    });
                }
            }
        }
    }
    
    #[cfg(target_os = "linux")]
    fn setup_routes(&self) -> Result<()> {
        use std::process::Command;
        
        // 添加默认路由
        Command::new("ip")
            .args(&["route", "add", "default", "dev", &self.name])
            .output()?;
        
        Ok(())
    }
}
```

### 2. 节点订阅实现

```rust
// crates/rsb-core/src/subscription.rs
use base64::{Engine as _, engine::general_purpose};

pub struct Subscription {
    url: String,
    user_agent: String,
    update_interval: Duration,
}

impl Subscription {
    pub async fn fetch(&self) -> Result<Vec<Outbound>> {
        let client = reqwest::Client::builder()
            .user_agent(&self.user_agent)
            .timeout(Duration::from_secs(30))
            .build()?;
        
        let response = client.get(&self.url).send().await?;
        let content = response.text().await?;
        
        // 检测格式
        if content.starts_with("vmess://") || content.starts_with("vless://") {
            self.parse_share_links(&content)
        } else {
            // Base64 编码的订阅
            let decoded = general_purpose::STANDARD.decode(content.trim())?;
            let decoded_str = String::from_utf8(decoded)?;
            self.parse_share_links(&decoded_str)
        }
    }
    
    fn parse_share_links(&self, content: &str) -> Result<Vec<Outbound>> {
        let mut outbounds = Vec::new();
        
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            
            if line.starts_with("vmess://") {
                outbounds.push(self.parse_vmess(line)?);
            } else if line.starts_with("vless://") {
                outbounds.push(self.parse_vless(line)?);
            } else if line.starts_with("ss://") {
                outbounds.push(self.parse_shadowsocks(line)?);
            } else if line.starts_with("trojan://") {
                outbounds.push(self.parse_trojan(line)?);
            } else if line.starts_with("hysteria2://") {
                outbounds.push(self.parse_hysteria2(line)?);
            }
        }
        
        Ok(outbounds)
    }
    
    fn parse_vmess(&self, url: &str) -> Result<Outbound> {
        // 实现 VMess 解析
        todo!()
    }
}
```

### 3. DNS over HTTPS 实现

```rust
// crates/rsb-dns/src/doh.rs
use hickory_proto::rr::{DNSClass, Name, RecordType};

pub struct DohClient {
    url: String,
    client: reqwest::Client,
}

impl DohClient {
    pub async fn query(&self, domain: &str, record_type: RecordType) -> Result<Vec<IpAddr>> {
        let name = Name::from_utf8(domain)?;
        
        // 构建 DNS 查询
        let mut message = Message::new();
        message.add_query(Query::query(name, record_type));
        message.set_recursion_desired(true);
        
        // 序列化为 wire format
        let query_bytes = message.to_vec()?;
        
        // 发送 DoH 请求
        let response = self.client
            .post(&self.url)
            .header("Content-Type", "application/dns-message")
            .body(query_bytes)
            .send()
            .await?;
        
        let response_bytes = response.bytes().await?;
        let response_msg = Message::from_vec(&response_bytes)?;
        
        // 解析响应
        let mut addrs = Vec::new();
        for answer in response_msg.answers() {
            match answer.data() {
                Some(RData::A(a)) => addrs.push(IpAddr::V4(a.0)),
                Some(RData::AAAA(aaaa)) => addrs.push(IpAddr::V6(aaaa.0)),
                _ => {}
            }
        }
        
        Ok(addrs)
    }
}
```

### 4. FakeIP 实现

```rust
// crates/rsb-dns/src/fakeip.rs
use ipnet::{Ipv4Net, Ipv6Net};

pub struct FakeIpPool {
    inet4_range: Ipv4Net,
    inet6_range: Ipv6Net,
    inet4_offset: AtomicU32,
    inet6_offset: AtomicU128,
    domain_to_ip: DashMap<String, IpAddr>,
    ip_to_domain: DashMap<IpAddr, String>,
}

impl FakeIpPool {
    pub fn new(inet4_range: &str, inet6_range: &str) -> Result<Self> {
        Ok(Self {
            inet4_range: inet4_range.parse()?,
            inet6_range: inet6_range.parse()?,
            inet4_offset: AtomicU32::new(1),
            inet6_offset: AtomicU128::new(1),
            domain_to_ip: DashMap::new(),
            ip_to_domain: DashMap::new(),
        })
    }
    
    pub fn lookup(&self, domain: &str) -> IpAddr {
        if let Some(ip) = self.domain_to_ip.get(domain) {
            return *ip;
        }
        
        let ip = self.allocate_ipv4();
        self.domain_to_ip.insert(domain.to_string(), ip);
        self.ip_to_domain.insert(ip, domain.to_string());
        ip
    }
    
    pub fn reverse_lookup(&self, ip: &IpAddr) -> Option<String> {
        self.ip_to_domain.get(ip).map(|s| s.clone())
    }
    
    fn allocate_ipv4(&self) -> IpAddr {
        let offset = self.inet4_offset.fetch_add(1, Ordering::Relaxed);
        let base = u32::from(self.inet4_range.network());
        let ip = Ipv4Addr::from(base + offset);
        IpAddr::V4(ip)
    }
}
```

---

**报告生成时间**：2026-06-26 07:00  
**对比版本**：sing-box v1.9.x vs rsbox v0.1.0  
**总体完成度**：49%

---

**🎯 下一步：按优先级实施缺失功能！**
