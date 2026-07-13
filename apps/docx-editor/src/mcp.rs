use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::{now, AppState};
use crate::docx;

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
            "serverInfo": { "name": "docx-editor-mcp", "version": "1.0.0" }
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
            "name": "docx_list",
            "description": "List every open document in the DOCX Editor (id, title, excerpt, size, updated_at). Start here to find one to work on.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "docx_create",
            "description": "Create a new blank document with a title (and optional starting content). Returns its id. Use docx_write to fill in the body.",
            "inputSchema": { "type": "object", "properties": {
                "title": { "type": "string" },
                "content": { "type": "string", "description": "Optional initial body text; \\n starts a new paragraph." }
            }, "required": ["title"] }
        },
        {
            "name": "docx_open",
            "description": "Open a document by id OR by exact title match. Returns id, title, and full plain-text content (paragraphs joined by \\n). If both are given, id wins.",
            "inputSchema": { "type": "object", "properties": {
                "id":    { "type": "number" },
                "title": { "type": "string" }
            } }
        },
        {
            "name": "docx_read",
            "description": "Read the plain-text content of a document by id (or a byte range). Use offset/limit (in characters) for very long docs.",
            "inputSchema": { "type": "object", "properties": {
                "id":     { "type": "number" },
                "offset": { "type": "number", "description": "Character offset to start reading from (default 0)" },
                "limit":  { "type": "number", "description": "Maximum characters to return (default 8000)" }
            }, "required": ["id"] }
        },
        {
            "name": "docx_write",
            "description": "REPLACE the entire body of a document with new text and re-save the .docx. Paragraphs are separated by \\n. Auto-saves the .docx blob.",
            "inputSchema": { "type": "object", "properties": {
                "id":      { "type": "number" },
                "content": { "type": "string" }
            }, "required": ["id", "content"] }
        },
        {
            "name": "docx_append",
            "description": "APPEND text to the end of a document (a leading \\n\\n is added so it starts a new paragraph). Auto-saves.",
            "inputSchema": { "type": "object", "properties": {
                "id":   { "type": "number" },
                "text": { "type": "string" }
            }, "required": ["id", "text"] }
        },
        {
            "name": "docx_replace",
            "description": "Find & replace a substring inside a document. Case-sensitive; set replace_all=false to change only the first match. Auto-saves. Returns the number of replacements.",
            "inputSchema": { "type": "object", "properties": {
                "id":          { "type": "number" },
                "find":        { "type": "string" },
                "replace":     { "type": "string" },
                "replace_all": { "type": "boolean", "description": "Default true" }
            }, "required": ["id", "find", "replace"] }
        },
        {
            "name": "docx_rename",
            "description": "Rename a document (changes its title / filename base). Content is untouched.",
            "inputSchema": { "type": "object", "properties": {
                "id":    { "type": "number" },
                "title": { "type": "string" }
            }, "required": ["id", "title"] }
        },
        {
            "name": "docx_delete",
            "description": "Delete a document permanently.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "number" } }, "required": ["id"] }
        },
        {
            "name": "docx_export_url",
            "description": "Return a URL the user can click to download the raw .docx file for a document.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "number" } }, "required": ["id"] }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    match name {
        "docx_list" => match state.db.list_docs() {
            Ok(docs) => json_result(json!(docs)),
            Err(e) => error_result(e.to_string()),
        },
        "docx_create" => {
            let title = args["title"].as_str().unwrap_or("").trim();
            let content = args["content"].as_str().unwrap_or("");
            if title.is_empty() {
                return error_result("title required".into());
            }
            match state.db.create_doc(title, content, now()) {
                Ok(id) => {
                    let blob = match docx::build_docx(content) {
                        Ok(b) => b,
                        Err(e) => return error_result(e.to_string()),
                    };
                    if let Err(e) = state.db.save_doc(id, None, content, Some(&blob), now()) {
                        return error_result(e.to_string());
                    }
                    text_result(format!(
                        "Created document \"{}\" (id={}, {} chars).",
                        title,
                        id,
                        content.chars().count()
                    ))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "docx_open" => {
            let id_opt = args["id"].as_i64();
            let id = match id_opt {
                Some(i) => i,
                None => {
                    let title = args["title"].as_str().unwrap_or("");
                    if title.is_empty() {
                        return error_result("id or title required".into());
                    }
                    match state.db.find_by_title(title) {
                        Ok(Some(i)) => i,
                        Ok(None) => return error_result(format!("no document titled \"{}\"", title)),
                        Err(e) => return error_result(e.to_string()),
                    }
                }
            };
            match state.db.get_doc(id) {
                Ok(Some(doc)) => json_result(json!({
                    "id": doc.id,
                    "title": doc.title,
                    "content": doc.content_text,
                    "chars": doc.content_text.chars().count(),
                    "updated_at": doc.updated_at,
                })),
                Ok(None) => error_result(format!("document id={} not found", id)),
                Err(e) => error_result(e.to_string()),
            }
        }
        "docx_read" => {
            let id = match args["id"].as_i64() {
                Some(i) => i,
                None => return error_result("id required".into()),
            };
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let limit = args["limit"].as_u64().unwrap_or(8000) as usize;
            match state.db.get_doc(id) {
                Ok(Some(doc)) => {
                    let total = doc.content_text.chars().count();
                    let slice: String = doc
                        .content_text
                        .chars()
                        .skip(offset)
                        .take(limit)
                        .collect();
                    let end = (offset + slice.chars().count()).min(total);
                    text_result(format!(
                        "[{}..{} of {} chars]\n{}",
                        offset, end, total, slice
                    ))
                }
                Ok(None) => error_result(format!("document id={} not found", id)),
                Err(e) => error_result(e.to_string()),
            }
        }
        "docx_write" => {
            let id = match args["id"].as_i64() {
                Some(i) => i,
                None => return error_result("id required".into()),
            };
            let content = args["content"].as_str().unwrap_or("");
            match state.db.get_doc(id) {
                Ok(Some(_)) => {
                    let blob = match docx::build_docx(content) {
                        Ok(b) => b,
                        Err(e) => return error_result(e.to_string()),
                    };
                    match state.db.save_doc(id, None, content, Some(&blob), now()) {
                        Ok(()) => text_result(format!(
                            "Saved. Document id={} now has {} chars ({} bytes .docx).",
                            id,
                            content.chars().count(),
                            blob.len()
                        )),
                        Err(e) => error_result(e.to_string()),
                    }
                }
                Ok(None) => error_result(format!("document id={} not found", id)),
                Err(e) => error_result(e.to_string()),
            }
        }
        "docx_append" => {
            let id = match args["id"].as_i64() {
                Some(i) => i,
                None => return error_result("id required".into()),
            };
            let extra = args["text"].as_str().unwrap_or("");
            match state.db.get_doc(id) {
                Ok(Some(doc)) => {
                    let mut new_content = doc.content_text.clone();
                    if !new_content.is_empty() && !new_content.ends_with('\n') {
                        new_content.push_str("\n\n");
                    } else if !new_content.is_empty() {
                        new_content.push('\n');
                    }
                    new_content.push_str(extra);
                    let blob = match docx::build_docx(&new_content) {
                        Ok(b) => b,
                        Err(e) => return error_result(e.to_string()),
                    };
                    match state.db.save_doc(id, None, &new_content, Some(&blob), now()) {
                        Ok(()) => text_result(format!(
                            "Appended {} chars. Document id={} is now {} chars.",
                            extra.chars().count(),
                            id,
                            new_content.chars().count()
                        )),
                        Err(e) => error_result(e.to_string()),
                    }
                }
                Ok(None) => error_result(format!("document id={} not found", id)),
                Err(e) => error_result(e.to_string()),
            }
        }
        "docx_replace" => {
            let id = match args["id"].as_i64() {
                Some(i) => i,
                None => return error_result("id required".into()),
            };
            let find = args["find"].as_str().unwrap_or("");
            let replace = args["replace"].as_str().unwrap_or("");
            let all = args["replace_all"].as_bool().unwrap_or(true);
            if find.is_empty() {
                return error_result("find must be non-empty".into());
            }
            match state.db.get_doc(id) {
                Ok(Some(doc)) => {
                    let (new_content, count) = if all {
                        let count = doc.content_text.matches(find).count();
                        (doc.content_text.replace(find, replace), count)
                    } else {
                        match doc.content_text.find(find) {
                            Some(pos) => {
                                let mut s = String::with_capacity(doc.content_text.len());
                                s.push_str(&doc.content_text[..pos]);
                                s.push_str(replace);
                                s.push_str(&doc.content_text[pos + find.len()..]);
                                (s, 1)
                            }
                            None => (doc.content_text.clone(), 0),
                        }
                    };
                    if count == 0 {
                        return text_result(format!("No matches for \"{}\".", find));
                    }
                    let blob = match docx::build_docx(&new_content) {
                        Ok(b) => b,
                        Err(e) => return error_result(e.to_string()),
                    };
                    match state.db.save_doc(id, None, &new_content, Some(&blob), now()) {
                        Ok(()) => text_result(format!("Replaced {} occurrence(s).", count)),
                        Err(e) => error_result(e.to_string()),
                    }
                }
                Ok(None) => error_result(format!("document id={} not found", id)),
                Err(e) => error_result(e.to_string()),
            }
        }
        "docx_rename" => {
            let id = match args["id"].as_i64() {
                Some(i) => i,
                None => return error_result("id required".into()),
            };
            let title = args["title"].as_str().unwrap_or("").trim();
            if title.is_empty() {
                return error_result("title required".into());
            }
            match state.db.rename_doc(id, title, now()) {
                Ok(()) => text_result(format!("Renamed id={} to \"{}\".", id, title)),
                Err(e) => error_result(e.to_string()),
            }
        }
        "docx_delete" => {
            let id = match args["id"].as_i64() {
                Some(i) => i,
                None => return error_result("id required".into()),
            };
            match state.db.delete_doc(id) {
                Ok(()) => text_result(format!("Deleted document id={}.", id)),
                Err(e) => error_result(e.to_string()),
            }
        }
        "docx_export_url" => {
            let id = match args["id"].as_i64() {
                Some(i) => i,
                None => return error_result("id required".into()),
            };
            match state.db.get_doc(id) {
                Ok(Some(doc)) => {
                    let base = std::env::var("SPACE_APP_BASE_URL")
                        .unwrap_or_else(|_| "http://localhost:4380".into());
                    text_result(format!(
                        "Download \"{}\": {}/api/doc/{}/download",
                        doc.title, base, id
                    ))
                }
                Ok(None) => error_result(format!("document id={} not found", id)),
                Err(e) => error_result(e.to_string()),
            }
        }
        _ => error_result(format!("unknown tool: {}", name)),
    }
}
