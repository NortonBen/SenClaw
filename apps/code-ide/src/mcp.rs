use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::{convert::Infallible, sync::Arc};

use crate::api::{expand, AppState};
use crate::workspace;

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
            "serverInfo": { "name": "code-ide-mcp", "version": "1.0.0" }
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
    json!([
        {
            "name": "ide_open",
            "description": "Open a local folder as the editor's workspace (absolute path). Run this first; every other tool operates on workspace-relative paths.",
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }
        },
        {
            "name": "ide_status",
            "description": "Current workspace: the open root path and its name (or none if nothing is open).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "ide_list_dir",
            "description": "List immediate children of a directory in the workspace. Empty path = workspace root. Dirs first, then files.",
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string", "description": "Workspace-relative dir (default root)" } } }
        },
        {
            "name": "ide_read_file",
            "description": "Read a UTF-8 text file's contents by workspace-relative path.",
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }
        },
        {
            "name": "ide_write_file",
            "description": "Write (create or overwrite) a text file at a workspace-relative path. Creates parent dirs. Reflected live in the editor.",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            }, "required": ["path", "content"] }
        },
        {
            "name": "ide_create",
            "description": "Create a new empty file or directory at a workspace-relative path.",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string" },
                "dir": { "type": "boolean", "description": "true = directory, false = file (default)" }
            }, "required": ["path"] }
        },
        {
            "name": "ide_rename",
            "description": "Move/rename a file or directory within the workspace.",
            "inputSchema": { "type": "object", "properties": {
                "from": { "type": "string" },
                "to": { "type": "string" }
            }, "required": ["from", "to"] }
        },
        {
            "name": "ide_delete",
            "description": "Delete a file or directory (recursive) at a workspace-relative path.",
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }
        },
        {
            "name": "ide_search",
            "description": "Case-insensitive plain-text search across the workspace (respects .gitignore). Returns path:line:text matches.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string" },
                "limit": { "type": "number" }
            }, "required": ["query"] }
        }
    ])
}

fn require_root(state: &Arc<AppState>) -> Result<PathBuf, String> {
    state
        .root
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "no workspace open — call ide_open first".to_string())
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    match name {
        "ide_open" => {
            let p = expand(args["path"].as_str().unwrap_or(""));
            let root = match PathBuf::from(&p).canonicalize() {
                Ok(r) if r.is_dir() => r,
                _ => return error_result(format!("not a directory: {p}")),
            };
            let name = root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            *state.root.lock().unwrap() = Some(root.clone());
            let _ = state.db.set_meta("root", &root.to_string_lossy());
            crate::watch::install_watcher(state, &root);
            json_result(json!({ "root": root.to_string_lossy(), "name": name }))
        }
        "ide_status" => {
            let root = state.root.lock().unwrap().clone();
            match root {
                Some(p) => json_result(json!({
                    "root": p.to_string_lossy(),
                    "name": p.file_name().map(|n| n.to_string_lossy().to_string()),
                })),
                None => json_result(json!({ "root": null })),
            }
        }
        "ide_list_dir" => {
            let root = match require_root(state) {
                Ok(r) => r,
                Err(e) => return error_result(e),
            };
            let rel = args["path"].as_str().unwrap_or("");
            match workspace::list_dir(&root, rel) {
                Ok(v) => json_result(json!(v)),
                Err(e) => error_result(e.to_string()),
            }
        }
        "ide_read_file" => {
            let root = match require_root(state) {
                Ok(r) => r,
                Err(e) => return error_result(e),
            };
            let rel = args["path"].as_str().unwrap_or("");
            match workspace::read_file(&root, rel) {
                Ok(v) => json_result(json!(v)),
                Err(e) => error_result(e.to_string()),
            }
        }
        "ide_write_file" => {
            let root = match require_root(state) {
                Ok(r) => r,
                Err(e) => return error_result(e),
            };
            let rel = args["path"].as_str().unwrap_or("");
            let content = args["content"].as_str().unwrap_or("");
            match workspace::write_file(&root, rel, content) {
                Ok(()) => json_result(json!({ "success": true, "path": rel })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "ide_create" => {
            let root = match require_root(state) {
                Ok(r) => r,
                Err(e) => return error_result(e),
            };
            let rel = args["path"].as_str().unwrap_or("");
            let dir = args["dir"].as_bool().unwrap_or(false);
            match workspace::create_path(&root, rel, dir) {
                Ok(()) => json_result(json!({ "success": true, "path": rel })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "ide_rename" => {
            let root = match require_root(state) {
                Ok(r) => r,
                Err(e) => return error_result(e),
            };
            let from = args["from"].as_str().unwrap_or("");
            let to = args["to"].as_str().unwrap_or("");
            match workspace::rename_path(&root, from, to) {
                Ok(()) => json_result(json!({ "success": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "ide_delete" => {
            let root = match require_root(state) {
                Ok(r) => r,
                Err(e) => return error_result(e),
            };
            let rel = args["path"].as_str().unwrap_or("");
            match workspace::delete_path(&root, rel) {
                Ok(()) => json_result(json!({ "success": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "ide_search" => {
            let root = match require_root(state) {
                Ok(r) => r,
                Err(e) => return error_result(e),
            };
            let q = args["query"].as_str().unwrap_or("").to_string();
            let limit = args["limit"].as_u64().unwrap_or(100) as usize;
            match workspace::search_text(&root, &q, limit) {
                Ok(v) => json_result(json!(v)),
                Err(e) => error_result(e.to_string()),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
