---
name: forge-skill
description: >-
  Tạo (dựng) một skill mới cho SenClaw từ mô tả bằng lời qua app Skill Builder.
  Dùng khi người dùng nói "tạo skill / tạo kỹ năng mới / làm skill cho tôi để …",
  "create/build/author a skill that …", hoặc muốn đóng gói một quy trình lặp lại
  thành skill có thể tự động nạp. Skill mới được soạn từ chính các MCP tool và
  sub-agent đang có, kèm triggers để tự surface khi câu người dùng khớp.
triggers:
  - "tạo skill"
  - "tạo kỹ năng mới"
  - "làm skill cho tôi"
  - "viết skill"
  - "dựng skill"
  - "thêm skill mới"
  - "build a skill"
  - "create a skill"
  - "author a skill"
  - "generate a skill"
---

# forge-skill

Dựng một **skill SenClaw mới** theo yêu cầu người dùng, bằng MCP server
`skill-builder-mcp` của app **Skill Builder**. Nguyên tắc cốt lõi: **tái sử dụng
những gì đã có** (MCP tool, sub-agent, skill) thay vì bịa ra khả năng mới, và
gắn **triggers** để skill tự động nạp trong đúng ngữ cảnh.

## Công cụ

- **`mcp__skill-builder-mcp__skill_inventory`** — liệt kê skill, sub-agent và MCP
  server/tool đang có. **Luôn gọi đầu tiên** để nắm bối cảnh và tránh trùng lặp.
- **`mcp__skill-builder-mcp__skill_draft`** — từ một *yêu cầu* (làm gì + khi nào
  chạy), sinh bản nháp skill (name, description, triggers, body, lý do) **nhưng
  chưa cài**. Dùng để xem trước / bàn với người dùng.
- **`mcp__skill-builder-mcp__skill_create`** — sinh **và cài luôn** skill vào
  SenClaw trong một bước, ghi triggers vào frontmatter để tự động nạp. Dùng khi
  người dùng đã rõ ý và muốn dùng ngay. Đặt `overwrite: true` để ghi đè skill
  trùng tên.
- **`mcp__skill-builder-mcp__skill_create_exact`** — cài một skill từ các trường
  bạn tự soạn (không nhờ AI sinh). Dùng khi đã có bản nháp và muốn cài nguyên văn.
- **`mcp__skill-builder-mcp__skill_list`** — xem các skill đang cài.
- **`mcp__skill-builder-mcp__skill_remove`** — gỡ một skill theo tên.

## Quy trình

1. **Làm rõ yêu cầu**: skill để *làm gì* và *khi nào nên chạy* (câu người dùng
   hay gõ, lịch, hay điều kiện). Nếu thiếu, hỏi ngắn gọn 1–2 câu.
2. **`skill_inventory`** để biết công cụ sẵn có. Nếu đã có skill trùng mục đích,
   nói với người dùng và đề xuất chỉnh skill cũ thay vì tạo mới.
3. **Xem trước rồi cài**: mặc định gọi `skill_draft` để trình bày bản nháp (đặc
   biệt là *triggers* và các tool nó dùng) cho người dùng duyệt; sau khi đồng ý
   thì cài bằng `skill_create_exact`. Nếu người dùng bảo "làm luôn / cứ tạo đi",
   dùng thẳng `skill_create`.
4. **Xác nhận**: báo tên skill, mô tả, và các trigger đã gắn; nhắc rằng skill sẽ
   tự nạp khi câu người dùng khớp trigger, hoặc agent có thể chủ động nạp bằng
   tool `Skill`.

## Lưu ý

- Body của skill phải **cụ thể**: nêu đúng tên tool `mcp__<server>__<tool>`, thứ
  tự gọi, điểm quyết định, cách trình bày kết quả — không viết chung chung.
- Triggers là cụm từ ngắn, viết thường; nên có cả tiếng Việt lẫn tiếng Anh khi
  hợp lý. Đừng đặt trigger quá rộng (gây nạp nhầm).
- Nếu cài báo trùng tên, hỏi người dùng rồi cài lại với `overwrite: true`.
- Trả lời bằng ngôn ngữ của người dùng (mặc định tiếng Việt).
