//! Heartbeat engine. On a cadence (and on demand via `POST /api/engine/tick`)
//! it reads the shop's buyer↔seller conversations and, for each unread buyer
//! message, DRAFTS a customer-service reply. It never sends on its own unless
//! the user set `autonomy=live` — and even then it only replies to that shop's
//! own customers. There is no outbound/broadcast path.
//!
//! Autonomy gate:
//!   * `observe` — do nothing (read-only).
//!   * `draft`   — queue a draft per unread conversation (default).
//!   * `live`    — queue + send immediately (via the same `send_draft` gate).

use crate::api::{self, AppState};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::time::Duration;

/// How often the background heartbeat runs. Cheap: one conversation-list call.
const CADENCE: Duration = Duration::from_secs(180);

/// Run one pass. Returns a JSON summary (also used by the manual tick route).
pub async fn tick(s: &AppState) -> Value {
    let autonomy =
        s.db.get_setting("autonomy")
            .unwrap_or_else(|| "draft".into());
    if autonomy == "observe" {
        return json!({ "ok": true, "skipped": "autonomy=observe" });
    }
    if api::client_from_settings(&s.db).is_none() || api::shop_id(&s.db).is_none() {
        return json!({ "ok": true, "skipped": "chưa kết nối shop" });
    }

    let convs = api::conversations_value(s).await;
    if let Some(err) = convs.get("error").and_then(|x| x.as_str()) {
        return json!({ "ok": false, "error": err });
    }

    // Don't double-draft: skip conversations that already have a pending draft.
    let already: HashSet<String> =
        s.db.list_drafts("pending")
            .into_iter()
            .map(|d| d.conversation_id)
            .collect();

    let list = convs
        .get("conversations")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    let mut drafted = 0;
    for c in &list {
        let unread = c.get("unread_count").and_then(|x| x.as_i64()).unwrap_or(0);
        if unread <= 0 {
            continue; // no new buyer message
        }
        let conversation_id = c
            .get("conversation_id")
            .map(value_to_string)
            .unwrap_or_default();
        if conversation_id.is_empty() || already.contains(&conversation_id) {
            continue;
        }
        let to_id = c.get("to_id").and_then(|x| x.as_i64()).unwrap_or(0);
        let to_name = c
            .get("to_name")
            .and_then(|x| x.as_str())
            .unwrap_or("khách")
            .to_string();
        let customer_msg = c
            .get("latest_message_content")
            .and_then(|m| m.get("text"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();

        let res = api::enqueue_or_send(
            s,
            &conversation_id,
            to_id,
            &to_name,
            None, // let the LLM compose
            &customer_msg,
            "", // context: could be enriched with order lookup later
            "heartbeat",
        )
        .await;
        if res.get("error").is_none() {
            drafted += 1;
        }
    }

    if drafted > 0 {
        s.db.log(
            "heartbeat",
            &format!("soạn {drafted} bản nháp trả lời khách"),
            "",
        );
    }
    json!({ "ok": true, "conversations": list.len(), "drafted": drafted, "autonomy": autonomy })
}

/// Shopee sometimes returns ids as numbers, sometimes strings.
fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

/// Spawn the background heartbeat. No-op passes are cheap and silent until the
/// user connects a shop and leaves autonomy at `draft`/`live`.
pub fn spawn_heartbeat(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(CADENCE).await;
            let _ = tick(&state).await;
        }
    });
}
