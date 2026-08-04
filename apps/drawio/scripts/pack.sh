#!/usr/bin/env bash
# Build + stage + zip the SenClaw Diagrams (draw.io) Space App.
# The draw.io editor webapp is NOT bundled (draw.war ~53MB exceeds install
# limits) — the binary downloads it on first run. The zip stays small.
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/drawio-app.zip"

( cd "$APP_DIR/web" && npm install --silent && npm run build )
( cd "$ROOT" && cargo build -p drawio --release )

rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$ROOT/target/release/drawio" "$REL/drawio"
chmod +x "$REL/drawio"
cp "$APP_DIR/senclaw-manifest.json" "$REL/"
[ -f "$APP_DIR/senclaw-hub.json" ] && cp "$APP_DIR/senclaw-hub.json" "$REL/"
[ -d "$APP_DIR/skills" ] && cp -R "$APP_DIR/skills" "$REL/skills"
[ -d "$APP_DIR/personas" ] && cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )
echo "Packed: $ZIP ($(du -h "$ZIP" | cut -f1))"
