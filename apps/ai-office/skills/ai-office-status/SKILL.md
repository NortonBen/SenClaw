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
2. Câu hỏi kiểu "tình hình công ty / hôm nay làm gì trước": `mcp__ai-office-mcp__office_dashboard` (độ bám hướng, mục tiêu quý, việc chờ duyệt, nhịp họp, token tháng); muốn Giám đốc vận hành chốt biên bản thì `office_run_meeting` (`kind: "morning"` sáng, `"evening"` tối — một lượt LLM thật, chỉ chạy khi Sếp yêu cầu họp).
3. Câu hỏi về bảng việc ("việc nào chờ duyệt / đang làm"): `mcp__ai-office-mcp__office_board` — 4 cột inbox / doing / waiting / done.
4. Nếu Sếp cần chi tiết: `mcp__ai-office-mcp__office_get_task` với id nhiệm vụ (có đủ phân công, kết quả từng phần, nhật ký bàn giao).
5. Nếu Sếp cần kết quả: `mcp__ai-office-mcp__office_get_report` (bỏ trống `id` để lấy báo cáo mới nhất). Trình bày nguyên văn báo cáo, đừng tóm tắt quá tay.
6. Câu hỏi kiểu "phòng đã chạy bao nhiêu việc / tốn bao nhiêu lượt LLM": `mcp__ai-office-mcp__office_stats`.

## Không làm
- Không gọi `office_create_task` trong skill này.
