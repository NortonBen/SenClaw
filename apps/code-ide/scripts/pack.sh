#!/usr/bin/env bash
# Build SenClaw Code (IDE) and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     code-ide               (release binary; manifest runtime.start = ./code-ide)
#     senclaw-manifest.json
#     skills/                (code-edit, explain-selection)
#     personas/              (pair-programmer)
#     web_dist/              (built React UI — main.rs serves web_dist next to the binary)
#   code-ide-app.zip         <- the artifact you install in SenClaw
#
# Usage: apps/code-ide/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/code-ide-app.zip"
BIN="$ROOT/target/release/code-ide"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building vendored DeepWiki UI"
  ( cd "$APP_DIR/deepwiki-web" && npm install --silent && npm run build )
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p code-ide --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }
[[ -d "$APP_DIR/deepwiki-web/dist" ]] || { echo "missing deepwiki-web/dist — build it first"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/code-ide"
chmod +x "$REL/code-ide"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
cp -R "$APP_DIR/skills" "$REL/skills"
cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"
cp -R "$APP_DIR/deepwiki-web/dist" "$REL/deepwiki_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
