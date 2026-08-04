#!/usr/bin/env bash
# Đóng gói AI Discuss Team thành discuss-app.zip (stage phẳng trong release/).
# Dùng: scripts/pack.sh [--skip-build]
set -euo pipefail

APP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "$APP_DIR/../.." && pwd)"
SKIP_BUILD="${1:-}"

if [[ "$SKIP_BUILD" != "--skip-build" ]]; then
  echo "==> Build web UI"
  (cd "$APP_DIR/web" && npm install --silent && npm run build)
  echo "==> Build Rust release"
  (cd "$REPO_ROOT" && cargo build -p discuss --release)
fi

BIN="$REPO_ROOT/target/release/discuss"
[[ -x "$BIN" ]] || { echo "Thiếu binary $BIN — chạy không kèm --skip-build"; exit 1; }
[[ -d "$APP_DIR/web/dist" ]] || { echo "Thiếu web/dist — build web trước"; exit 1; }

STAGE="$APP_DIR/release"
echo "==> Stage $STAGE"
rm -rf "$STAGE"
mkdir -p "$STAGE"
cp "$BIN" "$STAGE/discuss"
chmod +x "$STAGE/discuss"
cp "$APP_DIR/senclaw-manifest.json" "$STAGE/"
cp -R "$APP_DIR/skills" "$STAGE/skills"
cp -R "$APP_DIR/personas" "$STAGE/personas"
cp -R "$APP_DIR/web/dist" "$STAGE/web_dist"

echo "==> Zip"
(cd "$STAGE" && rm -f discuss-app.zip && zip -rq discuss-app.zip . -x '*.DS_Store')
ls -lh "$STAGE/discuss-app.zip"

cat <<EOF

Đăng ký chạy dev (KHÔNG trỏ vào apps/discuss — binary nằm trong release/):
  curl -X POST http://127.0.0.1:18788/api/space/apps/register-local \\
    -H 'Content-Type: application/json' \\
    -d '{"path":"${STAGE}"}'
EOF
