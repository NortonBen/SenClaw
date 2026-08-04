#!/usr/bin/env bash
# Build SenClaw Mini Browser and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     mini-browser           (release binary; manifest runtime.start = ./mini-browser)
#     senclaw-manifest.json
#     skills/                (browse-web, web-extract, web-task)
#     personas/              (web-operator)
#     web_dist/              (built React UI — main.rs serves web_dist next to the binary)
#   mini-browser-app.zip     <- the artifact you install in SenClaw
#
# Usage: apps/mini-browser/scripts/pack.sh [--skip-build]
#
# NOTE: at runtime the app needs Google Chrome / Chromium on the host (driven via
# CDP). Set MB_CHROME to override the executable path. The browser now runs
# headful by default wherever a display exists; MB_HEADLESS=1 forces headless.
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/mini-browser-app.zip"
BIN="$ROOT/target/release/mini-browser"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p mini-browser --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/mini-browser"
chmod +x "$REL/mini-browser"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
cp -R "$APP_DIR/skills" "$REL/skills"
cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
