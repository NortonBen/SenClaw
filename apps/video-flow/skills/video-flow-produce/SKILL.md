---
name: video-flow-produce
description: >-
  Sản xuất video AI end-to-end bằng Video Flow (Flow Kit): từ ý tưởng hoặc kịch
  bản → project → pipeline DAG đa agent (đạo diễn → biên kịch → scene plan →
  shot design → ảnh tham chiếu → ảnh scene → video Veo3 → audio → concat →
  critic) điều khiển Google Flow qua Chrome extension. Use for "làm video",
  "sản xuất video", "tạo video từ kịch bản/ý tưởng", "dựng phim", "chạy pipeline
  video", "make/produce a video", "video from script", "generate scenes".
---

# video-flow-produce

Drive an end-to-end AI video production through the **`video-flow-mcp`** server.
Mọi thao tác đi qua tool `mcp__video-flow-mcp__vf_*` — không gọi REST tay, không
thay bằng browser MCP khác (extension bridge của app là đường duy nhất tới
Google Flow).

## Điều kiện tiên quyết — check FIRST

1. **`mcp__video-flow-mcp__vf_status`** — nhìn `extension_connected`:
   - `false` → **mọi stage sinh ảnh/video sẽ treo**. Báo user: load `extension/`
     vào Chrome (chrome://extensions, Developer mode, Load unpacked), mở
     **labs.google** (Google Flow) để extension bắt token và nối WS về app
     (mặc định `:9222`). Planning/parsing stages (LLM) vẫn chạy được khi chưa
     kết nối, nhưng đừng hứa video.
2. Hỏi user **orientation** nếu chưa rõ: `VERTICAL` (Shorts/TikTok) hay
   `HORIZONTAL` (YouTube). **Never hardcode orientation** — mỗi scene giữ trạng
   thái vertical/horizontal ĐỘC LẬP; luôn truyền orientation user chọn.

## Flow chuẩn

### 1. Project

- Có sẵn? `mcp__video-flow-mcp__vf_project_list` → chọn theo tên.
- Chưa có? `mcp__video-flow-mcp__vf_project_create` với `name` + `story`
  (concept/kịch bản thô) + `material` (style hình ảnh: realistic, 3d_pixar,
  anime, stop_motion, minecraft, oil_painting). Hỏi user nếu chưa nói.
- Nhân vật/bối cảnh quan trọng mà user mô tả rõ →
  `mcp__video-flow-mcp__vf_character_create` (mô tả NGOẠI HÌNH mặc định, MỘT
  bộ trang phục duy nhất — trang phục theo cảnh nằm trong scene prompt).
  Pipeline cũng tự tạo entity nếu bỏ qua bước này.

### 2. Chạy pipeline — `mcp__video-flow-mcp__vf_pipeline_create`

- `mode: "production"` — user ĐÃ có kịch bản/screenplay → truyền vào `script`.
- `mode: "full"` — user chỉ có Ý TƯỞNG thô → pipeline chạy thêm pre-production
  (director → screenwriter → scene_plan → shot_design → visual_asset) trước.
- `mode: "custom"` + `goal` — việc đặc thù, orchestrator LLM tự lập DAG.
- Luôn truyền `orientation`. Một project chỉ một pipeline active — nếu bị chặn,
  xem pipeline cũ bằng `vf_pipeline_status` rồi hỏi user cancel hay chờ.

### 3. Monitor — `mcp__video-flow-mcp__vf_pipeline_status`

Poll giãn cách hợp lý (LLM stages: ~30s; image/video stages: 1–2 phút — mỗi
clip Veo3 mất 2–5 phút). Report tiến độ **theo stage, đúng thứ tự pipeline**:

```
director → screenwriter → scene_plan → shot_design → visual_asset   (pre-production, mode full)
→ scene_builder | script_parser → gen_ref → director_frame → character
→ image → video → audio → media_download → concat → critic
```

Ví dụ report: "Đã xong kịch bản + scene plan (12 cảnh). Đang sinh ảnh tham
chiếu 3/5 nhân vật. Còn: ảnh scene → video → ghép." Kèm số liệu từ
`mcp__video-flow-mcp__vf_scene_list` (image/video/upscale status từng cảnh)
khi đã có scene. Đừng bịa tiến độ — chỉ nói điều tool trả về.

### 3b. Lồng tiếng (stage `audio`) — TTS của SenClaw, KHÔNG cần extension

Stage `audio` tổng hợp giọng đọc cho mọi scene có `narrator_text` bằng hệ TTS
của chính SenClaw (VieNeu-TTS tiếng Việt 48 kHz, MMS-VITS, macOS Speech) — đây
là stage DUY NHẤT trong nhánh sản xuất chạy được khi Chrome extension chưa kết
nối. File WAV lưu vào media và gắn vào scene (`narrator_audio_url`,
`narrator_audio_status`).

- Kiểm tra trước bằng `mcp__video-flow-mcp__vf_tts_status`: nếu chưa cài model
  TTS nào, báo Sếp vào SenClaw Settings → TTS để cài, đừng chạy rồi báo lỗi.
- Chạy riêng (ngoài pipeline) bằng `mcp__video-flow-mcp__vf_generate_narration`
  với `video_id` (hoặc `project_id` / `scene_id`).
- Giọng lấy theo thứ tự: tham số của tool → `narrator_voice` + `language` của
  project → cấu hình TTS đang chọn trong SenClaw. Dùng **cùng một giọng** cho cả
  video; muốn đổi giọng thì phải `regenerate: true`.
- Scene đã có narration được bỏ qua; sửa `narrator_text` thì phải tổng hợp lại.

### 4. Khi task lỗi

- `vf_pipeline_status` cho task `error`/`timeout` + result → đọc nguyên nhân.
- `mcp__video-flow-mcp__vf_requests_status` xem hàng đợi generate: FAILED rows
  có `error_message` (hết quota, CAPTCHA, filter an toàn của Google…).
- Sửa được (vd sửa prompt bằng `vf_scene_update`) thì sửa, rồi
  `mcp__video-flow-mcp__vf_pipeline_control` `action: "retry_task"` (+`task_id`).
- Extension rơi kết nối → bảo user mở lại tab labs.google, check `vf_status`.

### 5. Kết thúc

Khi concat xong, report: video nằm ở đâu (URL/media trong project), critic nói
gì, cảnh nào bị skip/failed (nói thật, kể cả khi kết quả không trọn vẹn).

## Domain rules (không được quên)

- **Scene prompt tả HÀNH ĐỘNG, không tả ngoại hình nhân vật** — visual
  consistency đến từ ảnh tham chiếu, không phải từ prompt.
- **Ảnh tham chiếu của MỌI entity phải xong trước ảnh scene** (`vf_project_get`
  → `reference_ready`; thiếu thì `vf_generate_image` với `all_refs: true`).
- **Video prompt dùng sub-clip timing**: `0-3s: … 3-6s: … 6-8s: …`.
- **Media ID là UUID** — không phải chuỗi `CAMS...`/base64.
- Cascade: ảnh mới xoá video+upscale của orientation đó; video mới xoá upscale.

## Do not

- Không chờ (block) trong một tool call cho việc sinh video — các tool generate
  trả về ngay, tiến độ xem qua status tools.
- Không thay `video-flow-mcp` bằng Playwright hay browser MCP khác.
- Không tự duyệt/đăng gì ra ngoài app; sản phẩm là file/URL trong project.
