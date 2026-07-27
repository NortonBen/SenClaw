# SenClaw YouTube (apps/youtube)

Kết nối SenClaw với YouTube **qua phiên đăng nhập thật** của người dùng bằng một Chrome extension: tìm kiếm, duyệt kênh/community, và soạn bình luận theo pipeline **draft → duyệt → gửi**.

> Trạng thái: **Phase 1–11 code-complete** (27 unit/integration test pass, 20 MCP tool). ĐỌC + GHI (comment/reply/community-post) + remote-control giao diện + **cache/phân tích LLM/thống kê bình luận** + **kết nối CRM (pull-feed)/knowledge/cảnh báo từ khoá** đều đã nối và verify bằng harness giả lập extension + fixtures. Còn lại duy nhất: chạy thật với một phiên YouTube đăng nhập để xác nhận token/selector khớp. Thiết kế: [`docs/youtube-app-research.md`](../../docs/youtube-app-research.md) + [`docs/youtube-comments-tools-research.md`](../../docs/youtube-comments-tools-research.md).

## Vì sao cần extension

YouTube Data API v3 quá hẹp (search ~100 lần/ngày, không có API tạo community post / nhắn tin). Đường thực tế là **InnerTube** (`youtubei/v1/*`) — nhưng nó đòi request phát ra từ **browser thật đã đăng nhập** (BotGuard/PoToken). Vì vậy app không tự gọi API; nó **proxy qua extension**, nơi request được ký `SAPISIDHASH` + gửi kèm cookie phiên, same-origin.

## Kiến trúc

- `src/main.rs` — axum HTTP API (port `4491`) + serve web; spawn extension-bridge WS (port `9223`).
- `src/extbridge.rs` — WS server + `{id,method,params}` RPC tới extension (reply qua WS hoặc `POST /api/ext/callback`).
- `src/innertube.rs` — dựng payload `search`/`browse`/`next` + parse nhẹ.
- `src/youtube.rs` — proxy InnerTube qua bridge + pipeline draft.
- `src/mcp.rs` — MCP JSON-RPC (`mcp__youtube-mcp__youtube_*`).
- `src/llm.rs` — LLM qua app-space-sdk (không gọi provider trực tiếp).
- `extension/` — Chrome MV3: bắt trạng thái đăng nhập (cookie `SAPISID`), ký `SAPISIDHASH`, proxy `yt_fetch`.
- `web/` — React + **Ant Design**, multi-page (Sider menu): `pages/SearchPage` · `DashboardPage` (thống kê + bảng bình luận phân trang + hành động) · `DraftsPage` · `SettingsPage` (kết nối, LLM model, **đăng nhập Google/OAuth**). Bảng dùng AntD `Table` phân trang. Header có **toggle dark/light** (lưu localStorage) + trạng thái **đăng nhập Google** (avatar kênh / nút đăng nhập).

## Chạy dev

```bash
# 1. backend
cargo run -p youtube          # http://127.0.0.1:4491, WS bridge :9223

# 2. web (terminal khác)
cd apps/youtube/web && npm install && npm run dev

# 3. extension: chrome://extensions → Load unpacked → apps/youtube/extension
#    mở youtube.com đã đăng nhập; popup extension đặt WS 9223 + HTTP 4491
```

## Đóng gói cài vào SenClaw

```bash
apps/youtube/scripts/pack.sh   # → apps/youtube/youtube-app.zip
```

## MCP tools

**Đọc**: `youtube_status` · `youtube_search` · `youtube_browse` · `youtube_list_comments`
**Cache + phân tích bình luận**: `youtube_sync_comments` · `youtube_cached_comments` · `youtube_analyze_comments` · `youtube_comment_stats` · `youtube_scan_keywords` · `youtube_index_comments`
**Hành động bình luận**: `youtube_comment_action` (heart/like/pin/remove/report — InnerTube)
**Moderation (OAuth Data API)**: `youtube_oauth_status` · `youtube_moderate` (heldForReview/rejected/banAuthor)
**Remote-control UI**: `youtube_ui_open` · `youtube_ui_snapshot` · `youtube_ui_act`
**Ghi (draft-first)**: `youtube_draft_comment` · `youtube_list_drafts` · `youtube_approve_draft` · `youtube_send_draft`

### Kết nối nền tảng

- **CRM inbox (PULL)**: app expose `GET /api/inbox?since=` + `POST /api/inbox/reply` + `GET /api/status` giống `apps/social`. Thêm một CRM channel kind `social` trỏ `base_url=http://127.0.0.1:4491` → bình luận (đã sync) chảy vào inbox CRM; operator reply → route ngược lại YouTube. Zero sửa CRM.
- **Knowledge**: `youtube_index_comments` lưu bình luận vào knowledge space `youtube-comments` để recall khi soạn trả lời.
- **Cảnh báo từ khoá**: `youtube_scan_keywords` là nguồn dữ liệu; lịch chạy đặt qua daemon `POST /api/background/tasks` (interval + notify).

## Cảnh báo

Tự động hoá InnerTube ngoài API chính thức có rủi ro ToS/khoá tài khoản. Dùng tài khoản phụ, tần suất thấp, và luôn giữ bước duyệt của con người trước khi gửi.
