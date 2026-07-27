# YouTube — SenClaw (Chrome extension)

Cầu nối giữa app **SenClaw YouTube** và phiên đăng nhập YouTube thật của bạn. Không có extension này, app không đọc/đăng được gì (YouTube yêu cầu request phát ra từ browser thật — BotGuard/PoToken).

## Cài đặt

1. Mở `chrome://extensions` → bật **Developer mode** (góc trên phải).
2. Bấm **Load unpacked** → chọn thư mục `extension/` này.
3. Mở **youtube.com** và đảm bảo đã **đăng nhập**.
4. Click icon extension → đặt:
   - **WS port** = `9223` (khớp `YOUTUBE_WS_PORT` của app)
   - **HTTP port** = port app đang chạy (mặc định `4491`, daemon có thể cấp khác)
   - Bấm **Lưu & kết nối lại**.
5. Trong app, gọi `youtube_status` — phải thấy `extensionConnected: true` và `auth.hasSapisid: true`.

## Nó làm gì

- **Bắt trạng thái đăng nhập**: đọc cookie `SAPISID` (không gửi cookie thô đi đâu; chỉ báo cờ có/không).
- **Proxy InnerTube** (`yt_fetch`): ký `SAPISIDHASH` + fetch same-origin `credentials:'include'`; `rules.json` (DNR) set Origin/Referer. Đây là cách né chặn.
- **Remote-control UI** (`yt_ui_*`): lái trang bằng `chrome.debugger` → input **trusted** (cho các việc không có API, vd đăng community post qua Studio). Khi đang lái, Chrome hiện banner "đang được gỡ lỗi" — bình thường.

## Quyền yêu cầu

`storage, alarms, tabs, cookies, webRequest, scripting, declarativeNetRequest, debugger` + host `www.youtube.com`, `studio.youtube.com`, `youtubei.googleapis.com`, `127.0.0.1/localhost`.

> Tự động hoá InnerTube ngoài API chính thức có rủi ro ToS/khoá kênh. Dùng tài khoản phụ, tần suất thấp, và luôn duyệt trước khi gửi.
