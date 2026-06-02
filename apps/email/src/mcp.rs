use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::AppState;
use crate::{mailer, store};

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
fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
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
            "serverInfo": { "name": "email-mcp", "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or_default();
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params["arguments"].clone();
            let result = call_tool(&state, &name, &args).await;
            reply(result)
        }
        _ => Json(json!("ok")),
    }
}

fn tools_list() -> Value {
    json!([
        {
            "name": "email_inbox",
            "description": "List recent inbox emails (cached). Use account_id to filter a specific account.",
            "inputSchema": { "type": "object", "properties": {
                "account_id": { "type": "string" },
                "limit": { "type": "number" }
            }}
        },
        {
            "name": "email_read",
            "description": "Read the full content of an email by message_id.",
            "inputSchema": { "type": "object", "properties": {
                "message_id": { "type": "string" }
            }, "required": ["message_id"] }
        },
        {
            "name": "email_compose",
            "description": "Compose and send an email via SMTP. Draft the body carefully and confirm with the user first.",
            "inputSchema": { "type": "object", "properties": {
                "to": { "type": "string" },
                "subject": { "type": "string" },
                "body": { "type": "string" },
                "account_id": { "type": "string" }
            }, "required": ["to", "subject", "body"] }
        },
        {
            "name": "email_search",
            "description": "Search cached emails by keyword (subject or body).",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string" },
                "account_id": { "type": "string" },
                "limit": { "type": "number" }
            }, "required": ["query"] }
        },
        {
            "name": "email_summary",
            "description": "Return an email body plus an instruction to summarize it (key points, action items, sentiment).",
            "inputSchema": { "type": "object", "properties": {
                "message_id": { "type": "string" }
            }, "required": ["message_id"] }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    match name {
        "email_inbox" => {
            let account_id = args["account_id"].as_str().map(|s| s.to_string());
            let limit = args["limit"].as_u64().unwrap_or(20) as u32;
            match store::inbox(&state.db, account_id.as_deref(), limit) {
                Ok(rows) => text_result(serde_json::to_string_pretty(&rows).unwrap_or_default()),
                Err(e) => error_result(format!("Inbox failed: {e}")),
            }
        }
        "email_read" => {
            let id = args["message_id"].as_str().unwrap_or("");
            match store::read_msg(&state.db, id) {
                Ok(v) => text_result(serde_json::to_string_pretty(&v).unwrap_or_default()),
                Err(e) => error_result(format!("Email not found: {e}")),
            }
        }
        "email_search" => {
            let query = args["query"].as_str().unwrap_or("");
            let account_id = args["account_id"].as_str().map(|s| s.to_string());
            let limit = args["limit"].as_u64().unwrap_or(10) as u32;
            match store::search(&state.db, query, account_id.as_deref(), limit) {
                Ok(rows) => text_result(serde_json::to_string_pretty(&rows).unwrap_or_default()),
                Err(e) => error_result(format!("Email search failed: {e}")),
            }
        }
        "email_summary" => {
            let id = args["message_id"].as_str().unwrap_or("");
            match store::read_msg(&state.db, id) {
                Ok(v) => {
                    let body = v["body_text"].as_str().unwrap_or("(no body)");
                    let preview = &body[..body.len().min(2000)];
                    text_result(
                        json!({
                            "subject": v["subject"],
                            "from": v["from"],
                            "date": v["date"],
                            "body_preview": preview,
                            "instruction": "Summarize the above email in Vietnamese: key points, action items, sentiment.",
                        })
                        .to_string(),
                    )
                }
                Err(e) => error_result(format!("Email not found: {e}")),
            }
        }
        "email_compose" => {
            let to = args["to"].as_str().unwrap_or("").to_string();
            let subject = args["subject"].as_str().unwrap_or("").to_string();
            let body = args["body"].as_str().unwrap_or("").to_string();
            let account_id = args["account_id"].as_str().map(|s| s.to_string());

            let acct = match store::account_secret(&state.db, account_id.as_deref()) {
                Ok(a) => a,
                Err(e) => return error_result(format!("No email account configured. Add one first: {e}")),
            };
            let from = acct.email.clone();
            let send_acct = acct.clone();
            let (sto, ssub, sbody) = (to.clone(), subject.clone(), body.clone());
            let send = tokio::task::spawn_blocking(move || {
                mailer::send_smtp(&send_acct, &send_acct.email, &sto, &ssub, &sbody)
            })
            .await;
            match send {
                Ok(Ok(())) => {
                    let msg_id = store::record_sent(&state.db, &acct.id, &from, &to, &subject, &body)
                        .unwrap_or_default();
                    text_result(
                        json!({ "success": true, "message_id": msg_id, "to": to }).to_string(),
                    )
                }
                Ok(Err(e)) => error_result(format!("Send failed: {e}")),
                Err(e) => error_result(format!("Send task failed: {e}")),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
