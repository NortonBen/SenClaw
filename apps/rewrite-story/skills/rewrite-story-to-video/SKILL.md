---
name: rewrite-story-to-video
description: >-
  Chuyển một truyện đã viết lại sang app làm video (Video Flow): xuất kịch bản
  theo cảnh từ Rewrite Story rồi tạo project → video → pipeline bên Video Flow.
  Dùng khi Sếp nói "làm video từ truyện này", "chuyển truyện sang video", "xuất
  kịch bản sang Video Flow", "truyện này dựng thành phim ngắn", "story to video".
  KHÔNG dùng để chạy một lượt viết lại mới — dùng rewrite-story-run; cũng KHÔNG
  dùng để làm video từ ý tưởng thô — dùng thẳng video-flow-produce.
---

# rewrite-story-to-video

## Khi nào dùng

Đã có một bản viết lại (hoặc một truyện gốc) trong Rewrite Story, và Sếp muốn
biến nó thành video.

## Điều kiện tiên quyết

Cần **cả hai** app cùng chạy:

- `mcp__rewrite-story-mcp__rs_status`
- `mcp__video-flow-mcp__vf_status`

Nếu thiếu một trong hai, báo Sếp — đừng tự chế kịch bản bằng tay.

**Hai app không gọi trực tiếp được nhau.** Bridge của daemon có khai báo
`mcp.call` và `space.rest` nhưng chưa implement (`mcp.call` trả thẳng
"not enabled yet"). Bạn — agent — chính là cây cầu: đọc từ `rs_*`, ghi sang `vf_*`.

## Các bước

1. **Tìm đúng truyện.** `mcp__rewrite-story-mcp__rs_story_list`. Muốn làm video
   thì hầu như luôn dùng **bản đã viết lại**, không phải bản gốc — lấy
   `result_story_id` từ `rs_rewrite_status`, hoặc `rs_story_versions` rồi chọn.

2. **Xuất kịch bản.** `mcp__rewrite-story-mcp__rs_story_export` với `story_id`.
   - Trả về `screenplay` (markdown `# Cảnh N`), `total_scenes`, và `file` —
     đường dẫn bản ĐẦY ĐỦ trên đĩa.
   - `scene_chars` mặc định 900 ≈ một cảnh 8 giây. Muốn video ngắn/nhịp nhanh
     thì giảm; muốn ít cảnh hơn thì tăng.
   - **Báo Sếp số cảnh và thời lượng ước tính trước khi dựng** — mỗi cảnh là một
     lần sinh ảnh + một lần sinh video, tốn thật.

3. **Chốt phạm vi.** Truyện dài ra hàng trăm cảnh. Hỏi Sếp muốn làm cả truyện hay
   chỉ vài chương đầu, rồi dùng `from_scene`/`to_scene`. Đừng âm thầm dựng 300 cảnh.

4. **Tạo project bên Video Flow.**
   `mcp__video-flow-mcp__vf_project_create` với `name` (tên truyện), `story`
   (tóm tắt ngắn — KHÔNG nhét cả truyện vào đây), `language: "vi"`, `material`
   (phong cách hình: realistic / anime / 3d_pixar…). Hỏi Sếp phong cách nếu chưa rõ.

5. **Tạo video và chốt khung hình.**
   `mcp__video-flow-mcp__vf_video_create` với `project_id`, `title`, và
   `orientation`: `VERTICAL` cho TikTok/Shorts, `HORIZONTAL` cho YouTube ngang.
   **Luôn hỏi** — đừng mặc định.

6. **Chạy pipeline.** `mcp__video-flow-mcp__vf_pipeline_create` với
   `project_id`, `script` = chuỗi `screenplay` lấy ở bước 2, và
   `mode: "production"` (đúng chế độ cho kịch bản CÓ SẴN; `full` là cho ý tưởng thô).

7. **Theo dõi.** `mcp__video-flow-mcp__vf_pipeline_status`, poll thưa. Báo tiến
   độ theo stage, không bịa. Khâu ảnh/video cần Chrome extension của Google Flow
   — kiểm bằng `vf_status` trước khi hứa.

## Không làm

- **Không nhét toàn văn truyện vào `vf_project_create.story`** — trường đó là
  tóm tắt/ý tưởng. Kịch bản đi qua `vf_pipeline_create.script`.
- **Không dùng `mode: "full"`** khi đã có kịch bản — nó chạy lại cả khâu tiền kỳ
  (đạo diễn, biên kịch) và ghi đè kịch bản của bạn.
- **Không tự viết lại kịch bản bằng tay.** Việc cắt cảnh đã do bộ chia tiếng Việt
  của Rewrite Story làm, ngắt đúng chỗ chuyển cảnh.
- **Không kéo cả nghìn cảnh vào ngữ cảnh.** Dùng `from_scene`/`to_scene`, hoặc
  đưa Sếp đường dẫn `file`.
- **Không hứa có video** khi `vf_status` báo extension chưa kết nối.
