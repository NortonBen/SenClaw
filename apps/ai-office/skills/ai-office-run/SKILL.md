---
name: ai-office-run
description: >-
  Giao một nhiệm vụ cho văn phòng AI Office ("công ty một người") và theo dõi đến khi có
  báo cáo tổng hợp. Dùng khi Sếp muốn cả phòng agent xử lý một đầu việc — ví dụ "giao cho
  văn phòng lập kế hoạch marketing", "cho phòng làm việc xử lý việc này". KHÔNG dùng để hỏi
  tình hình hay lấy lại báo cáo cũ — dùng ai-office-status.
---

# ai-office-run

## Khi nào dùng
Sếp muốn văn phòng AI (Trưởng phòng + Nghiên cứu + Nội dung + Phân tích + Kiểm định) xử lý một nhiệm vụ trọn gói và nộp báo cáo tổng hợp.

## Các bước
1. Gọi `mcp__ai-office-mcp__office_status` — nếu đang có nhiệm vụ chạy (status khác `done`/`error`), báo Sếp chờ hoặc hỏi có muốn xem tiến độ không.
2. Gọi `mcp__ai-office-mcp__office_create_task` với `title`: nhiệm vụ nguyên văn của Sếp (giữ tiếng Việt).
3. Chờ và theo dõi bằng `mcp__ai-office-mcp__office_get_task` (id vừa trả về) — trạng thái đi qua `planning → running → review → done`. Mỗi bước mất vài chục giây; đừng poll dày hơn ~10 giây.
4. Khi `done`, lấy báo cáo bằng `mcp__ai-office-mcp__office_get_report` và trình bày lại cho Sếp, kèm nhận xét của Kiểm định nếu có rủi ro.

## Không làm
- Không tự bịa báo cáo khi nhiệm vụ chưa `done`.
- Không giao nhiệm vụ mới khi phòng đang bận — tool sẽ từ chối.
