# AI Chat — SenClaw Space App

Nền tảng **chatbot CSKH nhiều bot**, port sang Rust từ `ai-agent-chatbot` (Go), theo khuôn mẫu `apps/ai-office`. Là **module chat hỗ trợ của AI Office**.

## Điểm chính

- **Dùng lại SenClaw** qua bridge (`llm.request`, `agent.run`, `knowledge.*`) — không có LLM/vector/DB riêng. Trạng thái app lưu trong SQLite (`~/.senclaw/space-apps/ai-chat/ai-chat.db`).
- **Mỗi bot** có: system prompt, model, không gian kiến thức riêng `ai-chat:<key>` (dùng lại knowledge của SenClaw), và **chính sách bảo mật riêng**.
- **Chính sách MCP/skill cưỡng chế ở daemon**: `use_tools=true` + `allowed_mcp=[...]` → agent chỉ nhận ĐÚNG các công cụ đó (thay đổi nhỏ, cộng thêm ở `agent.run` bridge lõi). `allowed_mcp=[]` ⇒ không có công cụ nào (không bao giờ mở "tất cả").
- **Kiến thức**: màn hình Knowledge ghi thẳng vào knowledge space của SenClaw; engine tự chèn ngữ cảnh (pre-retrieval) nên cả bot không-công-cụ vẫn có RAG.
- **Kênh**: Telegram (polling), Web (WebSocket), Zalo OA + Facebook Messenger (polling + Send API), TikTok Shop IM (thử nghiệm). **Không dùng webhook** → không cần domain/callback công khai.
- **Handoff / Support Inbox**: hội thoại có thể bàn giao cho người thật hoặc cho AI Office.
- **MCP `ai-chat-mcp`**: để daemon/AI Office điều khiển nền tảng (tạo/cấu hình bot, gửi tin, bàn giao…).

## Chạy khi phát triển

```bash
cargo run -p ai-chat            # http://127.0.0.1:4440 (cần daemon SenClaw ở 18788)
npm --prefix apps/ai-chat/web run dev   # Vite dev server cho web UI
```

## Đóng gói

```bash
apps/ai-chat/scripts/pack.sh    # build web + binary → release/ + ai-chat-app.zip
```

## Cấu hình

- `PORT` (mặc định 4440), `SENCLAW_BASE_URL` (mặc định `http://127.0.0.1:18788`), `SENCLAW_SPACE_APP_ID` (`ai-chat`).
- Token/credential của từng kênh nhập trong màn hình **Channels** (lưu trong SQLite, che khi trả về UI).
