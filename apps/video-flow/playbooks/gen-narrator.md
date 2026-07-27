---
name: gen-narrator
description: Lồng tiếng narration cho các scene bằng hệ TTS của SenClaw (không cần Chrome extension)
triggers:
  - lồng tiếng
  - thuyết minh
  - đọc lời bình
  - giọng đọc
  - narration
  - voice over
  - sinh giọng
  - tts cho video
---

# Lồng tiếng narration

Tổng hợp giọng đọc cho mọi scene có `narrator_text`, lưu WAV vào media và gắn
vào scene. Đây là **stage duy nhất trong nhánh sản xuất chạy được khi chưa cắm
Chrome extension** — TTS chạy trên máy, không qua Google Flow.

## Kiểm tra trước

```bash
BASE="http://127.0.0.1:4460"
# Có model TTS nào chưa? Giọng nào đang active?
curl -sS -X POST "$BASE/api/mcp/message" -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"vf_tts_status","arguments":{}}}'
```

Chưa cài model nào thì báo người dùng vào **SenClaw → Settings → TTS** để cài
(VieNeu-TTS cho tiếng Việt, MMS-VITS, macOS Speech). Đừng chạy rồi mới báo lỗi.

## Chạy

```bash
curl -sS -X POST "$BASE/api/steps/agent" -H 'content-type: application/json' \
  -d '{"agent_type":"audio","project_id":"<PID>","video_id":"<VID>"}'
```

Bằng MCP (chạy nền, trả ngay):
`mcp__video-flow-mcp__vf_generate_narration` với `video_id` hoặc `project_id`.

Tham số tuỳ chọn khi cần đổi giọng: `voice`, `language`, `speed`, `model_id`,
và `regenerate: true` để làm lại scene đã có tiếng.

## Giọng lấy theo thứ tự

1. Tham số truyền vào
2. `narrator_voice` / `language` của project
3. Cấu hình TTS đang chọn trong SenClaw

Dùng **cùng một giọng** cho cả video. Muốn đổi giọng thì phải `regenerate`,
nếu không các scene cũ vẫn giữ giọng cũ và video sẽ lệch tông giữa chừng.

## Viết narrator_text cho khớp thời lượng

- ~3 từ/giây (Anh), ~4 âm tiết/giây (Việt)
- Scene 8 giây ≈ tối đa 24 từ Anh hoặc 32 âm tiết Việt
- VieNeu hiểu cue cảm xúc trong ngoặc vuông: `[cười]`, `[thở dài]` — giữ nguyên
  trong `narrator_text`

## Kết quả

Scene được điền `narrator_audio_url`, `narrator_audio_media_id`,
`narrator_audio_status`. Scene đã `COMPLETED` sẽ bị bỏ qua trừ khi `regenerate`.
