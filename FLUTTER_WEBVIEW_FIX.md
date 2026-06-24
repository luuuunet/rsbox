# Flutter Windows WebView 构建问题修复指南

## 问题描述

Flutter 项目在 Windows 平台构建时，`webview_win_floating` 插件下载 NuGet 依赖失败，MSBuild 退出码为 1。

## 常见原因

1. **NuGet 源访问问题**
   - 网络连接问题
   - 代理配置不正确
   - NuGet 源被墙或超时

2. **MSBuild 配置问题**
   - Visual Studio 版本不匹配
   - MSBuild 路径未配置
   - 缺少必要的 Windows SDK

3. **权限问题**
   - NuGet 缓存目录权限不足
   - 临时目录权限问题

4. **依赖版本冲突**
   - NuGet 包版本不兼容
   - .NET SDK 版本问题

---

## 🔧 解决方法

### 方法 1: 配置 NuGet 国内镜像

#### 1.1 查看当前 NuGet 配置

```bash
# 查看 NuGet 源
nuget sources list

# 或使用 dotnet
dotnet nuget list source
```

#### 1.2 添加国内镜像源

```bash
# 添加腾讯云镜像（推荐）
nuget sources Add -Name "Tencent" -Source "https://mirrors.cloud.tencent.com/nuget/"

# 或添加华为云镜像
nuget sources Add -Name "Huawei" -Source "https://repo.huaweicloud.com/repository/nuget/v3/index.json"

# 或使用 Azure China
nuget sources Add -Name "AzureChina" -Source "https://nuget.cdn.azure.cn/v3/index.json"
```

#### 1.3 设置 NuGet 配置文件

创建或编辑 `%AppData%\NuGet\NuGet.Config`:

```xml
<?xml version="1.0" encoding="utf-8"?>
<configuration>
  <packageSources>
    <add key="nuget.org" value="https://api.nuget.org/v3/index.json" protocolVersion="3" />
    <add key="Tencent" value="https://mirrors.cloud.tencent.com/nuget/" />
  </packageSources>
  <config>
    <add key="globalPackagesFolder" value="%userprofile%\.nuget\packages" />
    <add key="repositoryPath" value=".\packages" />
  </config>
</configuration>
```

---

### 方法 2: 清理缓存重试

```bash
# 清理 NuGet 缓存
nuget locals all -clear

# 或使用 dotnet
dotnet nuget locals all --clear

# 清理 Flutter 缓存
flutter clean
flutter pub cache clean

# 删除 pubspec.lock
del pubspec.lock  # Windows
rm pubspec.lock   # Linux/macOS

# 重新获取依赖
flutter pub get

# 重新构建
flutter build windows
```

---

### 方法 3: 使用代理

#### 3.1 配置系统代理

```bash
# PowerShell
$env:HTTP_PROXY="http://127.0.0.1:7890"
$env:HTTPS_PROXY="http://127.0.0.1:7890"

# 或使用 CMD
set HTTP_PROXY=http://127.0.0.1:7890
set HTTPS_PROXY=http://127.0.0.1:7890
```

#### 3.2 配置 NuGet 代理

在 `NuGet.Config` 中添加：

```xml
<configuration>
  <config>
    <add key="http_proxy" value="http://127.0.0.1:7890" />
    <add key="https_proxy" value="http://127.0.0.1:7890" />
  </config>
</configuration>
```

---

### 方法 4: 检查 Visual Studio 和 MSBuild

#### 4.1 确认 Visual Studio 版本

```bash
# 查找 MSBuild
where msbuild

# 查看版本
msbuild -version
```

**要求**:
- Visual Studio 2019 或更高版本
- Windows 10 SDK
- .NET Framework 4.7.2+

#### 4.2 安装缺失组件

打开 **Visual Studio Installer**，确保安装：
- ✅ 使用 C++ 的桌面开发
- ✅ Windows 10 SDK
- ✅ .NET 桌面开发

---

### 方法 5: 手动下载依赖（临时方案）

如果其他方法都失败，可以手动处理：

#### 5.1 查看具体依赖

```bash
# 进入插件目录
cd %LOCALAPPDATA%\Pub\Cache\hosted\pub.dev\webview_win_floating-*

# 查看 .csproj 或 pubspec.yaml
```

#### 5.2 手动下载 NuGet 包

1. 访问 https://www.nuget.org/
2. 搜索需要的包
3. 下载到本地
4. 放入 NuGet 缓存目录: `%userprofile%\.nuget\packages`

---

### 方法 6: 更新依赖版本

编辑 `pubspec.yaml`:

```yaml
dependencies:
  flutter:
    sdk: flutter
  
  # 尝试更新到最新版本
  webview_win_floating: ^2.3.0  # 检查最新版本
  
  # 或使用其他 webview 插件
  # webview_windows: ^0.4.0
  # desktop_webview_window: ^0.2.3
```

---

## 🚀 完整修复流程

### 步骤 1: 环境检查

```bash
# 检查 Flutter
flutter doctor -v

# 检查 NuGet
nuget help

# 检查 MSBuild
msbuild -version

# 检查 .NET SDK
dotnet --info
```

### 步骤 2: 配置镜像源

```bash
# 添加 NuGet 镜像
nuget sources Add -Name "Tencent" -Source "https://mirrors.cloud.tencent.com/nuget/"

# 验证
nuget sources list
```

### 步骤 3: 清理和重建

```bash
# 清理所有缓存
flutter clean
nuget locals all -clear
dotnet nuget locals all --clear

# 删除构建产物
Remove-Item -Recurse -Force build

# 重新获取依赖
flutter pub get

# 尝试构建
flutter build windows --verbose
```

### 步骤 4: 如果仍然失败

```bash
# 使用代理
$env:HTTP_PROXY="http://127.0.0.1:7890"
$env:HTTPS_PROXY="http://127.0.0.1:7890"

# 再次构建
flutter build windows --verbose

# 查看详细错误
```

---

## 📋 常见错误和解决方案

### 错误 1: "Unable to load the service index"

**原因**: NuGet 源无法访问

**解决**:
```bash
# 使用国内镜像
nuget sources Add -Name "Tencent" -Source "https://mirrors.cloud.tencent.com/nuget/"

# 或使用代理
$env:HTTPS_PROXY="http://127.0.0.1:7890"
```

### 错误 2: "MSBuild exited with code 1"

**原因**: MSBuild 配置问题或依赖冲突

**解决**:
```bash
# 更新 Visual Studio
# 安装 Windows SDK
# 清理缓存重试
```

### 错误 3: "Access to the path is denied"

**原因**: 权限不足

**解决**:
```bash
# 以管理员身份运行
# 或修改 NuGet 缓存目录权限
```

### 错误 4: "Package 'xxx' is not found"

**原因**: 包不存在或版本不对

**解决**:
```bash
# 更新插件版本
flutter pub upgrade

# 或手动指定版本
```

---

## 🔍 调试技巧

### 1. 查看详细构建日志

```bash
flutter build windows --verbose > build.log 2>&1
```

### 2. 单独测试 MSBuild

```bash
# 进入 windows 目录
cd windows

# 手动运行 MSBuild
msbuild Runner.sln /p:Configuration=Release /v:detailed
```

### 3. 检查 NuGet 日志

```bash
# 启用详细日志
$env:NUGET_SHOW_STACK=true

# 查看日志文件
Get-Content $env:TEMP\NuGet-*.log
```

---

## 🎯 替代方案

### 方案 A: 使用其他 WebView 插件

```yaml
dependencies:
  # 替换 webview_win_floating
  webview_windows: ^0.4.0
  # 或
  desktop_webview_window: ^0.2.3
```

### 方案 B: 暂时禁用 WebView

如果不是必需功能，可以临时注释掉：

```yaml
dependencies:
  # webview_win_floating: ^2.3.0  # 临时禁用
```

### 方案 C: 使用预构建的可执行文件

```bash
# 只更新 sing-box 核心
# 不重新构建整个 Flutter 应用
# 使用现有的 g5_client.exe
```

---

## 📞 获取帮助

### 查看插件 Issues

访问插件仓库查看已知问题：
- https://github.com/jakky1/webview_win_floating/issues
- https://pub.dev/packages/webview_win_floating

### Flutter 社区

- Flutter 中文社区: https://flutter.cn
- Stack Overflow: flutter + webview_win_floating

---

## ✅ 验证修复

```bash
# 构建成功后
flutter build windows

# 检查输出
ls build\windows\x64\runner\Release\

# 运行测试
build\windows\x64\runner\Release\g5_client.exe
```

---

**更新时间**: 2024-06-24
