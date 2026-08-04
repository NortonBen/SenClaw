//! MCP server `ba-mcp` — JSON-RPC over HTTP + SSE, khung y hệt các Space App
//! khác. Nguyên tắc: every tool calls the SAME `crate::api::*_value` /
//! `crate::engine` helpers the REST UI uses, so agents and humans see
//! identical behavior. Tool nhận `project`/`feature` dạng id HOẶC slug.

use crate::state::AppState;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.mcp_tx.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message".to_string()));
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().event("message").data(msg));
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}
fn json_result(v: &Value) -> Value {
    text_result(serde_json::to_string_pretty(v).unwrap_or_default())
}
fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<AppState>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    let reply = |result: Value| -> Json<Value> {
        let resp = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
        let _ = state.mcp_tx.send(resp.to_string());
        Json(resp)
    };
    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "ba-mcp", "version": env!("CARGO_PKG_VERSION") }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
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

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

pub fn tools_list() -> Value {
    let project_p = json!({ "type": "string", "description": "Dự án — id số hoặc slug." });
    let feature_p = json!({ "type": "string", "description": "Tính năng — id số hoặc slug (bỏ trống với tài liệu cấp dự án)." });
    json!([
        {
            "name": "ba_status",
            "description": "Trạng thái app BA Studio + số lượng dự án/tính năng/tài liệu/CR. Gọi đầu phiên để biết đang có gì.",
            "inputSchema": obj(json!({}), &[])
        },
        {
            "name": "ba_project_create",
            "description": "Tạo dự án BA mới. name bắt buộc; description mô tả ngắn; context là bối cảnh nền cho AI (domain, thị trường, nền tảng đích, đối tượng người dùng) — viết càng kỹ tài liệu sinh ra càng sát.",
            "inputSchema": obj(json!({ "name": { "type": "string" }, "description": { "type": "string" }, "context": { "type": "string" } }), &["name"])
        },
        {
            "name": "ba_project_list",
            "description": "Liệt kê mọi dự án kèm số tính năng, số tài liệu.",
            "inputSchema": obj(json!({}), &[])
        },
        {
            "name": "ba_project_get",
            "description": "Chi tiết một dự án (mô tả, context) + danh sách tính năng của nó.",
            "inputSchema": obj(json!({ "project": project_p }), &["project"])
        },
        {
            "name": "ba_project_update",
            "description": "Sửa dự án: name / description / context (chỉ truyền trường muốn đổi).",
            "inputSchema": obj(json!({ "project": project_p, "name": { "type": "string" }, "description": { "type": "string" }, "context": { "type": "string" } }), &["project"])
        },
        {
            "name": "ba_feature_add",
            "description": "Thêm một tính năng vào dự án. priority P0 (lõi) / P1 / P2, mặc định P1. Slug tự sinh từ tên (dùng làm prefix ID: FR-<slug>-001).",
            "inputSchema": obj(json!({ "project": project_p, "name": { "type": "string" }, "description": { "type": "string" }, "priority": { "type": "string", "enum": ["P0", "P1", "P2"] } }), &["project", "name"])
        },
        {
            "name": "ba_feature_list",
            "description": "Liệt kê tính năng của dự án (slug, ưu tiên, trạng thái, số tài liệu).",
            "inputSchema": obj(json!({ "project": project_p }), &["project"])
        },
        {
            "name": "ba_feature_update",
            "description": "Sửa tính năng: name / description / priority / status (active|done|dropped).",
            "inputSchema": obj(json!({ "project": project_p, "feature": feature_p, "name": { "type": "string" }, "description": { "type": "string" }, "priority": { "type": "string" }, "status": { "type": "string" } }), &["project", "feature"])
        },
        {
            "name": "ba_feature_import_from_prd",
            "description": "Bóc bảng 'Danh sách tính năng' trong PRD của dự án thành các tính năng thật (bỏ qua slug đã tồn tại). Chạy sau khi sinh /prd để dựng khung dự án nhanh.",
            "inputSchema": obj(json!({ "project": project_p }), &["project"])
        },
        {
            "name": "ba_doc_list",
            "description": "Liệt kê tài liệu (không kèm nội dung). Lọc theo feature (id/slug, hoặc 'project' = chỉ tài liệu cấp dự án) và doc_type.",
            "inputSchema": obj(json!({ "project": project_p, "feature": { "type": "string", "description": "id/slug tính năng, hoặc 'project' để chỉ lấy tài liệu cấp dự án; bỏ trống = tất cả." }, "doc_type": { "type": "string" } }), &["project"])
        },
        {
            "name": "ba_doc_get",
            "description": "Đọc một tài liệu đầy đủ nội dung. Truyền doc_id, HOẶC bộ (project + feature? + doc_type + subtype?) để tra theo khoá.",
            "inputSchema": obj(json!({ "doc_id": { "type": "number" }, "project": project_p, "feature": feature_p, "doc_type": { "type": "string" }, "subtype": { "type": "string" } }), &[])
        },
        {
            "name": "ba_doc_write",
            "description": "Ghi tài liệu trực tiếp (người/agent tự soạn, không qua AI sinh). doc_type phải có trong catalog (ba_workflow_templates trả kèm); tài liệu cấp tính năng bắt buộc truyền feature. Đã tồn tại thì thành version mới, trạng thái quay về draft. Sau khi ghi, chỉ mục truy vết ID được đánh lại tự động.",
            "inputSchema": obj(json!({ "project": project_p, "feature": feature_p, "doc_type": { "type": "string" }, "subtype": { "type": "string", "description": "Bắt buộc với doc_type=diagram: sequence|activity|activity_swimlane|bpmn|state|erd|architecture|dbml|usecase." }, "title": { "type": "string" }, "content": { "type": "string" } }), &["project", "doc_type", "content"])
        },
        {
            "name": "ba_doc_update_status",
            "description": "Chuyển lifecycle tài liệu: draft → in_review → revisions → approved → shipped (kanban dashboard đọc trạng thái này).",
            "inputSchema": obj(json!({ "doc_id": { "type": "number" }, "status": { "type": "string", "enum": ["draft", "in_review", "revisions", "approved", "shipped"] } }), &["doc_id", "status"])
        },
        {
            "name": "ba_doc_search",
            "description": "Tìm toàn văn (FTS5, gõ không dấu vẫn ra) trên mọi tài liệu, trả snippet. Dùng trước khi trả lời câu hỏi nghiệp vụ để biết đọc doc nào.",
            "inputSchema": obj(json!({ "query": { "type": "string" }, "project": project_p }), &["query"])
        },
        {
            "name": "ba_doc_versions",
            "description": "Lịch sử version của một tài liệu (mỗi lần AI sinh lại / sửa tay / CR apply là một version).",
            "inputSchema": obj(json!({ "doc_id": { "type": "number" } }), &["doc_id"])
        },
        {
            "name": "ba_generate",
            "description": "Sinh tài liệu BA bằng AI theo template chuẩn (31 loại — xem catalog trong ba_workflow_templates). QUY TRÌNH: đầu vào mỏng thì tool trả needs_input + questions[] (phỏng vấn làm rõ) — hỏi người dùng rồi gọi LẠI kèm answers; force=true để bỏ phỏng vấn (AI tự giả định, giả định vào Open Questions). Tài liệu sinh xong tự đánh chỉ mục truy vết. AI đọc sẵn tài liệu upstream (brainstorm→srs→story→test) làm ngữ cảnh. Loại đáng chú ý: srs (11 mục FR/NFR/BR/Error/SC), reverse_doc (tái lập SRS từ văn bản/code dán vào input, kèm mức tin cậy), diagram (+subtype), wireframe_html/prototype_html (trả HTML render được), gap_report (soi lỗ hổng), doc_drift (đối chiếu code trong input với tài liệu).",
            "inputSchema": obj(json!({ "project": project_p, "feature": feature_p, "doc_type": { "type": "string" }, "subtype": { "type": "string" }, "input": { "type": "string", "description": "Đầu vào thô: ý tưởng, ghi chú họp, tài liệu API dán vào, source code (với reverse_doc/doc_drift)..." }, "answers": { "type": "string", "description": "Trả lời các câu phỏng vấn ở lần gọi trước (giữ nguyên thứ tự câu)." }, "force": { "type": "boolean" } }), &["project", "doc_type"])
        },
        {
            "name": "ba_workflow_templates",
            "description": "Danh sách 3 workflow mẫu (full-lifecycle / story-first / prototype-first) + catalog đầy đủ 9 giai đoạn × 31 loại tài liệu (doc_type, subtype, scope, có phỏng vấn hay không).",
            "inputSchema": obj(json!({}), &[])
        },
        {
            "name": "ba_workflow_start",
            "description": "Bắt đầu workflow cho một tính năng: template (full-lifecycle | story-first | prototype-first) HOẶC steps tuỳ biến [{doc_type, subtype?}]. Workflow active cũ của tính năng bị thay (abandoned).",
            "inputSchema": obj(json!({ "project": project_p, "feature": feature_p, "template": { "type": "string" }, "steps": { "type": "array", "items": { "type": "object" } } }), &["project", "feature"])
        },
        {
            "name": "ba_workflow_status",
            "description": "Workflow active của tính năng: các bước, trạng thái từng bước, bước kế tiếp (next_step), tài liệu đã có sẵn cho bước nào (existing_doc_id).",
            "inputSchema": obj(json!({ "project": project_p, "feature": feature_p }), &["project", "feature"])
        },
        {
            "name": "ba_workflow_advance",
            "description": "Tiến một bước workflow. action=run: AI sinh tài liệu của bước rồi đánh done (có thể trả needs_input như ba_generate); done: gắn tài liệu có sẵn; skip: bỏ qua; reset: về pending. Mọi bước xong → workflow done.",
            "inputSchema": obj(json!({ "workflow_id": { "type": "number" }, "index": { "type": "number", "description": "Chỉ số bước, 0-based (xem ba_workflow_status)." }, "action": { "type": "string", "enum": ["run", "done", "skip", "reset"] }, "input": { "type": "string" }, "answers": { "type": "string" } }), &["workflow_id", "index", "action"])
        },
        {
            "name": "ba_cr_create",
            "description": "Mở Change Request: mô tả thay đổi → AI phân tích tác động trên TÀI LIỆU THẬT của dự án/tính năng, trả danh sách impact (tài liệu nào, sửa gì). Mã tự sinh CR-YYYYMMDD-NNN. severity low|medium|high. Sau đó dùng ba_cr_apply cập nhật từng tài liệu.",
            "inputSchema": obj(json!({ "project": project_p, "feature": feature_p, "title": { "type": "string" }, "description": { "type": "string", "description": "Thay đổi là gì, vì sao — càng cụ thể phân tích càng trúng." }, "severity": { "type": "string", "enum": ["low", "medium", "high"] } }), &["project", "title", "description"])
        },
        {
            "name": "ba_cr_list",
            "description": "Danh sách CR của dự án kèm số impact còn treo (pending).",
            "inputSchema": obj(json!({ "project": project_p }), &["project"])
        },
        {
            "name": "ba_cr_get",
            "description": "Chi tiết một CR: phân tích tác động + bảng impact từng tài liệu (pending/applied/skipped).",
            "inputSchema": obj(json!({ "cr_id": { "type": "number" } }), &["cr_id"])
        },
        {
            "name": "ba_cr_apply",
            "description": "Áp MỘT impact của CR: AI viết lại tài liệu đó theo thay đổi (giữ khung + ID cũ, thêm ghi chú CR dưới tiêu đề), lưu version mới, trạng thái tài liệu quay về draft chờ review. Không truyền impact_id thì áp impact pending đầu tiên. Áp hết pending → CR chuyển applied.",
            "inputSchema": obj(json!({ "cr_id": { "type": "number" }, "impact_id": { "type": "number" } }), &["cr_id"])
        },
        {
            "name": "ba_cr_update",
            "description": "Bỏ qua một impact (skip_impact=id) hoặc đóng CR (close=true).",
            "inputSchema": obj(json!({ "cr_id": { "type": "number" }, "skip_impact": { "type": "number" }, "close": { "type": "boolean" } }), &["cr_id"])
        },
        {
            "name": "ba_gap_check",
            "description": "Soi lỗ hổng nghiệp vụ của một tính năng: AI đối chiếu chéo toàn bộ tài liệu (FR thiếu flow, error không màn nào hiện, ngưỡng brainstorm mà SRS quên...) và lưu thành tài liệu gap_report. Tương đương ba_generate với doc_type=gap_report.",
            "inputSchema": obj(json!({ "project": project_p, "feature": feature_p }), &["project", "feature"])
        },
        {
            "name": "ba_ask",
            "description": "Hỏi đáp nghiệp vụ trên bộ tài liệu dự án — trả lời kèm trích dẫn tài liệu nguồn (doc #id); điều tài liệu chưa quy định sẽ nói thẳng là chưa quy định. Lưu vào nhật ký QA.",
            "inputSchema": obj(json!({ "project": project_p, "question": { "type": "string" } }), &["project", "question"])
        },
        {
            "name": "ba_trace",
            "description": "Ma trận truy vết deterministic của một tính năng: coverage FR↔US (FR chưa phủ, US mồ côi, US thiếu AC, FR/UC chưa có test), pipeline 8 chặng URD→BRD→PRD→SRS→UseCase→Story→AC→Test, độ tươi tài liệu (stale theo chuỗi upstream). Số liệu do code parse ID tính, không phải AI đoán.",
            "inputSchema": obj(json!({ "project": project_p, "feature": feature_p }), &["project", "feature"])
        },
        {
            "name": "ba_kg",
            "description": "Knowledge Graph liên kết tài liệu của dự án (deterministic, không AI): node = tài liệu, cạnh upstream (tài liệu sau đọc tài liệu trước khi sinh) + cạnh ref (tài liệu nhắc ID mà tài liệu kia định nghĩa, kèm số lượng). Trả JSON nodes/edges + chuỗi mermaid graph. Dùng để biết sửa một tài liệu thì lan sang đâu, hoặc chọn đúng tài liệu cần đọc thay vì quét cả bộ.",
            "inputSchema": obj(json!({ "project": project_p }), &["project"])
        },
        {
            "name": "ba_dashboard",
            "description": "Dashboard dự án: 4 KPI (coverage, pipeline, độ tươi, việc gấp), danh sách việc gấp (CR treo, doc stale, review quá hạn, OQ tồn), kanban tài liệu theo lifecycle, tiến độ từng tính năng, stale chain.",
            "inputSchema": obj(json!({ "project": project_p }), &["project"])
        },
        {
            "name": "ba_export",
            "description": "Xuất bộ tài liệu (cả dự án hoặc một tính năng) ra file trong thư mục exports: format md (gói markdown) | html (trang tự chứa giống trang preview, mở offline được, wireframe/prototype nhúng iframe). Trả đường dẫn file. PDF/Word: mở bản html rồi in/convert.",
            "inputSchema": obj(json!({ "project": project_p, "feature": feature_p, "format": { "type": "string", "enum": ["md", "html"] } }), &["project"])
        }
    ])
}

async fn call_tool(s: &AppState, name: &str, args: &Value) -> Value {
    let str_arg = |k: &str| -> String {
        match &args[k] {
            Value::String(v) => v.clone(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        }
    };
    let opt_str = |k: &str| args.get(k).and_then(|x| x.as_str()).map(|v| v.to_string());
    let i64_arg = |k: &str| args.get(k).and_then(|x| x.as_i64());
    let bool_arg = |k: &str| args.get(k).and_then(|x| x.as_bool()).unwrap_or(false);

    // Đa số tool cần project (id/slug) — resolve một lần.
    let need_project = || -> Result<i64, Value> {
        crate::api::resolve_project_value(s, &str_arg("project")).map_err(|e| error_result(e["error"].as_str().unwrap_or("dự án không tồn tại").to_string()))
    };
    let feature_of = |project_id: i64| -> Result<Option<i64>, Value> {
        crate::api::resolve_feature_opt(s, project_id, &str_arg("feature"))
            .map_err(|e| error_result(e["error"].as_str().unwrap_or("tính năng không tồn tại").to_string()))
    };
    let need_feature = |project_id: i64| -> Result<i64, Value> {
        match feature_of(project_id)? {
            Some(f) => Ok(f),
            None => Err(error_result("thiếu 'feature' (id hoặc slug)".into())),
        }
    };
    // Kết quả engine {error} → isError để agent thấy ngay.
    fn wrap(v: Value) -> Value {
        if let Some(e) = v.get("error").and_then(|x| x.as_str()) {
            error_result(e.to_string())
        } else {
            json_result(&v)
        }
    }

    match name {
        "ba_status" => json_result(&crate::api::status_value(s)),
        "ba_project_create" => wrap(crate::api::project_create_value(
            s,
            &str_arg("name"),
            &str_arg("description"),
            &str_arg("context"),
        )),
        "ba_project_list" => json_result(&json!({ "projects": s.db.list_projects() })),
        "ba_project_get" => match need_project() {
            Ok(p) => json_result(&json!({ "project": s.db.get_project(p), "features": s.db.list_features(p) })),
            Err(e) => e,
        },
        "ba_project_update" => match need_project() {
            Ok(p) => match s.db.update_project(p, opt_str("name").as_deref(), opt_str("description").as_deref(), opt_str("context").as_deref()) {
                Ok(()) => json_result(&json!({ "ok": true, "project": s.db.get_project(p) })),
                Err(e) => error_result(e.to_string()),
            },
            Err(e) => e,
        },
        "ba_feature_add" => match need_project() {
            Ok(p) => wrap(crate::api::feature_add_value(s, p, &str_arg("name"), &str_arg("description"), &str_arg("priority"))),
            Err(e) => e,
        },
        "ba_feature_list" => match need_project() {
            Ok(p) => json_result(&json!({ "features": s.db.list_features(p) })),
            Err(e) => e,
        },
        "ba_feature_update" => match need_project() {
            Ok(p) => match need_feature(p) {
                Ok(f) => match s.db.update_feature(f, opt_str("name").as_deref(), opt_str("description").as_deref(), opt_str("priority").as_deref(), opt_str("status").as_deref()) {
                    Ok(()) => json_result(&json!({ "ok": true, "feature": s.db.get_feature(f) })),
                    Err(e) => error_result(e.to_string()),
                },
                Err(e) => e,
            },
            Err(e) => e,
        },
        "ba_feature_import_from_prd" => match need_project() {
            Ok(p) => wrap(crate::engine::import_features_value(&s.db, p)),
            Err(e) => e,
        },
        "ba_doc_list" => match need_project() {
            Ok(p) => {
                let feature = match str_arg("feature").as_str() {
                    "" => None,
                    "project" => Some(None),
                    key => match s.db.resolve_feature(p, key) {
                        Some(f) => Some(Some(f)),
                        None => return error_result(format!("tính năng '{key}' không có trong dự án")),
                    },
                };
                json_result(&json!({ "documents": s.db.list_documents(p, feature, opt_str("doc_type").as_deref()) }))
            }
            Err(e) => e,
        },
        "ba_doc_get" => {
            if let Some(id) = i64_arg("doc_id") {
                match s.db.get_document(id) {
                    Some(d) => json_result(&json!({ "document": d })),
                    None => error_result(format!("tài liệu #{id} không tồn tại")),
                }
            } else {
                match need_project() {
                    Ok(p) => match feature_of(p) {
                        Ok(f) => {
                            let dt = str_arg("doc_type");
                            let st = str_arg("subtype");
                            let scope_feature = crate::templates::get(&dt, &st).map(|t| t.scope == crate::templates::Scope::Feature).unwrap_or(true);
                            let effective = if scope_feature { f } else { None };
                            match s.db.find_document(p, effective, &dt, &st).and_then(|id| s.db.get_document(id)) {
                                Some(d) => json_result(&json!({ "document": d })),
                                None => error_result(format!("chưa có tài liệu '{dt}/{st}' — ba_generate để sinh")),
                            }
                        }
                        Err(e) => e,
                    },
                    Err(e) => e,
                }
            }
        }
        "ba_doc_write" => match need_project() {
            Ok(p) => match feature_of(p) {
                Ok(f) => wrap(crate::api::doc_write_value(s, p, f, &str_arg("doc_type"), &str_arg("subtype"), &str_arg("title"), &str_arg("content"))),
                Err(e) => e,
            },
            Err(e) => e,
        },
        "ba_doc_update_status" => match i64_arg("doc_id") {
            Some(id) => match s.db.update_document(id, None, None, Some(&str_arg("status"))) {
                Ok(()) => json_result(&json!({ "ok": true, "document": s.db.get_document(id) })),
                Err(e) => error_result(e.to_string()),
            },
            None => error_result("thiếu 'doc_id'".into()),
        },
        "ba_doc_search" => {
            let q = str_arg("query");
            if q.trim().is_empty() {
                return error_result("thiếu 'query'".into());
            }
            let project_id = if str_arg("project").is_empty() {
                None
            } else {
                match need_project() {
                    Ok(p) => Some(p),
                    Err(e) => return e,
                }
            };
            json_result(&json!({ "results": s.db.search_docs(project_id, &q, 30) }))
        }
        "ba_doc_versions" => match i64_arg("doc_id") {
            Some(id) => json_result(&json!({ "versions": s.db.doc_versions(id) })),
            None => error_result("thiếu 'doc_id'".into()),
        },
        "ba_generate" => match need_project() {
            Ok(p) => match feature_of(p) {
                Ok(f) => wrap(
                    crate::engine::generate_value(&s.db, p, f, &str_arg("doc_type"), &str_arg("subtype"), &str_arg("input"), &str_arg("answers"), bool_arg("force")).await,
                ),
                Err(e) => e,
            },
            Err(e) => e,
        },
        "ba_workflow_templates" => json_result(&crate::engine::workflow_templates_value()),
        "ba_workflow_start" => match need_project() {
            Ok(p) => match need_feature(p) {
                Ok(f) => {
                    let custom = args.get("steps").filter(|v| v.is_array());
                    let template = str_arg("template").pipe_default("full-lifecycle");
                    wrap(crate::engine::workflow_start_value(&s.db, p, f, &template, custom))
                }
                Err(e) => e,
            },
            Err(e) => e,
        },
        "ba_workflow_status" => match need_project() {
            Ok(p) => match need_feature(p) {
                Ok(f) => wrap(crate::engine::workflow_status_value(&s.db, f)),
                Err(e) => e,
            },
            Err(e) => e,
        },
        "ba_workflow_advance" => match i64_arg("workflow_id") {
            Some(wid) => {
                let index = args.get("index").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                wrap(crate::engine::workflow_advance_value(&s.db, wid, index, &str_arg("action"), &str_arg("input"), &str_arg("answers")).await)
            }
            None => error_result("thiếu 'workflow_id'".into()),
        },
        "ba_cr_create" => match need_project() {
            Ok(p) => match feature_of(p) {
                Ok(f) => wrap(crate::cr::cr_create_value(&s.db, p, f, &str_arg("title"), &str_arg("description"), &str_arg("severity")).await),
                Err(e) => e,
            },
            Err(e) => e,
        },
        "ba_cr_list" => match need_project() {
            Ok(p) => json_result(&json!({ "crs": s.db.list_crs(p) })),
            Err(e) => e,
        },
        "ba_cr_get" => match i64_arg("cr_id") {
            Some(id) => match s.db.get_cr(id) {
                Some(cr) => json_result(&json!({ "cr": cr })),
                None => error_result(format!("CR #{id} không tồn tại")),
            },
            None => error_result("thiếu 'cr_id'".into()),
        },
        "ba_cr_apply" => match i64_arg("cr_id") {
            Some(id) => wrap(crate::cr::cr_apply_value(&s.db, id, i64_arg("impact_id")).await),
            None => error_result("thiếu 'cr_id'".into()),
        },
        "ba_cr_update" => match i64_arg("cr_id") {
            Some(id) => wrap(crate::cr::cr_update_value(&s.db, id, i64_arg("skip_impact"), bool_arg("close"))),
            None => error_result("thiếu 'cr_id'".into()),
        },
        "ba_gap_check" => match need_project() {
            Ok(p) => match need_feature(p) {
                Ok(f) => wrap(crate::engine::generate_value(&s.db, p, Some(f), "gap_report", "", "", "", true).await),
                Err(e) => e,
            },
            Err(e) => e,
        },
        "ba_ask" => match need_project() {
            Ok(p) => wrap(crate::engine::ask_value(&s.db, p, &str_arg("question")).await),
            Err(e) => e,
        },
        "ba_trace" => match need_project() {
            Ok(p) => match need_feature(p) {
                Ok(f) => wrap(crate::api::trace_value(s, f)),
                Err(e) => e,
            },
            Err(e) => e,
        },
        "ba_dashboard" => match need_project() {
            Ok(p) => wrap(crate::api::dashboard_value(s, p)),
            Err(e) => e,
        },
        "ba_kg" => match need_project() {
            Ok(p) => wrap(crate::api::kg_value(s, p)),
            Err(e) => e,
        },
        "ba_export" => match need_project() {
            Ok(p) => match feature_of(p) {
                Ok(f) => wrap(crate::export::export_value(&s.db, p, f, &str_arg("format"))),
                Err(e) => e,
            },
            Err(e) => e,
        },
        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

/// `"".pipe_default("x")` — gọn cho default template key.
trait PipeDefault {
    fn pipe_default(self, d: &str) -> String;
}
impl PipeDefault for String {
    fn pipe_default(self, d: &str) -> String {
        if self.trim().is_empty() {
            d.to_string()
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Vec<String> {
        tools_list()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn tool_names_unique_prefixed_counted() {
        let names = names();
        assert_eq!(names.len(), 31, "số tool thay đổi thì cập nhật test + manifest description");
        let mut seen = std::collections::HashSet::new();
        for n in &names {
            assert!(n.starts_with("ba_"), "{n} sai prefix");
            assert!(seen.insert(n.clone()), "{n} trùng");
        }
    }

    #[test]
    fn tool_descriptions_and_schemas_sane() {
        for t in tools_list().as_array().unwrap() {
            let name = t["name"].as_str().unwrap();
            assert!(
                t["description"].as_str().unwrap().chars().count() > 20,
                "{name} description quá ngắn"
            );
            assert_eq!(t["inputSchema"]["type"], "object", "{name} schema phải là object");
        }
    }

    #[tokio::test]
    async fn call_tool_full_flow_without_llm() {
        crate::export::ensure_test_data_dir();
        let s = crate::api::make_test_state();
        // status
        let st = call_tool(&s, "ba_status", &json!({})).await;
        assert!(st["isError"].is_null());
        // tool lạ
        let bad = call_tool(&s, "ba_nope", &json!({})).await;
        assert_eq!(bad["isError"], true);
        // project + feature + doc + trace qua MCP path (slug resolution)
        call_tool(&s, "ba_project_create", &json!({ "name": "Demo Shop", "context": "web bán hàng" })).await;
        let out = call_tool(&s, "ba_feature_add", &json!({ "project": "demo-shop", "name": "Giỏ hàng", "priority": "P0" })).await;
        assert!(out["isError"].is_null(), "{out}");
        let w = call_tool(
            &s,
            "ba_doc_write",
            &json!({ "project": "demo-shop", "feature": "gio-hang", "doc_type": "srs", "content": "# SRS\n| FR-gio-hang-001 | thêm vào giỏ | Khi bấm, hệ thống phải thêm | P0 | test | brainstorm |" }),
        )
        .await;
        assert!(w["isError"].is_null(), "{w}");
        let tr = call_tool(&s, "ba_trace", &json!({ "project": "demo-shop", "feature": "gio-hang" })).await;
        let body: Value = serde_json::from_str(tr["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(body["coverage"]["fr_total"], 1);
        // workflow không cần LLM với done/skip
        let ws = call_tool(&s, "ba_workflow_start", &json!({ "project": "demo-shop", "feature": "gio-hang", "template": "story-first" })).await;
        assert!(ws["isError"].is_null(), "{ws}");
        // search
        let se = call_tool(&s, "ba_doc_search", &json!({ "query": "gio hang", "project": "demo-shop" })).await;
        let sbody: Value = serde_json::from_str(se["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(sbody["results"].as_array().unwrap().len(), 1);
        // export md không cần LLM
        let ex = call_tool(&s, "ba_export", &json!({ "project": "demo-shop", "feature": "gio-hang", "format": "md" })).await;
        assert!(ex["isError"].is_null(), "{ex}");
    }
}
