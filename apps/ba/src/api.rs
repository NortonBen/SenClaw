//! REST API — mọi logic nằm trong các hàm `*_value` (trả `serde_json::Value`)
//! để MCP gọi ĐÚNG các hàm này: agent và người dùng UI thấy hành vi y hệt.
//! Việc gọi LLM lâu (generate / CR / ask) chạy qua job registry, UI poll
//! GET /api/jobs/:id; MCP thì await thẳng engine.

use crate::state::AppState;
use crate::{cr, engine, export, templates, trace};
use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::collections::HashMap;

pub fn make_state() -> AppState {
    let db = crate::db::Db::open_default().expect("mở được SQLite");
    AppState::new(db)
}

#[cfg(test)]
pub fn make_test_state() -> AppState {
    AppState::new(crate::db::Db::open_memory().unwrap())
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(h_status))
        .route("/catalog", get(h_catalog))
        .route("/activity", get(h_activity))
        .route("/projects", get(h_projects_list).post(h_project_create))
        .route("/projects/:id", get(h_project_get).patch(h_project_update))
        .route("/projects/:id/features", get(h_features_list).post(h_feature_add))
        .route("/projects/:id/import-features", post(h_import_features))
        .route("/projects/:id/dashboard", get(h_dashboard))
        .route("/projects/:id/kg", get(h_kg))
        .route("/projects/:id/crs", get(h_crs_list).post(h_cr_create))
        .route("/features/:id", patch_only_update_feature())
        .route("/features/:id/trace", get(h_trace))
        .route("/features/:id/workflow", get(h_workflow_status).post(h_workflow_start))
        .route("/workflows/:id/advance", post(h_workflow_advance))
        .route("/workflow-templates", get(h_workflow_templates))
        .route("/docs", get(h_docs_list).post(h_doc_write))
        .route("/docs/:id", get(h_doc_get).patch(h_doc_update).delete(h_doc_delete))
        .route("/docs/:id/versions", get(h_doc_versions))
        .route("/docs/:id/versions/:ver", get(h_doc_version_content))
        .route("/search", get(h_search))
        .route("/generate", post(h_generate))
        .route("/jobs/:id", get(h_job))
        .route("/crs/:id", get(h_cr_get))
        .route("/crs/:id/apply", post(h_cr_apply))
        .route("/crs/:id/update", post(h_cr_update))
        .route("/ask", post(h_ask))
        .route("/qa", get(h_qa_list))
        .route("/export", get(h_export))
        .route("/export/download", get(h_export_download))
        .route("/preview", get(h_preview))
        // MCP (HTTP + SSE), same shape as the other Space Apps.
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

fn patch_only_update_feature() -> axum::routing::MethodRouter<AppState> {
    axum::routing::patch(h_feature_update).get(h_feature_get)
}

fn err_status(v: &Value) -> StatusCode {
    if v.get("error").is_some() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::OK
    }
}

fn reply(v: Value) -> Response {
    let code = err_status(&v);
    (code, Json(v)).into_response()
}

// ---------- shared *_value helpers (REST + MCP) ----------

pub(crate) fn status_value(s: &AppState) -> Value {
    json!({
        "ok": true,
        "app": "ba",
        "name": "BA Studio",
        "version": env!("CARGO_PKG_VERSION"),
        "counts": s.db.counts(),
    })
}

pub(crate) fn project_create_value(s: &AppState, name: &str, description: &str, context: &str) -> Value {
    if name.trim().is_empty() {
        return json!({ "error": "tên dự án rỗng" });
    }
    match s.db.create_project(name.trim(), description, context) {
        Ok(id) => {
            s.db.log("user", "project_create", name);
            json!({ "ok": true, "project": s.db.get_project(id) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn resolve_project_value(s: &AppState, key: &str) -> Result<i64, Value> {
    if key.trim().is_empty() {
        return Err(json!({ "error": "thiếu 'project' (id hoặc slug)" }));
    }
    s.db
        .resolve_project(key)
        .ok_or_else(|| json!({ "error": format!("dự án '{key}' không tồn tại — ba_project_list để xem danh sách") }))
}

pub(crate) fn resolve_feature_opt(s: &AppState, project_id: i64, key: &str) -> Result<Option<i64>, Value> {
    if key.trim().is_empty() {
        return Ok(None);
    }
    s.db
        .resolve_feature(project_id, key)
        .map(Some)
        .ok_or_else(|| json!({ "error": format!("tính năng '{key}' không có trong dự án — ba_feature_list để xem") }))
}

pub(crate) fn feature_add_value(s: &AppState, project_id: i64, name: &str, description: &str, priority: &str) -> Value {
    if name.trim().is_empty() {
        return json!({ "error": "tên tính năng rỗng" });
    }
    let priority = if priority.is_empty() { "P1" } else { priority };
    if !["P0", "P1", "P2"].contains(&priority) {
        return json!({ "error": "priority phải là P0 | P1 | P2" });
    }
    match s.db.add_feature(project_id, name.trim(), description, priority) {
        Ok(id) => {
            s.db.log("user", "feature_add", name);
            json!({ "ok": true, "feature": s.db.get_feature(id) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn doc_write_value(
    s: &AppState,
    project_id: i64,
    feature_id: Option<i64>,
    doc_type: &str,
    subtype: &str,
    title: &str,
    content: &str,
) -> Value {
    let Some(tpl) = templates::get(doc_type, subtype) else {
        return json!({ "error": format!("loại tài liệu '{doc_type}/{subtype}' không có trong registry — GET /api/catalog") });
    };
    if content.trim().is_empty() {
        return json!({ "error": "content rỗng" });
    }
    let effective_feature = match tpl.scope {
        templates::Scope::Project => None,
        templates::Scope::Feature => match feature_id {
            Some(f) => Some(f),
            None => return json!({ "error": format!("'{}' là tài liệu cấp tính năng — truyền feature", tpl.title) }),
        },
    };
    let display = match effective_feature.and_then(|f| s.db.get_feature(f)) {
        Some(f) => f["name"].as_str().unwrap_or("").to_string(),
        None => s
            .db
            .get_project(project_id)
            .and_then(|p| p["name"].as_str().map(|x| x.to_string()))
            .unwrap_or_default(),
    };
    let title = if title.trim().is_empty() {
        format!("{} — {}", tpl.title, display)
    } else {
        title.trim().to_string()
    };
    match s.db.upsert_document(
        project_id,
        effective_feature,
        tpl.doc_type,
        tpl.subtype,
        &title,
        content,
        tpl.format,
        "user",
        "",
        "ghi trực tiếp",
    ) {
        Ok((id, ver)) => {
            trace::reindex_document(&s.db, id);
            s.db.log("user", "doc_write", &format!("{title} v{ver}"));
            json!({ "ok": true, "document": s.db.get_document(id) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn trace_value(s: &AppState, feature_id: i64) -> Value {
    let Some(f) = s.db.get_feature(feature_id) else {
        return json!({ "error": format!("tính năng #{feature_id} không tồn tại") });
    };
    let project_id = f["project_id"].as_i64().unwrap_or(0);
    json!({
        "feature": f,
        "coverage": trace::coverage(&s.db, project_id, feature_id),
        "pipeline": trace::pipeline(&s.db, project_id, feature_id),
        "staleness": trace::staleness(&s.db, project_id, Some(feature_id)),
    })
}

pub(crate) fn dashboard_value(s: &AppState, project_id: i64) -> Value {
    if s.db.get_project(project_id).is_none() {
        return json!({ "error": format!("dự án #{project_id} không tồn tại") });
    }
    trace::dashboard(&s.db, project_id)
}

pub(crate) fn kg_value(s: &AppState, project_id: i64) -> Value {
    if s.db.get_project(project_id).is_none() {
        return json!({ "error": format!("dự án #{project_id} không tồn tại") });
    }
    trace::knowledge_graph(&s.db, project_id)
}

// ---------- handlers ----------

async fn h_status(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

async fn h_catalog() -> Json<Value> {
    Json(json!({
        "phases": templates::catalog(),
        "statuses": templates::DOC_STATUSES,
        "pipeline": templates::PIPELINE,
    }))
}

async fn h_activity(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "activity": s.db.list_activity(100) }))
}

async fn h_projects_list(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "projects": s.db.list_projects() }))
}

async fn h_project_create(State(s): State<AppState>, Json(b): Json<Value>) -> Response {
    reply(project_create_value(
        &s,
        b["name"].as_str().unwrap_or(""),
        b["description"].as_str().unwrap_or(""),
        b["context"].as_str().unwrap_or(""),
    ))
}

async fn h_project_get(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    match s.db.get_project(id) {
        Some(p) => reply(json!({ "project": p, "features": s.db.list_features(id) })),
        None => reply(json!({ "error": format!("dự án #{id} không tồn tại") })),
    }
}

async fn h_project_update(State(s): State<AppState>, Path(id): Path<i64>, Json(b): Json<Value>) -> Response {
    if s.db.get_project(id).is_none() {
        return reply(json!({ "error": format!("dự án #{id} không tồn tại") }));
    }
    match s.db.update_project(id, b["name"].as_str(), b["description"].as_str(), b["context"].as_str()) {
        Ok(()) => reply(json!({ "ok": true, "project": s.db.get_project(id) })),
        Err(e) => reply(json!({ "error": e.to_string() })),
    }
}

async fn h_features_list(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(json!({ "features": s.db.list_features(id) }))
}

async fn h_feature_add(State(s): State<AppState>, Path(id): Path<i64>, Json(b): Json<Value>) -> Response {
    if s.db.get_project(id).is_none() {
        return reply(json!({ "error": format!("dự án #{id} không tồn tại") }));
    }
    reply(feature_add_value(
        &s,
        id,
        b["name"].as_str().unwrap_or(""),
        b["description"].as_str().unwrap_or(""),
        b["priority"].as_str().unwrap_or(""),
    ))
}

async fn h_feature_get(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    match s.db.get_feature(id) {
        Some(f) => reply(json!({ "feature": f })),
        None => reply(json!({ "error": format!("tính năng #{id} không tồn tại") })),
    }
}

async fn h_feature_update(State(s): State<AppState>, Path(id): Path<i64>, Json(b): Json<Value>) -> Response {
    if s.db.get_feature(id).is_none() {
        return reply(json!({ "error": format!("tính năng #{id} không tồn tại") }));
    }
    match s.db.update_feature(id, b["name"].as_str(), b["description"].as_str(), b["priority"].as_str(), b["status"].as_str()) {
        Ok(()) => reply(json!({ "ok": true, "feature": s.db.get_feature(id) })),
        Err(e) => reply(json!({ "error": e.to_string() })),
    }
}

async fn h_import_features(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    reply(engine::import_features_value(&s.db, id))
}

async fn h_dashboard(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    reply(dashboard_value(&s, id))
}

async fn h_kg(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    reply(kg_value(&s, id))
}

async fn h_trace(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    reply(trace_value(&s, id))
}

async fn h_docs_list(State(s): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(project_id) = q.get("project_id").and_then(|v| v.parse::<i64>().ok()) else {
        return reply(json!({ "error": "thiếu project_id" }));
    };
    let feature = match q.get("feature_id").map(|v| v.as_str()) {
        None => None,
        Some("project") => Some(None),
        Some(v) => match v.parse::<i64>() {
            Ok(fid) => Some(Some(fid)),
            Err(_) => return reply(json!({ "error": "feature_id phải là số hoặc 'project'" })),
        },
    };
    let docs = s.db.list_documents(project_id, feature, q.get("doc_type").map(|x| x.as_str()));
    reply(json!({ "documents": docs }))
}

async fn h_doc_write(State(s): State<AppState>, Json(b): Json<Value>) -> Response {
    let project_id = match resolve_project_value(&s, &body_key(&b, "project")) {
        Ok(p) => p,
        Err(e) => return reply(e),
    };
    let feature_id = match resolve_feature_opt(&s, project_id, &body_key(&b, "feature")) {
        Ok(f) => f,
        Err(e) => return reply(e),
    };
    reply(doc_write_value(
        &s,
        project_id,
        feature_id,
        b["doc_type"].as_str().unwrap_or(""),
        b["subtype"].as_str().unwrap_or(""),
        b["title"].as_str().unwrap_or(""),
        b["content"].as_str().unwrap_or(""),
    ))
}

fn body_key(b: &Value, k: &str) -> String {
    match &b[k] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

async fn h_doc_get(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    match s.db.get_document(id) {
        Some(d) => reply(json!({ "document": d })),
        None => reply(json!({ "error": format!("tài liệu #{id} không tồn tại") })),
    }
}

async fn h_doc_update(State(s): State<AppState>, Path(id): Path<i64>, Json(b): Json<Value>) -> Response {
    match s.db.update_document(id, b["title"].as_str(), b["content"].as_str(), b["status"].as_str()) {
        Ok(()) => {
            if b["content"].as_str().is_some() {
                trace::reindex_document(&s.db, id);
            }
            reply(json!({ "ok": true, "document": s.db.get_document(id) }))
        }
        Err(e) => reply(json!({ "error": e.to_string() })),
    }
}

async fn h_doc_delete(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    match s.db.delete_document(id) {
        Ok(()) => reply(json!({ "ok": true })),
        Err(e) => reply(json!({ "error": e.to_string() })),
    }
}

async fn h_doc_versions(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(json!({ "versions": s.db.doc_versions(id) }))
}

async fn h_doc_version_content(State(s): State<AppState>, Path((id, ver)): Path<(i64, i64)>) -> Response {
    match s.db.version_content(id, ver) {
        Some(c) => reply(json!({ "version": ver, "content": c })),
        None => reply(json!({ "error": format!("tài liệu #{id} không có version {ver}") })),
    }
}

async fn h_search(State(s): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let query = q.get("q").cloned().unwrap_or_default();
    if query.trim().is_empty() {
        return reply(json!({ "error": "thiếu q" }));
    }
    let project_id = q.get("project_id").and_then(|v| v.parse::<i64>().ok());
    reply(json!({ "results": s.db.search_docs(project_id, &query, 30) }))
}

async fn h_generate(State(s): State<AppState>, Json(b): Json<Value>) -> Response {
    let project_id = match resolve_project_value(&s, &body_key(&b, "project")) {
        Ok(p) => p,
        Err(e) => return reply(e),
    };
    let feature_id = match resolve_feature_opt(&s, project_id, &body_key(&b, "feature")) {
        Ok(f) => f,
        Err(e) => return reply(e),
    };
    let doc_type = b["doc_type"].as_str().unwrap_or("").to_string();
    let subtype = b["subtype"].as_str().unwrap_or("").to_string();
    let input = b["input"].as_str().unwrap_or("").to_string();
    let answers = b["answers"].as_str().unwrap_or("").to_string();
    let force = b["force"].as_bool().unwrap_or(false);
    let job_id = s.jobs.start("generate");
    let s2 = s.clone();
    tokio::spawn(async move {
        let out = engine::generate_value(&s2.db, project_id, feature_id, &doc_type, &subtype, &input, &answers, force).await;
        s2.jobs.finish(job_id, out);
    });
    reply(json!({ "ok": true, "job_id": job_id }))
}

async fn h_job(State(s): State<AppState>, Path(id): Path<u64>) -> Response {
    match s.jobs.get(id) {
        Some(j) => reply(j),
        None => reply(json!({ "error": format!("job #{id} không tồn tại (job không sống qua restart)") })),
    }
}

async fn h_workflow_templates() -> Json<Value> {
    Json(engine::workflow_templates_value())
}

async fn h_workflow_status(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    reply(engine::workflow_status_value(&s.db, id))
}

async fn h_workflow_start(State(s): State<AppState>, Path(id): Path<i64>, Json(b): Json<Value>) -> Response {
    let Some(f) = s.db.get_feature(id) else {
        return reply(json!({ "error": format!("tính năng #{id} không tồn tại") }));
    };
    let project_id = f["project_id"].as_i64().unwrap_or(0);
    let template = b["template"].as_str().unwrap_or("full-lifecycle");
    let custom = if b["steps"].is_array() { Some(&b["steps"]) } else { None };
    reply(engine::workflow_start_value(&s.db, project_id, id, template, custom))
}

async fn h_workflow_advance(State(s): State<AppState>, Path(id): Path<i64>, Json(b): Json<Value>) -> Response {
    let index = b["index"].as_u64().unwrap_or(0) as usize;
    let action = b["action"].as_str().unwrap_or("").to_string();
    let input = b["input"].as_str().unwrap_or("").to_string();
    let answers = b["answers"].as_str().unwrap_or("").to_string();
    if action == "run" {
        // Sinh AI lâu → job, UI poll.
        let job_id = s.jobs.start("workflow_run");
        let s2 = s.clone();
        tokio::spawn(async move {
            let out = engine::workflow_advance_value(&s2.db, id, index, "run", &input, &answers).await;
            s2.jobs.finish(job_id, out);
        });
        return reply(json!({ "ok": true, "job_id": job_id }));
    }
    reply(engine::workflow_advance_value(&s.db, id, index, &action, &input, &answers).await)
}

async fn h_crs_list(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(json!({ "crs": s.db.list_crs(id) }))
}

async fn h_cr_create(State(s): State<AppState>, Path(id): Path<i64>, Json(b): Json<Value>) -> Response {
    if s.db.get_project(id).is_none() {
        return reply(json!({ "error": format!("dự án #{id} không tồn tại") }));
    }
    let feature_id = match resolve_feature_opt(&s, id, &body_key(&b, "feature")) {
        Ok(f) => f,
        Err(e) => return reply(e),
    };
    let title = b["title"].as_str().unwrap_or("").to_string();
    let description = b["description"].as_str().unwrap_or("").to_string();
    let severity = b["severity"].as_str().unwrap_or("").to_string();
    let job_id = s.jobs.start("cr_create");
    let s2 = s.clone();
    tokio::spawn(async move {
        let out = cr::cr_create_value(&s2.db, id, feature_id, &title, &description, &severity).await;
        s2.jobs.finish(job_id, out);
    });
    reply(json!({ "ok": true, "job_id": job_id }))
}

async fn h_cr_get(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    match s.db.get_cr(id) {
        Some(cr) => reply(json!({ "cr": cr })),
        None => reply(json!({ "error": format!("CR #{id} không tồn tại") })),
    }
}

async fn h_cr_apply(State(s): State<AppState>, Path(id): Path<i64>, Json(b): Json<Value>) -> Response {
    let impact_id = b["impact_id"].as_i64();
    let job_id = s.jobs.start("cr_apply");
    let s2 = s.clone();
    tokio::spawn(async move {
        let out = cr::cr_apply_value(&s2.db, id, impact_id).await;
        s2.jobs.finish(job_id, out);
    });
    reply(json!({ "ok": true, "job_id": job_id }))
}

async fn h_cr_update(State(s): State<AppState>, Path(id): Path<i64>, Json(b): Json<Value>) -> Response {
    reply(cr::cr_update_value(&s.db, id, b["skip_impact"].as_i64(), b["close"].as_bool().unwrap_or(false)))
}

async fn h_ask(State(s): State<AppState>, Json(b): Json<Value>) -> Response {
    let project_id = match resolve_project_value(&s, &body_key(&b, "project")) {
        Ok(p) => p,
        Err(e) => return reply(e),
    };
    let question = b["question"].as_str().unwrap_or("").to_string();
    let job_id = s.jobs.start("ask");
    let s2 = s.clone();
    tokio::spawn(async move {
        let out = engine::ask_value(&s2.db, project_id, &question).await;
        s2.jobs.finish(job_id, out);
    });
    reply(json!({ "ok": true, "job_id": job_id }))
}

async fn h_qa_list(State(s): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(project_id) = q.get("project_id").and_then(|v| v.parse::<i64>().ok()) else {
        return reply(json!({ "error": "thiếu project_id" }));
    };
    reply(json!({ "qa": s.db.list_qa(project_id, 50) }))
}

fn export_params(q: &HashMap<String, String>) -> Result<(i64, Option<i64>, String), Value> {
    let Some(project_id) = q.get("project_id").and_then(|v| v.parse::<i64>().ok()) else {
        return Err(json!({ "error": "thiếu project_id" }));
    };
    let feature_id = q.get("feature_id").and_then(|v| v.parse::<i64>().ok());
    let format = q.get("format").cloned().unwrap_or_else(|| "md".into());
    Ok((project_id, feature_id, format))
}

async fn h_export(State(s): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    match export_params(&q) {
        Ok((p, f, fmt)) => reply(export::export_value(&s.db, p, f, &fmt)),
        Err(e) => reply(e),
    }
}

/// Tải thẳng nội dung (Content-Disposition) — UI mở link là có file.
async fn h_export_download(State(s): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let (project_id, feature_id, format) = match export_params(&q) {
        Ok(x) => x,
        Err(e) => return reply(e),
    };
    let (content, fname, ctype) = match format.as_str() {
        "html" => match export::preview_html(&s.db, project_id, feature_id, false) {
            Some(h) => (h, "ba-docs.html", "text/html; charset=utf-8"),
            None => return reply(json!({ "error": "không có tài liệu để xuất" })),
        },
        _ => match export::bundle_markdown(&s.db, project_id, feature_id) {
            Some((_, m)) => (m, "ba-docs.md", "text/markdown; charset=utf-8"),
            None => return reply(json!({ "error": "không có tài liệu để xuất" })),
        },
    };
    (
        [
            (header::CONTENT_TYPE, ctype.to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{fname}\"")),
        ],
        content,
    )
        .into_response()
}

async fn h_preview(State(s): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(project_id) = q.get("project_id").and_then(|v| v.parse::<i64>().ok()) else {
        return reply(json!({ "error": "thiếu project_id" }));
    };
    let feature_id = q.get("feature_id").and_then(|v| v.parse::<i64>().ok());
    match export::preview_html(&s.db, project_id, feature_id, true) {
        Some(html) => Html(html).into_response(),
        None => reply(json!({ "error": "dự án/tính năng không tồn tại" })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_write_respects_template_scope() {
        let s = make_test_state();
        let p = s.db.create_project("P", "", "").unwrap();
        // prd là cấp project → không cần feature
        let ok = doc_write_value(&s, p, None, "prd", "", "", "# PRD nội dung");
        assert_eq!(ok["ok"], true);
        // srs cấp feature → thiếu feature phải lỗi rõ
        let err = doc_write_value(&s, p, None, "srs", "", "", "# SRS");
        assert!(err["error"].as_str().unwrap().contains("cấp tính năng"));
        // loại không tồn tại
        let err2 = doc_write_value(&s, p, None, "nope", "", "", "x");
        assert!(err2["error"].as_str().unwrap().contains("registry"));
    }

    #[test]
    fn trace_value_reports_feature() {
        let s = make_test_state();
        let p = s.db.create_project("P", "", "").unwrap();
        let f = s.db.add_feature(p, "auth", "", "P0").unwrap();
        doc_write_value(&s, p, Some(f), "srs", "", "", "| FR-auth-001 | a | b |\n").get("ok").unwrap();
        let tv = trace_value(&s, f);
        assert_eq!(tv["coverage"]["fr_total"], 1);
        assert!(trace_value(&s, 999)["error"].as_str().is_some());
    }
}
