//! MCP server (HTTP + SSE) cho agent SenClaw điều khiển kiểm thử tự động.
//! Tool prefix `autotest_` theo convention đặt tên SenClaw; mọi tool gọi CHUNG
//! các helper `crate::api::*_value` với REST UI — agent và người thấy hành vi
//! y hệt nhau. `autotest_run_*` ĐỢI chạy xong và trả kết quả chi tiết từng
//! assertion nên agent nên giữ suite gọn (case bị cap timeout 10 phút/case).

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;

use crate::api::{self, AppState};

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
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
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
            "serverInfo": { "name": "autotest-mcp", "version": "1.0.0" }
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

fn tools_list() -> Value {
    let case_schema_note = "config tuỳ kind — http: {method,url,headers{},body} · script: {command,cwd,env{}} · web: {steps:[{action:navigate,url}|{action:act,instruction}|{action:wait,ms}]}. assertions: mảng — http: status/json(path)/body_contains/header/duration_max_ms · script: exit_code/stdout_contains/stdout_matches/stderr_contains · web: text_contains/text_not_contains/url_contains. extract (trích biến cho case sau): [{var,from:json,path}|{var,from:header,name}|{var,from:regex,pattern}]. Mọi chuỗi được thay {{biến}} từ environment + biến đã trích.";
    json!([
        {
            "name": "autotest_status",
            "description": "Trạng thái nhanh: số suite/case, run đang chạy, số run hôm nay, tỷ lệ pass gần đây, số lịch đang bật.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "autotest_suite_add",
            "description": "Tạo bộ kiểm thử (suite) mới. env_id gán environment mặc định (bộ biến {{var}}).",
            "inputSchema": { "type": "object", "properties": {
                "name":        { "type": "string" },
                "description": { "type": "string" },
                "env_id":      { "type": "number" }
            }, "required": ["name"] }
        },
        {
            "name": "autotest_suite_list",
            "description": "Liệt kê suite: số case, trạng thái + thời điểm lần chạy gần nhất.",
            "inputSchema": { "type": "object", "properties": {
                "all": { "type": "boolean", "description": "true = gồm cả suite archived." }
            } }
        },
        {
            "name": "autotest_suite_get",
            "description": "Chi tiết một suite: thông tin + toàn bộ test case (config/assertions/extract) + lịch chạy. Dùng TRƯỚC khi sửa case để thấy cấu trúc hiện tại.",
            "inputSchema": { "type": "object", "properties": {
                "suite_id": { "type": "number" }
            }, "required": ["suite_id"] }
        },
        {
            "name": "autotest_suite_update",
            "description": "Sửa suite (patch — chỉ trường truyền vào mới đổi). Lưu trữ bằng status='archived'; env_id=0 gỡ environment mặc định.",
            "inputSchema": { "type": "object", "properties": {
                "suite_id":    { "type": "number" },
                "name":        { "type": "string" },
                "description": { "type": "string" },
                "env_id":      { "type": "number" },
                "status":      { "type": "string", "enum": ["active", "archived"] }
            }, "required": ["suite_id"] }
        },
        {
            "name": "autotest_suite_delete",
            "description": "XOÁ suite cùng toàn bộ case, lịch chạy và lịch sử run của nó. Không hoàn tác được — cân nhắc archive (autotest_suite_update status='archived') trước.",
            "inputSchema": { "type": "object", "properties": {
                "suite_id": { "type": "number" }
            }, "required": ["suite_id"] }
        },
        {
            "name": "autotest_case_add",
            "description": format!("Thêm test case vào suite. kind: http (gọi API) | script (lệnh shell người dùng định nghĩa) | web (điều khiển Mini Browser — app Mini Browser port 4360 phải đang chạy). {case_schema_note}"),
            "inputSchema": { "type": "object", "properties": {
                "suite_id":   { "type": "number" },
                "name":       { "type": "string" },
                "kind":       { "type": "string", "enum": ["http", "script", "web"] },
                "config":     { "type": "object" },
                "assertions": { "type": "array" },
                "extract":    { "type": "array" },
                "timeout_ms": { "type": "number", "description": "Mặc định 30000, tối đa 600000." },
                "position":   { "type": "number", "description": "Thứ tự chạy trong suite (mặc định cuối)." },
                "enabled":    { "type": "boolean" }
            }, "required": ["suite_id", "name", "kind", "config"] }
        },
        {
            "name": "autotest_case_update",
            "description": "Sửa test case (patch). Truyền config/assertions/extract là THAY THẾ nguyên khối trường đó — lấy giá trị hiện tại qua autotest_suite_get rồi sửa.",
            "inputSchema": { "type": "object", "properties": {
                "case_id":    { "type": "number" },
                "name":       { "type": "string" },
                "kind":       { "type": "string", "enum": ["http", "script", "web"] },
                "config":     { "type": "object" },
                "assertions": { "type": "array" },
                "extract":    { "type": "array" },
                "timeout_ms": { "type": "number" },
                "position":   { "type": "number" },
                "enabled":    { "type": "boolean" }
            }, "required": ["case_id"] }
        },
        {
            "name": "autotest_case_delete",
            "description": "Xoá một test case.",
            "inputSchema": { "type": "object", "properties": {
                "case_id": { "type": "number" }
            }, "required": ["case_id"] }
        },
        {
            "name": "autotest_env_set",
            "description": "Tạo/cập nhật environment (upsert theo tên): bộ biến {{var}} như base_url, token… vars là object {\"base_url\":\"http://…\"}. Suite trỏ env mặc định; run có thể override.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" },
                "vars": { "type": "object" }
            }, "required": ["name", "vars"] }
        },
        {
            "name": "autotest_env_list",
            "description": "Liệt kê environment và biến của chúng.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "autotest_run_suite",
            "description": "CHẠY cả suite và ĐỢI kết quả: trạng thái từng case, từng assertion (desc/pass/actual/expected), log request/response. env_id override environment. Suite dài có thể chạy vài phút — giữ suite gọn khi gọi từ agent.",
            "inputSchema": { "type": "object", "properties": {
                "suite_id": { "type": "number" },
                "env_id":   { "type": "number" }
            }, "required": ["suite_id"] }
        },
        {
            "name": "autotest_run_case",
            "description": "Chạy MỘT test case và đợi kết quả chi tiết (vẫn ghi vào lịch sử). Dùng để debug nhanh một case.",
            "inputSchema": { "type": "object", "properties": {
                "case_id": { "type": "number" },
                "env_id":  { "type": "number" }
            }, "required": ["case_id"] }
        },
        {
            "name": "autotest_run_list",
            "description": "Lịch sử các lần chạy (mới nhất trước): trạng thái, đếm pass/fail/error/skipped, trigger manual|schedule|mcp.",
            "inputSchema": { "type": "object", "properties": {
                "suite_id": { "type": "number" },
                "limit":    { "type": "number", "description": "Mặc định 20." }
            } }
        },
        {
            "name": "autotest_run_get",
            "description": "Chi tiết một lần chạy: kết quả + log + từng assertion của mỗi case. Dùng khi cần biết CHÍNH XÁC cái gì lệch.",
            "inputSchema": { "type": "object", "properties": {
                "run_id": { "type": "number" }
            }, "required": ["run_id"] }
        },
        {
            "name": "autotest_report",
            "description": "Báo cáo sức khoẻ kiểm thử: xu hướng pass 30 run gần nhất, danh sách test FLAKY (lúc pass lúc fail — kèm chuỗi kết quả gần đây), case fail nhiều nhất 30 ngày. Dùng tool này TRƯỚC khi trả lời câu hỏi tổng quan về chất lượng test.",
            "inputSchema": { "type": "object", "properties": {
                "suite_id": { "type": "number", "description": "Giới hạn xu hướng theo một suite (flaky/top-fail luôn toàn cục)." }
            } }
        },
        {
            "name": "autotest_schedule_set",
            "description": "Đặt lịch chạy định kỳ cho suite: mỗi interval_min phút (≥1), enabled bật/tắt. Upsert theo suite.",
            "inputSchema": { "type": "object", "properties": {
                "suite_id":     { "type": "number" },
                "interval_min": { "type": "number" },
                "enabled":      { "type": "boolean" }
            }, "required": ["suite_id", "interval_min"] }
        },
        {
            "name": "autotest_schedule_list",
            "description": "Liệt kê lịch chạy định kỳ của các suite.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "autotest_schedule_delete",
            "description": "Xoá lịch chạy định kỳ của một suite.",
            "inputSchema": { "type": "object", "properties": {
                "suite_id": { "type": "number" }
            }, "required": ["suite_id"] }
        },
        {
            "name": "autotest_ai_generate",
            "description": "AI sinh test case từ mô tả tự nhiên / đoạn OpenAPI / lệnh curl mẫu, thêm thẳng vào suite (apply=false chỉ xem trước, không ghi). AI được gợi ý các biến environment sẵn có để dùng {{var}} thay vì hard-code.",
            "inputSchema": { "type": "object", "properties": {
                "suite_id":    { "type": "number" },
                "description": { "type": "string", "description": "Mô tả tính năng cần test, OpenAPI, curl…" },
                "apply":       { "type": "boolean", "description": "Mặc định true — ghi case vào suite." }
            }, "required": ["suite_id", "description"] }
        },
        {
            "name": "autotest_ai_diagnose",
            "description": "AI chẩn đoán một lần chạy fail: đọc log + assertion lệch, phân biệt lỗi sản phẩm vs lỗi test, đề xuất bước sửa. Trả markdown tiếng Việt.",
            "inputSchema": { "type": "object", "properties": {
                "run_id":   { "type": "number" },
                "question": { "type": "string", "description": "Câu hỏi cụ thể (tuỳ chọn)." }
            }, "required": ["run_id"] }
        }
    ])
}

fn as_i64(args: &Value, key: &str) -> Option<i64> {
    args.get(key).and_then(|v| v.as_i64())
}
fn as_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    let need = |key: &str| error_result(format!("thiếu tham số bắt buộc: {key}"));
    match name {
        "autotest_status" => json_result(&api::status_value(state)),
        "autotest_suite_add" => {
            let Some(name_) = as_str(args, "name") else {
                return need("name");
            };
            let b = api::SuiteIn {
                name: name_.to_string(),
                description: as_str(args, "description").unwrap_or("").to_string(),
                env_id: as_i64(args, "env_id"),
            };
            json_result(&api::add_suite_value(state, &b))
        }
        "autotest_suite_list" => json_result(&api::list_suites_value(
            state,
            args.get("all").and_then(|v| v.as_bool()).unwrap_or(false),
        )),
        "autotest_suite_get" => {
            let Some(id) = as_i64(args, "suite_id") else {
                return need("suite_id");
            };
            json_result(&api::get_suite_value(state, id))
        }
        "autotest_suite_update" => {
            let Some(id) = as_i64(args, "suite_id") else {
                return need("suite_id");
            };
            let b = api::SuiteUpdateIn {
                name: as_str(args, "name").map(String::from),
                description: as_str(args, "description").map(String::from),
                env_id: as_i64(args, "env_id"),
                status: as_str(args, "status").map(String::from),
            };
            json_result(&api::update_suite_value(state, id, &b))
        }
        "autotest_suite_delete" => {
            let Some(id) = as_i64(args, "suite_id") else {
                return need("suite_id");
            };
            json_result(&api::delete_suite_value(state, id))
        }
        "autotest_case_add" => {
            let Some(suite_id) = as_i64(args, "suite_id") else {
                return need("suite_id");
            };
            let Some(name_) = as_str(args, "name") else {
                return need("name");
            };
            let b = api::CaseIn {
                suite_id,
                name: name_.to_string(),
                kind: as_str(args, "kind").unwrap_or("http").to_string(),
                position: as_i64(args, "position"),
                enabled: args
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                timeout_ms: as_i64(args, "timeout_ms").unwrap_or(30000),
                config: args.get("config").cloned().unwrap_or(Value::Null),
                assertions: args.get("assertions").cloned().unwrap_or(Value::Null),
                extract: args.get("extract").cloned().unwrap_or(Value::Null),
            };
            json_result(&api::add_case_value(state, &b))
        }
        "autotest_case_update" => {
            let Some(id) = as_i64(args, "case_id") else {
                return need("case_id");
            };
            let b = api::CaseUpdateIn {
                name: as_str(args, "name").map(String::from),
                kind: as_str(args, "kind").map(String::from),
                position: as_i64(args, "position"),
                enabled: args.get("enabled").and_then(|v| v.as_bool()),
                timeout_ms: as_i64(args, "timeout_ms"),
                config: args.get("config").cloned().unwrap_or(Value::Null),
                assertions: args.get("assertions").cloned().unwrap_or(Value::Null),
                extract: args.get("extract").cloned().unwrap_or(Value::Null),
            };
            json_result(&api::update_case_value(state, id, &b))
        }
        "autotest_case_delete" => {
            let Some(id) = as_i64(args, "case_id") else {
                return need("case_id");
            };
            json_result(&api::delete_case_value(state, id))
        }
        "autotest_env_set" => {
            let Some(name_) = as_str(args, "name") else {
                return need("name");
            };
            let b = api::EnvIn {
                name: name_.to_string(),
                vars: args.get("vars").cloned().unwrap_or(Value::Null),
            };
            json_result(&api::set_env_value(state, &b))
        }
        "autotest_env_list" => json_result(&api::list_envs_value(state)),
        "autotest_run_suite" => {
            let Some(suite_id) = as_i64(args, "suite_id") else {
                return need("suite_id");
            };
            let b = api::RunSuiteIn {
                suite_id,
                env_id: as_i64(args, "env_id"),
                wait: true,
            };
            json_result(&api::run_suite_value(state, &b, "mcp").await)
        }
        "autotest_run_case" => {
            let Some(case_id) = as_i64(args, "case_id") else {
                return need("case_id");
            };
            let b = api::RunCaseIn {
                case_id,
                env_id: as_i64(args, "env_id"),
            };
            json_result(&api::run_case_value(state, &b, "mcp").await)
        }
        "autotest_run_list" => json_result(&api::list_runs_value(
            state,
            as_i64(args, "suite_id"),
            as_i64(args, "limit").unwrap_or(20),
        )),
        "autotest_run_get" => {
            let Some(id) = as_i64(args, "run_id") else {
                return need("run_id");
            };
            json_result(&api::get_run_value(state, id))
        }
        "autotest_report" => json_result(&api::report_value(state, as_i64(args, "suite_id"))),
        "autotest_schedule_set" => {
            let Some(suite_id) = as_i64(args, "suite_id") else {
                return need("suite_id");
            };
            let Some(interval) = as_i64(args, "interval_min") else {
                return need("interval_min");
            };
            let b = api::ScheduleIn {
                suite_id,
                interval_min: interval,
                enabled: args
                    .get("enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
            };
            json_result(&api::set_schedule_value(state, &b))
        }
        "autotest_schedule_list" => json_result(&api::list_schedules_value(state)),
        "autotest_schedule_delete" => {
            let Some(suite_id) = as_i64(args, "suite_id") else {
                return need("suite_id");
            };
            match state.db.schedule_delete(suite_id) {
                Ok(()) => json_result(&json!({ "ok": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "autotest_ai_generate" => {
            let Some(suite_id) = as_i64(args, "suite_id") else {
                return need("suite_id");
            };
            let Some(description) = as_str(args, "description") else {
                return need("description");
            };
            let b = api::GenerateIn {
                suite_id,
                description: description.to_string(),
                apply: args.get("apply").and_then(|v| v.as_bool()).unwrap_or(true),
            };
            json_result(&api::ai_generate_value(state, &b).await)
        }
        "autotest_ai_diagnose" => {
            let Some(run_id) = as_i64(args, "run_id") else {
                return need("run_id");
            };
            let b = api::DiagnoseIn {
                run_id,
                question: as_str(args, "question").unwrap_or("").to_string(),
            };
            json_result(&api::ai_diagnose_value(state, &b).await)
        }
        other => error_result(format!("tool không tồn tại: {other}")),
    }
}
