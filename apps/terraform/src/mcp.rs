//! MCP server (HTTP + SSE) cho agent SenClaw điều khiển Terraform.
//! Tool prefix `tf_` theo convention đặt tên SenClaw; mọi tool gọi CHUNG các
//! helper `crate::api::*_value` với REST UI — agent và người thấy hành vi y
//! hệt nhau. Các tool chạy lệnh (`tf_plan`, `tf_apply`…) mặc định ĐỢI kết quả
//! và trả đuôi console; run dài quá thì trả `done=false` để poll `tf_run_get`.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::convert::Infallible;
use std::time::Duration;

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
/// {ok:false} từ helper REST → isError để agent thấy rõ.
fn wrap(v: Value) -> Value {
    if v["ok"] == false {
        error_result(v["error"].as_str().unwrap_or("lỗi không rõ").to_string())
    } else {
        json_result(&v)
    }
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
            "serverInfo": { "name": "terraform-mcp", "version": "1.0.0" }
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

fn ws_id_schema() -> Value {
    json!({ "type": "object", "properties": {
        "workspace_id": { "type": "number" }
    }, "required": ["workspace_id"] })
}

fn run_cmd_schema(needs_confirm: bool) -> Value {
    let mut props = json!({
        "workspace_id": { "type": "number" },
        "var_file": { "type": "string", "description": "File .tfvars dùng cho lệnh (mặc định: var_file đã chọn của workspace)." },
        "wait": { "type": "boolean", "description": "Mặc định true — đợi chạy xong và trả đuôi console." },
        "timeout_seconds": { "type": "number", "description": "Trần đợi khi wait=true (mặc định 600, tối đa 1800)." }
    });
    let mut required = vec!["workspace_id"];
    if needs_confirm {
        props["confirm"] = json!({ "type": "boolean", "description": "BẮT BUỘC true — lệnh này thay đổi hạ tầng thật." });
        required.push("confirm");
    }
    json!({ "type": "object", "properties": props, "required": required })
}

fn tools_list() -> Value {
    json!([
        {
            "name": "tf_status",
            "description": "Trạng thái nhanh app Terraform: số workspace, run đang chạy, CLI đã có chưa (path + version).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "tf_cli_status",
            "description": "Kiểm tra Terraform CLI: tìm theo settings → bản app tự cài → PATH. Trả path/version/source, hoặc found=false kèm cách cài.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "tf_cli_install",
            "description": "Cài Terraform CLI hộ user (tải từ releases.hashicorp.com về ~/.senclaw/apps/terraform/bin — hỗ trợ macOS/Linux/Windows, arm64/amd64). Chạy nền, trả run_id; theo dõi bằng tf_run_get.",
            "inputSchema": { "type": "object", "properties": {
                "version": { "type": "string", "description": "Version cụ thể (vd 1.10.5). Bỏ trống = bản mới nhất theo checkpoint API." }
            } }
        },
        {
            "name": "tf_workspace_add",
            "description": "Thêm workspace Terraform. source=folder: dùng thư mục local sẵn có (path tuyệt đối). source=git: clone repo về thư mục app quản lý (chạy nền, trả run_id clone — đợi status workspace thành ready).",
            "inputSchema": { "type": "object", "properties": {
                "source":   { "type": "string", "enum": ["folder", "git"] },
                "name":     { "type": "string", "description": "Tên hiển thị (mặc định: tên thư mục/repo)." },
                "path":     { "type": "string", "description": "source=folder: đường dẫn tuyệt đối thư mục chứa *.tf." },
                "repo_url": { "type": "string", "description": "source=git: https://… hoặc git@host:path.git" },
                "branch":   { "type": "string", "description": "source=git: nhánh muốn clone (mặc định nhánh chính của repo)." },
                "subdir":   { "type": "string", "description": "Root Terraform TRONG repo (vd terraform hay infra/prod) khi *.tf không nằm ở gốc. Trống = gốc repo." }
            }, "required": ["source"] }
        },
        {
            "name": "tf_workspace_list",
            "description": "Liệt kê mọi workspace: nguồn (folder/git), thư mục, trạng thái, var_file đã chọn.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "tf_workspace_get",
            "description": "Chi tiết workspace: thông tin git (branch/commit/dirty), danh sách file tfvars, run đang chạy, đã init chưa.",
            "inputSchema": ws_id_schema()
        },
        {
            "name": "tf_workspace_set",
            "description": "Sửa workspace (patch): name, var_file (file tfvars mặc định cho plan/apply), auto_sync (git pull tự động trước plan/apply/destroy), subdir (root Terraform trong repo — dò ứng viên bằng tf_workspace_get, '' = gốc).",
            "inputSchema": { "type": "object", "properties": {
                "workspace_id": { "type": "number" },
                "name":      { "type": "string" },
                "var_file":  { "type": "string" },
                "auto_sync": { "type": "boolean" },
                "subdir":    { "type": "string" }
            }, "required": ["workspace_id"] }
        },
        {
            "name": "tf_workspace_delete",
            "description": "Xoá workspace. Nguồn git: xoá luôn bản clone app quản lý. Nguồn folder: KHÔNG đụng thư mục của user.",
            "inputSchema": { "type": "object", "properties": {
                "workspace_id": { "type": "number" },
                "confirm": { "type": "boolean", "description": "BẮT BUỘC true." }
            }, "required": ["workspace_id", "confirm"] }
        },
        {
            "name": "tf_sync",
            "description": "git pull --ff-only cho workspace nguồn git (đồng bộ code mới nhất). Mặc định đợi xong và trả kết quả.",
            "inputSchema": { "type": "object", "properties": {
                "workspace_id": { "type": "number" },
                "wait": { "type": "boolean", "description": "Mặc định true." }
            }, "required": ["workspace_id"] }
        },
        {
            "name": "tf_variables",
            "description": "Đọc mọi block variable trong *.tf của workspace (tên/type/description/default/sensitive) — đúng dữ liệu UI dùng render form Apply. Kèm danh sách file tfvars.",
            "inputSchema": ws_id_schema()
        },
        {
            "name": "tf_tfvars_get",
            "description": "Đọc giá trị một file .tfvars (mặc định: file đã chọn của workspace) → map biến→giá trị JSON.",
            "inputSchema": { "type": "object", "properties": {
                "workspace_id": { "type": "number" },
                "file": { "type": "string", "description": "vd prod.tfvars. Bỏ trống = var_file đã chọn." }
            }, "required": ["workspace_id"] }
        },
        {
            "name": "tf_tfvars_set",
            "description": "Ghi giá trị biến vào file .tfvars (tạo mới nếu chưa có) rồi đặt nó làm var_file của workspace. Mặc định MERGE đè lên giá trị sẵn có; replace=true thay cả file. Giá trị là JSON (string/number/bool/list/map).",
            "inputSchema": { "type": "object", "properties": {
                "workspace_id": { "type": "number" },
                "file":    { "type": "string", "description": "vd prod.tfvars" },
                "values":  { "type": "object", "description": "map tên biến → giá trị JSON" },
                "replace": { "type": "boolean" }
            }, "required": ["workspace_id", "file", "values"] }
        },
        {
            "name": "tf_init",
            "description": "terraform init cho workspace (tải provider/module). Đợi xong, trả đuôi console.",
            "inputSchema": run_cmd_schema(false)
        },
        {
            "name": "tf_validate",
            "description": "terraform validate — kiểm cú pháp/cấu hình. Tự init trước nếu chưa.",
            "inputSchema": run_cmd_schema(false)
        },
        {
            "name": "tf_plan",
            "description": "terraform plan với var-file đã chọn. Workspace git + auto_sync sẽ git pull trước. Tự init nếu chưa. Trả đuôi console để đọc plan.",
            "inputSchema": run_cmd_schema(false)
        },
        {
            "name": "tf_apply",
            "description": "terraform apply -auto-approve (THAY ĐỔI HẠ TẦNG THẬT — cần confirm=true, chỉ gọi khi user đã đồng ý rõ ràng). Workspace git + auto_sync sẽ git pull trước; dùng var-file đã chọn.",
            "inputSchema": run_cmd_schema(true)
        },
        {
            "name": "tf_destroy",
            "description": "terraform destroy -auto-approve (XOÁ HẠ TẦNG THẬT — cần confirm=true, chỉ gọi khi user đã đồng ý rõ ràng).",
            "inputSchema": run_cmd_schema(true)
        },
        {
            "name": "tf_output",
            "description": "terraform output — đọc output values của state hiện tại.",
            "inputSchema": run_cmd_schema(false)
        },
        {
            "name": "tf_open_dir",
            "description": "Mở thư mục workspace (bản clone nếu nguồn git) trong Finder/Explorer trên máy user.",
            "inputSchema": ws_id_schema()
        },
        {
            "name": "tf_run_list",
            "description": "Lịch sử run (mọi workspace hoặc lọc theo workspace_id): kind/status/exit_code/thời gian.",
            "inputSchema": { "type": "object", "properties": {
                "workspace_id": { "type": "number" },
                "limit": { "type": "number" }
            } }
        },
        {
            "name": "tf_run_get",
            "description": "Đọc console một run: trạng thái + các dòng output (after=seq để đọc tiếp phần mới).",
            "inputSchema": { "type": "object", "properties": {
                "run_id": { "type": "number" },
                "after":  { "type": "number" },
                "limit":  { "type": "number" }
            }, "required": ["run_id"] }
        },
        {
            "name": "tf_run_cancel",
            "description": "Huỷ run đang chạy (kill tiến trình terraform/git).",
            "inputSchema": { "type": "object", "properties": {
                "run_id": { "type": "number" }
            }, "required": ["run_id"] }
        },
        {
            "name": "tf_ai_explain",
            "description": "AI đọc console một run và giải thích lỗi / tóm tắt thay đổi bằng tiếng Việt (bridge LLM SenClaw).",
            "inputSchema": { "type": "object", "properties": {
                "run_id": { "type": "number" }
            }, "required": ["run_id"] }
        }
    ])
}

fn ws_id(args: &Value) -> Result<i64, Value> {
    args["workspace_id"]
        .as_i64()
        .ok_or_else(|| error_result("thiếu workspace_id".into()))
}

/// Chạy lệnh terraform rồi (mặc định) đợi xong, trả run + đuôi console.
async fn run_and_wait(state: &AppState, ws: i64, command: &str, args: &Value) -> Value {
    let req = api::RunReq {
        command: command.to_string(),
        var_file: args["var_file"].as_str().map(String::from),
        confirm: args["confirm"].as_bool().unwrap_or(false),
    };
    let started = api::run_value(state, ws, &req).await;
    if started["ok"] == false {
        return wrap(started);
    }
    let run_id = started["run_id"].as_i64().unwrap_or(0);
    if args["wait"].as_bool() == Some(false) {
        return json_result(&started);
    }
    let timeout = args["timeout_seconds"].as_u64().unwrap_or(600).clamp(10, 1800);
    match state.runner.wait_run(run_id, Duration::from_secs(timeout)).await {
        Ok((run, done)) => {
            let tail = state.db.run_tail(run_id, 150).unwrap_or_default();
            json_result(&json!({
                "ok": true,
                "run": run,
                "done": done,
                "hint": if done { Value::Null } else {
                    json!(format!("run #{run_id} vẫn chạy — poll tf_run_get thêm"))
                },
                "console_tail": tail,
            }))
        }
        Err(e) => error_result(e.to_string()),
    }
}

pub async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    match name {
        "tf_status" => {
            let mut v = api::status_value(state);
            v["cli"] = api::cli_value(state).await;
            json_result(&v)
        }
        "tf_cli_status" => json_result(&api::cli_value(state).await),
        "tf_cli_install" => wrap(api::cli_install_value(
            state,
            args["version"].as_str().map(String::from),
        )),
        "tf_workspace_add" => {
            let req = api::WsAddReq {
                name: args["name"].as_str().map(String::from),
                source: args["source"].as_str().unwrap_or("").to_string(),
                path: args["path"].as_str().map(String::from),
                repo_url: args["repo_url"].as_str().map(String::from),
                branch: args["branch"].as_str().map(String::from),
                subdir: args["subdir"].as_str().map(String::from),
            };
            wrap(api::ws_add_value(state, &req))
        }
        "tf_workspace_list" => wrap(api::ws_list_value(state)),
        "tf_workspace_get" => match ws_id(args) {
            Ok(id) => wrap(api::ws_get_value(state, id).await),
            Err(e) => e,
        },
        "tf_workspace_set" => match ws_id(args) {
            Ok(id) => {
                let req = api::WsPatchReq {
                    name: args["name"].as_str().map(String::from),
                    var_file: args["var_file"].as_str().map(String::from),
                    auto_sync: args["auto_sync"].as_bool(),
                    subdir: args["subdir"].as_str().map(String::from),
                };
                wrap(api::ws_patch_value(state, id, &req))
            }
            Err(e) => e,
        },
        "tf_workspace_delete" => match ws_id(args) {
            Ok(id) => wrap(api::ws_delete_value(
                state,
                id,
                args["confirm"].as_bool().unwrap_or(false),
            )),
            Err(e) => e,
        },
        "tf_sync" => match ws_id(args) {
            Ok(id) => {
                let started = api::ws_sync_value(state, id);
                if started["ok"] == false || args["wait"].as_bool() == Some(false) {
                    return wrap(started);
                }
                let run_id = started["run_id"].as_i64().unwrap_or(0);
                match state.runner.wait_run(run_id, Duration::from_secs(300)).await {
                    Ok((run, done)) => json_result(&json!({
                        "ok": true,
                        "run": run,
                        "done": done,
                        "console_tail": state.db.run_tail(run_id, 60).unwrap_or_default(),
                    })),
                    Err(e) => error_result(e.to_string()),
                }
            }
            Err(e) => e,
        },
        "tf_variables" => match ws_id(args) {
            Ok(id) => wrap(api::vars_value(state, id)),
            Err(e) => e,
        },
        "tf_tfvars_get" => match ws_id(args) {
            Ok(id) => wrap(api::tfvars_get_value(
                state,
                id,
                args["file"].as_str().map(String::from),
            )),
            Err(e) => e,
        },
        "tf_tfvars_set" => match ws_id(args) {
            Ok(id) => {
                let Some(file) = args["file"].as_str() else {
                    return error_result("thiếu file".into());
                };
                let values: Map<String, Value> = args["values"]
                    .as_object()
                    .cloned()
                    .unwrap_or_default();
                if values.is_empty() {
                    return error_result("values rỗng — truyền map biến→giá trị".into());
                }
                let req = api::TfvarsSetReq {
                    file: file.to_string(),
                    values,
                    replace: args["replace"].as_bool().unwrap_or(false),
                };
                wrap(api::tfvars_set_value(state, id, &req))
            }
            Err(e) => e,
        },
        "tf_init" | "tf_validate" | "tf_plan" | "tf_apply" | "tf_destroy" | "tf_output" => {
            match ws_id(args) {
                Ok(id) => {
                    let command = name.trim_start_matches("tf_");
                    run_and_wait(state, id, command, args).await
                }
                Err(e) => e,
            }
        }
        "tf_run_list" => wrap(api::runs_value(
            state,
            args["workspace_id"].as_i64(),
            args["limit"].as_i64().unwrap_or(30),
        )),
        "tf_run_get" => {
            let Some(id) = args["run_id"].as_i64() else {
                return error_result("thiếu run_id".into());
            };
            wrap(api::run_get_value(
                state,
                id,
                args["after"].as_i64().unwrap_or(0),
                args["limit"].as_i64().unwrap_or(500),
            ))
        }
        "tf_open_dir" => match ws_id(args) {
            Ok(id) => wrap(api::open_dir_value(state, id)),
            Err(e) => e,
        },
        "tf_run_cancel" => {
            let Some(id) = args["run_id"].as_i64() else {
                return error_result("thiếu run_id".into());
            };
            wrap(api::run_cancel_value(state, id))
        }
        "tf_ai_explain" => {
            let Some(id) = args["run_id"].as_i64() else {
                return error_result("thiếu run_id".into());
            };
            wrap(api::explain_value(state, id).await)
        }
        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_have_canonical_prefix_and_schema() {
        let tools = tools_list();
        let arr = tools.as_array().unwrap();
        assert_eq!(arr.len(), 23);
        for t in arr {
            let name = t["name"].as_str().unwrap();
            assert!(name.starts_with("tf_"), "{name} phải có prefix tf_");
            assert!(t["inputSchema"]["type"] == "object", "{name} thiếu schema");
            assert!(!t["description"].as_str().unwrap().is_empty());
        }
        // apply/destroy bắt buộc khai confirm trong schema.
        for danger in ["tf_apply", "tf_destroy"] {
            let t = arr.iter().find(|t| t["name"] == danger).unwrap();
            let req = t["inputSchema"]["required"].as_array().unwrap();
            assert!(req.iter().any(|r| r == "confirm"), "{danger} thiếu required confirm");
        }
    }
}
