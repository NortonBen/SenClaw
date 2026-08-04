# TikTok Downloader — SenClaw Space App

Tải nội dung TikTok **công khai** về máy, port từ ý tưởng
[Jettcodey/TikTok-Downloader](https://github.com/Jettcodey/TikTok-Downloader)
(C#/Playwright, Windows) sang Rust + React, không cần trình duyệt tự động —
link được phân giải qua endpoint công khai tikwm.com (cùng nguồn các desktop
downloader dùng cho bản HD / không logo).

## Tính năng

- **Video một link**: bản không logo (mặc định), HD, bản gốc có logo, hoặc chỉ
  tách **nhạc MP3**. Nhận link đầy đủ, link rút gọn `vm.tiktok.com` /
  `vt.tiktok.com`, cả link dán lẫn trong chữ; douyin.com cũng nhận.
- **Post ảnh (photo mode)**: tải trọn bộ ảnh vào một thư mục, kèm nhạc nền
  (tắt được trong cài đặt).
- **Tải hàng loạt**: dán danh sách link (đến 200/lần) — tự lọc link hợp lệ, tự
  bỏ link đã tải xong cùng chất lượng.
- **Trang cá nhân** *(best-effort)*: liệt kê / tải N video mới nhất của một
  tài khoản. Endpoint profile của tikwm nằm sau Cloudflare gắt hơn endpoint
  link lẻ nên có lúc bị chặn — UI/MCP đều báo rõ và gợi ý dán link thủ công
  (app gốc cũng phải mở trình duyệt thật cho việc này).
- **Avatar tác giả**: tải từ một link post bất kỳ của người đó.
- **Hàng đợi nền**: 1–4 job song song, tiến trình % theo bytes, hủy / tải lại
  từng job; job dở dang tự chạy tiếp khi app khởi động lại; link CDN hết hạn
  luôn được phân giải mới trước khi tải.
- **Lịch sử**: FTS5 tìm không dấu (`nau an` khớp "nấu ăn", có xử lý riêng
  `đ→d`), thumbnail, mở Finder, tải file qua trình duyệt, xoá bản ghi ± file.
- **Cài đặt**: thư mục lưu (mặc định `~/Downloads/TikTok`), chất lượng mặc
  định, mẫu tên file `{author}` `{id}` `{title}` `{date}` `{quality}`, số job
  song song, metadata JSON cạnh file.
- **Giao diện sáng / tối / theo hệ thống** (antd 6, lưu lựa chọn).

## Chạy dev

```bash
cargo run -p tiktok-dl                # backend :4670
cd apps/tiktok-dl/web && npm run dev  # UI dev :5173 (proxy /api → 4670)
```

Đóng gói zip cài vào SenClaw: `apps/tiktok-dl/scripts/pack.sh`.

## MCP

Server `tiktok-dl-mcp` (SSE `/api/mcp/sse`) — 18 tool prefix `tdl_`
(`mcp__tiktok-dl-mcp__tdl_download`, `tdl_history`, …). Danh sách + mô tả đầy
đủ trong `src/mcp.rs`; skill hướng dẫn agent ở
`skills/tiktok-downloader/SKILL.md`.

## Ghi chú pháp lý

Chỉ tải post công khai, phục vụ lưu trữ cá nhân. Tôn trọng bản quyền và quyền
riêng tư của tác giả; không dùng app để đăng lại nội dung của người khác.
