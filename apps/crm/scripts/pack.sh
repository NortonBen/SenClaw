#!/usr/bin/env bash
# Build SenClaw CRM and assemble an installable Space-App zip.
#
#   release/                 <- staged, flat install layout
#     crm                    (release binary; manifest runtime.start = ./crm)
#     senclaw-manifest.json
#     skills/                (whole dir: crm-quick-lookup, crm-log-interaction,
#                             crm-organizations, crm-inbox, crm-sale-followup,
#                             crm-sale-inbox — every path the manifest lists)
#     personas/              (whole dir: crm-assistant, sale-closer, sale-manager)
#     web_dist/              (built React UI — main.rs serves web_dist next to the binary)
#   crm-app.zip              <- the artifact you install in SenClaw
#
# skills/ and personas/ are copied wholesale, so adding one needs no change
# here — only a manifest entry. `docs/` is deliberately NOT staged: it is
# developer reference, and nothing at runtime reads it.
#
# Usage: apps/crm/scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "$0")/.." && pwd)"
ROOT="$(cd "$APP_DIR/../.." && pwd)"
REL="$APP_DIR/release"
ZIP="$APP_DIR/crm-app.zip"
BIN="$ROOT/target/release/crm"

if [[ "${1:-}" != "--skip-build" ]]; then
  echo "==> building web UI"
  ( cd "$APP_DIR/web" && npm install --silent && npm run build )
  echo "==> building release binary"
  ( cd "$ROOT" && cargo build -p crm --release )
fi

[[ -f "$BIN" ]] || { echo "missing $BIN — run without --skip-build"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "missing web/dist — build the web UI first"; exit 1; }

echo "==> staging release/"
rm -rf "$REL" "$ZIP"
mkdir -p "$REL"
cp "$BIN" "$REL/crm"
chmod +x "$REL/crm"
cp "$APP_DIR/senclaw-manifest.json" "$REL/senclaw-manifest.json"
cp -R "$APP_DIR/skills" "$REL/skills"
cp -R "$APP_DIR/personas" "$REL/personas"
cp -R "$APP_DIR/web/dist" "$REL/web_dist"

echo "==> zipping -> $ZIP"
( cd "$REL" && zip -rq "$ZIP" . -x '*.DS_Store' )

echo "done:"
echo "  staged: $REL"
echo "  zip:    $ZIP ($(du -h "$ZIP" | cut -f1))"
