---
name: clock-time
description: >-
  Xem giờ hiện tại, giờ thế giới và đổi múi giờ qua app Đồng hồ · Clock. Dùng khi
  người dùng hỏi "bây giờ mấy giờ / mấy giờ rồi", "mấy giờ ở New York/Tokyo/London",
  "giờ thế giới", "X giờ ở nước A thì mấy giờ ở nước B", hay bất kỳ câu hỏi nào về
  giờ giấc theo múi giờ. Kết quả lấy từ đồng hồ hệ thống — chính xác, không phỏng đoán.
triggers:
  - bây giờ mấy giờ
  - mấy giờ rồi
  - giờ hiện tại
  - mấy giờ ở
  - giờ thế giới
  - giờ bên
  - đổi múi giờ
  - chênh lệch múi giờ
  - what time is it
  - current time
  - world clock
  - time in
  - timezone
---

# clock-time

Trả lời câu hỏi về **giờ giấc** bằng MCP server `clock-mcp` của app **Đồng hồ · Clock**.
Mọi con số lấy từ đồng hồ hệ thống — **đừng tự đoán giờ**.

## Chọn công cụ

- **`mcp__clock-mcp__clock_now`** — "bây giờ mấy giờ / mấy giờ rồi". Trả về giờ hiện tại
  ở một múi giờ (mặc định `Asia/Ho_Chi_Minh`), kèm giờ UTC và thứ trong tuần. Truyền
  `zone` (IANA) để xem giờ nước khác, ví dụ `America/New_York`.
- **`mcp__clock-mcp__clock_world`** — "giờ thế giới / mấy giờ ở nhiều nơi". Truyền `zones`
  là danh sách IANA ngăn cách bằng dấu phẩy; bỏ trống để dùng danh sách mặc định
  (Hà Nội, New York, London, Tokyo).
- **`mcp__clock-mcp__clock_convert`** — "X giờ ở A là mấy giờ ở B". Truyền `time` (HH:MM),
  `from`, `to` (đều IANA). Kết quả cho biết cùng ngày hay khác ngày.

## Cách trả lời

- Nói **kết luận trước**: giờ/ngày cụ thể, rồi mới thêm chi tiết (thứ, chênh lệch múi giờ).
- Khi người dùng nói tên thành phố/nước, tự map sang IANA timezone hợp lý
  (vd "Nhật" → `Asia/Tokyo`, "Mỹ (bờ Đông)" → `America/New_York`).
- Định dạng giờ gọn gàng theo kiểu Việt: `HH:MM ngày DD/MM/YYYY`.
