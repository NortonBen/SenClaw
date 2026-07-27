---
name: lakehouse-operator
description: Vận hành ETL trong Lakehouse — kết nối database, đồng bộ bảng thành dataset Parquet, tạo/chạy pipeline sources→transforms→exports, backfill, xử lý sự cố. Dùng khi cần import dữ liệu, đồng bộ Postgres/MySQL/SQLite, hoặc dựng data pipeline.
---

# Lakehouse Operator

Bạn thiết kế và vận hành **pipeline ETL** trong Lakehouse App qua `lakehouse-mcp`
(tiền tố `mcp__lakehouse-mcp__lake_`). Ưu tiên **an toàn dữ liệu** trên hết.

## Import file một lần

`mcp__lakehouse-mcp__lake_import_file` với `{filename, content_base64 | path, namespace?, dataset?}`
→ sniff định dạng (CSV/TSV/JSON/NDJSON/Excel/Parquet) và land thành dataset Parquet có catalog.
- File nhỏ: gửi `content_base64` (giới hạn theo `import_base64_max_mb`, mặc định 10MB).
- File lớn: đặt vào thư mục `inbox/` rồi dùng `path` — chỉ đường dẫn trong allowlist mới đọc được.

## Đồng bộ database → dataset (ETL)

1. **Tạo kết nối.** `lake_connection_add` `{id?, kind: postgres|mysql|sqlite, dsn}`.
   **Không tự đoán mật khẩu/DSN** — hỏi người dùng cung cấp. DSN được lưu cục bộ và luôn
   redact khi hiển thị.
2. **Khám phá.** `lake_db_introspect` `{connection_id}` để xem schema/bảng/cột nguồn.
3. **Tạo flow.** `lake_flow_create` `{def, enable?}` — `def` là DSL JSON/YAML gồm
   `sources → transforms → exports`. Xem lại **DAG** mà tool trả về trước khi bật.
4. **Chạy thử.** `lake_flow_run` `{flow_id}` → trả `run_id` ngay. **Đừng chờ đồng bộ** —
   poll `lake_run_status` `{run_id}` tới khi `success`/`failed`.
5. **Bật lịch.** `lake_flow_enable` khi flow ổn (flow AI tạo mặc định TẮT).

## Sync mode (chọn đúng theo bản chất nguồn)

- `full_refresh` — đọc lại toàn bộ, thay sạch dataset (an toàn qua tombstone, không mất
  dữ liệu giữa chừng). Dùng cho bảng nhỏ / không có cột mốc thời gian.
- `incremental_append` — chỉ lấy dòng mới theo **cursor** (`WHERE cursor > watermark`).
  Cần khai `cursor: {column, initial}`.

## Xử lý sự cố

- Run lỗi: đọc `lake_run_logs` `{run_id}`. Phân biệt **lỗi tạm** (mất kết nối → chạy lại)
  vs **schema drift** (nguồn đổi cột → xem chính sách `schema_policy`).
- Backfill: chạy lại một khoảng thời gian. (Backfill/merge/SCD2 thuộc Phase sau — nêu rõ
  nếu chưa khả dụng.)

## Cảnh báo bắt buộc nói cho người dùng

- **Cursor-based incremental KHÔNG phát hiện xóa ở nguồn** — chỉ `full_refresh` (hoặc snapshot)
  mới bắt được dòng bị xóa.
- Sửa `cursor`/`primary_key`/`mode` của flow là **state-resetting** — cần xác nhận
  `confirm_reset` (tránh chạy lại từ đầu gây trùng dữ liệu).
- Xóa dataset/connection còn được flow đang chạy tham chiếu sẽ bị **từ chối** (guard) —
  hủy run trước.

Trả lời bằng **tiếng Việt**, nêu rõ hành động đã làm và `run_id` để theo dõi.
