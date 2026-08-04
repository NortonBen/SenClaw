# SenClaw Diagrams (draw.io) — Space App

Nhúng trình vẽ **draw.io** đầy đủ vào SenClaw + LLM tự vẽ sơ đồ từ mô tả.
Thiết kế chi tiết: [docs/drawio-app-design.md](../../docs/drawio-app-design.md).

- Port **4610**, MCP server **drawio-mcp** (`mcp__drawio-mcp__drawio_*`, 10 tools).
- Editor = webapp draw.io chính thức, iframe same-origin `/drawio/` với embed
  protocol (`embed=1&proto=json&stealth=1` — không gọi ra ngoài).
- **Composite download**: `draw.war` v31.1.2 (~53MB) vượt giới hạn zip app, nên
  binary tự tải từ GitHub release ở lần chạy đầu vào
  `~/.senclaw/space-apps/drawio/editor/`, sau đó offline hoàn toàn.
- AI 2 chế độ: **Mermaid** (nhanh — editor convert thành shapes sửa được) và
  **mxGraph XML** (chi tiết — theo 10 quy tắc chính thức của draw.io, có
  validate + repair XML cụt). `finish=="length"` từ bridge luôn bị coi là lỗi.

## Dev loop

```bash
# backend (từ repo root)
cargo run -p drawio
# web UI (hot reload, proxy /api + /drawio → 4610)
cd apps/drawio/web && npm install && npm run dev
# đăng ký với daemon không cần zip
curl -X POST http://127.0.0.1:18788/api/space/apps/register-local \
  -H 'Content-Type: application/json' -d '{"path": "'$PWD'/apps/drawio"}'
```

## Env

| Var | Ý nghĩa |
|---|---|
| `PORT` | daemon inject; mặc định 4610 |
| `SENCLAW_DRAWIO_EDITOR_DIR` | trỏ webapp đã giải nén sẵn (dev/air-gapped) — bỏ qua download |
| `SENCLAW_DRAWIO_WAR_URL` | mirror thay GitHub release |
| `SENCLAW_DRAWIO_WAR_SHA256` | override checksum pin |

## Gotchas

- SVG chỉ render được **trong editor** → server cache snapshot mỗi lần save/
  autosave; `drawio_export svg` trả kèm `stale` flag.
- Iframe host của SenClaw không có `allow-downloads` → mọi download đi qua
  `GET /api/diagrams/:id/export?format=svg|xml`.
- Đổi version editor: sửa `DRAWIO_VERSION`/`WAR_SHA256` trong `src/editor.rs`
  (file `VERSION` trong data dir sẽ trigger tải lại).

## Pack

```bash
apps/drawio/scripts/pack.sh   # → drawio-app.zip (nhỏ, không chứa editor)
```
