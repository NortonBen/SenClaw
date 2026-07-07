//! MCP server (JSON-RPC over SSE + POST), exposing the browser to AI agents.
//! Manual protocol impl mirroring the other SenClaw App Spaces.

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
fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
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
            "serverInfo": { "name": "mini-browser-mcp", "version": "1.0.0" }
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

fn tools_list() -> Value {
    json!([
        {
            "name": "browser_navigate",
            "description": "Navigate the active tab to a URL. A bare domain gets https://; a phrase becomes a Google search. Returns the resulting url and title.",
            "inputSchema": { "type": "object", "properties": { "url": { "type": "string" } }, "required": ["url"] }
        },
        {
            "name": "browser_snapshot",
            "description": "Capture the current page: url, title, a text summary, and a numbered list of interactive elements (each with an idx to use in browser_click / browser_type). ALWAYS snapshot before clicking or typing by index.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "browser_click",
            "description": "Click the interactive element with the given idx (from the latest browser_snapshot). Uses a human-like mouse movement + click.",
            "inputSchema": { "type": "object", "properties": { "index": { "type": "number" } }, "required": ["index"] }
        },
        {
            "name": "browser_type",
            "description": "Focus the element at idx and type text into it, one key at a time. Set submit=true to press Enter afterwards (e.g. to run a search).",
            "inputSchema": { "type": "object", "properties": {
                "index": { "type": "number" }, "text": { "type": "string" }, "submit": { "type": "boolean" }
            }, "required": ["index", "text"] }
        },
        {
            "name": "browser_press_key",
            "description": "Press a single key: Enter, Tab, Escape, Backspace, Delete, ArrowUp/Down/Left/Right, Home, End, PageUp, PageDown.",
            "inputSchema": { "type": "object", "properties": { "key": { "type": "string" } }, "required": ["key"] }
        },
        {
            "name": "browser_scroll",
            "description": "Scroll the page. direction: up|down (amount is pixels, default 600).",
            "inputSchema": { "type": "object", "properties": {
                "direction": { "type": "string", "enum": ["up", "down"] }, "amount": { "type": "number" }
            } }
        },
        {
            "name": "browser_back",
            "description": "Go back in the active tab's history.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "browser_forward",
            "description": "Go forward in the active tab's history.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "browser_reload",
            "description": "Reload the active tab.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "browser_get_info",
            "description": "Get the active tab's current url and title.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "browser_extract_text",
            "description": "Extract the visible text of the page (or of a CSS selector if given).",
            "inputSchema": { "type": "object", "properties": { "selector": { "type": "string" } } }
        },
        {
            "name": "browser_extract_links",
            "description": "List all links on the page as {href, text}.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "browser_execute_js",
            "description": "Run JavaScript in the page and return its (JSON-serializable) result. The snippet body is wrapped in a function — use 'return' to yield a value.",
            "inputSchema": { "type": "object", "properties": { "script": { "type": "string" } }, "required": ["script"] }
        },
        {
            "name": "browser_new_tab",
            "description": "Open a new tab (optionally at a url) and make it active.",
            "inputSchema": { "type": "object", "properties": { "url": { "type": "string" } } }
        },
        {
            "name": "browser_list_tabs",
            "description": "List open tabs (index, url, title, active).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "browser_switch_tab",
            "description": "Make the tab at the given index active.",
            "inputSchema": { "type": "object", "properties": { "index": { "type": "number" } }, "required": ["index"] }
        },
        {
            "name": "browser_close_tab",
            "description": "Close the tab at the given index (cannot close the last tab).",
            "inputSchema": { "type": "object", "properties": { "index": { "type": "number" } }, "required": ["index"] }
        },
        {
            "name": "browser_act",
            "description": "Autonomously pursue a natural-language goal on the live page (e.g. 'search for X and open the first result', 'fill the login form'). Runs an observe→decide→act loop for up to max_steps and returns a log of what it did. Best for multi-step tasks.",
            "inputSchema": { "type": "object", "properties": {
                "instruction": { "type": "string" }, "max_steps": { "type": "number", "description": "1-12, default 8" }
            }, "required": ["instruction"] }
        },
        {
            "name": "browser_extract",
            "description": "Answer a question about the current page, or extract structured data from it, using the page's text content and the AI. Does not modify the page.",
            "inputSchema": { "type": "object", "properties": { "request": { "type": "string" } }, "required": ["request"] }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let sess = &state.session;
    let wrap = |r: anyhow::Result<Value>| match r {
        Ok(v) => json_result(v),
        Err(e) => error_result(e.to_string()),
    };
    let idx = || -> i64 {
        args["index"].as_i64().or_else(|| args["index"].as_str().and_then(|s| s.parse().ok())).unwrap_or(-1)
    };

    match name {
        "browser_navigate" => {
            let v = sess.navigate(args["url"].as_str().unwrap_or("")).await;
            if let Ok(ref info) = v {
                state.db.add_history(info["url"].as_str().unwrap_or(""), info["title"].as_str().unwrap_or(""), crate::api::now()).ok();
            }
            wrap(v)
        }
        "browser_snapshot" => wrap(sess.snapshot().await),
        "browser_click" => wrap(sess.click_index(idx()).await),
        "browser_type" => {
            let submit = args["submit"].as_bool().unwrap_or(false);
            wrap(sess.type_index(idx(), args["text"].as_str().unwrap_or(""), submit).await)
        }
        "browser_press_key" => wrap(sess.press_key(args["key"].as_str().unwrap_or("Enter")).await),
        "browser_scroll" => {
            let amount = args["amount"].as_f64().unwrap_or(600.0);
            let dy = if args["direction"].as_str().unwrap_or("down").eq_ignore_ascii_case("up") { -amount } else { amount };
            wrap(sess.scroll(0.0, dy).await)
        }
        "browser_back" => wrap(sess.go_back().await),
        "browser_forward" => wrap(sess.go_forward().await),
        "browser_reload" => wrap(sess.reload().await),
        "browser_get_info" => wrap(sess.info().await),
        "browser_extract_text" => wrap(sess.extract_text(args["selector"].as_str()).await),
        "browser_extract_links" => wrap(sess.extract_links().await),
        "browser_execute_js" => wrap(sess.execute_js(args["script"].as_str().unwrap_or("")).await),
        "browser_new_tab" => wrap(sess.new_tab(args["url"].as_str()).await),
        "browser_list_tabs" => wrap(sess.list_tabs().await),
        "browser_switch_tab" => wrap(sess.switch_tab(idx().max(0) as usize).await),
        "browser_close_tab" => wrap(sess.close_tab(idx().max(0) as usize).await),
        "browser_act" => {
            let instruction = args["instruction"].as_str().unwrap_or("");
            if instruction.trim().is_empty() {
                return error_result("instruction is required".into());
            }
            let steps = args["max_steps"].as_u64().unwrap_or(8) as usize;
            match crate::llm::act(sess, instruction, steps).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }
        "browser_extract" => {
            let request = args["request"].as_str().unwrap_or("");
            if request.trim().is_empty() {
                return error_result("request is required".into());
            }
            match crate::llm::extract(sess, request).await {
                Ok((answer, model)) => json_result(json!({ "answer": answer, "model": model })),
                Err(e) => error_result(e),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
