---
name: rewrite-story-run
description: >-
  Viết lại một truyện bằng Rewrite Story: nhập truyện (nếu chưa có) → ước lượng
  số chunk → chạy tiến trình viết lại nền → theo dõi tiến độ → bàn giao bản mới.
  Dùng khi Sếp muốn "viết lại truyện", "làm bản mới của truyện", "đổi văn phong
  truyện", "viết lại theo phong cách cổ trang/hiện đại", "rewrite story", hoặc
  muốn "chạy tiếp" một tiến trình đã hỏng/bị hủy. KHÔNG dùng để chỉ liệt kê kho
  truyện, xem phiên bản hay chỉnh cấu hình — dùng rewrite-story-manage.
---

# rewrite-story-run

## Khi nào dùng

Sếp đưa một truyện (hoặc trỏ tới truyện đã có trong kho) và muốn một bản viết
lại theo phong cách nào đó. Cũng dùng khi một tiến trình cũ thất bại và cần chạy
tiếp.

## Điều kiện tiên quyết

- Space App `rewrite-story` đang chạy. Gọi `mcp__rewrite-story-mcp__rs_status`
  đầu tiên; nếu tool không tồn tại thì app chưa được cài/bật, báo Sếp chứ đừng
  tự viết lại truyện bằng tay.
- Viết lại là việc **chạy nền, tính bằng chục phút** với truyện dài. Không bao
  giờ chờ đồng bộ.

## Các bước

1. **Xem tình hình.** `mcp__rewrite-story-mcp__rs_status` — biết có tiến trình
   nào đang chạy không, và cấu hình chunk hiện tại.

2. **Có truyện chưa?**
   - Chưa: `mcp__rewrite-story-mcp__rs_story_import` với `name` + `text`.
   - Rồi: `mcp__rewrite-story-mcp__rs_story_list` để lấy `story_id`.

3. **Ước lượng khối lượng trước khi chạy.**
   `mcp__rewrite-story-mcp__rs_story_chunks` cho biết truyện sẽ thành bao nhiêu
   chunk. Mỗi chunk là một lần gọi model. **Báo con số này cho Sếp** ("truyện
   này 47 chunk, chạy khoảng 20-40 phút") trước khi bắt đầu — đừng âm thầm khởi
   động một việc dài.
   Nếu `longest_chunk` lớn bất thường (> 20000), cảnh báo là model có thể cắt
   output giữa chừng và đề nghị giảm `hybrid_split_max_size`.

4. **Chốt yêu cầu phong cách.** `version_plan` là chỉ dẫn quan trọng nhất — nó
   quyết định bản mới khác bản cũ ra sao. Nếu Sếp nói mơ hồ ("viết lại hay hơn"),
   hỏi lại cho cụ thể: giọng văn nào, giữ hay đổi bối cảnh, dài hơn hay bằng.
   `creativity_ratio` (0-100) quyết định được phép xa bản gốc tới đâu: ~20 là
   trau chuốt câu chữ, ~40 giữ nguyên cốt truyện, ~60 đổi được chi tiết phụ,
   ~85 chỉ giữ khung sự kiện.

5. **Chạy.** `mcp__rewrite-story-mcp__rs_rewrite_start` với `story_id`,
   `version_plan`, `user_prompt` (yêu cầu thêm), `creativity_ratio`.
   Tool trả về `process_id` ngay lập tức.

6. **Theo dõi.** `mcp__rewrite-story-mcp__rs_rewrite_status` với `process_id`.
   Poll thưa (vài phút một lần), không phải mỗi vài giây. Báo tiến độ theo
   chunk ("đang ở phần 12/47"), không bịa phần trăm.

7. **Xử lý lỗi — luôn chạy tiếp, đừng làm lại.**
   Nếu `status` là `failed` hoặc `cancelled`, gọi
   `mcp__rewrite-story-mcp__rs_rewrite_retry`. Các chunk đã xong được giữ
   nguyên, tiến trình bắt đầu lại từ chunk dở dang đầu tiên — trường
   `resuming_from_chunk` cho biết bỏ qua bao nhiêu. Chạy lại từ đầu là đốt tiền
   và thời gian vô ích.

8. **Bàn giao.** Khi `completed`, lấy `result_story_id` và đọc bản mới bằng
   `mcp__rewrite-story-mcp__rs_story_get` (nhớ dùng `offset`/`limit`). Đưa Sếp
   một đoạn mở đầu để duyệt chứ đừng đổ cả truyện vào chat.

## Không làm

- **Không tự viết lại truyện bằng chính mình** khi app đang chạy. Toàn bộ giá
  trị nằm ở chunking + resume; viết tay sẽ hỏng giữa chừng và mất sạch.
- **Không chờ đồng bộ** sau `rs_rewrite_start`.
- **Không gọi `rs_story_get` mà bỏ `limit`** — truyện có thể hàng triệu ký tự.
- **Không xoá truyện** (`rs_story_delete`) khi chưa được Sếp xác nhận rõ; nó xoá
  luôn mọi bản viết lại.
- **Không hứa** là đã xong khi chưa thấy `status: completed`.
