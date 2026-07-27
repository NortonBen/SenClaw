---
name: json-toolbox
description: >-
  Xử lý dữ liệu JSON qua app JSON Tools: format/minify, kiểm tra JSON hợp lệ (kèm dòng/cột
  lỗi), chuyển đổi qua lại giữa JSON · YAML · CSV · TSV · XML, lấy giá trị theo đường dẫn,
  so sánh hai tài liệu JSON, và mã hoá/giải mã base64 · URL · MessagePack. Dùng khi người
  dùng đưa một khối JSON/CSV/XML/YAML và muốn làm đẹp, kiểm lỗi, đổi định dạng hay so sánh.
  Mọi kết quả do công cụ tính — không tự tay format hay tự đoán.
triggers:
  - format json
  - làm đẹp json
  - minify json
  - json có hợp lệ không
  - validate json
  - kiểm tra json
  - json sang csv
  - csv sang json
  - json sang xml
  - xml sang json
  - json sang yaml
  - yaml sang json
  - chuyển đổi json
  - so sánh json
  - json diff
  - convert json
  - json to csv
  - csv to json
  - json to yaml
  - xml to json
  - base64 encode
  - base64 decode
  - url encode
  - url decode
  - msgpack
---

# json-toolbox

Dùng MCP server `json-mcp` của app **JSON Tools**. Các công cụ chạy cục bộ, thuần tính
toán — **không tự format bằng tay, không tự đoán kết quả chuyển đổi**.

## Chọn công cụ

- **`mcp__json-mcp__json_format`** — làm đẹp hoặc nén JSON. `mode: "pretty"` (mặc định)
  hoặc `"minify"`, `indent` mặc định 2. Thứ tự khoá gốc được giữ nguyên.
- **`mcp__json-mcp__json_validate`** — "JSON này có hợp lệ không". Trả về `valid`, và khi
  sai thì có `error` + `line` + `column`. Dùng công cụ này trước khi kết luận JSON hỏng.
- **`mcp__json-mcp__json_convert`** — đổi định dạng. `from`/`to` ∈ `json`, `yaml`, `csv`,
  `tsv`, `xml`. Thêm `root` khi xuất XML, `columns` để ghim thứ tự cột CSV/TSV.
- **`mcp__json-mcp__json_query`** — lấy một nhánh trong tài liệu lớn theo JSON Pointer
  (`/data/items/0/name`) hoặc dạng chấm (`data.items[0].name`). Ưu tiên công cụ này thay
  vì đọc thủ công khi tài liệu dài.
- **`mcp__json-mcp__json_diff`** — so sánh hai tài liệu, trả về danh sách
  `added` / `removed` / `changed` kèm đường dẫn.
- **`mcp__json-mcp__json_encode`** / **`json_decode`** — `format` ∈ `base64`, `url`,
  `msgpack` (MessagePack trao đổi dưới dạng base64).

## Cách trả lời

- **Kết luận trước**: JSON hợp lệ/không, có bao nhiêu khác biệt, kết quả chuyển đổi —
  rồi mới tới dữ liệu.
- Khi JSON sai, trích **dòng/cột** từ `json_validate` và chỉ ra chỗ hỏng, đừng chỉ nói
  "JSON không hợp lệ".
- Dán kết quả trong khối code kèm ngôn ngữ đúng (```json, ```yaml, ```csv, ```xml).
- Dữ liệu quá dài: dùng `json_query` để lấy đúng nhánh cần thay vì in cả tài liệu.
- CSV/TSV cần **mảng các object**; nếu đầu vào lồng nhau, nói rõ cần làm phẳng trước.
- Việc cần thao tác tay trên trình soạn thảo (xem cây, so sánh trực quan, TOON/TSON,
  formatter Java/Python/JS) thì chỉ người dùng mở giao diện app **JSON Tools**.
