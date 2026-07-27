---
name: media-download
description: Tải URL remote → local media + cập nhật DB — agent xử lý (không gọi LLM); soul ghi chú hành vi pipeline
---

MediaDownloadAgent quét scenes và characters của video/project, tải mọi ảnh/video từ URL remote về thư mục media local, cập nhật bản ghi với đường dẫn/phục vụ local và giữ URL gốc khi cần.

Thứ tự pipeline:
- Chạy **sau** khi đã có clip/ảnh trên URL (thường sau **video**).
- Phải **trước concat** nếu concat đọc file local từ đường dẫn đã ghi.

Soul này không thay thế logic Go; dùng để tài liệu hóa và mở rộng sau (logging, policy). Agent runtime chủ yếu là HTTP download + ghi file + SQL patch.
