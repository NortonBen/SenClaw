---
name: rewrite-story-manage
description: >-
  Quản lý kho truyện và cấu hình của Rewrite Story: liệt kê truyện, nhập/xoá
  truyện, xem các phiên bản đã viết lại và đọc nội dung, xem danh sách tiến
  trình, và chỉnh cấu hình chia chunk / mức sáng tạo mặc định. Dùng khi Sếp hỏi
  "kho truyện", "danh sách truyện", "các bản viết lại của truyện này", "tiến
  trình viết lại đang chạy", "chỉnh kích thước chunk". KHÔNG dùng để chạy một
  lượt viết lại mới — dùng rewrite-story-run.
---

# rewrite-story-manage

## Khi nào dùng

Xem, sắp xếp và cấu hình. Mọi việc *không* phải là chạy một tiến trình viết lại.

## Điều kiện tiên quyết

Space App `rewrite-story` đang chạy — kiểm tra bằng
`mcp__rewrite-story-mcp__rs_status`.

## Các bước

**Xem kho truyện**
1. `mcp__rewrite-story-mcp__rs_story_list` — id, tên, độ dài, số bản viết lại.
2. `mcp__rewrite-story-mcp__rs_story_versions` với `story_id` để xem các phiên
   bản. Mỗi phiên bản là một truyện riêng có id riêng.
3. `mcp__rewrite-story-mcp__rs_story_get` để đọc, **luôn kèm `offset` và
   `limit`**.

**So sánh bản gốc với bản viết lại**
Đọc cùng một khoảng `offset`/`limit` của cả hai `story_id` rồi đối chiếu. Đừng
tải toàn văn.

**Nhập / xoá truyện**
- `mcp__rewrite-story-mcp__rs_story_import` với `name` + `text`.
- `mcp__rewrite-story-mcp__rs_story_delete` — xoá luôn mọi phiên bản, chunk và
  tiến trình của truyện đó. **Xác nhận với Sếp trước.**

**Xem tiến trình**
- `mcp__rewrite-story-mcp__rs_rewrite_list` (lọc theo `status` nếu cần) để tìm
  `process_id`.
- `mcp__rewrite-story-mcp__rs_rewrite_status` để xem chi tiết một tiến trình.
- `mcp__rewrite-story-mcp__rs_rewrite_cancel` để dừng cái đang chạy. Các chunk
  đã xong vẫn được giữ.

**Cấu hình**
- `mcp__rewrite-story-mcp__rs_settings_get` để xem.
- `mcp__rewrite-story-mcp__rs_settings_set` để đổi.
  - `hybrid_split_max_size` — hay phải giảm nhất, khi model cắt output giữa
    chừng vì chunk quá dài.
  - `hybrid_split_threshold` — 0-1; cao hơn thì cắt chunk dày hơn tại các
    chuyển cảnh.
  - `max_concurrent_processes` — số tiến trình chạy song song.

## Không làm

- **Không gọi `rs_rewrite_start`** ở skill này — đó là việc của
  `rewrite-story-run`.
- **Không đổi `hybrid_split_*` rồi hứa là truyện cũ sẽ được cắt lại.** Chunk
  được lưu theo truyện ngay lần cắt đầu tiên; cấu hình mới chỉ áp dụng cho
  truyện chưa từng cắt. Muốn cắt lại phải nhập truyện thành bản mới.
- **Không đọc toàn văn truyện** vào ngữ cảnh.
