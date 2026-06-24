#!/usr/bin/env bash
# 性能分析脚本

set -e

echo "🔍 rsbox 性能分析工具"
echo "========================"
echo ""

RSBOX_BIN="${RSBOX_BIN:-./target/release/rsbox}"
CONFIG="${CONFIG:-config.example.json}"

# 检查工具
check_tools() {
    local missing=()

    for tool in perf flamegraph time; do
        if ! command -v $tool &> /dev/null; then
            missing+=("$tool")
        fi
    done

    if [ ${#missing[@]} -gt 0 ]; then
        echo "⚠️  缺少工具: ${missing[*]}"
        echo "安装: sudo apt-get install linux-tools-common flamegraph"
    fi
}

# 内存分析
analyze_memory() {
    echo "📊 内存使用分析"
    echo "----------------"

    # 启动程序
    $RSBOX_BIN run -c $CONFIG &
    PID=$!
    sleep 2

    # 监控 5 秒
    for i in {1..5}; do
        if ps -p $PID > /dev/null; then
            ps -p $PID -o pid,rss,vsz,pmem,comm
            sleep 1
        fi
    done

    kill $PID 2>/dev/null || true
    echo ""
}

# CPU 分析
analyze_cpu() {
    echo "📊 CPU 使用分析"
    echo "----------------"

    if command -v perf &> /dev/null; then
        echo "使用 perf 进行性能分析..."
        sudo perf record -F 99 -g -- $RSBOX_BIN run -c $CONFIG &
        PERF_PID=$!
        sleep 10
        kill $PERF_PID

        sudo perf report
    else
        echo "⚠️  perf 未安装，跳过 CPU 分析"
    fi
    echo ""
}

# 启动时间分析
analyze_startup() {
    echo "⏱️  启动时间分析"
    echo "----------------"

    for i in {1..5}; do
        /usr/bin/time -f "启动时间: %E (用户: %U, 系统: %S)" \
            $RSBOX_BIN check -c $CONFIG 2>&1 | grep "启动时间"
    done
    echo ""
}

# 二进制分析
analyze_binary() {
    echo "📦 二进制文件分析"
    echo "----------------"

    if [ -f "$RSBOX_BIN" ]; then
        echo "大小: $(du -h $RSBOX_BIN | cut -f1)"
        echo "类型: $(file $RSBOX_BIN)"

        if command -v cargo &> /dev/null; then
            echo ""
            echo "依赖分析:"
            cargo tree --depth 1 | head -20
        fi
    else
        echo "❌ 二进制文件不存在: $RSBOX_BIN"
    fi
    echo ""
}

# 主函数
main() {
    check_tools

    if [ ! -f "$CONFIG" ]; then
        echo "❌ 配置文件不存在: $CONFIG"
        exit 1
    fi

    analyze_binary
    analyze_startup
    analyze_memory

    echo "✅ 性能分析完成"
    echo ""
    echo "详细分析建议:"
    echo "  1. 使用 cargo-bloat: cargo install cargo-bloat && cargo bloat --release"
    echo "  2. 使用 cargo-flamegraph: cargo install flamegraph && cargo flamegraph"
    echo "  3. 使用 valgrind: valgrind --leak-check=full $RSBOX_BIN run -c $CONFIG"
}

main "$@"
