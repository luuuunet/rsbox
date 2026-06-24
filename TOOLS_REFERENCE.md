# rsbox 项目优化工具清单

本文档列出了项目中所有可用的工具、脚本和配置文件。

## 🛠️ 开发工具

### 构建脚本

```bash
# 快速构建
make build                    # 开发构建
make build-release            # 生产构建
make build-minimal            # 最小化构建

# 使用 cargo 直接构建
cargo build                   # 开发
cargo build --release         # 生产
cargo build --profile dist    # 发布
```

### 测试脚本

```bash
# 运行测试
make test                     # 所有测试
make test-all                 # 包含所有特性
cargo test --workspace        # Workspace 测试

# 性能测试
cargo bench                   # Benchmark
./scripts/profile.sh          # 性能分析
```

### 代码质量

```bash
# 格式化
make fmt                      # 格式化代码
make fmt-check                # 检查格式

# Linting
make clippy                   # 运行 clippy
make clippy-fix               # 自动修复

# CI 模拟
make ci                       # 本地 CI 检查
```

### 清理工具

```bash
make clean                    # 清理构建
cargo clean                   # 完全清理
```

## 📜 脚本文件

### 1. `scripts/generate-service.sh`
**功能**: 生成 systemd 服务文件

```bash
./scripts/generate-service.sh \
  -c /etc/rsbox/config.json \
  -b /usr/local/bin/rsbox \
  -u rsbox
```

### 2. `scripts/benchmark.sh`
**功能**: 性能基准测试

```bash
./scripts/benchmark.sh
```

### 3. `scripts/verify-fixes.sh`
**功能**: 验证代码修复

```bash
./scripts/verify-fixes.sh
```

### 4. `scripts/profile.sh`
**功能**: 性能分析

```bash
./scripts/profile.sh
```

### 5. `install.sh` / `install.ps1`
**功能**: 一键安装脚本

```bash
# Linux/macOS
curl -fsSL https://raw.githubusercontent.com/.../install.sh | bash

# Windows
iwr -useb https://raw.githubusercontent.com/.../install.ps1 | iex
```

## ⚙️ 配置文件

### Cargo 配置

| 文件 | 用途 |
|------|------|
| `Cargo.toml` | Workspace 配置 |
| `.cargo/config.toml` | 编译配置 |
| `rustfmt.toml` | 代码格式化 |

### CI/CD 配置

| 文件 | 用途 |
|------|------|
| `.github/workflows/ci.yml` | 持续集成 |
| `.github/workflows/release.yml` | 自动发布 |
| `.github/workflows/docker.yml` | Docker 构建 |

### Docker 配置

| 文件 | 用途 |
|------|------|
| `Dockerfile` | 镜像构建 |
| `docker-compose.yml` | 服务编排 |
| `.dockerignore` | 构建排除 |

### 开发工具

| 文件 | 用途 |
|------|------|
| `.vscode/settings.json` | VSCode 设置 |
| `.vscode/tasks.json` | VSCode 任务 |
| `.vscode/launch.json` | VSCode 调试 |
| `.editorconfig` | 编辑器配置 |

## 📚 文档工具

### 生成文档

```bash
# API 文档
cargo doc --no-deps --open

# 文档测试
cargo test --doc
```

### 文档列表

1. **入门文档**
   - README.md
   - docs/QUICK_START.md

2. **技术文档**
   - ARCHITECTURE.md
   - FEATURES.md
   - docs/PERFORMANCE.md
   - docs/DOCKER.md
   - docs/BUILD_OPTIMIZATION.md

3. **配置文档**
   - examples/README.md
   - 8 个配置示例

4. **改进报告**
   - IMPROVEMENTS_REPORT.md
   - IMPROVEMENTS_REPORT_V2.md
   - OPTIMIZATION_COMPLETE_V3.md
   - FINAL_OPTIMIZATION_SUMMARY.md

## 🧪 测试工具

### 单元测试

```bash
# 运行所有测试
cargo test --workspace

# 特定包
cargo test -p rsb-core

# 显示输出
cargo test -- --nocapture
```

### 集成测试

```bash
# 集成测试
cargo test --test integration_test
```

### Benchmark

```bash
# 运行所有 benchmark
cargo bench

# 特定 benchmark
cargo bench dns_resolver

# 保存基线
cargo bench -- --save-baseline before
```

### 测试工具模块

使用 `rsb-core::test_utils`:

```rust
use rsb_core::test_utils::*;

#[tokio::test]
async fn test_example() {
    let port = find_free_port().await;
    let config = minimal_config();
    // ...
}
```

## 📊 分析工具

### 性能分析

```bash
# 性能分析脚本
./scripts/profile.sh

# 火焰图
cargo flamegraph -- run -c config.json

# 内存分析
valgrind --leak-check=full ./target/release/rsbox
```

### 体积分析

```bash
# 安装工具
cargo install cargo-bloat

# 分析
cargo bloat --release -n 20
cargo bloat --release --crates
```

### 依赖分析

```bash
# 依赖树
cargo tree

# 重复依赖
cargo tree -d

# 未使用依赖
cargo machete
```

### 安全审计

```bash
# 安装工具
cargo install cargo-audit

# 审计
cargo audit

# 过期依赖
cargo outdated
```

## 🔧 开发工作流

### 日常开发

```bash
# 1. 拉取代码
git pull

# 2. 开发
cargo build
cargo test
cargo clippy

# 3. 提交前
make ci
git commit
```

### 发布流程

```bash
# 1. 更新版本
vim Cargo.toml CHANGELOG.md

# 2. 构建
cargo build --profile dist

# 3. 测试
cargo test --release

# 4. 发布
git tag -a v0.2.0 -m "Release v0.2.0"
git push --tags
```

## 🐳 Docker 工作流

### 构建镜像

```bash
# 本地构建
docker build -t rsbox:local .

# 多架构构建
docker buildx build --platform linux/amd64,linux/arm64 -t rsbox:latest .
```

### 运行容器

```bash
# 使用 docker-compose
docker-compose up -d

# 直接运行
docker run -d --name rsbox \
  -v ./config.json:/etc/rsbox/config.json \
  -p 7890:7890 \
  rsbox:latest
```

## 📦 发布工具

### 创建发布

```bash
# 本地发布
cargo build --profile dist
tar czf rsbox-linux-x86_64.tar.gz target/dist/rsbox

# GitHub Release
gh release create v0.2.0 \
  --title "Release v0.2.0" \
  --notes "Release notes" \
  target/dist/rsbox*
```

## 🎯 快速参考

### 常用命令

```bash
# 开发
cargo build && cargo test

# 检查
cargo clippy && cargo fmt --check

# 完整 CI
make ci

# 性能
cargo bench && ./scripts/profile.sh

# 发布
cargo build --profile dist
```

### 工具安装

```bash
# 必备工具
rustup component add rustfmt clippy

# 可选工具
cargo install cargo-audit
cargo install cargo-bloat
cargo install cargo-flamegraph
cargo install cargo-tarpaulin
cargo install cargo-machete
```

## 📞 获取帮助

### 命令帮助

```bash
# Makefile 帮助
make help

# Cargo 帮助
cargo --help
cargo build --help

# rsbox 帮助
rsbox --help
rsbox run --help
```

### 文档资源

- 在线文档: https://docs.rs/rsbox
- GitHub: https://github.com/yourusername/rsbox
- Issues: https://github.com/yourusername/rsbox/issues

---

**最后更新**: 2024-06-24  
**版本**: 0.1.0
