---
name: rule-engine-debug
description: >-
  Tìm nguyên nhân một rule chain trong Space App Rule Engine không chạy, chạy
  sai nhánh, hoặc dừng giữa chừng. Dùng khi người dùng nói "luồng không chạy",
  "sao không thấy tin nhắn gửi đi", "trace luồng", "xem log rule engine",
  "why did my flow stop", "rule chain không kích hoạt được". KHÔNG dùng khi
  người dùng muốn dựng luồng mới — đó là skill `rule-engine-author`.
---

# rule-engine-debug

Chẩn đoán theo thứ tự này, đừng đoán trước khi có dữ liệu.

## 1. Luồng có đang chạy không?

`mcp__rule-engine-mcp__rule_list_chains` → xem `status` và `deployed`.

- `status: "INACTIVE"` → chưa kích hoạt. `rule_activate`.
- `status: "ERROR"` → nạp thất bại. `rule_validate` để biết lý do.
- `status: "ACTIVE"` nhưng `deployed: false` → app vừa khởi động lại và nạp
  hỏng; xem `rule_logs`.

## 2. Đồ thị có hợp lệ không?

`rule_validate`. Những lỗi hay gặp:

| Thông báo | Nghĩa là |
|---|---|
| không có cổng ra `X` | tên cổng sai — `rule_registry` để lấy tên đúng |
| chỉ nhận 1 cạnh | cổng `arity: one` (nhánh quyết định) bị nối 2 lần |
| không có cổng vào nào được nối | node mồ côi, không bao giờ chạy |
| node nguồn chưa nối đi đâu | sự kiện phát ra rồi rơi vào hư vô |
| luồng không có node nguồn | không có gì kích hoạt luồng |
| có vòng lặp | chỉ là cảnh báo, nhưng phải có điều kiện thoát |

## 3. Có sự kiện nào vào không?

`rule_runs` → nếu danh sách rỗng thì vấn đề nằm ở **node nguồn**, không phải ở
phần thân luồng:

- `schedule`: cron sai (crate cron cần 6 trường có giây — 5 trường sẽ được tự
  thêm `0` ở đầu), hoặc timezone sai.
- `webhook`: bên gọi phải POST đúng `/api/hooks/<webhookId>`; sai secret sẽ bị
  từ chối 401.
- `manual`: chỉ chạy khi có người bấm "Chạy thử" hoặc gọi `rule_trigger`.

Cách nhanh nhất để tách bạch: `rule_trigger` bơm thẳng một sự kiện. Nếu bơm tay
chạy được thì lỗi ở nguồn.

## 4. Nó dừng ở đâu?

Trace chỉ được ghi khi bật debug:

1. `rule_set_debug` với `debug: true`
2. `rule_trigger` (lấy `runId` trả về)
3. `rule_run_trace` với `runId` đó

Đọc cột `outPort` của bước cuối:

- `outPort` rỗng và `kind: "data"` → nhánh kết thúc bình thường tại một node
  sink, hoặc **cổng đó không nối đi đâu**.
- `kind: "error"` → xem `error`. Nếu cổng `error` chưa nối thì nhánh dừng tại
  đây và lỗi chỉ nằm trong log.
- Không có bước nào cho một node → message chưa từng tới nó: kiểm tra cạnh phía
  trước, hoặc node đó đang chờ join.

## 5. Trạng thái của run

`rule_runs` cho biết kết cục:

- `done` — chạy hết, không lỗi.
- `failed` — có lỗi không được cổng `error` nào hứng.
- `timeout` — hết TTL. Gần như luôn là một node đặt `opts.join = "all"` mà một
  cổng vào không bao giờ có message. Kiểm tra `config.inputs` khớp với các cạnh
  thực sự nối vào, và cân nhắc `joinTimeoutMs`.
- `running` mãi — còn message đang chờ ở barrier.

## 6. Log

`rule_logs` gom mọi thứ ngoài trace: nguồn khởi động thất bại, join quá hạn,
node đích không tìm thấy, vượt trần số bước (mặc định 10000 — dấu hiệu vòng lặp
vô tận).

## Bẫy hay gặp

- **Hai cạnh vào cùng một node = node chạy hai lần**, không phải gộp. Muốn gộp
  phải dùng `join`/`merge` kèm `opts.join`.
- **Node lọc có state** (`moving-average`, `kalman`) nhớ giá trị cũ giữa các
  run. Sửa cấu hình xong nên xoá state trước khi so sánh kết quả — bấm nút
  **Xoá state** trên thanh công cụ của trình soạn luồng (canvas). Đây là thao
  tác trên UI; không có MCP tool nào làm việc này, đừng gọi REST hay bịa ra tool.
- **`${field}` không thay được** thường vì tên field sai tầng: dữ liệu vào node
  là payload phẳng, không phải object bọc ngoài. Xem `data` trong trace để biết
  chính xác node đó nhận gì.

Trả lời bằng ngôn ngữ người dùng đang dùng (tiếng Việt hoặc tiếng Anh).
