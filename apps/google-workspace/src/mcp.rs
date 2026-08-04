//! MCP server (HTTP/SSE) exposing Google Workspace to SenClaw agents.
//!
//! Ten tools over the same engine the UI uses: settings get/set, Gmail
//! list/read/send, Calendar list/create, Drive list/upload, and a sync that
//! pushes calendar events into Space Calendar. Tool names keep the original
//! `gworkspace_*` prefix so existing skills and transcripts stay valid.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::{run_sync, AppState};

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

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}
fn json_result(v: &Value) -> Value {
    text_result(serde_json::to_string_pretty(v).unwrap_or_default())
}
fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}
fn res_of(r: anyhow::Result<Value>) -> Value {
    match r {
        Ok(v) => json_result(&v),
        Err(e) => error_result(e.to_string()),
    }
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
            "serverInfo": { "name": "google-workspace-mcp", "version": "3.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => {
            Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} }))
        }
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or(json!({}));
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            reply(call_tool(&state, &name, &args).await)
        }
        _ => Json(json!("ok")),
    }
}

pub fn tools_list() -> Value {
    json!([
        {
            "name": "gworkspace_get_settings",
            "description": "Đọc cài đặt Google Workspace đã lưu (secrets bị che) + trạng thái kết nối và lần sync gần nhất. Gọi ĐẦU TIÊN khi cần biết app đã kết nối Google chưa.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "gworkspace_set_settings",
            "description": "Cập nhật cài đặt: clientId/clientSecret (OAuth client), days (cửa sổ sync 1-90), services (mảng gmail/calendar/drive), hoặc accessToken/refreshToken (kết nối bằng token dán tay). Chỉ trường nào truyền vào mới bị ghi đè.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "clientId": { "type": "string", "description": "Google OAuth Client ID." },
                    "clientSecret": { "type": "string", "description": "Google OAuth Client Secret." },
                    "days": { "type": "number", "description": "Cửa sổ sync (ngày), 1-90." },
                    "services": { "type": "array", "items": { "type": "string", "enum": ["gmail", "calendar", "drive"] }, "description": "Các dịch vụ bật sync." },
                    "accessToken": { "type": "string", "description": "Google OAuth access token (ya29.…) để kết nối ngay không cần OAuth flow." },
                    "refreshToken": { "type": "string", "description": "Refresh token (tuỳ chọn, đi cùng accessToken)." }
                }
            }
        },
        {
            "name": "gworkspace_list_emails",
            "description": "Liệt kê email gần nhất trong Gmail (id, subject, from, date, snippet). Hỗ trợ chuỗi tìm kiếm Gmail qua 'q' (vd 'is:unread', 'from:sep@x.com newer_than:7d').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "maxResults": { "type": "number", "description": "Số email tối đa (1-50). Mặc định 10." },
                    "q": { "type": "string", "description": "Gmail search query (tuỳ chọn)." }
                }
            }
        },
        {
            "name": "gworkspace_read_email",
            "description": "Đọc toàn bộ một email theo ID (lấy từ gworkspace_list_emails): headers, body text (đã decode), danh sách file đính kèm.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "string", "description": "Gmail message ID." }
                },
                "required": ["id"]
            }
        },
        {
            "name": "gworkspace_send_email",
            "description": "Gửi email qua Gmail (body là HTML; subject tiếng Việt được encode đúng chuẩn). Xác nhận với người dùng trước khi gửi nếu nội dung do agent tự soạn.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "to": { "type": "string", "description": "Địa chỉ người nhận." },
                    "subject": { "type": "string", "description": "Tiêu đề." },
                    "body": { "type": "string", "description": "Nội dung (HTML hoặc text thuần)." }
                },
                "required": ["to", "subject", "body"]
            }
        },
        {
            "name": "gworkspace_list_events",
            "description": "Liệt kê sự kiện sắp tới trong Google Calendar (lịch primary), sắp theo giờ bắt đầu.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "maxResults": { "type": "number", "description": "Số sự kiện tối đa (1-100). Mặc định 10." },
                    "days": { "type": "number", "description": "Chỉ lấy sự kiện trong N ngày tới (0 = không giới hạn)." }
                }
            }
        },
        {
            "name": "gworkspace_create_event",
            "description": "Tạo sự kiện mới trong Google Calendar (lịch primary). Thời gian nhận RFC3339 ('2026-07-30T15:00:00+07:00'), 'YYYY-MM-DDTHH:MM' (giờ local) hoặc 'YYYY-MM-DD' (cả ngày).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "summary": { "type": "string", "description": "Tên sự kiện." },
                    "description": { "type": "string", "description": "Mô tả (tuỳ chọn)." },
                    "startTime": { "type": "string", "description": "Thời gian bắt đầu." },
                    "endTime": { "type": "string", "description": "Thời gian kết thúc." }
                },
                "required": ["summary", "startTime", "endTime"]
            }
        },
        {
            "name": "gworkspace_list_files",
            "description": "Liệt kê file trên Google Drive, mới sửa đổi trước. Hỗ trợ Drive query qua 'q' (vd \"name contains 'báo cáo'\").",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "maxResults": { "type": "number", "description": "Số file tối đa (1-100). Mặc định 10." },
                    "q": { "type": "string", "description": "Drive search query (tuỳ chọn)." }
                }
            }
        },
        {
            "name": "gworkspace_upload_file",
            "description": "Tải một file văn bản lên Google Drive (nội dung truyền dạng text).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Tên file trên Drive." },
                    "mimeType": { "type": "string", "description": "MIME type. Mặc định text/plain." },
                    "textContent": { "type": "string", "description": "Nội dung file." }
                },
                "required": ["name", "textContent"]
            }
        },
        {
            "name": "gworkspace_sync",
            "description": "Chạy sync ngay: gmail/drive lấy snapshot mới, calendar đẩy sự kiện vào Space Calendar của SenClaw. Mặc định dùng services + days trong cài đặt.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "services": { "type": "array", "items": { "type": "string", "enum": ["gmail", "calendar", "drive"] }, "description": "Ghi đè danh sách dịch vụ." },
                    "days": { "type": "number", "description": "Ghi đè cửa sổ sync (ngày)." }
                }
            }
        }
    ])
}

fn arg_str(args: &Value, key: &str) -> String {
    args.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string()
}

fn arg_u32(args: &Value, key: &str, default: u32) -> u32 {
    args.get(key)
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .unwrap_or(default)
}

pub async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    let g = &state.google;
    let db = &g.db;
    match name {
        "gworkspace_get_settings" => json_result(&json!({
            "settings": db.masked_settings(),
            "lastRun": db.last_run(),
        })),
        "gworkspace_set_settings" => {
            let apply = || -> anyhow::Result<Value> {
                if let Some(v) = args.get("clientId").and_then(|v| v.as_str()) {
                    db.set_setting("client_id", v.trim())?;
                }
                if let Some(v) = args.get("clientSecret").and_then(|v| v.as_str()) {
                    if v != "***" {
                        db.set_setting("client_secret", v.trim())?;
                    }
                }
                if let Some(v) = args.get("days").and_then(|v| v.as_u64()) {
                    db.set_setting("days", &(v.clamp(1, 90)).to_string())?;
                }
                if let Some(v) = args.get("services").and_then(|v| v.as_array()) {
                    let list: Vec<String> = v
                        .iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect();
                    db.set_setting("services", &serde_json::to_string(&list)?)?;
                }
                if let Some(v) = args.get("accessToken").and_then(|v| v.as_str()) {
                    if !v.trim().is_empty() {
                        let mut tokens = db.tokens();
                        tokens.access_token = v.trim().to_string();
                        if let Some(r) = args.get("refreshToken").and_then(|v| v.as_str()) {
                            if !r.trim().is_empty() {
                                tokens.refresh_token = r.trim().to_string();
                            }
                        }
                        tokens.expires_at = 0;
                        db.save_tokens(&tokens)?;
                    }
                }
                Ok(json!({ "settings": db.masked_settings() }))
            };
            res_of(apply())
        }
        "gworkspace_list_emails" => res_of(
            g.list_emails(arg_u32(args, "maxResults", 10), &arg_str(args, "q"))
                .await,
        ),
        "gworkspace_read_email" => {
            let id = arg_str(args, "id");
            if id.is_empty() {
                return error_result("Thiếu 'id' — lấy từ gworkspace_list_emails.".into());
            }
            res_of(g.read_email(&id).await)
        }
        "gworkspace_send_email" => {
            let (to, subject, body) = (
                arg_str(args, "to"),
                arg_str(args, "subject"),
                arg_str(args, "body"),
            );
            if to.is_empty() || subject.is_empty() {
                return error_result("Thiếu 'to' hoặc 'subject'.".into());
            }
            match g.send_email(&to, &subject, &body).await {
                Ok(v) => {
                    let _ = db.add_run("gmail", "completed", &format!("sent to {to}"));
                    json_result(&json!({
                        "sent": true,
                        "id": v["id"],
                        "threadId": v["threadId"],
                    }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "gworkspace_list_events" => res_of(
            g.list_events(arg_u32(args, "maxResults", 10), arg_u32(args, "days", 0))
                .await,
        ),
        "gworkspace_create_event" => {
            let summary = arg_str(args, "summary");
            let (start, end) = (arg_str(args, "startTime"), arg_str(args, "endTime"));
            if summary.is_empty() || start.is_empty() || end.is_empty() {
                return error_result("Thiếu 'summary', 'startTime' hoặc 'endTime'.".into());
            }
            res_of(
                g.create_event(&summary, &arg_str(args, "description"), &start, &end)
                    .await
                    .map(|e| {
                        json!({
                            "created": true,
                            "id": e["id"],
                            "htmlLink": e["htmlLink"],
                            "start": e["start"],
                            "end": e["end"],
                        })
                    }),
            )
        }
        "gworkspace_list_files" => res_of(
            g.list_files(arg_u32(args, "maxResults", 10), &arg_str(args, "q"))
                .await,
        ),
        "gworkspace_upload_file" => {
            let name_arg = arg_str(args, "name");
            if name_arg.is_empty() {
                return error_result("Thiếu 'name'.".into());
            }
            res_of(
                g.upload_file(
                    &name_arg,
                    &arg_str(args, "mimeType"),
                    &arg_str(args, "textContent"),
                )
                .await
                .map(|f| json!({ "uploaded": true, "file": f })),
            )
        }
        "gworkspace_sync" => {
            let services = args
                .get("services")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|s| s.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                })
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| db.services());
            let days = arg_u32(args, "days", db.days());
            json_result(&run_sync(state, services, days).await)
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn test_state() -> Arc<AppState> {
        crate::api::make_state(Arc::new(Db::open_memory().unwrap()), 4310)
    }

    fn text_of(v: &Value) -> String {
        v["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    #[tokio::test]
    async fn every_listed_tool_is_callable() {
        let state = test_state();
        let tools = tools_list();
        let list = tools.as_array().unwrap();
        assert_eq!(list.len(), 10, "tool count changed — update SKILL.md too");
        for tool in list {
            let name = tool["name"].as_str().unwrap();
            let out = call_tool(&state, name, &json!({})).await;
            // Missing args / not connected are real errors, but never "Unknown tool".
            assert!(
                !text_of(&out).starts_with("Unknown tool"),
                "{name} is listed but not implemented"
            );
        }
        let out = call_tool(&state, "gworkspace_nope", &json!({})).await;
        assert!(text_of(&out).starts_with("Unknown tool"));
    }

    #[tokio::test]
    async fn settings_tools_roundtrip_with_masking() {
        let state = test_state();
        let out = call_tool(
            &state,
            "gworkspace_set_settings",
            &json!({
                "clientId": "cid.apps.googleusercontent.com",
                "clientSecret": "topsecret",
                "days": 30,
                "services": ["gmail"],
                "accessToken": "ya29.manual",
            }),
        )
        .await;
        let text = text_of(&out);
        assert!(text.contains("cid.apps.googleusercontent.com"));
        assert!(!text.contains("topsecret"), "secret must be masked");
        assert!(!text.contains("ya29.manual"), "token must be masked");

        let get = call_tool(&state, "gworkspace_get_settings", &json!({})).await;
        let parsed: Value = serde_json::from_str(&text_of(&get)).unwrap();
        assert_eq!(parsed["settings"]["connected"], true);
        assert_eq!(parsed["settings"]["days"], 30);
        assert_eq!(parsed["settings"]["services"], json!(["gmail"]));

        // The "***" mask coming back must not clobber the stored secret.
        let _ = call_tool(
            &state,
            "gworkspace_set_settings",
            &json!({ "clientSecret": "***" }),
        )
        .await;
        assert_eq!(state.google.db.client_secret(), "topsecret");
    }

    #[tokio::test]
    async fn network_tools_fail_cleanly_when_not_connected() {
        let state = test_state();
        for (name, args) in [
            ("gworkspace_list_emails", json!({})),
            ("gworkspace_read_email", json!({ "id": "m1" })),
            (
                "gworkspace_send_email",
                json!({ "to": "a@b.c", "subject": "s", "body": "b" }),
            ),
            ("gworkspace_list_events", json!({})),
            (
                "gworkspace_create_event",
                json!({ "summary": "s", "startTime": "2026-07-30T10:00", "endTime": "2026-07-30T11:00" }),
            ),
            ("gworkspace_list_files", json!({})),
            (
                "gworkspace_upload_file",
                json!({ "name": "a.txt", "textContent": "x" }),
            ),
        ] {
            let out = call_tool(&state, name, &args).await;
            assert_eq!(out["isError"], json!(true), "{name} should error");
            assert!(
                text_of(&out).contains("Chưa kết nối Google"),
                "{name} should explain how to connect, got: {}",
                text_of(&out)
            );
        }
    }

    #[tokio::test]
    async fn missing_required_args_error_without_network() {
        let state = test_state();
        let out = call_tool(&state, "gworkspace_read_email", &json!({})).await;
        assert_eq!(out["isError"], json!(true));
        let out = call_tool(
            &state,
            "gworkspace_create_event",
            &json!({ "summary": "x" }),
        )
        .await;
        assert_eq!(out["isError"], json!(true));
        let out = call_tool(&state, "gworkspace_upload_file", &json!({})).await;
        assert_eq!(out["isError"], json!(true));
    }

    #[tokio::test]
    async fn sync_logs_errors_per_service_when_offline() {
        let state = test_state();
        let out = call_tool(&state, "gworkspace_sync", &json!({ "services": ["gmail"] })).await;
        let parsed: Value = serde_json::from_str(&text_of(&out)).unwrap();
        assert_eq!(parsed["results"]["gmail"]["status"], "error");
        let runs = state.google.db.recent_runs(5);
        assert_eq!(runs[0]["service"], "gmail");
        assert_eq!(runs[0]["status"], "error");
    }
}
