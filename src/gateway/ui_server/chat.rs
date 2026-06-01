//! HTTP endpoints that let the mobile app resolve pending agent interactions
//! (tool-permission requests and ask-question batches) over the relay RPC
//! tunnel. The web UI does this over the WS gateway (`permission:response` /
//! `question:response`); these routes expose the same capability via HTTP so
//! the relay bridge can reach it.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::Json};
use serde::Deserialize;

use super::core::{AppError, UiState};

fn agent_api(s: &Arc<UiState>) -> Result<&Arc<dyn super::core::UiApi>, AppError> {
    s.agent_api.as_ref().ok_or_else(|| {
        AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "agent API unavailable".into(),
        )
    })
}

#[derive(Deserialize)]
pub(crate) struct PermissionRespondBody {
    #[serde(rename = "requestId")]
    request_id: String,
    #[serde(rename = "optionKey")]
    option_key: String,
}

pub(crate) async fn chat_permission_respond(
    State(s): State<Arc<UiState>>,
    Json(b): Json<PermissionRespondBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if b.request_id.is_empty() || b.option_key.is_empty() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "requestId and optionKey required".into(),
        ));
    }
    agent_api(&s)?.resolve_permission(&b.request_id, &b.option_key);
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct QuestionRespondBody {
    #[serde(rename = "requestId")]
    request_id: String,
    /// `{ "<questionIndex>": optionIndex | [optionIndex, …] }` (−1 = "Other").
    answers: serde_json::Value,
    #[serde(rename = "otherTexts", default)]
    other_texts: Option<serde_json::Value>,
}

pub(crate) async fn chat_question_respond(
    State(s): State<Arc<UiState>>,
    Json(b): Json<QuestionRespondBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if b.request_id.is_empty() || b.answers.is_null() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "requestId and answers required".into(),
        ));
    }
    agent_api(&s)?.resolve_ask_question(&b.request_id, &b.answers, b.other_texts.as_ref());
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub(crate) struct PlanRespondBody {
    #[serde(rename = "groupJid")]
    group_jid: String,
    #[serde(rename = "agentId", default = "default_agent_id")]
    agent_id: String,
    /// `startEditing` | `clearContextAndStart` | `cancelled`.
    selected: String,
}

fn default_agent_id() -> String {
    "main".to_string()
}

pub(crate) async fn chat_plan_respond(
    State(s): State<Arc<UiState>>,
    Json(b): Json<PlanRespondBody>,
) -> Result<Json<serde_json::Value>, AppError> {
    if b.group_jid.is_empty() || b.selected.is_empty() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "groupJid and selected required".into(),
        ));
    }
    agent_api(&s)?.resolve_plan_exit(&b.group_jid, &b.agent_id, &b.selected);
    Ok(Json(serde_json::json!({ "ok": true })))
}
