---
name: hub-control
description: >-
  Điều khiển thiết bị IoT qua app Thiết bị · Device Hub (kết nối Dipper IoT Hub). Dùng khi
  người dùng muốn bật/tắt thiết bị (đèn, máy bơm, quạt...), đặt giá trị thuộc tính, hoặc
  gửi lệnh tuỳ ý xuống thiết bị. Lệnh đi qua Dipper Hub → MQTT xuống thiết bị thật —
  LUÔN xác nhận với người dùng trước khi gửi lệnh thay đổi trạng thái.
triggers:
  - bật thiết bị
  - tắt thiết bị
  - điều khiển thiết bị
  - gửi lệnh thiết bị
  - bật đèn
  - tắt đèn
  - bật máy bơm
  - tắt máy bơm
  - bật quạt
  - tắt quạt
  - turn on device
  - turn off device
  - control device
  - send command
---

# hub-control

Điều khiển thiết bị IoT thật bằng MCP server `hub-mcp` của app **Thiết bị · Device Hub**.

## Quy trình BẮT BUỘC

1. **Xác định đúng thiết bị**: `mcp__hub-mcp__hub_list_devices` (lọc `q` theo tên người dùng nói).
   Nếu nhiều thiết bị khớp, hỏi lại người dùng chọn cái nào — **không tự chọn bừa**.
2. **Kiểm tra thiết bị online**: lệnh gửi tới thiết bị offline sẽ không có tác dụng —
   báo cho người dùng biết nếu offline.
3. **Xác nhận trước khi gửi**: nêu rõ "sẽ gửi lệnh X tới thiết bị Y" và chờ người dùng đồng ý,
   trừ khi trong cùng tin nhắn người dùng đã ra lệnh rõ ràng, cụ thể (vd "tắt máy bơm vườn ngay").
4. **Gửi lệnh**: `mcp__hub-mcp__hub_send_command` với `device_id`, `command` (tên action trên
   Dipper Hub) và `params` (JSON payload). Action có sẵn: `sendMsgToDevice` (gửi payload thẳng
   xuống thiết bị qua MQTT — dùng mặc định), `updateServerPropertyDevice`,
   `switchServerPropertyDevice`. Ví dụ bật/tắt: command `sendMsgToDevice`, params `{"on": true}` —
   payload cụ thể do firmware thiết bị định nghĩa.
5. **Kiểm chứng kết quả**: sau vài giây gọi lại `hub_device_status` / `hub_telemetry` để xác nhận
   trạng thái đã đổi, và báo kết quả thật (đừng nói "đã bật" nếu chưa thấy trạng thái đổi).

## Lưu ý an toàn

- Đây là thiết bị vật lý thật. Với lệnh có rủi ro (van, motor, nguồn điện), luôn xác nhận lại.
- Không gửi lệnh hàng loạt tới nhiều thiết bị trong một lượt trừ khi người dùng yêu cầu rõ.
- Nếu Dipper Hub trả lỗi, báo nguyên văn lỗi — đừng thử lại quá 2 lần.
