#!/usr/bin/env bash
# Build SenClaw Siêu Dự Đoán and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     predict                (release binary; manifest runtime.start = ./predict)
#     senclaw-manifest.json
#     senclaw-hub.json
#     README.md
#     skills/  personas/
#     web_dist/              (built React UI — main.rs serves web_dist next to the binary)
#   predict-app.zip          <- the artifact you install in SenClaw
#
# Usage: apps/predict/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/predict-app.zip"
BIN="$ROOT/target/release/predict"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI (React + Ant Design)"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p predict --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -f "$APP_DIR/web/dist/index.html" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/predict"
chmod +x "$REL/predict"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
cp "$APP_DIR/senclaw-hub.json" "$REL/senclaw-hub.json"
cp "$APP_DIR/README.md" "$REL/README.md"
cp -R "$APP_DIR/skills" "$REL/skills"
cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

# Daemon install-zip ceiling is 50MB compressed (space.rs). Warn if exceeded.
ZBYTES=$(wc -c < "$ZIP")
echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
if (( ZBYTES > 50 * 1024 * 1024 )); then
  echo "  !! CẢNH BÁO: zip > 50MB — vượt trần install-zip của daemon."
fi
