use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use crate::deepwiki::query;
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::deepwiki::api::AppState;
use crate::deepwiki::wiki;

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
            "serverInfo": { "name": "deepwiki-mcp", "version": "1.0.0" }
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
            "name": "deepwiki_index",
            "description": "Index (or re-index) a repository by absolute path so the wiki can be generated from it. Run this first.",
            "inputSchema": { "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }
        },
        {
            "name": "deepwiki_outline",
            "description": "High-level structural map of the indexed repo: stats, top-level directories, largest files, architectural types (classes/structs/traits/interfaces), and the most-called symbols. Use this to PLAN which wiki pages to write.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "deepwiki_context",
            "description": "Source-grounded evidence for a topic or question: matching symbols (with signatures/docs/line numbers), their callers/callees, and the outlines of the most relevant files. Use this to WRITE a page or ANSWER a question WITHOUT hallucinating. Always cite the returned file paths.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string" },
                "depth": { "type": "number" }
            }, "required": ["query"] }
        },
        {
            "name": "deepwiki_search",
            "description": "Full-text search over symbol names/signatures/docs — a lighter alternative to deepwiki_context for quickly locating where something is defined.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string" },
                "limit": { "type": "number" }
            }, "required": ["query"] }
        },
        {
            "name": "deepwiki_explore",
            "description": "PREFERRED for understanding code. Given a symbol name or query, returns matching definitions (file/line, signature, doc), the callers and callees, and the transitive blast radius in one shot — so you avoid grep/glob/read crawling.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string" },
                "depth": { "type": "number", "description": "Blast-radius depth (default 3)" }
            }, "required": ["query"] }
        },
        {
            "name": "deepwiki_symbol",
            "description": "Look up a symbol by exact name: its definition(s), direct callers, and direct callees.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" }
            }, "required": ["name"] }
        },
        {
            "name": "deepwiki_impact",
            "description": "Impact analysis: the transitive set of callers (blast radius) that could be affected by changing a symbol.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" },
                "depth": { "type": "number" }
            }, "required": ["name"] }
        },
        {
            "name": "deepwiki_file_outline",
            "description": "List all symbols defined in a single file (its structural outline) plus its imports.",
            "inputSchema": { "type": "object", "properties": {
                "path": { "type": "string", "description": "Repo-relative file path" }
            }, "required": ["path"] }
        },
        {
            "name": "deepwiki_list_files",
            "description": "List all indexed files with language and line count.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "deepwiki_snippet",
            "description": "Read the actual source code of a symbol (by `name`) or a file line range (`path` + `start`/`end`). Use to quote exact code in a wiki page or answer.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string" },
                "path": { "type": "string" },
                "start": { "type": "number" },
                "end": { "type": "number" },
                "context": { "type": "number" }
            } }
        },
        {
            "name": "deepwiki_status",
            "description": "Index status: indexed repo root, file/symbol/edge counts, and number of wiki pages.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "deepwiki_save_page",
            "description": "Create or update a wiki page (Markdown). Use a kebab-case slug; set parent to nest under another page's slug for the sidebar tree.",
            "inputSchema": { "type": "object", "properties": {
                "slug": { "type": "string" },
                "title": { "type": "string" },
                "content": { "type": "string", "description": "Markdown body" },
                "parent": { "type": "string" },
                "ord": { "type": "number" }
            }, "required": ["slug", "title", "content"] }
        },
        {
            "name": "deepwiki_list_pages",
            "description": "List existing wiki pages (slug, title, parent).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "deepwiki_get_page",
            "description": "Get the full Markdown content of a wiki page by slug.",
            "inputSchema": { "type": "object", "properties": { "slug": { "type": "string" } }, "required": ["slug"] }
        },
        {
            "name": "deepwiki_delete_page",
            "description": "Delete a wiki page by slug.",
            "inputSchema": { "type": "object", "properties": { "slug": { "type": "string" } }, "required": ["slug"] }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let __dbo = state.db(); let db = &__dbo;
    match name {
        "deepwiki_index" => {
            let p = args["path"].as_str().unwrap_or("");
            let root = std::path::PathBuf::from(crate::deepwiki::api::expand(p));
            if !root.is_dir() {
                return error_result(format!("not a directory: {}", root.display()));
            }
            match crate::deepwiki::index::index_repo(db, &root) {
                Ok(rep) => json_result(serde_json::to_value(rep).unwrap_or_default()),
                Err(e) => error_result(format!("index failed: {e}")),
            }
        }
        "deepwiki_outline" => match wiki::outline(db) {
            Ok(v) => json_result(v),
            Err(e) => error_result(format!("outline failed: {e}")),
        },
        "deepwiki_context" => {
            let q = args["query"].as_str().unwrap_or("");
            let depth = args["depth"].as_u64().unwrap_or(3) as u32;
            match wiki::context(db, q, depth) {
                Ok(v) => json_result(v),
                Err(e) => error_result(format!("context failed: {e}")),
            }
        }
        "deepwiki_search" => {
            let q = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(30) as u32;
            match query::search(db, q, limit) {
                Ok(rows) => json_result(json!(rows)),
                Err(e) => error_result(format!("search failed: {e}")),
            }
        }
        "deepwiki_explore" => {
            let q = args["query"].as_str().unwrap_or("");
            let depth = args["depth"].as_u64().unwrap_or(3) as u32;
            match query::explore(db, q, depth) {
                Ok(ex) => json_result(serde_json::to_value(ex).unwrap_or_default()),
                Err(e) => error_result(format!("explore failed: {e}")),
            }
        }
        "deepwiki_symbol" => {
            let n = args["name"].as_str().unwrap_or("");
            let defs = query::symbols_by_name(db, n).unwrap_or_default();
            let callers = query::callers(db, n, 100).unwrap_or_default();
            let callees = query::callees(db, n, 100).unwrap_or_default();
            json_result(json!({ "name": n, "definitions": defs, "callers": callers, "callees": callees }))
        }
        "deepwiki_impact" => {
            let n = args["name"].as_str().unwrap_or("");
            let depth = args["depth"].as_u64().unwrap_or(4) as u32;
            match query::blast_radius(db, n, depth) {
                Ok(rows) => json_result(json!({ "name": n, "blast_radius": rows, "count": rows.len() })),
                Err(e) => error_result(format!("impact failed: {e}")),
            }
        }
        "deepwiki_file_outline" => {
            let p = args["path"].as_str().unwrap_or("");
            let outline = query::file_outline(db, p).unwrap_or_default();
            let imports = query::imports_of_file(db, p).unwrap_or_default();
            json_result(json!({ "path": p, "outline": outline, "imports": imports }))
        }
        "deepwiki_list_files" => match query::list_files(db) {
            Ok(rows) => json_result(json!(rows)),
            Err(e) => error_result(format!("list_files failed: {e}")),
        },
        "deepwiki_snippet" => {
            let ctx = args["context"].as_i64().unwrap_or(2);
            let res = if let Some(name) = args["name"].as_str().filter(|s| !s.is_empty()) {
                query::symbol_source(db, name, ctx)
            } else if let Some(path) = args["path"].as_str().filter(|s| !s.is_empty()) {
                let start = args["start"].as_i64().unwrap_or(1);
                let end = args["end"].as_i64().unwrap_or(start + 40);
                query::snippet(db, path, start, end, ctx)
            } else {
                return error_result("provide either `name` or `path`+`start`/`end`".into());
            };
            match res {
                Ok(v) => json_result(v),
                Err(e) => error_result(format!("snippet failed: {e}")),
            }
        }
        "deepwiki_status" => match query::stats(db) {
            Ok(s) => {
                let root = db.get_meta("root").ok().flatten();
                let pages = wiki::page_count(db).unwrap_or(0);
                json_result(json!({ "root": root, "stats": s, "pages": pages }))
            }
            Err(e) => error_result(format!("status failed: {e}")),
        },
        "deepwiki_save_page" => {
            let input = wiki::PageInput {
                slug: args["slug"].as_str().unwrap_or("").to_string(),
                title: args["title"].as_str().unwrap_or("").to_string(),
                parent: args["parent"].as_str().map(|s| s.to_string()),
                content: args["content"].as_str().unwrap_or("").to_string(),
                ord: args["ord"].as_i64().unwrap_or(0),
            };
            if input.slug.is_empty() {
                return error_result("slug is required".into());
            }
            match wiki::save_page(db, &input) {
                Ok(()) => json_result(json!({ "success": true, "slug": input.slug })),
                Err(e) => error_result(format!("save failed: {e}")),
            }
        }
        "deepwiki_list_pages" => match wiki::list_pages(db) {
            Ok(v) => json_result(json!(v)),
            Err(e) => error_result(format!("list failed: {e}")),
        },
        "deepwiki_get_page" => {
            let slug = args["slug"].as_str().unwrap_or("");
            match wiki::get_page(db, slug) {
                Ok(Some(p)) => json_result(json!(p)),
                Ok(None) => error_result(format!("no page: {slug}")),
                Err(e) => error_result(format!("get failed: {e}")),
            }
        }
        "deepwiki_delete_page" => {
            let slug = args["slug"].as_str().unwrap_or("");
            match wiki::delete_page(db, slug) {
                Ok(()) => json_result(json!({ "success": true })),
                Err(e) => error_result(format!("delete failed: {e}")),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
