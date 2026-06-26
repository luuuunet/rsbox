# GitHub Release 构建问题诊断报告

## 问题描述
2026年6月26日 14:00

**用户反馈**：Release 里面没有 Windows/Mac/Linux/Android/iOS 等构建产物

---

## 🔍 问题诊断

### 1. 检查 Release Workflow 配置

**当前配置的平台**：
```yaml
matrix:
  include:
    - os: ubuntu-latest
      target: x86_64-unknown-linux-gnu
      asset_name: rsbox-linux-x86_64
      
    - os: ubuntu-latest
      target: aarch64-unknown-linux-gnu
      asset_name: rsbox-linux-aarch64
      
    - os: windows-latest
      target: x86_64-pc-windows-msvc
      asset_name: rsbox-windows-x86_64.exe
      
    - os: macos-latest
      target: x86_64-apple-darwin
      asset_name: rsbox-macos-x86_64
      
    - os: macos-latest
      target: aarch64-apple-darwin
      asset_name: rsbox-macos-aarch64
```

**配置状态**：✅ 5 个平台已配置

---

### 2. 问题分析

#### 问题 1：Android 和 iOS 未配置

**当前状态**：
- ❌ Android 构建未在 release.yml 中
- ❌ iOS 构建未在 release.yml 中
- ✅ Android/iOS 在单独的 mobile.yml 中

**原因**：
- 移动平台需要特殊构建环境
- 单独的 mobile.yml 文件
- 未集成到主 Release 流程

#### 问题 2：Release 未触发

**触发条件**：
```yaml
on:
  push:
    tags:
      - 'v*'
  workflow_dispatch:
```

**检查**：
- Tag v0.1.1 已存在
- 但可能未正确触发 Workflow

---

## 🛠️ 解决方案

### 方案 1：立即触发 Release（快速）

#### 步骤 1：删除旧 Tag 并重新创建

```bash
# 删除本地 Tag
git tag -d v0.1.1

# 删除远程 Tag
git push origin :refs/tags/v0.1.1

# 重新创建 Tag
git tag -a v0.1.1 -m "Release v0.1.1 - Full sing-box parity with complete platform support"

# 推送 Tag
git push origin v0.1.1
```

#### 步骤 2：手动触发

访问：https://github.com/luuuunet/rsbox/actions/workflows/release.yml
点击 "Run workflow"

---

### 方案 2：完善 Release Workflow（推荐）

#### 添加 Android 和 iOS 构建

创建完整的 Release workflow：

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'
  workflow_dispatch:
    inputs:
      version:
        description: 'Version to release'
        required: true
        default: 'v0.1.1'

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: ""

permissions:
  contents: write

jobs:
  # 桌面平台构建
  build-desktop:
    name: Build Desktop (${{ matrix.asset_name }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        include:
          # Linux x86_64
          - os: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            artifact_name: rsbox
            asset_name: rsbox-linux-x86_64
            use_cross: false

          # Linux aarch64
          - os: ubuntu-latest
            target: aarch64-unknown-linux-gnu
            artifact_name: rsbox
            asset_name: rsbox-linux-aarch64
            use_cross: true

          # Windows x86_64
          - os: windows-latest
            target: x86_64-pc-windows-msvc
            artifact_name: rsbox.exe
            asset_name: rsbox-windows-x86_64.exe
            use_cross: false

          # macOS x86_64
          - os: macos-latest
            target: x86_64-apple-darwin
            artifact_name: rsbox
            asset_name: rsbox-macos-x86_64
            use_cross: false

          # macOS ARM64
          - os: macos-latest
            target: aarch64-apple-darwin
            artifact_name: rsbox
            asset_name: rsbox-macos-aarch64
            use_cross: false

    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Cache cargo
        uses: actions/cache@v4
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: release-${{ matrix.os }}-${{ matrix.target }}-${{ hashFiles('**/Cargo.lock') }}

      - name: Install cross
        if: matrix.use_cross
        run: cargo install cross --git https://github.com/cross-rs/cross

      - name: Build release
        if: ${{ !matrix.use_cross }}
        run: cargo build --release --target ${{ matrix.target }} -p rsbox

      - name: Build release (cross)
        if: matrix.use_cross
        run: cross build --release --target ${{ matrix.target }} -p rsbox

      - name: Prepare release asset
        shell: bash
        run: |
          mkdir -p dist
          cp "target/${{ matrix.target }}/release/${{ matrix.artifact_name }}" "dist/${{ matrix.asset_name }}"

      - name: Upload build artifact
        uses: actions/upload-artifact@v4
        with:
          name: desktop-${{ matrix.asset_name }}
          path: dist/${{ matrix.asset_name }}
          if-no-files-found: error

  # Android 构建
  build-android:
    name: Build Android (${{ matrix.target }})
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false
      matrix:
        target:
          - aarch64-linux-android
          - armv7-linux-androideabi
          - x86_64-linux-android
          - i686-linux-android
    
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable

      - name: Install Android NDK
        uses: nttld/setup-ndk@v1
        with:
          ndk-version: r26b

      - name: Add Android targets
        run: |
          rustup target add ${{ matrix.target }}

      - name: Build Android
        env:
          ANDROID_NDK_HOME: ${{ steps.setup-ndk.outputs.ndk-path }}
        run: |
          cargo install cargo-ndk
          cargo ndk -t ${{ matrix.target }} build --release -p rsbox

      - name: Prepare Android asset
        run: |
          mkdir -p dist
          cp target/${{ matrix.target }}/release/rsbox dist/rsbox-android-${{ matrix.target }}

      - name: Upload Android artifact
        uses: actions/upload-artifact@v4
        with:
          name: android-${{ matrix.target }}
          path: dist/rsbox-android-${{ matrix.target }}

  # iOS 构建
  build-ios:
    name: Build iOS (${{ matrix.target }})
    runs-on: macos-latest
    strategy:
      fail-fast: false
      matrix:
        target:
          - aarch64-apple-ios
          - x86_64-apple-ios
    
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Build iOS
        run: cargo build --release --target ${{ matrix.target }} -p rsbox

      - name: Prepare iOS asset
        run: |
          mkdir -p dist
          cp target/${{ matrix.target }}/release/rsbox dist/rsbox-ios-${{ matrix.target }}

      - name: Upload iOS artifact
        uses: actions/upload-artifact@v4
        with:
          name: ios-${{ matrix.target }}
          path: dist/rsbox-ios-${{ matrix.target }}

  # 创建 Release
  release:
    name: Create GitHub Release
    needs: [build-desktop, build-android, build-ios]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
        with:
          path: dist
          merge-multiple: true

      - name: List all artifacts
        run: find dist -type f -exec ls -lh {} \;

      - name: Create checksums
        run: |
          cd dist
          sha256sum * > SHA256SUMS

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          files: |
            dist/rsbox-*
            dist/SHA256SUMS
          draft: false
          prerelease: false
```

---

## 📊 当前问题总结

| 平台 | Release 配置 | Mobile 配置 | 状态 |
|------|-------------|-------------|------|
| **Linux x86_64** | ✅ | - | 已配置 |
| **Linux ARM64** | ✅ | - | 已配置 |
| **Windows x86_64** | ✅ | - | 已配置 |
| **macOS x86_64** | ✅ | - | 已配置 |
| **macOS ARM64** | ✅ | - | 已配置 |
| **Android** | ❌ | ✅ | 未集成 |
| **iOS** | ❌ | ✅ | 未集成 |

---

## 🚀 立即修复步骤

### 快速方案（今天可完成）

#### 1. 重新触发 Release

```bash
# 删除并重建 Tag
git tag -d v0.1.1
git push origin :refs/tags/v0.1.1
git tag -a v0.1.1 -m "Release v0.1.1 - 100% sing-box parity"
git push origin v0.1.1
```

#### 2. 手动触发 Workflow

访问：https://github.com/luuuunet/rsbox/actions/workflows/release.yml
- 点击 "Run workflow"
- 输入版本：v0.1.1
- 点击 "Run workflow"

---

### 完整方案（包含移动平台）

#### 1. 更新 release.yml

将上面的完整配置保存到 `.github/workflows/release.yml`

#### 2. 提交并创建新 Tag

```bash
git add .github/workflows/release.yml
git commit -m "feat: add Android and iOS builds to Release workflow"
git push origin main

# 创建新版本
git tag -a v0.1.2 -m "Release v0.1.2 - Complete platform support"
git push origin v0.1.2
```

---

## 📈 预期结果

### 桌面平台（5 个）
- ✅ rsbox-linux-x86_64
- ✅ rsbox-linux-aarch64
- ✅ rsbox-windows-x86_64.exe
- ✅ rsbox-macos-x86_64
- ✅ rsbox-macos-aarch64

### 移动平台（6 个）
- ✅ rsbox-android-aarch64
- ✅ rsbox-android-armv7
- ✅ rsbox-android-x86_64
- ✅ rsbox-android-i686
- ✅ rsbox-ios-aarch64
- ✅ rsbox-ios-x86_64

### 其他
- ✅ SHA256SUMS（校验和）

**总计**：11 个构建产物

---

## ⏱️ 预计构建时间

| 阶段 | 时间 |
|------|------|
| 桌面平台 | 10-15 分钟 |
| Android | 15-20 分钟 |
| iOS | 10-15 分钟 |
| Release 创建 | 2-3 分钟 |
| **总计** | **40-50 分钟** |

---

## 🎯 推荐行动

### 立即执行（快速）

1. ✅ 重新触发当前 Release（5 个桌面平台）
2. ✅ 验证构建成功

### 后续完善（完整）

1. ✅ 更新 release.yml 添加移动平台
2. ✅ 创建 v0.1.2 Tag
3. ✅ 获得 11 个平台的完整构建

---

**诊断报告生成时间**：2026-06-26 14:00  
**问题**：Release 缺少构建产物  
**原因**：Workflow 未触发或构建失败  
**解决方案**：重新触发 Release + 完善配置

---

🔗 **GitHub Actions**: https://github.com/luuuunet/rsbox/actions  
🔗 **Releases**: https://github.com/luuuunet/rsbox/releases
