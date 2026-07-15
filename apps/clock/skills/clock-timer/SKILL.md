---
name: clock-timer
description: >-
  Hẹn giờ và đếm ngược qua app Đồng hồ · Clock. Dùng khi người dùng muốn "hẹn giờ
  X phút", "đếm ngược Y phút", "báo tôi sau Z phút", "X phút nữa là mấy giờ". Trả về
  thời điểm kết thúc chính xác; bộ đếm ngược trực quan nằm ở tab 'Hẹn giờ' và app sẽ
  báo (thông báo hệ thống + chuông) khi hết giờ.
triggers:
  - hẹn giờ
  - đếm ngược
  - countdown
  - báo tôi sau
  - nhắc tôi sau
  - phút nữa là mấy giờ
  - set a timer
  - timer for
  - remind me in
---

# clock-timer

Xử lý yêu cầu **hẹn giờ / đếm ngược** bằng MCP server `clock-mcp` của app **Đồng hồ · Clock**.

## Công cụ

- **`mcp__clock-mcp__clock_countdown`** — tính thời điểm **kết thúc** của một bộ đếm ngược
  tính từ bây giờ. Truyền `minutes` và/hoặc `seconds` (và `zone` nếu cần). Trả về giờ bắt
  đầu, giờ kết thúc và nhãn thời lượng.

## Cách trả lời

- Cho biết ngay **hết giờ lúc mấy giờ** (giờ kết thúc), rồi nhắc người dùng có thể mở tab
  **Hẹn giờ** trong app Đồng hồ để chạy bộ đếm trực quan — **app sẽ tự báo bằng thông báo
  hệ thống + tiếng chuông** khi countdown về 00:00.
- Nếu người dùng nói "báo/nhắc tôi sau X phút" và cần một lời nhắc THẬT (chạy nền, kể cả
  khi không mở app), đó là việc của bộ lập lịch (schedule) — gợi ý dùng nhắc lịch thay vì
  chỉ bộ đếm client-side. Bộ đếm của app chỉ báo khi tab đang mở.
