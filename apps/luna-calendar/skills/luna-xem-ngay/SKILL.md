---
name: luna-xem-ngay
description: >-
  Xem lịch âm và ngày tốt xấu Việt Nam qua app Lịch Âm · Luna Calendar. Dùng khi
  người dùng hỏi "hôm nay ngày âm bao nhiêu / hôm nay tốt hay xấu", "xem ngày <ngày>",
  "giờ hoàng đạo hôm nay", "chọn ngày tốt để cưới/khai trương/xuất hành", "đổi ngày
  âm sang dương (giỗ, Tết, sinh nhật)", "những ngày tốt trong tháng", hay bất kỳ câu
  hỏi nào về can chi, tiết khí, hướng xuất hành, ngày kỵ. Kết quả là dữ liệu xác định
  (thuật toán Hồ Ngọc Đức), không phải phỏng đoán.
---

# luna-xem-ngay

Trả lời các câu hỏi về **lịch âm Việt Nam** và **xem ngày tốt xấu** bằng MCP server
`luna-mcp` của app **Lịch Âm · Luna Calendar**. Mọi kết quả (âm lịch, can chi, hoàng
đạo, giờ tốt) đều được tính bằng thuật toán, chính xác và nhất quán — đừng tự bịa.

## Chọn công cụ

- **`mcp__luna-mcp__luna_today`** — "hôm nay là ngày gì / tốt hay xấu / âm lịch bao nhiêu".
  Trả về toàn bộ almanac của hôm nay (múi giờ +7).
- **`mcp__luna-mcp__luna_day`** — xem ngày tốt xấu cho MỘT ngày dương lịch cụ thể
  (`date` = `YYYY-MM-DD`). Dùng cho "xem ngày mai/ngày kia", "ngày 20/8 có tốt không".
- **`mcp__luna-mcp__luna_good_hours`** — chỉ lấy giờ Hoàng Đạo / Hắc Đạo của một ngày.
- **`mcp__luna-mcp__luna_solar_to_lunar`** — đổi ngày **dương → âm** (kèm can chi + tốt/xấu).
- **`mcp__luna-mcp__luna_lunar_to_solar`** — đổi ngày **âm → dương**. Dùng để tìm ngày
  dương của giỗ/Tết/sinh nhật âm (`lunar_day`, `lunar_month`, `lunar_year`, `leap`).
- **`mcp__luna-mcp__luna_good_days`** — liệt kê **ngày tốt (hoang-dao)** hoặc **ngày xấu
  (hac-dao)** trong một tháng dương (`year`, `month`, `kind`). Dùng để "chọn ngày tốt
  tháng này để cưới/khai trương".
- **`mcp__luna-mcp__luna_advise`** — luận giải AI xem một ngày có hợp một **việc** cụ thể
  (`activity` = "cưới hỏi", "khai trương"…). Cần daemon bật LLM.

## Cách làm

1. **Hỏi về hôm nay** → gọi `luna_today`, tóm tắt: âm lịch, can chi ngày, Hoàng Đạo hay
   Hắc Đạo, giờ Hoàng Đạo, và cảnh báo ngày kỵ nếu có.
2. **Hỏi một ngày cụ thể** → `luna_day` với `date`. Nêu kết luận tốt/xấu trước, rồi chi tiết.
3. **Chọn ngày tốt trong khoảng** → `luna_good_days` cho tháng liên quan, lọc bỏ ngày phạm
   kỵ (Nguyệt kỵ/Tam nương) nếu người dùng cần ngày "sạch"; gợi ý vài ngày kèm giờ Hoàng Đạo.
4. **Đổi lịch** → `luna_solar_to_lunar` hoặc `luna_lunar_to_solar`. Với ngày âm có thể là
   tháng nhuận, hỏi rõ hoặc thử cả hai.
5. **Hợp việc gì** → sau khi có almanac, gọi `luna_advise` (nếu có LLM) để đưa lời khuyên
   nên/không nên kèm khung giờ và hướng tốt.

## Lưu ý

- Kết quả tốt/xấu chính là **Hoàng Đạo/Hắc Đạo** cộng với ngày kỵ dân gian; hãy trình bày
  như thông tin tham khảo văn hoá truyền thống, không khẳng định tuyệt đối.
- Luôn kèm **giờ Hoàng Đạo** và **hướng xuất hành** khi người dùng định làm việc lớn.
- Trả lời bằng ngôn ngữ của người dùng (mặc định tiếng Việt).
