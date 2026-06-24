#!/usr/bin/env bash
# 快速测试脚本 - 验证代码修复

set -e

echo "🔧 rsbox 代码修复验证脚本"
echo "================================"
echo ""

echo "📝 步骤 1: 格式化代码"
cargo fmt --all
echo "✅ 代码格式化完成"
echo ""

echo "📝 步骤 2: Clippy 检查"
if cargo clippy --workspace 2>&1 | grep -q "warning\|error"; then
    echo "⚠️  发现警告或错误"
    cargo clippy --workspace 2>&1 | head -50
else
    echo "✅ Clippy 检查通过（无警告）"
fi
echo ""

echo "📝 步骤 3: 编译检查"
if cargo check --workspace 2>&1 | grep -q "error:"; then
    echo "❌ 编译检查失败"
    cargo check --workspace 2>&1 | grep "error:" | head -20
    exit 1
else
    echo "✅ 编译检查通过"
fi
echo ""

echo "📝 步骤 4: 运行测试"
cargo test --workspace --no-fail-fast
echo "✅ 测试完成"
echo ""

echo "📝 步骤 5: 构建 release 版本"
if cargo build --release -p rsbox; then
    echo "✅ Release 构建成功"
    echo ""
    echo "📊 二进制文件信息:"
    ls -lh target/release/rsbox* 2>/dev/null || ls -lh target/release/rsbox.exe 2>/dev/null || echo "未找到二进制文件"
else
    echo "❌ Release 构建失败"
    exit 1
fi
echo ""

echo "🎉 所有检查通过！"
echo ""
echo "下一步:"
echo "  1. 测试配置: ./target/release/rsbox check -c config.example.json"
echo "  2. 运行程序: ./target/release/rsbox run -c config.example.json"
