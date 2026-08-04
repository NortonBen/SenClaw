# SenClaw AutoTest — Tự Động Hoá Kiểm Thử

Space App tự động hoá kiểm thử, dữ liệu 100% local (SQLite tại
`~/.senclaw/apps/autotest/autotest.db`). Port **4640**, MCP server **`autotest-mcp`**
(tool prefix `autotest_`, 21 tools).

## Tính năng

- **Bộ kiểm thử (suite) + test case 3 loại:**
  - `http` — gọi API: method/url/headers/body; assertion `status`, `json` (path + op),
    `body_contains`, `header`, `duration_max_ms`.
  - `script` — chạy lệnh shell người dùng định nghĩa; assertion `exit_code`,
    `stdout_contains`, `stdout_matches` (regex), `stderr_contains`.
  - `web` — điều khiển app **Mini Browser** (port 4360) qua MCP HTTP: steps
    `navigate` / `act` (mô tả tiếng tự nhiên, tự kiểm chứng) / `wait`; assertion
    `text_contains`, `url_contains` trên trang cuối.
- **Environment + biến `{{var}}`** — thay trong toàn bộ config/assertions/extract
  (thay sau parse, trên từng string — không bao giờ phá JSON).
- **Extract nối chuỗi biến** — case login trích `token` từ response (json path /
  header / regex) → case sau dùng `{{token}}` trong cùng lần chạy suite.
- **Chạy tay / theo lịch** — scheduler nền quét mỗi 30s, interval theo phút từng suite.
- **Lịch sử + báo cáo** — log request/response/stdout đầy đủ, kết quả từng assertion
  (desc/pass/actual/expected), xu hướng pass, **phát hiện flaky** (≥2 lần đổi trạng
  thái pass↔fail trong 10 kết quả gần nhất), case fail nhiều nhất 30 ngày.
- **AI qua bridge SenClaw** — `autotest_ai_generate` sinh test case từ mô tả/OpenAPI/
  curl (validate + normalize từng case trước khi ghi); `autotest_ai_diagnose` chẩn đoán
  run fail, phân biệt lỗi sản phẩm vs lỗi test.

## Chạy dev

```bash
cargo run -p autotest                      # backend :4640 (serve web/dist nếu đã build)
cd apps/autotest/web && npm run dev        # UI dev :5173, proxy /api → :4640
cargo test -p autotest                     # 37 unit tests
```

Env: `PORT` (mặc định 4640), `SENCLAW_DATA_DIR` (đổi chỗ chứa DB),
`AUTOTEST_BROWSER_URL` (Mini Browser, mặc định `http://127.0.0.1:4360` — đổi được
trong tab Lịch chạy → Cài đặt).

## Đóng gói

```bash
apps/autotest/scripts/pack.sh              # → apps/autotest/autotest-app.zip
```

## Kiến trúc

```
src/
  main.rs    — axum server + serve web_dist
  db.rs      — SQLite: suites/cases/environments/runs/results/schedules/activity
  tmpl.rs    — {{var}} substitution + json path (dot notation)
  assert.rs  — engine đánh giá assertion (3 loại case)
  runner.rs  — thực thi http (reqwest) / script (sh -c, kill_on_drop) / web
               (Mini Browser MCP), timeout per-case, cancel per-run, extract biến
  sched.rs   — vòng lặp lịch chạy định kỳ (30s tick, tuần tự)
  llm.rs     — AI generate (parse + repair mảng JSON, chặn finish=length) + diagnose
  api.rs     — REST; mọi handler qua *_value helpers
  mcp.rs     — MCP HTTP+SSE, 21 tools autotest_*, dùng chung *_value với REST
web/         — React 19 + Vite + AntD 6 (dark), tabs: Tổng quan / Bộ kiểm thử /
               Lịch sử chạy / Môi trường / Lịch chạy / Hoạt động
```

Ghi chú an toàn: app chỉ chạy test do người dùng tự định nghĩa trên máy của họ; không
có tool nào tự ý gọi ra ngoài trừ API đích mà test nhắm tới, bridge LLM của daemon và
Mini Browser local.
