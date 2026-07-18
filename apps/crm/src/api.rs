//! HTTP API for the CRM app. All CRUD lives here; the MCP server ([`crate::mcp`])
//! is a thin JSON-RPC front-end that reuses the same [`AppState`] and db calls.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::db::{
    default_data_dir, ChannelCreate, ChannelPatch, Customer, CustomerChannel, CustomerCreate,
    CustomerPatch, Db, Deal, DealCreate, DealPatch, Interaction, Relationship, RelationshipCreate,
    Task, TaskCreate,
};
use crate::llm;

/// Human-readable Vietnamese label for a role slug. Kept in sync with the UI's
/// `ROLE_META` map so the activity log reads the same as the badges.
fn role_label(role: &str) -> &'static str {
    match role {
        "lead" => "Đầu mối",
        "prospect" => "Tiềm năng",
        "customer" => "Khách hàng",
        "vip" => "VIP",
        "contact" => "Người liên hệ",
        "partner" => "Đối tác",
        "referrer" => "Người giới thiệu",
        "supplier" => "Nhà cung cấp",
        "investor" => "Nhà đầu tư",
        "employee" => "Nhân viên",
        "former" => "Khách cũ",
        "paused" => "Tạm dừng",
        "lost" => "Đã mất",
        _ => "khác",
    }
}
fn stage_label(stage: &str) -> &'static str {
    match stage {
        "qualifying" => "Đang xác định",
        "proposal" => "Đã báo giá",
        "negotiation" => "Đàm phán",
        "won" => "Thắng",
        "lost" => "Đã mất",
        _ => "khác",
    }
}

/// Compact money for the activity log (matches UI's short format).
fn fmt_money_short(n: f64, currency: &str) -> String {
    let base = if n.abs() >= 1_000_000_000.0 {
        format!("{:.1}B", n / 1_000_000_000.0)
    } else if n.abs() >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n.abs() >= 1_000.0 {
        format!("{:.0}k", n / 1_000.0)
    } else {
        format!("{n:.0}")
    };
    format!("{base} {currency}")
}

fn empty_or(s: &str) -> String {
    if s.is_empty() { "(trống)".into() } else { s.to_string() }
}

/// Compute a customer diff summary + detail lines. Returns None when nothing changed.
fn diff_customer(old: &Customer, new: &Customer) -> Option<(String, String)> {
    let mut fields: Vec<&str> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    let push = |k: &str, o: &str, n: &str, out_f: &mut Vec<&str>, out_l: &mut Vec<String>| {
        if o != n {
            let field: &'static str = match k {
                "Tên" => "Tên", "Email" => "Email", "Điện thoại" => "Điện thoại",
                "Công ty" => "Công ty", "Chức danh" => "Chức danh", "Vai trò" => "Vai trò",
                "Địa chỉ" => "Địa chỉ", "Sinh nhật" => "Sinh nhật", "Nguồn" => "Nguồn",
                "Ghi chú" => "Ghi chú", "Tags" => "Tags", "Avatar" => "Avatar",
                _ => "khác",
            };
            out_f.push(field);
            out_l.push(format!("- {}: {} → {}", field, empty_or(o), empty_or(n)));
        }
    };
    push("Tên", &old.name, &new.name, &mut fields, &mut lines);
    push("Email", &old.email, &new.email, &mut fields, &mut lines);
    push("Điện thoại", &old.phone, &new.phone, &mut fields, &mut lines);
    push("Công ty", &old.company, &new.company, &mut fields, &mut lines);
    push("Chức danh", &old.title, &new.title, &mut fields, &mut lines);
    if old.role != new.role {
        fields.push("Vai trò");
        lines.push(format!("- Vai trò: {} → {}", role_label(&old.role), role_label(&new.role)));
    }
    push("Địa chỉ", &old.address, &new.address, &mut fields, &mut lines);
    push("Sinh nhật", &old.birthday, &new.birthday, &mut fields, &mut lines);
    push("Nguồn", &old.source, &new.source, &mut fields, &mut lines);
    if old.tags != new.tags {
        fields.push("Tags");
        lines.push(format!("- Tags: [{}] → [{}]", old.tags.join(", "), new.tags.join(", ")));
    }
    if old.notes != new.notes {
        fields.push("Ghi chú");
        // Notes can be long — don't dump them verbatim, just say they changed.
        lines.push("- Ghi chú đã thay đổi".into());
    }
    if old.avatar_url != new.avatar_url {
        fields.push("Avatar");
        lines.push("- Avatar đã đổi".into());
    }
    if fields.is_empty() {
        return None;
    }
    Some((format!("Cập nhật hồ sơ: {}", fields.join(", ")), lines.join("\n")))
}

/// Deal diff. Returns None when nothing changed.
fn diff_deal(old: &Deal, new: &Deal) -> Option<(String, String)> {
    let mut fields: Vec<&str> = Vec::new();
    let mut lines: Vec<String> = Vec::new();
    if old.title != new.title {
        fields.push("Tên");
        lines.push(format!("- Tên: {} → {}", empty_or(&old.title), empty_or(&new.title)));
    }
    if (old.amount - new.amount).abs() > f64::EPSILON || old.currency != new.currency {
        fields.push("Giá trị");
        lines.push(format!(
            "- Giá trị: {} → {}",
            fmt_money_short(old.amount, &old.currency),
            fmt_money_short(new.amount, &new.currency),
        ));
    }
    if old.stage != new.stage {
        fields.push("Giai đoạn");
        lines.push(format!("- Giai đoạn: {} → {}", stage_label(&old.stage), stage_label(&new.stage)));
    }
    if old.probability != new.probability {
        fields.push("Xác suất");
        lines.push(format!("- Xác suất: {}% → {}%", old.probability, new.probability));
    }
    if old.expected_close_at != new.expected_close_at {
        fields.push("Ngày dự kiến");
        lines.push("- Ngày dự kiến đóng deal đã đổi".into());
    }
    if old.notes != new.notes {
        fields.push("Ghi chú");
        lines.push("- Ghi chú deal đã thay đổi".into());
    }
    if fields.is_empty() {
        return None;
    }
    Some((
        format!("Cập nhật deal \"{}\": {}", if new.title.is_empty() { old.title.clone() } else { new.title.clone() }, fields.join(", ")),
        lines.join("\n"),
    ))
}

/// Combine an auto-generated diff detail with the user-provided change note.
fn compose_details(diff: &str, user_note: Option<&str>) -> String {
    match user_note {
        Some(n) if !n.trim().is_empty() => format!("{diff}\n\nGhi chú: {}", n.trim()),
        _ => diff.to_string(),
    }
}

pub struct AppState {
    /// `Arc` because the channel pollers and the sales scheduler hold the same
    /// handle from background tasks; `Db` itself is a mutex-guarded connection.
    pub db: Arc<Db>,
    /// Broadcasts raw JSON-RPC responses to any connected MCP SSE clients.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
    /// Live UI events (new message, review queued, escalation opened) fanned out
    /// over `GET /api/events`. Bigger buffer than `mcp_tx`: a busy inbox can
    /// burst, and a slow browser tab shouldn't drop frames it could still catch up on.
    pub events: tokio::sync::broadcast::Sender<String>,
    pub channels: Arc<crate::channels::ChannelManager>,
}

pub fn make_state() -> Arc<AppState> {
    let data_dir = default_data_dir("crm");
    let db = Arc::new(Db::open(&data_dir.join("crm.db")).expect("failed to open CRM database"));
    // Lazy backfill: if search_index is empty (fresh DB or first upgrade),
    // rebuild it from customers + interactions + mentions + catalogue.
    if db.search_index_empty().unwrap_or(true) {
        let _ = db.reindex_all();
    }
    // A job left `running` by a crash is otherwise stranded forever — nothing
    // else ever revisits that state.
    if let Ok(n) = db.requeue_stuck_jobs() {
        if n > 0 {
            eprintln!("crm: requeued {n} follow-up job(s) stuck in `running`");
        }
    }
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let (events, _) = tokio::sync::broadcast::channel(500);
    let channels = crate::channels::ChannelManager::new(db.clone(), events.clone());
    channels.spawn();
    // The sales engine reaches for the channel manager from inside the inbound
    // path, where it has no state handle to thread it through.
    crate::sale::set_channels(channels.clone());
    crate::sale::spawn_scheduler(db.clone(), events.clone(), channels.clone());
    Arc::new(AppState { db, mcp_tx, events, channels })
}

/// Publish a UI event. Fire-and-forget: `send` errors only when nobody is
/// listening, which is the normal state of a headless daemon.
pub fn emit(events: &tokio::sync::broadcast::Sender<String>, kind: &str, payload: Value) {
    let _ = events.send(json!({ "type": kind, "data": payload }).to_string());
}

pub struct ApiError(pub StatusCode, pub String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
pub fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}
pub fn not_found(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, e.to_string())
}
pub fn server(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

pub fn now_ts() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/stats", get(get_stats))
        .route("/tags", get(get_tags))
        .route("/customers", get(list_customers).post(create_customer))
        .route("/customers/:id", get(get_customer).patch(update_customer).delete(delete_customer))
        .route("/customers/:id/interactions", get(list_interactions).post(add_interaction))
        .route("/customers/:id/deals", get(list_customer_deals).post(create_deal))
        .route("/customers/:id/tasks", get(list_customer_tasks).post(create_task_for_customer))
        .route("/customers/:id/summary", post(post_summary))
        .route("/customers/:id/next-step", post(post_next_step))
        .route("/interactions/:id", delete(delete_interaction))
        .route("/deals", get(list_deals))
        .route("/deals/:id", axum::routing::patch(update_deal).delete(delete_deal))
        .route("/tasks", get(list_tasks).post(create_task))
        .route("/tasks/:id", axum::routing::patch(update_task).delete(delete_task))
        .route("/upcoming", get(get_upcoming))
        .route("/activity", get(get_activity))
        .route("/report", post(post_report))
        .route("/customers/:id/relationships", get(list_customer_relationships))
        .route("/customers/:id/extract", post(post_extract))
        .route("/relationships", get(list_all_relationships).post(create_relationship))
        .route("/relationships/:id", delete(delete_relationship))
        .route("/graph", get(get_graph))
        .route("/graph/path", get(get_graph_path))
        .route("/graph/path_ai", post(post_graph_path_ai))
        .route("/graph/expand", get(get_graph_expand))
        .route("/state/:key", get(get_state).put(put_state).delete(delete_state))
        .route("/customers/:id/similar", get(get_similar))
        .route("/customers/:id/channels", get(list_channels).post(add_channel))
        .route("/channels/:id", axum::routing::patch(update_channel).delete(delete_channel))
        .route("/customers/:id/find_common", post(post_find_common))
        .route("/search", get(get_search))
        .route("/mentions", get(list_mentions))
        .route("/reindex", post(post_reindex))
        .route("/sync/calendar", post(post_sync_calendar))
        .route("/sync/callback", post(post_sync_callback))
        .route("/export.csv", get(export_csv))
        .route("/models", get(get_models).post(post_active_model))
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        // Merged-in surfaces, each kept in its own module so this file stays the
        // core-CRM router rather than a dumping ground.
        .merge(crate::api_org::routes())
        .merge(crate::api_dashboard::routes())
        .merge(crate::api_inbox::routes())
        .merge(crate::api_sale::routes())
        .with_state(state)
}

async fn status() -> Json<Value> {
    Json(json!({ "ok": true, "app": "crm" }))
}

async fn get_stats(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    s.db.stats().map(Json).map_err(server)
}

async fn get_tags(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let tags = s.db.all_tags().map_err(server)?;
    Ok(Json(json!({ "tags": tags })))
}

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    q: Option<String>,
    #[serde(default)]
    tag: Option<String>,
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn list_customers(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Value>, ApiError> {
    let list = s
        .db
        .list_customers(q.q.as_deref(), q.tag.as_deref(), q.role.as_deref(), q.limit.unwrap_or(100))
        .map_err(server)?;
    Ok(Json(json!({ "customers": list, "count": list.len() })))
}

async fn create_customer(
    State(s): State<Arc<AppState>>,
    Json(body): Json<CustomerCreate>,
) -> Result<Json<Customer>, ApiError> {
    let id = s.db.create_customer(&body, now_ts()).map_err(bad)?;
    let c = s.db.get_customer(id).map_err(server)?.ok_or_else(|| server("just-inserted customer vanished"))?;
    // Kick off the welcome sequence, but only for an actual lead and only when
    // the operator has opted in (`auto_welcome`, off by default). Adding a
    // contact to a CRM is a filing action; it must not message them by itself.
    if c.role == "lead" {
        crate::sale::enroll_welcome(&s.db, &s.events, id).await;
    }
    Ok(Json(c))
}

async fn get_customer(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let c = s.db.get_customer(id).map_err(server)?.ok_or_else(|| not_found("customer not found"))?;
    // Include the 20 most recent interactions in the same response so the UI's
    // detail pane can render in one round-trip.
    let interactions = s.db.list_interactions(id, 20).map_err(server)?;
    Ok(Json(json!({ "customer": c, "interactions": interactions })))
}

async fn update_customer(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Customer>, ApiError> {
    // Pluck the optional user-supplied change note before deserializing the
    // patch (unknown fields on CustomerPatch are ignored, but reading it first
    // keeps the intent explicit).
    let change_note = body.get("change_note").and_then(|v| v.as_str()).map(str::to_string);
    let patch: CustomerPatch = serde_json::from_value(body).map_err(bad)?;
    let old = s.db.get_customer(id).map_err(server)?.ok_or_else(|| not_found("customer not found"))?;
    let now = now_ts();
    s.db.update_customer(id, &patch, now).map_err(bad)?;
    let new = s.db.get_customer(id).map_err(server)?.ok_or_else(|| not_found("customer not found"))?;
    // Auto-log a `profile_update` interaction so the change is visible in the
    // activity feed. Silent when nothing actually changed.
    if let Some((summary, diff)) = diff_customer(&old, &new) {
        let details = compose_details(&diff, change_note.as_deref());
        let _ = s.db.add_interaction(id, "profile_update", &summary, &details, now, now);
    }
    Ok(Json(new))
}

async fn delete_customer(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_customer(id).map_err(bad)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn list_interactions(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let list = s.db.list_interactions(id, 200).map_err(server)?;
    Ok(Json(json!({ "interactions": list })))
}

#[derive(Deserialize)]
struct AddInteractionBody {
    #[serde(default = "default_kind")]
    kind: String,
    summary: String,
    #[serde(default)]
    details: String,
    /// Unix seconds; defaults to now.
    #[serde(default)]
    occurred_at: Option<i64>,
}
fn default_kind() -> String {
    "note".into()
}

async fn add_interaction(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<AddInteractionBody>,
) -> Result<Json<Interaction>, ApiError> {
    let now = now_ts();
    let occurred = body.occurred_at.unwrap_or(now);
    let new_id = s
        .db
        .add_interaction(id, &body.kind, &body.summary, &body.details, occurred, now)
        .map_err(bad)?;
    let list = s.db.list_interactions(id, 500).map_err(server)?;
    let created = list
        .into_iter()
        .find(|i| i.id == new_id)
        .ok_or_else(|| server("just-inserted interaction vanished"))?;
    Ok(Json(created))
}

async fn delete_interaction(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_interaction(id, now_ts()).map_err(bad)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn post_summary(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let c = s.db.get_customer(id).map_err(server)?.ok_or_else(|| not_found("customer not found"))?;
    let interactions = s.db.list_interactions(id, 20).map_err(server)?;
    match llm::summarize(&c, &interactions).await {
        Ok((text, model)) => Ok(Json(json!({ "text": text, "model": model }))),
        Err(e) => Err(ApiError(StatusCode::BAD_GATEWAY, e)),
    }
}

async fn post_next_step(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let c = s.db.get_customer(id).map_err(server)?.ok_or_else(|| not_found("customer not found"))?;
    let interactions = s.db.list_interactions(id, 10).map_err(server)?;
    match llm::suggest_next_step(&c, &interactions).await {
        Ok((text, model)) => Ok(Json(json!({ "text": text, "model": model }))),
        Err(e) => Err(ApiError(StatusCode::BAD_GATEWAY, e)),
    }
}

// ---- deals ----

#[derive(Deserialize)]
struct DealsQuery {
    #[serde(default)]
    stage: Option<String>,
}

async fn list_deals(
    State(s): State<Arc<AppState>>,
    Query(q): Query<DealsQuery>,
) -> Result<Json<Value>, ApiError> {
    let deals = s.db.list_deals(q.stage.as_deref()).map_err(server)?;
    Ok(Json(json!({ "count": deals.len(), "deals": deals })))
}

async fn list_customer_deals(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let deals = s.db.deals_of_customer(id).map_err(server)?;
    Ok(Json(json!({ "deals": deals })))
}

#[derive(Deserialize)]
struct DealBody {
    #[serde(default)]
    title: String,
    #[serde(default)]
    amount: f64,
    #[serde(default)]
    currency: String,
    #[serde(default)]
    stage: String,
    #[serde(default)]
    probability: Option<i64>,
    #[serde(default)]
    expected_close_at: Option<i64>,
    #[serde(default)]
    notes: String,
    /// Omit to inherit the contact's primary organization.
    #[serde(default)]
    organization_id: Option<i64>,
    #[serde(default)]
    period_start: Option<i64>,
    #[serde(default)]
    period_end: Option<i64>,
}

async fn create_deal(
    State(s): State<Arc<AppState>>,
    Path(customer_id): Path<i64>,
    Json(b): Json<DealBody>,
) -> Result<Json<Deal>, ApiError> {
    let create = DealCreate {
        customer_id,
        title: b.title,
        amount: b.amount,
        currency: b.currency,
        stage: b.stage,
        probability: b.probability,
        expected_close_at: b.expected_close_at,
        notes: b.notes,
        organization_id: b.organization_id,
        period_start: b.period_start,
        period_end: b.period_end,
    };
    let id = s.db.create_deal(&create, now_ts()).map_err(bad)?;
    let deal = s
        .db
        .list_deals(None)
        .map_err(server)?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| server("deal vanished after insert"))?;
    Ok(Json(deal))
}

async fn update_deal(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let change_note = body.get("change_note").and_then(|v| v.as_str()).map(str::to_string);
    let patch: DealPatch = serde_json::from_value(body).map_err(bad)?;
    let all = s.db.list_deals(None).map_err(server)?;
    let old = all.into_iter().find(|d| d.id == id).ok_or_else(|| not_found("deal not found"))?;
    let now = now_ts();
    s.db.update_deal(id, &patch, now).map_err(bad)?;
    let new = s
        .db
        .list_deals(None)
        .map_err(server)?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| not_found("deal not found"))?;
    if let Some((summary, diff)) = diff_deal(&old, &new) {
        let details = compose_details(&diff, change_note.as_deref());
        let _ = s.db.add_interaction(new.customer_id, "deal_update", &summary, &details, now, now);
    }
    Ok(Json(json!({ "deal": new })))
}

async fn delete_deal(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_deal(id).map_err(bad)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

// ---- tasks ----

#[derive(Deserialize)]
struct TasksQuery {
    #[serde(default)]
    open_only: Option<bool>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn list_tasks(
    State(s): State<Arc<AppState>>,
    Query(q): Query<TasksQuery>,
) -> Result<Json<Value>, ApiError> {
    let tasks = s
        .db
        .list_tasks(q.open_only.unwrap_or(true), q.limit.unwrap_or(200))
        .map_err(server)?;
    Ok(Json(json!({ "tasks": tasks, "count": tasks.len() })))
}

async fn list_customer_tasks(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let tasks = s.db.tasks_of_customer(id).map_err(server)?;
    Ok(Json(json!({ "tasks": tasks })))
}

#[derive(Deserialize)]
struct TaskBody {
    title: String,
    #[serde(default)]
    details: String,
    #[serde(default)]
    due_at: Option<i64>,
}

async fn create_task(
    State(s): State<Arc<AppState>>,
    Json(b): Json<TaskBody>,
) -> Result<Json<Task>, ApiError> {
    create_task_common(&s, None, b).await
}

async fn create_task_for_customer(
    State(s): State<Arc<AppState>>,
    Path(customer_id): Path<i64>,
    Json(b): Json<TaskBody>,
) -> Result<Json<Task>, ApiError> {
    create_task_common(&s, Some(customer_id), b).await
}

async fn create_task_common(
    s: &Arc<AppState>,
    customer_id: Option<i64>,
    b: TaskBody,
) -> Result<Json<Task>, ApiError> {
    let id = s
        .db
        .create_task(
            &TaskCreate {
                customer_id,
                title: b.title,
                details: b.details,
                due_at: b.due_at,
            },
            now_ts(),
        )
        .map_err(bad)?;
    let task = s
        .db
        .list_tasks(false, 1000)
        .map_err(server)?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| server("task vanished after insert"))?;
    Ok(Json(task))
}

#[derive(Deserialize)]
struct TaskPatch {
    #[serde(default)]
    done: Option<bool>,
}

async fn update_task(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(p): Json<TaskPatch>,
) -> Result<Json<Value>, ApiError> {
    if let Some(done) = p.done {
        s.db.set_task_done(id, done, now_ts()).map_err(bad)?;
    }
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn delete_task(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_task(id).map_err(bad)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

// ---- feeds ----

#[derive(Deserialize)]
struct UpcomingQuery {
    #[serde(default)]
    days: Option<i64>,
}

async fn get_upcoming(
    State(s): State<Arc<AppState>>,
    Query(q): Query<UpcomingQuery>,
) -> Result<Json<Value>, ApiError> {
    let up = s.db.upcoming(now_ts(), q.days.unwrap_or(14)).map_err(server)?;
    Ok(Json(up))
}

#[derive(Deserialize)]
struct ActivityQuery {
    #[serde(default)]
    limit: Option<i64>,
}

async fn get_activity(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ActivityQuery>,
) -> Result<Json<Value>, ApiError> {
    let items = s.db.recent_activity(q.limit.unwrap_or(100)).map_err(server)?;
    Ok(Json(json!({ "items": items })))
}

// ---- AI aggregate report ----

async fn post_report(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let stats = s.db.stats().map_err(server)?;
    let top_deals = s.db.top_open_deals(5).map_err(server)?;
    let top_active = s.db.top_active_customers(5).map_err(server)?;
    let recent = s.db.recent_activity(8).map_err(server)?;
    let upcoming = s.db.upcoming(now_ts(), 14).map_err(server)?;
    let overdue = s.db.overdue_tasks(now_ts(), 5).map_err(server)?;
    let snap = llm::ReportSnapshot {
        stats: &stats,
        top_deals: &top_deals,
        top_active_customers: &top_active,
        recent_activity: &recent,
        upcoming: &upcoming,
        overdue_tasks: &overdue,
    };
    match llm::aggregate_report(&snap).await {
        Ok((text, model)) => Ok(Json(json!({
            "text": text,
            "model": model,
            "generated_at": now_ts(),
            "grounding": {
                "customers": stats.get("customers").cloned().unwrap_or(json!(0)),
                "open_deals": stats.get("open_deals").cloned().unwrap_or(json!(0)),
                "pipeline_value": stats.get("pipeline_value").cloned().unwrap_or(json!(0)),
                "top_deals": top_deals.len(),
                "recent_events": recent.len(),
                "overdue_tasks": overdue.len(),
            }
        }))),
        Err(e) => Err(ApiError(StatusCode::BAD_GATEWAY, e)),
    }
}

// ---- relationships ----

async fn list_customer_relationships(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let rels = s.db.relationships_of(id).map_err(server)?;
    Ok(Json(json!({ "relationships": rels })))
}

async fn list_all_relationships(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let rels = s.db.all_relationships().map_err(server)?;
    Ok(Json(json!({ "relationships": rels })))
}

async fn create_relationship(
    State(s): State<Arc<AppState>>,
    Json(body): Json<RelationshipCreate>,
) -> Result<Json<Relationship>, ApiError> {
    let id = s.db.add_relationship(&body, now_ts()).map_err(bad)?;
    let rel = s
        .db
        .all_relationships()
        .map_err(server)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| server("relationship vanished"))?;
    Ok(Json(rel))
}

async fn delete_relationship(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_relationship(id).map_err(bad)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

// ---- graph ----

async fn get_graph(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let nodes = s.db.graph_nodes().map_err(server)?;
    let edges = s.db.all_relationships().map_err(server)?;
    Ok(Json(json!({ "nodes": nodes, "edges": edges })))
}

#[derive(Deserialize)]
struct PathQuery {
    from: i64,
    to: i64,
}
async fn get_graph_path(
    State(s): State<Arc<AppState>>,
    Query(q): Query<PathQuery>,
) -> Result<Json<Value>, ApiError> {
    let path = s.db.find_path(q.from, q.to).map_err(server)?;
    let (nodes, edges) = match &path {
        Some(ids) => {
            // Fetch a rich node projection for the path so the UI can render
            // avatars + roles without another round-trip.
            let all_nodes = s.db.graph_nodes().map_err(server)?;
            let id_set: std::collections::BTreeSet<i64> = ids.iter().copied().collect();
            let filtered: Vec<Value> = all_nodes
                .into_iter()
                .filter(|n| n.get("id").and_then(|v| v.as_i64()).map(|i| id_set.contains(&i)).unwrap_or(false))
                .collect();
            // Only include edges that lie on the path itself.
            let all = s.db.all_relationships().map_err(server)?;
            let mut edge_set: std::collections::HashSet<(i64, i64)> = std::collections::HashSet::new();
            for w in ids.windows(2) {
                edge_set.insert((w[0], w[1]));
                edge_set.insert((w[1], w[0]));
            }
            let e: Vec<crate::db::Relationship> = all
                .into_iter()
                .filter(|r| edge_set.contains(&(r.from_id, r.to_id)))
                .collect();
            (filtered, e)
        }
        None => (Vec::new(), Vec::new()),
    };
    Ok(Json(json!({
        "found": path.is_some(),
        "hops": path.as_ref().map(|p| p.len() as i64 - 1).unwrap_or(-1),
        "path_ids": path,
        "nodes": nodes,
        "edges": edges,
    })))
}

#[derive(Deserialize)]
struct ExpandQuery {
    focus: i64,
    #[serde(default)]
    hops: Option<i64>,
}
async fn get_graph_expand(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ExpandQuery>,
) -> Result<Json<Value>, ApiError> {
    let (nodes, edges) = s.db.subgraph_within(q.focus, q.hops.unwrap_or(1)).map_err(server)?;
    Ok(Json(json!({ "focus": q.focus, "hops": q.hops.unwrap_or(1), "nodes": nodes, "edges": edges })))
}

async fn get_state(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let v = s.db.get_state(&key).map_err(server)?;
    Ok(Json(json!({ "key": key, "value": v })))
}

async fn put_state(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    s.db.set_state(&key, &body, now_ts()).map_err(server)?;
    Ok(Json(json!({ "ok": true, "key": key })))
}

async fn delete_state(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_state(&key).map_err(server)?;
    Ok(Json(json!({ "ok": true, "key": key })))
}

#[derive(Deserialize)]
struct PathAiQuery {
    from: i64,
    to: i64,
}
async fn post_graph_path_ai(
    State(s): State<Arc<AppState>>,
    Query(q): Query<PathAiQuery>,
) -> Result<Json<Value>, ApiError> {
    let a = s.db.get_customer(q.from).map_err(server)?.ok_or_else(|| not_found("from customer not found"))?;
    let b = s.db.get_customer(q.to).map_err(server)?.ok_or_else(|| not_found("to customer not found"))?;
    let a_ctx = s.db.compact_context(q.from).map_err(server)?;
    let b_ctx = s.db.compact_context(q.to).map_err(server)?;
    // BFS path (names) for grounding.
    let path_ids = s.db.find_path(q.from, q.to).map_err(server)?;
    let path_names: Option<Vec<String>> = match path_ids.as_ref() {
        Some(ids) => {
            let all_nodes = s.db.graph_nodes().map_err(server)?;
            let by_id: std::collections::HashMap<i64, String> = all_nodes
                .iter()
                .filter_map(|n| Some((n.get("id")?.as_i64()?, n.get("name")?.as_str()?.to_string())))
                .collect();
            Some(ids.iter().map(|i| by_id.get(i).cloned().unwrap_or_default()).collect())
        }
        None => None,
    };
    match llm::path_ai(&a, &a_ctx, &b, &b_ctx, path_names.as_deref()).await {
        Ok((out, model)) => Ok(Json(json!({
            "from": q.from, "to": q.to, "model": model,
            "summary": out.summary,
            "connections": out.connections.iter().map(|c| json!({
                "type": c.r#type, "detail": c.detail, "strength": c.strength,
            })).collect::<Vec<_>>(),
            "bfs_path_ids": path_ids,
            "bfs_path_names": path_names,
        }))),
        Err(e) => Err(ApiError(StatusCode::BAD_GATEWAY, e)),
    }
}

async fn list_channels(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let list = s.db.list_channels(id).map_err(server)?;
    Ok(Json(json!({ "channels": list })))
}

async fn add_channel(
    State(s): State<Arc<AppState>>,
    Path(customer_id): Path<i64>,
    Json(body): Json<ChannelCreate>,
) -> Result<Json<CustomerChannel>, ApiError> {
    let id = s.db.add_channel(customer_id, &body, now_ts()).map_err(bad)?;
    let ch = s
        .db
        .list_channels(customer_id)
        .map_err(server)?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| server("channel vanished"))?;
    Ok(Json(ch))
}

async fn update_channel(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(patch): Json<ChannelPatch>,
) -> Result<Json<Value>, ApiError> {
    s.db.update_channel(id, &patch, now_ts()).map_err(bad)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn delete_channel(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_channel(id).map_err(bad)?;
    Ok(Json(json!({ "ok": true, "id": id })))
}

async fn post_find_common(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let focus = s.db.get_customer(id).map_err(server)?.ok_or_else(|| not_found("customer not found"))?;
    let focus_ctx = s.db.compact_context(id).map_err(server)?;
    // Collect (id, name, compact_context) for every other customer.
    let others_meta = s.db.list_customers(None, None, None, 2000).map_err(server)?;
    let others: Vec<(i64, String, String)> = others_meta
        .into_iter()
        .filter(|c| c.id != id)
        .map(|c| {
            let ctx = s.db.compact_context(c.id).unwrap_or_default();
            (c.id, c.name, ctx)
        })
        .collect();
    match llm::find_common_themes(id, &focus.name, &focus_ctx, &others).await {
        Ok((themes, model)) => {
            // De-dup highlighted ids across every theme so the UI can pulse them.
            let mut highlight = std::collections::BTreeSet::<i64>::new();
            for t in &themes {
                for cid in &t.customer_ids {
                    highlight.insert(*cid);
                }
            }
            Ok(Json(json!({
                "focus_id": id,
                "model": model,
                "themes": themes.iter().map(|t| json!({
                    "theme": t.theme,
                    "why": t.why,
                    "customer_ids": t.customer_ids,
                })).collect::<Vec<_>>(),
                "highlight_ids": highlight.iter().copied().collect::<Vec<_>>(),
            })))
        }
        Err(e) => Err(ApiError(StatusCode::BAD_GATEWAY, e)),
    }
}

async fn get_similar(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let sim = s.db.similar_customers(id, 8).map_err(bad)?;
    let items: Vec<Value> = sim
        .into_iter()
        .map(|(c, score, reasons)| json!({ "customer": c, "score": score, "reasons": reasons }))
        .collect();
    Ok(Json(json!({ "similar": items, "count": items.len() })))
}

// ---- FTS5 search ----

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    #[serde(default)]
    limit: Option<i64>,
}

async fn get_search(
    State(s): State<Arc<AppState>>,
    Query(qq): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    let hits = s.db.search(&qq.q, qq.limit.unwrap_or(30)).map_err(server)?;
    Ok(Json(json!({ "q": qq.q, "count": hits.len(), "hits": hits })))
}

// ---- extracted mentions ----

#[derive(Deserialize)]
struct MentionsQuery {
    #[serde(default)]
    unresolved_only: Option<bool>,
    #[serde(default)]
    limit: Option<i64>,
}

async fn list_mentions(
    State(s): State<Arc<AppState>>,
    Query(q): Query<MentionsQuery>,
) -> Result<Json<Value>, ApiError> {
    let m = s
        .db
        .list_mentions(q.unresolved_only.unwrap_or(false), q.limit.unwrap_or(100))
        .map_err(server)?;
    Ok(Json(json!({ "mentions": m })))
}

// ---- AI graph extraction ----

async fn post_extract(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let c = s.db.get_customer(id).map_err(server)?.ok_or_else(|| not_found("customer not found"))?;
    let interactions = s.db.list_interactions(id, 30).map_err(server)?;
    let (people, model) = match llm::extract_graph(&c, &interactions).await {
        Ok(v) => v,
        Err(e) => return Err(ApiError(StatusCode::BAD_GATEWAY, e)),
    };
    // Resolve each mention against existing customers by name match (case-
    // insensitive substring OR exact). Auto-materialize a relationship when we
    // find a confident hit; otherwise save as an unresolved mention.
    let now = now_ts();
    let all_customers = s.db.list_customers(None, None, None, 5000).map_err(server)?;
    let mut created_rels = 0;
    let mut created_mentions = 0;
    let mut resolved_hits = Vec::new();
    for p in &people {
        if p.name.trim().is_empty() {
            continue;
        }
        // Best-effort name match — prefer exact (case-insensitive), else the
        // first customer whose name contains all tokens of the extracted name.
        let name_lc = p.name.to_lowercase();
        let resolved = all_customers.iter().find(|c| c.name.to_lowercase() == name_lc).map(|c| c.id).or_else(|| {
            let tokens: Vec<String> = name_lc.split_whitespace().map(str::to_string).collect();
            all_customers
                .iter()
                .find(|c| {
                    let n = c.name.to_lowercase();
                    tokens.iter().all(|t| n.contains(t))
                })
                .map(|c| c.id)
        });
        let kind = if p.kind.trim().is_empty() { "contact_of" } else { p.kind.trim() };
        let role_guess = if p.role_guess.trim().is_empty() { "contact" } else { p.role_guess.trim() };
        s.db.add_mention(id, &p.name, role_guess, kind, &p.context, p.confidence, resolved, now)
            .map_err(bad)?;
        created_mentions += 1;
        if let Some(r) = resolved {
            if r != id {
                created_rels += 1;
                resolved_hits.push(json!({
                    "name": p.name,
                    "resolved_customer_id": r,
                    "kind": kind,
                    "confidence": p.confidence,
                }));
            }
        }
    }
    Ok(Json(json!({
        "model": model,
        "extracted": people.len(),
        "mentions_saved": created_mentions,
        "relationships_created": created_rels,
        "resolved": resolved_hits,
    })))
}

// ---- calendar sync ----

#[derive(Deserialize)]
struct SyncCalendarBody {
    /// Push CRM tasks + upcoming birthdays to the Space Calendar app. Defaults
    /// to true so a bare-body POST does the expected thing.
    #[serde(default = "default_true")]
    space_calendar: bool,
}
fn default_true() -> bool { true }

/// Push open tasks + upcoming birthdays to configured Space App calendars.
/// The Space Apps live in the same daemon on their own ports — luna-calendar
/// on 4351, event-space is a stub for now. Failures per-target are captured in
/// `warnings` so the UI can show a partial-success message.
async fn post_sync_calendar(
    State(s): State<Arc<AppState>>,
    Json(body): Json<SyncCalendarBody>,
) -> Result<Json<Value>, ApiError> {
    sync_calendar_impl(&s.db, body.space_calendar)
        .await
        .map(Json)
        .map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))
}

/// Shared calendar-sync implementation reused by REST + MCP. Actually POSTs
/// the CRM's open tasks + upcoming birthdays to each target Space App's
/// `/api/events/import` endpoint as an idempotent bulk upsert keyed by
/// `external_id` (`crm-task-{id}` / `crm-birthday-{customer_id}`). Returns a
/// JSON value with per-target counts + warnings. Never hard-errors — this is a
/// best-effort push and the caller should surface the warnings.
pub async fn sync_calendar_impl(db: &Db, space_calendar: bool) -> Result<Value, String> {
    let tasks = db.list_tasks(true, 500).map_err(|e| e.to_string())?;
    let upcoming = db.upcoming(now_ts(), 365).map_err(|e| e.to_string())?;
    let birthdays = upcoming.get("birthdays").and_then(|v| v.as_array()).cloned().unwrap_or_default();

    // Build the event batch once — every target gets the same payload.
    let mut events: Vec<Value> = Vec::new();
    for t in &tasks {
        // Only tasks with a due date land on the calendar.
        let Some(due) = t.due_at else { continue };
        let mut desc = String::new();
        if let Some(name) = &t.customer_name {
            desc.push_str(&format!("👤 {}", name));
        }
        if !t.details.trim().is_empty() {
            if !desc.is_empty() { desc.push_str("\n"); }
            desc.push_str(t.details.trim());
        }
        events.push(json!({
            "source": "crm",
            "external_id": format!("crm-task-{}", t.id),
            "title": format!("✅ {}", t.title),
            "description": desc,
            "occurred_at": due,
            "url": format!("http://127.0.0.1:4390/#task-{}", t.id),
            "tags": ["crm", "task"],
        }));
    }
    for b in &birthdays {
        let (Some(cid), Some(name), Some(next_at)) = (
            b.get("customer_id").and_then(|v| v.as_i64()),
            b.get("customer_name").and_then(|v| v.as_str()),
            b.get("next_at").and_then(|v| v.as_i64()),
        ) else { continue };
        events.push(json!({
            "source": "crm",
            "external_id": format!("crm-birthday-{}", cid),
            "title": format!("🎂 Sinh nhật {}", name),
            "description": b.get("birthday").and_then(|v| v.as_str()).unwrap_or_default(),
            "occurred_at": next_at,
            "url": format!("http://127.0.0.1:4390/#customer-{}", cid),
            "tags": ["crm", "birthday"],
        }));
    }

    let mut targets: Vec<&'static str> = Vec::new();
    if space_calendar { targets.push("space-calendar"); }
    if targets.is_empty() {
        return Ok(json!({
            "ok": false,
            "note": "Chưa bật Space Calendar để đồng bộ.",
            "pushed_tasks": 0,
            "pushed_birthdays": 0,
        }));
    }

    let mut warnings: Vec<String> = Vec::new();
    let mut per_target: Vec<Value> = Vec::new();
    let mut pushed_tasks = 0i64;
    let mut pushed_birthdays = 0i64;

    for t in &targets {
        let base = match *t {
            // Placeholder Space App — the "Space Calendar" app is expected to
            // implement POST /api/events/import + PATCH/DELETE /api/events/:src/:id
            // + POST /api/sync/callback for reverse updates. Port is stable so
            // the sync works out of the box once the app is installed.
            "space-calendar" => "http://127.0.0.1:4392",
            _ => continue,
        };
        // Upsert-only (no replace) so events the USER already edited or
        // deleted on the calendar side are preserved. When the CRM user later
        // deletes a task, we fire an explicit /api/events/... DELETE for JUST
        // that external_id (see delete_task_and_notify).
        let payload = json!({
            "source": "crm",
            "replace": false,
            "events": events,
        });
        let client = reqwest::Client::new();
        let res = client
            .post(format!("{base}/api/events/import"))
            .timeout(std::time::Duration::from_secs(4))
            .json(&payload)
            .send()
            .await;
        match res {
            Ok(r) if r.status().is_success() => {
                let body: Value = r.json().await.unwrap_or(json!({}));
                let inserted = body.get("inserted").and_then(|v| v.as_i64()).unwrap_or(0);
                let updated = body.get("updated").and_then(|v| v.as_i64()).unwrap_or(0);
                pushed_tasks += tasks.iter().filter(|t| t.due_at.is_some()).count() as i64;
                pushed_birthdays += birthdays.len() as i64;
                per_target.push(json!({
                    "target": t, "ok": true, "inserted": inserted, "updated": updated,
                }));
            }
            Ok(r) => {
                warnings.push(format!("{t}: HTTP {}", r.status()));
                per_target.push(json!({ "target": t, "ok": false, "http": r.status().as_u16() }));
            }
            Err(_) => {
                warnings.push(format!("{t} không truy cập được (Space App có thể chưa cài hoặc chưa chạy)."));
                per_target.push(json!({ "target": t, "ok": false, "unreachable": true }));
            }
        }
    }

    Ok(json!({
        "ok": true,
        "targets": targets,
        "per_target": per_target,
        "pushed_tasks": pushed_tasks,
        "pushed_birthdays": pushed_birthdays,
        "events_count": events.len(),
        "warnings": warnings,
        "synced_at": now_ts(),
        "note": if warnings.is_empty() { "Đã đồng bộ tất cả target." } else { "Đồng bộ một phần." },
    }))
}

// ---- reverse sync callback (from calendar Space Apps) ----

#[derive(Deserialize)]
struct CallbackBody {
    #[serde(default)]
    action: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    external_id: String,
    #[serde(default)]
    changes: Value,
}

/// Reverse-sync entrypoint: when the user edits or deletes an event on the
/// calendar side (currently luna-calendar), the calendar POSTs here so the
/// change flows back into the CRM. Only events whose `external_id` was minted
/// by the CRM are actionable — everything else is ignored.
async fn post_sync_callback(
    State(s): State<Arc<AppState>>,
    Json(body): Json<CallbackBody>,
) -> Result<Json<Value>, ApiError> {
    let (kind, id) = match parse_external_id(&body.external_id) {
        Some(v) => v,
        None => return Ok(Json(json!({ "ok": false, "reason": "external_id không do CRM tạo" }))),
    };
    let now = now_ts();
    let src = if body.source.is_empty() { "space-calendar".to_string() } else { body.source };

    match (body.action.as_str(), kind) {
        ("delete", "task") => {
            let _ = s.db.delete_task(id);
            // Log the reverse-sync as an interaction so the timeline explains
            // "why is this task gone?".
            let cid = 0; // task has no direct customer link retained after delete
            let _ = cid;
            return Ok(Json(json!({ "ok": true, "action": "deleted_task", "task_id": id })));
        }
        ("delete", "birthday") => {
            return Ok(Json(json!({ "ok": true, "action": "ignored_birthday_delete", "customer_id": id })));
        }
        ("update", "task") => {
            let new_due = body.changes.get("occurred_at").and_then(|v| v.as_i64());
            let new_title = body.changes.get("title").and_then(|v| v.as_str()).map(str::to_string);
            let did_something = new_due.is_some() || new_title.is_some();
            if !did_something {
                return Ok(Json(json!({ "ok": true, "reason": "no supported changes" })));
            }
            // Update by touching the task row directly (there's no partial
            // task-patch endpoint; do it via a mini SQL escape hatch on the DB).
            let ok = s.db.reverse_update_task(id, new_title.as_deref(), new_due, now).is_ok();
            if let Ok(t) = s.db.list_tasks(false, 5000) {
                if let Some(tt) = t.iter().find(|t| t.id == id) {
                    if let Some(cid) = tt.customer_id {
                        // Log an interaction so the CRM timeline shows the
                        // external edit.
                        let summary = format!("Cập nhật task \"{}\" từ {}", tt.title, src);
                        let details = format!("Thay đổi qua callback: {:?}", body.changes);
                        let _ = s.db.add_interaction(cid, "sync_update", &summary, &details, now, now);
                    }
                }
            }
            return Ok(Json(json!({ "ok": ok, "action": "updated_task", "task_id": id })));
        }
        ("update", "birthday") => {
            let new_bday = body.changes.get("title").and_then(|v| v.as_str()).map(str::to_string);
            let new_at = body.changes.get("occurred_at").and_then(|v| v.as_i64());
            if let Some(ts) = new_at {
                // Convert unix ts → YYYY-MM-DD and store on customer.birthday.
                let ymd_bday = unix_to_ymd_string(ts);
                let _ = s.db.update_customer(
                    id,
                    &crate::db::CustomerPatch {
                        birthday: Some(ymd_bday),
                        ..Default::default()
                    },
                    now,
                );
                let _ = new_bday;
                return Ok(Json(json!({ "ok": true, "action": "updated_birthday", "customer_id": id })));
            }
            return Ok(Json(json!({ "ok": false, "reason": "birthday update needs occurred_at" })));
        }
        _ => Ok(Json(json!({ "ok": false, "reason": format!("unknown action/kind: {} / {}", body.action, kind) }))),
    }
}

/// Parse an external_id like `crm-task-42` → (`"task"`, 42). Returns None if
/// the id wasn't minted by the CRM (some third-party events may share the
/// calendar).
fn parse_external_id(s: &str) -> Option<(&'static str, i64)> {
    let s = s.strip_prefix("crm-")?;
    let (kind_s, rest) = s.split_once('-')?;
    let id = rest.parse::<i64>().ok()?;
    let kind: &'static str = match kind_s {
        "task" => "task",
        "birthday" => "birthday",
        _ => return None,
    };
    Some((kind, id))
}

fn unix_to_ymd_string(ts: i64) -> String {
    // UTC+7 date component.
    let secs = ts + 7 * 3600;
    let days = secs.div_euclid(86400);
    let jd = days + 2440588;
    let (y, m, d) = jd_to_ymd_local(jd);
    format!("{y:04}-{m:02}-{d:02}")
}
fn jd_to_ymd_local(jd: i64) -> (i64, i64, i64) {
    let a = jd + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - (146097 * b) / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = 100 * b + d - 4800 + m / 10;
    (year, month, day)
}

// ---- reindex ----

async fn post_reindex(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let n = s.db.reindex_all().map_err(server)?;
    Ok(Json(json!({ "ok": true, "reindexed_customers": n })))
}

// ---- CSV export ----

async fn export_csv(State(s): State<Arc<AppState>>) -> Response {
    match s.db.export_customers_csv() {
        Ok(body) => {
            let filename = format!("crm-customers-{}.csv", now_ts());
            (
                [
                    ("content-type", "text/csv; charset=utf-8".to_string()),
                    ("content-disposition", format!("attachment; filename=\"{filename}\"")),
                ],
                body,
            )
                .into_response()
        }
        Err(e) => ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_models() -> Result<Json<Value>, ApiError> {
    llm::list_models().await.map(Json).map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))
}

#[derive(Deserialize)]
struct SetModelBody {
    id: String,
}
async fn post_active_model(Json(b): Json<SetModelBody>) -> Result<Json<Value>, ApiError> {
    llm::set_active_model(&b.id).await.map_err(|e| ApiError(StatusCode::BAD_GATEWAY, e))?;
    Ok(Json(json!({ "ok": true, "id": b.id })))
}
