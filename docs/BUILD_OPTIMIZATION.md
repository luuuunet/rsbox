# Build optimization guide for rsbox

This document describes various build optimizations and their trade-offs.

## Quick Reference

```bash
# Development (fast compile)
cargo build

# Release (production)
cargo build --release

# Distribution (maximum optimization)
cargo build --profile dist

# Benchmarking
cargo bench

# With debug info
cargo build --profile release-with-debug
```

## Profile Comparison

| Profile | Compile Time | Binary Size | Performance | Debug Info |
|---------|--------------|-------------|-------------|------------|
| dev | ⭐⭐⭐⭐⭐ | Large | Slow | Yes |
| release | ⭐⭐ | Small | Fast | No |
| dist | ⭐ | Smallest | Fastest | No |
| bench | ⭐⭐ | Medium | Fast | Yes |
| release-with-debug | ⭐⭐ | Large | Fast | Yes |

## Advanced Optimizations

### 1. LTO (Link-Time Optimization)

```toml
[profile.release]
lto = "fat"  # 完整 LTO，最慢但最优
# lto = "thin"  # 更快但优化少一些
# lto = true    # 默认值
```

**Trade-off**: 编译时间增加 2-3 倍，但二进制更小、更快

### 2. Codegen Units

```toml
[profile.release]
codegen-units = 1  # 单个单元，最优但最慢
# codegen-units = 16  # 默认值，并行编译
```

**Trade-off**: 并行度 vs 优化程度

### 3. Optimization Level

```toml
[profile.release]
opt-level = "z"  # 优化体积
# opt-level = 3    # 最大性能（dist profile）
# opt-level = 2    # 默认平衡
# opt-level = "s"  # 优化体积（比 z 少）
```

### 4. Panic Strategy

```toml
[profile.release]
panic = "abort"  # 不展开栈，更小的二进制
# panic = "unwind"  # 默认值，可以捕获 panic
```

### 5. Strip Symbols

```toml
[profile.release]
strip = true  # 移除调试符号
# strip = "debuginfo"  # 只移除调试信息
# strip = "symbols"    # 移除所有符号
```

## Platform-Specific Optimizations

### Windows

```bash
# 静态链接 CRT
set RUSTFLAGS=-C target-feature=+crt-static
cargo build --release
```

### Linux

```bash
# 使用 musl 构建静态二进制
cargo build --release --target x86_64-unknown-linux-musl

# 使用 mold 链接器（需要安装）
cargo install mold
RUSTFLAGS="-C link-arg=-fuse-ld=mold" cargo build --release
```

### macOS

```bash
# Universal binary (Intel + Apple Silicon)
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
lipo -create target/{x86_64,aarch64}-apple-darwin/release/rsbox \
     -output rsbox-universal
```

## Size Optimization

### Analyze Binary Size

```bash
# Install cargo-bloat
cargo install cargo-bloat

# Analyze
cargo bloat --release -n 20

# Compare with stripped version
cargo bloat --release --crates
```

### Reduce Dependencies

```bash
# Check dependency tree
cargo tree --depth 1

# Find duplicate dependencies
cargo tree -d

# Remove unused dependencies
cargo machete  # cargo install cargo-machete
```

### Feature Flags

```toml
[dependencies]
tokio = { version = "1", features = ["rt", "net"] }  # 只启用需要的
# 而不是
# tokio = { version = "1", features = ["full"] }
```

## Compile Time Optimization

### Incremental Compilation

```bash
# Enable (default in dev)
export CARGO_INCREMENTAL=1

# Disable for clean builds
export CARGO_INCREMENTAL=0
```

### Parallel Compilation

```bash
# Use all cores
cargo build -j $(nproc)

# Or set in .cargo/config.toml
```

### Caching

```bash
# Use sccache
cargo install sccache
export RUSTC_WRAPPER=sccache

# Check stats
sccache --show-stats
```

## Performance Profiling

### CPU Profiling

```bash
# Linux with perf
cargo build --profile bench
perf record -g ./target/bench/rsbox run -c config.json
perf report

# Flamegraph
cargo install flamegraph
cargo flamegraph -- run -c config.json
```

### Memory Profiling

```bash
# Valgrind
cargo build --profile release-with-debug
valgrind --leak-check=full ./target/release-with-debug/rsbox

# Heaptrack (Linux)
heaptrack ./target/release/rsbox run -c config.json
heaptrack_gui heaptrack.rsbox.*.gz
```

### Benchmarking

```bash
# Run benchmarks
cargo bench

# Compare with baseline
cargo bench -- --save-baseline before
# ... make changes ...
cargo bench -- --baseline before
```

## Production Build Checklist

- [ ] Use `--profile dist` or `--release`
- [ ] Enable LTO (`lto = "fat"`)
- [ ] Set `codegen-units = 1`
- [ ] Strip symbols (`strip = true`)
- [ ] Use `panic = "abort"`
- [ ] Test on target platform
- [ ] Run benchmarks
- [ ] Check binary size
- [ ] Verify performance metrics
- [ ] Test memory usage

## Troubleshooting

### Out of Memory During Compilation

```bash
# Reduce codegen units
export CARGO_CODEGEN_UNITS=4

# Or disable LTO temporarily
cargo build --release --config profile.release.lto=false
```

### Slow Link Times

```bash
# Use faster linker (Linux)
sudo apt-get install lld
export RUSTFLAGS="-C link-arg=-fuse-ld=lld"

# Or mold (even faster)
cargo install mold
export RUSTFLAGS="-C link-arg=-fuse-ld=mold"
```

### Large Binary Size

1. Check dependencies: `cargo tree`
2. Analyze with cargo-bloat
3. Strip symbols: `strip target/release/rsbox`
4. Use UPX compression: `upx --best target/release/rsbox`

## References

- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Cargo Profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [LLVM LTO](https://llvm.org/docs/LinkTimeOptimization.html)
