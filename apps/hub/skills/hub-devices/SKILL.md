---
name: hub-devices
description: >-
  Theo dõi thiết bị IoT qua app Thiết bị · Device Hub (kết nối Dipper IoT Hub). Dùng khi
  người dùng hỏi về danh sách thiết bị, thiết bị nào đang online/offline, dữ liệu cảm biến
  / telemetry mới nhất (nhiệt độ, độ ẩm...), hoặc cảnh báo từ thiết bị. Kết quả lấy trực
  tiếp từ Dipper Hub — chính xác, không phỏng đoán.
triggers:
  - thiết bị
  - danh sách thiết bị
  - thiết bị online
  - trạng thái thiết bị
  - dữ liệu cảm biến
  - telemetry
  - cảnh báo thiết bị
  - device hub
  - iot hub
  - list devices
  - device status
  - sensor data
---

# hub-devices

Theo dõi thiết bị IoT bằng MCP server `hub-mcp` của app **Thiết bị · Device Hub**.
Mọi dữ liệu lấy từ Dipper Hub qua API — **đừng tự bịa số liệu**.

## Chọn công cụ

- **`mcp__hub-mcp__hub_status`** — kiểm tra app đã kết nối Dipper Hub chưa (URL, đã đăng nhập chưa).
  Gọi ĐẦU TIÊN nếu các tool khác báo lỗi chưa kết nối.
- **`mcp__hub-mcp__hub_list_devices`** — danh sách thiết bị (tên, id, online/offline, model).
  Hỗ trợ lọc `q` theo tên.
- **`mcp__hub-mcp__hub_device_status`** — chi tiết một thiết bị: trạng thái, thuộc tính, lần
  cuối gửi dữ liệu. Truyền `device_id` (lấy từ hub_list_devices).
- **`mcp__hub-mcp__hub_telemetry`** — dữ liệu telemetry của thiết bị: bản ghi mới nhất hoặc
  chuỗi thời gian. Truyền `device_id`, tuỳ chọn `field`, `limit`.
- **`mcp__hub-mcp__hub_alerts`** — cảnh báo gần đây từ Dipper Hub.

## Cách trả lời

- **Kết luận trước**: "3/5 thiết bị online", "Nhiệt độ hiện tại 28.5°C" — rồi mới liệt kê chi tiết.
- Tên thiết bị người dùng nói có thể không khớp chính xác — dùng `hub_list_devices` với `q`
  để tìm gần đúng trước, đừng đoán `device_id`.
- Nếu chưa kết nối Dipper Hub, hướng dẫn người dùng mở app Device Hub → Cài đặt kết nối
  (URL + tài khoản) rồi thử lại.
