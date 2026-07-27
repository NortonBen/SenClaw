# TikTok Activity Controller (extension)

Extension MV3 (unpacked) điều khiển **một tab TikTok đã đăng nhập** cho app `tiktok-activity`.

## Cách hoạt động

- Service worker dial WebSocket tới app: `ws://127.0.0.1:9225/` (đổi bằng `TIKTOK_EXT_WS_PORT`).
- App gửi `{id, method, params}`; extension chạy primitive trên tab điều khiển qua `chrome.debugger`:
  - `eval` → `Runtime.evaluate` (không bị chặn bởi CSP như eval nhúng trang),
  - `mouse_click` / `type_text` / `press_key` / `wheel` → `Input.dispatch*` (sự kiện thật, `isTrusted`),
  - `navigate` / `url` → `chrome.tabs`.
- Trả kết quả `{id, result}` (hoặc `{id, error}`) qua WS; fallback `POST /api/ext/callback` nếu WS rớt.

Logic action TikTok (like/comment/share/atomics…) nằm ở **app (Rust)**; extension chỉ chạy primitive.

## Cài đặt

1. Chạy app: `TIKTOK_CONTROL_MODE=extension cargo run -p tiktok-activity` (mặc định đã là extension).
2. Chrome → `chrome://extensions` → bật **Developer mode** → **Load unpacked** → chọn thư mục `apps/tiktok-activity/extension`.
3. Mở `https://www.tiktok.com/` và đăng nhập.
4. Bấm icon extension → **Điều khiển tab này** (chọn đúng tab TikTok).
5. Trạng thái kết nối kiểm tra tại `GET /api/ext/status` (hoặc chấm xanh trong popup).
6. Chạy flow từ UI app (:4580) — engine điều khiển tab đó.

> `chrome.debugger` sẽ hiện banner "… đang gỡ lỗi trình duyệt này" — đó là cơ chế điều khiển, giữ nguyên khi chạy.

## Panel nhật ký (theo dõi điều khiển)

Popup có **Nhật ký điều khiển** cập nhật realtime: mỗi lệnh app gửi (navigate / mouse_click / type_text / press_key / eval / wheel…) hiện một dòng gồm giờ, tên method, tóm tắt tham số và thời gian chạy (ms). Lệnh lỗi tô đỏ kèm lý do; sự kiện kết nối/mất kết nối tô xanh. Giữ 200 dòng gần nhất, nút **Xóa** để dọn.

Cơ chế: `background.js` ghi vào `chrome.storage.local` (key `ctrlLog`, throttle 150ms); popup lắng nghe `chrome.storage.onChanged` nên đồng bộ tức thời (kể cả khi mở popup sau).

## Bảng điều khiển (panel — trang riêng)

Popup đóng khi click ra ngoài, nên có **Bảng điều khiển** (`panel.html`) mở trong một **tab riêng** để theo dõi liên tục — kiểu WS Debugger:

- Mở bằng: nút **"Mở bảng điều khiển"** trong popup, hoặc `chrome://extensions` → Details → **Extension options**.
- 3 tab:
  - **Hoạt động** — log realtime dạng bảng (giờ · method · tham số · ms), lọc **Chỉ lỗi**, **Tự cuộn**, **Xóa log**.
  - **Kết nối** — thẻ số liệu: trạng thái, cổng WS, số lệnh đã chạy, số lệnh lỗi, số lần kết nối, uptime.
  - **Cài đặt** — chọn tab tiktok.com để điều khiển (danh sách các tab đang mở), xem tab đang điều khiển.
- Mở panel ở một cửa sổ riêng để vừa xem log vừa để app điều khiển tab TikTok ở cửa sổ khác.

`background.js` còn lưu bộ đếm ở `chrome.storage.local` key `stats` (`cmdCount`/`errCount`/`connects`/`connectedSince`) cho tab Kết nối.
