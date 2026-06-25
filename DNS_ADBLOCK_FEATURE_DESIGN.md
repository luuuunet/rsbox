# DNS 污染防护与广告屏蔽功能设计方案

## 设计时间
2026年6月25日

## 📋 功能需求

### 1. DNS 污染防护（中国特色）
- 防止 DNS 劫持和污染
- 支持 DoH/DoT 加密查询
- 可信 DNS 列表
- DNS 缓存优化

### 2. 广告屏蔽功能
- 基于域名黑名单屏蔽
- 广告过滤规则支持
- 自定义规则
- 定期更新规则

---

## 🎯 实现方案

### 方案 A：DNS 污染防护

#### 1.1 配置结构

```json
{
  "dns": {
    "servers": [
      {
        "address": "https://1.1.1.1/dns-query",
        "tag": "cloudflare-doh",
        "strategy": "prefer_ipv4"
      },
      {
        "address": "tls://8.8.8.8",
        "tag": "google-dot"
      }
    ],
    "rules": [
      {
        "domain": ["cn", "taobao.com", "qq.com"],
        "server": "local"
      },
      {
        "domain_suffix": [".cn"],
        "server": "local"
      },
      {
        "geoip": ["cn"],
        "server": "local"
      },
      {
        "server": "cloudflare-doh"
      }
    ],
    "anti_pollution": {
      "enabled": true,
      "trusted_servers": ["cloudflare-doh", "google-dot"],
      "check_method": "dual_query",
      "ttl_override": 300
    }
  }
}
```

#### 1.2 实现要点

**新增模块**：`crates/rsb-dns/src/anti_pollution.rs`

```rust
pub struct AntiPollutionConfig {
    pub enabled: bool,
    pub trusted_servers: Vec<String>,
    pub check_method: CheckMethod,
    pub ttl_override: Option<u32>,
}

pub enum CheckMethod {
    DualQuery,      // 同时查询多个 DNS，对比结果
    TrustedOnly,    // 只使用可信 DNS
    Fallback,       // 本地失败则使用可信 DNS
}

impl AntiPollution {
    pub async fn resolve(&self, domain: &str) -> Result<Vec<IpAddr>> {
        match self.config.check_method {
            CheckMethod::DualQuery => {
                // 同时查询本地 DNS 和可信 DNS
                // 如果结果差异大，使用可信结果
                let (local, trusted) = tokio::join!(
                    self.query_local(domain),
                    self.query_trusted(domain)
                );
                self.validate_results(local, trusted)
            }
            CheckMethod::TrustedOnly => {
                // 直接使用可信 DNS
                self.query_trusted(domain).await
            }
            CheckMethod::Fallback => {
                // 先本地，失败则可信
                match self.query_local(domain).await {
                    Ok(addrs) if self.is_valid(&addrs) => Ok(addrs),
                    _ => self.query_trusted(domain).await,
                }
            }
        }
    }

    fn validate_results(
        &self,
        local: Result<Vec<IpAddr>>,
        trusted: Result<Vec<IpAddr>>
    ) -> Result<Vec<IpAddr>> {
        // 检测污染特征：
        // 1. 返回特定 IP 段（如 10.x.x.x, 127.x.x.x）
        // 2. 返回已知污染 IP
        // 3. 结果与可信 DNS 差异过大
        
        if let Ok(local_addrs) = local {
            if !self.is_polluted(&local_addrs) {
                return Ok(local_addrs);
            }
        }
        
        trusted
    }

    fn is_polluted(&self, addrs: &[IpAddr]) -> bool {
        for addr in addrs {
            // 检查是否为污染特征 IP
            if self.is_poison_ip(addr) {
                return true;
            }
        }
        false
    }

    fn is_poison_ip(&self, addr: &IpAddr) -> bool {
        match addr {
            IpAddr::V4(ip) => {
                let octets = ip.octets();
                // 常见污染 IP 特征
                matches!(octets[0], 10 | 127 | 0)
                    || (octets[0] == 203 && octets[1] == 98)  // 某些特定段
                    || (octets[0] == 159 && octets[1] == 226)
            }
            _ => false,
        }
    }
}
```

---

### 方案 B：广告屏蔽功能

#### 2.1 配置结构

```json
{
  "dns": {
    "adblock": {
      "enabled": true,
      "rules": [
        "https://raw.githubusercontent.com/privacy-protection-tools/anti-AD/master/anti-ad-domains.txt",
        "/path/to/local/adblock.txt"
      ],
      "custom_rules": [
        "||ads.example.com^",
        "||tracker.example.com^"
      ],
      "update_interval": 86400,
      "action": "reject"
    }
  }
}
```

#### 2.2 实现要点

**新增模块**：`crates/rsb-dns/src/adblock.rs`

```rust
pub struct AdBlockConfig {
    pub enabled: bool,
    pub rules: Vec<String>,      // 规则来源 URL/文件
    pub custom_rules: Vec<String>,
    pub update_interval: u64,
    pub action: BlockAction,
}

pub enum BlockAction {
    Reject,         // 直接拒绝
    Nxdomain,      // 返回域名不存在
    ReturnEmpty,   // 返回空结果
    ReturnLocal,   // 返回 127.0.0.1
}

pub struct AdBlockFilter {
    domains: HashSet<String>,
    patterns: Vec<Pattern>,
    last_update: std::time::Instant,
}

impl AdBlockFilter {
    pub async fn load_rules(&mut self) -> Result<()> {
        for rule_source in &self.config.rules {
            let rules = if rule_source.starts_with("http") {
                self.download_rules(rule_source).await?
            } else {
                self.load_file(rule_source)?
            };
            
            self.parse_rules(&rules);
        }
        
        // 加载自定义规则
        for rule in &self.config.custom_rules {
            self.parse_rule(rule);
        }
        
        Ok(())
    }

    fn parse_rule(&mut self, rule: &str) {
        // 支持多种规则格式
        if rule.starts_with("||") {
            // AdBlock Plus 格式：||example.com^
            let domain = rule.trim_start_matches("||").trim_end_matches('^');
            self.domains.insert(domain.to_string());
        } else if rule.starts_with("@@") {
            // 白名单规则
            // TODO: 实现白名单
        } else {
            // 简单域名格式
            self.domains.insert(rule.to_string());
        }
    }

    pub fn should_block(&self, domain: &str) -> bool {
        // 精确匹配
        if self.domains.contains(domain) {
            return true;
        }
        
        // 子域名匹配
        for blocked in &self.domains {
            if domain.ends_with(&format!(".{}", blocked)) {
                return true;
            }
        }
        
        // 模式匹配
        for pattern in &self.patterns {
            if pattern.matches(domain) {
                return true;
            }
        }
        
        false
    }

    pub async fn auto_update(&mut self) {
        if self.last_update.elapsed().as_secs() > self.config.update_interval {
            if let Err(e) = self.load_rules().await {
                log::error!("Failed to update adblock rules: {}", e);
            } else {
                self.last_update = std::time::Instant::now();
                log::info!("Adblock rules updated");
            }
        }
    }
}
```

---

## 📦 配置示例

### 完整配置示例

```json
{
  "log": {
    "level": "info"
  },
  "dns": {
    "servers": [
      {
        "address": "https://1.1.1.1/dns-query",
        "tag": "cloudflare",
        "strategy": "prefer_ipv4"
      },
      {
        "address": "https://dns.google/dns-query",
        "tag": "google"
      },
      {
        "address": "223.5.5.5",
        "tag": "ali-cn"
      }
    ],
    "rules": [
      {
        "domain_suffix": [".cn", ".taobao.com", ".qq.com"],
        "server": "ali-cn"
      },
      {
        "geoip": ["cn"],
        "server": "ali-cn"
      },
      {
        "server": "cloudflare"
      }
    ],
    "anti_pollution": {
      "enabled": true,
      "trusted_servers": ["cloudflare", "google"],
      "check_method": "dual_query",
      "poison_ips": [
        "203.98.7.65",
        "159.226.50.10"
      ]
    },
    "adblock": {
      "enabled": true,
      "rules": [
        "https://anti-ad.net/domains.txt",
        "https://raw.githubusercontent.com/privacy-protection-tools/anti-AD/master/anti-ad-domains.txt"
      ],
      "custom_rules": [
        "||ads.example.com^",
        "||analytics.example.com^"
      ],
      "update_interval": 86400,
      "action": "reject"
    }
  },
  "inbounds": [
    {
      "type": "mixed",
      "listen": "127.0.0.1",
      "listen_port": 17890
    }
  ],
  "outbounds": [
    {
      "type": "direct",
      "tag": "direct"
    }
  ]
}
```

---

## 🔧 推荐的广告规则源

### 中文广告规则

1. **anti-AD**（推荐）
   ```
   https://anti-ad.net/domains.txt
   https://anti-ad.net/easylist.txt
   ```

2. **AdGuard 中文规则**
   ```
   https://anti-ad.net/adguard-filter.txt
   ```

3. **OISD**
   ```
   https://big.oisd.nl/domainswild
   ```

### 国际广告规则

4. **EasyList**
   ```
   https://easylist.to/easylist/easylist.txt
   ```

5. **Steven Black Hosts**
   ```
   https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts
   ```

---

## 🚀 实现计划

### 阶段 1: 基础实现（1-2天）

1. **DNS 污染防护**
   - ✅ 创建 `anti_pollution.rs` 模块
   - ✅ 实现双查询逻辑
   - ✅ 污染 IP 检测
   - ✅ DoH/DoT 支持

2. **广告屏蔽**
   - ✅ 创建 `adblock.rs` 模块
   - ✅ 规则解析器
   - ✅ 域名匹配引擎
   - ✅ 规则自动更新

### 阶段 2: 优化（1天）

3. **性能优化**
   - ✅ 使用 Bloom Filter 加速匹配
   - ✅ 规则缓存
   - ✅ 并发查询优化

4. **配置优化**
   - ✅ 配置验证
   - ✅ 热重载支持

### 阶段 3: 测试（1天）

5. **测试**
   - ✅ 单元测试
   - ✅ 集成测试
   - ✅ 实际场景测试

---

## 📝 使用示例

### 示例 1: 基础防污染

```json
{
  "dns": {
    "servers": [
      {
        "address": "https://1.1.1.1/dns-query",
        "tag": "cloudflare"
      }
    ],
    "anti_pollution": {
      "enabled": true,
      "trusted_servers": ["cloudflare"],
      "check_method": "trusted_only"
    }
  }
}
```

### 示例 2: 中国优化 + 广告屏蔽

```json
{
  "dns": {
    "servers": [
      {
        "address": "https://dns.alidns.com/dns-query",
        "tag": "ali"
      },
      {
        "address": "https://1.1.1.1/dns-query",
        "tag": "cf"
      }
    ],
    "rules": [
      {
        "domain_suffix": [".cn"],
        "server": "ali"
      },
      {
        "server": "cf"
      }
    ],
    "anti_pollution": {
      "enabled": true,
      "trusted_servers": ["cf"]
    },
    "adblock": {
      "enabled": true,
      "rules": ["https://anti-ad.net/domains.txt"],
      "action": "reject"
    }
  }
}
```

---

## 🎯 预期效果

### DNS 污染防护

- ✅ 自动检测并绕过 DNS 劫持
- ✅ 提高域名解析准确性
- ✅ 支持 DoH/DoT 加密查询
- ✅ 可选的智能分流（国内/国外）

### 广告屏蔽

- ✅ 屏蔽常见广告域名
- ✅ 减少网络流量消耗
- ✅ 提升浏览速度
- ✅ 保护隐私（屏蔽追踪器）

---

## 📊 性能影响

| 功能 | 内存增加 | CPU 增加 | 延迟增加 |
|------|---------|---------|---------|
| DNS 污染防护 | ~5 MB | ~2% | ~20ms |
| 广告屏蔽 | ~10 MB | ~1% | ~1ms |

---

**准备好开始实现了吗？** 🚀
