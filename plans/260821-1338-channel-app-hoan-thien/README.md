# Hoàn thiện channel_app

**Ngày:** 2026-08-21 · **Nhánh:** main · **Trạng thái:** xong

## Bối cảnh

`channel_app` là Flutter remote-control client, nói chuyện với daemon **chỉ** qua
relay hub mã hoá. Mọi `/api/*` đi qua `CTRL_API_REQ/RESP` và được daemon replay
lại đúng axum router (`src/gateway/ui_server/relay_bridge.rs`).

**Đã đo:** relay bridge **không có allowlist route** — mọi `/api/*` đều tunnel
được. Nên phần bù parity là thuần Flutter.

**Đã đo:** `broadcast()` (`src/gateway/websocket_gateway/gateway.rs:273`) mirror
mọi jid `app:*` sang `app_event_sink` → relay. Cả 4 hàm `notify_workbench_*`
gọi `broadcast(chat_jid, …)`, nên **workbench event đã tới app channel rồi** —
ghi chú "workbench chỉ tới WS admin" trong memory đã lỗi thời.

`notify_dispatch_update` thì vẫn chỉ `broadcast_to_admins` → dispatch cần sửa Rust.

## Phase

| # | Việc | Đụng Rust | Trạng thái |
|---|------|-----------|------------|
| 1 | Patterns (`/api/patterns/*`) | không | xong |
| 2 | Usage (`/api/usage/*`) | không | xong |
| 3 | Kits (`/api/kits*`) | không | xong |
| 4 | Workbench (event đã có sẵn) | không | xong |
| 5 | Dispatch | **có** (`GET /api/dispatch`) | xong |
| 6 | Cowork team chat (`cowork:<id>`) | không | **chỉ đọc** — xem dưới |
| 7 | Chất lượng & đóng gói | không | xong |

## Hai quyết định đáng ghi

**Dispatch — thêm endpoint đọc, KHÔNG forward event.** Phương án còn lại là
đẩy `dispatch:update` sang app channel. Bỏ vì: đẩy cả cây parents tới mọi máy
đã ghép đôi ở mỗi lần đổi trạng thái dù không ai mở màn hình, mà vẫn *mất* — chu
kỳ reconnect của relay nuốt event, client sẽ thấy cây dở dang và không có cách
đối soát. Nên thêm `DispatchBridge::parents_snapshot()` +
`GET /api/dispatch`, màn hình poll (5s khi đang chạy, 30s khi rảnh) — đúng
kiểu màn Tác vụ nền đã làm với `bg:*`.

**Cowork team chat — chỉ làm nửa đọc.** Đọc chạy ngay: team là group
`cowork:<id>` và `GET /api/chat/history` nhận mọi jid. Gửi thì không: đường ra
chọn kênh bằng `Channel::owns_jid`, mà app channel chỉ khớp
`app:<channelId>:user:<sender>`. Cho nó nhận `cowork:*` là đổi quyền sở hữu
kênh **toàn daemon** — 7 vòng lặp gửi trong `lib.rs` đều lấy kênh khớp đầu
tiên, nên app sẽ cướp tin của Telegram và các kênh khác. Không đáng đổi để lấy
một ô nhập liệu.

## Kết quả verify

- `flutter analyze` — sạch
- `flutter test` — 37/37 xanh (1 smoke + 36 test parse model mới)
- `flutter build apk --debug` — ✓ `build/app/outputs/flutter-apk/app-debug.apk`
- `cargo test --lib` — 2145 passed, 0 failed (gồm 2 test mới cho `parents_snapshot`)

## Acceptance

- `flutter analyze` sạch
- `flutter test` xanh, có test parse cho mọi model mới
- `flutter build apk --debug` (hoặc web) thành công
- `cargo test` xanh nếu có sửa Rust
- README channel_app phản ánh đúng surface mới

## Non-goal

- Cấu hình daemon (LLM/embedding/oauth/provider-catalog) — README nói rõ app
  này không phải chỗ quản trị.
- E2E với daemon sống (user chọn mức verify analyze+test+build).
