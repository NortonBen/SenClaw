use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::{app_url, broadcast_update, norm_kind, now, AppState};
use crate::llm::truncate;

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
            "serverInfo": { "name": "drawio-mcp", "version": "1.0.0" }
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
            "name": "drawio_list",
            "description": "List all draw.io diagrams (id, name, kind, cell count, updated_at). Start here to find a diagram to work on.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "drawio_create",
            "description": "Create a new empty diagram. Returns its id and a URL that opens it in the Diagrams app. To create WITH content in one step, prefer drawio_generate.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "Diagram title" },
                "kind": { "type": "string", "enum": ["flowchart", "sequence", "architecture", "er", "state", "class", "org", "network", "bpmn"] }
            }, "required": ["name"] }
        },
        {
            "name": "drawio_get",
            "description": "Get a diagram's metadata and its full mxGraph XML by id.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "number" } }, "required": ["id"] }
        },
        {
            "name": "drawio_rename",
            "description": "Rename a diagram.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }, "name": { "type": "string" }
            }, "required": ["id", "name"] }
        },
        {
            "name": "drawio_delete",
            "description": "Delete a diagram permanently.",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "number" } }, "required": ["id"] }
        },
        {
            "name": "drawio_generate",
            "description": "AI-generate a full draw.io diagram from a plain-language description and save it. Creates a new diagram (or overwrites diagram_id if given). Returns {id, path, url, direct_url, svg_path, cells, model}. In a SenClaw chat reply, link the diagram as a markdown link using `path` (e.g. [Mở sơ đồ](/space/app/drawio?d=5)) — it opens the Diagrams app right inside the SenClaw screen; `url` is the absolute variant for outside contexts.",
            "inputSchema": { "type": "object", "properties": {
                "prompt": { "type": "string", "description": "What to draw, e.g. 'quy trình đăng ký tài khoản với xác thực OTP'" },
                "name": { "type": "string", "description": "Diagram title (defaults to a snippet of the prompt)" },
                "kind": { "type": "string", "enum": ["flowchart", "sequence", "architecture", "er", "state", "class", "org", "network", "bpmn"], "description": "Diagram family hint (default flowchart)" },
                "diagram_id": { "type": "number", "description": "Overwrite this existing diagram instead of creating a new one" }
            }, "required": ["prompt"] }
        },
        {
            "name": "drawio_edit_ai",
            "description": "AI-edit an existing diagram in place: give a plain-language instruction (e.g. 'thêm bước xác thực OTP sau login', 'đổi hướng flow sang ngang'). The updated diagram is saved and pushed live to an open editor.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" },
                "instruction": { "type": "string" }
            }, "required": ["id", "instruction"] }
        },
        {
            "name": "drawio_get_xml",
            "description": "Get only the raw mxGraph XML of a diagram (for programmatic transforms).",
            "inputSchema": { "type": "object", "properties": { "id": { "type": "number" } }, "required": ["id"] }
        },
        {
            "name": "drawio_set_xml",
            "description": "Replace a diagram's mxGraph XML wholesale. The XML is validated (mandatory root cells 0/1, unique ids, vertex xor edge) before saving.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" },
                "xml": { "type": "string", "description": "Uncompressed <mxGraphModel>…</mxGraphModel> XML" }
            }, "required": ["id", "xml"] }
        },
        {
            "name": "drawio_export",
            "description": "Export a diagram: format 'xml' returns the source; 'svg' returns the last SVG snapshot cached from the editor plus `svg_path` — a same-origin URL you can pass to the emit_widget tool (kind 'image', data.url = svg_path) to show the diagram INLINE in the chat. stale=true means the diagram changed since the editor last rendered it (open it in the app once to refresh); a missing snapshot is an error until the diagram has been opened once.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" },
                "format": { "type": "string", "enum": ["xml", "svg"] }
            }, "required": ["id"] }
        }
    ])
}

/// Derive a diagram title from the prompt when none is given.
fn name_from_prompt(prompt: &str) -> String {
    truncate(prompt.trim().lines().next().unwrap_or("Sơ đồ AI"), 48)
}

/// Daemon UI origin (the chat lives there). Injected by the daemon at launch.
fn daemon_base() -> String {
    std::env::var("SENCLAW_BASE_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:18788".into())
        .trim_end_matches('/')
        .to_string()
}

/// Same-origin path that opens the diagram inside the SenClaw UI shell
/// (Space → app frame; the frame forwards `?d=` to the app). Use THIS form in
/// chat replies — it works from any host the SenClaw web UI is opened on.
fn diagram_path(id: i64) -> String {
    format!("/space/app/drawio?d={id}")
}

/// Absolute variant of [`diagram_path`] for contexts outside the SenClaw UI.
fn diagram_url(id: i64) -> String {
    format!("{}{}", daemon_base(), diagram_path(id))
}

/// Same-origin path serving the cached SVG snapshot through the daemon's app
/// proxy — usable as `emit_widget` image url to show the diagram inline in
/// chat. 404s until the diagram has been rendered once by an open editor.
fn svg_path(id: i64) -> String {
    format!("/api/space/apps/drawio/proxy/api/diagrams/{id}/export?format=svg")
}

fn link_fields(id: i64) -> Value {
    json!({
        "url": diagram_url(id),
        "path": diagram_path(id),
        "direct_url": format!("{}/?d={id}", app_url()),
        "svg_path": svg_path(id),
    })
}

/// Merge the link fields into a tool result object.
fn with_links(mut v: Value, id: i64) -> Value {
    if let (Some(obj), Some(links)) = (v.as_object_mut(), link_fields(id).as_object()) {
        for (k, val) in links {
            obj.insert(k.clone(), val.clone());
        }
    }
    v
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let db = &state.db;
    match name {
        "drawio_list" => match db.list() {
            Ok(v) => json_result(json!(v)),
            Err(e) => error_result(e.to_string()),
        },
        "drawio_create" => {
            let title = args["name"].as_str().unwrap_or("").trim();
            if title.is_empty() {
                return error_result("name is required".into());
            }
            let kind = norm_kind(args["kind"].as_str());
            match db.create(title, kind, "", now()) {
                Ok(id) => {
                    broadcast_update(state, id);
                    json_result(with_links(json!({ "id": id, "kind": kind }), id))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "drawio_get" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match db.get(id) {
                Ok(Some(d)) => json_result(json!(d)),
                Ok(None) => error_result(format!("diagram {id} not found")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "drawio_rename" => {
            let id = args["id"].as_i64().unwrap_or(0);
            let title = args["name"].as_str().unwrap_or("").trim();
            if title.is_empty() {
                return error_result("name is required".into());
            }
            match db.rename(id, title, now()) {
                Ok(()) => {
                    broadcast_update(state, id);
                    json_result(json!({ "success": true }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "drawio_delete" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match db.delete(id) {
                Ok(()) => {
                    let _ = state
                        .events_tx
                        .send(json!({ "type": "diagram:delete", "id": id }).to_string());
                    json_result(json!({ "success": true }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "drawio_generate" => {
            let prompt = args["prompt"].as_str().unwrap_or("").trim();
            if prompt.is_empty() {
                return error_result("prompt is required".into());
            }
            let kind = norm_kind(args["kind"].as_str());
            let target = args["diagram_id"].as_i64().filter(|&i| i > 0);
            // Headless generation must produce storable XML, so this is always
            // XML mode (Mermaid conversion only exists inside the editor).
            match crate::llm::generate_xml(prompt, kind).await {
                Ok((xml, model)) => {
                    let cells = xml.matches("<mxCell").count();
                    let result = match target {
                        Some(id) => db.set_xml(id, &xml, now()).map(|_| id),
                        None => {
                            let title = args["name"]
                                .as_str()
                                .map(str::trim)
                                .filter(|t| !t.is_empty())
                                .map(str::to_string)
                                .unwrap_or_else(|| name_from_prompt(prompt));
                            db.create(&title, kind, &xml, now())
                        }
                    };
                    match result {
                        Ok(id) => {
                            db.log_ai(id, prompt, "xml", &model, "stop", true, now());
                            broadcast_update(state, id);
                            json_result(with_links(
                                json!({ "id": id, "cells": cells, "model": model }),
                                id,
                            ))
                        }
                        Err(e) => error_result(e.to_string()),
                    }
                }
                Err(e) => {
                    db.log_ai(
                        target.unwrap_or(0),
                        prompt,
                        "xml",
                        "",
                        "error",
                        false,
                        now(),
                    );
                    error_result(e)
                }
            }
        }
        "drawio_edit_ai" => {
            let id = args["id"].as_i64().unwrap_or(0);
            let instruction = args["instruction"].as_str().unwrap_or("").trim();
            if instruction.is_empty() {
                return error_result("instruction is required".into());
            }
            let current = match db.get(id) {
                Ok(Some(d)) if !d.xml.trim().is_empty() => d.xml,
                Ok(Some(_)) => {
                    return error_result("diagram is empty — use drawio_generate instead".into())
                }
                Ok(None) => return error_result(format!("diagram {id} not found")),
                Err(e) => return error_result(e.to_string()),
            };
            match crate::llm::edit_xml(&current, instruction).await {
                Ok((xml, model)) => match db.set_xml(id, &xml, now()) {
                    Ok(()) => {
                        db.log_ai(id, instruction, "edit", &model, "stop", true, now());
                        broadcast_update(state, id);
                        json_result(with_links(json!({ "id": id, "model": model }), id))
                    }
                    Err(e) => error_result(e.to_string()),
                },
                Err(e) => {
                    db.log_ai(id, instruction, "edit", "", "error", false, now());
                    error_result(e)
                }
            }
        }
        "drawio_get_xml" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match db.get(id) {
                Ok(Some(d)) => text_result(d.xml),
                Ok(None) => error_result(format!("diagram {id} not found")),
                Err(e) => error_result(e.to_string()),
            }
        }
        "drawio_set_xml" => {
            let id = args["id"].as_i64().unwrap_or(0);
            let xml = args["xml"].as_str().unwrap_or("");
            if let Err(e) = crate::llm::validate_mxgraph(xml) {
                return error_result(format!("invalid mxGraph XML: {e}"));
            }
            match db.set_xml(id, xml, now()) {
                Ok(()) => {
                    broadcast_update(state, id);
                    json_result(with_links(json!({ "success": true }), id))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "drawio_export" => {
            let id = args["id"].as_i64().unwrap_or(0);
            match args["format"].as_str().unwrap_or("xml") {
                "svg" => match db.get_svg(id) {
                    Ok(Some((svg, stale))) if !svg.is_empty() => {
                        json_result(json!({ "stale": stale, "svg_path": svg_path(id), "svg": svg }))
                    }
                    Ok(_) => error_result(
                        "no SVG snapshot yet — open the diagram in the Diagrams app once so the editor renders it"
                            .into(),
                    ),
                    Err(e) => error_result(e.to_string()),
                },
                _ => match db.get(id) {
                    Ok(Some(d)) => text_result(d.xml),
                    Ok(None) => error_result(format!("diagram {id} not found")),
                    Err(e) => error_result(e.to_string()),
                },
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
