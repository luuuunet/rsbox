# 贡献指南

感谢您对 rsbox 项目的关注！我们欢迎各种形式的贡献。

## 开发环境设置

### 1. 安装依赖

```bash
# Rust 工具链 (需要 1.93+)
rustup update stable

# 安装开发工具
rustup component add rustfmt clippy
```

### 2. 克隆仓库

```bash
git clone https://github.com/yourusername/rsbox.git
cd rsbox
```

### 3. 构建项目

```bash
# 完整构建
cargo build --all-features

# 运行测试
cargo test --workspace
```

## 代码规范

### Rust 代码风格

1. **格式化** - 提交前运行：
   ```bash
   cargo fmt --all
   ```

2. **Linting** - 修复所有警告：
   ```bash
   cargo clippy --all-features -- -D warnings
   ```

3. **命名约定**：
   - 类型: `PascalCase`
   - 函数/变量: `snake_case`
   - 常量: `SCREAMING_SNAKE_CASE`
   - 模块: `snake_case`

4. **注释**：
   - 公开 API 必须有文档注释 (`///`)
   - 复杂逻辑添加内联注释 (`//`)
   - 优先使用英文注释

### 错误处理

- ❌ 避免使用 `unwrap()`
- ✅ 使用 `?` 操作符传播错误
- ✅ 使用 `expect("明确的错误信息")` 说明为什么不会失败
- ✅ 使用 `anyhow` 或 `thiserror` 处理错误

### 异步代码

- 使用 `async-trait` 实现异步 trait
- 避免阻塞操作（使用 `tokio::task::spawn_blocking`）
- 合理使用 `tokio::select!` 和 `timeout`

## 提交规范

### Commit Message 格式

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Type**:
- `feat`: 新功能
- `fix`: Bug 修复
- `docs`: 文档更新
- `style`: 代码格式（不影响功能）
- `refactor`: 重构
- `perf`: 性能优化
- `test`: 测试相关
- `chore`: 构建/工具链变更

**示例**:
```
feat(protocol): add tuic v5 support

- Implement tuic v5 client
- Add congestion control
- Update tests

Closes #123
```

## Pull Request 流程

### 1. 创建分支

```bash
git checkout -b feature/your-feature-name
```

### 2. 开发并测试

```bash
# 编写代码
# ...

# 运行测试
cargo test --workspace

# 检查代码质量
cargo fmt --all --check
cargo clippy --all-features
```

### 3. 提交更改

```bash
git add .
git commit -m "feat: your feature description"
```

### 4. 推送并创建 PR

```bash
git push origin feature/your-feature-name
```

然后在 GitHub 上创建 Pull Request。

### PR 检查清单

- [ ] 代码已通过 `cargo fmt` 格式化
- [ ] 代码已通过 `cargo clippy` 检查
- [ ] 所有测试通过 `cargo test`
- [ ] 添加了必要的测试用例
- [ ] 更新了相关文档
- [ ] Commit message 符合规范
- [ ] PR 描述清晰说明了改动内容

## 添加新协议

参考 [ARCHITECTURE.md](ARCHITECTURE.md) 的"统一注册表"部分：

1. 在 `crates/rsb-protocol/src/` 下创建协议模块
2. 实现 `Inbound` 或 `Outbound` trait
3. 在 `registry.rs` 注册协议
4. 在 `rsb-constant` 添加类型常量
5. 添加单元测试
6. 更新文档

## 测试指南

### 单元测试

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_varint_roundtrip() {
        // 测试代码
    }

    #[tokio::test]
    async fn test_async_function() {
        // 异步测试
    }
}
```

### 集成测试

在 `tests/` 目录下创建集成测试：

```rust
// tests/integration_test.rs
use rsbox::*;

#[tokio::test]
async fn test_full_proxy_chain() {
    // 端到端测试
}
```

### 运行测试

```bash
# 运行所有测试
cargo test --workspace

# 运行特定测试
cargo test -p rsb-protocol test_name

# 显示输出
cargo test -- --nocapture
```

## 性能测试

使用 `criterion` 进行基准测试：

```bash
cargo bench
```

## 文档

### 生成文档

```bash
cargo doc --workspace --no-deps --open
```

### 文档注释示例

```rust
/// Creates a new Salamander obfuscator.
///
/// # Arguments
///
/// * `password` - The obfuscation password
///
/// # Examples
///
/// ```
/// let obfs = Salamander::new("secret");
/// ```
pub fn new(password: &str) -> Self {
    // ...
}
```

## 发布流程

仅维护者可以发布新版本：

1. 更新版本号在 `Cargo.toml`
2. 更新 `CHANGELOG.md`
3. 创建 Git tag: `git tag -a v0.2.0 -m "Release v0.2.0"`
4. 推送 tag: `git push --tags`
5. GitHub Actions 自动构建并发布

## 需要帮助？

- 查看 [Issues](https://github.com/yourusername/rsbox/issues)
- 参与 [Discussions](https://github.com/yourusername/rsbox/discussions)
- 阅读 [ARCHITECTURE.md](ARCHITECTURE.md) 了解架构

## 行为准则

- 尊重所有贡献者
- 接受建设性批评
- 专注于对项目最有利的事情
- 对社区成员保持同理心

感谢您的贡献！🎉
