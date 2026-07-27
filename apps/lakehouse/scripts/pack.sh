#!/usr/bin/env bash
# Build SenClaw Lakehouse and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     lakehouse              (release binary; manifest runtime.start = ./lakehouse)
#     senclaw-manifest.json
#     skills/  personas/
#     web_dist/              (built React UI — main.rs serves web_dist next to the binary)
#   lakehouse-app.zip        <- the artifact you install in SenClaw (limit 50MB nén — §11)
#
# Usage: apps/lakehouse/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/lakehouse-app.zip"
BIN="$ROOT/target/release/lakehouse"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building release binary (DataFusion — có thể mất vài phút + vài GB target/)"
  ( cd "$ROOT" && cargo build -p lakehouse --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/lakehouse"
chmod +x "$REL/lakehouse"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
[[ -d "$APP_DIR/skills" ]] && cp -R "$APP_DIR/skills" "$REL/skills" || true
[[ -d "$APP_DIR/personas" ]] && cp -R "$APP_DIR/personas" "$REL/personas" || true
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

# Trần install-zip của daemon là 50MB nén (space.rs:939) + DefaultBodyLimit 64MB
# (core.rs:635). Cảnh báo nếu vượt — §11 design doc.
ZBYTES=$(wc -c < "$ZIP")
echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
if (( ZBYTES > 50 * 1024 * 1024 )); then
  echo "  !! CẢNH BÁO: zip > 50MB — vượt trần install-zip; xem §11 (nâng cả space.rs:939 + core.rs:635)."
fi
