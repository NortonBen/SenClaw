//! REST surface — route table trong docs/data-lake-app-design.md §8.
//! Paths ĐĂNG KÝ KHÔNG có tiền tố `/api`; `main.rs` nest router này dưới `/api`.
//! Error envelope: status code + `{"error": string}` (kiểu rewrite-story).
//!
//! REST ↔ MCP parity (§8/§9): mọi nghiệp vụ nằm trong các hàm `logic_*` dùng CHUNG;
//! REST map `ApiError.code` sang HTTP, MCP map lỗi sang `isError`. Tên tham số 1:1.

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::config;
use crate::connectors::{self, redact_dsn};
use crate::db::{run_status, ConnectionInfo, Db};
use crate::engine;
use crate::flow;
use crate::ingest;
use crate::lake;
use crate::runner::{self, EnqueueOutcome};

/// State dùng chung cho REST + MCP. Chỉ giữ catalog — engine/lake tự mở session
/// per-request (SessionContext không tái dùng được qua await an toàn).
/// `hub` phát sự kiện dashboard; `cancels` cho phép cancel một run đang chạy.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub hub: crate::dashws::DashHub,
    pub cancels: crate::runner::CancelRegistry,
}

/// Mở Db (tạo mọi thư mục data cần thiết). Gọi một lần lúc boot.
pub fn make_state() -> anyhow::Result<AppState> {
    for dir in [
        config::data_dir(),
        config::lake_dir(),
        config::inbox_dir(),
        config::exports_dir(),
    ] {
        std::fs::create_dir_all(&dir)
            .map_err(|e| anyhow::anyhow!("tạo thư mục '{}' thất bại: {e}", dir.display()))?;
    }
    let db = Db::open(&config::db_path())?;
    Ok(AppState {
        db: Arc::new(db),
        hub: crate::dashws::DashHub::new(),
        cancels: crate::runner::new_cancel_registry(),
    })
}

/// State test-only: db in-memory + hub trống + registry mới. Tránh lặp ở mỗi test module.
#[cfg(test)]
pub fn test_state() -> AppState {
    AppState {
        db: Arc::new(Db::open_memory().unwrap()),
        hub: crate::dashws::DashHub::new(),
        cancels: crate::runner::new_cancel_registry(),
    }
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/health", get(status))
        .route("/datasets", get(list_datasets))
        .route("/datasets/:ns/:name", get(get_dataset).delete(delete_dataset))
        .route("/datasets/:ns/:name/preview", get(preview_dataset))
        .route("/datasets/:ns/:name/lineage", get(dataset_lineage))
        .route("/datasets/:ns/:name/compact", post(compact_dataset))
        .route("/import", post(import))
        .route("/query", post(query))
        .route("/query/explain", post(query_explain))
        .route("/query/export", post(query_export))
        .route("/exports/:file", get(download_export))
        .route("/settings", get(get_settings).put(put_settings))
        // connections (§8)
        .route("/connections", get(list_connections).post(add_connection))
        .route("/connections/:id", axum::routing::delete(delete_connection))
        .route("/connections/:id/test", post(test_connection))
        .route("/connections/:id/introspect", get(introspect_connection))
        // flows (§8)
        .route("/flows", get(list_flows).post(create_flow))
        .route("/flows/generate", post(generate_flow))
        .route(
            "/flows/:id",
            get(get_flow).put(update_flow).delete(delete_flow),
        )
        .route("/flows/:id/run", post(run_flow))
        .route("/flows/:id/backfill", post(backfill_flow))
        .route("/flows/:id/enable", post(enable_flow))
        // runs (§8)
        .route("/runs", get(list_runs))
        .route("/runs/:id", get(get_run))
        .route("/runs/:id/cancel", post(cancel_run))
        .route("/runs/:id/logs", get(get_run_logs))
        // WS dashboard (§6.7)
        .route("/ws/dashboard", get(crate::dashws::ws_dashboard))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        // Trần body cho /import (base64 64MB) — áp cả router, các route khác nhẹ.
        .layer(DefaultBodyLimit::max(64 * 1024 * 1024))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// ApiError — nghiệp vụ trả lỗi có status; REST→HTTP, MCP→isError
// ---------------------------------------------------------------------------

/// Lỗi nghiệp vụ mang mã HTTP để REST map thẳng và MCP quy về isError.
/// `details` (nếu có) mang payload phụ — vd danh sách `FieldError` khi validate flow
/// (§6.1/§8): REST nhét vào body cạnh `error`, MCP nối vào text isError.
#[derive(Debug)]
pub struct ApiError {
    pub code: StatusCode,
    pub msg: String,
    pub details: Option<Value>,
}

impl ApiError {
    pub fn new(code: StatusCode, msg: impl Into<String>) -> Self {
        Self {
            code,
            msg: msg.into(),
            details: None,
        }
    }
    pub fn bad(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, msg)
    }
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, msg)
    }
    pub fn conflict(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, msg)
    }
    pub fn too_many(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, msg)
    }
    pub fn with_details(mut self, details: Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

// ---------------------------------------------------------------------------
// response helpers
// ---------------------------------------------------------------------------

fn respond(code: StatusCode, v: Value) -> Response {
    (code, Json(v)).into_response()
}

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(json!({ "error": msg.into() }))).into_response()
}

/// Map `Result<Value, ApiError>` của một hàm logic sang Response (200 khi Ok).
/// Lỗi có `details` → body `{error, details}`; không → `{error}`.
fn ok_or(result: Result<Value, ApiError>) -> Response {
    match result {
        Ok(v) => respond(StatusCode::OK, v),
        Err(e) => match e.details {
            Some(d) => respond(e.code, json!({ "error": e.msg, "details": d })),
            None => err(e.code, e.msg),
        },
    }
}

type MaybeJson = Result<Json<Value>, JsonRejection>;

#[allow(clippy::result_large_err)]
fn parse_body<T: for<'de> Deserialize<'de>>(body: MaybeJson) -> Result<T, Response> {
    match body {
        Ok(Json(v)) => {
            serde_json::from_value(v).map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))
        }
        Err(e) => Err(err(StatusCode::BAD_REQUEST, e.to_string())),
    }
}

// ---------------------------------------------------------------------------
// shared logic — REST + MCP gọi chung
// ---------------------------------------------------------------------------

/// Chuẩn hoá một mảnh tên thành identifier an toàn (chữ/số/underscore, thường hoá).
/// Rỗng → "table". Giữ nguyên đủ để làm tên bảng SQL không cần quote phức tạp.
pub(crate) fn sanitize_ident(s: &str) -> String {
    let mut out: String = s
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    out = out.trim_matches('_').to_lowercase();
    if out.is_empty() {
        "table".to_string()
    } else {
        out
    }
}

/// `GET /status` — health + LakeStats thật.
pub(crate) fn logic_stats(db: &Db) -> Result<Value, ApiError> {
    let s = db.stats()?;
    Ok(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "datasets": s.datasets,
        "total_rows": s.total_rows,
        "total_bytes": s.total_bytes,
        "runs_active": s.runs_active,
        "runs_24h": s.runs_24h,
        "next": "Dùng lake_dataset_list để xem dataset, lake_query để truy vấn."
    }))
}

pub(crate) fn logic_dataset_list(
    db: &Db,
    namespace: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Value, ApiError> {
    let rows = db.dataset_list(namespace, limit, offset)?;
    Ok(json!({
        "total": rows.len(),
        "datasets": rows,
        "next": "Xem schema bằng lake_dataset_schema, dữ liệu mẫu bằng lake_dataset_preview."
    }))
}

/// Chi tiết dataset: meta + schema hiện tại + lịch sử version + file summary.
pub(crate) fn logic_dataset_get(db: &Db, ns: &str, name: &str) -> Result<Value, ApiError> {
    let ds = db
        .dataset_get(ns, name)?
        .ok_or_else(|| ApiError::not_found(format!("không có dataset {ns}.{name}")))?;
    let schema = match db.schema_version_current(ds.id)? {
        Some(sv) => serde_json::from_str::<Value>(&sv.arrow_schema).unwrap_or(Value::Null),
        None => Value::Null,
    };
    let versions = db.schema_version_history(ds.id)?;
    let files = db.manifest_active_files(ds.id)?;
    Ok(json!({
        "dataset": ds,
        "schema": schema,
        "schema_versions": versions,
        "files": {
            "active": files.len(),
            "bytes": files.iter().map(|f| f.byte_size).sum::<i64>(),
        },
        "owner_flow_id": ds.owner_flow_id,
        "next": "Xem dữ liệu bằng lake_dataset_preview hoặc lake_query."
    }))
}

/// Preview N dòng đầu — clamp 200. Đọc qua engine (manifest + schema catalog).
pub(crate) async fn logic_dataset_preview(
    db: &Db,
    ns: &str,
    name: &str,
    limit: i64,
) -> Result<Value, ApiError> {
    // 404 nếu dataset không tồn tại (query rỗng cũng OK nhưng báo rõ hơn).
    if db.dataset_get(ns, name)?.is_none() {
        return Err(ApiError::not_found(format!("không có dataset {ns}.{name}")));
    }
    let limit = limit.clamp(1, 200);
    let sql = format!("SELECT * FROM \"{ns}\".\"{name}\"");
    let page = engine::query_page(db, &sql, Some(limit), Some(0))
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    Ok(json!({
        "namespace": ns,
        "dataset": name,
        "columns": page.columns,
        "rows": page.rows,
        "returned": page.returned,
        "has_more": page.has_more,
        "next": "Truy vấn đầy đủ bằng lake_query với SQL + LIMIT."
    }))
}

/// Xoá dataset — 404 nếu không có; 409 nếu flow chủ đang có run active (§8).
/// File vật lý dọn sau bởi GC/reconcile (catalog không đụng filesystem).
pub(crate) fn logic_dataset_delete(db: &Db, ns: &str, name: &str) -> Result<Value, ApiError> {
    let ds = db
        .dataset_get(ns, name)?
        .ok_or_else(|| ApiError::not_found(format!("không có dataset {ns}.{name}")))?;
    if let Some(flow) = ds.owner_flow_id.as_deref() {
        let active = db.run_list(Some(flow), None, 50, 0)?;
        if active.iter().any(|r| run_status::is_active(&r.status)) {
            return Err(ApiError::conflict(format!(
                "dataset {ns}.{name} thuộc flow '{flow}' đang chạy — huỷ run trước khi xoá"
            )));
        }
    }
    db.dataset_delete(ds.id)?;
    Ok(json!({
        "ok": true,
        "deleted": format!("{ns}.{name}"),
        "next": "File Parquet sẽ được GC dọn sau; dùng lake_dataset_list để xác nhận."
    }))
}

/// Import bytes đã giải mã thành một hoặc nhiều dataset. `dataset` override tên
/// CHỈ khi ingest ra đúng một bảng; nhiều bảng (sheet Excel) giữ tên bảng gốc.
/// Cap độ dài payload base64 ĐÃ decode (setting `import_base64_max_mb`, kẹp 1..=64).
/// DÙNG CHUNG REST (/import base64-only) và MCP (resolve_import_bytes nhánh base64) —
/// trước đây chỉ MCP check nên REST base64 chỉ bị chặn bởi DefaultBodyLimit 64MB.
/// KHÔNG áp cho import theo `path`: path là escape hatch cho file lớn.
pub(crate) fn check_base64_import_cap(db: &Db, decoded_len: usize) -> Result<(), String> {
    let cap_mb = db.setting_i64("import_base64_max_mb", 10).clamp(1, 64);
    let cap = cap_mb as usize * 1024 * 1024;
    if decoded_len > cap {
        return Err(format!(
            "nội dung {decoded_len} bytes vượt cap base64 {cap_mb}MB — dùng tham số 'path' cho file lớn"
        ));
    }
    Ok(())
}

/// Import từ base64 (đường REST): decode → enforce cap → logic_import. Tách riêng để test
/// được cả nhánh cap mà không cần dựng router.
pub(crate) fn logic_import_b64(
    db: &Db,
    filename: &str,
    content_base64: &str,
    namespace: Option<&str>,
    dataset: Option<&str>,
) -> Result<Value, ApiError> {
    let bytes = decode_base64_maybe_data_url(content_base64).map_err(ApiError::bad)?;
    check_base64_import_cap(db, bytes.len())
        .map_err(|e| ApiError::new(StatusCode::PAYLOAD_TOO_LARGE, e))?;
    logic_import(db, filename, &bytes, namespace, dataset)
}

pub(crate) fn logic_import(
    db: &Db,
    filename: &str,
    bytes: &[u8],
    namespace: Option<&str>,
    dataset: Option<&str>,
) -> Result<Value, ApiError> {
    if bytes.is_empty() {
        return Err(ApiError::bad("nội dung import rỗng"));
    }
    let tables = ingest::ingest(filename, bytes).map_err(|e| ApiError::bad(e.to_string()))?;
    if tables.is_empty() {
        return Err(ApiError::bad(format!(
            "không nhận ra bảng dữ liệu nào trong '{filename}'"
        )));
    }
    let ns = sanitize_ident(namespace.unwrap_or("raw"));
    let run_id = uuid::Uuid::now_v7().to_string();
    let single = tables.len() == 1;
    let mut created = Vec::new();
    for t in &tables {
        let name = if single {
            dataset
                .map(sanitize_ident)
                .unwrap_or_else(|| sanitize_ident(&t.name))
        } else {
            sanitize_ident(&t.name)
        };
        let c = lake::create_dataset_from_ingested(db, &ns, &name, t, &run_id)
            .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        created.push(json!({
            "namespace": ns,
            "dataset": name,
            "rows": c.row_count,
            "origin": t.origin,
            "note": t.note,
        }));
    }
    Ok(json!({
        "ok": true,
        "run_id": run_id,
        "datasets": created,
        "next": "Xem bằng lake_dataset_list, truy vấn bằng lake_query."
    }))
}

pub(crate) async fn logic_query(
    db: &Db,
    sql: &str,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<Value, ApiError> {
    if sql.trim().is_empty() {
        return Err(ApiError::bad("sql rỗng"));
    }
    let page = engine::query_page(db, sql, limit, offset)
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    Ok(json!({
        "columns": page.columns,
        "rows": page.rows,
        "returned": page.returned,
        "has_more": page.has_more,
        "total_estimate": page.total_estimate,
        "next": if page.has_more {
            "Còn dòng — tăng offset để lấy trang tiếp."
        } else {
            "Đã trả hết kết quả."
        }
    }))
}

pub(crate) async fn logic_explain(db: &Db, sql: &str) -> Result<Value, ApiError> {
    if sql.trim().is_empty() {
        return Err(ApiError::bad("sql rỗng"));
    }
    let plan = engine::explain(db, sql)
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    Ok(json!({
        "plan": plan,
        "next": "Nếu plan quét toàn bảng, thêm điều kiện lọc trước khi lake_query."
    }))
}

/// Export kết quả một SELECT tùy ý ra file (§8 POST /query/export). Ghi file đầy đủ
/// vào exports/, trả path + cửa sổ preview inline.
pub(crate) async fn logic_query_export(
    db: &Db,
    sql: &str,
    format: &str,
) -> Result<Value, ApiError> {
    let fmt = crate::export::ExportFormat::parse(format).map_err(|e| ApiError::bad(e.to_string()))?;
    if sql.trim().is_empty() {
        return Err(ApiError::bad("sql rỗng"));
    }
    let rep = crate::export::export_query(db, sql, fmt)
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    Ok(export_report_json(rep))
}

/// Export một dataset ra file (MCP lake_dataset_export). `sql` tùy chọn (filter/projection).
pub(crate) async fn logic_dataset_export(
    db: &Db,
    ns: &str,
    dataset: &str,
    format: &str,
    sql: Option<&str>,
) -> Result<Value, ApiError> {
    let fmt = crate::export::ExportFormat::parse(format).map_err(|e| ApiError::bad(e.to_string()))?;
    // 404 khi dataset không tồn tại (export_dataset trả anyhow "không có dataset").
    if db.dataset_get(ns, dataset)?.is_none() {
        return Err(ApiError::not_found(format!("không có dataset {ns}.{dataset}")));
    }
    let rep = crate::export::export_dataset(db, ns, dataset, fmt, sql)
        .await
        .map_err(|e| ApiError::bad(e.to_string()))?;
    Ok(export_report_json(rep))
}

fn export_report_json(rep: crate::export::ExportReport) -> Value {
    json!({
        "ok": true,
        "file": rep.file,
        "path": rep.path,
        "format": rep.format,
        "rows": rep.rows,
        "bytes": rep.bytes,
        "columns": rep.columns,
        "preview": rep.preview,
        "download_url": format!("/api/exports/{}", rep.file),
        "next": "Tải file đầy đủ qua GET /api/exports/<file>; preview chỉ là cửa sổ nhỏ."
    })
}

/// Compaction một dataset (§12 Phase 4 / POST /datasets/:ns/:name/compact). Chạy đồng
/// bộ (thường nhanh — gộp file nhỏ). 404 nếu dataset không tồn tại.
pub(crate) fn logic_dataset_compact(db: &Db, ns: &str, name: &str) -> Result<Value, ApiError> {
    let ds = db
        .dataset_get(ns, name)?
        .ok_or_else(|| ApiError::not_found(format!("không có dataset {ns}.{name}")))?;
    let rep = lake::compact(db, ds.id).map_err(ApiError::from)?;
    Ok(json!({
        "ok": true,
        "dataset": format!("{ns}.{name}"),
        "compacted": rep.compacted,
        "partitions_compacted": rep.partitions_compacted,
        "files_before": rep.files_before,
        "files_after": rep.files_after,
        "rows": rep.rows,
        "run_id": rep.run_id,
        "next": if rep.compacted {
            "Đã gộp; file cũ tombstone, GC dọn sau grace. Query không đổi kết quả."
        } else {
            "Dataset đã gọn (mỗi partition ≤ 1 file) — không cần gộp."
        }
    }))
}

pub(crate) fn logic_settings_get(db: &Db) -> Result<Value, ApiError> {
    let all = db.all_settings()?;
    let map: serde_json::Map<String, Value> = all
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect();
    Ok(Value::Object(map))
}

/// Ghi settings — chỉ key trong allowlist, validate kiểu/biên (db::validate_setting).
pub(crate) fn logic_settings_put(db: &Db, body: &Value) -> Result<Value, ApiError> {
    let obj = body
        .as_object()
        .ok_or_else(|| ApiError::bad("body phải là object {key: value}"))?;
    for (k, v) in obj {
        let sval = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        crate::db::validate_setting(k, &sval).map_err(|e| ApiError::bad(e.to_string()))?;
        db.set_setting(k, &sval)?;
    }
    logic_settings_get(db)
}

// ---------------------------------------------------------------------------
// connections (§8/§9) — DSN LUÔN redact khi ra client
// ---------------------------------------------------------------------------

/// Kind kết nối hợp lệ (khớp connectors::connector_for). Redact/enum dùng chung.
const CONNECTION_KINDS: &[&str] =
    &["postgres", "postgresql", "mysql", "mariadb", "sqlite", "clickhouse"];

/// View an toàn của một connection — DSN đã redact (§11). KHÔNG serialize thẳng
/// ConnectionInfo (chứa DSN nguyên văn).
fn connection_view(c: &ConnectionInfo) -> Value {
    json!({
        "id": c.id,
        "kind": c.kind,
        "dsn": redact_dsn(&c.dsn),
        "created_at": c.created_at,
        "last_ok_at": c.last_ok_at,
    })
}

pub(crate) fn logic_connection_list(db: &Db) -> Result<Value, ApiError> {
    let rows = db.connection_list()?;
    let views: Vec<Value> = rows.iter().map(connection_view).collect();
    Ok(json!({
        "total": views.len(),
        "connections": views,
        "next": "Introspect bằng lake_db_introspect, dựng flow bằng lake_flow_create."
    }))
}

/// Thêm/ghi đè một connection. TEST trước khi lưu (§8) — nguồn chết thì không lưu.
pub(crate) async fn logic_connection_add(
    db: &Db,
    id: Option<&str>,
    kind: &str,
    dsn: &str,
) -> Result<Value, ApiError> {
    let kind = kind.trim().to_lowercase();
    if !CONNECTION_KINDS.contains(&kind.as_str()) {
        return Err(ApiError::bad(format!(
            "kind '{kind}' không hợp lệ; hợp lệ: {}",
            CONNECTION_KINDS.join(", ")
        )));
    }
    if dsn.trim().is_empty() {
        return Err(ApiError::bad("dsn rỗng"));
    }
    // id mặc định = kind (một nguồn mỗi kind) khi client không đặt.
    let id = sanitize_ident(id.filter(|s| !s.trim().is_empty()).unwrap_or(&kind));

    // Test bằng ConnectionInfo tạm (chưa ghi DB) — nguồn chết → 400, không lưu.
    let probe = ConnectionInfo {
        id: id.clone(),
        kind: kind.clone(),
        dsn: dsn.to_string(),
        created_at: String::new(),
        last_ok_at: None,
    };
    let connector = connectors::connector_for(probe).map_err(|e| ApiError::bad(e.to_string()))?;
    connector
        .test()
        .await
        .map_err(|e| ApiError::bad(format!("kết nối thất bại, không lưu: {e}")))?;

    db.connection_add(&id, &kind, dsn)?;
    db.connection_mark_ok(&id)?;
    let saved = db
        .connection_get(&id)?
        .ok_or_else(|| ApiError::from(anyhow::anyhow!("connection biến mất sau khi lưu")))?;
    Ok(json!({
        "ok": true,
        "connection": connection_view(&saved),
        "next": "Introspect bằng lake_db_introspect để xem bảng/cột."
    }))
}

/// Test một connection đã lưu; cập nhật last_ok_at khi thành công.
pub(crate) async fn logic_connection_test(db: &Db, id: &str) -> Result<Value, ApiError> {
    let conn = db
        .connection_get(id)?
        .ok_or_else(|| ApiError::not_found(format!("không có connection '{id}'")))?;
    let connector = connectors::connector_for(conn).map_err(|e| ApiError::bad(e.to_string()))?;
    match connector.test().await {
        Ok(()) => {
            db.connection_mark_ok(id)?;
            Ok(json!({ "ok": true, "connection_id": id, "next": "Nguồn sống — dựng flow bằng lake_flow_create." }))
        }
        Err(e) => Err(ApiError::bad(format!("kết nối '{id}' thất bại: {e}"))),
    }
}

/// Introspect schema/table/column của một connection (§8). `schema` lọc theo schema nguồn.
pub(crate) async fn logic_connection_introspect(
    db: &Db,
    id: &str,
    schema: Option<&str>,
) -> Result<Value, ApiError> {
    let conn = db
        .connection_get(id)?
        .ok_or_else(|| ApiError::not_found(format!("không có connection '{id}'")))?;
    let connector = connectors::connector_for(conn).map_err(|e| ApiError::bad(e.to_string()))?;
    let mut tables = connector
        .introspect()
        .await
        .map_err(|e| ApiError::bad(format!("introspect '{id}' thất bại: {e}")))?;
    if let Some(sc) = schema.filter(|s| !s.trim().is_empty()) {
        tables.retain(|t| t.schema.as_deref() == Some(sc));
    }
    Ok(json!({
        "connection_id": id,
        "total": tables.len(),
        "tables": tables,
        "next": "Dùng tên bảng/cột để viết source step trong lake_flow_create."
    }))
}

/// Xóa connection — 409 nếu còn flow tham chiếu (source.connection / export.connection).
pub(crate) fn logic_connection_delete(db: &Db, id: &str) -> Result<Value, ApiError> {
    if db.connection_get(id)?.is_none() {
        return Err(ApiError::not_found(format!("không có connection '{id}'")));
    }
    let refs = flows_referencing_connection(db, id)?;
    if !refs.is_empty() {
        return Err(ApiError::conflict(format!(
            "connection '{id}' còn flow tham chiếu: {} — sửa/xóa flow trước",
            refs.join(", ")
        )));
    }
    db.connection_delete(id)?;
    Ok(json!({
        "ok": true,
        "deleted": id,
        "next": "Dùng lake_connection_list để xác nhận."
    }))
}

/// Danh sách flow id có source/export trỏ tới `conn_id` (đọc def, parse best-effort).
fn flows_referencing_connection(db: &Db, conn_id: &str) -> Result<Vec<String>, ApiError> {
    let mut out = Vec::new();
    for f in db.flow_list()? {
        if let Ok(def) = flow::parse(&f.def) {
            let in_src = def.sources.iter().any(|s| s.connection == conn_id);
            let in_exp = def
                .exports
                .iter()
                .any(|e| e.connection.as_deref() == Some(conn_id));
            if in_src || in_exp {
                out.push(f.id.clone());
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// flows (§8/§9)
// ---------------------------------------------------------------------------

/// Chuẩn hóa `def` (object JSON hoặc chuỗi) về chuỗi cho flow::parse.
fn def_to_string(def: &Value) -> Result<String, ApiError> {
    match def {
        Value::String(s) => Ok(s.clone()),
        Value::Object(_) | Value::Array(_) => {
            serde_json::to_string(def).map_err(|e| ApiError::bad(e.to_string()))
        }
        Value::Null => Err(ApiError::bad("thiếu 'def'")),
        other => Ok(other.to_string()),
    }
}

/// Parse + validate `def`; lỗi validate → 400 kèm danh sách FieldError trong `details`.
fn parse_validate(def: &Value) -> Result<flow::FlowDef, ApiError> {
    let s = def_to_string(def)?;
    let parsed = flow::parse(&s).map_err(|e| ApiError::bad(e.to_string()))?;
    if let Err(errs) = flow::validate(&parsed) {
        let details = serde_json::to_value(&errs).unwrap_or(Value::Null);
        return Err(ApiError::bad("flow def không hợp lệ").with_details(details));
    }
    Ok(parsed)
}

fn flow_view(f: &crate::db::FlowRow) -> Value {
    // DAG suy được (best-effort) để agent/UI thấy thứ tự step.
    let dag = flow::parse(&f.def)
        .ok()
        .and_then(|d| flow::derive_dag(&d).ok());
    json!({
        "id": f.id,
        "name": f.name,
        "def": serde_json::from_str::<Value>(&f.def).unwrap_or(Value::String(f.def.clone())),
        "def_version": f.def_version,
        "enabled": f.enabled,
        "schedule": f.schedule,
        "last_scheduled_at": f.last_scheduled_at,
        "created_at": f.created_at,
        "updated_at": f.updated_at,
        "dag": dag,
    })
}

pub(crate) fn logic_flow_list(db: &Db) -> Result<Value, ApiError> {
    let rows = db.flow_list()?;
    let flows: Vec<Value> = rows.iter().map(|f| flow_view(f)).collect();
    Ok(json!({
        "total": flows.len(),
        "flows": flows,
        "next": "Chạy một flow bằng lake_flow_run, xem chi tiết bằng lake_flow_get."
    }))
}

pub(crate) fn logic_flow_get(db: &Db, id: &str) -> Result<Value, ApiError> {
    let f = db
        .flow_get(id)?
        .ok_or_else(|| ApiError::not_found(format!("không có flow '{id}'")))?;
    Ok(json!({
        "flow": flow_view(&f),
        "next": "Chạy bằng lake_flow_run, hoặc sửa bằng lake_flow_update."
    }))
}

/// Tạo flow mới (§8). Parse+validate → lưu → set owner dataset target cho mỗi source.
/// `enable=false` mặc định (flow AI-gen không auto chạy, §6.6).
pub(crate) fn logic_flow_create(db: &Db, def: &Value, enable: bool) -> Result<Value, ApiError> {
    let parsed = parse_validate(def)?;
    let canon = flow::to_canonical_json(&parsed).map_err(ApiError::from)?;

    // Không ghi đè flow đã tồn tại qua create (dùng update).
    if db.flow_get(&parsed.flow)?.is_some() {
        return Err(ApiError::conflict(format!(
            "flow '{}' đã tồn tại — dùng lake_flow_update để sửa",
            parsed.flow
        )));
    }
    // Chiếm owner dataset target TRƯỚC khi lưu — nguồn xung đột thì báo 409, không lưu.
    claim_source_targets(db, &parsed)?;
    db.flow_upsert(&parsed.flow, None, &canon, enable, schedule_json(&parsed).as_deref())?;

    let f = db
        .flow_get(&parsed.flow)?
        .ok_or_else(|| ApiError::from(anyhow::anyhow!("flow biến mất sau khi lưu")))?;
    Ok(json!({
        "ok": true,
        "flow": flow_view(&f),
        "dag": flow::derive_dag(&parsed).ok(),
        "next": if enable {
            "Flow đã bật — chạy bằng lake_flow_run hoặc chờ lịch."
        } else {
            "Flow tạo ở trạng thái tắt — bật bằng lake_flow_enable rồi lake_flow_run."
        }
    }))
}

/// Đăng ký owner dataset cho mỗi source target (một dataset chỉ một flow ghi, §6.1).
/// Dataset đã thuộc flow khác → 409.
fn claim_source_targets(db: &Db, def: &flow::FlowDef) -> Result<(), ApiError> {
    for s in &def.sources {
        let (ns, name) = flow::source_target(s);
        let ds_id = db.dataset_upsert(&ns, &name, None, None, None)?;
        if !db.dataset_set_owner(ds_id, Some(&def.flow))? {
            return Err(ApiError::conflict(format!(
                "dataset {ns}.{name} đã thuộc flow khác — đổi target hoặc gỡ flow cũ"
            )));
        }
    }
    Ok(())
}

/// Tính impact (§6.3) giữa def cũ và mới: step nào reset state, step nào giữ, dataset mồ
/// côi. Logic ở flow::diff_impact; ở đây chỉ serialize sang JSON cho REST/MCP.
fn compute_impact(old: &flow::FlowDef, new: &flow::FlowDef) -> Value {
    let imp = flow::diff_impact(old, new);
    serde_json::to_value(&imp).unwrap_or_else(|_| json!({
        "steps_reset": imp.steps_reset,
        "steps_kept": imp.steps_kept,
        "datasets_orphaned": imp.datasets_orphaned,
    }))
}

/// Sửa flow (§6.3). Thay đổi state-resetting cần `confirm_reset`; thiếu → 409 kèm impact.
pub(crate) fn logic_flow_update(
    db: &Db,
    id: &str,
    def: &Value,
    confirm_reset: bool,
) -> Result<Value, ApiError> {
    let existing = db
        .flow_get(id)?
        .ok_or_else(|| ApiError::not_found(format!("không có flow '{id}'")))?;
    let old_def = flow::parse(&existing.def).map_err(|e| ApiError::from(anyhow::anyhow!(e)))?;
    let new_def = parse_validate(def)?;
    if new_def.flow != id {
        return Err(ApiError::bad(format!(
            "def.flow '{}' khác id đường dẫn '{id}' — không đổi id qua update",
            new_def.flow
        )));
    }

    let impact = compute_impact(&old_def, &new_def);
    let reset_steps: Vec<String> = impact["steps_reset"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    if !reset_steps.is_empty() && !confirm_reset {
        return Err(ApiError::conflict(
            "thay đổi state-resetting cần confirm_reset=true",
        )
        .with_details(impact));
    }

    let canon = flow::to_canonical_json(&new_def).map_err(ApiError::from)?;
    // Lịch lấy từ def mới (cho phép đổi schedule qua update; None = gỡ lịch).
    db.flow_upsert(id, existing.name.as_deref(), &canon, existing.enabled, schedule_json(&new_def).as_deref())?;
    claim_source_targets(db, &new_def)?;

    if !reset_steps.is_empty() {
        // def_version tăng (skip-lookup cũ vô hiệu) + xóa watermark/interval step reset.
        db.flow_bump_def_version(id)?;
        for step in &reset_steps {
            db.stream_state_delete(id, step)?;
            db.step_interval_delete(id, step)?;
        }
    }

    let f = db
        .flow_get(id)?
        .ok_or_else(|| ApiError::from(anyhow::anyhow!("flow biến mất sau khi lưu")))?;
    Ok(json!({
        "ok": true,
        "flow": flow_view(&f),
        "impact": impact,
        "next": "Chạy lại bằng lake_flow_run."
    }))
}

/// Backfill một flow (§6.2). Per-step: `incremental_by_time` chạy lại range `[start,end)`;
/// transform full + source SKIP mặc định; `rebuild` chạy lại (merge/SCD2 cần confirm).
pub(crate) async fn logic_flow_backfill(
    db: &Db,
    id: &str,
    start: &str,
    end: &str,
    steps: Option<Vec<String>>,
    rebuild: Vec<String>,
    confirm: bool,
) -> Result<Value, ApiError> {
    if db.flow_get(id)?.is_none() {
        return Err(ApiError::not_found(format!("không có flow '{id}'")));
    }
    if start.trim().is_empty() || end.trim().is_empty() {
        return Err(ApiError::bad("backfill cần start và end (mốc thời gian)"));
    }
    let outcome = crate::runner::backfill_run(
        &crate::config::lake_dir(),
        db,
        id,
        start,
        end,
        steps.as_deref(),
        &rebuild,
        confirm,
    )
    .await
    .map_err(|e| ApiError::bad(e.to_string()))?;
    Ok(json!({
        "ok": true,
        "flow": id,
        "range": { "start": start, "end": end },
        "steps_run": outcome.steps_run,
        "steps_skipped": outcome.steps_skipped,
        "intervals_run": outcome.intervals_run,
        "rows_written": outcome.rows_written,
        "next": "Xem lại dữ liệu bằng lake_query; step SKIP là merge/SCD2 (dùng rebuild có confirm nếu cần)."
    }))
}

/// Serialize `schedule` của một def sang JSON string cho cột `flow.schedule` (None = gỡ).
fn schedule_json(def: &flow::FlowDef) -> Option<String> {
    def.schedule.as_ref().and_then(|s| serde_json::to_string(s).ok())
}

/// Sinh **draft** flow từ mô tả tự nhiên (§9). Introspect connection (nếu có) → build
/// prompt (schema + DSL spec) → bridge `llm.request` → parse + validate draft. KHÔNG
/// auto-enable, KHÔNG lưu — trả draft để agent kiểm rồi gọi lake_flow_create.
pub(crate) async fn logic_flow_generate(
    db: &Db,
    description: &str,
    connection_id: Option<&str>,
) -> Result<Value, ApiError> {
    if description.trim().is_empty() {
        return Err(ApiError::bad("cần 'description' mô tả pipeline cần sinh"));
    }
    // Introspect nguồn (best-effort): lỗi introspect KHÔNG chặn generate — chỉ bớt ngữ cảnh.
    let introspection = match connection_id.filter(|s| !s.trim().is_empty()) {
        Some(cid) => logic_connection_introspect(db, cid, None).await.ok(),
        None => None,
    };
    let system = crate::generate::system_prompt();
    let prompt = crate::generate::build_prompt(description, introspection.as_ref());

    let max_tokens = db.setting_i64("generate_max_tokens", 8000).clamp(256, 32000) as u32;
    let reply = crate::transport::llm_request(&system, &prompt, max_tokens)
        .await
        .map_err(|e| ApiError::bad(format!("sinh flow qua bridge thất bại: {e}")))?;

    let draft = crate::generate::parse_draft(&reply.text)
        .map_err(|e| ApiError::bad(e.to_string()))?;
    let canon: Value = serde_json::from_str(
        &flow::to_canonical_json(&draft).map_err(ApiError::from)?,
    )
    .unwrap_or(Value::Null);
    let dag = flow::derive_dag(&draft).ok();
    Ok(json!({
        "ok": true,
        "draft": canon,
        "dag": dag,
        "model": reply.model,
        "next": "Draft CHƯA được lưu. Kiểm rồi tạo bằng lake_flow_create{def: draft}."
    }))
}

/// Lineage up/downstream của một dataset (§4). Cạnh dataset suy từ mọi flow def
/// (`flow::dataset_edges`) — cấu trúc, không phụ thuộc run đã chạy. BFS theo `depth`.
pub(crate) fn logic_lineage(
    db: &Db,
    ns: &str,
    name: &str,
    depth: i64,
) -> Result<Value, ApiError> {
    if db.dataset_get(ns, name)?.is_none() {
        return Err(ApiError::not_found(format!("không có dataset {ns}.{name}")));
    }
    let depth = depth.clamp(1, 10) as usize;

    // Gom cạnh (parent → child) toàn cục từ mọi flow.
    let mut edges: Vec<((String, String), (String, String))> = Vec::new();
    for f in db.flow_list()? {
        if let Ok(def) = flow::parse(&f.def) {
            edges.extend(flow::dataset_edges(&def));
        }
    }
    let root = (ns.to_string(), name.to_string());
    let upstream = bfs_lineage(&edges, &root, depth, true);
    let downstream = bfs_lineage(&edges, &root, depth, false);
    Ok(json!({
        "dataset": format!("{ns}.{name}"),
        "namespace": ns,
        "name": name,
        "depth": depth,
        "upstream": upstream,
        "downstream": downstream,
        "next": "upstream = nguồn nuôi dataset này; downstream = dataset phái sinh."
    }))
}

/// BFS lineage một chiều. `upward=true` đi ngược cạnh (child→parent, tìm tổ tiên);
/// `false` đi xuôi (parent→child, tìm hậu duệ). Mỗi node kèm `depth` (khoảng cách 1..N).
fn bfs_lineage(
    edges: &[((String, String), (String, String))],
    root: &(String, String),
    max_depth: usize,
    upward: bool,
) -> Vec<Value> {
    use std::collections::HashSet;
    let mut seen: HashSet<(String, String)> = HashSet::new();
    seen.insert(root.clone());
    let mut frontier = vec![root.clone()];
    let mut out = Vec::new();
    for d in 1..=max_depth {
        let mut next = Vec::new();
        for node in &frontier {
            for (parent, child) in edges {
                // upward: cạnh có child == node → parent là tổ tiên.
                // downward: cạnh có parent == node → child là hậu duệ.
                let (anchor, other) = if upward { (child, parent) } else { (parent, child) };
                if anchor == node && !seen.contains(other) {
                    seen.insert(other.clone());
                    out.push(json!({
                        "namespace": other.0,
                        "name": other.1,
                        "dataset": format!("{}.{}", other.0, other.1),
                        "depth": d,
                    }));
                    next.push(other.clone());
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    out
}

/// Xóa flow (§8). 409 nếu còn run active; thả owner dataset (dataset giữ dữ liệu).
pub(crate) fn logic_flow_delete(db: &Db, id: &str) -> Result<Value, ApiError> {
    let f = db
        .flow_get(id)?
        .ok_or_else(|| ApiError::not_found(format!("không có flow '{id}'")))?;
    let active = db.run_list(Some(id), None, 50, 0)?;
    if active.iter().any(|r| run_status::is_active(&r.status)) {
        return Err(ApiError::conflict(format!(
            "flow '{id}' đang có run chạy — hủy run trước khi xóa"
        )));
    }
    // Thả owner mọi dataset thuộc flow (dataset + dữ liệu giữ nguyên, §6.3).
    if let Ok(def) = flow::parse(&f.def) {
        for s in &def.sources {
            let (ns, name) = flow::source_target(s);
            if let Some(ds) = db.dataset_get(&ns, &name)? {
                if ds.owner_flow_id.as_deref() == Some(id) {
                    db.dataset_set_owner(ds.id, None)?;
                }
            }
        }
    }
    db.flow_delete(id)?;
    Ok(json!({
        "ok": true,
        "deleted": id,
        "next": "Dataset vẫn còn dữ liệu; dùng lake_dataset_list để xác nhận."
    }))
}

/// Bật/tắt flow (§8).
pub(crate) fn logic_flow_enable(db: &Db, id: &str, enabled: bool) -> Result<Value, ApiError> {
    let f = db
        .flow_get(id)?
        .ok_or_else(|| ApiError::not_found(format!("không có flow '{id}'")))?;
    db.flow_upsert(id, f.name.as_deref(), &f.def, enabled, f.schedule.as_deref())?;
    Ok(json!({
        "ok": true,
        "flow_id": id,
        "enabled": enabled,
        "next": if enabled { "Chạy bằng lake_flow_run." } else { "Flow đã tắt." }
    }))
}

/// Kích hoạt một run manual (§8). 409 nếu flow đang chạy; 429 nếu queue đầy.
/// `hub` (nếu có) nhận run:status=queued để UI cập nhật ngay.
pub(crate) fn logic_flow_run(
    db: &Db,
    hub: Option<&crate::dashws::DashHub>,
    id: &str,
) -> Result<Value, ApiError> {
    if db.flow_get(id)?.is_none() {
        return Err(ApiError::not_found(format!("không có flow '{id}'")));
    }
    match runner::enqueue(db, id, crate::db::trigger::MANUAL)? {
        EnqueueOutcome::Created(run_id) => {
            if let Some(h) = hub {
                h.emit_run_status(&run_id, id, run_status::QUEUED);
            }
            Ok(json!({
                "ok": true,
                "run_id": run_id,
                "next": "Chạy async — poll bằng lake_run_status, ĐỪNG chờ đồng bộ."
            }))
        }
        EnqueueOutcome::FlowBusy => Err(ApiError::conflict(format!(
            "flow '{id}' đang có run active — chờ xong hoặc hủy trước"
        ))),
        EnqueueOutcome::Backpressure => Err(ApiError::too_many(
            "hàng đợi run đầy — thử lại sau",
        )),
    }
}

// ---------------------------------------------------------------------------
// runs (§8/§9)
// ---------------------------------------------------------------------------

pub(crate) fn logic_run_list(
    db: &Db,
    flow_id: Option<&str>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Value, ApiError> {
    let rows = db.run_list(flow_id, status, limit, offset)?;
    Ok(json!({
        "total": rows.len(),
        "runs": rows,
        "next": "Chi tiết + step bằng lake_run_status, log bằng lake_run_logs."
    }))
}

pub(crate) fn logic_run_get(db: &Db, id: &str) -> Result<Value, ApiError> {
    let run = db
        .run_get(id)?
        .ok_or_else(|| ApiError::not_found(format!("không có run '{id}'")))?;
    let steps = db.step_runs_for(id)?;
    Ok(json!({
        "run": run,
        "steps": steps,
        "next": "Xem log bằng lake_run_logs; hủy bằng lake_run_cancel nếu còn chạy."
    }))
}

/// Hủy một run (§8). Set cancel token (nếu đang chạy) + guarded flip queued→cancelled.
pub(crate) fn logic_run_cancel(
    db: &Db,
    cancels: &runner::CancelRegistry,
    id: &str,
) -> Result<Value, ApiError> {
    let run = db
        .run_get(id)?
        .ok_or_else(|| ApiError::not_found(format!("không có run '{id}'")))?;
    if run_status::is_terminal(&run.status) {
        return Err(ApiError::conflict(format!(
            "run '{id}' đã kết thúc ({}) — không thể hủy",
            run.status
        )));
    }
    // Báo worker đang chạy dừng giữa batch (nếu có), và ép queued → cancelled ngay.
    let signalled = runner::request_cancel(cancels, id);
    db.run_update_status_guarded(id, run_status::CANCELLED, Some("hủy theo yêu cầu"))
        .ok();
    Ok(json!({
        "ok": true,
        "run_id": id,
        "signalled": signalled,
        "next": "Poll lake_run_status tới khi status = cancelled."
    }))
}

pub(crate) fn logic_run_logs(db: &Db, id: &str, tail: i64) -> Result<Value, ApiError> {
    if db.run_get(id)?.is_none() {
        return Err(ApiError::not_found(format!("không có run '{id}'")));
    }
    let lines = db.run_log_tail(id, tail)?;
    Ok(json!({
        "run_id": id,
        "returned": lines.len(),
        "logs": lines,
        "next": "Tăng tail (tối đa 500) nếu cần thêm dòng."
    }))
}

// ---------------------------------------------------------------------------
// REST handlers — mỏng, chỉ gọi logic_*
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ListQuery {
    namespace: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
struct PreviewQuery {
    limit: Option<i64>,
}

async fn status(State(st): State<AppState>) -> Response {
    ok_or(logic_stats(&st.db))
}

async fn list_datasets(State(st): State<AppState>, Query(q): Query<ListQuery>) -> Response {
    ok_or(logic_dataset_list(
        &st.db,
        q.namespace.as_deref(),
        q.limit.unwrap_or(100),
        q.offset.unwrap_or(0),
    ))
}

async fn get_dataset(State(st): State<AppState>, Path((ns, name)): Path<(String, String)>) -> Response {
    ok_or(logic_dataset_get(&st.db, &ns, &name))
}

async fn preview_dataset(
    State(st): State<AppState>,
    Path((ns, name)): Path<(String, String)>,
    Query(q): Query<PreviewQuery>,
) -> Response {
    ok_or(logic_dataset_preview(&st.db, &ns, &name, q.limit.unwrap_or(50)).await)
}

async fn delete_dataset(
    State(st): State<AppState>,
    Path((ns, name)): Path<(String, String)>,
) -> Response {
    ok_or(logic_dataset_delete(&st.db, &ns, &name))
}

#[derive(Deserialize)]
struct LineageQuery {
    depth: Option<i64>,
}

async fn dataset_lineage(
    State(st): State<AppState>,
    Path((ns, name)): Path<(String, String)>,
    Query(q): Query<LineageQuery>,
) -> Response {
    ok_or(logic_lineage(&st.db, &ns, &name, q.depth.unwrap_or(2)))
}

#[derive(Deserialize)]
struct ImportBody {
    filename: String,
    #[serde(alias = "contentBase64")]
    content_base64: String,
    namespace: Option<String>,
    dataset: Option<String>,
}

async fn import(State(st): State<AppState>, body: MaybeJson) -> Response {
    let b: ImportBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    ok_or(logic_import_b64(
        &st.db,
        &b.filename,
        &b.content_base64,
        b.namespace.as_deref(),
        b.dataset.as_deref(),
    ))
}

#[derive(Deserialize)]
struct QueryBody {
    sql: String,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn query(State(st): State<AppState>, body: MaybeJson) -> Response {
    let b: QueryBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    ok_or(logic_query(&st.db, &b.sql, b.limit, b.offset).await)
}

#[derive(Deserialize)]
struct ExplainBody {
    sql: String,
}

async fn query_explain(State(st): State<AppState>, body: MaybeJson) -> Response {
    let b: ExplainBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    ok_or(logic_explain(&st.db, &b.sql).await)
}

#[derive(Deserialize)]
struct ExportBody {
    sql: String,
    format: String,
}

async fn query_export(State(st): State<AppState>, body: MaybeJson) -> Response {
    let b: ExportBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    ok_or(logic_query_export(&st.db, &b.sql, &b.format).await)
}

async fn compact_dataset(
    State(st): State<AppState>,
    Path((ns, name)): Path<(String, String)>,
) -> Response {
    ok_or(logic_dataset_compact(&st.db, &ns, &name))
}

/// GET /exports/:file — tải file export đầy đủ. Chặn path traversal trong export::read_export_file.
async fn download_export(Path(file): Path<String>) -> Response {
    match crate::export::read_export_file(&file) {
        Ok(bytes) => {
            let ctype = crate::export::content_type_for(&file);
            (
                StatusCode::OK,
                [
                    (axum::http::header::CONTENT_TYPE, ctype.to_string()),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{file}\""),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        // Tên không hợp lệ / ngoài thư mục → 400; không tìm thấy → 404 (message phân biệt).
        Err(e) => {
            let msg = e.to_string();
            let code = if msg.contains("không có file") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            err(code, msg)
        }
    }
}

async fn get_settings(State(st): State<AppState>) -> Response {
    ok_or(logic_settings_get(&st.db))
}

async fn put_settings(State(st): State<AppState>, body: MaybeJson) -> Response {
    let v: Value = match parse_body(body) {
        Ok(v) => v,
        Err(r) => return r,
    };
    ok_or(logic_settings_put(&st.db, &v))
}

// ---- connections ----

async fn list_connections(State(st): State<AppState>) -> Response {
    ok_or(logic_connection_list(&st.db))
}

#[derive(Deserialize)]
struct ConnectionBody {
    id: Option<String>,
    kind: String,
    dsn: String,
}

async fn add_connection(State(st): State<AppState>, body: MaybeJson) -> Response {
    let b: ConnectionBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    ok_or(logic_connection_add(&st.db, b.id.as_deref(), &b.kind, &b.dsn).await)
}

async fn test_connection(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    ok_or(logic_connection_test(&st.db, &id).await)
}

#[derive(Deserialize)]
struct IntrospectQuery {
    schema: Option<String>,
}

async fn introspect_connection(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<IntrospectQuery>,
) -> Response {
    ok_or(logic_connection_introspect(&st.db, &id, q.schema.as_deref()).await)
}

async fn delete_connection(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    ok_or(logic_connection_delete(&st.db, &id))
}

// ---- flows ----

async fn list_flows(State(st): State<AppState>) -> Response {
    ok_or(logic_flow_list(&st.db))
}

#[derive(Deserialize)]
struct FlowCreateBody {
    def: Value,
    #[serde(default)]
    enable: bool,
}

async fn create_flow(State(st): State<AppState>, body: MaybeJson) -> Response {
    let b: FlowCreateBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    ok_or(logic_flow_create(&st.db, &b.def, b.enable))
}

async fn get_flow(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    ok_or(logic_flow_get(&st.db, &id))
}

#[derive(Deserialize)]
struct GenerateBody {
    description: String,
    #[serde(default)]
    connection_id: Option<String>,
}

async fn generate_flow(State(st): State<AppState>, body: MaybeJson) -> Response {
    let b: GenerateBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    ok_or(logic_flow_generate(&st.db, &b.description, b.connection_id.as_deref()).await)
}

#[derive(Deserialize)]
struct FlowUpdateBody {
    def: Value,
    #[serde(default)]
    confirm_reset: bool,
}

async fn update_flow(
    State(st): State<AppState>,
    Path(id): Path<String>,
    body: MaybeJson,
) -> Response {
    let b: FlowUpdateBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    ok_or(logic_flow_update(&st.db, &id, &b.def, b.confirm_reset))
}

async fn delete_flow(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    ok_or(logic_flow_delete(&st.db, &id))
}

async fn run_flow(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    ok_or(logic_flow_run(&st.db, Some(&st.hub), &id))
}

#[derive(Deserialize)]
struct BackfillBody {
    start: String,
    end: String,
    #[serde(default)]
    steps: Option<Vec<String>>,
    #[serde(default)]
    rebuild: Vec<String>,
    #[serde(default)]
    confirm: bool,
}

async fn backfill_flow(
    State(st): State<AppState>,
    Path(id): Path<String>,
    body: MaybeJson,
) -> Response {
    let b: BackfillBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    ok_or(logic_flow_backfill(&st.db, &id, &b.start, &b.end, b.steps, b.rebuild, b.confirm).await)
}

#[derive(Deserialize)]
struct EnableBody {
    enabled: bool,
}

async fn enable_flow(
    State(st): State<AppState>,
    Path(id): Path<String>,
    body: MaybeJson,
) -> Response {
    let b: EnableBody = match parse_body(body) {
        Ok(b) => b,
        Err(r) => return r,
    };
    ok_or(logic_flow_enable(&st.db, &id, b.enabled))
}

// ---- runs ----

#[derive(Deserialize)]
struct RunListQuery {
    flow_id: Option<String>,
    status: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_runs(State(st): State<AppState>, Query(q): Query<RunListQuery>) -> Response {
    ok_or(logic_run_list(
        &st.db,
        q.flow_id.as_deref(),
        q.status.as_deref(),
        q.limit.unwrap_or(100),
        q.offset.unwrap_or(0),
    ))
}

async fn get_run(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    ok_or(logic_run_get(&st.db, &id))
}

async fn cancel_run(State(st): State<AppState>, Path(id): Path<String>) -> Response {
    ok_or(logic_run_cancel(&st.db, &st.cancels, &id))
}

#[derive(Deserialize)]
struct LogsQuery {
    tail: Option<i64>,
}

async fn get_run_logs(
    State(st): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<LogsQuery>,
) -> Response {
    ok_or(logic_run_logs(&st.db, &id, q.tail.unwrap_or(100)))
}

// ---------------------------------------------------------------------------
// base64 helper (dùng chung REST + MCP)
// ---------------------------------------------------------------------------

/// Giải mã base64, chấp nhận cả prefix `data:...;base64,` (mô hình ontology api.rs).
pub(crate) fn decode_base64_maybe_data_url(s: &str) -> Result<Vec<u8>, String> {
    use base64::Engine;
    let payload = match s.find("base64,") {
        Some(i) => &s[i + "base64,".len()..],
        None => s,
    };
    let cleaned: String = payload.split_whitespace().collect();
    base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|e| format!("base64 không hợp lệ: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> AppState {
        super::test_state()
    }

    #[test]
    fn sanitize_ident_strips_and_lowercases() {
        assert_eq!(sanitize_ident("Orders 2024.csv"), "orders_2024_csv");
        assert_eq!(sanitize_ident("  "), "table");
        assert_eq!(sanitize_ident("raw"), "raw");
    }

    #[test]
    fn base64_accepts_data_url_prefix() {
        let raw = decode_base64_maybe_data_url("data:text/csv;base64,YQ==").unwrap();
        assert_eq!(raw, b"a");
        let raw2 = decode_base64_maybe_data_url("YQ==").unwrap();
        assert_eq!(raw2, b"a");
    }

    #[test]
    fn lineage_up_and_downstream() {
        let st = state();
        let db = &st.db;
        // Flow: source raw.events → transform marts.daily (FROM events) → transform
        // marts.weekly (FROM daily). Chuỗi raw.events → marts.daily → marts.weekly.
        let def = json!({
            "flow": "chain",
            "sources": [{"id": "events", "connection": "c", "table": "t", "mode": "full_refresh",
                         "target": {"namespace": "raw", "dataset": "events"}}],
            "transforms": [
                {"id": "daily", "kind": "full", "sql": "SELECT * FROM events",
                 "target": {"namespace": "marts", "dataset": "daily"}},
                {"id": "weekly", "kind": "full", "sql": "SELECT * FROM daily",
                 "target": {"namespace": "marts", "dataset": "weekly"}}
            ]
        })
        .to_string();
        db.flow_upsert("chain", None, &def, false, None).unwrap();
        // Dataset phải tồn tại để lineage root-check qua (upsert node trần).
        for (ns, name) in [("raw", "events"), ("marts", "daily"), ("marts", "weekly")] {
            db.dataset_upsert(ns, name, None, None, None).unwrap();
        }

        // Từ marts.daily: upstream = raw.events (depth 1); downstream = marts.weekly (depth 1).
        let lin = logic_lineage(db, "marts", "daily", 2).unwrap();
        let up = lin["upstream"].as_array().unwrap();
        let down = lin["downstream"].as_array().unwrap();
        assert_eq!(up.len(), 1);
        assert_eq!(up[0]["dataset"], json!("raw.events"));
        assert_eq!(down.len(), 1);
        assert_eq!(down[0]["dataset"], json!("marts.weekly"));

        // Từ raw.events: downstream lan tỏa 2 bậc (daily depth 1, weekly depth 2).
        let lin2 = logic_lineage(db, "raw", "events", 2).unwrap();
        let down2 = lin2["downstream"].as_array().unwrap();
        assert_eq!(down2.len(), 2);
        assert!(down2.iter().any(|d| d["dataset"] == json!("marts.daily") && d["depth"] == json!(1)));
        assert!(down2.iter().any(|d| d["dataset"] == json!("marts.weekly") && d["depth"] == json!(2)));
        assert!(lin2["upstream"].as_array().unwrap().is_empty(), "source không có cha");

        // depth=1 chặn lan tỏa bậc 2.
        let lin3 = logic_lineage(db, "raw", "events", 1).unwrap();
        assert_eq!(lin3["downstream"].as_array().unwrap().len(), 1);

        // Dataset lạ → 404.
        assert!(logic_lineage(db, "raw", "nope", 2).is_err());
    }

    #[tokio::test]
    async fn import_then_query_roundtrip() {
        let st = state();
        let csv = "id,name\n1,alice\n2,bob\n";
        let out = logic_import(&st.db, "people.csv", csv.as_bytes(), Some("raw"), None).unwrap();
        assert_eq!(out["ok"], json!(true));
        let ds = out["datasets"][0]["dataset"].as_str().unwrap().to_string();

        let list = logic_dataset_list(&st.db, None, 100, 0).unwrap();
        assert_eq!(list["total"], json!(1));

        let page = logic_query(
            &st.db,
            &format!("SELECT count(*) AS n FROM raw.\"{ds}\""),
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(page["rows"][0][0], json!(2));
    }

    #[test]
    fn delete_missing_dataset_is_404() {
        let st = state();
        let e = logic_dataset_delete(&st.db, "raw", "nope").unwrap_err();
        assert_eq!(e.code, StatusCode::NOT_FOUND);
    }

    #[test]
    fn settings_put_rejects_unknown_key() {
        let st = state();
        let e = logic_settings_put(&st.db, &json!({ "nonsense": 1 })).unwrap_err();
        assert_eq!(e.code, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn rest_import_enforces_base64_cap() {
        use base64::Engine;
        let st = state();
        st.db.set_setting("import_base64_max_mb", "1").unwrap();
        // 2MB > cap 1MB — REST base64 (logic_import_b64) phải từ chối 413, không chỉ MCP.
        let big = vec![b'a'; 2 * 1024 * 1024];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&big);
        let e = logic_import_b64(&st.db, "big.csv", &b64, Some("raw"), None).unwrap_err();
        assert_eq!(e.code, StatusCode::PAYLOAD_TOO_LARGE);

        // Dưới cap thì đi tiếp qua ingest (CSV nhỏ hợp lệ).
        let csv = base64::engine::general_purpose::STANDARD.encode(b"id,x\n1,a\n");
        let ok = logic_import_b64(&st.db, "small.csv", &csv, Some("raw"), None);
        assert!(ok.is_ok(), "payload dưới cap phải qua: {ok:?}");
    }
}
