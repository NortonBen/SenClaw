---
name: social-engage
description: >-
  Đăng bài, tìm kiếm, duyệt feed/bài viết, duyệt hội nhóm Facebook, đọc và trả
  lời tin nhắn trên các nền tảng đã kết nối trong Social (Facebook, X, Threads,
  Instagram, TikTok). Cũng dùng để xem và duyệt/bỏ các NHÁP chờ gửi. Dùng khi
  Sếp muốn "đăng bài lên …", "đăng bài threads", "tìm kiếm trên …", "duyệt
  feed", "duyệt hội nhóm facebook", "đọc/trả lời tin nhắn khách", "xem nháp chờ
  duyệt", "duyệt nháp". KHÔNG dùng để kết nối tài khoản hay đổi chế độ tự chủ —
  dùng social-manage.
---

# social-engage

## Khi nào dùng

Sếp muốn thao tác thật lên nền tảng: đăng bài, tìm kiếm, duyệt, trả lời tin, hoặc
duyệt các nháp đang chờ.

## Công cụ (MCP `social-mcp`)

- `social_post` — đăng bài. FB Page/X/Threads đi **API chính thức**; FB **profile
  cá nhân** đi bằng **điều khiển DOM** (extension tự mở ô soạn trong tab đăng nhập,
  gõ nội dung, bấm Đăng — như người thật, không dính lỗi token 1357004). Cần có
  tab facebook.com đăng nhập. (Tuỳ chọn `use_api` để thử đường GraphQL trước rồi
  tự lùi về DOM.)
- `social_send_dm` — **chỉ trả lời** tin nhắn có sẵn (không nhắn nguội).
- `social_search` / `social_feed` — tìm kiếm / duyệt qua extension.
- `social_groups` — duyệt hội nhóm **Facebook** (nền tảng khác không có nhóm).
- `social_page_scan` — **quét Page Sếp QUẢN TRỊ** qua Graph API (ổn định, đúng ToS):
  `kind=info` (tên/hạng mục/follower/like/website), `kind=feed` (bài gần đây +
  reaction/comment/share), `kind=insights` (thống kê). Cần `official_config
  {page_id, access_token}`. KHÔNG quét được Page/profile không do Sếp quản trị.
- `social_inbox_poll` / `social_inbox_list` — đọc hộp thư / tin đã lưu.
- `social_drafts` → `social_approve` / `social_reject` — xem và duyệt/bỏ nháp.
- `social_post_log` — kiểm chứng đã đăng thật chưa.

## Quét thông tin Facebook (scan)

- **Page Sếp quản trị → dùng `social_page_scan`** (đường ổn định nhất). Cần đã
  `social_connect facebook` với `official_config {page_id, access_token}` (Page token).
- **Feed/nhóm cá nhân → `social_feed` / `social_groups`** (qua phiên web). Lần đầu
  phải để Sếp **cuộn feed/nhóm 1 lần** cho extension học truy vấn; nếu chưa học,
  tool trả `not_wired` kèm hướng dẫn — chuyển nguyên văn cho Sếp, đừng bịa dữ liệu.
- Đường phiên cá nhân mong manh (Meta đổi cấu trúc) — nếu trả rỗng thì báo thật.

## Luồng bắt buộc: NHÁP TRƯỚC, GỬI SAU

App mặc định ở chế độ `draft`. Vì vậy:

1. Gọi `social_post` / `social_send_dm` → **kết quả trả về là một NHÁP**
   (`drafted: true, draft_id`), **chưa hề gửi đi**.
2. Báo Sếp nội dung nháp + `draft_id`, **hỏi Sếp có duyệt không**.
3. Chỉ khi Sếp đồng ý mới gọi `social_approve(draft_id)`. Nếu Sếp không ưng →
   `social_reject(draft_id)`.

**Tuyệt đối không tự gọi `social_approve` ngay sau khi tạo nháp** — như vậy là
vô hiệu hoá lớp bảo vệ. Cũng không tự đổi sang chế độ `live` để né bước duyệt.

## Quy tắc khác

- **Gọi `social_status` trước.** Extension chưa kết nối thì các thao tác web sẽ lỗi.
- **Tôn trọng bộ điều tiết nhịp.** Bị `blocked` vì chạm hạn mức ngày thì báo Sếp,
  đừng lách.
- **DM chỉ phản hồi**, không nhắn nguội/hàng loạt — dễ bị gắn cờ spam nhất.
- **Không hứa "chắc chắn không bị chặn".**
- Nếu `social_approve` lỗi (vd thiếu token), nháp **vẫn giữ pending** kèm lý do —
  chuyển nguyên văn lý do cho Sếp, đừng bịa là đã đăng.
