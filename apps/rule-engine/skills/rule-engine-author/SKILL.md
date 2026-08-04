---
name: rule-engine-author
description: >-
  Dựng và sửa luồng xử lý dữ liệu dạng đồ thị (rule chain) trong Space App
  Rule Engine: chọn node nguồn, nối các node lọc/rẽ nhánh/biến đổi qua cổng
  vào–ra, rồi kích hoạt; hoặc dựng luồng để app khác gọi đồng bộ (request →
  respond). Dùng khi người dùng nói đại loại "tạo luồng tự động", "mỗi 5 phút
  gọi API rồi báo Telegram nếu nhiệt độ > 35", "thêm nhánh điều kiện vào luồng",
  "nối node A sang node B", "làm một endpoint tính giá", "build an automation
  flow", "create a rule chain". KHÔNG dùng khi người dùng chỉ muốn xem vì sao
  luồng không chạy — đó là việc của skill `rule-engine-debug`.
---

# rule-engine-author

Bạn dựng luồng bằng MCP của app Rule Engine (`rule-engine-mcp`). Mọi liên kết giữa
các node nằm ở `edges`; **tuyệt đối không nhét id node vào `config`**.

Tên tool đầy đủ là `mcp__rule-engine-mcp__<tool>` (ví dụ
`mcp__rule-engine-mcp__rule_registry`) — theo quy ước MCP của SenClaw, không rút gọn.

Chuẩn thiết kế sâu hơn: [`docs/rule-engine-build-standard.md`](../../../../docs/rule-engine-build-standard.md).

## Trình tự

1. **`rule_registry`** — đọc danh mục node **trước khi làm gì khác**. Trả về mọi
   loại node kèm cổng vào/ra và JSON Schema cấu hình *thật*. Đừng đoán tên node,
   tên cổng hay tên field config — sai sẽ bị chặn ở `rule_validate`. Cổng động
   (`switch` → theo `cases`; `join`/`merge` → theo `inputs`) được đánh dấu
   `dynamicInputs`/`dynamicOutputs`.
2. **Chọn node nguồn** — mỗi chain đúng một node `isSource: true` (bảng bên dưới).
3. **Tạo/chọn luồng** — `rule_create_chain`, hoặc `rule_list_chains` +
   `rule_get_chain` để sửa cái có sẵn.
4. **Ghi đồ thị** — `rule_update_graph` với `nodes` + `edges`.
5. **Kiểm tra** — `rule_validate`. Lỗi (`level: "error"`) phải sửa hết; cảnh báo
   (`warning`) đọc rồi tự quyết.
6. **Kích hoạt** — `rule_activate`.
7. **Chạy thử** — `rule_push`/`rule_trigger` (async), `rule_call` (đồng bộ), hoặc
   `rule_get` (đọc state). Xem trace bằng `rule_set_debug` + `rule_run_trace`.

`rule_generate` dựng nháp nhanh từ một câu mô tả (tự đọc registry, sinh graph,
validate, lưu — **không** tự kích hoạt). Vẫn phải đọc lại và `rule_validate`.

### Hình dạng payload của `rule_update_graph`

```json
{
  "chainId": 123,
  "nodes": [
    { "id": "n1", "rule": "schedule", "name": "Mỗi 5 phút",
      "config": { "cron": "0 */5 * * * *", "timezone": "Asia/Ho_Chi_Minh" }, "x": 0, "y": 0 },
    { "id": "n2", "rule": "http-request", "name": "Gọi API",
      "config": { "method": "GET", "url": "https://..." }, "x": 320, "y": 0 },
    { "id": "n3", "rule": "conditional", "name": "Nóng quá?",
      "config": { "expr": "temperature > 35" }, "x": 640, "y": 0 },
    { "id": "n4", "rule": "telegram-send", "name": "Báo",
      "config": { "botToken": "...", "chatId": "...", "message": "Nóng ${temperature} độ" },
      "x": 960, "y": -160 }
  ],
  "edges": [
    { "id": "e1", "from": {"node":"n1","port":"out"},     "to": {"node":"n2","port":"in"} },
    { "id": "e2", "from": {"node":"n2","port":"success"}, "to": {"node":"n3","port":"in"} },
    { "id": "e3", "from": {"node":"n3","port":"true"},    "to": {"node":"n4","port":"in"} }
  ]
}
```

- Id node tự đặt, ngắn, ổn định. `x` giãn **320** một bước, `y` giãn **160** giữa
  các nhánh (xem chuẩn §2).
- `opts` (tuỳ chọn) trên node: `join` (`any`/`all`/`merge`), `joinTimeoutMs`,
  `corrKey`, `concurrency`.

## Danh mục node đầy đủ

Cổng ra `error` **ngầm định** ở mọi node (không liệt lại). Cổng `arity: "one"`
chỉ nhận một cạnh; còn lại là `many` (fan-out, mỗi cạnh một bản sao). Field config
đánh dấu **(bắt buộc)** là `required`; còn lại tuỳ chọn, kèm mặc định nếu có.

### Nguồn (source) — mỗi chain đúng một, không có cổng vào

| rule | config chính | cổng ra | ghi chú |
|---|---|---|---|
| `manual` | *(không có)* | `out` | Bấm "Chạy thử" / `rule_push` / `rule_trigger`. Dữ liệu gửi kèm ra thẳng `out`. |
| `webhook` | `webhookId`**(bb)**, `secret`, `includeHeaders`=false | `out` | `POST /api/hooks/<webhookId>`. Body JSON → `data`. Có `secret` thì cần header `X-Webhook-Secret`. |
| `schedule` | `cron`**(bb)**, `timezone`=Asia/Ho_Chi_Minh, `payload`={} | `out` | Cron 6 trường `giây phút giờ ngày tháng thứ` (5 trường cũng được, giây=0). Ra: `payload` + `ts` + `iso`. |
| `telegram-poll` | `botToken`**(bb)**, `timeout`=30, `allowedUpdates`, `dropPending`=true, `deleteWebhook`=true | `out` | Long polling `getUpdates` — không cần URL công khai. Mỗi update → một lần chạy, update vào `data`, `meta.updateId`. `botToken` lấy từ @BotFather. Offset được lưu nên khởi động lại không đọc trùng. Lỗi Telegram → cổng `error`, tự thử lại. |
| `request` 🎯 | *(không có)* | `out` | **MỚI.** Điểm vào đồng bộ có tên. App khác gọi qua `rule_call {chainId, node, data}`; `data` vào luồng trên `out`. Đi kèm `respond`. |

### Logic

| rule | config chính | cổng ra | ghi chú |
|---|---|---|---|
| `conditional` | `expr`**(bb)**, `setResultTo` | `true`/`false` *(one)* | Rẽ nhánh boolean. Biểu thức lỗi/không boolean → `error`. |
| `switch` | `key`**(bb)**, `matchType`=auto, `cases`**(bb)** | *(động: 1 cổng / case)* + `default` | Mỗi case một cổng; duyệt theo thứ tự, case đầu khớp thắng. Không khớp → `default`; thiếu `key` → `error`. |
| `fork` | *(không có)* | `out` *(many)* | Chia nhánh song song, mỗi cạnh một bản sao độc lập. |
| `join` | `inputs`**(bb)** | `out` | *(cổng vào động theo `inputs`)* Chờ đủ, phát `{ "<cổng>": data }`. **Bắt buộc `opts.join="all"`.** |
| `merge` | `inputs`**(bb)** | `out` | *(cổng vào động)* Như `join` nhưng deep-merge phẳng. **Bắt buộc `opts.join="merge"`.** |
| `trigger-time` | `left`**(bb)**=now(), `right`**(bb)**, `unit`**(bb)**=hour, `timezone` | `true`/`false` *(one)* | So một thành phần thời gian (minute/hour/day/weekday/month/year). |

### Biến đổi (transform)

| rule | config chính | cổng ra | ghi chú |
|---|---|---|---|
| `arithmetic` | `operators`**(bb)** (mảng `{target, expr}`) | `out` | Chạy lần lượt trên xuống; dòng sau thấy kết quả dòng trước. Lỗi → `error`. |
| `format` | `fields`**(bb)** (mảng `{source, target, type, format}`) | `out` | Ép kiểu: string/number/double/bool/time/timestamp. Thiếu field nguồn = bỏ qua (warn); đổi kiểu hỏng = `error`. |
| `project` | `recreate`=false, `fields`**(bb)** (mảng `{key, type, value}`) | `out` | Dựng payload: `assign`/`set_string`/`set_number`/`set_float`/`set_bool`/`expr`. `recreate=true` = bỏ field không khai. |
| `delay` | `ms`=1000 (0–300000) | `out` | Giữ message rồi cho đi nguyên vẹn. Chiếm 1 worker suốt lúc chờ. |
| `split` | `path`, `as`, `includeIndex`=false | `item` + `done` | Tách mảng: `item` phát N lần, `done` phát 1 lần `{count:N}`. Không phải mảng → `error`. |
| `store` 🗄️ | `key` | `out` | **MỚI.** Cache `data` (hoặc sub-field `key`) tại node; đọc lại bằng `rule_get`. Passthrough trên `out`. Dùng cho endpoint "giá trị mới nhất". |
| `aggregate` 📦 | `count`=10 (`0`=chỉ flush thủ công) | `out` | **MỚI.** Nghịch đảo `split`. Cổng vào `in` (tích luỹ) + `flush` (ép phát). Phát `{items:[...], count:N}`. |

### Lọc (filter)

| rule | config chính | cổng ra | ghi chú |
|---|---|---|---|
| `moving-average` | `field`**(bb)**, `windowSize`, `threshold`=0, `outputField` | `pass`/`noise` | Trung bình trượt; lệch quá ngưỡng → `noise`. **Có state** (theo chain+node). |
| `kalman` | `field`**(bb)**, `r`, `q`, `p`, `initial`, `outputField` | `out` | Làm mượt Kalman 1 chiều. **Có state.** |
| `dedup` 🚫 | `key` (bỏ trống = cả data), `windowMs`=60000 | `out`/`dropped` | **MỚI.** Lần đầu → `out`; lặp trong cửa sổ → `dropped`. |
| `rate-limit` ⏱️ | `rate`=5, `perMs`=1000 | `out`/`dropped` | **MỚI.** Token bucket. Trong hạn → `out`; vượt → `dropped`. |

> **State**: `moving-average`, `kalman` (và các filter mới) nhớ giá trị giữa các
> run. Sửa config xong nên xoá state (nút **Xoá state** trên canvas, hoặc
> `DELETE /api/chains/:id/state`) trước khi so kết quả.

### Đích (sink)

| rule | config chính | cổng ra | ghi chú |
|---|---|---|---|
| `http-request` | `url`**(bb)**, `method`=GET, `headers`, `body`, `timeoutMs`, `parseJson`=true | `success`/`failed` | Phát `{status, body, headers}`. 2xx→`success`, khác 2xx→`failed`, không gọi được→`error`. URL/body có `${field}`. |
| `telegram-send` | `botToken`**(bb)**, `chatId`**(bb)**, `message`**(bb)**, `parseMode`=HTML, `silent`, `apiBase` | `out` | Gửi Telegram; phát `{sent, messageId, chatId}`. `ok:false`→`error`. |
| `notification` | `message`**(bb)**, `title`, `level`=info | `out` | Ghi thông báo vào log chain + đẩy UI. Không chặn nhánh. |
| `log` | `level`=info, `message` | `out` | Soi dữ liệu giữa chuỗi; message đi tiếp nguyên vẹn. Bỏ trống = in cả payload. |
| `mcp-call` | `app`**(bb)**, `tool`**(bb)**, `args`, `argsFrom`, `outputField`=result | `out` | Gọi tool MCP của Space App **khác** (đang chạy). KHÔNG gọi được `mcp__senclaw-*`. Kết quả gắn vào `outputField`. |
| `senclaw-send` | `target`**(bb)**, `message`**(bb)**, `timeoutSeconds`=120 | `out` | Gửi tin qua kênh SenClaw (`web:main`, `telegram:123`…). Đường vòng qua 1 lượt agent — chậm, tốn token. Chỉ báo nội bộ thì dùng `notification`. |
| `respond` 📤 | *(không có)* | *(terminal)* | **MỚI.** Bất cứ gì tới đây thành **kết quả** trả về bởi `rule_call`. Terminal. Luồng gọi đồng bộ phải kết ở **đúng một** `respond`. |

### AI

| rule | config chính | cổng ra | ghi chú |
|---|---|---|---|
| `ai-agent` | `backend`=senclaw (`senclaw`/`persona`/`provider`), `systemPrompt`, `userPrompt`, `maxTokens`, `profile`, `persona`, `tools`, `model`, `provider`, `apiKey`, `outputField`=response, `parseJson` | `out` | Gọi LLM, gắn trả lời vào `outputField`. `finish=length` (cụt token) → `error`. `temperature` chỉ tác dụng ở backend `provider`. |
| `knowledge` | `action`**(bb)** (`save`/`search`/`recall`), `text`, `query`, `space`, `tags`, `limit`=6, `outputField`=knowledge | `out` | Ghi/tra kho tri thức SenClaw. Bỏ trống `space` = KHÔNG tìm toàn cục, chỉ không gian của app này. |

## Bốn kiểu luồng dữ liệu

Một chain có thể chạy tự động (schedule/webhook) hoặc được app khác gọi như một hàm.

- **Push (async)** — `rule_push {chainId, node?, port?, data, meta?}`: bắn sự kiện,
  trả về ngay. `node` mặc định = `manual` đầu tiên. (`rule_trigger` là biến thể
  dùng cho chạy thử.)
- **Get (đọc state)** — `rule_get {chainId, node}`: đọc giá trị mới nhất một node
  `store` đang cache, **không tạo Run**.
- **Pull (đồng bộ)** — `rule_call {chainId, node?, data, timeoutMs?}`: bơm vào
  `request`/`manual`, **chờ** Run chạm `respond`, trả `{status, result, error}`.
- **Callback** — trong một Run, node `http-request`/`mcp-call` gọi ra ngoài rồi
  dùng kết quả đi tiếp. Vì mọi chain có `request`+`respond` đều gọi được qua
  `rule_call`, một chain có thể gọi chain khác.

### Ví dụ Pull (request → logic → respond)

```json
{
  "chainId": 123,
  "nodes": [
    { "id": "n1", "rule": "request",    "name": "Vào",        "config": {}, "x": 0,   "y": 0 },
    { "id": "n2", "rule": "arithmetic", "name": "Tính VAT",   "config": { "operators": [ { "target": "vat", "expr": "gia * 0.1" }, { "target": "tong", "expr": "gia + vat" } ] }, "x": 320, "y": 0 },
    { "id": "n3", "rule": "respond",    "name": "Trả kết quả","config": {}, "x": 640, "y": 0 }
  ],
  "edges": [
    { "id": "e1", "from": { "node": "n1", "port": "out" }, "to": { "node": "n2", "port": "in" } },
    { "id": "e2", "from": { "node": "n2", "port": "out" }, "to": { "node": "n3", "port": "in" } }
  ]
}
```

Gọi: `rule_call {chainId: 123, data: {"gia": 100}}` → `{status, result: {gia:100, vat:10, tong:110}}`.

### Ví dụ Get (source → store)

```json
{
  "chainId": 124,
  "nodes": [
    { "id": "n1", "rule": "schedule", "name": "Mỗi phút",  "config": { "cron": "0 * * * * *" }, "x": 0,   "y": 0 },
    { "id": "n2", "rule": "http-request", "name": "Đo giá", "config": { "method": "GET", "url": "https://api.example.com/price" }, "x": 320, "y": 0 },
    { "id": "n3", "rule": "store", "name": "Giữ mới nhất",  "config": { "key": "body.price" }, "x": 640, "y": 0 }
  ],
  "edges": [
    { "id": "e1", "from": { "node": "n1", "port": "out" },     "to": { "node": "n2", "port": "in" } },
    { "id": "e2", "from": { "node": "n2", "port": "success" }, "to": { "node": "n3", "port": "in" } }
  ]
}
```

Đọc: `rule_get {chainId: 124, node: "n3"}`.

## Cổng, gộp nhánh, biểu thức

- **Cổng động**: `switch` sinh một cổng ra cho mỗi phần tử `config.cases` + cổng
  `default`. `join`/`merge` sinh một cổng vào cho mỗi tên trong `config.inputs`.
- **Gộp nhiều nhánh**: hai cạnh vào cùng node = node chạy **hai lần**, không gộp.
  Muốn gộp phải dùng `join`/`merge` với `config.inputs` **và** đặt `opts.join` =
  `"all"`/`"merge"`. Dựng qua MCP thì **phải tự đặt** — mặc định `"any"` không bật
  rào chắn, không cảnh báo. Đặt `opts.joinTimeoutMs` nếu một nhánh có thể không
  tới; `opts.corrKey` để gộp theo giá trị nghiệp vụ. Chi tiết: chuẩn §5.
- **Biểu thức** (`conditional`, `arithmetic`, `project` type `expr`): toán tử
  `+ - * / % **`, `== != <> < > <= >=`, `&& || !`, ba ngôi `? :`; hàm `strlen len
  abs round floor ceil min max lower upper trim contains startsWith endsWith str
  num int bool coalesce now`; đường dẫn lồng `user.name`, `list[0]`; metadata qua
  `sFromObj(meta_data, 'device_id')`. Chuỗi template (`message`, `url`, `body`,
  `userPrompt`…) dùng `${field}` / `${a.b[0]}`.

## Lưu ý

- Bí mật (bot token, API key, secret) người dùng tự cung cấp — đừng bịa.
- `rule_generate` chỉ dựng nháp; luôn `rule_validate` và đọc lại trước khi kích hoạt.
- Xử lý lỗi, checklist trước khi kích hoạt, và bốn kiểu luồng chi tiết:
  [`docs/rule-engine-build-standard.md`](../../../../docs/rule-engine-build-standard.md).

Trả lời bằng ngôn ngữ người dùng đang dùng.
