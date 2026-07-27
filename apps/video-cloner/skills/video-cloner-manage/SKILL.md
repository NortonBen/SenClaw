---
name: video-cloner-manage
description: >-
  Quản lý kho dự án Video Cloner: liệt kê dự án đã sao chép, xem lại bộ prompt
  JSON đã tạo, xuất prompt để dán vào Veo 3, kiểm tra cấu hình/API key, đổi
  cấu hình sao chép mà chưa chạy lại, và xoá dự án cũ. Dùng khi Sếp hỏi "có
  những dự án video nào", "xem lại prompt đã tạo", "xuất prompt video", "đổi
  phong cách dự án", "cấu hình Video Cloner", "nhập Gemini API key". KHÔNG dùng
  để chạy phân tích sinh prompt mới — dùng video-cloner-run.
---

# video-cloner-manage

## Khi nào dùng

Sếp muốn xem lại, tra cứu, chỉnh cấu hình hoặc dọn dẹp — không phải chạy một
lượt phân tích mới.

## Điều kiện tiên quyết

- Space App `video-cloner` đang chạy. Gọi `mcp__video-cloner-mcp__vc_status`
  trước; nếu tool không tồn tại thì app chưa được cài/bật, báo Sếp.

## Các bước

1. **Liệt kê kho.** `mcp__video-cloner-mcp__vc_project_list` — tên dự án, phong
   cách, số đoạn đã tạo, dự án nào đang chạy.

2. **Xem chi tiết một dự án.** `mcp__video-cloner-mcp__vc_project_get` cho toàn
   bộ cấu hình sao chép, số đoạn, và danh sách nhân vật kèm `character_id` thật
   (cần cho `vc_replace`) và ai có lời thoại.

3. **Đọc prompt.** `mcp__video-cloner-mcp__vc_scenes` với `offset`/`limit`.
   Mặc định 5 đoạn — đừng bỏ `limit`.

4. **Xuất để dán vào Veo 3.** `mcp__video-cloner-mcp__vc_export`. Mặc định chỉ
   trả thống kê và đoạn đầu; `full: true` khi Sếp thật sự cần cả khối.

5. **Đổi cấu hình mà chưa chạy lại.**
   `mcp__video-cloner-mcp__vc_project_config` lưu phong cách/nhân vật/bối
   cảnh/độ tương đồng mới. Nói rõ với Sếp là **các đoạn đã tạo không đổi** —
   cấu hình mới chỉ áp dụng cho những lượt phân tích sau. Muốn áp dụng cho cả
   video thì phải chạy lại `vc_analyze` với `mode: "start"`, và việc đó xoá hết
   kết quả cũ.

6. **Sửa tên/giọng nhân vật hàng loạt.** `mcp__video-cloner-mcp__vc_replace` —
   xem `video-cloner-run` bước 8 để biết vì sao phải dùng tool này.

7. **Kiểm tra API key.** `vc_status` trả `has_api_key`. Key nhập qua giao diện
   web (nút Cài đặt), không qua MCP — **đừng bao giờ hỏi Sếp đọc key ra chat.**

8. **Xem lịch sử.** `mcp__video-cloner-mcp__vc_history` liệt kê các lượt bóc
   tách đã chạy và các điểm khôi phục đã tự lưu. Dùng khi Sếp hỏi "sao kết quả
   lại thành thế này" hoặc muốn lùi về bản trước.

9. **Khôi phục.** `mcp__video-cloner-mcp__vc_restore` với `snapshot_id` lấy từ
   `vc_history`. Việc khôi phục cũng được lưu lại nên quay ngược tiếp được.
   Nói rõ cho Sếp là thao tác này **thay toàn bộ** scene hiện tại.

10. **Soi lỗi một lượt hỏng.** `mcp__video-cloner-mcp__vc_job_raw` trả về nội
    dung thô model đã trả về, lưu nguyên vẹn. Dùng khi một lượt báo "không có
    scene JSON nào đọc được".

11. **Dọn dẹp.** `mcp__video-cloner-mcp__vc_project_delete` xoá dự án cùng file
    video, toàn bộ prompt và mọi điểm khôi phục.

## Không làm

- **Không chạy `vc_analyze` từ skill này** — đó là việc của `video-cloner-run`.
- **Không xoá dự án khi chưa được Sếp xác nhận rõ ràng.** Không hoàn tác được.
- **Không đổ cả bộ prompt vào chat** chỉ vì Sếp hỏi "có gì trong đó" — tóm tắt
  số đoạn và đưa một đoạn mẫu.
- **Không nhận hay hiển thị Gemini API key trong hội thoại.**
- **Không nói cấu hình mới đã được áp dụng** cho các đoạn đã tạo — nó không.
- **Không khôi phục khi Sếp chưa xác nhận** — nó thay toàn bộ scene hiện tại.
