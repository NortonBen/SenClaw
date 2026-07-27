---
name: video-flow-manage
description: >-
  Quản trị & chỉnh sửa trong Video Flow: xem/sửa project, video, scene, nhân
  vật/bối cảnh; sinh lại ảnh tham chiếu / ảnh scene / video từng cảnh; upscale
  4K; soi hàng đợi request; đọc & tinh chỉnh soul (prompt) của từng sub-agent.
  Use for "danh sách video project", "cảnh quay", "scene", "nhân vật video",
  "ảnh tham chiếu", "regenerate scene", "upscale video", "trạng thái pipeline",
  "pipeline status", "sửa prompt agent", "list scenes".
---

# video-flow-manage

Inspect and manage a Video Flow production via **`video-flow-mcp`**. Mọi dữ
liệu phải đến từ tool — không bịa id, status hay URL. IDs (project/video/scene/
character/media) đều là **UUID**.

## Tra cứu (read)

- **`mcp__video-flow-mcp__vf_status`** — sức khoẻ app: extension connected?,
  worker, LLM profile, đếm project/scene/request. Vào đây đầu tiên khi có gì
  "không chạy".
- **`mcp__video-flow-mcp__vf_project_list`** / **`vf_project_get`** — danh sách
  project; chi tiết = project + videos (kèm scene_count, orientation) +
  characters (kèm `reference_ready`).
- **`mcp__video-flow-mcp__vf_video_list`** — video của project (lấy `video_id`).
- **`mcp__video-flow-mcp__vf_scene_list`** — tiến độ từng cảnh, gọn: status
  image/video/upscale cho CẢ vertical và horizontal (độc lập nhau).
- **`mcp__video-flow-mcp__vf_scene_get`** — full một cảnh (prompt, video_prompt,
  narrator_text, URLs, media_ids) — đọc trước khi sửa.
- **`mcp__video-flow-mcp__vf_character_list`** — entity + ảnh tham chiếu sẵn chưa.
- **`mcp__video-flow-mcp__vf_requests_status`** — hàng đợi generate: đếm theo
  status/type + các request gần nhất; FAILED có `error_message`.
- **`mcp__video-flow-mcp__vf_pipeline_status`** — DAG hiện tại (truyền
  `project_id` là đủ, tự lấy pipeline mới nhất).

## Chỉnh sửa (write)

- CRUD: **`vf_project_create/update/delete`**, **`vf_video_create`**,
  **`vf_scene_create/update/delete`**, **`vf_character_create/update`**.
  `vf_project_delete` xoá CẢ videos/scenes/requests/pipelines — xác nhận với
  user trước.
- Scene prompt: tả **hành động**, không tả ngoại hình nhân vật; `video_prompt`
  theo sub-clip timing `0-3s: …`. Sửa prompt xong media KHÔNG tự sinh lại —
  phải generate lại (bên dưới).
- Pipeline: **`mcp__video-flow-mcp__vf_pipeline_control`** — `pause` / `start`
  / `cancel` / `retry_task` (+`task_id` từ `vf_pipeline_status`).

## Regenerate & cascade (nắm chắc trước khi bấm)

Tool generate **trả về ngay** (chạy nền) — theo dõi bằng `vf_scene_get` /
`vf_requests_status`. Cần `extension_connected: true` (`vf_status`).

- **Ảnh tham chiếu entity**: `mcp__video-flow-mcp__vf_generate_image`
  `{ character_id, regenerate: true }` — sau khi đổi mô tả ngoại hình.
  Thiếu ref hàng loạt: `{ project_id, all_refs: true }`.
- **Ảnh scene**: `vf_generate_image { scene_id, orientation, regenerate: true }`
  (thêm `edit_prompt` để EDIT ảnh hiện có thay vì sinh mới — giữ continuity cho
  cảnh CONTINUATION).
  ⚠️ **Cascade**: ảnh mới → **xoá video + upscale** của orientation đó
  (tự động, không hỏi lại) → phải gen video lại.
- **Video scene**: `mcp__video-flow-mcp__vf_generate_video`
  `{ scene_id, orientation, regenerate: true }` — cần ảnh orientation đó đã
  COMPLETED. ⚠️ Video mới → **xoá upscale** của orientation đó.
- **Upscale 4K**: `mcp__video-flow-mcp__vf_upscale_video { scene_id, orientation }`
  — chỉ sau khi video COMPLETED; cần tài khoản Flow TIER_TWO.

Thứ tự sửa một cảnh hỏng: sửa prompt (`vf_scene_update`) → regen ảnh (nếu hình
sai) hoặc chỉ regen video (nếu chuyển động sai) → chờ COMPLETED → upscale lại.

## Souls — tinh chỉnh prompt từng sub-agent

- **`mcp__video-flow-mcp__vf_agents_list`** — 17 built-in (director,
  screenwriter, scene_plan, shot_design, visual_asset, scene_builder,
  script_parser, gen_ref, director_frame, character, image, video, audio,
  media_download, concat, critic, orchestrator) + skill agents, kèm excerpt soul.
- **`mcp__video-flow-mcp__vf_soul_get`** `{ agent_type }` — đọc nguyên văn.
- **`mcp__video-flow-mcp__vf_soul_set`** `{ agent_type, content }` — GHI ĐÈ cả
  file. Quy trình an toàn: get → sửa có chủ đích (giữ cấu trúc/ý gốc) → set →
  báo user thay đổi áp dụng từ lần chạy sau. Ví dụ: "critic chấm gắt hơn",
  "screenwriter viết giọng tài liệu".

## Do not

- Không hardcode orientation — đọc từ video/scene hoặc hỏi user.
- Không gọi generate lặp lại khi request đang PENDING/PROCESSING (app có lock
  per-scene, nhưng đừng spam).
- Không thay server MCP khác cho các thao tác này.

## Ảnh/video mất hình (URL Google Flow hết hạn)

Google Flow trả asset qua URL ký ngắn hạn. App tự tải về máy ngay khi sinh
xong, nhưng project tạo từ bản cũ (hoặc lúc tải bị lỗi mạng) có thể còn URL
remote — vài giờ sau là mất hình.

- Sửa: `mcp__video-flow-mcp__vf_media_localize` với `project_id` (bỏ trống để
  quét toàn bộ app). Trả về số đã tải / bỏ qua / lỗi.
- Sau khi chạy, mọi URL trong DB trỏ về `/api/media/{id}/file` — không phụ
  thuộc Google nữa.

## Yêu cầu kẹt "PROCESSING"

Một `request` không thể còn đang chạy sau khi app khởi động lại, nên lúc boot
app tự đối soát: asset đã tồn tại → COMPLETED, chưa có → FAILED kèm lý do.
Nếu vẫn thấy PROCESSING lâu bất thường trong lúc app đang chạy, kiểm tra
`vf_status` (extension còn kết nối không) và `vf_requests_status` (thông báo
lỗi gần nhất).
