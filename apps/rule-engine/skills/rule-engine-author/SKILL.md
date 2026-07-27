---
name: rule-engine-author
description: >-
  Dựng và sửa luồng xử lý dữ liệu dạng đồ thị (rule chain) trong Space App
  Rule Engine: chọn node nguồn, nối các node lọc/rẽ nhánh/biến đổi qua cổng
  vào–ra, rồi kích hoạt. Dùng khi người dùng nói đại loại "tạo luồng tự động",
  "mỗi 5 phút gọi API rồi báo Telegram nếu nhiệt độ > 35", "thêm nhánh điều
  kiện vào luồng", "nối node A sang node B", "build an automation flow",
  "create a rule chain". KHÔNG dùng khi người dùng chỉ muốn xem vì sao luồng
  không chạy — đó là việc của skill `rule-engine-debug`.
---

# rule-engine-author

Bạn dựng luồng bằng MCP của app Rule Engine. Mọi liên kết giữa các node nằm ở
`edges`; **tuyệt đối không nhét id node vào `config`**.

## Trình tự

1. **Đọc danh mục node trước khi làm gì khác**

   `mcp__rule-engine-mcp__rule_registry` trả về mọi loại node kèm cổng vào/ra và
   JSON Schema cấu hình. Đừng đoán tên node hay tên cổng — cổng sai sẽ bị chặn
   ở bước kiểm tra.

2. **Chọn node nguồn.** Mỗi luồng cần đúng một node có `isSource: true`:

   | Node | Khi nào dùng |
   |---|---|
   | `manual` | chạy thử, hoặc để agent/UI bơm sự kiện |
   | `schedule` | chạy theo cron (kèm timezone) |
   | `webhook` | có hệ thống ngoài POST vào `/api/hooks/<webhookId>` |
   | `telegram-hook` | nhận update từ bot Telegram |

3. **Tạo hoặc chọn luồng**: `rule_create_chain`, hoặc `rule_list_chains` rồi
   `rule_get_chain` nếu sửa cái có sẵn.

4. **Ghi đồ thị**: `rule_update_graph` với `nodes` + `edges`.

   ```json
   {
     "chainId": 123,
     "nodes": [
       { "id": "n1", "rule": "schedule", "name": "Mỗi 5 phút",
         "config": { "cron": "*/5 * * * *", "timezone": "Asia/Ho_Chi_Minh" }, "x": 0, "y": 0 },
       { "id": "n2", "rule": "http-request", "name": "Gọi API",
         "config": { "method": "GET", "url": "https://..." }, "x": 320, "y": 0 },
       { "id": "n3", "rule": "conditional", "name": "Nóng quá?",
         "config": { "expr": "temperature > 35" }, "x": 640, "y": 0 },
       { "id": "n4", "rule": "telegram-send", "name": "Báo",
         "config": { "botToken": "...", "chatId": "...", "message": "Nóng ${temperature}độ" },
         "x": 960, "y": -160 }
     ],
     "edges": [
       { "id": "e1", "from": {"node":"n1","port":"out"},     "to": {"node":"n2","port":"in"} },
       { "id": "e2", "from": {"node":"n2","port":"success"}, "to": {"node":"n3","port":"in"} },
       { "id": "e3", "from": {"node":"n3","port":"true"},    "to": {"node":"n4","port":"in"} }
     ]
   }
   ```

5. **Kiểm tra**: `rule_validate`. Lỗi (`level: "error"`) phải sửa hết mới kích
   hoạt được; cảnh báo (`warning`) thì đọc rồi tự quyết.

6. **Kích hoạt**: `rule_activate`. Sau đó `rule_trigger` để bơm một sự kiện thử
   (chỉ được với node nguồn; `manual` là tiện nhất).

## Quy tắc về cổng

- Mỗi node có cổng ra `error` **ngầm định** dù không khai báo. Nối nó khi muốn
  bắt lỗi; không nối thì nhánh dừng và lỗi được ghi log.
- `conditional` → `true` / `false`. `http-request` → `success` / `failed`.
  `moving-average` → `pass` / `noise`. `split` → `item` / `done`.
- `switch` sinh cổng động: mỗi phần tử trong `config.cases` là một cổng, cộng
  cổng `default`.
- Cổng `arity: "one"` chỉ nhận đúng một cạnh; `"many"` thì fan-out, mỗi cạnh
  nhận một bản sao độc lập của dữ liệu.

## Gộp nhiều nhánh (nhiều cổng vào)

Mặc định mỗi message vào node là một lần chạy riêng — hai cạnh trỏ vào cùng một
node nghĩa là node đó chạy **hai lần**. Muốn chờ đủ rồi mới chạy một lần:

- dùng node `join` (giữ riêng từng nhánh) hoặc `merge` (gộp thành một object),
- khai `config.inputs` là danh sách tên cổng vào (dùng **nguyên văn** làm khoá
  cạnh — chỉ chữ, số, `_`, `-`; không khoảng trắng, không trùng),
- và **bắt buộc** đặt `opts.join` = `"all"` (join) hoặc `"merge"` trên chính node
  đó.

⚠️ Khi dựng qua MCP (`rule_update_graph`) bạn **phải tự đặt `opts.join`** — mặc
định là `"any"`, mà `"any"` thì rào chắn KHÔNG bật: node chạy một lần cho mỗi
cạnh vào (hai nhánh → node sau chạy hai lần với dữ liệu chưa gộp), không có cảnh
báo nào. Ví dụ node `join`:

```json
{ "id": "j1", "rule": "join", "name": "Chờ đủ",
  "config": { "inputs": ["thoi_tiet", "ton_kho"] },
  "opts": { "join": "all" }, "x": 640, "y": 0 }
```

Đặt `opts.joinTimeoutMs` nếu một nhánh có thể không bao giờ tới; quá hạn thì
nhánh bị huỷ và ghi log, không treo mãi.

## Biểu thức

`conditional`, `arithmetic`, `project` (type `expr`) dùng cùng một cú pháp:

- toán tử `+ - * / % **`, `== != <> < > <= >=`, `&& || !`, ba ngôi `? :`
- hàm `strlen len abs round floor ceil min max lower upper trim contains
  startsWith endsWith str num int bool coalesce now`
- đường dẫn lồng nhau: `user.name`, `list[0]`
- metadata của message nằm trong `meta_data`: `sFromObj(meta_data, 'device_id')`

Chuỗi template (`message`, `url`, `body`, `userPrompt`...) dùng `${field}` hoặc
`${a.b[0]}`.

## Lưu ý

- Toạ độ: node rộng ~230px, nên `x` giãn **320** một bước (0, 320, 640...) và `y`
  giãn ~160 giữa các nhánh. Giãn hẹp hơn thì cạnh nối ngắn tới mức gần như
  không nhìn thấy. Đồ thị không có toạ độ sẽ chồng lên nhau.
- `rule_generate` dựng nháp nhanh từ một câu mô tả — vẫn phải `rule_validate` và
  đọc lại trước khi kích hoạt.
- Bí mật (bot token, API key) người dùng phải tự cung cấp; đừng bịa.

Trả lời bằng ngôn ngữ người dùng đang dùng (tiếng Việt hoặc tiếng Anh).
