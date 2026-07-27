# Kaen Vocabulary Helper - Chrome Extension

Chrome Extension để tra cứu và lưu từ vựng tiếng Anh vào **Kaen** — Space App học từ vựng của SenClaw chạy local (mặc định `http://localhost:4500`). Không cần đăng nhập: app là single-user, extension chỉ cần app đang chạy.

Bản này được adapt từ Kaizen Vocabulary Helper (đã gỡ toàn bộ auth/JWT).

## Tính năng

### 🔍 Tra cứu từ vựng

- Tra từ Cambridge Dictionary với IPA, definition, examples
- Dịch qua Google Translate (mặc định tiếng Việt)
- Fallback: Free Dictionary API, rồi đến dictionary của chính app Kaen (`/api/dictionary/lookup`, có cache riêng)
- Cache kết quả 7 ngày để tra nhanh hơn

### 💾 Lưu vào Lesson

- Chọn lesson (hoặc tạo lesson mới ngay trong popup) để lưu từ
- Tự động điền IPA, part of speech, definition (→ `explain`), ví dụ (→ `examples`), nghĩa dịch (→ `meanings.vi`)

### 🖱️ Tra nhanh trên trang web

- Double-click vào từ bất kỳ để tra (mini popup)
- Right-click context menu
- Keyboard shortcut: `Ctrl+Shift+K`

## Cài đặt (Load unpacked)

1. Đảm bảo app **Kaen** đang chạy trong SenClaw (port **4500**) — kiểm tra nhanh: mở <http://localhost:4500/api/status> phải trả `{"ok":true,"name":"kaen",...}`
2. Mở Chrome, vào `chrome://extensions/`
3. Bật **Developer mode** (góc phải trên)
4. Click **Load unpacked**
5. Chọn thư mục `apps/kaen/extension` này

Icon extension sẽ hiện trạng thái kết nối: **Đã kết nối Kaen ✓** (chấm xanh) hoặc **Kaen offline** (chấm đỏ — hãy mở app SenClaw rồi bấm Retry).

## Cấu hình

1. Click vào icon extension → mở **Settings** (icon ⚙️)
2. Tuỳ chỉnh:
   - **Kaen App URL**: mặc định `http://localhost:4500/api` (đổi nếu app chạy port khác)
   - **Translation Language**: ngôn ngữ dịch (mặc định Tiếng Việt)
   - **Quick Lookup**: bật/tắt double-click và auto-lookup

## Sử dụng

### Tra từ trong Popup

1. Click icon extension
2. Nhập từ cần tra
3. Xem kết quả với nghĩa, phát âm, ví dụ
4. Chọn lesson và click **Save to Lesson**

### Tra từ trên trang web

1. **Double-click** vào từ bất kỳ
2. Hoặc **bôi đen** từ và nhấn `Ctrl+Shift+K`
3. Hoặc **right-click** và chọn "Dictionary lookup"

## API Endpoints sử dụng (Kaen, không auth)

| Endpoint | Method | Mô tả |
|----------|--------|-------|
| `/api/status` | GET | Health-check (`{ok:true,name:"kaen"}`) |
| `/api/lessons?search=&limit=100` | GET | Danh sách lessons — envelope `{lessons:[...], total,...}` |
| `/api/lessons` | POST | Tạo lesson `{title}` |
| `/api/lessons/:id/cards` | POST | Lưu từ `{word, ipa?, partOfSpeech?, examples?, explain?, meanings?}` |
| `/api/dictionary/lookup?word=X&targetLang=vi` | GET | Tra từ bằng dictionary nội bộ của app (fallback) |

## Cấu trúc thư mục

```
extension/
├── manifest.json           # Chrome Extension config (MV3)
├── icons/                  # Extension icons
├── src/
│   ├── popup/              # Popup UI (popup.html/js/css)
│   ├── background/         # Service Worker (background.js)
│   └── content/            # Content Script (content.js/css)
└── test/
    └── contract-test.mjs   # Script verify API contract với backend thật
```

## Phát triển

```bash
# Không cần build - vanilla JS
# Sau khi thay đổi code, reload extension trong chrome://extensions

# Verify contract với backend thật (node >= 18):
cargo build -p kaen                     # từ repo SemaClaw
PORT=4505 KAEN_DATA_DIR=$(mktemp -d) ./target/debug/kaen &
node apps/kaen/extension/test/contract-test.mjs http://localhost:4505/api
```

## Troubleshooting

### Popup báo "Kaen offline"

- Kiểm tra app Kaen đang chạy (SenClaw daemon → Space App Kaen, port 4500)
- Kiểm tra Kaen App URL trong Settings đúng (`http://localhost:4500/api`)

### Extension không tra được từ

- Kiểm tra kết nối internet (Cambridge/Google Translate)
- Cambridge có thể block IP, extension sẽ tự fallback sang nguồn khác

## License

MIT
