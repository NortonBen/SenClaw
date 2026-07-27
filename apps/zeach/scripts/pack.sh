#!/usr/bin/env bash
# Build Search and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     zeach                 (release binary; manifest runtime.start = ./zeach)
#     senclaw-manifest.json
#     web_dist/              (built React UI — main.rs serves web_dist next to the binary)
#   zeach-app.zip           <- the artifact you install in SenClaw
#
# Usage: apps/zeach/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/zeach-app.zip"
BIN="$ROOT/target/release/zeach"

if [[ "${1:-}" != "--skip-build" ]]; then
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  ( cd "$ROOT" && cargo build -p zeach --release )
fi
[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }

rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/zeach"
chmod +x "$REL/zeach"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
[[ -f "$APP_DIR/senclaw-hub.json" ]] && cp "$APP_DIR/senclaw-hub.json" "$REL/senclaw-hub.json"
# skills/ and personas/ arrive in P4; copy them only once they exist so the
# manifest and the staged layout never disagree.
[[ -d "$APP_DIR/skills" ]]   && [[ -n "$(ls -A "$APP_DIR/skills" 2>/dev/null)" ]]   && cp -R "$APP_DIR/skills" "$REL/skills"
[[ -d "$APP_DIR/personas" ]] && [[ -n "$(ls -A "$APP_DIR/personas" 2>/dev/null)" ]] && cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"   # NOTE: dist -> web_dist rename

( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )
echo "packed: $ZIP"
