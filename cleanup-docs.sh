#!/bin/bash

# 要保留的核心文档
KEEP_DOCS=(
    "README.md"
    "FINAL_100_COMPLETION.md"
    "ARCHITECTURE_AUDIT.md"
    "DEVELOPER_MODE.md"
    "FEATURE_COMPARISON.md"
    "REMAINING_FEATURES.md"
    "HYSTERIA2_SINGBOX_COMPATIBILITY.md"
    "CONNECTION_TROUBLESHOOTING.md"
    "CONNECTION_ISSUE_ACTIVE_PROBING.md"
)

echo "========================================"
echo "  🗑️  清理不需要的 MD 文件"
echo "========================================"
echo ""
echo "保留以下核心文档："
for doc in "${KEEP_DOCS[@]}"; do
    echo "  ✅ $doc"
done
echo ""

# 删除其他 MD 文件
echo "删除以下文档："
for file in *.md; do
    # 检查是否在保留列表中
    keep=false
    for keep_doc in "${KEEP_DOCS[@]}"; do
        if [ "$file" = "$keep_doc" ]; then
            keep=true
            break
        fi
    done
    
    # 如果不在保留列表，删除
    if [ "$keep" = false ] && [ -f "$file" ]; then
        echo "  ❌ $file"
        rm "$file"
    fi
done

echo ""
echo "✅ 清理完成！"
echo ""
echo "剩余文档："
ls -1 *.md | wc -l
