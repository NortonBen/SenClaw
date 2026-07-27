---
name: parse-script
description: Bóc tách kịch bản .md thành JSON scene (preview, không ghi DB)
triggers:
  - phân tích kịch bản
  - bóc tách kịch bản
  - script thành cảnh
  - parse script

---

# parse-script — Bóc Tách Kịch Bản Thành Danh Sách Cảnh

Parse một file kịch bản (.md) thành danh sách cảnh quay có cấu trúc JSON, không ghi vào DB.

## Mô Tả

Gọi ScriptParserAgent (LLM-powered) để phân tích kịch bản và trả về:
- Danh sách scenes với prompt, video_prompt, character_names, duration, shot_type, camera_movement
- Danh sách characters với description và image_prompt
- Không lưu vào DB (standalone, dùng để preview trước khi tạo project)

## Sử Dụng

```bash
curl -X POST http://127.0.0.1:4460/api/script/parse \
  -H "Content-Type: application/json" \
  -d '{
    "script": "# Cảnh 1 - Buổi Sáng Trên Cánh Đồng\n\nNam (25 tuổi, áo trắng) đứng nhìn ra cánh đồng lúa vàng rực.\nGió thổi nhẹ làm lúa lao xao.\n\nNAM (nhỏ giọng): Đây là quê hương...\n\n# Cảnh 2 - Dòng Sông\n\nHoa (23 tuổi, váy xanh) ngồi bên bờ sông.",
    "provider": "gemini"
  }'
```

Provider options: `"gemini"` (default) | `"claude"` | `"openai"`

## Output Format

```json
{
  "scenes": [
    {
      "display_order": 1,
      "prompt": "Young man in white shirt standing in golden rice field at sunrise, wide angle",
      "video_prompt": "Camera slowly pans left across the rice field, wind gently moves the stalks, man stands still looking into the distance",
      "character_names": ["Nam"],
      "duration": 8.0,
      "shot_type": "WIDE",
      "camera_movement": "PAN",
      "narrator_text": "Đây là quê hương..."
    }
  ],
  "characters": [
    {
      "name": "Nam",
      "entity_type": "character",
      "description": "Chàng trai 25 tuổi, khuôn mặt hiền lành, mặc áo trắng",
      "image_prompt": "Character reference portrait: young Vietnamese man, 25 years old, kind face, white shirt, natural lighting, photorealistic"
    }
  ]
}
```

## Tích Hợp Với Project

Sau khi parse, dùng kết quả để tạo project:

```bash
# 1. Parse script
PARSED=$(curl -s -X POST http://127.0.0.1:4460/api/script/parse \
  -H "Content-Type: application/json" \
  -d '{"script": "...kịch bản..."}')

# 2. Tạo project
curl -X POST http://127.0.0.1:4460/api/projects \
  -H "Content-Type: application/json" \
  -d '{"name": "Tên phim", "language": "vi"}'

# 3. Tạo pipeline với script
curl -X POST http://127.0.0.1:4460/api/pipeline/create \
  -H "Content-Type: application/json" \
  -d '{"project_id": "...", "script": "...kịch bản...", "orientation": "VERTICAL"}'
```

## Timing Reference

| Độ dài cảnh | Số trang | Duration |
|------------|----------|----------|
| Cảnh ngắn | 1/16 trang | 4s |
| Cảnh vừa | 1/8 trang | 8s |
| Cảnh dài | 1/4 trang | 16s |

Shot types: `WIDE` | `MEDIUM` | `CLOSE_UP` | `EXTREME_CLOSE_UP`
Camera: `STATIC` | `PAN` | `TILT` | `DOLLY` | `ZOOM` | `HANDHELD` | `CRANE`
