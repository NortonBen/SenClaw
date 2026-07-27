---
name: youtube-browse
description: Tìm kiếm, duyệt kênh/community và soạn bình luận YouTube qua phiên đăng nhập thật (Chrome extension). Dùng khi người dùng muốn tìm video, xem community post, hoặc soạn/gửi bình luận trên YouTube.
---

# YouTube (SenClaw)

Kết nối SenClaw với YouTube **qua phiên đăng nhập thật của người dùng** bằng một Chrome extension. Không dùng YouTube Data API chính thức (quá hẹp: ~100 lần tìm/ngày, không có API community post/nhắn tin) — thay vào đó proxy **InnerTube** trong browser đã đăng nhập để vừa có đủ quyền vừa vượt được BotGuard/PoToken.

## Điều kiện tiên quyết

Mọi thao tác đọc/ghi **chỉ hoạt động khi Chrome extension kết nối**. Luôn gọi `youtube_status` TRƯỚC:

- `extensionConnected: false` → bảo người dùng cài extension `apps/youtube/extension`, mở youtube.com đã đăng nhập, và đặt đúng WS port (9223) + HTTP port của app trong popup extension.
- `auth.hasSapisid: false` → extension đã kết nối nhưng chưa thấy phiên đăng nhập; bảo người dùng đăng nhập YouTube trong Chrome đó.

## Đọc (an toàn, không cần duyệt)

- `youtube_search { query }` → danh sách video `{ videoId, title, channel, published, views }`.
- `youtube_browse { browse_id, params? }` → duyệt kênh (`UC…`) hoặc feed; `params` chọn tab (vd tab Community). Trả `{ videos, posts }`.
- `youtube_list_comments { video_id | continuation }` → bình luận `{ commentId, author, text, replyParams }`. Dùng `replyParams` làm `target` khi trả lời.

## Ghi (bắt buộc draft → duyệt → gửi)

Đây là guardrail human-in-the-loop — **không bao giờ gửi thẳng**:

1. `youtube_draft_comment { kind, context, target?, instruction? }` → AI soạn nội dung và lưu thành **draft** (kind: `comment` | `reply` | `community_post`). Trả `id` + `body`.
   - `comment`: `target` = **videoId**.
   - `reply`: `target` = **replyParams** (lấy từ `youtube_list_comments`).
2. `youtube_list_drafts { status? }` → xem các draft (draft/approved/sent/failed).
3. `youtube_approve_draft { id }` → người dùng xác nhận (bước duyệt bắt buộc).
4. `youtube_send_draft { id }` → gửi qua InnerTube (`comment/create_comment`, `comment/create_comment_reply`). Chỉ nhận draft đã **approved**.

> `community_post`: không có endpoint InnerTube ổn định → `send` sẽ **tự lái giao diện composer** (mở Studio, gõ, bấm Đăng) bằng input trusted. `target` = channel id hoặc URL composer (bỏ trống = studio.youtube.com).
> Đường ghi đã nối và test bằng harness giả lập; lần gửi thật đầu tiên cần một phiên YouTube đăng nhập để xác nhận token (`createCommentParams`/`createReplyParams`) khớp shape hiện tại.

## Phân tích & thống kê bình luận

Bình luận lấy live không đủ để phân tích — phải **cache** trước:

1. `youtube_sync_comments { video_id, max_pages? }` → kéo + cache (phân trang). Chạy trước mọi phân tích.
2. `youtube_analyze_comments { max? }` → LLM chấm sentiment/intent/topic/spam/lang cho các comment chưa phân tích.
3. `youtube_comment_stats { video_id }` → tổng hợp: sentiment/ý định/ngôn ngữ, top người bình luận, spam, cảm xúc TB.
4. `youtube_scan_keywords { keywords[], video_id? }` → tìm comment chứa từ khoá (nguồn cho cảnh báo).
5. `youtube_cached_comments { video_id }` → đọc comment đã cache (kèm nhãn phân tích).

> YouTube Analytics API **không** có sentiment/metric per-comment — mọi phân tích tính tại app bằng LLM. Nhãn cảm xúc là **gợi ý**, hiển thị kèm ngữ cảnh, đừng để nó tự quyết xoá/ẩn.

## Hành động trên bình luận

- `youtube_comment_action { comment_id, action: heart|like }` — dùng token bắt lúc sync (chạy `youtube_sync_comments` gần thời điểm hành động để token còn hạn). remove/report chưa mở.

## Kết nối nền tảng

- **CRM**: bình luận đã sync tự chảy vào CRM inbox nếu operator thêm một channel kind `social` trỏ về app (pull-feed `/api/inbox`). Câu hỏi/khiếu nại thành hội thoại CRM.
- **Knowledge**: `youtube_index_comments { video_id }` lưu bình luận vào bộ nhớ để recall khi soạn trả lời.

## Remote-control giao diện (khi không có API)

Extension lái trang bằng **CDP input trusted** (`chrome.debugger`) — giống thao tác người thật, khác với event tổng hợp mà YouTube bỏ qua.

- `youtube_ui_open { url }` → mở/focus tab YouTube hoặc Studio.
- `youtube_ui_snapshot` → liệt kê element tương tác được, đánh số `idx`.
- `youtube_ui_act { action, index?, text?, key? }` → `click` / `type` (click rồi gõ) / `press`.

**Luôn snapshot lại trước mỗi act** — `idx` chỉ đúng với snapshot mới nhất. Dùng bộ này khi `youtube_send_draft` cho community post báo không tìm thấy ô soạn/nút Đăng, hoặc khi cần thao tác bất kỳ mà InnerTube không có API.

> Lưu ý UX: khi lái giao diện, Chrome hiện thanh "đang được gỡ lỗi" — bình thường; gọi xong có thể để hệ thống tự nhả.

## Nguyên tắc

- Luôn `youtube_status` trước; nếu chưa kết nối/đăng nhập thì hướng dẫn, đừng thử đọc.
- Bình luận phải tự nhiên, đúng ngữ cảnh, **không spam**, không rải link, không @-mention hàng loạt.
- Tôn trọng ToS YouTube: dùng tài khoản phụ, tần suất thấp, luôn để người dùng duyệt trước khi gửi.
- "Nhắn tin" trên YouTube ≈ bình luận / trả lời (DM không có API); đừng hứa gửi tin nhắn riêng.
