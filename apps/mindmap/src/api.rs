use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::{default_data_dir, Db};
use crate::llm::{self, ChatBody};

pub struct AppState {
    pub db: Arc<Db>,
    /// Broadcasts the raw JSON-RPC responses to any connected MCP SSE clients.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub struct ApiError(pub StatusCode, pub String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}
fn gateway(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_GATEWAY, e.to_string())
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn make_state() -> Arc<AppState> {
    let db_path = default_data_dir("mindmap").join("mindmap.db");
    let db = Arc::new(Db::open(&db_path).expect("open mindmap db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState { db, mcp_tx })
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/llm-info", get(llm_info))
        .route("/maps", get(list_maps).post(create_map))
        .route("/maps/from-template", post(create_from_template))
        .route("/templates", get(templates_list))
        .route("/map", get(get_map))
        .route("/map/rename", post(rename_map))
        .route("/map/layout", post(set_layout))
        .route("/map/delete", post(delete_map))
        .route("/node/add", post(add_node))
        .route("/node/update", post(update_node))
        .route("/node/delete", post(delete_node))
        .route("/node/move", post(move_node))
        .route("/node/ai-note", post(ai_note))
        .route("/positions", post(save_positions))
        .route("/map/clear-positions", post(reset_positions))
        .route("/map/restore", post(restore_map))
        .route("/maps/import", post(import_map))
        .route("/generate", post(generate))
        .route("/import", post(import_file))
        .route("/chat", post(chat))
        .route("/chat/sessions", get(list_sessions).post(create_session))
        .route("/chat/session/rename", post(rename_session))
        .route("/chat/session/delete", post(delete_session))
        .route("/chat/messages", get(session_messages))
        .route("/models", get(models))
        .route("/model-active", post(model_active))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status() -> Json<Value> {
    Json(json!({ "ok": true, "app": "mindmap" }))
}

async fn list_maps(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.db.list_maps().map_err(bad)?)))
}

/// Valid layout styles; anything else falls back to `mindmap`.
pub fn norm_layout(l: Option<&str>) -> &'static str {
    match l.unwrap_or("mindmap") {
        "org" => "org",
        "outline" => "outline",
        "right" => "right",
        _ => "mindmap",
    }
}

#[derive(Deserialize)]
struct CreateMapBody {
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    layout: Option<String>,
}

async fn create_map(
    State(s): State<Arc<AppState>>,
    Json(b): Json<CreateMapBody>,
) -> Result<Json<Value>, ApiError> {
    let title = if b.title.trim().is_empty() {
        "Untitled map"
    } else {
        b.title.trim()
    };
    let layout = norm_layout(b.layout.as_deref());
    let (map_id, root_id) =
        s.db.create_map(title, b.description.trim(), layout, now())
            .map_err(bad)?;
    Ok(Json(
        json!({ "id": map_id, "rootId": root_id, "layout": layout }),
    ))
}

async fn templates_list() -> Json<Value> {
    Json(json!(crate::templates::list()))
}

#[derive(Deserialize)]
struct FromTemplateBody {
    template_id: String,
    /// Optional title override; defaults to the template's own root label.
    #[serde(default)]
    title: Option<String>,
}

/// Instantiate a new map from a built-in template (layout + styled node tree).
async fn create_from_template(
    State(s): State<Arc<AppState>>,
    Json(b): Json<FromTemplateBody>,
) -> Result<Json<Value>, ApiError> {
    let tpl = crate::templates::find(&b.template_id).ok_or_else(|| bad("unknown template"))?;
    let title = b
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or(tpl.root);
    let (map_id, root_id) =
        s.db.create_map(title, tpl.description, tpl.layout, now())
            .map_err(bad)?;
    let children = (tpl.build)();
    let added =
        s.db.insert_subtree(root_id, &children, now())
            .map_err(bad)?;
    Ok(Json(
        json!({ "id": map_id, "rootId": root_id, "layout": tpl.layout, "added": added }),
    ))
}

#[derive(Deserialize)]
struct SetLayoutBody {
    id: i64,
    layout: String,
}

async fn set_layout(
    State(s): State<Arc<AppState>>,
    Json(b): Json<SetLayoutBody>,
) -> Result<Json<Value>, ApiError> {
    let layout = norm_layout(Some(&b.layout));
    s.db.set_layout(b.id, layout, now()).map_err(bad)?;
    Ok(Json(json!({ "success": true, "layout": layout })))
}

#[derive(Deserialize)]
struct MapQuery {
    id: i64,
}

async fn get_map(
    State(s): State<Arc<AppState>>,
    Query(q): Query<MapQuery>,
) -> Result<Json<Value>, ApiError> {
    let meta =
        s.db.map_meta(q.id)
            .map_err(bad)?
            .ok_or_else(|| bad("map not found"))?;
    let tree = s.db.tree_of(q.id).map_err(bad)?;
    Ok(Json(json!({ "meta": meta, "tree": tree })))
}

#[derive(Deserialize)]
struct RenameMapBody {
    id: i64,
    title: String,
    #[serde(default)]
    description: String,
}

async fn rename_map(
    State(s): State<Arc<AppState>>,
    Json(b): Json<RenameMapBody>,
) -> Result<Json<Value>, ApiError> {
    s.db.rename_map(b.id, b.title.trim(), b.description.trim(), now())
        .map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

async fn delete_map(
    State(s): State<Arc<AppState>>,
    Json(b): Json<MapQuery>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_map(b.id).map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
struct AddNodeBody {
    parent_id: i64,
    #[serde(default)]
    text: String,
    #[serde(default)]
    note: String,
    #[serde(default)]
    color: Option<String>,
}

async fn add_node(
    State(s): State<Arc<AppState>>,
    Json(b): Json<AddNodeBody>,
) -> Result<Json<Value>, ApiError> {
    let text = if b.text.trim().is_empty() {
        "New idea"
    } else {
        b.text.trim()
    };
    let id =
        s.db.add_node(b.parent_id, text, b.note.trim(), b.color.as_deref(), now())
            .map_err(bad)?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
struct UpdateNodeBody {
    id: i64,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    note: Option<String>,
    /// Present key = set color; value null clears it. Absent key = leave as-is.
    #[serde(default, deserialize_with = "double_option")]
    color: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    shape: Option<Option<String>>,
    #[serde(default)]
    fill: Option<bool>,
    #[serde(default, deserialize_with = "double_option")]
    icon: Option<Option<String>>,
    #[serde(default)]
    collapsed: Option<bool>,
}

async fn update_node(
    State(s): State<Arc<AppState>>,
    Json(b): Json<UpdateNodeBody>,
) -> Result<Json<Value>, ApiError> {
    let color = b.color.as_ref().map(|o| o.as_deref());
    let shape = b.shape.as_ref().map(|o| o.as_deref());
    let icon = b.icon.as_ref().map(|o| o.as_deref());
    s.db.update_node(
        b.id,
        b.text.as_deref(),
        b.note.as_deref(),
        color,
        shape,
        b.fill,
        icon,
        b.collapsed,
        now(),
    )
    .map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
struct NodeIdBody {
    id: i64,
}

async fn delete_node(
    State(s): State<Arc<AppState>>,
    Json(b): Json<NodeIdBody>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_node(b.id, now()).map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
struct MoveNodeBody {
    id: i64,
    new_parent: i64,
}

async fn move_node(
    State(s): State<Arc<AppState>>,
    Json(b): Json<MoveNodeBody>,
) -> Result<Json<Value>, ApiError> {
    s.db.move_node(b.id, b.new_parent, now()).map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
struct AiNoteBody {
    node_id: i64,
}

/// AI-write a note for a node and save it. Returns the note.
async fn ai_note(
    State(s): State<Arc<AppState>>,
    Json(b): Json<AiNoteBody>,
) -> Result<Json<Value>, ApiError> {
    let text = s.db.node_text(b.node_id).map_err(bad)?;
    let path = s.db.ancestor_path(b.node_id).unwrap_or_default();
    let (note, model) = llm::ai_note(&text, &path).await.map_err(gateway)?;
    let note = note.trim();
    s.db.update_node(
        b.node_id,
        None,
        Some(note),
        None,
        None,
        None,
        None,
        None,
        now(),
    )
    .map_err(bad)?;
    Ok(Json(json!({ "note": note, "model": model })))
}

#[derive(Deserialize)]
struct PosItem {
    id: i64,
    x: f64,
    y: f64,
}
#[derive(Deserialize)]
struct PositionsBody {
    items: Vec<PosItem>,
}

/// Persist custom (free-drag) positions for a batch of nodes.
async fn save_positions(
    State(s): State<Arc<AppState>>,
    Json(b): Json<PositionsBody>,
) -> Result<Json<Value>, ApiError> {
    let items: Vec<(i64, f64, f64)> = b.items.into_iter().map(|p| (p.id, p.x, p.y)).collect();
    s.db.set_positions(&items, now()).map_err(bad)?;
    Ok(Json(json!({ "success": true, "count": items.len() })))
}

/// Clear all custom positions in a map → back to auto-layout.
async fn reset_positions(
    State(s): State<Arc<AppState>>,
    Json(b): Json<MapQuery>,
) -> Result<Json<Value>, ApiError> {
    s.db.clear_positions(b.id, now()).map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
struct RestoreBody {
    map_id: i64,
    #[serde(default)]
    layout: Option<String>,
    nodes: Vec<crate::db::RestoreNode>,
}

/// Restore a map's whole node set + layout from a snapshot (undo/redo).
async fn restore_map(
    State(s): State<Arc<AppState>>,
    Json(b): Json<RestoreBody>,
) -> Result<Json<Value>, ApiError> {
    let layout = norm_layout(b.layout.as_deref());
    s.db.restore_map(b.map_id, &b.nodes, layout, now())
        .map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
struct ImportBody {
    title: String,
    #[serde(default)]
    layout: Option<String>,
    #[serde(default)]
    children: Vec<crate::db::GenNode>,
}

/// Create a new map from an imported/parsed node tree (JSON/Markdown/OPML/FreeMind
/// are parsed on the client into this shape).
async fn import_map(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ImportBody>,
) -> Result<Json<Value>, ApiError> {
    let title = if b.title.trim().is_empty() {
        "Sơ đồ nhập"
    } else {
        b.title.trim()
    };
    let layout = norm_layout(b.layout.as_deref());
    let (map_id, root_id) = s.db.create_map(title, "", layout, now()).map_err(bad)?;
    let added =
        s.db.insert_subtree(root_id, &b.children, now())
            .map_err(bad)?;
    Ok(Json(
        json!({ "id": map_id, "rootId": root_id, "added": added }),
    ))
}

#[derive(Deserialize)]
struct GenerateBody {
    /// The node to attach the generated subtree under (usually the selected node).
    parent_id: i64,
    /// Optional topic override; defaults to the parent node's own text.
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    instruction: Option<String>,
    /// Optional source content to structure into the map (file/OCR/chat text).
    #[serde(default)]
    source: Option<String>,
    /// Replace the parent's existing children instead of appending.
    #[serde(default)]
    replace: bool,
}

/// AI-generate a subtree of ideas under `parent_id` and insert it into the map.
async fn generate(
    State(s): State<Arc<AppState>>,
    Json(b): Json<GenerateBody>,
) -> Result<Json<Value>, ApiError> {
    let parent_text = s.db.node_text(b.parent_id).map_err(bad)?;
    let topic = b
        .topic
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(&parent_text)
        .to_string();
    let path = s.db.ancestor_path(b.parent_id).unwrap_or_default();

    let gen = llm::generate(&topic, &path, b.instruction.as_deref(), b.source.as_deref())
        .await
        .map_err(gateway)?;
    let added = if b.replace {
        s.db.replace_children(b.parent_id, &gen.children, now())
            .map_err(bad)?
    } else {
        s.db.insert_subtree(b.parent_id, &gen.children, now())
            .map_err(bad)?
    };
    // Find the map id via reload so the client can refetch the tree.
    Ok(Json(json!({ "added": added, "model": gen.model })))
}

#[derive(Deserialize)]
struct ChatSendBody {
    /// The session to append to (must already exist).
    session_id: i64,
    content: String,
    #[serde(default)]
    map_outline: Option<String>,
}

/// Send a chat turn: persist the user message, run the LLM over the session's
/// history (grounded in the map outline), persist + return the assistant reply.
async fn chat(
    State(s): State<Arc<AppState>>,
    Json(b): Json<ChatSendBody>,
) -> Result<Json<Value>, ApiError> {
    let content = b.content.trim();
    if content.is_empty() {
        return Err(bad("empty message"));
    }
    // Prior history for context, then append the new user turn.
    let history = s.db.session_messages(b.session_id).map_err(bad)?;
    let mut messages: Vec<llm::ChatMessage> = history
        .into_iter()
        .map(|m| llm::ChatMessage {
            role: m.role,
            content: m.content,
        })
        .collect();
    messages.push(llm::ChatMessage {
        role: "user".into(),
        content: content.to_string(),
    });
    s.db.add_message(b.session_id, "user", content, None, now())
        .map_err(bad)?;

    let body = ChatBody {
        messages,
        map_outline: b.map_outline,
    };
    match llm::chat(&body).await {
        Ok((text, model)) => {
            s.db.add_message(b.session_id, "assistant", &text, Some(&model), now())
                .map_err(bad)?;
            Ok(Json(json!({ "text": text, "model": model })))
        }
        Err(e) => Err(gateway(e)),
    }
}

#[derive(Deserialize)]
struct MapIdQuery {
    map_id: i64,
}

async fn list_sessions(
    State(s): State<Arc<AppState>>,
    Query(q): Query<MapIdQuery>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.db.list_sessions(q.map_id).map_err(bad)?)))
}

#[derive(Deserialize)]
struct CreateSessionBody {
    map_id: i64,
    #[serde(default)]
    title: Option<String>,
}

async fn create_session(
    State(s): State<Arc<AppState>>,
    Json(b): Json<CreateSessionBody>,
) -> Result<Json<Value>, ApiError> {
    let title = b
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .unwrap_or("Hội thoại mới");
    let id = s.db.create_session(b.map_id, title, now()).map_err(bad)?;
    Ok(Json(json!({ "id": id, "title": title })))
}

#[derive(Deserialize)]
struct RenameSessionBody {
    id: i64,
    title: String,
}

async fn rename_session(
    State(s): State<Arc<AppState>>,
    Json(b): Json<RenameSessionBody>,
) -> Result<Json<Value>, ApiError> {
    let title = if b.title.trim().is_empty() {
        "Hội thoại"
    } else {
        b.title.trim()
    };
    s.db.rename_session(b.id, title).map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

async fn delete_session(
    State(s): State<Arc<AppState>>,
    Json(b): Json<NodeIdBody>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_session(b.id).map_err(bad)?;
    Ok(Json(json!({ "success": true })))
}

#[derive(Deserialize)]
struct SessionIdQuery {
    session_id: i64,
}

async fn session_messages(
    State(s): State<Arc<AppState>>,
    Query(q): Query<SessionIdQuery>,
) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s
        .db
        .session_messages(q.session_id)
        .map_err(bad)?)))
}

/// Accept an uploaded file, extract its text (OCR for images via the daemon),
/// and return the text so the client can generate a map from it.
async fn import_file(mut multipart: axum::extract::Multipart) -> Result<Json<Value>, ApiError> {
    let mut filename = String::from("file");
    let mut bytes: Option<Vec<u8>> = None;
    while let Some(field) = multipart.next_field().await.map_err(bad)? {
        if field.name() == Some("file") {
            if let Some(fname) = field.file_name() {
                filename = fname.to_string();
            }
            bytes = Some(field.bytes().await.map_err(bad)?.to_vec());
        }
    }
    let bytes = bytes.ok_or_else(|| bad("no file field"))?;
    if bytes.is_empty() {
        return Err(bad("empty file"));
    }
    let (text, ocr) = crate::ingest::extract_text(&filename, bytes)
        .await
        .map_err(|e| ApiError(StatusCode::BAD_REQUEST, e))?;
    let name = filename.rsplit('/').next().unwrap_or(&filename).to_string();
    Ok(Json(
        json!({ "text": text, "name": name, "chars": text.chars().count(), "ocr": ocr }),
    ))
}

async fn models() -> Result<Json<Value>, ApiError> {
    llm::list_models().await.map(Json).map_err(gateway)
}

#[derive(Deserialize)]
struct ModelActiveBody {
    id: String,
}

async fn model_active(Json(b): Json<ModelActiveBody>) -> Result<Json<Value>, ApiError> {
    llm::set_active_model(&b.id).await.map_err(gateway)?;
    Ok(Json(json!({ "success": true, "activeId": b.id })))
}

/// Which SenClaw LLM the bridge will use (probes the daemon's llm-config).
async fn llm_info() -> Json<Value> {
    let base =
        std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".into());
    let url = format!("{}/api/llm-config", base.trim_end_matches('/'));
    let fetch = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(6))
        .send()
        .await;
    match fetch {
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                let active = v.get("activeId").and_then(|x| x.as_str()).unwrap_or("");
                let cfg = v.get("configs").and_then(|a| a.as_array()).and_then(|a| {
                    a.iter()
                        .find(|c| c.get("id").and_then(|x| x.as_str()) == Some(active))
                });
                let model = cfg
                    .and_then(|c| c.get("modelName"))
                    .and_then(|x| x.as_str());
                Json(json!({ "ok": model.is_some(), "daemon": base, "model": model }))
            }
            Err(e) => Json(json!({ "ok": false, "daemon": base, "error": format!("parse: {e}") })),
        },
        Err(e) => Json(
            json!({ "ok": false, "daemon": base, "error": format!("Không kết nối daemon: {e}") }),
        ),
    }
}

/// serde helper: distinguish `"color": null` (clear) from an absent key.
fn double_option<'de, D>(de: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Some(Option::<String>::deserialize(de)?))
}
