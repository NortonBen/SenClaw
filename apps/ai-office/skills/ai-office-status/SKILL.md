---
name: ai-office-status
description: >-
  Xem tình hình văn phòng AI Office: agent nào đang làm gì, nhiệm vụ đang chạy tới đâu,
  và lấy báo cáo tổng hợp đã nộp. Dùng khi Sếp hỏi "văn phòng đang làm gì", "xong chưa",
  "cho xem báo cáo". KHÔNG dùng để giao việc mới — dùng ai-office-run.
---

# ai-office-status

## Khi nào dùng
Sếp muốn biết tiến độ hoặc kết quả của văn phòng AI, không giao việc mới.

## Các bước
1. `mcp__ai-office-mcp__office_status` — trạng thái từng agent + nhiệm vụ gần nhất.
2. Nếu Sếp cần chi tiết: `mcp__ai-office-mcp__office_get_task` với id nhiệm vụ (có đủ phân công, kết quả từng phần, nhật ký bàn giao).
3. Nếu Sếp cần kết quả: `mcp__ai-office-mcp__office_get_report` (bỏ trống `id` để lấy báo cáo mới nhất). Trình bày nguyên văn báo cáo, đừng tóm tắt quá tay.
4. Câu hỏi kiểu "phòng đã chạy bao nhiêu việc / tốn bao nhiêu lượt LLM": `mcp__ai-office-mcp__office_stats`.

## Không làm
- Không gọi `office_create_task` trong skill này.
