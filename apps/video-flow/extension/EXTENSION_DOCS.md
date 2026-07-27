# Video Flow — Chrome Extension: Tài liệu kỹ thuật

## Tổng quan

Video Flow (SenClaw) là một Chrome Extension (Manifest V3) đóng vai trò **bridge** giữa SenClaw Space App `video-flow` (Rust/axum) và Google Flow API (`labs.google`). Extension chạy trong trình duyệt vì:

1. Google Flow yêu cầu Bearer token `ya29.*` (OAuth của user) — chỉ có trong browser session
2. Google Flow dùng **reCAPTCHA Enterprise** để bảo vệ mỗi API call — phải giải trong browser context
3. Các request tới `aisandbox-pa.googleapis.com` phải đến từ domain `labs.google` (CORS)

---

## Kiến trúc tổng thể

```
╔══════════════════════════════════════════════════════════════════╗
║                        Chrome Browser                           ║
║                                                                  ║
║  ┌──────────────────┐    WS ws://127.0.0.1:9222    ┌──────────┐ ║
║  │  background.js   │◄───────────────────────────►│  Go      │ ║
║  │  (Service Worker)│                              │  Backend │ ║
║  │                  │─── HTTP POST /api/ext/ ─────►│  :4460   │ ║
║  │  ┌────────────┐  │         callback             │          │ ║
║  │  │ Request Log│  │                              └──────────┘ ║
║  │  │ (max 100)  │  │                                           ║
║  │  └────────────┘  │                                           ║
║  └───────┬──────────┘                                           ║
║          │                                                       ║
║          │ chrome.tabs.sendMessage (GET_CAPTCHA)                 ║
║          │ chrome.runtime.sendMessage (TRPC_MEDIA_URLS)          ║
║          │                                                       ║
║  ┌───────▼──────────────────────────────────────┐               ║
║  │  labs.google Tab                             │               ║
║  │                                              │               ║
║  │  ┌─────────────────────────────────────┐    │               ║
║  │  │  content.js  (Isolated World)       │    │               ║
║  │  │  - Bridge message ↔ CustomEvent     │    │               ║
║  │  │  - Forward TRPC_MEDIA_URLS          │    │               ║
║  │  └──────────┬──────────────────────────┘    │               ║
║  │             │ window.dispatchEvent           │               ║
║  │             │ window.addEventListener        │               ║
║  │  ┌──────────▼──────────────────────────┐    │               ║
║  │  │  injected.js  (MAIN World)          │    │               ║
║  │  │  - window.grecaptcha.enterprise     │    │               ║
║  │  │  - monkey-patch window.fetch        │    │               ║
║  │  └─────────────────────────────────────┘    │               ║
║  └──────────────────────────────────────────────┘               ║
║                                                                  ║
║  ┌──────────────┐     ┌────────────────────────┐                ║
║  │  popup.html  │     │   side_panel.html       │                ║
║  │  (mini log)  │     │   (full dashboard)      │                ║
║  └──────────────┘     └────────────────────────┘                ║
╚══════════════════════════════════════════════════════════════════╝
```

---

## Các file & vai trò

| File | Loại | Vai trò |
|------|------|---------|
| `manifest.json` | Config | Khai báo permissions, content scripts, rules |
| `background.js` | Service Worker | Não chính: WS client, token capture, API proxy |
| `content.js` | Content Script | Bridge injected↔background qua CustomEvents |
| `injected.js` | Injected Script | Chạy trong MAIN world — gọi grecaptcha, patch fetch |
| `rules.json` | DNR Rules | Giả mạo Referer/Origin cho CORS |
| `popup.html/js` | Popup UI | Mini log viewer khi click icon extension |
| `side_panel.html/js` | Side Panel | Dashboard đầy đủ: metrics, log, preview media |

---

## Luồng hoạt động chi tiết

### 1. Khởi động & Lifecycle

```
Chrome khởi động / Extension install
    │
    ▼
onInstalled / onStartup
    │
    ▼
init()
    ├── chrome.storage.local.get(['flowKey', 'metrics', 'callbackSecret'])
    │       └── Khôi phục state từ lần trước
    ├── connectToAgent()
    └── chrome.alarms.create('keepAlive', { periodInMinutes: 0.4 })
                                              (mỗi 24 giây)

                    ┌─────────────────────────────────┐
                    │         Alarm Loop               │
                    │  keepAlive ──► ping / reconnect  │
                    │  reconnect ──► connectToAgent()  │
                    │  token-refresh ──► captureToken  │
                    └─────────────────────────────────┘
```

### 2. Kết nối WebSocket tới Go agent

```
connectToAgent()
    │
    ├─[guard]─ manualDisconnect? → return
    ├─[guard]─ ws.readyState === CONNECTING? → return
    ├─[guard]─ ws.readyState === OPEN? → return
    │
    ▼
new WebSocket('ws://127.0.0.1:9222')
    │
    ├─ onopen ──────────────────────────────────────────────────┐
    │       │                                                   │
    │       ├── alarms.clear('reconnect')                       │
    │       ├── setState('idle')  →  badge ● #22c55e xanh       │
    │       ├── alarms.create('token-refresh', 45 phút)         │
    │       ├── send { type: 'extension_ready',                 │
    │       │          flowKeyPresent, tokenAge }               │
    │       └── nếu flowKey → send { type: 'token_captured' }   │
    │                                                           │
    ├─ onmessage ────────────────────────────────────────────── │
    │       └── dispatch: api_request / trpc_request /          │
    │                     solve_captcha / get_status /          │
    │                     callback_secret / pong                │
    │                                                           │
    ├─ onclose ─────────────────────────────────────────────── ─┘
    │       ├── setState('off')  →  badge ○ xám
    │       ├── alarms.clear('token-refresh')
    │       └── scheduleReconnect()
    │               └── alarms.create('reconnect', delay: 5s)
    │
    └─ onerror
            └── metrics.lastError = 'WS_ERROR'

keepAlive (mỗi 24 giây):
    ├── ws.OPEN   → send { type: 'ping' }
    └── ws.closed → connectToAgent()
```

### 3. Thu thập Bearer Token

```
Mọi request ra ngoài trình duyệt
    │
    ▼
webRequest.onBeforeSendHeaders
    urls: ['https://aisandbox-pa.googleapis.com/*',
           'https://labs.google/*']
    │
    ├─[filter]─ Không có header Authorization? → skip
    ├─[filter]─ Không bắt đầu bằng 'Bearer ya29.'? → skip
    │
    ▼
flowKey = token  ←── cắt bỏ 'Bearer ' prefix
metrics.tokenCapturedAt = Date.now()
chrome.storage.local.set({ flowKey, metrics })
    │
    └── ws.OPEN? → send { type: 'token_captured', flowKey }

─────────────────────────────────────────────

Auto refresh (alarm 'token-refresh', mỗi 45 phút):
    │
    ▼
captureTokenFromFlowTab()
    │
    ├── chrome.tabs.query({ url: 'https://labs.google/fx/tools/flow*' })
    │
    ├─[có tab]──► chrome.scripting.executeScript({ files: ['content.js'] })
    │                   └── content.js inject → page gửi request → token bị bắt
    │
    └─[không có tab]──► _openingFlowTab guard (chống double-open)
                            ├── chrome.tabs.create({ url: 'labs.google/fx/tools/flow',
                            │                        active: false })
                            ├── sleep(3000ms)
                            └── retry executeScript trên tab mới
```

### 4. Giải reCAPTCHA (luồng quan trọng nhất)

```
Go Backend
    │
    │  WS: { method: 'solve_captcha', id, params: { captchaAction } }
    ▼
background.js: handleSolveCaptcha()
    │
    ▼
solveCaptcha(requestId, captchaAction)
    │
    ├── chrome.tabs.query({ url: 'labs.google/fx/tools/flow*' })
    │
    ├─[không có tab]──► tự mở tab → sleep(3s) → retry
    │                   thất bại → return { error: 'NO_FLOW_TAB' }
    │
    └─[có tab]──► requestCaptchaFromTab(tabId, requestId, captchaAction)
                    │
                    ├─[content.js chưa inject]──► executeScript({ files: ['content.js'] })
                    │                             sleep(200ms) → retry
                    │
                    └── chrome.tabs.sendMessage(tabId, {
                              type: 'GET_CAPTCHA', requestId, pageAction
                        })
                                │
                     ┌──────────▼────────────────────────┐
                     │         content.js                 │
                     │  (Isolated World)                  │
                     │                                    │
                     │  1. Đăng ký listener:              │
                     │     window.addEventListener(       │
                     │       'CAPTCHA_RESULT', handler)   │
                     │                                    │
                     │  2. Dispatch sang MAIN world:      │
                     │     window.dispatchEvent(          │
                     │       'GET_CAPTCHA',               │
                     │       { requestId, pageAction })   │
                     │                                    │
                     │  3. Timeout: 25 giây               │
                     └──────────┬────────────────────────┘
                                │ CustomEvent 'GET_CAPTCHA'
                     ┌──────────▼────────────────────────┐
                     │         injected.js                │
                     │  (MAIN World)                      │
                     │                                    │
                     │  1. waitForGrecaptcha()            │
                     │     poll 200ms, timeout 10s        │
                     │     until window.grecaptcha        │
                     │       .enterprise.execute exists   │
                     │                                    │
                     │  2. grecaptcha.enterprise.execute( │
                     │       SITE_KEY,                    │
                     │       { action: pageAction }       │
                     │     )  ← async, ~500ms             │
                     │                                    │
                     │  3. window.dispatchEvent(          │
                     │       'CAPTCHA_RESULT',            │
                     │       { requestId, token })        │
                     └──────────┬────────────────────────┘
                                │ CustomEvent 'CAPTCHA_RESULT'
                     ┌──────────▼────────────────────────┐
                     │         content.js                 │
                     │  Nhận CAPTCHA_RESULT               │
                     │  reply({ token }) → background.js  │
                     └───────────────────────────────────┘
                                │
                     background.js nhận token
                                │
    ┌───────────────────────────┘
    │
    ├── metrics.requestCount++
    ├── token ok? → metrics.successCount++
    │   token fail? → metrics.failedCount++
    │
    └── sendToAgent({ id, result: { token } })

SITE_KEY: 6LdsFiUsAAAAAIjVDZcuLhaHiDn5nnHVXVRQGeMV
Timeout tổng: 30 giây (Promise.race)
```

### 5. Proxy API Request (generate image/video)

```
Go Backend
    │
    │  WS: { method: 'api_request', id, params: {
    │          url: 'https://aisandbox-pa.googleapis.com/...',
    │          method: 'POST',
    │          headers: { 'Content-Type': 'application/json', 'x-goog-api-key': API_KEY },
    │          body: { clientContext: { recaptchaContext: { token: '' } }, ... },
    │          captchaAction: 'VIDEO_GENERATION'
    │        }}
    ▼
handleApiRequest()
    │
    ├─[validate]─ url missing? → sendToAgent({ id, error: 'MISSING_URL' })
    ├─[validate]─ url không phải aisandbox? → sendToAgent({ id, error: 'INVALID_URL' })
    │
    ├── setState('running')  →  badge ▶ vàng
    ├── metrics.requestCount++  (nếu có captchaAction)
    ├── addRequestLog({ type, status: 'processing', ... })
    │
    │   ┌─────────── STEP 1: Giải CAPTCHA ──────────────────┐
    ├── │  captchaAction?                                   │
    │   │      └── solveCaptcha()  (xem luồng 4)            │
    │   │      captchaToken = result.token                  │
    │   │      thất bại → sendToAgent 403, return           │
    │   └───────────────────────────────────────────────────┘
    │
    │   ┌─────────── STEP 2: Inject token vào body ─────────┐
    ├── │  deep clone body                                  │
    │   │  body.clientContext.recaptchaContext.token        │
    │   │    = captchaToken                                 │
    │   │  (cũng patch body.requests[*].clientContext      │
    │   │   nếu là batch request)                          │
    │   └───────────────────────────────────────────────────┘
    │
    │   ┌─────────── STEP 3: Auth ──────────────────────────┐
    ├── │  fetchHeaders['authorization'] = 'Bearer {flowKey}'│
    │   │  flowKey missing? → sendToAgent 503, return       │
    │   └───────────────────────────────────────────────────┘
    │
    │   ┌─────────── STEP 4: Fetch từ browser context ──────┐
    ├── │  fetch(url, {                                     │
    │   │    method: 'POST',                               │
    │   │    headers: fetchHeaders,                        │
    │   │    credentials: 'include',  ← dùng browser cookie│
    │   │    body: JSON.stringify(finalBody)               │
    │   │  })                                              │
    │   └───────────────────────────────────────────────────┘
    │
    ├── Parse response (JSON hoặc plain text)
    ├── Extract outputUrl từ GCS regex
    ├── updateRequestLog(success/failed)
    ├── update metrics
    ├── setState('idle')
    └── sendToAgent({ id, status, data })

URL → log type mapping:
  uploadImage             → UPLOAD
  batchGenerateImages     → GEN_IMG     ← visible
  UpsampleVideo           → UPSCALE     ← visible
  ReferenceImages         → GEN_VID_REF ← visible
  batchAsyncGenerateVideo → GEN_VID     ← visible
  batchCheckAsync         → POLL
  upsampleImage           → UPS_IMG
  /media/                 → MEDIA
  /credits                → CREDITS
```

### 6. Proxy tRPC Request

```
Go Backend
    │
    │  WS: { method: 'trpc_request', id, params: {
    │          url: 'https://labs.google/fx/api/trpc/...',
    │          method: 'POST',
    │          body: { ... }
    │        }}
    ▼
handleTrpcRequest()
    │
    ├─[validate]─ url không bắt đầu 'https://labs.google/'?
    │               → sendToAgent({ id, error: 'INVALID_TRPC_URL' })
    │
    ├── setState('running')
    ├── fetchHeaders['authorization'] = 'Bearer {flowKey}'
    ├── fetch(url, { credentials: 'include', body: JSON.stringify(body) })
    ├── setState('idle')
    └── sendToAgent({ id, status, data })

Note: tRPC calls không hiện trong request log (silent),
      không tính vào metrics (không dùng captcha)
```

### 7. Gửi phản hồi về Go Backend (dual-channel)

```
sendToAgent(msg)
    │
    ├─[msg.id có giá trị]─────────────────────────────────┐
    │                                                     │
    │  HTTP POST http://127.0.0.1:4460/api/ext/callback   │
    │  body: JSON.stringify(msg)                          │
    │       │                                             │
    │       ├── OK → done                                 │
    │       └── FAIL → fallback: ws.send(msg)             │
    │                                                     │
    └─[msg.id không có]──────────────────────────────────┘
         (ping, status, token_captured)
         ws.OPEN → ws.send(msg)

Lý do dual-channel: Service Worker có thể bị Chrome
terminate; HTTP callback đảm bảo delivery kể cả khi
WS bị ngắt trong lúc đang xử lý request dài.
```

### 8. Thu thập Media URL từ tRPC (passive monitor)

```
[injected.js — hoạt động ngầm mọi lúc]

window.fetch = async function(...args) {
    response = await _originalFetch(...args)
    │
    ├─ url chứa '/fx/api/trpc/' && response.ok?
    │       │
    │       └── response.clone().text()
    │               │
    │               └── body chứa 'storage.googleapis.com/ai-sandbox-videofx/'?
    │                       │
    │                       └── window.dispatchEvent('TRPC_MEDIA_URLS', { url, body })
    │
    return response  ← không ảnh hưởng original behavior
}
        │
        │ CustomEvent 'TRPC_MEDIA_URLS'
        ▼
[content.js]
window.addEventListener('TRPC_MEDIA_URLS', (e) => {
    chrome.runtime.sendMessage({ type: 'TRPC_MEDIA_URLS', body })
})
        │
        ▼
[background.js] handleTrpcMediaUrls(trpcUrl, bodyText)
    │
    ├── Regex: /https:\/\/storage\.googleapis\.com\/ai-sandbox-videofx\/(image|video)\/[uuid]\?[params]/g
    ├── Unescape: & → & (JSON escaped ampersand)
    ├── Dedup theo mediaId UUID (giữ lại URL mới nhất)
    │
    └── ws.send({ type: 'media_urls_refresh',
                  urls: [{ mediaType, url, mediaId }] })

Mục đích: GCS signed URL hết hạn → Go backend cập nhật DB
          với URL mới nhất mỗi khi user mở project trên browser.
```

### 9. Telemetry giả lập hành vi người dùng

```
scheduleTelemetry()  ← gọi đệ quy, delay random 45-120 giây
    │
    ▼
sendTelemetry()
    │
    ├─[guard]─ !flowKey || state === 'off' → skip
    │
    ├─[50% random]──► POST aisandbox-pa.googleapis.com/v1:batchLog
    │                 body: { appEvents: [
    │                   { event: 'FLOW_IMAGE_LATENCY' | 'FLOW_VIDEO_LATENCY',
    │                     eventProperties: [
    │                       { key: 'CURRENT_TIME_MS', doubleValue: Date.now() },
    │                       { key: 'DURATION_MS', doubleValue: random(150-800) },
    │                       { key: 'USER_AGENT', stringValue: navigator.userAgent },
    │                       { key: 'IS_DESKTOP', booleanValue: true }
    │                     ],
    │                     eventMetadata: { sessionId },
    │                     eventTime: ISO8601
    │                   }
    │                 ]}
    │
    └─[50% random]──► POST aisandbox-pa.googleapis.com/v1/flow:batchLogFrontendEvents
                      body: { events: [
                        { eventType: one_of(
                            FLOW_IMAGE_LATENCY, FLOW_VIDEO_LATENCY,
                            GRID_SCROLL_DEPTH, FLOW_PROJECT_OPEN, FLOW_SCENE_VIEW),
                          metadata: { sessionId, createTime, additionalParams }
                        }
                      ]}

Session ID: ";{timestamp}" — reset ngẫu nhiên mỗi 25-35 phút
            (giống user mở tab mới sau một thời gian không dùng)

Không log vào requestLog, không đếm vào metrics.
```

### 10. CORS Bypass (rules.json — Declarative Net Request)

```
Browser gửi request tới aisandbox-pa.googleapis.com
    │
    ├─[trước khi gửi]──► DNR Rule áp dụng:
    │
    │  Điều kiện: urlFilter = "aisandbox-pa.googleapis.com"
    │             resourceTypes = ["xmlhttprequest"]
    │
    │  Hành động: modifyHeaders
    │    ├── Referer: https://labs.google/      (giả vờ gọi từ labs.google)
    │    └── Origin:  https://labs.google       (vượt CORS check)
    │
    └─► Server nhận request với Origin hợp lệ → trả về response
```

---

## rules.json — Header Injection (declarativeNetRequest)

```json
Điều kiện: request tới aisandbox-pa.googleapis.com (xmlhttprequest)
Hành động:
    - Set Referer: https://labs.google/
    - Set Origin: https://labs.google
```

Mục đích: Bypass CORS check trên phía server — server chỉ chấp nhận request từ labs.google.

---

## Permissions giải thích

| Permission | Lý do |
|-----------|-------|
| `storage` | Lưu flowKey, metrics, callbackSecret |
| `alarms` | keepAlive, reconnect, token-refresh timers |
| `tabs` | Query/tạo tab labs.google, inject scripts |
| `webRequest` | Bắt Authorization header để lấy token |
| `scripting` | Inject content.js vào tab khi cần |
| `declarativeNetRequest` | Modify Referer/Origin headers |
| `sidePanel` | Mở Side Panel UI |
| host: `labs.google/*` | Đọc request headers, inject scripts |
| host: `aisandbox-pa.googleapis.com/*` | Intercept API requests |
| host: `127.0.0.1/*` | HTTP callback về app (cổng cấu hình được) |

---

## State Machine

```
                          init() / alarm reconnect
                                   │
                    ┌──────────────▼─────────────┐
                    │             OFF             │
                    │   badge: ○  color: #6b7280  │
                    │   (xám — không kết nối)     │
                    └──────────────┬──────────────┘
                                   │ WS onopen
                    ┌──────────────▼─────────────┐
          ┌────────►│            IDLE             │◄──────────┐
          │         │   badge: ●  color: #22c55e  │           │
          │         │   (xanh — sẵn sàng)         │           │
          │         └──────────────┬──────────────┘           │
          │                        │ api_request /             │
          │                        │ trpc_request /            │
          │                        │ solve_captcha             │
          │         ┌──────────────▼─────────────┐            │
          │         │           RUNNING           │            │
          │         │   badge: ▶  color: #f59e0b  │────────────┘
          │         │   (vàng — đang xử lý)       │  request complete
          │         └─────────────────────────────┘
          │
          │ manualDisconnect (DISCONNECT msg)
          │ hoặc WS onclose
          └─────── OFF ◄──────────────────────────

Transitions:
  OFF → IDLE:     WS kết nối thành công (onopen)
  IDLE → RUNNING: Nhận request từ Go backend
  RUNNING → IDLE: Request hoàn thành (success hoặc error)
  ANY → OFF:      WS đóng hoặc user toggle OFF
  OFF → IDLE:     User toggle ON (RECONNECT) hoặc auto-reconnect

manualDisconnect flag:
  DISCONNECT msg → manualDisconnect=true  → ngừng auto-reconnect
  RECONNECT msg  → manualDisconnect=false → cho phép reconnect
```

---

## Message API (internal)

### background.js → popup/side_panel (push)

| Message type | Nội dung |
|-------------|---------|
| `STATUS_PUSH` | Thông báo state thay đổi → UI tự fetch |
| `REQUEST_LOG_UPDATE` | `{ log: [...] }` — log mới nhất |

### popup/side_panel → background.js (request)

| Message type | Tham số | Phản hồi |
|-------------|---------|---------|
| `STATUS` | — | `{ connected, state, flowKeyPresent, tokenAge, metrics }` |
| `DISCONNECT` | — | `{ ok: true }` |
| `RECONNECT` | — | `{ ok: true }` |
| `REQUEST_LOG` | — | `{ log: [...] }` |
| `OPEN_FLOW_TAB` | — | `{ ok, tabId }` |
| `DELETE_LOG_ENTRY` | `{ id }` | `{ ok }` |
| `CLEAR_LOG` | — | `{ ok }` |
| `REFRESH_TOKEN` | — | `{ ok }` |
| `TEST_CAPTCHA` | `{ pageAction }` | `{ token } \| { error }` |
| `TRPC_MEDIA_URLS` | `{ trpcUrl, body }` | `{ ok }` |

### Go backend → Extension (WS)

| method | params | Mô tả |
|--------|--------|-------|
| `api_request` | `{ url, method, headers, body, captchaAction? }` | Proxy API call |
| `trpc_request` | `{ url, method, headers, body }` | Proxy tRPC call |
| `solve_captcha` | `{ captchaAction }` | Chỉ giải captcha, không gọi API |
| `get_status` | — | Trả về state hiện tại |

### Extension → Go backend (WS/HTTP)

| type/path | Nội dung |
|-----------|---------|
| WS `extension_ready` | `{ flowKeyPresent, tokenAge }` |
| WS `token_captured` | `{ flowKey }` |
| WS `ping` | keepalive |
| WS `media_urls_refresh` | `{ urls: [{ mediaType, url, mediaId }] }` |
| HTTP POST `/api/ext/callback` | `{ id, status, data } \| { id, error }` |

---

## Request Log Entry Schema

```js
{
  id: string,           // UUID từ Go backend
  type: string,         // GEN_IMG | GEN_VID | GEN_VID_REF | UPSCALE | ...
  time: ISO8601,        // Thời điểm nhận request
  status: string,       // 'processing' | 'success' | 'failed'
  error: string|null,   // Mô tả lỗi nếu có
  url: string,          // API endpoint URL
  payloadSummary: string,    // 200 ký tự đầu của request body
  responseSummary: string,   // 300 ký tự đầu của response
  httpStatus: number,        // HTTP status code
  outputUrl: string|null,    // GCS URL của media output
}
```

---

## UI Components

### Popup (`popup.html`)
- 360px wide, compact
- Danh sách request log có thể expand từng entry
- Nút "Side Panel" để mở full dashboard

### Side Panel (`side_panel.html`)
- Full-height panel bên phải browser
- **Header**: logo + connection dot (nhấp nháy khi connected) + ON/OFF toggle
- **Metrics**: 3 ô — Total / Done / Failed (màu xanh/đỏ)
- **Status bar**: state badge + token age
- **Request log table**: ID | Type | Time | Status | Output | Delete
  - Click ID → Detail modal (fields + media preview)
  - Click thumbnail/▶ → Preview modal (ảnh/video full size)
  - Click × → Xóa entry
- **Bottom bar**: "Open Flow Tab" + "Refresh Token"

---

## Các kỹ thuật đặc biệt

### Monkey-patching `window.fetch`
`injected.js` override `window.fetch` global để tự động intercept tất cả TRPC responses — không cần user làm gì, hoạt động trong suốt.

### Injected script vào MAIN world
Content scripts chạy trong isolated world — không có `window.grecaptcha`. Extension inject `injected.js` bằng thẻ `<script>` DOM để chạy trong MAIN world, nơi page đã load reCAPTCHA.

### Dual-channel response (HTTP + WS fallback)
Responses quan trọng (có ID) được gửi qua HTTP để đảm bảo delivery ngay cả khi WS bị ngắt. WS chỉ là fallback.

### Auto-reconnect với Chrome Alarms
Service Worker có thể bị terminate bởi Chrome. Alarm API đảm bảo wake-up định kỳ và reconnect.
