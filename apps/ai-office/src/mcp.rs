use crate::api::AppState;
use crate::engine;
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
            "description": "Giao một nhiệm vụ mới cho văn phòng. Trưởng phòng sẽ phân công Nghiên cứu → Nội dung → Phân tích → Kiểm định rồi nộp báo cáo tổng hợp. mode='demo' chạy mô phỏng (không gọi API), mode='live' để các agent xử lý thật qua LLM. Trả về id nhiệm vụ; theo dõi bằng office_get_task.",
            "inputSchema": { "type": "object", "properties": {
                "title": { "type": "string", "description": "Nội dung nhiệm vụ Sếp giao, ví dụ: 'lập kế hoạch marketing ra mắt hệ thống Agent office'" },
                "mode": { "type": "string", "enum": ["demo", "live"], "description": "Chế độ chạy, mặc định 'live'" }
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
            "description": "Đổi tên / vai trò / mô tả nhiệm vụ của một nhân sự ảo (key: truong-phong, nghien-cuu, noi-dung, phan-tich, kiem-dinh).",
            "inputSchema": { "type": "object", "properties": {
                "key": { "type": "string", "description": "Khóa agent, ví dụ 'noi-dung'" },
                "name": { "type": "string", "description": "Tên hiển thị mới" },
                "role": { "type": "string", "description": "Vai trò ngắn gọn mới" },
                "duty": { "type": "string", "description": "Mô tả nhiệm vụ cố định mới" }
            }, "required": ["key"] }
        },
        {
            "name": "office_stats",
            "description": "Sổ kế toán của văn phòng: tổng số nhiệm vụ, số đã hoàn thành, số lần gọi LLM và model gần nhất.",
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
            match db.has_running_task() {
                Ok(true) => {
                    return error_result(
                        "phòng đang xử lý một nhiệm vụ khác — dùng office_status để theo dõi, chờ xong rồi giao tiếp".into(),
                    )
                }
                Ok(false) => {}
                Err(e) => return error_result(e.to_string()),
            }
            let mode = match args["mode"].as_str() {
                Some("demo") => "demo",
                _ => "live",
            };
            db.create_task(&title, mode).map(|task| {
                engine::spawn(db.clone(), task.id);
                json!({ "task": task, "hint": "theo dõi bằng office_get_task, lấy kết quả bằng office_get_report" })
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
            db.update_agent(
                key,
                args["name"].as_str(),
                args["role"].as_str(),
                args["duty"].as_str(),
            )
            .and_then(|found| {
                if found {
                    Ok(json!({ "ok": true }))
                } else {
                    anyhow::bail!("không có agent '{}'", key)
                }
            })
        }
        "office_stats" => db.stats(),
        other => return error_result(format!("không có tool '{}'", other)),
    };
    match out {
        Ok(v) => json_result(v),
        Err(e) => error_result(e.to_string()),
    }
}
