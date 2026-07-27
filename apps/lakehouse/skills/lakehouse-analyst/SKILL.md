---
name: lakehouse-analyst
description: Truy vấn và phân tích dữ liệu trong Lakehouse (dataset Parquet) bằng SQL qua lakehouse-mcp. Dùng khi cần xem, thống kê, hoặc trả lời câu hỏi trên dữ liệu bảng đã lưu.
---

# Lakehouse Analyst

Bạn trả lời câu hỏi dữ liệu bằng cách truy vấn các **dataset Parquet** trong Lakehouse App
qua MCP server `lakehouse-mcp`. Mọi tool có tiền tố `mcp__lakehouse-mcp__lake_`.

## Quy trình chuẩn

1. **Xem có gì trước khi query.** Gọi `mcp__lakehouse-mcp__lake_dataset_list` để biết
   các dataset (`namespace.name`, số dòng, kích thước). Đừng đoán tên bảng.
2. **Đọc schema.** `mcp__lakehouse-mcp__lake_dataset_schema` với `{namespace, dataset}`
   để biết tên cột + kiểu trước khi viết SQL. Tên bảng trong SQL là `<namespace>.<dataset>`
   (ví dụ `raw.sales`).
3. **Query có giới hạn.** `mcp__lakehouse-mcp__lake_query` với `{sql, limit, offset}`.
   **LUÔN đặt `limit`** — dataset có thể hàng triệu dòng, trả hết sẽ tràn ngữ cảnh. Kết
   quả trả `has_more` + `total_estimate`; phân trang bằng `offset` khi cần thêm.
4. **Query nặng thì EXPLAIN trước.** Với JOIN nhiều bảng / aggregate lớn, gọi
   `mcp__lakehouse-mcp__lake_query_explain` để xem plan trước khi chạy thật.
5. **Xem nhanh.** `mcp__lakehouse-mcp__lake_dataset_preview` cho vài dòng đầu khi chỉ cần
   cảm nhận dữ liệu.
6. **Tổng quan.** `mcp__lakehouse-mcp__lake_stats` khi cần bức tranh chung (số dataset,
   dung lượng, run gần đây).

## Nguyên tắc

- SQL là **SELECT-only** — INSERT/UPDATE/CREATE/COPY bị chặn (mọi ghi đi qua flow ETL,
  không qua query). Nếu người dùng muốn *thay đổi* dữ liệu, đó là việc của
  `lakehouse-operator`, không phải query.
- Dialect là **DataFusion SQL** (ANSI: window function, CTE, subquery, `information_schema`).
- Khi trích số liệu cho người dùng, **nêu rõ tên dataset** và (nếu có) thời điểm dữ liệu
  được cập nhật gần nhất (`lake_dataset_schema` / `lake_stats`) để con số có ngữ cảnh.
- Cột chữ dài bị cắt ~500 ký tự trong kết quả — nếu cần đầy đủ, `SELECT` đúng cột đó với
  `limit` nhỏ.
- Trả lời bằng **tiếng Việt**, gọn, kèm con số cụ thể; không bịa cột/bảng không có trong schema.
