#!/usr/bin/env bash
# Build SenClaw DOCX Editor and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     docx-editor             (release binary; manifest runtime.start = ./docx-editor)
#     senclaw-manifest.json
#     skills/                (docx-open, docx-edit)
#     personas/              (docx-writer)
#     web_dist/              (built React UI — main.rs serves web_dist next to the binary)
#   docx-editor-app.zip      <- the artifact you install in SenClaw
#
# Usage: apps/docx-editor/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/docx-editor-app.zip"
BIN="$ROOT/target/release/docx-editor"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p docx-editor --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/docx-editor"
chmod +x "$REL/docx-editor"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
cp -R "$APP_DIR/skills" "$REL/skills"
cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
