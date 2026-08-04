---
name: discuss-team
description: Điều khiển app AI Discuss Team — mở phiên thảo luận cho đội AI (member có bộ nhớ riêng, Thư ký ghi biên bản, Manager điều phối độc lập), BOSS đặt đề bài + tiêu chí, theo dõi tiến độ, nghiệm thu kết quả có phân mức chứng minh thực tiễn/lý thuyết.
---

# AI Discuss Team — điều khiển phòng thảo luận AI

App `discuss` (port 4760) mở phòng thảo luận nhiều AI: BOSS (người dùng) đặt chủ đề +
**yêu cầu kết quả** (tiêu chí chốt); các member AI tranh luận theo luật (đồng tình phải xét bổ
sung, phản đối phải kèm dẫn chứng, luận điểm gắn loại `evidence|inference|creative` + mức
`practical|theoretical`, 6 mũ tư duy); Thư ký ghi biên bản mỗi vòng; Manager độc lập chấm
tiến độ so với yêu cầu BOSS, bắt member im lặng phát biểu, đề nghị chốt khi đủ.

## Tool MCP (server `discuss-mcp` — tên đầy đủ `mcp__discuss-mcp__<tool>`)

| Nhóm | Tool |
|---|---|
| Phiên | `discuss_create` (title + **requirement** bắt buộc, `start:true` chạy luôn), `discuss_start`, `discuss_pause`, `discuss_resume` |
| BOSS | `discuss_say` (chen lời — đội ưu tiên trả lời trước tiên), `discuss_conclude` (ép chốt), `discuss_approve`, `discuss_reject` (feedback bắt buộc) |
| Theo dõi | `discuss_status`, `discuss_messages` (feed tăng dần, `after`), `discuss_minutes` (biên bản), `discuss_progress` (điểm Manager + ai im lặng), `discuss_result` |
| Đội | `discuss_members`, `discuss_member_add`, `discuss_member_update`, `discuss_member_memory` (bộ nhớ riêng + thinking) |
| Kho tài liệu | `discuss_docs_add`, `discuss_docs_search`, `discuss_docs_get` |

## Cách dùng chuẩn

1. **Mở phiên**: `discuss_create` với `title` + `requirement` viết thành tiêu chí đo được
   (ví dụ: "1) kết luận nên/không nên kèm mức chứng minh; 2) ≥3 dẫn chứng kiểm được; 3) rủi ro chính").
   Thiếu requirement thì hỏi lại người dùng — Manager cần nó để biết khi nào ĐỦ.
2. Nạp tài liệu nền bằng `discuss_docs_add` TRƯỚC khi `discuss_start` (member trích dẫn `doc:<id>`).
3. Theo dõi bằng `discuss_progress` + `discuss_messages` (truyền `after` = id cuối đã đọc, đừng đọc lại từ 0).
4. Người dùng muốn nói gì với đội → `discuss_say` nguyên văn, đừng diễn dịch lại.
5. Khi phiên sang trạng thái `review`: đọc `discuss_result` cho người dùng duyệt —
   CHỈ gọi `discuss_approve`/`discuss_reject` khi người dùng nói rõ ý họ, không tự quyết.
6. Phiên chạy nền — không cần chờ; quay lại đọc `discuss_progress` sau.

## Lưu ý

- Member dùng tool qua agent.run của daemon (Search/Zeach/News/Thinking/memory/wiki…);
  chế độ `parallel` chạy tối đa 3 member cùng lúc.
- Mỗi member có bộ nhớ riêng xuyên phiên (kể cả mạch thinking) — xem bằng `discuss_member_memory`.
- Kết quả cuối phân mức **THỰC TIỄN** (có nguồn kiểm được) vs **LÝ THUYẾT** (suy diễn/giả thuyết) — đọc nguyên nhãn, không tự nâng cấp.
