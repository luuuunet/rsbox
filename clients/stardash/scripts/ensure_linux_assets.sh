#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ASSET_DIR="$ROOT/g5_client/assets/binaries/linux"
mkdir -p "$ASSET_DIR"
if [[ ! -f "$ASSET_DIR/sing-box" ]]; then
  : > "$ASSET_DIR/sing-box"
fi
echo "Linux asset placeholder ok"
