---
name: facebook-manage
description: Kết nối Fanpage Facebook qua Developer App (OAuth/Graph API), chọn Trang, đặt chế độ tự chủ, và quản lý trigger auto-reply. Dùng khi người dùng muốn kết nối Facebook, xem trạng thái, hoặc cấu hình luật trả lời.
---

# Facebook Pro — kết nối & cấu hình (draft-first)

App Facebook Pro chạy trên `http://127.0.0.1:4590`, dùng **Facebook Graph API
CHÍNH THỨC** qua Developer App của người dùng. KHÔNG scraping, KHÔNG trộm
cookie/session, KHÔNG né anti-bot, KHÔNG đăng/nhắn hàng loạt.

MCP server: `facebook-mcp` (công cụ `mcp__facebook-mcp__fb_*`).

## Kết nối (một lần)

1. Người dùng tạo **Facebook Developer App** (Business) tại
   <https://developers.facebook.com/apps>, thêm sản phẩm *Facebook Login*, và
   whitelist redirect `http://127.0.0.1:4590/api/oauth/callback`. Lấy **App ID** +
   **App Secret** → nhập ở tab **Kết nối** (hoặc `fb_status` để kiểm tra).
2. Cấp quyền theo một trong hai cách:
   - **OAuth**: `fb_connect_link {redirect}` → mở URL cho admin **tự bấm Đồng ý**.
     Facebook redirect về `/api/oauth/callback` → app đổi token + lấy Trang.
   - **Token**: `fb_connect_token {user_token}` — dán User Access Token từ Graph
     API Explorer; app đổi sang token dài hạn (~60 ngày) và lấy Trang + Page token.
3. `fb_pages` để xem Trang, `fb_select_page {page_id}` để chọn Trang active.

Scopes: `pages_show_list, pages_manage_posts, pages_read_engagement,
pages_manage_engagement, pages_read_user_content, read_insights`.

## Chế độ tự chủ

`fb_autonomy_set {mode}` — `observe` (chỉ đọc) · `draft` (soạn nháp, mặc định) ·
`live` (tự đăng). Khuyến nghị **draft**.

## Trigger auto-reply (bình luận mới → luồng)

- `fb_trigger_create {name, match_type, match_value, action, reply_hint, page_id?}`
  - `match_type`: `all` | `keyword` (dùng `match_value` CSV) | `question`.
  - `action`: `draft_reply` (AI soạn nháp trả lời) | `notify` (ghi thông báo).
  - `page_id` bỏ trống = áp cho mọi Trang.
- `fb_triggers` liệt kê, `fb_trigger_delete {id}` xoá.
- `fb_tick` chạy một nhịp quét bình luận ngay (tôn trọng autonomy).

## Nguyên tắc

- Chỉ tác động lên **Trang người dùng quản trị**. Không có API đăng/nhắn hàng loạt.
- App Secret + token chỉ lưu cục bộ, chỉ gửi tới `graph.facebook.com`.
