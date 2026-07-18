//! REST for the proactive-selling layer: the lead view over customers, the
//! review queue, escalations, sequences, jobs, the agent action log, and the
//! guardrail settings.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::api::{bad, emit, not_found, now_ts, server, ApiError, AppState};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sale/stats", get(stats))
        .route("/sale/leads", get(list_leads))
        .route("/sale/leads/:id", get(get_lead))
        .route("/sale/leads/:id/next-action", post(next_action))
        .route("/sale/leads/:id/draft", post(draft))
        .route("/sale/leads/:id/stage", post(set_stage))
        .route("/sale/leads/:id/unsubscribe", post(unsubscribe))
        .route("/sale/leads/:id/sequence", post(start_sequence))
        .route("/sale/leads/:id/send", post(send_now))
        .route("/sale/sequences", get(list_sequences))
        .route("/sale/sequences/:key/enabled", post(set_sequence_enabled))
        .route("/sale/reviews", get(list_reviews))
        .route("/sale/reviews/:id/approve", post(approve_review))
        .route("/sale/reviews/:id/reject", post(reject_review))
        .route("/sale/escalations", get(list_escalations))
        .route("/sale/escalations/:id/resolve", post(resolve_escalation))
        .route("/sale/actions", get(list_actions))
        .route("/sale/jobs", get(list_jobs))
        .route("/settings", get(get_settings).post(update_settings))
}

async fn stats(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.db.sale_stats().map_err(server)?))
}

#[derive(Deserialize)]
struct LeadQuery {
    stage: Option<String>,
    temperature: Option<String>,
    q: Option<String>,
    limit: Option<i64>,
}

async fn list_leads(
    State(s): State<Arc<AppState>>,
    Query(q): Query<LeadQuery>,
) -> Result<Json<Value>, ApiError> {
    let leads = s
        .db
        .list_leads(
            q.stage.as_deref(),
            q.temperature.as_deref(),
            q.q.as_deref(),
            q.limit.unwrap_or(200).clamp(1, 500),
        )
        .map_err(server)?;
    Ok(Json(json!({ "leads": leads })))
}

/// Customer 360 through the sales lens: profile + sales state + transcript +
/// reasoning replay + what's scheduled next.
async fn get_lead(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let state = s
        .db
        .sale_state(id)
        .map_err(server)?
        .ok_or_else(|| not_found(format!("customer {id} not found")))?;
    let customer = s.db.get_customer(id).map_err(server)?;
    Ok(Json(json!({
        "lead": state,
        "customer": customer,
        "organizations": s.db.orgs_of_customer(id).map_err(server)?,
        "messages": s.db.recent_messages_of_customer(id, 100).map_err(server)?,
        "actions": s.db.list_actions(Some(id), 50).map_err(server)?,
        "runs": s.db.list_runs(Some(id)).map_err(server)?,
        "jobs": s.db.list_jobs(Some(id), 20).map_err(server)?,
        "reviews": s.db.list_reviews(None, 20).map_err(server)?
            .into_iter().filter(|r| r.customer_id == id).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
struct IntentInput {
    #[serde(default)]
    intent: Option<String>,
    #[serde(default)]
    channel: Option<String>,
}

/// Run one proactive turn: decide, draft, and push it through the guardrail.
/// The response says which of the three things happened — sent, held for
/// review, or blocked — so the UI can report it rather than just claim success.
async fn next_action(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<IntentInput>,
) -> Result<Json<Value>, ApiError> {
    let intent = input.intent.unwrap_or_else(|| "share_value_content".into());
    let out = crate::sale::next_action(
        &s.db,
        &s.events,
        &s.channels,
        id,
        &intent,
        input.channel.as_deref(),
    )
    .await
    .map_err(bad)?;
    Ok(Json(out))
}

/// Draft only — no send, no guardrail decision recorded. The preview affordance.
async fn draft(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<IntentInput>,
) -> Result<Json<Value>, ApiError> {
    let intent = input.intent.unwrap_or_else(|| "share_value_content".into());
    let text = crate::sale::draft_message(&s.db, id, &intent).await.map_err(bad)?;
    Ok(Json(json!({ "draft": text })))
}

#[derive(Deserialize)]
struct SendInput {
    text: String,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    is_reply: bool,
}

/// Send specific words to a lead. Still goes through the guardrail — this is an
/// operator shortcut, not an escape hatch.
async fn send_now(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<SendInput>,
) -> Result<Json<Value>, ApiError> {
    let channel = input.channel.unwrap_or_else(|| "telegram".into());
    let out = crate::sale::send(
        &s.db,
        &s.events,
        &s.channels,
        id,
        &channel,
        &input.text,
        input.is_reply,
        false,
    )
    .await
    .map_err(bad)?;
    Ok(Json(json!({ "outcome": out, "action": out.action(), "detail": out.detail() })))
}

#[derive(Deserialize)]
struct StageInput {
    stage: Option<String>,
    temperature: Option<String>,
    lead_score: Option<i64>,
}

async fn set_stage(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<StageInput>,
) -> Result<Json<Value>, ApiError> {
    s.db.update_sale_stage(
        id,
        input.stage.as_deref(),
        input.temperature.as_deref(),
        input.lead_score,
        now_ts(),
    )
    .map_err(bad)?;
    emit(&s.events, "lead", json!({ "id": id, "action": "stage" }));
    Ok(Json(json!({ "lead": s.db.sale_state(id).map_err(server)? })))
}

#[derive(Deserialize)]
struct UnsubInput {
    #[serde(default = "yes")]
    on: bool,
}
fn yes() -> bool {
    true
}

async fn unsubscribe(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<UnsubInput>,
) -> Result<Json<Value>, ApiError> {
    s.db.set_unsubscribed(id, input.on, now_ts()).map_err(bad)?;
    emit(&s.events, "lead", json!({ "id": id, "action": "unsubscribe" }));
    Ok(Json(json!({ "lead": s.db.sale_state(id).map_err(server)? })))
}

#[derive(Deserialize)]
struct SequenceInput {
    sequence_key: String,
}

async fn start_sequence(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<SequenceInput>,
) -> Result<Json<Value>, ApiError> {
    let run_id = crate::sale::start_sequence(&s.db, &s.events, id, &input.sequence_key)
        .await
        .map_err(bad)?;
    Ok(Json(json!({ "run_id": run_id, "runs": s.db.list_runs(Some(id)).map_err(server)? })))
}

async fn list_sequences(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "sequences": s.db.list_sequences().map_err(server)? })))
}

#[derive(Deserialize)]
struct EnabledInput {
    enabled: bool,
}

async fn set_sequence_enabled(
    State(s): State<Arc<AppState>>,
    Path(key): Path<String>,
    Json(input): Json<EnabledInput>,
) -> Result<Json<Value>, ApiError> {
    s.db.set_sequence_enabled(&key, input.enabled).map_err(not_found)?;
    Ok(Json(json!({ "ok": true })))
}

// ---- review queue ----

#[derive(Deserialize)]
struct StatusQuery {
    status: Option<String>,
    limit: Option<i64>,
    customer_id: Option<i64>,
}

async fn list_reviews(
    State(s): State<Arc<AppState>>,
    Query(q): Query<StatusQuery>,
) -> Result<Json<Value>, ApiError> {
    let status = q.status.unwrap_or_else(|| "pending".into());
    let status = if status == "all" { None } else { Some(status) };
    let reviews = s
        .db
        .list_reviews(status.as_deref(), q.limit.unwrap_or(100).clamp(1, 500))
        .map_err(server)?;
    Ok(Json(json!({ "reviews": reviews })))
}

#[derive(Deserialize)]
struct ApproveInput {
    #[serde(default)]
    edited: Option<String>,
    #[serde(default)]
    by: Option<String>,
}

async fn approve_review(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<ApproveInput>,
) -> Result<Json<Value>, ApiError> {
    let by = input.by.unwrap_or_else(|| "operator".into());
    let out =
        crate::sale::approve_review(&s.db, &s.events, &s.channels, id, input.edited.as_deref(), &by)
            .await
            .map_err(bad)?;
    Ok(Json(out))
}

async fn reject_review(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<ApproveInput>,
) -> Result<Json<Value>, ApiError> {
    let by = input.by.unwrap_or_else(|| "operator".into());
    s.db.resolve_review(id, "rejected", "", &by, now_ts()).map_err(not_found)?;
    emit(&s.events, "review", json!({ "id": id, "action": "rejected" }));
    Ok(Json(json!({ "ok": true })))
}

// ---- escalations ----

async fn list_escalations(
    State(s): State<Arc<AppState>>,
    Query(q): Query<StatusQuery>,
) -> Result<Json<Value>, ApiError> {
    let status = q.status.unwrap_or_else(|| "open".into());
    let status = if status == "all" { None } else { Some(status) };
    let escalations = s
        .db
        .list_escalations(status.as_deref(), q.limit.unwrap_or(100).clamp(1, 500))
        .map_err(server)?;
    Ok(Json(json!({ "escalations": escalations })))
}

async fn resolve_escalation(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<ApproveInput>,
) -> Result<Json<Value>, ApiError> {
    let by = input.by.unwrap_or_else(|| "operator".into());
    s.db.resolve_escalation(id, &by, now_ts()).map_err(not_found)?;
    emit(&s.events, "escalation", json!({ "id": id, "action": "resolved" }));
    Ok(Json(json!({ "ok": true })))
}

// ---- audit ----

async fn list_actions(
    State(s): State<Arc<AppState>>,
    Query(q): Query<StatusQuery>,
) -> Result<Json<Value>, ApiError> {
    let actions = s
        .db
        .list_actions(q.customer_id, q.limit.unwrap_or(100).clamp(1, 500))
        .map_err(server)?;
    Ok(Json(json!({ "actions": actions })))
}

async fn list_jobs(
    State(s): State<Arc<AppState>>,
    Query(q): Query<StatusQuery>,
) -> Result<Json<Value>, ApiError> {
    let jobs = s
        .db
        .list_jobs(q.customer_id, q.limit.unwrap_or(100).clamp(1, 500))
        .map_err(server)?;
    Ok(Json(json!({ "jobs": jobs })))
}

// ---- settings ----

/// The keys the UI is allowed to read and write. An allowlist rather than a dump
/// of the `settings` table, so an internal key can be added without it appearing
/// in a form.
const EXPOSED: &[&str] = &[
    "brand_voice",
    "risky_keywords",
    "complaint_keywords",
    "max_messages_per_customer_24h",
    "auto_welcome",
    "language",
];

async fn get_settings(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let all = s.db.all_settings().map_err(server)?;
    let mut out = serde_json::Map::new();
    for k in EXPOSED {
        if let Some(v) = all.get(*k) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    Ok(Json(Value::Object(out)))
}

async fn update_settings(
    State(s): State<Arc<AppState>>,
    Json(patch): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let obj = patch.as_object().ok_or_else(|| bad("expected a JSON object"))?;
    for (k, v) in obj {
        if !EXPOSED.contains(&k.as_str()) {
            continue;
        }
        let val = match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => if *b { "1".into() } else { "0".into() },
            _ => continue,
        };
        if k == "language" && !["vi", "en"].contains(&val.as_str()) {
            return Err(bad("language must be 'vi' or 'en'"));
        }
        if k == "max_messages_per_customer_24h" && val.parse::<i64>().map(|n| n < 1).unwrap_or(true)
        {
            return Err(bad("max_messages_per_customer_24h must be a positive integer"));
        }
        s.db.set_setting(k, &val).map_err(server)?;
    }
    get_settings(State(s)).await
}
