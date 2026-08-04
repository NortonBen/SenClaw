//! `tiktok-mcp` — hand-rolled JSON-RPC MCP over HTTP + SSE.
//! Mirrors apps/zeach/src/mcp.rs so the daemon's auto-registration picks it up.
//! Exposes the shared "drive TikTok activity" surface to other agents/apps.

use crate::api::AppState;
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::Stream;
use serde_json::{json, Value};
use std::convert::Infallible;

pub const SERVER_NAME: &str = "tiktok-mcp";

#[derive(serde::Deserialize)]
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

fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
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
            "serverInfo": { "name": SERVER_NAME, "version": "1.0.0" }
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
        _ => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
    }
}

fn tools_list() -> Value {
    json!([
        {
            "name": "tiktok_list_accounts",
            "description": "Liệt kê các account TikTok đã cấu hình (id, username). Không trả mật khẩu.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "tiktok_list_flows",
            "description": "Liệt kê các flow automation đã lưu (id, name, số bước).",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "tiktok_run_flow",
            "description": "Khởi chạy một flow trên một account. Trả về run_id để theo dõi.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "account_id": { "type": "string" },
                    "flow_id": { "type": "string" },
                    "params": { "type": "object", "additionalProperties": { "type": "string" } }
                },
                "required": ["account_id", "flow_id"]
            }
        },
        {
            "name": "tiktok_run_status",
            "description": "Trạng thái + log của một run theo run_id.",
            "inputSchema": {
                "type": "object",
                "properties": { "run_id": { "type": "string" } },
                "required": ["run_id"]
            }
        },
        {
            "name": "tiktok_generate_flow",
            "description": "Sinh flow bằng AI từ mục tiêu ngôn ngữ tự nhiên + catalog action (paletteId).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "prompt": { "type": "string" },
                    "actions_catalog": { "type": "array" },
                    "account_id": { "type": "string" },
                    "page_url": { "type": "string" }
                },
                "required": ["prompt", "actions_catalog"]
            }
        }
    ])
}

pub async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    match name {
        "tiktok_list_accounts" => {
            let list: Vec<Value> = state
                .db
                .list_accounts()
                .into_iter()
                .map(|a| json!({ "id": a.id, "username": a.username }))
                .collect();
            json_result(json!({ "accounts": list }))
        }
        "tiktok_list_flows" => {
            let list: Vec<Value> = state
                .db
                .list_flows()
                .into_iter()
                .map(|f| json!({ "id": f.id, "name": f.name, "steps": f.actions.len() }))
                .collect();
            json_result(json!({ "flows": list }))
        }
        "tiktok_run_flow" => {
            let account_id = args["account_id"].as_str().unwrap_or("");
            let flow_id = args["flow_id"].as_str().unwrap_or("");
            let params = args
                .get("params")
                .and_then(|v| serde_json::from_value::<crate::domain::StrMap>(v.clone()).ok());
            let account = match state.runs.find_account(account_id) {
                Ok(a) => a,
                Err(_) => return error_result("account not found".into()),
            };
            if state.db.get_flow(flow_id).is_err() {
                return error_result("flow not found".into());
            }
            match state.runs.start_flow_run(account, flow_id, "", params) {
                Ok(run) => json_result(json!({ "run_id": run.id, "status": run.status })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "tiktok_run_status" => {
            let run_id = args["run_id"].as_str().unwrap_or("");
            match state.db.get_run(run_id) {
                Ok(r) => json_result(json!({
                    "id": r.id, "status": r.status, "account_id": r.account_id,
                    "flow_id": r.flow_id, "logs": r.logs, "started_at": r.started_at, "ended_at": r.ended_at
                })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "tiktok_generate_flow" => {
            let prompt = args["prompt"].as_str().unwrap_or("");
            let catalog: Vec<crate::ai::FlowGenCatalogItem> = args
                .get("actions_catalog")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            let page_url = args["page_url"]
                .as_str()
                .unwrap_or("https://www.tiktok.com/");
            let ctx = format!("Trang mục tiêu: {page_url}\n");
            match crate::ai::generate_flow_from_catalog(&state.bridge, prompt, &catalog, &ctx).await
            {
                Ok(out) => json_result(serde_json::to_value(out).unwrap_or_default()),
                Err(e) => error_result(e.to_string()),
            }
        }
        _ => error_result(format!("unknown tool: {name}")),
    }
}
