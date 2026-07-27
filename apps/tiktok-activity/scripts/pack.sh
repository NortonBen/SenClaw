#!/usr/bin/env bash
# Build TikTok Activity and assemble an installable Space-App zip.
#
#   release/                  <- staged, flat install layout
#     tiktok-activity         (release binary; manifest runtime.start = ./tiktok-activity)
#     senclaw-manifest.json
#     senclaw-hub.json
#     skills/                 (tiktok-activity)
#     souls/                  (tiktok-operator persona)
#     extension/              (MV3 Chrome extension — load unpacked to control 1 TikTok tab)
#     web_dist/               (built React + Ant Design admin UI — main.rs serves web_dist next to the binary)
#   tiktok-activity-app.zip   <- the artifact you install in SenClaw
#
# Usage: apps/tiktok-activity/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/tiktok-activity-app.zip"
BIN="$ROOT/target/release/tiktok-activity"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI (React + Ant Design)"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p tiktok-activity --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -f "$APP_DIR/web/dist/index.html" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }

echo "==> staging release/ (extension/ preserved)"
# Keep any already-staged extension/ (it's a plain unpacked MV3 folder), rebuild the rest.
find "$REL" -mindepth 1 -maxdepth 1 ! -name extension -exec rm -rf {} + 2>/dev/null || true
rm -f "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/tiktok-activity"
chmod +x "$REL/tiktok-activity"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
[[ -f "$APP_DIR/senclaw-hub.json" ]] && cp "$APP_DIR/senclaw-hub.json" "$REL/senclaw-hub.json"
cp -R "$APP_DIR/skills" "$REL/skills"
[[ -d "$APP_DIR/souls" ]] && cp -R "$APP_DIR/souls" "$REL/souls"
rm -rf "$REL/extension"
cp -R "$APP_DIR/extension" "$REL/extension"
rm -rf "$REL/web_dist"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
