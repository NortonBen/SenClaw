---
name: video-cloner-run
description: >-
  Sao chép một video bằng Video Cloner: chọn dự án (video đã tải lên) → chốt
  phong cách/nhân vật/bối cảnh → chạy phân tích nền để sinh prompt JSON 8 giây
  cho Veo 3 → phân tích tiếp từng đoạn → bàn giao bộ prompt. Dùng khi Sếp muốn
  "sao chép video này", "clone video", "nhái lại video", "tái tạo video theo
  phong cách khác", "đổi nhân vật/bối cảnh trong video", hoặc cần "prompt Veo 3"
  từ một video có sẵn. KHÔNG dùng để chỉ liệt kê dự án, xem lại prompt cũ hay
  nhập API key — dùng video-cloner-manage.
---

# video-cloner-run

## Khi nào dùng

Sếp có một video và muốn bộ prompt JSON để tái tạo lại nó bằng Veo 3 — giữ
nguyên phong cách gốc, hoặc đổi sang phong cách/nhân vật/bối cảnh khác.

## Điều kiện tiên quyết

- Space App `video-cloner` đang chạy. Gọi `mcp__video-cloner-mcp__vc_status`
  đầu tiên. Nếu tool không tồn tại thì app chưa được cài/bật — báo Sếp, đừng tự
  ngồi viết prompt Veo bằng tay.
- **Phải có Gemini API key.** `vc_status` trả `has_api_key: false` nghĩa là mọi
  lệnh phân tích sẽ hỏng. Báo Sếp mở Cài đặt của Video Cloner nhập key trước,
  đừng gọi `vc_analyze` để "thử xem".
- **Video phải được tải lên qua giao diện web.** MCP không nhận file. Nếu Sếp
  chưa có dự án nào, hướng dẫn Sếp mở app kéo video vào, rồi quay lại.
- Phân tích **chạy nền, mỗi đoạn 8 giây mất vài phút**. Không bao giờ chờ đồng bộ.

## Các bước

1. **Xem tình hình.** `mcp__video-cloner-mcp__vc_status` — có API key chưa, có
   dự án nào đang chạy không.

2. **Chọn dự án.** `mcp__video-cloner-mcp__vc_project_list` để lấy `project_id`.
   Nếu danh sách rỗng, xem mục "Điều kiện tiên quyết" ở trên.

3. **Đọc cấu hình hiện tại.** `mcp__video-cloner-mcp__vc_project_get` cho biết
   phong cách đang đặt, đã có bao nhiêu đoạn, và ID nhân vật đã phát hiện.

4. **Chốt yêu cầu sáng tạo trước khi tiêu tiền model.** Bốn núm quan trọng:
   - `style` — phong cách đích. Gọi `mcp__video-cloner-mcp__vc_presets` để đưa
     Sếp chọn thay vì tự bịa tên phong cách. Giữ nguyên bản gốc thì chọn mục
     "Phân tích theo video gốc".
   - `char_description` — thay nhân vật chính. Để rỗng = giữ nhân vật gốc.
   - `bg_description` — thay bối cảnh. **Để rỗng KHÔNG có nghĩa là giữ bối cảnh
     gốc** — AI sẽ tự nghĩ ra một bối cảnh mới hợp phong cách. Sếp muốn giữ
     nguyên bối cảnh thì phải mô tả lại bối cảnh gốc.
   - `visual_similarity` (0-100) — bám sát hay sáng tạo. 100 = trung thực với
     video gốc, 0 = sáng tạo tối đa. Đây là núm điều khiển temperature.

   `auto_magic: true` là chế độ "AI tự do sáng tạo": nó **ghi đè** cả
   `char_description` lẫn `bg_description` và ép `visual_similarity` về 0. Đừng
   bật nó chung với mô tả nhân vật cụ thể rồi thắc mắc sao AI không nghe.

5. **Chạy đoạn đầu.** `mcp__video-cloner-mcp__vc_analyze` với `project_id`,
   `mode: "start"` và các núm đã chốt. Tool trả `job_id` ngay.

   **`mode: "start"` XOÁ mọi đoạn đã tạo trước đó.** Dự án đã có kết quả mà Sếp
   chỉ muốn thêm đoạn thì dùng `"continue"`, đừng dùng `"start"`.

6. **Theo dõi.** `mcp__video-cloner-mcp__vc_job` với `job_id`. Poll thưa —
   30-60 giây một lần, không phải mỗi vài giây. Video lớn còn phải tải lên
   Gemini ở lần chạy đầu nên lâu hơn các lần sau.

7. **Phân tích các đoạn tiếp theo.** Mỗi lần `vc_analyze` chỉ sinh ra một đoạn
   8 giây. Video 40 giây cần 5 lượt `mode: "continue"`. Hỏi Sếp video dài bao
   nhiêu để ước lượng số lượt, và **báo con số đó trước khi chạy** — đừng âm
   thầm gọi model chục lần.

8. **Sửa tên và giọng nhân vật.** Dùng
   `mcp__video-cloner-mcp__vc_replace`, không bao giờ sửa từng scene bằng tay.
   Tool này đồng bộ lại `voice_id`, `gender`, `audio_markers` và `voice_marker`
   trên **mọi** scene cùng lúc. Lấy `character_id` đúng từ `vc_project_get`.

9. **Bàn giao.** `mcp__video-cloner-mcp__vc_export` cho thống kê và đoạn đầu.
   Chỉ đặt `full: true` khi Sếp thật sự cần cả khối để dán vào Veo 3.

10. **Bàn giao để sinh video thật.** Kịch bản ở đây mới là prompt, chưa phải
    video. Muốn ra video thì chuyển sang app khác:
    - `mcp__video-cloner-mcp__vc_export_write` ghi ra thư mục chia sẻ và/hoặc
      wiki, để Sếp hoặc công cụ khác cầm đi.
    - `mcp__video-cloner-mcp__vc_handoff_video_flow` bàn giao thẳng sang
      video-flow. **Luôn chạy `dry_run: true` trước** và đưa Sếp duyệt số đoạn,
      số nhân vật, hướng khung hình. Bật `translate: true` vì video-flow đưa
      prompt thẳng cho Veo 3 nên tiếng Anh cho kết quả tốt hơn.
    - **Sau khi bàn giao, tuyệt đối không gọi `vf_pipeline_create` bên
      video-flow.** Nó xoá sạch scene vừa nhận rồi dựng lại bằng LLM. Dùng
      `vf_workflow_run` hoặc các bước `steps/*`.

11. **Lỡ tay thì lùi lại, đừng chạy lại.** App tự lưu một điểm khôi phục ngay
    *trước* mỗi thao tác ghi đè (`mode: "start"`, `regenerate`, `vc_replace`).
    Gọi `mcp__video-cloner-mcp__vc_history` để xem các điểm đó rồi
    `mcp__video-cloner-mcp__vc_restore` với `snapshot_id` tương ứng. Khôi phục
    mất vài giây; chạy lại từ đầu mất vài phút và tốn tiền model — và vì mỗi
    lượt chạy đều lấy mẫu ở temperature khác 0 nên kết quả cũ **không tái tạo
    lại được y hệt**.

## Không làm

- **Không tự viết prompt Veo 3 bằng chính mình** khi app đang chạy. Giá trị nằm
  ở việc xem được video thật; bịa từ mô tả của Sếp sẽ ra prompt sai bố cục.
- **Không chờ đồng bộ** sau `vc_analyze`.
- **Không dùng `mode: "start"`** khi Sếp chỉ muốn phân tích thêm đoạn — nó xoá
  sạch kết quả cũ.
- **Không gọi `vc_scenes` mà bỏ `limit`** — mỗi scene rất dài, đọc cả dự án sẽ
  ngập context. Mặc định 5 đoạn là có lý do.
- **Không tự sửa `voice_id` trong từng scene.** Voice id lệch giữa các đoạn sẽ
  khiến Veo 3 hiểu thành hai nhân vật khác nhau.
- **Không xoá dự án** (`vc_project_delete`) khi chưa được Sếp xác nhận rõ — nó
  xoá luôn file video và toàn bộ prompt.
- **Không chạy lại từ đầu để "sửa" một lần đổi tên sai** — dùng `vc_restore`.
- **Không bàn giao mà chưa cho Sếp xem `dry_run`** — nó tạo hàng loạt bản ghi
  bên app khác.
- **Không gọi `vf_pipeline_create` sau khi bàn giao** — mất sạch scene vừa gửi.
- **Không hứa** là xong khi chưa thấy `status: completed`.
