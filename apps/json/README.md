# JSON Tools — SenClaw Space App

Bộ công cụ JSON chạy **hoàn toàn cục bộ** (không CDN, không gọi mạng): format /
validate / viewer / diff, chuyển đổi giữa JSON ↔ CSV · XML · YAML · TSV · TOON ·
TSON, formatter cho XML/Java/Python/JS, và encoder base64 · URL · MessagePack.

- **Port**: 4540
- **MCP**: `json-mcp` tại `/api/mcp/sse` (7 tool)
- **Skill**: `json-toolbox` · **Persona**: `json-wrangler`

Giao diện React port từ dự án `json-tool` (Vite + React 19 + ace + Bootstrap 3).
Khác biệt so với bản gốc:

- Bỏ toàn bộ script CDN (jQuery, bootstrap.js, Font Awesome). Dropdown/navbar chạy
  bằng state React trong [`Nav.tsx`](web/src/components/Nav.tsx); các icon `fa-*`
  được thay bằng ký tự Unicode trong [`globals.css`](web/src/styles/globals.css).
- Bootstrap CSS được nhúng sẵn ở `web/public/dist/4.0/css/jsonmain.css`; các file
  JS legacy của trang gốc đã bị xoá (không được dùng).
- `vite base: '/'` + SPA fallback phía Rust để deep route (`/json-to-csv`, …) trả
  **200 kèm index.html**, còn asset thiếu vẫn 404 (xem [`main.rs`](src/main.rs)).

## Chạy dev

```bash
cargo run -p json-tool                       # API + UI đã build tại :4540
( cd apps/json/web && npm install && npm run dev )   # Vite dev, proxy /api → 4540
```

## Test & đóng gói

```bash
cargo test -p json-tool        # 27 test cho fmt / convert / mcp
apps/json/scripts/pack.sh      # → apps/json/json-app.zip (cài vào SenClaw)
```

## MCP tools (`json-mcp`)

| Tool | Việc |
|---|---|
| `json_format` | pretty/minify, **giữ nguyên thứ tự khoá** |
| `json_validate` | hợp lệ hay không, kèm dòng/cột lỗi |
| `json_convert` | json ↔ yaml ↔ csv ↔ tsv ↔ xml (mọi chiều) |
| `json_query` | lấy giá trị theo JSON Pointer / `a.b[0]` |
| `json_diff` | so sánh cấu trúc, liệt kê added/removed/changed |
| `json_encode` / `json_decode` | base64 · URL · MessagePack (base64) |

Ghi chú kỹ thuật:

- `json_format` **không** đi qua `serde_json::Value` (crate cố tình không bật
  `preserve_order` để tránh rò tính năng sang các member khác của workspace) —
  [`fmt.rs`](src/fmt.rs) scan lại text gốc nên thứ tự khoá và số liệu giữ nguyên.
  Các phép chuyển đổi khác đi qua `Value` nên khoá bị sắp A→Z.
- CSV/TSV cần **mảng các object**; ô rỗng → `null`, chuỗi số có số 0 ở đầu
  (số điện thoại, mã bưu chính) giữ nguyên dạng chuỗi.
- XML dùng quy ước "compact": thuộc tính `@name`, text `#text`, thẻ trùng tên gom
  thành mảng.
- TOON/TSON và formatter Java/Python/JS chỉ có ở giao diện web (thư viện JS),
  không có trong MCP.

## REST

`POST /api/{format,validate,convert,diff,query,encode,decode}` — thân JSON, luôn
trả HTTP 200 với `{ ok: true, … }` hoặc `{ ok: false, error }`.
`GET /api/status` là health check của manifest.
