# 创建 GitHub Release

本文档说明如何为 rsbox 项目创建新版本并自动构建多平台二进制文件。

## 自动发布流程

rsbox 配置了完整的 CI/CD 流程，支持以下平台的自动构建：

### 支持的平台

#### Windows
- ✅ x86_64 (64位，主流)
- ✅ i686 (32位)
- ✅ aarch64 (ARM64，Surface Pro X 等)

#### Linux
- ✅ x86_64-gnu (标准 Linux)
- ✅ x86_64-musl (静态链接，适合 Alpine)
- ✅ aarch64 (ARM64，树莓派 4+，服务器)
- ✅ armv7 (ARM32，树莓派 3 等)

#### macOS
- ✅ x86_64 (Intel Mac)
- ✅ aarch64 (Apple Silicon，M1/M2/M3)

#### Docker
- ✅ linux/amd64
- ✅ linux/arm64
- ✅ linux/arm/v7

## 发布新版本

### 方法 1：创建 Git Tag（推荐）

```bash
# 1. 确保在 main 分支
git checkout main
git pull origin main

# 2. 创建新版本标签
git tag -a v0.1.1 -m "Release v0.1.1"

# 3. 推送标签到 GitHub
git push origin v0.1.1
```

### 方法 2：手动触发（GitHub 界面）

1. 进入 GitHub 仓库
2. 点击 **Actions** 标签
3. 选择 **Release** workflow
4. 点击 **Run workflow**
5. 输入版本号（例如：v0.1.1）
6. 点击 **Run workflow**

## 自动化流程

一旦触发发布，GitHub Actions 会自动：

1. ✅ 创建 GitHub Release
2. ✅ 为所有平台构建二进制文件（10 个平台）
3. ✅ 生成 SHA256 校验和
4. ✅ 上传所有文件到 Release
5. ✅ 构建并推送 Docker 镜像
6. ✅ 发布到 Docker Hub 和 GHCR

整个流程约需 30-45 分钟完成。

## 发布后的产物

### 二进制文件

```
rsbox-v0.1.1-windows-x86_64.zip
rsbox-v0.1.1-windows-i686.zip
rsbox-v0.1.1-windows-aarch64.zip
rsbox-v0.1.1-linux-x86_64.tar.gz
rsbox-v0.1.1-linux-x86_64-musl.tar.gz
rsbox-v0.1.1-linux-aarch64.tar.gz
rsbox-v0.1.1-linux-armv7.tar.gz
rsbox-v0.1.1-macos-x86_64.tar.gz
rsbox-v0.1.1-macos-aarch64.tar.gz
```

### Docker 镜像

```bash
# Docker Hub
docker pull luuuunet/rsbox:latest
docker pull luuuunet/rsbox:v0.1.1

# GitHub Container Registry
docker pull ghcr.io/luuuunet/rsbox:latest
docker pull ghcr.io/luuuunet/rsbox:v0.1.1
```

## 验证发布

### 检查 Release

```bash
# 查看所有 releases
gh release list

# 查看特定 release
gh release view v0.1.1
```

### 下载测试

```bash
# 下载 Linux 版本
wget https://github.com/luuuunet/rsbox/releases/download/v0.1.1/rsbox-v0.1.1-linux-x86_64.tar.gz

# 解压并测试
tar xzf rsbox-v0.1.1-linux-x86_64.tar.gz
./rsbox version
```

### Docker 测试

```bash
# 拉取镜像
docker pull luuuunet/rsbox:latest

# 运行测试
docker run --rm luuuunet/rsbox:latest version
```

## 配置 Secrets

为了完整使用所有功能，需要在 GitHub 仓库设置以下 Secrets：

### 必需的 Secrets

1. **GITHUB_TOKEN**
   - 自动提供，无需配置

### 可选的 Secrets（Docker 发布）

2. **DOCKER_USERNAME**
   - 你的 Docker Hub 用户名
   - 设置路径：Settings → Secrets → Actions → New repository secret

3. **DOCKER_PASSWORD**
   - Docker Hub 访问令牌（不是密码）
   - 获取方式：Docker Hub → Account Settings → Security → New Access Token
   - 设置路径：Settings → Secrets → Actions → New repository secret

## 版本号规范

遵循语义化版本（Semantic Versioning）：

```
v主版本.次版本.修订号

v0.1.0 - 初始版本
v0.1.1 - Bug 修复
v0.2.0 - 新功能
v1.0.0 - 第一个稳定版本
```

### 版本类型

- **主版本**（Major）：不兼容的 API 修改
- **次版本**（Minor）：向下兼容的新功能
- **修订号**（Patch）：向下兼容的 bug 修复

## 故障排查

### 构建失败

1. 检查 Actions 日志
2. 验证 Cargo.toml 版本号
3. 确保所有测试通过

### Docker 推送失败

1. 验证 DOCKER_USERNAME 和 DOCKER_PASSWORD
2. 检查 Docker Hub 配额
3. 确认镜像名称正确

### 无法创建 Release

1. 确认有仓库写权限
2. 检查 tag 是否已存在
3. 验证 GITHUB_TOKEN 权限

## 快速命令

```bash
# 创建并推送新版本（一键发布）
VERSION="v0.1.1"
git tag -a $VERSION -m "Release $VERSION"
git push origin $VERSION

# 删除错误的 tag（本地和远程）
git tag -d v0.1.1
git push origin :refs/tags/v0.1.1

# 查看最新 tag
git describe --tags --abbrev=0
```

## 下一步

完成首次发布后：

1. ✅ 在 README 中添加下载链接
2. ✅ 更新 CHANGELOG
3. ✅ 通知用户新版本
4. ✅ 在社交媒体分享

---

**现在就开始发布第一个多平台版本吧！** 🚀

```bash
git tag -a v0.1.0 -m "Release v0.1.0 - First stable release"
git push origin v0.1.0
```
