# Lakehouse — SenClaw Space App

Data lake + data warehouse cá nhân: import file/database thành **dataset Parquet**, query
bằng **SQL (Apache DataFusion)**, và dựng **pipeline ETL** `sources → transforms → exports`
có lập lịch — tất cả trong một binary Rust, chạy offline, không tải gì lúc runtime.

- **Port:** 4560 · **MCP:** `lakehouse-mcp` (tool prefix `lake_`)
- **Thiết kế đầy đủ:** [`docs/data-lake-app-design.md`](../../docs/data-lake-app-design.md)

## Chạy dev

```bash
cargo run -p lakehouse            # tự bind PORT (mặc định 4560)
# UI: cd web && npm install && npm run dev  (proxy /api → 127.0.0.1:4560)
```

Dữ liệu nằm **ngoài** thư mục cài đặt (install zip xoá sạch app dir mỗi lần update):
`~/.senclaw/space-app-data/lakehouse/` — `catalog.sqlite` + `lake/<ns>/<ds>/*.parquet`.
Override bằng `LAKEHOUSE_DATA_DIR`.

## Quyết định thiết kế (đã research + review đối kháng — xem design doc §2, §11)

- **DataFusion 54, không DuckDB.** DuckDB dùng scanner postgres/mysql/httpfs là extension
  *tải runtime* (chết khi offline) và build C++ nặng; DataFusion thuần Rust, async-native,
  khớp daemon axum/tokio, zero runtime download. Arrow chỉ qua re-export `datafusion::arrow`
  (tránh 2 bản arrow lệch major). Bỏ feature `compression` của datafusion — nó link `lzma`,
  đụng `links` với `zip` của core.
- **Catalog-as-manifest, không atomic-rename thư mục, không Delta/Iceberg.** Bảng
  `dataset_file` giữ danh sách file active; query dựng `ListingTable` từ danh sách file +
  schema catalog (CẤM inference). File Parquet land xuống đĩa là "vô hình" tới khi vào
  manifest; "swap" (kể cả merge nhiều partition) = **một transaction SQLite**. Reader
  isolation qua GC grace period. Delta pin DataFusion 53 (lệch major), Iceberg write chỉ
  append — đều loại.
- **SQL query là SELECT-only** qua `SQLOptions` của DataFusion (chặn cả `EXPLAIN ANALYZE
  INSERT`); mọi ghi đi qua flow engine để có lineage + kiểm soát.
- **Connectors:** Postgres/MySQL qua `sqlx 0.9` (KHÔNG bật feature `sqlite` — đụng
  `libsqlite3-sys` với `rusqlite` bundled); SQLite dùng `rusqlite` sẵn có; **ClickHouse
  qua HTTP `reqwest` thuần** (FORMAT JSONEachRow, zero dep mới). Tier mở rộng còn lại
  (Mongo/MSSQL/Snowflake/BigQuery/Oracle/ODBC) — mỗi cái cần dep/system-lib/live-server
  riêng, làm khi có tài nguyên verify.
- **Không đăng ký core scheduler** — tự lập lịch bằng tokio loop, `last_scheduled_at` lưu
  SQLite (sống sót restart), giống moltbook/shopee.

## Trạng thái

- **Phase 1 — Lake core:** ✅ catalog (10 bảng), manifest land/GC/reconcile, DataFusion
  query SELECT-only, sniffer import (CSV/TSV/JSON/NDJSON/Excel/Parquet), REST + MCP
  (dataset/query/import/stats). Verified end-to-end.
- **Phase 2 — Connectors + ETL cơ bản:** ✅ trait Connector (Postgres/MySQL/SQLite), sync
  `full_refresh` + `incremental_append` (cursor), flow DSL + DAG, runner (queue + claim +
  cancel + watchdog), REST/MCP `connection_*`/`flow_*`/`run_*`, WS dashboard.
- **Phase 3 — Transform + ETL đầy đủ:** ✅ transforms (`full` + `incremental_by_time`) +
  `incremental_merge` + `snapshot` SCD2 + schema-evolution + backfill + scheduler + AI sinh flow.
- **Phase 4 — Export + đóng gói:** ✅ file export (csv/json/parquet) + compaction + UI 7 tab
  + skills/persona + pack.sh.
- **No-code flow builder:** ✅ trình dựng flow **kéo-thả trực quan** (react-flow) trong tab
  Flows — kéo node nguồn/biến đổi/xuất, nối cạnh, cấu hình qua form → sinh DSL tự động, không
  cần gõ JSON/YAML; nạp ngược flow cũ ra canvas để sửa; giữ chế độ "JSON nâng cao".
- **Phase 5 — DB-load export:** ✅ ghi dataset trở lại database ngoài (SQLite/Postgres/MySQL)
  qua sqlx batched INSERT — `full_refresh`/`append`/`upsert` + `create_if_missing`. (COPY BINARY
  của pgpq để tối ưu tốc độ là việc tương lai — batched INSERT tránh bẫy arrow-pin §2.3.)

**Trạng thái: 171 cargo test xanh, ETL 2 chiều verified end-to-end trên release binary
(file/DB → lake Parquet → transform → file/DB).** Connector tier mở rộng
(ClickHouse/Mongo/MSSQL/Snowflake/BigQuery/Oracle/ODBC) là việc tương lai. Xem design doc §12.
