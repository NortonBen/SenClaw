# AI Discuss Team (`apps/discuss`, port 4760)

Phòng thảo luận AI theo đội cho SenClaw: **BOSS** (người dùng) đặt đề bài + tiêu chí kết
quả; các **member AI** (bộ nhớ riêng xuyên phiên, nhớ cả mạch thinking) tranh luận theo
luật; **Thư ký AI** ghi biên bản từng vòng; **Manager AI** độc lập — không bàn nội dung,
chấm tiến độ so với yêu cầu BOSS, bắt member im lặng phát biểu, đề nghị chốt khi đủ.
Kết quả cuối phân mức chứng minh **THỰC TIỄN / LÝ THUYẾT**, BOSS nghiệm thu (duyệt / từ
chối kèm góp ý → phiên mở lại). UI 2 chế độ: chat + phòng họp 3D isometric (kiểu AI Office).

## Luật thảo luận (engine cưỡng chế)

- Luận điểm gắn `claim_type`: `evidence` (tìm kiếm có dẫn chứng) · `inference` (suy diễn
  từ thông tin đã có) · `creative` (sáng tạo, chưa có bằng chứng) + `provability`:
  `practical`/`theoretical`. `evidence` không kèm nguồn bị hạ nhãn tự động.
- Member khác phải phản hồi luận điểm mở: `agree` (xét bổ sung) hoặc `disagree`
  (**bắt buộc dẫn chứng** — thiếu thì engine trả lượt yêu cầu sửa, tái phạm bị gắn cờ).
- Tin BOSS là ngắt ưu tiên — member kế tiếp phải trả lời trước tiên.
- Im lặng ≥2 vòng → lượt sau bị "bắt phát biểu" (lệnh Manager trong prompt).
- 6 mũ tư duy là nhãn metadata từng phát biểu (Manager giữ mũ xanh dương).
- Citations kiểm được: `doc:<id>` phải tồn tại trong kho (không thì gắn ⚠ chưa kiểm được).

## Kiến trúc

- `src/engine.rs` — vòng thảo luận: tuần tự hoặc song song (Semaphore(3) dưới trần 4
  agent.run/app của daemon), pace đọc lại mỗi lượt, Thư ký + Manager mỗi vòng, trần vòng.
- `src/llm.rs` — bridge daemon: `llm.request` (Thư ký/Manager/member không tool) và
  `agent.run` (member có tool; `space=discuss:<key>` tách bộ nhớ daemon per member;
  `workspace` = thư mục tài liệu phiên để member `Read`/`Grep`). Lưu ý hiện trạng daemon:
  payload `tools` chưa được enforce (soft — ghi trong system prompt), `model` bị bỏ qua.
- `src/db.rs` — SQLite `~/.senclaw/space-app-data/discuss/discuss.sqlite`: discussions,
  members, messages (claims/reactions/citations/flags), documents(+FTS fold đ→d),
  member_memory(+FTS), member_thinking, minutes, results.
- `src/mcp.rs` — MCP `discuss-mcp`, 20 tool `discuss_*`, JSON-RPC qua `/api/mcp/sse` +
  `/api/mcp/message` (reply không mirror lên SSE).
- `web/` — React 19 + Vite 8 + **Ant Design 6.5** (dark, base `/`): MeetingScene isometric
  SVG, ChatFeed nhãn luận điểm + trích dẫn bấm được, panel Tiến độ/Biên bản/Kết quả/
  Tài liệu/Đội, poll 1.2s. Form thành viên: Chuyên môn/Phong cách là TextArea; **mũ thiên
  hướng chọn được NHIỀU** (lưu comma-list, mỗi phát biểu member dùng 1 mũ trong số đó).

## Chạy dev

```bash
cargo run -p discuss                 # backend :4760 (cần daemon :18788 để có LLM)
cd apps/discuss/web && npm run dev   # UI dev, proxy /api → 4760
cargo test -p discuss                # 29 unit tests, không cần LLM
```

## Đóng gói & đăng ký

```bash
apps/discuss/scripts/pack.sh         # → apps/discuss/release/discuss-app.zip
curl -X POST http://127.0.0.1:18788/api/space/apps/register-local \
  -H 'Content-Type: application/json' \
  -d '{"path":"/Users/benji/Projects/SemaClaw/apps/discuss/release"}'
```

⚠ register-local phải trỏ **`apps/discuss/release`** (binary + web_dist nằm đó, không
phải thư mục nguồn). Data nằm ngoài thư mục cài nên update không mất phiên/bộ nhớ.

## Roster mặc định

Quản Lý (manager, mũ xanh dương) · Thư Ký (secretary) · An • Dẫn chứng (white, tools) ·
Bình • Phản biện (black, tools) · Chi • Suy luận (yellow, không tool) · Dũng • Sáng tạo
(green, không tool) · Én • Thời sự (red, tools, tắt sẵn). Thêm/sửa trong tab Đội —
mặc định member thấy **toàn bộ tool MCP hệ thống**, có thể giới hạn per-member.
