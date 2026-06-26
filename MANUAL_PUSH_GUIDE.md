# 手动推送指南 - 触发 Release 构建

## 当前状态
2026年6月26日 14:30

### ✅ 已完成（本地）
- ✅ 删除旧 Tag v0.1.1（本地）
- ✅ 创建新 Tag v0.1.2
- ✅ 提交诊断文档
- ✅ 所有代码准备就绪

### ⏳ 待执行（需要网络）
- ⏳ 推送代码到 GitHub
- ⏳ 推送 Tag 触发 Release
- ⏳ 删除远程旧 Tag

---

## 🚀 手动执行步骤（网络恢复后）

### 步骤 1：推送代码到 main 分支

```bash
git push origin main
```

**预期结果**：
```
To https://github.com/luuuunet/rsbox.git
   46179ba..df25736  main -> main
```

---

### 步骤 2：删除远程旧 Tag（可选）

```bash
git push origin :refs/tags/v0.1.1
```

**预期结果**：
```
To https://github.com/luuuunet/rsbox.git
 - [deleted]         v0.1.1
```

---

### 步骤 3：推送新 Tag 触发 Release

```bash
git push origin v0.1.2
```

**预期结果**：
```
To https://github.com/luuuunet/rsbox.git
 * [new tag]         v0.1.2 -> v0.1.2
```

**🎯 这将触发 Release workflow！**

---

## 📊 预期结果

### Release Workflow 将自动：

1. **构建 5 个桌面平台**：
   - ✅ rsbox-linux-x86_64
   - ✅ rsbox-linux-aarch64
   - ✅ rsbox-windows-x86_64.exe
   - ✅ rsbox-macos-x86_64
   - ✅ rsbox-macos-aarch64

2. **创建 GitHub Release**：
   - 版本：v0.1.2
   - 标题：自动生成
   - 说明：Release notes 自动生成
   - 附件：5 个构建产物 + SHA256SUMS

3. **构建时间**：
   - 预计：15-20 分钟

---

## 🔗 查看位置

### GitHub Actions（查看构建进度）
https://github.com/luuuunet/rsbox/actions

**你会看到**：
- Workflow：Release
- 状态：🟡 In Progress（运行中）
- Jobs：5 个构建任务并行运行

### GitHub Releases（查看最终产物）
https://github.com/luuuunet/rsbox/releases

**构建完成后你会看到**：
- Release v0.1.2
- 5 个下载链接
- SHA256 校验和文件

---

## ❓ 为什么 Release 之前没有构建产物

### 原因分析：

1. **Tag 未正确触发 Workflow**
   - Tag v0.1.1 创建时可能网络问题
   - Workflow 未被触发

2. **Workflow 配置正确**
   - 配置文件 `.github/workflows/release.yml` ✅
   - 触发条件 `on: push: tags: - 'v*'` ✅
   - 构建矩阵 5 个平台 ✅

3. **需要重新触发**
   - 删除旧 Tag
   - 创建新 Tag
   - 推送触发构建

---

## 📱 关于移动平台（Android/iOS）

### 当前状态

移动平台构建在单独的 workflow 中：
- 文件：`.github/workflows/mobile.yml`
- 平台：Android (4) + iOS (2)

### 为什么不在 Release 中？

1. **构建复杂度**
   - Android 需要 NDK
   - iOS 需要 Xcode
   - 构建时间更长（30-40 分钟）

2. **单独触发**
   - 访问：https://github.com/luuuunet/rsbox/actions/workflows/mobile.yml
   - 点击 "Run workflow"
   - 手动触发移动平台构建

### 如何集成到 Release？

参考文档：`RELEASE_BUILD_DIAGNOSIS.md`
- 包含完整的集成配置
- 可以合并为一个统一的 Release

---

## ✅ 验证步骤

### 1. 推送成功后

访问：https://github.com/luuuunet/rsbox/actions

**检查**：
- 是否有新的 "Release" workflow 运行
- 状态是否为 🟡 "In Progress"

### 2. 构建进行中

点击运行中的 workflow，查看：
- 5 个并行的构建任务
- 每个任务的实时日志
- 构建进度

### 3. 构建完成后

访问：https://github.com/luuuunet/rsbox/releases

**检查**：
- 是否有 v0.1.2 Release
- 是否有 5 个文件可下载
- 是否有 SHA256SUMS 文件

---

## 🎯 一键推送脚本

将以下内容保存为 `push-release.sh`：

```bash
#!/bin/bash

echo "========================================"
echo "  推送 rsbox v0.1.2 Release"
echo "========================================"
echo ""

# 推送代码
echo "1. 推送代码到 main..."
git push origin main
if [ $? -eq 0 ]; then
    echo "   ✅ 推送成功"
else
    echo "   ❌ 推送失败"
    exit 1
fi

echo ""

# 删除远程旧 Tag
echo "2. 删除远程旧 Tag v0.1.1..."
git push origin :refs/tags/v0.1.1
if [ $? -eq 0 ]; then
    echo "   ✅ 删除成功"
else
    echo "   ⚠️  Tag 可能不存在（正常）"
fi

echo ""

# 推送新 Tag
echo "3. 推送新 Tag v0.1.2..."
git push origin v0.1.2
if [ $? -eq 0 ]; then
    echo "   ✅ 推送成功"
    echo ""
    echo "🎉 Release 构建已触发！"
    echo ""
    echo "🔗 查看进度："
    echo "   https://github.com/luuuunet/rsbox/actions"
    echo ""
    echo "⏱️  预计 15-20 分钟后完成"
else
    echo "   ❌ 推送失败"
    exit 1
fi

echo ""
echo "========================================"
```

**使用方法**：
```bash
chmod +x push-release.sh
./push-release.sh
```

---

## 📖 相关文档

1. **RELEASE_BUILD_DIAGNOSIS.md**
   - 完整的问题诊断
   - Release workflow 详解
   - 移动平台集成方案

2. **GITHUB_ACTIONS_DIAGNOSIS.md**
   - GitHub Actions 配置说明
   - Workflow 触发条件
   - 手动触发方法

---

## 💡 常见问题

### Q1: 推送后多久能看到构建？
A1: 通常 10-30 秒内 GitHub Actions 会开始运行。

### Q2: 如何查看构建失败原因？
A2: 
1. 访问 Actions 页面
2. 点击失败的运行
3. 查看红色 ❌ 的步骤
4. 展开查看详细日志

### Q3: 构建需要多久？
A3: 
- Linux: 5-8 分钟
- Windows: 5-8 分钟
- macOS: 8-12 分钟
- 总计: 15-20 分钟（并行构建）

### Q4: 如何手动触发 Release？
A4:
1. 访问：https://github.com/luuuunet/rsbox/actions/workflows/release.yml
2. 点击 "Run workflow"
3. 输入版本号：v0.1.2
4. 点击绿色按钮

---

**生成时间**：2026-06-26 14:30  
**状态**：等待网络推送  
**下一步**：网络恢复后执行推送命令

---

🎯 **网络恢复后，依次执行上述 3 个 git push 命令即可！**
