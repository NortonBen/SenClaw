---
name: ai-chat-inbox
description: >-
  Theo dõi hộp thư hỗ trợ của AI Chat và trả lời khách trong một hội thoại, kể cả
  khi nhận bàn giao (handoff) từ bot. Dùng khi Sếp/nhân viên muốn "xem hội thoại đang
  chờ", "trả lời khách hàng này", "nhận tiếp nhận một cuộc chat". KHÔNG dùng để tạo hay
  cấu hình bot — dùng ai-chat-manage.
---

# ai-chat-inbox

## Khi nào dùng
Có hội thoại cần người thật xử lý (bot đã đề nghị bàn giao, hoặc khách yêu cầu gặp nhân viên),
hoặc Sếp muốn rà soát các cuộc chat gần đây và trả lời trực tiếp.

## Các bước
1. `mcp__ai-chat-mcp__chat_list_sessions` — xem hội thoại gần đây; chú ý `handoff_state`:
   `pending` = đang chờ tiếp nhận, `with_operator` = người thật đang xử lý.
2. `mcp__ai-chat-mcp__chat_get_session` (id) — đọc toàn bộ tin nhắn để nắm bối cảnh.
3. Nếu tiếp nhận: `mcp__ai-chat-mcp__chat_handoff` với `state="with_operator"`.
4. Trả lời khách bằng `mcp__ai-chat-mcp__chat_send` (sessionId + text) — tin đi qua đúng kênh
   của phiên (Telegram/Web/Zalo/Facebook) và được lưu vào hội thoại.
5. Khi xong, trả lại cho bot bằng `chat_handoff` với `state="bot"` (nếu phù hợp).

## Kết nối với AI Office
Khi một cuộc CSKH cần chuyên môn của văn phòng, có thể giao cho AI Office xử lý (office_create_task),
rồi dùng `chat_send` để chuyển kết quả cho khách. Đây là vai trò "module chat hỗ trợ của AI Office".

## Không làm
- Không trả lời thay khi phiên vẫn do bot xử lý (`handoff_state="bot"`) trừ khi Sếp yêu cầu.
- Không bịa thông tin — nếu thiếu dữ kiện, hỏi lại hoặc tra kiến thức của bot.
