---
name: ai-chat-manage
description: >-
  Tạo và cấu hình chatbot CSKH trong app AI Chat, gồm việc đặt CHÍNH SÁCH BẢO MẬT
  cho từng bot — allowlist MCP (allowed_mcp) và skill (allowed_skills) mà bot được
  phép dùng. Dùng khi Sếp muốn "tạo một bot chăm sóc khách hàng", "giới hạn công cụ
  của bot", "cho bot dùng knowledge X". KHÔNG dùng để trả lời khách hay xem hộp thư —
  dùng ai-chat-inbox.
---

# ai-chat-manage

## Khi nào dùng
Sếp muốn tạo hoặc chỉnh một chatbot: đổi system prompt, chọn model, bật/tắt công cụ,
và quan trọng nhất là **giới hạn bot chỉ được dùng đúng MCP/skill cho phép** (tránh
bot bị lạm dụng chạm tới công cụ nhạy cảm).

## Nguyên tắc bảo mật (quan trọng)
- `use_tools=false` → bot chỉ dùng LLM + kiến thức, KHÔNG có công cụ nào (an toàn nhất).
- `use_tools=true` + `allowed_mcp=[]` → vẫn KHÔNG có công cụ (không bao giờ mở "tất cả").
- `use_tools=true` + `allowed_mcp=[...]` → bot chỉ dùng ĐÚNG các công cụ liệt kê. Daemon
  cưỡng chế danh sách này (agent.run tools allowlist) — bot không thể gọi công cụ ngoài danh sách.
- Chỉ thêm vào allowlist những gì thật sự cần. Cân nhắc kỹ `Bash`, ghi tệp, hay MCP có tác dụng phụ.

## Các bước
1. Gọi `mcp__ai-chat-mcp__chat_list_bots` xem các bot hiện có.
2. Tạo mới bằng `mcp__ai-chat-mcp__chat_create_bot` (name + greeting + system_prompt), hoặc chỉnh bằng
   `mcp__ai-chat-mcp__chat_update_bot`.
3. Khi đặt allowlist: tên công cụ phải là tên ĐẦY ĐỦ. Ví dụ MCP: `mcp__senclaw-browser__browser_navigate`;
   công cụ lõi: `WebSearch`, `Read`. (Trong Web UI có bộ chọn liệt kê sẵn từ `/api/mcp-inventory`.)
4. Xác nhận lại với Sếp danh sách công cụ/skill trước khi bật cho một bot công khai.

## Không làm
- Không cấp `allowed_mcp`/`allowed_skills` rộng hơn yêu cầu.
- Không bật `use_tools` cho bot công khai mà không rà allowlist.
