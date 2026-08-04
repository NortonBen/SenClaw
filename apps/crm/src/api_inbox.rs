//! REST for the inbox: connected channel accounts, threads, and the live event
//! stream the UI subscribes to.
//!
//! Note the namespace split. `/api/customers/:id/channels` and `/api/channels/:id`
//! (in `api.rs`) are the CONTACT's identities — an email, a handle. Everything
//! here lives under `/api/inbox/*` and refers to OUR connected accounts. Two
//! different things wearing the same word; the URL keeps them apart.

use axum::{
    extract::{Path, Query, State},
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::Arc;

use crate::api::{bad, emit, not_found, now_ts, server, ApiError, AppState};
use crate::db_inbox::{redact_config, ChannelInput, ChannelPatch};

pub fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/inbox/stats", get(stats))
        .route("/inbox/channels", get(list_channels).post(create_channel))
        .route(
            "/inbox/channels/:id",
            axum::routing::patch(update_channel).delete(delete_channel),
        )
        .route("/inbox/channels/:id/test", post(test_channel))
        .route(
            "/inbox/conversations",
            get(list_conversations).post(start_conversation),
        )
        .route("/inbox/conversations/:id", get(get_conversation))
        .route("/inbox/conversations/:id/send", post(send_message))
        .route("/inbox/conversations/:id/link", post(link_conversation))
        .route("/inbox/conversations/:id/status", post(set_status))
        .route("/inbox/conversations/:id/handoff", post(set_handoff))
        .route("/inbox/conversations/:id/read", post(mark_read))
        .route("/events", get(events_sse))
}

// ---- channels (our accounts) ----

async fn stats(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    Ok(Json(s.db.inbox_stats().map_err(server)?))
}

/// Secrets never leave the process in the clear — `redact_config` swaps tokens
/// for a mask, and `merge_config` on the way back in treats the mask as
/// "unchanged" so re-saving a form can't clobber the real value.
async fn list_channels(State(s): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let channels: Vec<Value> =
        s.db.list_channels_all()
            .map_err(server)?
            .into_iter()
            .map(|c| {
                json!({
                    "id": c.id, "kind": c.kind, "name": c.name,
                    "config": redact_config(&c.config),
                    "enabled": c.enabled, "last_sync_at": c.last_sync_at,
                    "last_status": c.last_status, "last_error": c.last_error,
                    "created_at": c.created_at,
                })
            })
            .collect();
    Ok(Json(json!({ "channels": channels })))
}

async fn create_channel(
    State(s): State<Arc<AppState>>,
    Json(input): Json<ChannelInput>,
) -> Result<Json<Value>, ApiError> {
    let id = s.db.create_channel(&input, now_ts()).map_err(bad)?;
    emit(
        &s.events,
        "channel",
        json!({ "id": id, "action": "created" }),
    );
    Ok(Json(json!({ "id": id })))
}

async fn update_channel(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(patch): Json<ChannelPatch>,
) -> Result<Json<Value>, ApiError> {
    s.db.update_channel_cfg(id, &patch).map_err(bad)?;
    emit(
        &s.events,
        "channel",
        json!({ "id": id, "action": "updated" }),
    );
    Ok(Json(json!({ "ok": true })))
}

async fn delete_channel(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.delete_channel_cfg(id).map_err(not_found)?;
    emit(
        &s.events,
        "channel",
        json!({ "id": id, "action": "deleted" }),
    );
    Ok(Json(json!({ "ok": true })))
}

/// Credential health check: poll once and report what the platform said.
async fn test_channel(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let ch =
        s.db.get_channel(id)
            .map_err(server)?
            .ok_or_else(|| not_found(format!("channel {id} not found")))?;
    match s.channels.probe(&ch).await {
        Ok(info) => Ok(Json(json!({ "ok": true, "info": info }))),
        Err(e) => Ok(Json(json!({ "ok": false, "error": e }))),
    }
}

// ---- conversations ----

#[derive(Deserialize)]
struct ConvQuery {
    status: Option<String>,
    kind: Option<String>,
    customer_id: Option<i64>,
    q: Option<String>,
    limit: Option<i64>,
}

async fn list_conversations(
    State(s): State<Arc<AppState>>,
    Query(q): Query<ConvQuery>,
) -> Result<Json<Value>, ApiError> {
    let convs =
        s.db.list_conversations(
            q.status.as_deref(),
            q.kind.as_deref(),
            q.customer_id,
            q.q.as_deref(),
            q.limit.unwrap_or(100).clamp(1, 500),
        )
        .map_err(server)?;
    Ok(Json(json!({ "conversations": convs })))
}

async fn get_conversation(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    let conv =
        s.db.get_conversation(id)
            .map_err(server)?
            .ok_or_else(|| not_found(format!("conversation {id} not found")))?;
    let messages = s.db.list_conv_messages(id, 200).map_err(server)?;
    // The linked profile, when there is one — the operator needs to know who
    // they're talking to without leaving the thread.
    let customer = if conv.customer_id != 0 {
        s.db.get_customer(conv.customer_id).map_err(server)?
    } else {
        None
    };
    Ok(Json(
        json!({ "conversation": conv, "messages": messages, "customer": customer }),
    ))
}

#[derive(Deserialize)]
struct SendInput {
    text: String,
    #[serde(default)]
    by: String,
}

/// Operator reply. Goes out over the channel and is recorded with role
/// `operator`, and takes the thread off the bot as a side effect — a human
/// having typed is the clearest possible signal that they own it now.
async fn send_message(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<SendInput>,
) -> Result<Json<Value>, ApiError> {
    let text = input.text.trim();
    if text.is_empty() {
        return Err(bad("text is required"));
    }
    let conv =
        s.db.get_conversation(id)
            .map_err(server)?
            .ok_or_else(|| not_found(format!("conversation {id} not found")))?;
    s.channels
        .send_to_conversation(&conv, text)
        .await
        .map_err(|e| ApiError(axum::http::StatusCode::BAD_GATEWAY, e))?;
    let now = now_ts();
    let msg_id =
        s.db.add_conv_message(id, "outbound", "operator", text, "sent", now)
            .map_err(server)?;
    if conv.handoff_state == crate::db_inbox::HANDOFF_BOT {
        let _ = s.db.set_handoff(id, crate::db_inbox::HANDOFF_OPERATOR);
    }
    let _ = s.db.mark_conversation_read(id);
    emit(
        &s.events,
        "message",
        json!({ "conversation_id": id, "id": msg_id, "by": input.by }),
    );
    Ok(Json(json!({ "ok": true, "id": msg_id })))
}

#[derive(Deserialize)]
struct StartInput {
    kind: String,
    #[serde(default)]
    channel_id: Option<i64>,
    #[serde(default)]
    customer_id: Option<i64>,
    #[serde(default)]
    external_id: Option<String>,
    #[serde(default)]
    text: Option<String>,
}

/// Open a thread with someone who has never written first — the cold-start path.
/// Resolves the target from the customer's stored identity for that channel kind
/// when `external_id` isn't given outright.
async fn start_conversation(
    State(s): State<Arc<AppState>>,
    Json(input): Json<StartInput>,
) -> Result<Json<Value>, ApiError> {
    let now = now_ts();
    let ch = match input.channel_id {
        Some(cid) => s.db.get_channel(cid).map_err(server)?,
        None => s.db.channel_of_kind(&input.kind).map_err(server)?,
    }
    .ok_or_else(|| bad(format!("no enabled '{}' channel is connected", input.kind)))?;

    let external_id = match input.external_id {
        Some(e) if !e.trim().is_empty() => e.trim().to_string(),
        _ => {
            let cid = input
                .customer_id
                .ok_or_else(|| bad("external_id or customer_id is required"))?;
            s.db.list_channels(cid)
                .map_err(server)?
                .into_iter()
                .find(|c| c.kind == ch.kind)
                .map(|c| c.value)
                .ok_or_else(|| {
                    bad(format!(
                        "customer {cid} has no '{}' identity on file",
                        ch.kind
                    ))
                })?
        }
    };

    let name = input
        .customer_id
        .and_then(|cid| s.db.get_customer(cid).ok().flatten())
        .map(|c| c.name)
        .unwrap_or_default();
    let conv =
        s.db.get_or_create_conversation(ch.id, &ch.kind, &external_id, &name, now)
            .map_err(server)?;
    if let Some(cid) = input.customer_id {
        if conv.customer_id == 0 {
            s.db.link_conversation(conv.id, cid, now).map_err(bad)?;
        }
    }
    // Deliver before persisting: a failed send must not leave a phantom message
    // in a transcript that never reached anyone.
    if let Some(text) = input
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        s.channels
            .send_raw(&ch, &external_id, text)
            .await
            .map_err(|e| ApiError(axum::http::StatusCode::BAD_GATEWAY, e))?;
        s.db.add_conv_message(conv.id, "outbound", "operator", text, "sent", now)
            .map_err(server)?;
    }
    emit(
        &s.events,
        "conversation",
        json!({ "id": conv.id, "action": "created" }),
    );
    let conv = s.db.get_conversation(conv.id).map_err(server)?;
    Ok(Json(json!({ "conversation": conv })))
}

#[derive(Deserialize)]
struct LinkInput {
    customer_id: i64,
}

async fn link_conversation(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<LinkInput>,
) -> Result<Json<Value>, ApiError> {
    s.db.link_conversation(id, input.customer_id, now_ts())
        .map_err(bad)?;
    emit(
        &s.events,
        "conversation",
        json!({ "id": id, "action": "linked" }),
    );
    Ok(Json(
        json!({ "conversation": s.db.get_conversation(id).map_err(server)? }),
    ))
}

#[derive(Deserialize)]
struct StatusInput {
    status: String,
}

async fn set_status(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<StatusInput>,
) -> Result<Json<Value>, ApiError> {
    s.db.set_conversation_status(id, &input.status)
        .map_err(bad)?;
    emit(
        &s.events,
        "conversation",
        json!({ "id": id, "action": "status" }),
    );
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct HandoffInput {
    state: String,
}

async fn set_handoff(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(input): Json<HandoffInput>,
) -> Result<Json<Value>, ApiError> {
    s.db.set_handoff(id, &input.state).map_err(bad)?;
    emit(
        &s.events,
        "handoff",
        json!({ "id": id, "state": input.state }),
    );
    Ok(Json(json!({ "ok": true })))
}

async fn mark_read(
    State(s): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    s.db.mark_conversation_read(id).map_err(server)?;
    Ok(Json(json!({ "ok": true })))
}

// ---- live events ----

/// SSE stream of UI events. Lagged receivers skip ahead rather than dying: a
/// dropped frame just means the client refetches slightly staler data, which is
/// the whole point of using the stream as a refresh trigger rather than as the
/// source of truth.
async fn events_sse(
    State(s): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = s.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(msg) => yield Ok(Event::default().data(msg)),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}
