use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post, put},
    Json, Router,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::{default_data_dir, Db};
use crate::editor::Editor;

pub struct AppState {
    pub db: Arc<Db>,
    /// Broadcasts the raw JSON-RPC responses to any connected MCP SSE clients.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
    /// App events for the web UI (`/api/events` SSE): `diagram:update` etc, so
    /// MCP-driven changes show up live in an open editor.
    pub events_tx: tokio::sync::broadcast::Sender<String>,
    pub editor: Editor,
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
fn not_found(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, e.to_string())
}

pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The app's own base URL (for links returned by MCP tools).
pub fn app_url() -> String {
    let port = std::env::var("PORT").unwrap_or_else(|_| "4610".to_string());
    format!("http://127.0.0.1:{port}")
}

pub fn make_state() -> Arc<AppState> {
    let db_path = default_data_dir("drawio").join("drawio.db");
    let db = Arc::new(Db::open(&db_path).expect("open drawio db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let (events_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState {
        db,
        mcp_tx,
        events_tx,
        editor: Editor::new(),
    })
}

pub fn broadcast_update(state: &AppState, id: i64) {
    let _ = state
        .events_tx
        .send(json!({ "type": "diagram:update", "id": id }).to_string());
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/editor/retry", post(editor_retry))
        .route("/diagrams", get(list_diagrams).post(create_diagram))
        .route("/diagrams/:id", get(get_diagram))
        .route("/diagrams/:id/rename", post(rename_diagram))
        .route("/diagrams/:id/delete", post(delete_diagram))
        .route("/diagrams/:id/xml", put(put_xml))
        .route("/diagrams/:id/svg", put(put_svg))
        .route("/diagrams/:id/export", get(export_diagram))
        .route("/generate", post(generate))
        .route("/edit", post(edit))
        .route("/models", get(models))
        .route("/model-active", post(model_active))
        .route("/events", get(events_sse))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "ok": true, "app": "drawio", "editor": s.editor.status_json() }))
}

async fn editor_retry(State(s): State<Arc<AppState>>) -> Json<Value> {
    crate::editor::spawn_ensure(s.clone());
    Json(json!({ "ok": true, "editor": s.editor.status_json() }))
}

async fn list_diagrams(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!(s.db.list().map_err(bad)?)))
}

/// Valid diagram kinds (a hint for the AI prompt, not an enum the editor cares about).
pub fn norm_kind(k: Option<&str>) -> &'static str {
    match k.unwrap_or("flowchart") {
        "sequence" => "sequence",
        "architecture" => "architecture",
        "er" => "er",
        "state" => "state",
        "class" => "class",
        "org" => "org",
        "network" => "network",
        "bpmn" => "bpmn",
        _ => "flowchart",
    }
}

#[derive(Deserialize)]
struct CreateBody {
    name: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    xml: Option<String>,
}

async fn create_diagram(
    State(s): State<Arc<AppState>>,
    Json(b): Json<CreateBody>,
) -> Result<Json<Value>, ApiError> {
    let name = if b.name.trim().is_empty() {
        "Sơ đồ mới"
    } else {
        b.name.trim()
    };
    let kind = norm_kind(b.kind.as_deref());
    let id =
        s.db.create(name, kind, b.xml.as_deref().unwrap_or(""), now())
            .map_err(bad)?;
    broadcast_update(&s, id);
    Ok(Json(json!({ "id": id, "name": name, "kind": kind })))
}

async fn get_diagram(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let d =
        s.db.get(id)
            .map_err(bad)?
            .ok_or_else(|| not_found(format!("diagram {id} not found")))?;
    Ok(Json(json!(d)))
}

#[derive(Deserialize)]
struct RenameBody {
    name: String,
}

async fn rename_diagram(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<RenameBody>,
) -> Result<Json<Value>, ApiError> {
    if b.name.trim().is_empty() {
        return Err(bad("name is required"));
    }
    s.db.rename(id, b.name.trim(), now()).map_err(not_found)?;
    broadcast_update(&s, id);
    Ok(Json(json!({ "ok": true })))
}

async fn delete_diagram(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete(id).map_err(bad)?;
    let _ = s
        .events_tx
        .send(json!({ "type": "diagram:delete", "id": id }).to_string());
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct XmlBody {
    xml: String,
}

/// Autosave sink for the editor. The UI is the single writer while it is open;
/// we do not broadcast `diagram:update` here to avoid echoing the UI's own
/// writes back at it.
async fn put_xml(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<XmlBody>,
) -> Result<Json<Value>, ApiError> {
    s.db.set_xml(id, &b.xml, now()).map_err(not_found)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SvgBody {
    /// Raw SVG text — the UI decodes the editor's `data:image/svg+xml;base64,…`
    /// export before uploading, so the server never needs a base64 dependency.
    svg: String,
}

async fn put_svg(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(b): Json<SvgBody>,
) -> Result<Json<Value>, ApiError> {
    s.db.set_svg(id, &b.svg, now()).map_err(not_found)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct ExportQuery {
    #[serde(default)]
    format: Option<String>,
}

/// Download a diagram. The host iframe sandbox has no `allow-downloads`, so the
/// editor's own File→Download is unusable — the UI links here instead.
async fn export_diagram(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Query(q): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let d =
        s.db.get(id)
            .map_err(bad)?
            .ok_or_else(|| not_found(format!("diagram {id} not found")))?;
    let safe_name: String = d
        .meta
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    match q.format.as_deref().unwrap_or("xml") {
        "svg" => {
            let (svg, stale) = s.db.get_svg(id).map_err(bad)?.unwrap_or_default();
            if svg.is_empty() {
                return Err(not_found(
                    "no SVG snapshot yet — open the diagram in the editor once",
                ));
            }
            Ok((
                [
                    (header::CONTENT_TYPE, "image/svg+xml".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{safe_name}.svg\""),
                    ),
                    (
                        header::HeaderName::from_static("x-svg-stale"),
                        stale.to_string(),
                    ),
                ],
                svg,
            )
                .into_response())
        }
        _ => Ok((
            [
                (header::CONTENT_TYPE, "application/xml".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{safe_name}.drawio\""),
                ),
            ],
            d.xml,
        )
            .into_response()),
    }
}

#[derive(Deserialize)]
struct GenerateBody {
    prompt: String,
    #[serde(default)]
    kind: Option<String>,
    /// "mermaid" (fast, editor converts client-side) or "xml" (precise).
    #[serde(default)]
    mode: Option<String>,
    /// For the AI log only — the UI applies the result to the editor itself.
    #[serde(default)]
    diagram_id: Option<i64>,
}

/// UI generation endpoint: returns content for the editor to apply (the editor
/// stays the single writer; its autosave persists the converted result).
/// Headless/MCP generation persists server-side instead — see mcp.rs.
async fn generate(
    State(s): State<Arc<AppState>>,
    Json(b): Json<GenerateBody>,
) -> Result<Json<Value>, ApiError> {
    if b.prompt.trim().is_empty() {
        return Err(bad("prompt is required"));
    }
    let kind = norm_kind(b.kind.as_deref());
    let mode = if b.mode.as_deref() == Some("xml") {
        "xml"
    } else {
        "mermaid"
    };
    let diagram_id = b.diagram_id.unwrap_or(0);

    let result = if mode == "xml" {
        crate::llm::generate_xml(b.prompt.trim(), kind)
            .await
            .map(|(c, m)| (c, m))
    } else {
        crate::llm::generate_mermaid(b.prompt.trim(), kind).await
    };
    match result {
        Ok((content, model)) => {
            s.db.log_ai(
                diagram_id,
                b.prompt.trim(),
                mode,
                &model,
                "stop",
                true,
                now(),
            );
            let key = if mode == "xml" { "xml" } else { "mermaid" };
            Ok(Json(json!({ "mode": mode, key: content, "model": model })))
        }
        Err(e) => {
            s.db.log_ai(diagram_id, b.prompt.trim(), mode, "", "error", false, now());
            Err(gateway(e))
        }
    }
}

#[derive(Deserialize)]
struct EditBody {
    #[serde(default)]
    diagram_id: Option<i64>,
    /// Current XML from the live editor (may be ahead of the DB's autosave).
    #[serde(default)]
    xml: Option<String>,
    instruction: String,
}

async fn edit(
    State(s): State<Arc<AppState>>,
    Json(b): Json<EditBody>,
) -> Result<Json<Value>, ApiError> {
    if b.instruction.trim().is_empty() {
        return Err(bad("instruction is required"));
    }
    let current = match (&b.xml, b.diagram_id) {
        (Some(x), _) if !x.trim().is_empty() => x.clone(),
        (_, Some(id)) => {
            s.db.get(id)
                .map_err(bad)?
                .ok_or_else(|| not_found(format!("diagram {id} not found")))?
                .xml
        }
        _ => return Err(bad("xml or diagram_id is required")),
    };
    if current.trim().is_empty() {
        return Err(bad("the diagram is empty — use generate instead"));
    }
    match crate::llm::edit_xml(&current, b.instruction.trim()).await {
        Ok((xml, model)) => {
            s.db.log_ai(
                b.diagram_id.unwrap_or(0),
                b.instruction.trim(),
                "edit",
                &model,
                "stop",
                true,
                now(),
            );
            Ok(Json(json!({ "xml": xml, "model": model })))
        }
        Err(e) => {
            s.db.log_ai(
                b.diagram_id.unwrap_or(0),
                b.instruction.trim(),
                "edit",
                "",
                "error",
                false,
                now(),
            );
            Err(gateway(e))
        }
    }
}

async fn models() -> Result<Json<Value>, ApiError> {
    Ok(Json(crate::llm::list_models().await.map_err(gateway)?))
}

#[derive(Deserialize)]
struct ModelActiveBody {
    id: String,
}

async fn model_active(Json(b): Json<ModelActiveBody>) -> Result<Json<Value>, ApiError> {
    crate::llm::set_active_model(&b.id).await.map_err(gateway)?;
    Ok(Json(json!({ "ok": true })))
}

async fn events_sse(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.events_tx.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("hello").data("{}"));
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().event("message").data(msg));
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}
