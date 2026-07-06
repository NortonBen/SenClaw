use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::{norm_layout, now, AppState};

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
            "serverInfo": { "name": "mindmap-mcp", "version": "1.0.0" }
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
            "name": "mindmap_list",
            "description": "List all mind maps (id, title, description, node count). Start here to find a map to work on.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mindmap_create",
            "description": "Create a new mind map. Returns its id and the id of its root node (the map's central topic). Add nodes under the root with mindmap_add_node. Optionally pick a layout: mindmap (two-sided, default), org (top-down org chart), outline (indented list), right (horizontal tree).",
            "inputSchema": { "type": "object", "properties": {
                "title": { "type": "string", "description": "The central topic / map title" },
                "description": { "type": "string" },
                "layout": { "type": "string", "enum": ["mindmap", "org", "outline", "right"] }
            }, "required": ["title"] }
        },
        {
            "name": "mindmap_templates",
            "description": "List the built-in starter templates (id, name, category, layout). Use mindmap_from_template to instantiate one.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "mindmap_from_template",
            "description": "Create a new map pre-filled from a built-in template (its layout + styled node tree). Get template ids from mindmap_templates.",
            "inputSchema": { "type": "object", "properties": {
                "template_id": { "type": "string" },
                "title": { "type": "string", "description": "Optional title override" }
            }, "required": ["template_id"] }
        },
        {
            "name": "mindmap_set_layout",
            "description": "Change a map's layout style: mindmap | org | outline | right.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" },
                "layout": { "type": "string", "enum": ["mindmap", "org", "outline", "right"] }
            }, "required": ["id", "layout"] }
        },
        {
            "name": "mindmap_get",
            "description": "Get a mind map's full node tree by id. Each node has id, text, note, color, collapsed, and nested children.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "number" } }, "required": ["id"] }
        },
        {
            "name": "mindmap_delete",
            "description": "Delete a mind map and all of its nodes.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "number" } }, "required": ["id"] }
        },
        {
            "name": "mindmap_add_node",
            "description": "Add a child node under an existing node (parent_id). Use the root id from mindmap_create to add top-level branches, then nest deeper by passing a branch's id. Returns the new node id.",
            "inputSchema": { "type": "object", "properties": {
                "parent_id": { "type": "number" },
                "text": { "type": "string", "description": "Short node label" },
                "note": { "type": "string", "description": "Optional longer note" },
                "color": { "type": "string", "description": "Optional hex color, e.g. #f97316" }
            }, "required": ["parent_id", "text"] }
        },
        {
            "name": "mindmap_update_node",
            "description": "Edit a node's text, note, color, shape, fill and/or icon. shape: rounded|rect|pill|ellipse|line. fill: true = filled with color, false = outlined. icon: a single emoji shown before the label.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" },
                "text": { "type": "string" },
                "note": { "type": "string" },
                "color": { "type": "string", "description": "hex color e.g. #3b82f6" },
                "shape": { "type": "string", "enum": ["rounded", "rect", "pill", "ellipse", "line"] },
                "fill": { "type": "boolean" },
                "icon": { "type": "string", "description": "a single emoji" }
            }, "required": ["id"] }
        },
        {
            "name": "mindmap_delete_node",
            "description": "Delete a node and its whole subtree. Cannot delete a map's root node.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "number" } }, "required": ["id"] }
        },
        {
            "name": "mindmap_generate",
            "description": "AI-generate a structured hierarchy of sub-topics under a node and insert it into the map in one step. Great for fleshing out a topic quickly. Set replace=true to overwrite the node's current children.",
            "inputSchema": { "type": "object", "properties": {
                "parent_id": { "type": "number", "description": "Node to attach the generated subtree under" },
                "topic": { "type": "string", "description": "Topic to expand (defaults to the parent node's text)" },
                "instruction": { "type": "string", "description": "Optional guidance, e.g. 'focus on risks' or '5 branches only'" },
                "replace": { "type": "boolean", "description": "Replace existing children instead of appending" }
            }, "required": ["parent_id"] }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let db = &state.db;
    match name {
        "mindmap_list" => match db.list_maps() {
            Ok(v) => json_result(json!(v)),
            Err(e) => error_result(e.to_string()),
        },
        "mindmap_create" => {
            let title = args["title"].as_str().unwrap_or("").trim();
            if title.is_empty() {
                return error_result("title is required".into());
            }
            let desc = args["description"].as_str().unwrap_or("");
            let layout = norm_layout(args["layout"].as_str());
            match db.create_map(title, desc, layout, now()) {
                Ok((id, root_id)) => json_result(json!({ "id": id, "rootId": root_id, "layout": layout })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "mindmap_templates" => json_result(json!(crate::templates::list())),
        "mindmap_from_template" => {
            let tid = args["template_id"].as_str().unwrap_or("");
            let tpl = match crate::templates::find(tid) {
                Some(t) => t,
                None => return error_result(format!("unknown template: {tid}")),
            };
            let title = args["title"].as_str().map(str::trim).filter(|t| !t.is_empty()).unwrap_or(tpl.root);
            match db.create_map(title, tpl.description, tpl.layout, now()) {
                Ok((id, root_id)) => {
                    let children = (tpl.build)();
                    match db.insert_subtree(root_id, &children, now()) {
                        Ok(added) => json_result(json!({ "id": id, "rootId": root_id, "layout": tpl.layout, "added": added })),
                        Err(e) => error_result(e.to_string()),
                    }
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "mindmap_set_layout" => {
            let id = args["id"].as_i64().unwrap_or(0);
            let layout = norm_layout(args["layout"].as_str());
            match db.set_layout(id, layout, now()) {
                Ok(()) => json_result(json!({ "success": true, "layout": layout })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "mindmap_get" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match (db.map_meta(id), db.tree_of(id)) {
                (Ok(Some(meta)), Ok(tree)) => json_result(json!({ "meta": meta, "tree": tree })),
                (Ok(None), _) => error_result(format!("map {id} not found")),
                (Err(e), _) | (_, Err(e)) => error_result(e.to_string()),
            }
        }
        "mindmap_delete" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match db.delete_map(id) {
                Ok(()) => json_result(json!({ "success": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "mindmap_add_node" => {
            let parent = args["parent_id"].as_i64().unwrap_or(0);
            let text = args["text"].as_str().unwrap_or("").trim();
            if text.is_empty() {
                return error_result("text is required".into());
            }
            let note = args["note"].as_str().unwrap_or("");
            let color = args["color"].as_str();
            match db.add_node(parent, text, note, color, now()) {
                Ok(id) => json_result(json!({ "id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "mindmap_update_node" => {
            let id = args["id"].as_i64().unwrap_or(0);
            let text = args["text"].as_str();
            let note = args["note"].as_str();
            let color = args.get("color").map(|c| c.as_str());
            let shape = args.get("shape").map(|c| c.as_str());
            let fill = args["fill"].as_bool();
            let icon = args.get("icon").map(|c| c.as_str());
            match db.update_node(id, text, note, color, shape, fill, icon, None, now()) {
                Ok(()) => json_result(json!({ "success": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "mindmap_delete_node" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match db.delete_node(id, now()) {
                Ok(()) => json_result(json!({ "success": true })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "mindmap_generate" => {
            let parent = args["parent_id"].as_i64().unwrap_or(0);
            let parent_text = match db.node_text(parent) {
                Ok(t) => t,
                Err(e) => return error_result(e.to_string()),
            };
            let topic = args["topic"].as_str().filter(|t| !t.trim().is_empty()).unwrap_or(&parent_text).to_string();
            let instruction = args["instruction"].as_str();
            let replace = args["replace"].as_bool().unwrap_or(false);
            let path = db.ancestor_path(parent).unwrap_or_default();
            match crate::llm::generate(&topic, &path, instruction, None).await {
                Ok(gen) => {
                    let res = if replace {
                        db.replace_children(parent, &gen.children, now())
                    } else {
                        db.insert_subtree(parent, &gen.children, now())
                    };
                    match res {
                        Ok(added) => json_result(json!({ "added": added, "model": gen.model })),
                        Err(e) => error_result(e.to_string()),
                    }
                }
                Err(e) => error_result(e),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
