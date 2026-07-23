#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SECRETS="$ROOT/g5_client/lib/config/app_secrets.dart"
EXAMPLE="$ROOT/g5_client/lib/config/app_secrets.example.dart"
if [[ ! -f "$SECRETS" ]]; then
  cp "$EXAMPLE" "$SECRETS"
  echo "Created app_secrets.dart from example (CI / dev stub)."
else
  echo "app_secrets.dart already present."
fi
