# Mở SenClaw ra LAN an toàn (bind host + API token)

Mặc định daemon **chỉ bind loopback** — Web UI (18788) và WS gateway (18789)
đều nghe trên `127.0.0.1`, máy khác không gọi tới được. Muốn mở ra LAN phải
opt-in tường minh, và khi đó mọi peer không phải loopback bắt buộc kèm token.

## Bật truy cập từ xa

```bash
SENCLAW_UI_BIND_HOST=0.0.0.0 senclaw
```

Khi bind host không phải loopback, daemon tự bật chế độ token:

- Token đọc từ `SENCLAW_API_TOKEN` (env), nếu không có thì dùng/tự sinh
  `~/.senclaw/api_token` (32 byte ngẫu nhiên, hex, chmod `0600`).
- Log khởi động in đường dẫn file token (không in giá trị).
- **Peer loopback luôn được miễn token** — desktop app bundled, Space App gọi
  ngược về daemon, tooling cùng máy chạy y như cũ, không cần cấu hình gì.

`SENCLAW_UI_BIND_HOST` cố ý **tách khỏi** `SENCLAW_BIND_HOST` của Space App:
app không có auth riêng nên không được kéo theo daemon ra LAN (và ngược lại).
Truy cập app từ xa đi qua proxy `/api/space/...` của daemon — đã nằm sau token.

## Client gửi token thế nào

| Kênh | Cách gửi |
|---|---|
| REST | `Authorization: Bearer <token>` hoặc `X-SenClaw-Token: <token>` |
| WS upgrade (18789, `/api/ws/terminal`) | `?token=<token>` hoặc cookie |
| Trình duyệt (iframe Space App, WS) | cookie `senclaw_token` — mint bằng `POST /api/auth/login {token}` (HttpOnly, SameSite=Lax) |

Hai endpoint mở (không cần token, chỉ trả boolean/login):
`GET /api/auth/status` → `{authRequired, authorized}` và `POST /api/auth/login`.
`GET /api/config` có thêm trường `authRequired`.

- **Web UI**: tự hiện màn hình nhập token khi `authRequired && !authorized`
  (`web/src/components/TokenGate.tsx`); mọi `fetch` `/api/*` cùng origin được
  patch để kèm `X-SenClaw-Token` (`web/src/lib/auth.ts`), 401 → khoá lại gate.
- **Desktop app**: thứ tự nguồn token — Settings → General → Connection (prefs)
  → `--dart-define=SENCLAW_API_TOKEN` → `~/.senclaw/api_token` (cùng máy).
  Gắn header trong `ApiClient` + các call multipart, `?token=` cho WS.

## Vá kèm trong cùng thay đổi

- **CORS**: bỏ `CorsLayer::permissive()` (ACAO `*`) — trước đây *bất kỳ trang
  web nào* user đang mở cũng fetch được `http://127.0.0.1:18788/api/llm-config`
  và đọc API key cleartext. Giờ chỉ origin loopback (Vite dev...) được phép
  cross-origin; UI chính là same-origin nên không cần CORS.
- **WS gateway**: chặn ngay tại HTTP upgrade cho cả 3 route (`/`, `/browser`,
  `/browser-mcp`) — check `connect` in-band không đủ vì dispatcher vẫn chạy
  handler cho socket chưa auth.

## Giới hạn đã biết

- LAN là HTTP thường — token đi plaintext trên mạng nội bộ. Muốn qua Internet
  hãy đặt sau reverse-proxy TLS (khi đó cookie nên thêm `Secure` — chưa làm).
- So sánh token dùng constant-time; token 256-bit nên không cần rate-limit.
- Ảnh `NetworkImage` trong desktop app chưa gắn header — chỉ ảnh hưởng cấu hình
  desktop trỏ tới daemon từ xa, không ảnh hưởng mặc định loopback.

Test: `cargo test ui_server::auth` (Rust, 15 test middleware/token/cookie/CORS),
`flutter test test/api_token_test.dart` (desktop).
