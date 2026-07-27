---
name: social-manage
description: >-
  Kết nối và kiểm tra các tài khoản mạng xã hội trong Social (Facebook, X,
  Threads, Instagram, TikTok), và xem/đổi chế độ tự chủ (observe/draft/live).
  Dùng khi Sếp muốn "kết nối facebook/tiktok/threads/…", "khai báo tài khoản",
  "xem trạng thái social", "extension đã kết nối chưa", "liệt kê tài khoản đã
  kết nối", "bật chế độ nháp/live", hoặc nhập cấu hình API chính thức. KHÔNG
  dùng để đăng bài, tìm kiếm, duyệt feed hay nhắn tin — dùng social-engage.
---

# social-manage

## Khi nào dùng

Sếp muốn thiết lập/kiểm tra kết nối: khai báo tài khoản, dán cấu hình API chính
thức, xem extension và các phiên đăng nhập, hoặc đổi chế độ tự chủ.

## Công cụ (MCP `social-mcp`)

- `social_status` — **gọi trước tiên**: đếm tài khoản, extension đã kết nối chưa,
  host nào đang có phiên đăng nhập.
- `social_ext_status` — chi tiết extension (uptime, hosts_ready) để chẩn đoán.
- `social_accounts` — liệt kê tài khoản.
- `social_connect` — khai báo/cập nhật tài khoản + `official_config`.
- `social_autonomy` — xem/đổi chế độ: `observe` (chỉ đọc) · `draft` (mặc định,
  mọi bài/tin thành nháp chờ duyệt) · `live` (gửi ngay).

## `official_config` cần gì (theo nền tảng)

| Nền tảng | Khoá cần | Ghi chú |
|---|---|---|
| facebook | `page_id`, `access_token` | Page access token; đăng Page + `social_page_scan`. Profile cá nhân KHÔNG cần config (đi extension). `graph_version` tuỳ chọn (mặc định v23.0) |
| x | `access_token` | OAuth2 user-context, tier trả phí |
| threads | `threads_user_id`, `access_token` | token đúc từ IG liên kết |
| instagram | `ig_user_id`, `access_token` | IG Business/Creator; cần media |
| tiktok | `access_token` | scope video.publish, app phải được duyệt; cần media |

## Quy tắc

- **Đăng bài đi API chính thức**; tìm kiếm/duyệt/nhắn tin đi extension. Nếu
  extension chưa kết nối, nói Sếp mở Chrome đã cài extension và đăng nhập nền
  tảng — đừng hứa chạy được.
- **Không lưu token phiên web** — token do extension giữ, app chỉ biết "có phiên".
- **Không tự ý đổi sang `live`.** Chỉ đổi khi Sếp yêu cầu rõ, và nói trước rằng
  chế độ đó bỏ bước duyệt tay.
- Trung thực về giới hạn: TikTok không có DM bên thứ ba; Threads không có DM;
  "hội nhóm" chỉ có ở Facebook; không có cách nào bảo đảm 100% không bị chặn.
