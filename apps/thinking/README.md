# Tư Duy — 6 Mũ & 5W (SenClaw Space App)

Phân tích, đánh giá vấn đề và ra quyết định **100% cục bộ** theo hai phương pháp
kinh điển:

- **5W** — Who / What / When / Where / Why: làm rõ bản chất vấn đề và nguyên
  nhân gốc trước khi bàn giải pháp.
- **6 Mũ Tư Duy** (Edward de Bono) — ⚪ Trắng dữ kiện · 🔴 Đỏ cảm xúc · ⚫ Đen
  rủi ro · 🟡 Vàng lợi ích · 🟢 Xanh Lá sáng tạo · 🔵 Xanh Dương tổng kết: soi
  vấn đề từ sáu góc nhìn có kỷ luật.

Vòng đời một vấn đề: `open` → `analyzing` → `decided` → `closed`. Mỗi vấn đề có
5 ô 5W + 6 ô mũ (điền tay hoặc AI soạn nháp — AI **không ghi đè** nội dung người
dùng trừ khi `force`), danh sách giải pháp được chấm 4 tiêu chí 0–10 (lợi ích /
rủi ro / khả thi / công sức) với **điểm tổng hợp 0–100 do hệ thống tính**
(lợi ích 35% + an toàn 30% + khả thi 25% + nhẹ công 10%), bảng so sánh xếp hạng,
quyết định kèm lý do và báo cáo markdown đầy đủ.

Độ hoàn thiện phân tích (completeness) = 5W chiếm 40% + 6 mũ chiếm 60%, luôn suy
ra từ dữ liệu thật.

AI chạy qua **bridge SenClaw** (`llm.request`) — app không bao giờ thấy khóa
provider. App chỉ **ghi sổ phân tích** — không tool nào thực thi quyết định
trong thế giới thật.

## Build & run

```bash
# backend (từ repo root)
cargo build -p thinking
PORT=4650 ./target/debug/thinking

# web UI
cd apps/thinking/web && npm install && npm run build   # hoặc npm run dev (proxy :4650)

# đóng gói zip cài vào SenClaw
apps/thinking/scripts/pack.sh
```

Dữ liệu: `~/.senclaw/apps/thinking/thinking.db` (đổi bằng `SENCLAW_DATA_DIR`).

## MCP

Server `thinking-mcp` (HTTP + SSE tại `/api/mcp/sse`), tool prefix `think_` —
tên đầy đủ từ Claude Code: `mcp__thinking-mcp__think_*`. 21 tools:

| Nhóm | Tools |
|---|---|
| Tổng quan | `think_status` · `think_dashboard` · `think_activity` |
| Vấn đề | `think_problem_add` · `think_problem_list` · `think_problem_get` · `think_problem_update` · `think_problem_delete` |
| 5W | `think_5w_set` (tay) · `think_5w_generate` (AI, chỉ ô trống trừ khi force) |
| 6 mũ | `think_hat_set` (tay) · `think_hats_generate` (AI, một mũ hoặc cả sáu) |
| Giải pháp | `think_solution_add` · `think_solutions_generate` (AI) · `think_solution_update` · `think_solution_delete` |
| Đánh giá | `think_solution_evaluate` (AI hoặc chấm tay đủ 4 điểm) · `think_compare` |
| Kết luận | `think_decide` · `think_analyze` (trọn gói) · `think_report` |

## REST

`GET /api/status` · `GET /api/dashboard` · `GET /api/activity` ·
`GET|POST /api/problems` · `GET|POST /api/problems/:id` ·
`POST /api/problems/:id/{delete,w,w/generate,hats,hats/generate,solutions,solutions/generate,decide,analyze}` ·
`GET /api/problems/:id/{compare,report}` ·
`POST /api/solutions/:id{,/delete,/evaluate}`

## Tests

```bash
cargo test -p thinking   # db + logic + llm(parse) + mcp schema
```
