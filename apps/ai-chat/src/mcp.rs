//! `ai-chat-mcp` — an MCP server (SSE + JSON-RPC 2.0) exposing the chat
//! platform to the daemon's agents and to ai-office (the support-chat hook):
//! list/create/configure bots, inspect sessions, push a message to a customer,
//! and drive human handoff.

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
            "serverInfo": { "name": "ai-chat-mcp", "version": "1.0.0" }
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
            "name": "chat_list_bots",
            "description": "Danh sách các chatbot trong AI Chat (key, tên, kênh, chính sách MCP/skill, phạm vi kiến thức). Gọi trước khi cấu hình hay giao việc.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "chat_create_bot",
            "description": "Tạo một chatbot mới với tên + lời chào + system prompt. Bot mới tự có kênh Web chat (WebSocket) và một không gian kiến thức riêng ai-chat:<key>.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" },
                "system_prompt": { "type": "string" },
                "greeting": { "type": "string" }
            }, "required": ["name"] }
        },
        {
            "name": "chat_update_bot",
            "description": "Cập nhật chính sách/hồ sơ một bot: system prompt, model, phạm vi kiến thức, allowlist MCP (allowed_mcp), allowed_skills, bật/tắt công cụ (use_tools), tự học kiến thức (auto_ingest), enabled. Bot CHỈ được dùng đúng MCP/skill trong allowlist.",
            "inputSchema": { "type": "object", "properties": {
                "key": { "type": "string" },
                "system_prompt": { "type": "string" },
                "model": { "type": "string" },
                "knowledge_scope": { "type": "string", "enum": ["bot", "session", "user"] },
                "allowed_mcp": { "type": "array", "items": { "type": "string" }, "description": "Tên công cụ đầy đủ, ví dụ mcp__senclaw-browser__browser_navigate hoặc WebSearch" },
                "allowed_skills": { "type": "array", "items": { "type": "string" } },
                "use_tools": { "type": "boolean" },
                "auto_ingest": { "type": "boolean" },
                "enabled": { "type": "boolean" }
            }, "required": ["key"] }
        },
        {
            "name": "chat_list_sessions",
            "description": "Liệt kê các phiên hội thoại gần đây (kèm kênh, tên khách, trạng thái handoff). Truyền 'bot' để lọc theo một bot.",
            "inputSchema": { "type": "object", "properties": {
                "bot": { "type": "string" },
                "limit": { "type": "number" }
            } }
        },
        {
            "name": "chat_get_session",
            "description": "Chi tiết một phiên: thông tin phiên + toàn bộ tin nhắn (khách/bot/nhân viên).",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "chat_send",
            "description": "Gửi một tin nhắn tới khách trong một phiên (đóng vai nhân viên hỗ trợ). Tin sẽ đi qua đúng kênh của phiên (Telegram/Web/Zalo/Facebook) và được lưu vào hội thoại.",
            "inputSchema": { "type": "object", "properties": {
                "sessionId": { "type": "number" },
                "text": { "type": "string" }
            }, "required": ["sessionId", "text"] }
        },
        {
            "name": "chat_handoff",
            "description": "Chuyển trạng thái xử lý một phiên: 'with_operator' (người thật tiếp nhận), 'pending' (chờ tiếp nhận), hoặc 'bot' (trả lại cho bot). Dùng khi AI Office nhận bàn giao một hội thoại CSKH.",
            "inputSchema": { "type": "object", "properties": {
                "sessionId": { "type": "number" },
                "state": { "type": "string", "enum": ["bot", "pending", "with_operator"] }
            }, "required": ["sessionId", "state"] }
        },
        {
            "name": "chat_stats",
            "description": "Thống kê: số bot, kênh đang bật, phiên, tin nhắn, số handoff đang mở, số lần gọi LLM và token ước tính.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "chat_create_issue",
            "description": "Tạo một support ticket (khiếu nại/vấn đề của khách) — bot vẫn tiếp tục hỗ trợ. Có thể gắn vào một phiên (sessionId) hoặc để trống.",
            "inputSchema": { "type": "object", "properties": {
                "sessionId": { "type": "number" },
                "botKey": { "type": "string" },
                "title": { "type": "string" },
                "description": { "type": "string" },
                "priority": { "type": "string", "enum": ["low", "medium", "high", "urgent"] },
                "category": { "type": "string" },
                "sentiment": { "type": "string", "enum": ["positive", "neutral", "negative"] }
            }, "required": ["title"] }
        },
        {
            "name": "chat_list_issues",
            "description": "Liệt kê support ticket, lọc theo status (open/in_progress/resolved/closed), priority, bot, hoặc từ khoá.",
            "inputSchema": { "type": "object", "properties": {
                "status": { "type": "string" },
                "priority": { "type": "string" },
                "bot": { "type": "string" },
                "search": { "type": "string" },
                "limit": { "type": "number" }
            } }
        },
        {
            "name": "chat_update_issue",
            "description": "Cập nhật một ticket: status, priority, category, assignee, resolution_note, title. Đổi status sang resolved/closed sẽ đóng ticket.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" },
                "status": { "type": "string", "enum": ["open", "in_progress", "resolved", "closed"] },
                "priority": { "type": "string", "enum": ["low", "medium", "high", "urgent"] },
                "category": { "type": "string" },
                "assignee": { "type": "string" },
                "resolution_note": { "type": "string" }
            }, "required": ["id"] }
        },
        {
            "name": "chat_analytics",
            "description": "Phân tích support tổng hợp: ticket theo status/priority/category/sentiment, phiên theo kênh, số handoff đang mở, token/LLM.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "chat_analyze_session",
            "description": "Đánh giá chất lượng CSKH của MỘT phiên bằng LLM: trả về sentiment, điểm chất lượng 1-5, đã giải quyết chưa, tóm tắt và gợi ý cải thiện.",
            "inputSchema": { "type": "object", "properties": {
                "sessionId": { "type": "number" }
            }, "required": ["sessionId"] }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let db = &state.db;
    match name {
        "chat_list_bots" => match db.list_bots() {
            Ok(bots) => json_result(json!({ "bots": bots })),
            Err(e) => error_result(e.to_string()),
        },
        "chat_create_bot" => {
            let name = args["name"].as_str().unwrap_or("").trim();
            if name.is_empty() {
                return error_result("thiếu 'name'".into());
            }
            match db.create_bot(
                name,
                args["system_prompt"].as_str().unwrap_or(""),
                args["greeting"].as_str().unwrap_or(""),
            ) {
                Ok(bot) => {
                    let _ = db.create_channel(&bot.key, "websocket", "Web chat", &json!({}));
                    json_result(json!({ "bot": bot }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "chat_update_bot" => {
            let key = args["key"].as_str().unwrap_or("");
            if key.is_empty() {
                return error_result("thiếu 'key'".into());
            }
            let arr = |k: &str| -> Option<Vec<String>> {
                args[k].as_array().map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
            };
            match db.update_bot(
                key,
                None,
                args["system_prompt"].as_str(),
                None,
                args["model"].as_str(),
                args["knowledge_scope"].as_str(),
                arr("allowed_mcp").as_deref(),
                arr("allowed_skills").as_deref(),
                args["use_tools"].as_bool(),
                None,
                args["auto_ingest"].as_bool(),
                args["auto_issue"].as_bool(),
                args["enabled"].as_bool(),
            ) {
                Ok(true) => json_result(json!({ "bot": db.get_bot(key).ok().flatten() })),
                Ok(false) => error_result(format!("không có bot '{key}'")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "chat_list_sessions" => {
            let limit = args["limit"].as_i64().unwrap_or(30).clamp(1, 200);
            match db.list_sessions(args["bot"].as_str(), limit) {
                Ok(sessions) => json_result(json!({ "sessions": sessions })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "chat_get_session" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("thiếu 'id'".into());
            };
            match db.get_session(id) {
                Ok(Some(session)) => {
                    let messages = db.list_messages(id, 200).unwrap_or_default();
                    json_result(json!({ "session": session, "messages": messages }))
                }
                Ok(None) => error_result(format!("không có phiên id={id}")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "chat_send" => {
            let Some(id) = args["sessionId"].as_i64() else {
                return error_result("thiếu 'sessionId'".into());
            };
            let text = args["text"].as_str().unwrap_or("").trim();
            if text.is_empty() {
                return error_result("thiếu 'text'".into());
            }
            let Some(session) = db.get_session(id).ok().flatten() else {
                return error_result(format!("không có phiên id={id}"));
            };
            let _ = db.add_message(id, "operator", text);
            engine::emit(
                &state.events,
                json!({ "type": "message", "sessionId": id, "role": "operator", "content": text }),
            );
            match state.channels.send_to_session(&session, text).await {
                Ok(()) => json_result(json!({ "ok": true })),
                Err(e) => error_result(e),
            }
        }
        "chat_handoff" => {
            let Some(id) = args["sessionId"].as_i64() else {
                return error_result("thiếu 'sessionId'".into());
            };
            let statev = args["state"].as_str().unwrap_or("");
            if !["bot", "pending", "with_operator"].contains(&statev) {
                return error_result("state phải là bot|pending|with_operator".into());
            }
            match db.set_handoff(id, statev) {
                Ok(()) => {
                    engine::emit(
                        &state.events,
                        json!({ "type": "handoff", "sessionId": id, "state": statev }),
                    );
                    json_result(json!({ "ok": true }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "chat_stats" => match db.stats() {
            Ok(v) => json_result(v),
            Err(e) => error_result(e.to_string()),
        },
        "chat_create_issue" => {
            let title = args["title"].as_str().unwrap_or("").trim();
            if title.is_empty() {
                return error_result("thiếu 'title'".into());
            }
            let session_id = args["sessionId"].as_i64();
            let (bot_key, external_id) =
                match session_id.and_then(|id| db.get_session(id).ok().flatten()) {
                    Some(sess) => (sess.bot_key, sess.external_id),
                    None => (
                        args["botKey"].as_str().unwrap_or("").to_string(),
                        String::new(),
                    ),
                };
            match db.create_issue(
                session_id,
                &bot_key,
                &external_id,
                title,
                args["description"].as_str().unwrap_or(""),
                args["priority"].as_str().unwrap_or("medium"),
                args["category"].as_str().unwrap_or(""),
                args["sentiment"].as_str().unwrap_or(""),
                "",
                &[],
            ) {
                Ok(issue) => {
                    engine::emit(
                        &state.events,
                        json!({ "type": "issue", "issueId": issue.id, "title": issue.title }),
                    );
                    json_result(json!({ "issue": issue }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "chat_list_issues" => {
            let limit = args["limit"].as_i64().unwrap_or(30).clamp(1, 200);
            match db.list_issues(
                args["status"].as_str(),
                args["priority"].as_str(),
                args["bot"].as_str(),
                args["search"].as_str(),
                limit,
            ) {
                Ok(issues) => json_result(json!({ "issues": issues })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "chat_update_issue" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("thiếu 'id'".into());
            };
            let patch = crate::db::IssuePatch {
                status: args["status"].as_str().map(str::to_string),
                priority: args["priority"].as_str().map(str::to_string),
                category: args["category"].as_str().map(str::to_string),
                assignee: args["assignee"].as_str().map(str::to_string),
                resolution_note: args["resolution_note"].as_str().map(str::to_string),
                title: None,
            };
            match db.update_issue(id, &patch, "agent") {
                Ok(true) => json_result(json!({ "issue": db.get_issue(id).ok().flatten() })),
                Ok(false) => error_result(format!("không có ticket id={id}")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "chat_analytics" => match db.analytics() {
            Ok(v) => json_result(v),
            Err(e) => error_result(e.to_string()),
        },
        "chat_analyze_session" => {
            let Some(id) = args["sessionId"].as_i64() else {
                return error_result("thiếu 'sessionId'".into());
            };
            match engine::analyze_session(db, id).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }
        other => error_result(format!("không có tool '{other}'")),
    }
}
