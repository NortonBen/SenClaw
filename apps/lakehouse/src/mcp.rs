//! MCP server — JSON-RPC over HTTP + SSE hand-rolled, khớp các Space App khác
//! (không dùng crate `rmcp`). Tool prefix `lake_`; agent gọi
//! `mcp__lakehouse-mcp__lake_*`. Xem docs/data-lake-app-design.md §9.
//!
//! Load-bearing:
//!   * **KHÔNG mirror kết quả tools/call lên SSE** — bài học rewrite-story: fan-out
//!     làm payload của agent này lọt vào stream của agent khác.
//!   * Mọi kết quả `{"content":[{"type":"text","text":<pretty JSON>}]}` (+`isError`),
//!     kèm field `next`.
//!   * Nghiệp vụ dùng chung `api::logic_*` (parity REST↔MCP).

#![allow(dead_code)]

use std::convert::Infallible;
use std::path::{Path, PathBuf};

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::{self, ApiError, AppState};
use crate::config;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(_state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // Chỉ phát `endpoint` rồi giữ mở bằng keep-alive. KHÔNG broadcast tools/call.
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message"));
        let () = std::future::pending().await;
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}

fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
}

fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

/// Map kết quả `logic_*` (Value | ApiError) sang MCP content/isError.
/// `details` (vd danh sách FieldError khi validate flow) được nối vào text isError
/// để agent thấy đủ lỗi một lần (parity với REST body `{error, details}`).
fn from_logic(r: Result<Value, ApiError>) -> Value {
    match r {
        Ok(v) => json_result(v),
        Err(e) => match e.details {
            Some(d) => error_result(format!(
                "{}\n{}",
                e.msg,
                serde_json::to_string_pretty(&d).unwrap_or_default()
            )),
            None => error_result(e.msg),
        },
    }
}

pub async fn mcp_message(State(state): State<AppState>, Json(req): Json<JsonRpcRequest>) -> Json<Value> {
    let reply = |result: Value| -> Json<Value> {
        Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": result }))
    };
    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "lakehouse-mcp", "version": env!("CARGO_PKG_VERSION") }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => {
            Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} }))
        }
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or_default();
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params["arguments"].clone();
            reply(call_tool(&state, &name, &args).await)
        }
        _ => Json(json!("ok")),
    }
}

// ---- arg helpers ----

fn s(args: &Value, k: &str) -> String {
    args[k].as_str().unwrap_or("").trim().to_string()
}

fn opt_s(args: &Value, k: &str) -> Option<String> {
    let v = s(args, k);
    (!v.is_empty()).then_some(v)
}

fn opt_int(args: &Value, k: &str) -> Option<i64> {
    args[k].as_i64()
}

// ---------------------------------------------------------------------------
// tool catalogue
// ---------------------------------------------------------------------------

fn tools_list() -> Value {
    json!([
        {
            "name": "lake_stats",
            "description": "Tổng quan data lake: số dataset, tổng dòng/byte, số run đang chạy và trong 24h. Gọi TRƯỚC TIÊN khi người dùng hỏi về kho dữ liệu. Dùng cho 'tình hình data lake', 'lake status'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "lake_dataset_list",
            "description": "Liệt kê dataset trong lake (mỗi dòng: namespace, tên, số dòng, kích thước, version schema, flow chủ). KHÔNG trả dữ liệu — dùng lake_dataset_preview/lake_query để đọc. Dùng cho 'có những bảng nào', 'list datasets'.",
            "inputSchema": { "type": "object", "properties": {
                "namespace": { "type": "string", "description": "Lọc theo namespace (vd 'raw'). Bỏ trống = tất cả." },
                "limit":  { "type": "integer", "description": "Số dataset tối đa (mặc định 100)." },
                "offset": { "type": "integer", "description": "Bỏ qua bao nhiêu dataset đầu (phân trang)." }
            } }
        },
        {
            "name": "lake_dataset_schema",
            "description": "Xem schema (danh sách cột + kiểu) và lịch sử version của một dataset. Gọi TRƯỚC khi viết lake_query để biết tên cột chính xác. Dùng cho 'bảng này có cột gì', 'schema của dataset'.",
            "inputSchema": { "type": "object", "properties": {
                "namespace": { "type": "string", "description": "Namespace của dataset." },
                "dataset":   { "type": "string", "description": "Tên dataset." }
            }, "required": ["namespace", "dataset"] }
        },
        {
            "name": "lake_dataset_preview",
            "description": "Xem nhanh vài dòng đầu của một dataset (không cần viết SQL). Dùng để hiểu dữ liệu trước khi truy vấn. Dùng cho 'cho xem dữ liệu mẫu', 'preview bảng'.",
            "inputSchema": { "type": "object", "properties": {
                "namespace": { "type": "string", "description": "Namespace của dataset." },
                "dataset":   { "type": "string", "description": "Tên dataset." },
                "limit":     { "type": "integer", "description": "Số dòng (1..200, mặc định 50)." }
            }, "required": ["namespace", "dataset"] }
        },
        {
            "name": "lake_dataset_delete",
            "description": "Xoá một dataset khỏi catalog (file Parquet dọn sau bởi GC). Từ chối (409) nếu dataset đang thuộc một flow có run chạy dở. Dùng cho 'xoá bảng này', 'delete dataset'.",
            "inputSchema": { "type": "object", "properties": {
                "namespace": { "type": "string", "description": "Namespace của dataset." },
                "dataset":   { "type": "string", "description": "Tên dataset." }
            }, "required": ["namespace", "dataset"] }
        },
        {
            "name": "lake_import_file",
            "description": "Nhập một file (CSV/TSV/JSON/NDJSON/Excel/Parquet) vào lake thành dataset. Truyền nội dung qua 'content_base64' (nhỏ, cap import_base64_max_mb) HOẶC 'path' tới file đã nằm trong thư mục cho phép (inbox/ + import_paths). Định dạng tự dò theo magic bytes, không chỉ theo đuôi tên. Dùng cho 'nhập file này vào lake', 'import CSV'.",
            "inputSchema": { "type": "object", "properties": {
                "filename":      { "type": "string", "description": "Tên file (gợi ý đặt tên + đuôi). Bắt buộc." },
                "content_base64":{ "type": "string", "description": "Nội dung file mã base64 (chấp nhận cả 'data:...;base64,'). Vượt cap thì dùng 'path'." },
                "path":          { "type": "string", "description": "Đường dẫn file trên máy — CHỈ trong allowlist (inbox/ + import_paths). Dùng cho file lớn." },
                "namespace":     { "type": "string", "description": "Namespace đích (mặc định 'raw')." },
                "dataset":       { "type": "string", "description": "Tên dataset đích (chỉ áp khi file ra đúng một bảng)." }
            }, "required": ["filename"] }
        },
        {
            "name": "lake_dataset_export",
            "description": "Xuất một dataset ra file (CSV/JSON/Parquet) trong thư mục exports/. Ghi FILE ĐẦY ĐỦ ra đĩa (không phân trang), trả đường dẫn + download_url + cửa sổ preview nhỏ. Có thể lọc/chiếu cột bằng 'sql' tùy chọn. Dùng cho 'xuất bảng này ra CSV', 'export dataset'.",
            "inputSchema": { "type": "object", "properties": {
                "namespace": { "type": "string", "description": "Namespace của dataset." },
                "dataset":   { "type": "string", "description": "Tên dataset." },
                "format":    { "type": "string", "enum": ["csv", "json", "parquet"], "description": "Định dạng file xuất." },
                "sql":       { "type": "string", "description": "SELECT tùy chọn để lọc/chiếu cột (mặc định SELECT * toàn dataset)." }
            }, "required": ["namespace", "dataset", "format"] }
        },
        {
            "name": "lake_dataset_compact",
            "description": "Gộp nhiều file Parquet nhỏ của một dataset (theo từng partition) thành file lớn hơn — giảm số file, query nhanh hơn. Idempotent: dataset đã gọn thì không làm gì. File cũ được tombstone và GC dọn sau. Dùng cho 'gộp file nhỏ', 'compact dataset', 'tối ưu lưu trữ'.",
            "inputSchema": { "type": "object", "properties": {
                "namespace": { "type": "string", "description": "Namespace của dataset." },
                "dataset":   { "type": "string", "description": "Tên dataset." }
            }, "required": ["namespace", "dataset"] }
        },
        {
            "name": "lake_query",
            "description": "Chạy SQL SELECT (chỉ đọc) trên lake. Bảng tham chiếu dạng namespace.dataset (vd raw.orders). LUÔN dùng LIMIT — dataset có thể hàng triệu dòng và trả hết sẽ tràn ngữ cảnh. INSERT/UPDATE/DDL đều bị chặn. Dùng cho 'truy vấn dữ liệu', 'run SQL'.",
            "inputSchema": { "type": "object", "properties": {
                "sql":    { "type": "string", "description": "Câu SELECT duy nhất." },
                "limit":  { "type": "integer", "description": "Số dòng trả về (1..1000, mặc định 100)." },
                "offset": { "type": "integer", "description": "Bỏ qua bao nhiêu dòng đầu (phân trang, mặc định 0)." }
            }, "required": ["sql"] }
        },
        {
            "name": "lake_query_explain",
            "description": "Xem kế hoạch thực thi (EXPLAIN) của một câu SELECT mà KHÔNG chạy. Dùng để ước lượng trước khi lake_query nặng. Dùng cho 'query này chạy thế nào', 'explain SQL'.",
            "inputSchema": { "type": "object", "properties": {
                "sql": { "type": "string", "description": "Câu SELECT cần giải thích." }
            }, "required": ["sql"] }
        },
        {
            "name": "lake_connection_add",
            "description": "Thêm một kết nối database nguồn (Postgres/MySQL/SQLite/ClickHouse). TEST kết nối trước khi lưu — nguồn chết thì không lưu. DSN luôn được redact khi trả về. Dùng cho 'kết nối tới database', 'add data source'.",
            "inputSchema": { "type": "object", "properties": {
                "id":   { "type": "string", "description": "Định danh connection (tùy chọn, mặc định = kind). Ghi đè nếu trùng." },
                "kind": { "type": "string", "enum": ["postgres", "postgresql", "mysql", "mariadb", "sqlite", "clickhouse"], "description": "Loại database nguồn." },
                "dsn":  { "type": "string", "description": "Chuỗi kết nối (vd 'postgres://user:pass@host:5432/db', đường dẫn file .sqlite, hoặc 'clickhouse://user:pass@host:8123/db')." }
            }, "required": ["kind", "dsn"] }
        },
        {
            "name": "lake_connection_list",
            "description": "Liệt kê các kết nối database đã lưu (DSN luôn redact — mật khẩu ẩn). Dùng cho 'có những nguồn nào', 'list connections'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "lake_connection_test",
            "description": "Kiểm tra một kết nối đã lưu còn sống không; cập nhật thời điểm test thành công. Dùng cho 'nguồn còn kết nối được không', 'test connection'.",
            "inputSchema": { "type": "object", "properties": {
                "connection_id": { "type": "string", "description": "Id connection cần test." }
            }, "required": ["connection_id"] }
        },
        {
            "name": "lake_connection_delete",
            "description": "Xóa một kết nối. Từ chối (409) nếu còn flow tham chiếu tới nó. Dùng cho 'xóa nguồn này', 'delete connection'.",
            "inputSchema": { "type": "object", "properties": {
                "connection_id": { "type": "string", "description": "Id connection cần xóa." }
            }, "required": ["connection_id"] }
        },
        {
            "name": "lake_db_introspect",
            "description": "Liệt kê bảng/cột/kiểu của một kết nối database nguồn. Gọi TRƯỚC khi dựng flow để biết tên bảng/cột chính xác. Dùng cho 'database này có bảng gì', 'introspect schema'.",
            "inputSchema": { "type": "object", "properties": {
                "connection_id": { "type": "string", "description": "Id connection cần introspect." },
                "schema":        { "type": "string", "description": "Lọc theo schema nguồn (vd 'public'). Bỏ trống = tất cả." }
            }, "required": ["connection_id"] }
        },
        {
            "name": "lake_flow_create",
            "description": "Tạo một flow ETL/ELT từ định nghĩa DSL (JSON). Parse + validate; lỗi trả kèm danh sách FieldError. Trả DAG đã suy để kiểm. Flow mặc định TẮT (không tự chạy). Dùng cho 'tạo pipeline đồng bộ', 'create flow'.",
            "inputSchema": { "type": "object", "properties": {
                "def":    { "type": "object", "description": "Định nghĩa flow DSL (object JSON — xem SKILL). Bắt buộc." },
                "enable": { "type": "boolean", "description": "Bật flow ngay (mặc định false — không tự chạy)." }
            }, "required": ["def"] }
        },
        {
            "name": "lake_flow_update",
            "description": "Sửa định nghĩa một flow. Thay đổi state-resetting (đổi cursor/mode/connection/table…) cần confirm_reset=true; thiếu → trả impact {steps_reset, steps_kept, datasets_orphaned} để xác nhận. Dùng cho 'sửa flow', 'update pipeline'.",
            "inputSchema": { "type": "object", "properties": {
                "flow_id":       { "type": "string", "description": "Id flow cần sửa." },
                "def":           { "type": "object", "description": "Định nghĩa flow mới (object JSON)." },
                "confirm_reset": { "type": "boolean", "description": "Xác nhận reset state cho thay đổi state-resetting (mặc định false)." }
            }, "required": ["flow_id", "def"] }
        },
        {
            "name": "lake_flow_list",
            "description": "Liệt kê tất cả flow (kèm trạng thái bật/tắt, def_version, DAG). Dùng cho 'có những flow nào', 'list flows'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "lake_flow_get",
            "description": "Xem chi tiết một flow: định nghĩa + DAG + version. Dùng cho 'flow này làm gì', 'get flow'.",
            "inputSchema": { "type": "object", "properties": {
                "flow_id": { "type": "string", "description": "Id flow." }
            }, "required": ["flow_id"] }
        },
        {
            "name": "lake_flow_delete",
            "description": "Xóa một flow (dataset + dữ liệu GIỮ NGUYÊN, chỉ thả quyền sở hữu). Từ chối (409) nếu flow đang có run chạy. Dùng cho 'xóa flow này', 'delete flow'.",
            "inputSchema": { "type": "object", "properties": {
                "flow_id": { "type": "string", "description": "Id flow cần xóa." }
            }, "required": ["flow_id"] }
        },
        {
            "name": "lake_flow_generate",
            "description": "Sinh DRAFT một flow từ mô tả tự nhiên (qua LLM bridge). Nếu cho connection_id sẽ introspect schema nguồn để dùng đúng tên bảng/cột. Trả draft (chưa lưu, chưa bật) + DAG để kiểm — tạo thật bằng lake_flow_create{def: draft}. Dùng cho 'tạo pipeline từ mô tả', 'generate flow'.",
            "inputSchema": { "type": "object", "properties": {
                "description":   { "type": "string", "description": "Mô tả pipeline cần sinh (nguồn, bảng, tần suất, biến đổi). Bắt buộc." },
                "connection_id": { "type": "string", "description": "Connection để introspect schema nguồn (tùy chọn — có thì draft dùng đúng tên bảng/cột)." }
            }, "required": ["description"] }
        },
        {
            "name": "lake_flow_run",
            "description": "Kích hoạt một run cho flow (bất đồng bộ). Trả run_id — poll bằng lake_run_status, ĐỪNG chờ đồng bộ. 409 nếu flow đang chạy, 429 nếu hàng đợi đầy. Dùng cho 'chạy flow này', 'run pipeline'.",
            "inputSchema": { "type": "object", "properties": {
                "flow_id": { "type": "string", "description": "Id flow cần chạy." }
            }, "required": ["flow_id"] }
        },
        {
            "name": "lake_flow_backfill",
            "description": "Chạy lại (backfill) một flow trên dải thời gian [start,end). Per-step: transform incremental_by_time chạy lại range (idempotent); transform full + source bị SKIP mặc định; muốn làm lại phải liệt kê trong 'rebuild' (merge/SCD2 rebuild MẤT lịch sử → cần confirm=true). Dùng cho 'backfill', 'chạy lại dữ liệu quá khứ'.",
            "inputSchema": { "type": "object", "properties": {
                "flow_id": { "type": "string", "description": "Id flow." },
                "start":   { "type": "string", "description": "Mốc bắt đầu (YYYY-MM-DD hoặc YYYY-MM-DD HH:MM:SS)." },
                "end":     { "type": "string", "description": "Mốc kết thúc (loại trừ)." },
                "steps":   { "type": "array", "items": { "type": "string" }, "description": "Chỉ backfill các step này (mặc định tất cả)." },
                "rebuild": { "type": "array", "items": { "type": "string" }, "description": "Step chạy lại full-refresh-equivalent (transform full / source merge/SCD2)." },
                "confirm": { "type": "boolean", "description": "Xác nhận rebuild merge/SCD2 (mất lịch sử). Mặc định false." }
            }, "required": ["flow_id", "start", "end"] }
        },
        {
            "name": "lake_run_status",
            "description": "Xem trạng thái + tiến độ per-step của một run (queued/running/success/failed/cancelled). Poll tool này sau lake_flow_run. Dùng cho 'run xong chưa', 'run status'.",
            "inputSchema": { "type": "object", "properties": {
                "run_id": { "type": "string", "description": "Id run." }
            }, "required": ["run_id"] }
        },
        {
            "name": "lake_run_list",
            "description": "Liệt kê các run (lọc theo flow/status). Dùng cho 'lịch sử chạy', 'list runs'.",
            "inputSchema": { "type": "object", "properties": {
                "flow_id": { "type": "string", "description": "Lọc theo flow." },
                "status":  { "type": "string", "description": "Lọc theo status (queued/running/success/failed/cancelled)." },
                "limit":   { "type": "integer", "description": "Số run tối đa (mặc định 100)." },
                "offset":  { "type": "integer", "description": "Phân trang." }
            } }
        },
        {
            "name": "lake_run_cancel",
            "description": "Hủy một run đang chạy/chờ. Set cờ hủy (worker dừng giữa batch) + ép trạng thái sang cancelled. Dùng cho 'dừng run này', 'cancel run'.",
            "inputSchema": { "type": "object", "properties": {
                "run_id": { "type": "string", "description": "Id run cần hủy." }
            }, "required": ["run_id"] }
        },
        {
            "name": "lake_run_logs",
            "description": "Xem log của một run (mặc định 100 dòng cuối, tối đa 500). Dùng để chẩn đoán khi run failed. Dùng cho 'log của run', 'run logs'.",
            "inputSchema": { "type": "object", "properties": {
                "run_id": { "type": "string", "description": "Id run." },
                "tail":   { "type": "integer", "description": "Số dòng cuối (mặc định 100, clamp 500)." }
            }, "required": ["run_id"] }
        },
        {
            "name": "lake_lineage",
            "description": "Xem lineage up/downstream của một dataset: dataset nào nuôi nó (upstream) và dataset nào phái sinh từ nó (downstream), suy từ định nghĩa mọi flow. Dùng cho 'dataset này đến từ đâu', 'đổi bảng này ảnh hưởng gì', 'lineage'.",
            "inputSchema": { "type": "object", "properties": {
                "namespace": { "type": "string", "description": "Namespace dataset." },
                "dataset":   { "type": "string", "description": "Tên dataset." },
                "depth":     { "type": "integer", "description": "Số bậc lan tỏa mỗi chiều (mặc định 2, clamp 1..10)." }
            }, "required": ["namespace", "dataset"] }
        }
    ])
}

// ---------------------------------------------------------------------------
// dispatch
// ---------------------------------------------------------------------------

async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    let db = &state.db;
    match name {
        "lake_stats" => from_logic(api::logic_stats(db)),

        "lake_dataset_list" => from_logic(api::logic_dataset_list(
            db,
            opt_s(args, "namespace").as_deref(),
            opt_int(args, "limit").unwrap_or(100),
            opt_int(args, "offset").unwrap_or(0),
        )),

        "lake_dataset_schema" => {
            let ns = s(args, "namespace");
            let ds = s(args, "dataset");
            from_logic(api::logic_dataset_get(db, &ns, &ds))
        }

        "lake_dataset_preview" => {
            let ns = s(args, "namespace");
            let ds = s(args, "dataset");
            let limit = opt_int(args, "limit").unwrap_or(50);
            from_logic(api::logic_dataset_preview(db, &ns, &ds, limit).await)
        }

        "lake_dataset_delete" => {
            let ns = s(args, "namespace");
            let ds = s(args, "dataset");
            from_logic(api::logic_dataset_delete(db, &ns, &ds))
        }

        "lake_import_file" => {
            let filename = s(args, "filename");
            if filename.is_empty() {
                return error_result("filename là bắt buộc".into());
            }
            let bytes = match resolve_import_bytes(db, args) {
                Ok(b) => b,
                Err(msg) => return error_result(msg),
            };
            from_logic(api::logic_import(
                db,
                &filename,
                &bytes,
                opt_s(args, "namespace").as_deref(),
                opt_s(args, "dataset").as_deref(),
            ))
        }

        "lake_dataset_export" => {
            let ns = s(args, "namespace");
            let ds = s(args, "dataset");
            let format = s(args, "format");
            from_logic(
                api::logic_dataset_export(db, &ns, &ds, &format, opt_s(args, "sql").as_deref()).await,
            )
        }

        "lake_dataset_compact" => {
            let ns = s(args, "namespace");
            let ds = s(args, "dataset");
            from_logic(api::logic_dataset_compact(db, &ns, &ds))
        }

        "lake_query" => {
            let sql = s(args, "sql");
            from_logic(api::logic_query(db, &sql, opt_int(args, "limit"), opt_int(args, "offset")).await)
        }

        "lake_query_explain" => {
            let sql = s(args, "sql");
            from_logic(api::logic_explain(db, &sql).await)
        }

        // ---- connections ----
        "lake_connection_add" => from_logic(
            api::logic_connection_add(db, opt_s(args, "id").as_deref(), &s(args, "kind"), &s(args, "dsn"))
                .await,
        ),
        "lake_connection_list" => from_logic(api::logic_connection_list(db)),
        "lake_connection_test" => {
            from_logic(api::logic_connection_test(db, &s(args, "connection_id")).await)
        }
        "lake_connection_delete" => {
            from_logic(api::logic_connection_delete(db, &s(args, "connection_id")))
        }
        "lake_db_introspect" => from_logic(
            api::logic_connection_introspect(db, &s(args, "connection_id"), opt_s(args, "schema").as_deref())
                .await,
        ),

        // ---- flows ----
        "lake_flow_create" => from_logic(api::logic_flow_create(
            db,
            &args["def"],
            args["enable"].as_bool().unwrap_or(false),
        )),
        "lake_flow_update" => from_logic(api::logic_flow_update(
            db,
            &s(args, "flow_id"),
            &args["def"],
            args["confirm_reset"].as_bool().unwrap_or(false),
        )),
        "lake_flow_list" => from_logic(api::logic_flow_list(db)),
        "lake_flow_get" => from_logic(api::logic_flow_get(db, &s(args, "flow_id"))),
        "lake_flow_delete" => from_logic(api::logic_flow_delete(db, &s(args, "flow_id"))),
        "lake_flow_generate" => from_logic(
            api::logic_flow_generate(db, &s(args, "description"), opt_s(args, "connection_id").as_deref())
                .await,
        ),
        "lake_flow_run" => {
            from_logic(api::logic_flow_run(db, Some(&state.hub), &s(args, "flow_id")))
        }
        "lake_flow_backfill" => {
            let str_list = |key: &str| -> Vec<String> {
                args[key]
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_default()
            };
            let steps = if args["steps"].is_array() { Some(str_list("steps")) } else { None };
            from_logic(
                api::logic_flow_backfill(
                    db,
                    &s(args, "flow_id"),
                    &s(args, "start"),
                    &s(args, "end"),
                    steps,
                    str_list("rebuild"),
                    args["confirm"].as_bool().unwrap_or(false),
                )
                .await,
            )
        }

        // ---- runs ----
        "lake_run_status" => from_logic(api::logic_run_get(db, &s(args, "run_id"))),
        "lake_run_list" => from_logic(api::logic_run_list(
            db,
            opt_s(args, "flow_id").as_deref(),
            opt_s(args, "status").as_deref(),
            opt_int(args, "limit").unwrap_or(100),
            opt_int(args, "offset").unwrap_or(0),
        )),
        "lake_run_cancel" => {
            from_logic(api::logic_run_cancel(db, &state.cancels, &s(args, "run_id")))
        }
        "lake_run_logs" => from_logic(api::logic_run_logs(
            db,
            &s(args, "run_id"),
            opt_int(args, "tail").unwrap_or(100),
        )),

        "lake_lineage" => from_logic(api::logic_lineage(
            db,
            &s(args, "namespace"),
            &s(args, "dataset"),
            opt_int(args, "depth").unwrap_or(2),
        )),

        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

// ---------------------------------------------------------------------------
// import: base64 (có cap) vs path (allowlist) — chặn local-file-disclosure
// ---------------------------------------------------------------------------

/// Lấy bytes để import: ưu tiên `content_base64` (cap `import_base64_max_mb`), nếu
/// không có thì đọc từ `path` — CHỈ khi path nằm trong allowlist (inbox/ + import_paths).
fn resolve_import_bytes(db: &crate::db::Db, args: &Value) -> Result<Vec<u8>, String> {
    if let Some(b64) = opt_s(args, "content_base64") {
        let bytes = api::decode_base64_maybe_data_url(&b64)?;
        // Cap dùng chung với REST (§import) — không lặp lại logic.
        api::check_base64_import_cap(db, bytes.len())?;
        return Ok(bytes);
    }
    if let Some(path) = opt_s(args, "path") {
        return read_allowed_path(db, &path);
    }
    Err("cần 'content_base64' hoặc 'path'".to_string())
}

/// Đọc file tại `path` chỉ khi canonicalize được và nằm DƯỚI một thư mục allowlist
/// (inbox/ luôn có + mỗi mục trong setting import_paths). Chặn `../` bằng cách so
/// prefix trên đường dẫn ĐÃ canonicalize (đã resolve symlink/`..`).
fn read_allowed_path(db: &crate::db::Db, path: &str) -> Result<Vec<u8>, String> {
    let target = PathBuf::from(path)
        .canonicalize()
        .map_err(|e| format!("không mở được path '{path}': {e}"))?;

    let mut allowed: Vec<PathBuf> = vec![config::inbox_dir()];
    if let Ok(v) = serde_json::from_str::<Vec<String>>(&db.setting("import_paths", "[]")) {
        allowed.extend(v.into_iter().map(PathBuf::from));
    }
    let allowed_canon: Vec<PathBuf> = allowed
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .collect();

    let ok = allowed_canon.iter().any(|root| is_under(&target, root));
    if !ok {
        return Err(format!(
            "path '{path}' ngoài allowlist import (inbox/ + import_paths) — bị chặn"
        ));
    }
    std::fs::read(&target).map_err(|e| format!("đọc file '{path}' thất bại: {e}"))
}

/// `target` có nằm dưới `root` không (so trên đường dẫn đã canonicalize).
fn is_under(target: &Path, root: &Path) -> bool {
    target.starts_with(root)
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use std::sync::Arc;

    fn state() -> AppState {
        crate::api::test_state()
    }

    fn b64(bytes: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    /// Mọi tool quảng cáo phải có nhánh dispatch — không tool nào rơi "không tồn tại".
    #[tokio::test]
    async fn every_advertised_tool_is_dispatchable() {
        let st = state();
        for tool in tools_list().as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            let result = call_tool(&st, name, &json!({})).await;
            let text = result["content"][0]["text"].as_str().unwrap_or("");
            assert!(
                !text.contains("tool không tồn tại"),
                "{name} quảng cáo nhưng không có nhánh dispatch"
            );
        }
    }

    #[tokio::test]
    async fn import_csv_then_list_query_preview() {
        let st = state();
        let csv = "id,city\n1,hanoi\n2,hue\n3,hcm\n";
        let out = call_tool(
            &st,
            "lake_import_file",
            &json!({ "filename": "places.csv", "content_base64": b64(csv.as_bytes()), "namespace": "raw" }),
        )
        .await;
        assert!(out.get("isError").is_none(), "import lỗi: {out}");
        let payload: Value =
            serde_json::from_str(out["content"][0]["text"].as_str().unwrap()).unwrap();
        let ds = payload["datasets"][0]["dataset"].as_str().unwrap().to_string();

        // list thấy dataset.
        let list = call_tool(&st, "lake_dataset_list", &json!({})).await;
        let lp: Value = serde_json::from_str(list["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(lp["total"], json!(1));

        // query đếm đúng 3 dòng.
        let q = call_tool(
            &st,
            "lake_query",
            &json!({ "sql": format!("SELECT count(*) AS n FROM raw.\"{ds}\"") }),
        )
        .await;
        let qp: Value = serde_json::from_str(q["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(qp["rows"][0][0], json!(3));

        // preview trả dòng.
        let pv = call_tool(
            &st,
            "lake_dataset_preview",
            &json!({ "namespace": "raw", "dataset": ds, "limit": 2 }),
        )
        .await;
        let pp: Value = serde_json::from_str(pv["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(pp["returned"], json!(2));
    }

    #[tokio::test]
    async fn query_blocks_insert() {
        let st = state();
        let out = call_tool(
            &st,
            "lake_query",
            &json!({ "sql": "INSERT INTO raw.x VALUES (1)" }),
        )
        .await;
        assert_eq!(out["isError"], json!(true), "INSERT phải bị chặn");
    }

    #[test]
    fn import_path_outside_allowlist_is_blocked() {
        let db = Db::open_memory().unwrap();
        // File ngoài inbox/ + import_paths (rỗng) → bị chặn.
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut tmp.as_file(), b"id,x\n1,a\n").unwrap();
        let err = read_allowed_path(&db, tmp.path().to_str().unwrap()).unwrap_err();
        assert!(err.contains("ngoài allowlist"), "kỳ vọng chặn: {err}");
    }

    #[test]
    fn import_path_inside_allowlist_reads() {
        let db = Db::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let canon = dir.path().canonicalize().unwrap();
        db.set_setting("import_paths", &json!([canon.to_str().unwrap()]).to_string())
            .unwrap();
        let fpath = canon.join("d.csv");
        std::fs::write(&fpath, b"id\n1\n").unwrap();
        let bytes = read_allowed_path(&db, fpath.to_str().unwrap()).unwrap();
        assert_eq!(bytes, b"id\n1\n");
    }

    #[test]
    fn base64_over_cap_points_to_path() {
        let db = Db::open_memory().unwrap();
        db.set_setting("import_base64_max_mb", "1").unwrap();
        let big = vec![b'a'; 2 * 1024 * 1024]; // 2MB > cap 1MB
        let err = resolve_import_bytes(&db, &json!({ "content_base64": b64(&big) })).unwrap_err();
        assert!(err.contains("path"), "lỗi phải trỏ sang path: {err}");
    }

    // ---- helpers cho integration test connector qua SQLite ----

    /// Tạo file SQLite nguồn + seed rows events(id INTEGER, label TEXT).
    fn seed_sqlite(path: &str, rows: &[(i64, &str)]) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS events (id INTEGER, label TEXT);")
            .unwrap();
        for (id, label) in rows {
            conn.execute(
                "INSERT INTO events (id, label) VALUES (?1, ?2)",
                rusqlite::params![id, label],
            )
            .unwrap();
        }
    }

    /// Rút payload JSON từ content text của một kết quả MCP (không phải isError).
    fn payload(v: &Value) -> Value {
        assert!(v.get("isError").is_none(), "kỳ vọng thành công, nhận: {v}");
        serde_json::from_str(v["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    /// End-to-end qua đúng MCP surface: add sqlite connection → introspect →
    /// flow_create (full_refresh) → flow_run → thực thi → run_status success →
    /// lake_query thấy dữ liệu → run_logs có dòng. (Poller không chạy trong test nên
    /// gọi runner::execute_run_at thẳng — land vào config::lake_dir() để lake_query đọc.)
    #[tokio::test]
    async fn e2e_sqlite_connection_flow_run_query_logs() {
        use std::sync::atomic::AtomicBool;
        let st = state();
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src_e2e.sqlite");
        seed_sqlite(src_path.to_str().unwrap(), &[(1, "a"), (2, "b"), (3, "c")]);

        // Dataset độc nhất để không đụng file leaked của test khác trong lake dir chung.
        let uniq = uuid::Uuid::now_v7().simple().to_string();
        let ds_name = format!("events_e2e_{}", &uniq[..8]);
        let flow_id = format!("e2e{}", &uniq[..8]);

        // 1) add sqlite connection (test-before-save chạy thật với file sống).
        let add = call_tool(
            &st,
            "lake_connection_add",
            &json!({ "id": "src_e2e", "kind": "sqlite", "dsn": src_path.to_str().unwrap() }),
        )
        .await;
        let ap = payload(&add);
        assert_eq!(ap["ok"], json!(true));

        // 2) introspect thấy bảng events + cột.
        let intro = call_tool(&st, "lake_db_introspect", &json!({ "connection_id": "src_e2e" })).await;
        let ip = payload(&intro);
        assert!(
            ip["tables"].as_array().unwrap().iter().any(|t| t["name"] == json!("events")),
            "introspect phải thấy bảng events: {ip}"
        );

        // 3) flow_create full_refresh, target raw.<ds_name>.
        let def = json!({
            "flow": flow_id,
            "sources": [{
                "id": "events", "connection": "src_e2e", "table": "events",
                "mode": "full_refresh",
                "target": { "namespace": "raw", "dataset": ds_name }
            }]
        });
        let created = call_tool(&st, "lake_flow_create", &json!({ "def": def })).await;
        let cp = payload(&created);
        assert_eq!(cp["ok"], json!(true));
        assert!(cp["dag"].as_array().is_some(), "flow_create trả DAG: {cp}");

        // 4) flow_run → run_id (enqueued).
        let run = call_tool(&st, "lake_flow_run", &json!({ "flow_id": flow_id })).await;
        let rp = payload(&run);
        let run_id = rp["run_id"].as_str().unwrap().to_string();

        // Thực thi thật (poller vắng mặt trong test) — land vào lake dir mặc định.
        let cancel = Arc::new(AtomicBool::new(false));
        crate::runner::execute_run_at(&crate::config::lake_dir(), &st.db, &run_id, cancel)
            .await
            .unwrap();

        // 5) poll run_status → success.
        let status = call_tool(&st, "lake_run_status", &json!({ "run_id": run_id })).await;
        let sp = payload(&status);
        assert_eq!(sp["run"]["status"], json!("success"), "run phải success: {sp}");

        // 6) lake_query thấy đủ 3 dòng.
        let q = call_tool(
            &st,
            "lake_query",
            &json!({ "sql": format!("SELECT count(*) AS n FROM raw.\"{ds_name}\"") }),
        )
        .await;
        let qp = payload(&q);
        assert_eq!(qp["rows"][0][0], json!(3), "lake_query phải thấy 3 dòng: {qp}");

        // 7) run_logs có dòng.
        let logs = call_tool(&st, "lake_run_logs", &json!({ "run_id": run_id })).await;
        let lp = payload(&logs);
        assert!(lp["returned"].as_i64().unwrap() > 0, "run_logs phải có dòng: {lp}");
    }

    /// Xóa connection bị chặn khi còn flow tham chiếu (409 → isError).
    #[tokio::test]
    async fn connection_delete_blocked_when_flow_references() {
        let st = state();
        // Chèn connection thẳng (bỏ qua test-before-save — không cần nguồn sống ở đây).
        st.db
            .connection_add("src_ref", "sqlite", "/tmp/whatever.sqlite")
            .unwrap();
        let def = json!({
            "flow": "refflow",
            "sources": [{
                "id": "s1", "connection": "src_ref", "table": "t",
                "mode": "full_refresh",
                "target": { "namespace": "raw", "dataset": "ref_s1" }
            }]
        });
        let created = call_tool(&st, "lake_flow_create", &json!({ "def": def })).await;
        assert!(created.get("isError").is_none(), "flow_create lỗi: {created}");

        let del = call_tool(&st, "lake_connection_delete", &json!({ "connection_id": "src_ref" })).await;
        assert_eq!(del["isError"], json!(true), "xóa phải bị chặn");
        assert!(
            del["content"][0]["text"].as_str().unwrap().contains("còn flow tham chiếu"),
            "thông báo phải nêu flow tham chiếu: {del}"
        );
    }

    /// DSN LUÔN redact ở list (mật khẩu bị ẩn).
    #[tokio::test]
    async fn connection_list_redacts_dsn() {
        let st = state();
        // Chèn thẳng một DSN có mật khẩu (add() sẽ thử kết nối postgres — không có ở test).
        st.db
            .connection_add("pg", "postgres", "postgres://user:secret@host:5432/db")
            .unwrap();
        let list = call_tool(&st, "lake_connection_list", &json!({})).await;
        let lp = payload(&list);
        let pg = lp["connections"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == json!("pg"))
            .unwrap();
        assert_eq!(pg["dsn"], json!("postgres://user:•••@host:5432/db"), "DSN phải redact");
        assert!(
            !pg["dsn"].as_str().unwrap().contains("secret"),
            "mật khẩu không được lộ: {pg}"
        );
    }
}
