#!/usr/bin/env bash
# Build SenClaw Widget Pack and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     widget-pack            (release binary; manifest runtime.start = ./widget-pack)
#     senclaw-manifest.json
#     web_dist/              (static widget pages — main.rs serves web_dist next to the binary)
#   widget-pack-app.zip      <- the artifact you install in SenClaw
#
# No web build step: web/ is plain static HTML used as-is.
#
# Usage: apps/widget-pack/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/widget-pack-app.zip"
BIN="$ROOT/target/release/widget-pack"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p widget-pack --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -f "$APP_DIR/web/index.html" ]] || { echo "missing web/index.html"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/widget-pack"
chmod +x "$REL/widget-pack"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
[[ -d "$APP_DIR/skills" ]] && cp -R "$APP_DIR/skills" "$REL/skills" || true
cp -R "$APP_DIR/web" "$REL/web_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
