# Cài Chrome extension cho Video Flow

Extension là cầu nối duy nhất tới Google Flow: app **không** gọi thẳng
`aisandbox-pa.googleapis.com`, mọi lệnh sinh ảnh/video đều đi qua phiên đăng
nhập Google của bạn trong trình duyệt.

## Cài

1. Mở `chrome://extensions` → bật **Developer mode**.
2. **Load unpacked** → chọn thư mục `extension/` của app này.
3. Mở https://labs.google/fx/tools/flow và đăng nhập. Extension tự bắt bearer
   token (`ya29.*`) từ phiên đó.
4. Bấm icon extension → kiểm tra 3 đèn đều xanh:
   - **Kết nối app Video Flow** — WebSocket tới app (mặc định `:9222`)
   - **API app (HTTP)** — REST của app (mặc định `:4460`)
   - **Token Google Flow** — đã bắt được token, còn hạn (~60 phút)

## Khi SenClaw cấp cổng khác

App có thể chạy ở cổng khác 4460 (SenClaw tự cấp). Mở popup →
**Kết nối · cổng của app** → nhập lại cổng WS/HTTP → **Lưu & kết nối lại**.
Cổng lưu trong `chrome.storage.local`, không cần build lại extension.

Cổng thật của app xem ở dashboard Video Flow (thẻ trạng thái trên cùng) hoặc:

```bash
curl http://127.0.0.1:<port>/api/status
```

## Popup báo gì khi lỗi

| Tình huống | Popup nói |
|---|---|
| App chưa chạy | "Chưa thấy app Video Flow ở cổng …" → mở app trong SenClaw |
| App chạy nhưng WS sai cổng | "API app chạy ở :4460 nhưng cầu nối WS :… chưa nối được" |
| Đã nối, chưa có token | "Bấm Mở Google Flow và đăng nhập labs.google" |
| Token >55 phút | "Token đã cũ và có thể hết hạn — bấm Lấy lại token" |
| Bạn tự ngắt | "Đang ngắt kết nối thủ công. Bấm Kết nối lại" |

Chi tiết kiến trúc: [EXTENSION_DOCS.md](extension/EXTENSION_DOCS.md).
