#!/usr/bin/env bash
# Build SenClaw Luna Calendar and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     luna-calendar          (release binary; manifest runtime.start = ./luna-calendar)
#     senclaw-manifest.json
#     skills/                (luna-xem-ngay)
#     personas/              (luna-almanac-master)
#     web_dist/              (built React UI — main.rs serves web_dist next to the binary)
#   luna-calendar-app.zip    <- the artifact you install in SenClaw
#
# Usage: apps/luna-calendar/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/luna-calendar-app.zip"
BIN="$ROOT/target/release/luna-calendar"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p luna-calendar --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/luna-calendar"
chmod +x "$REL/luna-calendar"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
cp -R "$APP_DIR/skills" "$REL/skills"
cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
