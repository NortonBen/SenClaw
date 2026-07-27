---
name: facebook-engage
description: Đăng bài (chữ/link/ảnh), sửa/xoá bài, đọc & trả lời bình luận, like, phân tích bài viết bằng AI, và xem Insights Fanpage Facebook — draft-first, chỉ đăng khi được duyệt. Dùng khi người dùng muốn đăng/trả lời/phân tích trên Facebook.
---

# Facebook Pro — đăng bài & tương tác (draft-first)

App Facebook Pro trên `http://127.0.0.1:4590`, MCP `facebook-mcp`. Mọi thao tác
tạo nội dung công khai (đăng bài, bình luận, trả lời, sửa bài) đi qua **hàng chờ
duyệt** — chỉ đăng khi con người bấm Duyệt (hoặc autonomy = live).

Trước tiên cần đã kết nối + chọn Trang (xem skill `facebook-manage`). Không truyền
`page_id` thì mặc định dùng Trang đang chọn.

## Đăng bài

- `fb_post_create {message, link?, image_url?}` — SOẠN bài. Có `image_url` (URL ảnh
  công khai) → đăng **bài ảnh**; có `link` → kèm link. Trả về `draft_id`.
- `fb_post_edit {post_id, message}` — SOẠN sửa nội dung bài.
- `fb_post_delete {post_id}` — xoá bài (tức thời, thao tác rõ ràng).
- `fb_posts {limit?}` — liệt kê bài gần đây; `fb_post_get {post_id}` — chi tiết.

## Bình luận & tương tác

- `fb_comments {object_id}` — đọc bình luận của một bài (hoặc reply của một cmt).
- `fb_comment_reply {comment_id, message?|comment_text?, hint?}` — SOẠN trả lời
  một bình luận. Bỏ trống `message` thì AI tự soạn từ `comment_text` + `hint`.
- `fb_comment_create {object_id, message}` — SOẠN một bình luận lên bài.
- `fb_like {object_id}` — thả like (tức thời).

## Tin nhắn (Inbox) & tổng quan

Cần quyền `pages_messaging` để đọc/gửi tin nhắn.

- `fb_overview` — tổng quan tương tác Trang (tổng reactions/comments/shares các bài
  gần đây, top bài, số nháp chờ) → thống kê nhanh.
- `fb_conversations {page_id?}` — danh sách hội thoại Messenger của Trang.
- `fb_conversation_messages {conversation_id}` — các tin trong một thread.
- `fb_message_reply {recipient_id, message?|customer_msg?, hint?}` — SOẠN trả lời
  tin nhắn (draft-first, Send API dạng RESPONSE — không broadcast). Bỏ trống
  `message` thì AI tự soạn. `recipient_id` là PSID người dùng (lấy từ participants).

## Duyệt & đăng

- `fb_drafts` — xem nháp chờ duyệt.
- `fb_approve {draft_id}` — **cổng duy nhất** thực sự gọi Graph API để đăng/trả lời.
- `fb_reject {draft_id}` — bỏ nháp.

## Phân tích & thống kê

- `fb_analyze {post_id? | message?}` — AI phân tích bài (điểm mạnh/yếu, gợi ý,
  mức tương tác). Có `post_id` → lấy nội dung + tương tác thật.
- `fb_page_insights {metric?, period?}` — thống kê cấp Trang (Insights API).
- `fb_post_insights {post_id, metric?}` — thống kê cấp bài.

## Quảng cáo — chỉ số & đánh giá (Marketing API)

Cần user token có quyền `ads_read` (và `ads_management` để tắt/bật QC).

- `fb_ad_accounts` — liệt kê tài khoản QC; `fb_ad_select_account {account_id}` chọn tài khoản active.
- `fb_ad_campaigns {account_id?}` — danh sách chiến dịch (id/tên/trạng thái/ngân sách).
- `fb_ads_insights {object_id?, level?, date_preset?}` — chỉ số **CTR, CPC, CPM**,
  chi tiêu, reach, kết quả, ROAS. `level`: account|campaign|adset|ad;
  `date_preset`: last_7d|last_30d|today|maximum.
- `fb_ads_analyze {object_id?, level?, date_preset?, currency?}` — AI đọc số liệu
  thật và kết luận từng chiến dịch: **HIỆU QUẢ ✅ / THEO DÕI ⚠️ / ĐỐT TIỀN ❌**,
  giải thích (CTR quá thấp? CPC/CPM cao? chi nhiều mà 0 kết quả / ROAS<1?) và có
  **nên tắt** không. Dùng khi người dùng hỏi "quảng cáo này oke không / có đốt tiền
  không / có nên tắt không".
- `fb_ad_status {entity_id, status}` — **TẮT** (PAUSED) hoặc **BẬT** (ACTIVE) một
  chiến dịch/nhóm QC/quảng cáo. Thao tác tức thời trên tài khoản của người dùng —
  chỉ chạy khi được yêu cầu rõ ràng (vd sau khi phân tích thấy đang đốt tiền).

Diễn giải nhanh: CTR cao = nội dung hút; CPC/CPM thấp = rẻ; spend cao mà kết quả≈0
hoặc ROAS<1 = đang lỗ, cân nhắc tắt.

## Nguyên tắc

- Trình bản nháp cho người dùng trước; chỉ `fb_approve` khi được đồng ý.
- Dựa trên dữ liệu thật; không bịa số liệu/khuyến mãi/chính sách.
- Chỉ tác động Trang người dùng quản trị; không đăng/nhắn hàng loạt.
