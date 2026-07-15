# AI Office — công ty một người 🏢

Space App mô phỏng ý tưởng "one-person company": Sếp (bạn) giao nhiệm vụ, một văn phòng
AI gồm **Trưởng phòng, Nghiên cứu, Nội dung, Phân tích, Kiểm định** tự phân công, làm
việc, bàn giao lẫn nhau và nộp lại **Báo cáo tổng hợp** — kèm mô phỏng văn phòng 3D
isometric theo thời gian thực (agent đi lại bàn giao, speech bubble, trạng thái
`đang làm / ✓ xong / đi bàn giao`).

## Vận hành

Mỗi bước là một lượt LLM thật qua daemon bridge (`llm.request`): Trưởng phòng lập kế
hoạch phân công (JSON), từng agent xử lý phần việc của mình với ngữ cảnh là các phần đã
bàn giao (+ trí nhớ riêng, wiki, skill nắm giữ), Kiểm định soát rủi ro, Trưởng phòng
tổng hợp báo cáo.

Biên chế linh hoạt: nhân sự **tự nhận nhiệm vụ** (luôn có phần việc) hoặc **tăng cường**
(chỉ được giao khi nhiệm vụ cần chuyên môn đó); có thể **tạm dừng / kích hoạt** từng
nhân sự và gán **skill / sub-agent** (persona) mà nhân sự nắm giữ — lấy từ inventory
của daemon (`/api/skills`, `/api/cowork/personas`).

## Kiến trúc

Một binary Rust (axum + rusqlite) phục vụ tất cả trên port **4420**:

- `/` — web UI (React + Vite, build sẵn trong `web_dist/`)
- `/api/*` — REST: `agents`, `tasks`, `tasks/:id/events` (UI poll mỗi giây), `stats`, `llm-info`
- `/api/mcp/sse` — MCP server **`ai-office-mcp`** (JSON-RPC 2.0 qua HTTP+SSE), 10 tools
  `office_*`: giao việc (`office_create_task`), theo dõi (`office_status`,
  `office_get_task`), lấy báo cáo (`office_get_report`), biên chế (`office_list_agents`,
  `office_add_agent`, `office_remove_agent`, `office_update_agent` — gồm enabled /
  auto_assign / skills), sổ sách (`office_stats`, `office_list_tasks`).

DB tại `~/.senclaw/space-apps/ai-office/ai-office.db` (bảng `agents`, `tasks`, `steps`, `events`).

## Trí nhớ riêng & kho tài liệu (cần daemon SenClaw mới)

- **Mỗi nhân sự có knowledge space riêng** `ai-office:<key>` trong hệ Knowledge của
  daemon: ở LIVE mode agent recall trí nhớ của mình trước khi làm và lưu memo sau
  khi xong — trí nhớ các agent độc lập, không lẫn nhau. Xem trong dialog Chi tiết
  của Nhân sự, hoặc trên desktop_app (Knowledge → dropdown chọn space).
- **Wiki là kho tài liệu của văn phòng**: trước khi làm việc phòng tra cứu
  `/api/wiki/search`, báo cáo tổng hợp tự lưu vào `wiki/ai-office/…` (event 📚).
- **Biên chế động**: thêm/sửa/xoá nhân sự (tối đa 7 bàn) qua UI hoặc MCP
  (`office_add_agent`, `office_remove_agent`); engine phân công theo roster hiện tại.

Kèm theo:

- `personas/` — 5 persona (manager/research/content/analysis/qa) được daemon cài khi
  install app; dùng được ngay với Cowork/DAG dispatch (`persona:ai-office__office-*`).
- `skills/` — `ai-office-run` (giao việc & chờ báo cáo), `ai-office-status` (xem tiến độ/báo cáo).
- widget `office-status` cho dashboard.

## Dev

```bash
cargo run -p ai-office            # backend trên :4420 (PORT env để đổi)
cd apps/ai-office/web && npm run dev   # Vite dev server, proxy /api → :4420
```

## Đóng gói

```bash
apps/ai-office/scripts/pack.sh    # build web + binary → release/ + ai-office-app.zip
```

Cài zip qua Space Apps của SenClaw; daemon tự chạy binary, health-check `/api/status`,
proxy iframe và auto-register MCP `ai-office-mcp`.
