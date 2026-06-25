# Android 版本支持

## Android 平台支持

rsbox 现已支持 Android 平台！通过 GitHub Actions 自动构建以下 Android 版本：

### 支持的 Android 架构

1. **aarch64 (ARM64)** - 推荐
   - 适用于：现代 Android 设备（2016年后）
   - 处理器：Snapdragon 820+, Exynos 8890+, Kirin 950+
   - 设备示例：大部分现代 Android 手机和平板

2. **armv7 (ARM32)**
   - 适用于：较旧的 Android 设备
   - 处理器：Snapdragon 400-800 系列（旧版）
   - 设备示例：2012-2016 年的 Android 设备

3. **x86_64 (Intel/AMD 64位)**
   - 适用于：x86 Android 设备和模拟器
   - 设备示例：Android-x86、模拟器

## 下载

访问 [Releases](https://github.com/luuuunet/rsbox/releases) 页面下载对应架构的版本：

```
rsbox-android-aarch64   # ARM64 (推荐)
rsbox-android-armv7     # ARM32 (旧设备)
rsbox-android-x86_64    # x86 模拟器
```

## 安装和使用

### 前提条件

- Android 5.0 (API 21) 或更高版本
- 已 Root 的设备（推荐）或使用 Termux

### 方法 1：使用 Termux（无需 Root）

1. **安装 Termux**
   - 从 [F-Droid](https://f-droid.org/packages/com.termux/) 下载安装

2. **下载 rsbox**
   ```bash
   # 更新包管理器
   pkg update && pkg upgrade
   
   # 安装依赖
   pkg install wget
   
   # 下载 rsbox (ARM64 版本为例)
   wget https://github.com/luuuunet/rsbox/releases/latest/download/rsbox-android-aarch64
   
   # 重命名并添加执行权限
   mv rsbox-android-aarch64 rsbox
   chmod +x rsbox
   ```

3. **运行 rsbox**
   ```bash
   # 查看版本
   ./rsbox version
   
   # 检查配置
   ./rsbox check -c config.json
   
   # 运行服务
   ./rsbox run -c config.json
   ```

### 方法 2：Root 设备

1. **下载到设备**
   ```bash
   # 使用 adb 推送
   adb push rsbox-android-aarch64 /data/local/tmp/rsbox
   adb shell chmod +x /data/local/tmp/rsbox
   ```

2. **通过 adb shell 运行**
   ```bash
   adb shell
   su
   cd /data/local/tmp
   ./rsbox version
   ```

## 配置示例

Android 上的配置与其他平台相同：

```json
{
  "log": {
    "level": "info"
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

## 作为系统代理

### 使用 VPN 模式（Termux）

虽然 rsbox 不直接支持 VPN 模式，但可以结合其他工具：

1. **rsbox 作为本地代理**
   ```bash
   ./rsbox run -c config.json
   ```

2. **使用 Proxy Droid 等应用**
   - 设置 HTTP/SOCKS5 代理：127.0.0.1:17890

### 使用 iptables（需要 Root）

```bash
# 将流量重定向到 rsbox
iptables -t nat -A OUTPUT -p tcp -j REDIRECT --to-ports 17890
```

## 常见问题

### 1. 如何确定我的设备架构？

```bash
# 在 Termux 中运行
uname -m

# 输出对应关系：
# aarch64 → 使用 rsbox-android-aarch64
# armv7l → 使用 rsbox-android-armv7
# x86_64 → 使用 rsbox-android-x86_64
```

### 2. 权限被拒绝

```bash
# 确保文件有执行权限
chmod +x rsbox
```

### 3. 无法绑定端口

```bash
# 使用非特权端口（> 1024）
# 或者使用 Root 权限运行
```

### 4. 后台运行

```bash
# 使用 nohup 后台运行
nohup ./rsbox run -c config.json > rsbox.log 2>&1 &

# 查看日志
tail -f rsbox.log

# 停止服务
pkill rsbox
```

## 性能说明

- **内存占用**：约 20-40 MB
- **CPU 使用**：空闲时 < 1%
- **电池影响**：轻微（后台运行）
- **网络性能**：接近原生速度

## 限制

1. **TUN 模式**：需要 Root 权限或 VPN API
2. **端口 < 1024**：需要 Root 权限
3. **后台保活**：Android 可能会终止后台进程

## 推荐使用场景

- ✅ 开发测试
- ✅ Termux 命令行环境
- ✅ Root 设备系统代理
- ✅ Android 平板/电视盒子
- ⚠️ 日常手机使用（建议使用专门的 VPN 客户端）

## 与其他客户端对比

| 特性 | rsbox | sing-box (Go) | Clash |
|------|-------|---------------|-------|
| 内存占用 | ~30MB | ~50MB | ~40MB |
| 启动速度 | 快 | 快 | 中 |
| 协议支持 | 38+ | 30+ | 20+ |
| 安装大小 | ~8MB | ~15MB | ~12MB |
| VPN 模式 | 需配合 | 原生支持 | 原生支持 |

## 贡献

如果你在 Android 上遇到问题或有改进建议，欢迎提交 Issue！

## 许可证

与主项目相同：GPL-3.0-or-later
