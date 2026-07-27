#!/usr/bin/env bash
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/rule-engine-app.zip"
BIN="$ROOT/target/release/rule-engine"

if [[ "${1:-}" != "--skip-build" ]]; then
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  ( cd "$ROOT" && cargo build -p rule-engine --release )
fi

[[ -f "$BIN" ]] || { echo "thiếu binary: $BIN"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "thiếu web/dist"; exit 1; }

rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/rule-engine"
chmod +x "$REL/rule-engine"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
[[ -d "$APP_DIR/skills"   ]] && cp -R "$APP_DIR/skills"   "$REL/skills"   || true
[[ -d "$APP_DIR/personas" ]] && cp -R "$APP_DIR/personas" "$REL/personas" || true
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )
echo "đã đóng gói: $ZIP"
