#!/usr/bin/env bash
# Build SenClaw Social and assemble an installable Space-App zip.
#
#   release/                  <- staged, flat install layout
#     social                  (release binary; manifest runtime.start = ./social)
#     senclaw-manifest.json
#     skills/                 (social-manage, social-engage)
#     personas/               (social-manager)
#     extension/              (the shared MV3 Chrome extension — load unpacked)
#     web_dist/               (built React + Ant Design admin UI — main.rs serves web_dist next to the binary)
#   social-app.zip            <- the artifact you install in SenClaw
#
# Usage: apps/social/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/social-app.zip"
BIN="$ROOT/target/release/social"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI (React + Ant Design)"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building extension (WXT + TypeScript)"
  ( cd "$APP_DIR/extension" && npm install --silent && npm run build )
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p social --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -f "$APP_DIR/web/dist/index.html" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }
[[ -f "$APP_DIR/extension/dist/chrome-mv3/manifest.json" ]] || { echo "missing extension/dist — build the extension first"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/social"
chmod +x "$REL/social"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
cp -R "$APP_DIR/skills" "$REL/skills"
cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/extension/dist/chrome-mv3" "$REL/extension"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
