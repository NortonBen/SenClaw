#!/usr/bin/env bash
# Build SenClaw Video Flow and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     video-flow             (release binary; manifest runtime.start = ./video-flow)
#     senclaw-manifest.json
#     skills/                (video-flow-produce, video-flow-manage)
#     personas/              (video-producer)
#     souls/                 (per-sub-agent system prompts — editable at runtime)
#     playbooks/             (internal skill-agent prompt playbooks)
#     extension/             (the Chrome MV3 Google Flow bridge — ships with the app)
#     web_dist/              (built React UI — main.rs serves web_dist next to the binary)
#   video-flow-app.zip       <- the artifact you install in SenClaw
#
# Usage: apps/video-flow/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/video-flow-app.zip"
BIN="$ROOT/target/release/video-flow"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p video-flow --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/video-flow"
chmod +x "$REL/video-flow"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
cp -R "$APP_DIR/skills" "$REL/skills"
cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/souls" "$REL/souls"
cp -R "$APP_DIR/playbooks" "$REL/playbooks"
cp -R "$APP_DIR/extension" "$REL/extension"
# Chrome refuses to load an unpacked extension containing a _metadata dir.
rm -rf "$REL/extension/_metadata"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
