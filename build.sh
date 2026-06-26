#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

echo "==> Building rsbox (desktop CLI)..."
cargo build --release -p rsbox

echo "==> Building rsb-libbox (mobile FFI)..."
cargo build --release -p rsb-libbox

echo
echo "Done."
echo "  Desktop: target/release/rsbox"
echo "  Libbox:  target/release/librsb_libbox.so (Linux/Android) or .dylib/.a (macOS/iOS)"
