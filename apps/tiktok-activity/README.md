# TikTok Activity (SenClaw App Space)

Bộ điều phối hoạt động TikTok — port từ backend Go (`playwright-go` + React) sang **Rust (axum + rusqlite) + React/TypeScript**, chạy như một App Space của SenClaw.

Quản lý nhiều account (proxy + browser profile riêng), dựng "flow" kéo-thả (chuỗi action: search / xem / like / comment / share / follow / login…), chạy thủ công hoặc theo lịch. Engine là **graph-walker** điều khiển Chromium thật qua CDP (chromiumoxide) khi bật `TIKTOK_USE_PLAYWRIGHT=1`, có nhánh success/error/alt, vòng lặp, và template `{{param}}`/`{{prev}}`/`{{step.id}}`.

## Chạy

```bash
cargo run -p tiktok-activity            # cần web/dist đã build
cd apps/tiktok-activity/web && npm install && npm run build
```

Biến môi trường chính:

| Env | Ý nghĩa |
|---|---|
| `PORT` | Cổng HTTP (mặc định 4580; daemon tự inject) |
| `TIKTOK_DATA_DIR` | Thư mục data (mặc định `~/.senclaw/space-app-data/tiktok-activity`) |
| `TIKTOK_SQLITE_PATH` | Đường dẫn DB (mặc định `<data>/app.db`, WAL) |
| `TIKTOK_CONTROL_MODE` | `extension` (mặc định — điều khiển 1 tab TikTok qua extension) hoặc `stub` (mô phỏng, không trình duyệt) |
| `TIKTOK_EXT_WS_PORT` | Cổng WS mà extension dial vào (mặc định 9225) |
| `SENCLAW_BASE_URL` | UI server của daemon (bridge LLM) |

## Kiến trúc (map từ bản Go)

| Go | Rust |
|---|---|
| `internal/domain` | `src/domain.rs` |
| `internal/store` (SQLite) | `src/db.rs` |
| `internal/engine/runner.go` (graph walker) | `src/engine/mod.rs` |
| `playwrightexec/run_state.go` (templating) | `src/engine/run_state.rs` |
| `cmd/server/*.go` (HTTP) | `src/api.rs` |
| `schedules.go` (run lifecycle + scheduler) | `src/run_manager.rs` |
| `internal/agent` (LLM) | `src/ai.rs` (qua SenClaw bridge `llm.request`) |
| — MCP | `src/mcp.rs` (`tiktok-mcp`) |
| `web/` (React+Vite+AntD+@xyflow) | `web/` (giữ nguyên, chỉnh proxy port) |

LLM đi qua SenClaw bridge — không có `temperature`, có trần output (`finish=length` bị coi là lỗi).

## Trạng thái port

- ✅ Data model + SQLite store (đầy đủ bảng: accounts/proxies/profiles/flows/runs/schedules/notifications/agent_skills/saved_actions/settings/engine_kv/activity).
- ✅ Engine graph-walker: nhánh, vòng lặp (loop_repeat/loop_if), control-flow (log/notification/set_params/run_next_flow/record_*/account_meta), templating.
- ✅ StubDriver (mặc định như bản Go khi chưa bật Playwright).
- ✅ Toàn bộ HTTP `/api/*` + scheduler + notification rules.
- ✅ AI qua bridge: sinh flow từ catalog, gợi ý bước, nháp profile.
- ✅ MCP `tiktok-mcp` (list/run/status/generate).
- ✅ Frontend React/TS chạy end-to-end trên backend Rust.
- ✅ **ExtensionDriver (1 tài khoản)** — điều khiển một tab TikTok đã đăng nhập qua extension (WS `:9225` + `chrome.debugger`), serialized (Semaphore(1)). Logic ở app; extension chỉ chạy primitive (`eval`/`mouse_click`/`type_text`/`press_key`/`wheel`/`navigate`/`url`). Đã verify RPC round-trip + báo lỗi khi chưa kết nối.
- ✅ **Executor TikTok từng action** — navigate/engage/comment/share/reply/auth_login/check_login/if_condition/check_scroll_end/random_yes_no + atomic kinds (click/click_button_text/fill/press/wait_ms/wait_load/goto/scroll/assert/click_unless_contains) + legacy like/follow/next_video (qua rules JSON import). Chạy trên `PageOps` (không phụ thuộc transport).
- ✅ **AI executors qua bridge** — ai_gent_comment, get_info_post, get_comments_in_page, reply_comment_ai, ai_playwright_agent (tool-loop rút gọn: goto/click_text/fill/done).
- ✅ **Extension MV3** tại `extension/` (load unpacked) — popup chọn tab, tự reconnect, callback fallback.
- ⏳ Còn lại (đã flag): analyze-page (agent-skills / saved-actions) trả 503; AI flow-gen bỏ live browser probe; AI-memory panel disabled (dùng bridge knowledge.save). Bỏ chế độ đa-account/CDP theo yêu cầu refactor (1 tài khoản qua extension).
