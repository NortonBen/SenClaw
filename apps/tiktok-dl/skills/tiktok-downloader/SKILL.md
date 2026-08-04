---
name: tiktok-downloader
description: >-
  Tải nội dung TikTok về máy qua app TikTok Downloader: video không logo / HD /
  bản gốc có logo, tách nhạc MP3, trọn bộ ảnh của post photo-mode, avatar tác
  giả; tải một link, tải hàng loạt từ danh sách link, hoặc tải N video mới nhất
  của một trang cá nhân (best-effort). Theo dõi hàng đợi tải, hủy/tải lại, tìm
  lại lịch sử đã tải (gõ không dấu vẫn khớp), đổi cài đặt thư mục lưu / chất
  lượng mặc định / mẫu tên file. Dùng khi người dùng gửi link TikTok nhờ tải,
  hỏi "tải xong chưa", muốn tìm video đã tải, hoặc chỉnh cách lưu file.
triggers:
  - tải tiktok
  - tai tiktok
  - tải video tiktok
  - download tiktok
  - tiktok downloader
  - tải video không logo
  - không watermark
  - no watermark
  - tách nhạc tiktok
  - tải nhạc tiktok
  - tải bộ ảnh tiktok
  - photo mode
  - tải cả kênh tiktok
  - tải trang cá nhân tiktok
  - vm.tiktok.com
  - vt.tiktok.com
  - douyin
  - lịch sử tải
  - tải hàng loạt
---

# tiktok-downloader

Dùng MCP server `tiktok-dl-mcp` của app **TikTok Downloader**. App tải post
TikTok **công khai** về máy đang chạy SenClaw (thư mục trong cài đặt, mặc định
`~/Downloads/TikTok`). Không cần đăng nhập TikTok; file chỉ nằm trên máy này —
không có tool nào đăng hay gửi nội dung đi đâu.

## Nguyên tắc bắt buộc

- **Tải là async.** `tdl_download`/`tdl_download_batch` chỉ XẾP HÀNG rồi trả về
  ngay kèm `id`. Muốn biết xong chưa: `tdl_queue` (đang chạy/đang chờ) hoặc
  `tdl_history_get` với id đó. Đừng báo "đã tải xong" khi status chưa `done`.
- **Link lẫn trong chữ vẫn được.** Cứ đưa nguyên câu người dùng gửi vào `url` /
  `text` — app tự lọc link tiktok.com / vm.tiktok.com / vt.tiktok.com /
  douyin.com, bỏ link lạ.
- **Trùng thì app tự bỏ qua.** Link đã tải xong cùng chất lượng sẽ bị skip
  (trả `duplicate: true`). Người dùng muốn tải lại thật sự → `force: true`
  hoặc `tdl_retry` với id cũ.
- **Chỉ nội dung công khai, dùng cá nhân.** Post riêng tư / bị xoá sẽ lỗi
  "link không phân giải được" — nói thẳng, đừng thử lách. Người dùng nhờ
  đăng lại video của người khác lên nền tảng khác → nhắc tôn trọng bản quyền.

## Chọn công cụ

- **`mcp__tiktok-dl-mcp__tdl_download`** — có link, muốn tải: `quality` =
  `nowm` (không logo, mặc định) | `hd` | `wm` (bản gốc có logo) | `audio`
  (tách nhạc MP3). Post ảnh tự tải trọn bộ ảnh bất kể quality.
- **`tdl_download_batch`** — nhiều link một lúc (đến 200): đưa nguyên đoạn
  text chứa các link vào `text`.
- **`tdl_resolve`** — người dùng chỉ muốn XEM thông tin (caption, tác giả,
  lượt xem/tim, thời lượng, dung lượng từng bản) mà chưa tải.
- **`tdl_profile_feed`** / **`tdl_profile_download`** — liệt kê / tải N video
  mới nhất của một tài khoản. Nguồn dữ liệu profile hay bị Cloudflare chặn
  hơn link lẻ — lỗi lặp lại thì khuyên người dùng dán link video cụ thể và
  dùng `tdl_download_batch` (đừng thử đi thử lại quá 2 lần).
- **`tdl_avatar`** — tải ảnh đại diện tác giả (cần một link post bất kỳ của
  người đó).
- **`tdl_queue`** — "tải xong chưa?": job đang chạy kèm % và đang chờ.
- **`tdl_cancel`** / **`tdl_retry`** — hủy / tải lại theo `download_id`.
- **`tdl_history`** — tìm trong lịch sử: `q` (không dấu vẫn khớp), lọc
  `status` / `kind`. **`tdl_history_get`** — chi tiết + đường dẫn file.
- **`tdl_open`** — mở thư mục chứa file trong Finder (`reveal: true` chọn
  thẳng file).
- **`tdl_history_delete`** / **`tdl_history_clear`** — dọn lịch sử. `with_file(s)`
  xoá cả file trên đĩa — KHÔNG hoàn tác được, chỉ dùng khi người dùng nói rõ.
- **`tdl_settings_get`** / **`tdl_settings_set`** — thư mục lưu, chất lượng mặc
  định, mẫu tên file (`{author}` `{id}` `{title}` `{date}` `{quality}`), số tải
  đồng thời (1-4), tải nhạc kèm post ảnh, ghi metadata JSON, trần video profile.

## Mẫu tình huống

- "Tải giúp em cái này https://vm.tiktok.com/ZS…" → `tdl_download` với nguyên
  câu, quality mặc định → báo đã xếp hàng kèm id → `tdl_queue` khi được hỏi.
- "Tải hết đống link này, lấy bản HD" → `tdl_download_batch` `{text: "...",
  quality: "hd"}` → báo số link xếp / bỏ qua.
- "Lấy nhạc bài này thôi" → `tdl_download` `{quality: "audio"}`.
- "Hôm trước tải video nấu ăn nào ấy nhỉ" → `tdl_history` `{q: "nau an"}`.
- "Tải 20 video mới nhất của @abc" → `tdl_profile_download` `{unique_id:
  "@abc", max: 20}`; bị chặn thì giải thích + gợi ý dán link.
