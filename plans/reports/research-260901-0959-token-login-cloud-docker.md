# Token login cho Web + Desktop, chạy an toàn trên Cloud/Docker

Ngày 2026-09-01 · nhánh `main` · phạm vi: `src/gateway/ui_server/auth.rs`, `web/`, `desktop_app/`, deployment.

**Trạng thái: ĐÃ TRIỂN KHAI** (nghiên cứu bên dưới giữ nguyên làm hồ sơ quyết định).

## TL;DR

Token login đã có sẵn từ 2026-08-06 (web `TokenGate`, desktop ô token, loopback miễn trừ).
Ba yêu cầu mới đều là phần thiếu, nay đã làm xong:

| Yêu cầu | Trước | Sau |
|---|---|---|
| Web + desktop token login, local không hỏi | ✅ | giữ nguyên (`auto` là mặc định) |
| Cloud bắt buộc token | ❌ reverse proxy cùng máy làm mọi khách thành loopback | ✅ `SENCLAW_AUTH_MODE=always` |
| Bật/tắt được | ❌ suy từ bind host | ✅ tri-state `auto`/`always`/`off`, đổi live ở UI |
| Docker | ❌ không có Dockerfile | ✅ `Dockerfile` + `docker-compose.yml` |

### Đã ship

- **Tri-state `SENCLAW_AUTH_MODE`** (`auto` mặc định | `always` | `off`); giá trị lạ → `auto`,
  không bao giờ `off`. Override runtime ở `GET`/`PUT /api/auth/mode` (`router_state` khoá
  `auth:mode`), không cần restart. Route này **có** gác token.
- **`authorize()` chỉ miễn loopback khi mode ≠ `always`** — thứ tự này bị test guard chốt.
- **Caller loopback nội bộ**: token phát qua `util::internal_auth` (OnceLock, **không**
  `env::set_var` — daemon đã đa luồng ở thời điểm đó); MCP con nhận qua env map của config
  (`mcp::helper::with_daemon_token`); `query_llm`, `cognitive::llm_openai`, `kanban::llm_info`
  gắn `X-SenClaw-Token` khi URL là route `/api/` loopback của chính daemon.
- **Cookie `Secure`** suy từ `X-Forwarded-Proto`, ép bằng `SENCLAW_AUTH_COOKIE_SECURE`.
- **UI**: web `ApiAuthModeCard` (Settings → General) + TokenGate đổi lời theo mode;
  desktop `ApiAuthModeField` + chuỗi tiếng Việt.
- **Docker**: image bind `0.0.0.0`, mode `always`, non-root, healthcheck `/api/auth/status`,
  volume `~/.senclaw`, publish 18788 + 18789.

### Kiểm chứng

| | |
|---|---|
| `cargo test --lib` | 2154 pass / 0 fail |
| `cargo test --test daemon_auth_guard` | 9 pass (4 test mới: thứ tự loopback-vs-mode, fallback typo, route bị gác, 2 bất biến Dockerfile) |
| web `tsc --noEmit` | sạch |
| `flutter analyze` + `flutter test api_token_test network_bind_test` | sạch, 10 pass |
| **E2E daemon thật** (loopback, `always`) | 25 check pass: loopback không token → 401, có token → 200, cookie đủ, `Secure` chỉ khi `X-Forwarded-Proto: https`, đổi mode live có hiệu lực ngay, typo → 400, SPA shell vẫn mở |

**Chưa kiểm chứng**: `docker build` — Docker daemon không chạy trên máy này. Dockerfile chỉ được
chốt bằng test tĩnh (healthcheck dùng route mở, không set `SENCLAW_BIND_HOST=0.0.0.0`).

---

## 1. Hiện trạng (đã xác minh trong source)

**Daemon** — [src/gateway/ui_server/auth.rs](src/gateway/ui_server/auth.rs)

- `ApiAuth { required, token }`; `required = !is_loopback_host(bind_host)` — quyết định
  duy nhất ở [src/lib.rs:2224](src/lib.rs:2224).
- Token: `SENCLAW_API_TOKEN` → `~/.senclaw/api_token` (32 byte hex, chmod 0600).
- Nhận qua `Authorization: Bearer`, `X-SenClaw-Token`, `?token=`, cookie `senclaw_token`.
- So sánh constant-time; `peer = None` fail-closed.
- HTTP mw gác `/api/*` trừ `/api/auth/{login,status}`; WS mw gác **mọi** path lúc upgrade.
- CORS chỉ cho origin loopback (đã bỏ `permissive()` từng rò `/api/llm-config`).

**Web** — [web/src/lib/auth.ts](web/src/lib/auth.ts) patch `window.fetch` gắn header cho
`/api/*` same-origin (thay vì sửa ~180 call-site) + phát `UNAUTHORIZED_EVENT` khi 401;
[web/src/components/TokenGate.tsx](web/src/components/TokenGate.tsx) bọc `<App/>`, chỉ khoá
khi `authRequired && !authorized`.

**Desktop** — `AppConfig.apiToken`, thứ tự nguồn: prefs (Settings → General → Connection)
→ `--dart-define` → `~/.senclaw/api_token`. Có sẵn công tắc Private/Public ở
Settings → General → Network access. Test: `desktop_app/test/api_token_test.dart`.

**Guard** — `tests/daemon_auth_guard.rs` chốt 3 điều: cấm CORS permissive, cấm hardcode
bind host, bắt buộc `into_make_service_with_connect_info`.

## 2. Lỗ hổng thật cho kịch bản cloud

`authorize()` trả `true` **ngay** khi `peer.ip().is_loopback()`, trước cả khi xét token.
Triển khai cloud chuẩn là nginx/Caddy terminate TLS rồi proxy về `127.0.0.1:18788` —
lúc đó **mọi khách từ Internet đều xuất hiện là loopback** ⇒ token không bao giờ được
đòi. docs/remote-access-security.md đã ghi nhận đây là "giới hạn cố hữu"; với yêu cầu
"chạy trên cloud khi đăng nhập cần token" thì nó là bug chặn.

Không có xử lý `X-Forwarded-For` ở đâu trong `src/` (đã grep) — nên hiện tại **không có
cách nào** bắt token sau reverse proxy.

Hệ quả phụ: cookie `senclaw_token` không có `Secure` (cố ý, vì LAN là HTTP trần) — sau
TLS thì thiếu.

## 3. Thiết kế đề xuất

### 3.1 Công tắc tri-state `SENCLAW_AUTH_MODE`

```
auto (mặc định) = hành vi hiện tại: required ⇔ bind host non-loopback, loopback miễn
always          = mọi peer phải có token, kể cả loopback  ← đáp án cho cloud/reverse proxy
off             = không bao giờ đòi                        ← docker network tin cậy, đã có auth tầng trên
```

Chọn `always` thay vì tin `X-Forwarded-For` vì không phải cấu hình danh sách proxy tin cậy,
không có bề mặt giả mạo header, và đúng cả khi proxy nằm cùng pod/sidecar.

Giá trị lạ → fallback **`auto`**, không bao giờ về `off` (cùng nguyên tắc với
`SENCLAW_APP_TOKEN_MODE`: typo không được âm thầm tắt bảo vệ).

Đổi runtime không cần restart: lưu ở `router_state` khoá `auth:mode`, route
`GET/PUT /api/auth/mode`, middleware đọc theo từng request — y hệt tiền lệ
`space:appTokenMode`. **Bẫy**: bật `always` từ phiên loopback sẽ tự khoá chính mình;
UI phải gọi `/api/auth/login` ngay sau khi PUT thành công.

### 3.2 Mode `always` bắt buộc kèm: bơm token cho mọi caller loopback nội bộ

Đây là phần đắt nhất và là lý do `always` không thể là mặc định. Danh sách caller gọi
ngược vào `/api/*` qua loopback (đã grep):

| Caller | Nơi | Hiện có token? |
|---|---|---|
| MCP `space` / `patterns` / `ocr` (subprocess) | [src/mcp/helper.rs:186](src/mcp/helper.rs:186) `SENCLAW_SPACE_API_URL` | chỉ khi env `SENCLAW_API_TOKEN` được set sẵn ([space_apps.rs:61](src/mcp/space_apps.rs:61)) |
| Kanban `llm_info` | [src/kanban/api.rs:895](src/kanban/api.rs:895) `SENCLAW_BASE_URL` | không |
| LLM của Space App (mlx-lm/candle) qua `/api/space/apps/*/proxy/v1` | [src/apps/llm_provider.rs](src/apps/llm_provider.rs), `query_llm.rs`, `memory/cognitive/llm_openai.rs` | không (`api_key` rỗng là **cố ý**) |
| Desktop app cùng máy | `token_file_io.dart` | có (đọc file) |

Cách rẻ nhất và gọn: sau `resolve_token()` ở startup, daemon `set_var("SENCLAW_API_TOKEN", token)`
cho chính process ⇒ mọi child kế thừa; đồng thời các HTTP client in-process gắn header.
Rủi ro: token lộ qua `/proc/<pid>/environ` — chỉ cùng user, ngang bằng mức bảo vệ hiện
tại của token Space App.

### 3.3 Cookie `Secure` sau TLS

Thêm `SENCLAW_AUTH_COOKIE_SECURE` (mặc định suy từ `X-Forwarded-Proto: https`), vì
`Secure` bật nhầm trên LAN HTTP trần sẽ làm cookie bị trình duyệt vứt ⇒ login "thành
công" mà vẫn 401.

### 3.4 Docker

Repo hiện **không có Dockerfile cho daemon** (chỉ `9router/` và `hub-backend/`). Cần thêm
`Dockerfile` + `docker-compose.yml` + mục trong docs. Các quyết định và bẫy:

- `SENCLAW_UI_BIND_HOST=0.0.0.0` **trong image** — bắt buộc để `-p` tới được. Guard test
  chỉ cấm hardcode trong `src/*.rs`, không đụng Dockerfile.
- `SENCLAW_AUTH_MODE=always` là **mặc định của image**: với `-p` thì peer là gateway bridge
  (non-loopback) nên `auto` cũng đủ, nhưng `--network host` hoặc sidecar proxy cùng pod sẽ
  thành loopback ⇒ `always` mới an toàn ở mọi cách chạy.
- **Volume `~/.senclaw` là bắt buộc.** Không mount thì token + SQLite sinh lại mỗi lần
  restart ⇒ cookie/localStorage đã lưu ở trình duyệt hoá vô hiệu, người dùng tưởng lỗi login.
- **Healthcheck phải trỏ `/api/auth/status`** (route mở). Trỏ `/api/config` sẽ 401 ⇒
  container flap unhealthy vô hạn.
- **Không set `SENCLAW_BIND_HOST=0.0.0.0`** trong compose: biến đó là của Space App, app
  không có auth riêng — set nhầm là phơi ~50 app ra network của container.
- Publish **cả hai** cổng 18788 và 18789: web dial WS theo `window.location.hostname`.
  Sau một reverse proxy duy nhất phải proxy cả hai (hoặc ghi rõ giới hạn này).
- Linux container ⇒ **không có MLX**: model local (mlx-lm/candle Space App) không dùng
  được; `crates/senclaw-media` (Whisper MLX) cũng không ⇒ voice chat/transcribe tắt.
  Phải nói rõ trong docs thay vì để người dùng debug.
- Runner `node`/`python` của Space App cần node + python trong image, nếu không `requires`
  sẽ chặn launch (đúng thiết kế, nhưng cần ghi chú).
- Chạy non-root; `~/.senclaw` 0700.

## 4. Phạm vi thay đổi ước lượng

| Phase | File | Ghi chú |
|---|---|---|
| 1. Tri-state mode | `src/config.rs`, `src/gateway/ui_server/auth.rs`, `src/lib.rs` | `AuthMode` enum + `authorize()` bỏ nhánh loopback khi `always` |
| 2. Route đổi runtime | `auth.rs`, `core.rs` | `GET/PUT /api/auth/mode` + `router_state` |
| 3. Bơm token nội bộ | `src/lib.rs`, `mcp/helper.rs`, `kanban/api.rs`, `apps/llm_provider.rs`, `zen_core/query_llm.rs`, `memory/cognitive/llm_openai.rs` | phần rủi ro nhất; cần test |
| 4. Cookie Secure | `auth.rs` | |
| 5. UI | `web/src/components/TokenGate.tsx` + panel Settings; `desktop_app/.../settings_screen.dart` | thêm công tắc, sửa copy "chỉ cần khi ngoài localhost" |
| 6. Docker | `Dockerfile`, `docker-compose.yml`, `docs/` | |
| 7. Test | `tests/daemon_auth_guard.rs`, `cargo test ui_server::auth`, `flutter test api_token_test.dart` | thêm case: `always` chặn cả loopback; typo mode → `auto` |

## 5. Còn lại

1. `docker build` chưa chạy được ở đây (Docker daemon tắt) — cần build thử một lần trên máy có Docker.
2. Image kèm sẵn node + python cho Space App runner; bỏ đi thì nhẹ hơn nhưng app dùng runner đó sẽ bị
   `requires` chặn.
3. Sau reverse proxy vẫn phải proxy cả 18788 lẫn 18789 (chưa gộp một cổng).
