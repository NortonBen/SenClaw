# Rule Engine — SenClaw Space App

Luồng xử lý dữ liệu dạng đồ thị: node nguồn bơm sự kiện, các node lọc / rẽ nhánh
/ biến đổi nối với nhau qua **cổng có tên**, mỗi cạnh mang một bản sao dữ liệu.

Port **4550**. MCP server `rule-engine-mcp` tại `/api/mcp/sse`.

Đây là bản viết lại bằng Rust của `dipper-hub/services/engine-runner` (Go) và
thư viện lõi `dipper-engine`. Thiết kế và lý do khác bản gốc:
[`docs/rule-engine-app-design.md`](../../docs/rule-engine-app-design.md).

---

## Khác biệt cốt lõi so với bản Go

| | dipper-engine (Go) | app này |
|---|---|---|
| Định tuyến | rule tự đọc id node kế tiếp từ `option` rồi ghi vào `Next []string` | rule phát ra **tên cổng**; engine tra bảng `edges` |
| Cổng ra | quy ước đặt tên field (`next_success`, `next_true`, `map_switch`), mỗi rule một kiểu; `maping.go` chỉ hiểu 3 trường hợp | `RuleSpec` khai báo cổng, có cả cổng động (switch) |
| Cổng vào | không có — 2 cạnh vào 1 node = node chạy 2 lần | `join` / `merge` với barrier, có TTL |
| Cổng lỗi | rule quên set `Next` → nhánh chết im lặng | cổng `error` **ngầm định** cho mọi node; không nối thì vẫn ghi log |
| Hàng đợi | 1 queue cho mỗi *loại* rule | 1 mailbox cho mỗi **node**, `concurrency` riêng |
| Vòng đời | `Session` vĩnh viễn, `EndCount` đếm sai, `Infinity()` khiến không bao giờ kết thúc | **Run** cho mỗi sự kiện, kết thúc khi hết message in-flight |
| Timeout | `TimeoutSession: 30` bị ép thành 30 **nanosecond** và không ai đọc | join TTL + run TTL + trần số bước, đều cấu hình được |
| Dữ liệu | `map[string]interface{}` chia sẻ giữa các nhánh fan-out (data race) | `serde_json::Value`, deep clone tại điểm fan-out |
| Dừng | `Rule.Stop()` không bao giờ được gọi | `SourceRule::stop` được gọi khi gỡ luồng |

## Chạy khi phát triển

```bash
cargo run -p rule-engine                 # backend :4550
cd apps/rule-engine/web && npm run dev   # UI dev server, proxy /api
cargo test -p rule-engine                # test
```

Đăng ký với daemon (daemon thấy port 4550 đã healthy sẽ dùng lại tiến trình của
bạn thay vì spawn bản khác):

```bash
curl -X POST http://127.0.0.1:18788/api/space/apps/register-local \
  -H 'content-type: application/json' \
  -d '{"path":"/Users/benji/Projects/SemaClaw/apps/rule-engine"}'
```

Đóng gói: `./scripts/pack.sh` → `rule-engine-app.zip`.

## Mô hình

```
nguồn ──out──▶ ┌────────┐ ──true──▶ ...
               │  node  │ ──false─▶ ...
   ──in──────▶ └────────┘ ──error─▶ ...
```

- **Chain** — đồ thị đã lưu. `ACTIVE` thì được nạp và chạy.
- **Run** — một sự kiện chảy qua đồ thị. Kết thúc khi không còn message nào đang
  bay *và* không còn barrier nào đang chờ.
- **Hop** — một bước trong run. Chỉ ghi lại khi bật debug.
- **Message** — `{ data, meta }`. `data` là payload phẳng; `meta` là thông tin
  kèm theo (nguồn, headers, thời điểm). Biểu thức thấy `data` ở tầng trên cùng
  và `meta` dưới tên `meta_data`.

### Nhiều cổng vào

Mặc định (`opts.join = "any"`) mỗi message vào node là một lần chạy độc lập —
đúng như engine Go. Đổi thành:

- `"all"` — chờ đủ một message trên **mỗi cổng vào đang được nối**, chạy một lần
  với `{ "<tên cổng>": <data> }`
- `"merge"` — như trên nhưng deep-merge thành một object

Barrier gom theo `(run, node, thế hệ)`, hoặc theo `opts.corrKey` nếu muốn gom
theo một giá trị nghiệp vụ. Quá `joinTimeoutMs` thì nhánh bị huỷ và ghi log —
không treo im lặng.

## Danh mục node

Gọi `GET /api/registry` (hoặc MCP `rule_registry`) để lấy danh sách đầy đủ kèm
cổng và JSON Schema cấu hình. Nhóm chính:

- **Nguồn** — `manual`, `webhook`, `schedule`, `telegram-poll`
- **Logic** — `conditional`, `switch`, `fork`, `join`, `merge`, `trigger-time`
- **Biến đổi** — `arithmetic`, `format`, `project`, `delay`, `split`
- **Lọc có state** — `moving-average`, `kalman`
- **Đích** — `http-request`, `telegram-send`, `notification`, `log`, `mcp-call`,
  `senclaw-send`
- **AI** — `ai-agent` (bridge SenClaw / persona / provider trực tiếp), `knowledge`

## REST

| | |
|---|---|
| `GET /api/status` | sức khoẻ + số liệu |
| `GET /api/registry` | danh mục node cho UI |
| `GET/POST /api/chains` | liệt kê / tạo |
| `GET/PATCH/DELETE /api/chains/:id` | chi tiết / sửa metadata / xoá |
| `PUT /api/chains/:id/graph` | thay toàn bộ đồ thị |
| `POST /api/chains/:id/validate\|activate\|deactivate\|trigger` | |
| `GET /api/chains/:id/runs\|logs` | lịch sử |
| `GET /api/runs/:id/hops` | trace từng bước |
| `DELETE /api/chains/:id/state` | xoá state của node lọc |
| `GET /api/events` | SSE realtime (hop / log / trạng thái run) |
| `POST /api/hooks/:webhookId` | ingress cho node `webhook` |

## Biến môi trường

| | mặc định | |
|---|---|---|
| `PORT` | 4550 | |
| `RULE_ENGINE_DATA_DIR` | `~/.senclaw/space-app-data/rule-engine` | nằm ngoài thư mục cài, vì cài lại zip sẽ xoá sạch `app_dir` |
| `RULE_ENGINE_JOIN_TIMEOUT_MS` | 60000 | hạn mặc định của barrier |
| `RULE_ENGINE_MAX_HOPS` | 10000 | trần số bước mỗi run, chặn vòng lặp vô tận |
| `RULE_ENGINE_RUN_TTL_SECS` | 900 | run quá hạn bị thu hồi |
| `SENCLAW_BASE_URL` | `http://127.0.0.1:18788` | daemon, cho bridge LLM/knowledge |

## Giới hạn đã biết

- Bridge `llm.request` **không có `temperature`** (daemon hard-code 0.2). Node
  `ai-agent` chỉ cho chỉnh nó ở backend `provider` (gọi thẳng nhà cung cấp).
- `finish == "length"` được coi là **lỗi**, không phải kết quả — câu trả lời cụt
  sẽ ra cổng `error` thay vì đi tiếp với JSON gãy.
- Bridge `mcp.call` của core vẫn là stub, nên node `mcp-call` POST thẳng vào
  `/api/mcp/message` của app đích.
- Nhóm node IoT của Dipper (`input-telemetry`, `output-model`, `get-last-model`…)
  **chưa có** — chúng phụ thuộc bảng device/model của dipper-hub. Xem §10 của
  design doc.
