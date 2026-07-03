#!/usr/bin/env bash
# Build SSH Manager and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     ssh-manager            (release binary; manifest runtime.start = ./ssh-manager)
#     senclaw-manifest.json
#     skills/                (ssh-connect / ssh-guide / ssh-reporting)
#     web_dist/              (built React UI — main.rs serves web_dist next to the binary)
#   ssh-manager.zip          <- the artifact you install in SenClaw
#
# Usage: apps/ssh-manager/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/ssh-manager.zip"
BIN="$ROOT/target/release/ssh-manager"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p ssh-manager --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/ssh-manager"
chmod +x "$REL/ssh-manager"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
cp -R "$APP_DIR/skills" "$REL/skills"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

echo "==> zipping"
( cd "$REL" && zip -qr "$ZIP" senclaw-manifest.json ssh-manager skills web_dist )
echo "packed: $ZIP"
