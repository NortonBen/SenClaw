use crate::api::AppState;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

/// GET /api/mcp/sse — opens the SSE channel; first event tells the client
/// where to POST JSON-RPC messages.
pub async fn mcp_sse(
    State(state): State<Arc<AppState>>,
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

/// POST /api/mcp/sse or /api/mcp/message — JSON-RPC 2.0 dispatch.
pub async fn mcp_message(
    State(state): State<Arc<AppState>>,
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
            "serverInfo": { "name": "ai-office-mcp", "version": "1.0.0" }
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

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}

fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
}

fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

fn tools_list() -> Value {
    json!([
        {
            "name": "office_status",
            "description": "Tình hình văn phòng AI ngay lúc này: trạng thái từng agent (đang làm / xong / đi bàn giao) và nhiệm vụ đang chạy (nếu có). Gọi trước khi giao việc mới.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "office_create_task",
            "description": "Giao một nhiệm vụ mới cho văn phòng. Nếu phòng đang bận, nhiệm vụ được XẾP VÀO HÀNG ĐỢI và tự chạy khi xong việc trước (trả về queued=true). Trưởng phòng lập kế hoạch phân công các nhân sự đang hoạt động (nhân sự 'tự nhận nhiệm vụ' luôn có phần việc, nhân sự tăng cường chỉ khi cần); nhân sự nắm skill/sub-agent sẽ dùng công cụ thật (MCP/search/browser). Kiểm định soát chất lượng rồi Trưởng phòng nộp báo cáo tổng hợp. Trả về id; theo dõi bằng office_get_task.",
            "inputSchema": { "type": "object", "properties": {
                "title": { "type": "string", "description": "Nội dung nhiệm vụ Sếp giao, ví dụ: 'lập kế hoạch marketing ra mắt hệ thống Agent office'" }
            }, "required": ["title"] }
        },
        {
            "name": "office_list_tasks",
            "description": "Liệt kê các nhiệm vụ gần đây của văn phòng kèm trạng thái (pending/planning/running/review/done/error) và chế độ chạy.",
            "inputSchema": { "type": "object", "properties": {
                "limit": { "type": "number", "description": "Số nhiệm vụ tối đa, mặc định 20" }
            } }
        },
        {
            "name": "office_get_task",
            "description": "Chi tiết một nhiệm vụ: các phần việc đã phân công cho từng agent, kết quả từng phần và nhật ký bàn giao đầy đủ.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number", "description": "Id nhiệm vụ" }
            }, "required": ["id"] }
        },
        {
            "name": "office_get_report",
            "description": "Lấy BÁO CÁO TỔNG HỢP mà Trưởng phòng đã nộp. Bỏ trống id để lấy báo cáo của nhiệm vụ hoàn thành gần nhất.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number", "description": "Id nhiệm vụ (tùy chọn)" }
            } }
        },
        {
            "name": "office_list_agents",
            "description": "Danh sách nhân sự ảo của văn phòng: tên, vai trò, mô tả nhiệm vụ cố định và trạng thái hiện tại.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "office_update_agent",
            "description": "Cập nhật hồ sơ một nhân sự ảo: tên / vai trò / nhiệm vụ cố định, bật-tắt hoạt động (enabled), chế độ tự nhận nhiệm vụ (auto_assign), và danh sách skill/sub-agent nắm giữ (skills — lấy tên từ office inventory hoặc /api/skills của daemon, sub-agent dùng tiền tố 'persona:').",
            "inputSchema": { "type": "object", "properties": {
                "key": { "type": "string", "description": "Khóa agent, ví dụ 'noi-dung'" },
                "name": { "type": "string", "description": "Tên hiển thị mới" },
                "role": { "type": "string", "description": "Vai trò ngắn gọn mới" },
                "duty": { "type": "string", "description": "Mô tả nhiệm vụ cố định mới" },
                "enabled": { "type": "boolean", "description": "false = tạm nghỉ (không tham gia nhiệm vụ)" },
                "auto_assign": { "type": "boolean", "description": "true = luôn được phân công; false = chỉ khi cần chuyên môn" },
                "skills": { "type": "array", "items": { "type": "string" }, "description": "Skill/sub-agent nắm giữ, ví dụ ['browser', 'persona:researcher']" }
            }, "required": ["key"] }
        },
        {
            "name": "office_add_agent",
            "description": "Tuyển thêm một nhân sự ảo vào văn phòng (tối đa 7 bàn). kind: 'worker' (chuyên môn — mặc định), 'manager'/'qa' chỉ khi chưa có. Nhân sự mới có knowledge space riêng ai-office:<key>.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "Tên hiển thị, ví dụ 'THIẾT KẾ'" },
                "role": { "type": "string", "description": "Vai trò ngắn gọn, ví dụ 'Thiết kế & hình ảnh'" },
                "duty": { "type": "string", "description": "Mô tả nhiệm vụ cố định" },
                "kind": { "type": "string", "enum": ["worker", "manager", "qa"], "description": "Loại nhân sự, mặc định worker" }
            }, "required": ["name"] }
        },
        {
            "name": "office_remove_agent",
            "description": "Cho một nhân sự ảo nghỉ việc (không xoá được Trưởng phòng; phải giữ ít nhất 1 worker; không đổi biên chế khi phòng đang chạy nhiệm vụ).",
            "inputSchema": { "type": "object", "properties": {
                "key": { "type": "string", "description": "Khóa agent, ví dụ 'thiet-ke'" }
            }, "required": ["key"] }
        },
        {
            "name": "office_stats",
            "description": "Sổ kế toán của văn phòng: tổng số nhiệm vụ, số đã hoàn thành, số lần gọi LLM, số token đã dùng (ước tính, vào/ra) và model gần nhất.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let db = &state.db;
    let out = match name {
        "office_status" => db.list_agents().and_then(|agents| {
            let task = db.latest_task()?;
            Ok(json!({ "agents": agents, "latestTask": task }))
        }),
        "office_create_task" => {
            let title = args["title"].as_str().unwrap_or("").trim().to_string();
            if title.is_empty() {
                return error_result("thiếu 'title' — nội dung nhiệm vụ".into());
            }
            match db.list_agents() {
                Ok(agents) if !agents.iter().any(|a| a.kind == "worker" && a.enabled) => {
                    return error_result(
                        "không còn nhân sự chuyên môn nào đang hoạt động — bật lại bằng office_update_agent {enabled: true}".into(),
                    )
                }
                Err(e) => return error_result(e.to_string()),
                _ => {}
            }
            // Luôn xếp hàng đợi; nếu phòng đang bận, nhiệm vụ chạy sau khi xong việc hiện tại.
            let busy = db.has_running_task().unwrap_or(false);
            db.create_task(&title, "live").map(|task| {
                let hint = if busy {
                    "phòng đang bận — nhiệm vụ đã xếp vào hàng đợi, sẽ tự chạy khi xong việc hiện tại"
                } else {
                    "theo dõi bằng office_get_task, lấy kết quả bằng office_get_report"
                };
                json!({ "task": task, "queued": busy, "hint": hint })
            })
        }
        "office_list_tasks" => {
            let limit = args["limit"].as_i64().unwrap_or(20).clamp(1, 200);
            db.list_tasks(limit).map(|tasks| json!({ "tasks": tasks }))
        }
        "office_get_task" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("thiếu 'id' nhiệm vụ".into());
            };
            match db.get_task(id) {
                Ok(Some(task)) => db.list_steps(id).and_then(|steps| {
                    let events = db.list_events(Some(id), 0, 500)?;
                    Ok(json!({ "task": task, "steps": steps, "events": events }))
                }),
                Ok(None) => return error_result(format!("không có nhiệm vụ id={}", id)),
                Err(e) => Err(e),
            }
        }
        "office_get_report" => {
            let task = match args["id"].as_i64() {
                Some(id) => db.get_task(id),
                None => db.latest_task(),
            };
            match task {
                Ok(Some(t)) if !t.report.is_empty() => Ok(json!({
                    "taskId": t.id, "title": t.title, "status": t.status, "report": t.report
                })),
                Ok(Some(t)) => {
                    return error_result(format!(
                        "nhiệm vụ {} ({}) chưa có báo cáo — trạng thái hiện tại: {}",
                        t.id, t.title, t.status
                    ))
                }
                Ok(None) => return error_result("chưa có nhiệm vụ nào".into()),
                Err(e) => Err(e),
            }
        }
        "office_list_agents" => db.list_agents().map(|agents| json!({ "agents": agents })),
        "office_update_agent" => {
            let key = args["key"].as_str().unwrap_or("");
            if key.is_empty() {
                return error_result("thiếu 'key' của agent".into());
            }
            let skills: Option<Vec<String>> = args["skills"].as_array().map(|arr| {
                arr.iter()
                    .filter_map(|s| s.as_str().map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .collect()
            });
            if args["enabled"].as_bool() == Some(false) {
                match db.has_running_task() {
                    Ok(true) => {
                        return error_result(
                            "phòng đang xử lý nhiệm vụ — chờ xong rồi tạm dừng nhân sự".into(),
                        )
                    }
                    Err(e) => return error_result(e.to_string()),
                    _ => {}
                }
                match db.list_agents() {
                    Ok(agents) => {
                        if let Some(a) = agents.iter().find(|a| a.key == key) {
                            if a.kind == "manager" {
                                return error_result("không thể tạm dừng Trưởng phòng".into());
                            }
                            if a.kind == "worker"
                                && agents.iter().filter(|x| x.kind == "worker" && x.enabled).count() <= 1
                            {
                                return error_result(
                                    "phòng cần ít nhất một nhân sự chuyên môn đang hoạt động".into(),
                                );
                            }
                        }
                    }
                    Err(e) => return error_result(e.to_string()),
                }
            }
            db.update_agent(
                key,
                args["name"].as_str(),
                args["role"].as_str(),
                args["duty"].as_str(),
                args["enabled"].as_bool(),
                args["auto_assign"].as_bool(),
                skills.as_deref(),
            )
            .and_then(|found| {
                if found {
                    Ok(json!({ "ok": true }))
                } else {
                    anyhow::bail!("không có agent '{}'", key)
                }
            })
        }
        "office_add_agent" => {
            let name = args["name"].as_str().unwrap_or("").trim().to_string();
            if name.is_empty() {
                return error_result("thiếu 'name' của nhân sự".into());
            }
            let kind = match args["kind"].as_str() {
                Some("manager") => "manager",
                Some("qa") => "qa",
                _ => "worker",
            };
            match db.list_agents() {
                Ok(agents) if kind != "worker" && agents.iter().any(|a| a.kind == kind) => {
                    return error_result(format!("phòng đã có một nhân sự giữ vai trò '{}'", kind))
                }
                Err(e) => return error_result(e.to_string()),
                _ => {}
            }
            db.add_agent(
                &name,
                args["role"].as_str().unwrap_or(""),
                args["duty"].as_str().unwrap_or(""),
                kind,
            )
            .map(|agent| json!({ "agent": agent }))
        }
        "office_remove_agent" => {
            let key = args["key"].as_str().unwrap_or("");
            if key.is_empty() {
                return error_result("thiếu 'key' của nhân sự".into());
            }
            match db.has_running_task() {
                Ok(true) => {
                    return error_result("phòng đang chạy nhiệm vụ — chờ xong rồi thay đổi biên chế".into())
                }
                Err(e) => return error_result(e.to_string()),
                _ => {}
            }
            match db.list_agents() {
                Ok(agents) => {
                    let Some(agent) = agents.iter().find(|a| a.key == key) else {
                        return error_result(format!("không có agent '{}'", key));
                    };
                    if agent.kind == "manager" {
                        return error_result("không thể xoá Trưởng phòng".into());
                    }
                    if agent.kind == "worker"
                        && agents.iter().filter(|a| a.kind == "worker").count() <= 1
                    {
                        return error_result("phòng cần ít nhất một nhân sự chuyên môn (worker)".into());
                    }
                }
                Err(e) => return error_result(e.to_string()),
            }
            db.delete_agent(key).map(|_| json!({ "ok": true }))
        }
        "office_stats" => db.stats(),
        other => return error_result(format!("không có tool '{}'", other)),
    };
    match out {
        Ok(v) => json_result(v),
        Err(e) => error_result(e.to_string()),
    }
}
