#!/usr/bin/env bash
set -euo pipefail
APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/ai-office-app.zip"
BIN="$ROOT/target/release/ai-office"

if [[ "${1:-}" != "--skip-build" ]]; then
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  ( cd "$ROOT" && cargo build -p ai-office --release )
fi
[[ -f "$BIN" ]] || { echo "missing $BIN"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "missing web/dist"; exit 1; }

rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/ai-office"
chmod +x "$REL/ai-office"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
cp -R "$APP_DIR/skills" "$REL/skills"
cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )
echo "packed: $ZIP"
