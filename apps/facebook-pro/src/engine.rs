//! Heartbeat engine. On a cadence (and on demand via `POST /api/engine/tick`) it
//! reads recent comments on the active Page's recent posts and, per the Page's
//! enabled **rule triggers**, either DRAFTS a reply (LLM-composed) or logs a
//! notification. It never publishes on its own unless the user set
//! `autonomy=live` — and even then only through the same [`crate::api::send_draft`]
//! gate. There is no outbound/broadcast path.
//!
//! Autonomy gate:
//!   * `observe` — do nothing (read-only).
//!   * `draft`   — queue a reply draft per matching comment (default).
//!   * `live`    — queue + publish immediately (same gate).

use crate::api::{self, AppState};
use crate::db::{DraftInput, Trigger};
use serde_json::{json, Value};
use std::time::Duration;

/// How often the background heartbeat runs. Cheap: a few Page reads.
const CADENCE: Duration = Duration::from_secs(180);

/// Does a comment match a trigger's rule? Pure + unit-tested.
pub fn trigger_matches(t: &Trigger, comment: &str) -> bool {
    if !t.enabled {
        return false;
    }
    let text = comment.to_lowercase();
    match t.match_type.as_str() {
        "all" => true,
        "keyword" => t
            .match_value
            .split(',')
            .map(|k| k.trim().to_lowercase())
            .filter(|k| !k.is_empty())
            .any(|k| text.contains(&k)),
        "question" => {
            text.contains('?')
                || ["giá", "bao nhiêu", "ship", "còn hàng", "sao", "thế nào", "khi nào", "ở đâu", "làm sao"]
                    .iter()
                    .any(|q| text.contains(q))
        }
        _ => false,
    }
}

/// Run one pass. Returns a JSON summary (also used by the manual tick route).
pub async fn tick(s: &AppState) -> Value {
    let autonomy = s.db.autonomy();
    if autonomy == "observe" {
        return json!({ "ok": true, "skipped": "autonomy=observe" });
    }
    let Some(page_id) = s.db.active_page_id() else {
        return json!({ "ok": true, "skipped": "chưa chọn Trang" });
    };
    let triggers: Vec<Trigger> = s.db.list_triggers(Some(&page_id)).into_iter().filter(|t| t.enabled && t.event == "new_comment").collect();
    if triggers.is_empty() {
        return json!({ "ok": true, "skipped": "không có trigger new_comment nào bật", "page_id": page_id });
    }

    // Recent posts → their recent comments.
    let posts = api::posts_value(s, Some(&page_id), 10).await;
    if let Some(err) = posts.get("error").and_then(|x| x.as_str()) {
        return json!({ "ok": false, "error": err });
    }
    let post_list = posts.get("data").and_then(|x| x.as_array()).cloned().unwrap_or_default();

    let pending = s.db.pending_targets();
    let page_name = trigger_page_name(s, &page_id);
    let mut drafted = 0;
    let mut notified = 0;
    let mut scanned = 0;

    for post in &post_list {
        let Some(post_id) = post.get("id").and_then(|x| x.as_str()) else {
            continue;
        };
        let comments = api::comments_value(s, post_id, Some(&page_id), 25).await;
        let clist = comments.get("data").and_then(|x| x.as_array()).cloned().unwrap_or_default();
        for c in &clist {
            let Some(cid) = c.get("id").and_then(|x| x.as_str()) else {
                continue;
            };
            // Skip the Page's own comments so we don't reply to ourselves.
            if comment_from_id(c) == Some(page_id.clone()) {
                s.db.mark_comment_seen(cid);
                continue;
            }
            if s.db.is_comment_seen(cid) || pending.contains(cid) {
                continue;
            }
            scanned += 1;
            let text = c.get("message").and_then(|x| x.as_str()).unwrap_or("");
            let Some(t) = triggers.iter().find(|t| trigger_matches(t, text)) else {
                s.db.mark_comment_seen(cid); // no rule cares about it; don't re-scan forever
                continue;
            };
            s.db.mark_comment_seen(cid);
            match t.action.as_str() {
                "notify" => {
                    s.db.log("notify", &format!("[{}] bình luận khớp: {}", t.name, truncate(text, 120)), cid);
                    notified += 1;
                }
                _ => {
                    // draft_reply — compose via LLM using the trigger's hint.
                    let (message, model) = crate::llm::compose_reply(&s.sc, &page_name, text, &t.reply_hint).await;
                    let res = api::enqueue_or_send(
                        s,
                        DraftInput {
                            kind: "reply".into(),
                            page_id: page_id.clone(),
                            target_id: cid.to_string(),
                            message,
                            model,
                            source: format!("trigger:{}", t.name),
                            ..Default::default()
                        },
                    )
                    .await;
                    if res.get("error").is_none() {
                        drafted += 1;
                    }
                }
            }
        }
    }

    if drafted > 0 || notified > 0 {
        s.db.log("heartbeat", &format!("soạn {drafted} nháp trả lời, {notified} thông báo"), &page_id);
    }
    json!({ "ok": true, "page_id": page_id, "scanned": scanned, "drafted": drafted, "notified": notified, "autonomy": autonomy })
}

fn comment_from_id(c: &Value) -> Option<String> {
    c.get("from").and_then(|f| f.get("id")).and_then(|x| x.as_str()).map(|s| s.to_string())
}

fn trigger_page_name(s: &AppState, page_id: &str) -> String {
    s.db
        .list_pages()
        .into_iter()
        .find(|p| p.get("page_id").and_then(|x| x.as_str()) == Some(page_id))
        .and_then(|p| p.get("name").and_then(|x| x.as_str()).map(|x| x.to_string()))
        .unwrap_or_else(|| "Trang".into())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect::<String>() + "…"
}

/// Spawn the background heartbeat. No-op passes are cheap and silent until the
/// user connects, picks a Page, adds a trigger, and leaves autonomy at draft/live.
pub fn spawn_heartbeat(state: AppState) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(CADENCE).await;
            let _ = tick(&state).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Trigger;

    fn trig(match_type: &str, match_value: &str) -> Trigger {
        Trigger {
            id: 1,
            name: "t".into(),
            page_id: "P1".into(),
            event: "new_comment".into(),
            match_type: match_type.into(),
            match_value: match_value.into(),
            action: "draft_reply".into(),
            reply_hint: "".into(),
            enabled: true,
        }
    }

    #[test]
    fn all_matches_everything() {
        assert!(trigger_matches(&trig("all", ""), "bất kỳ nội dung nào"));
    }

    #[test]
    fn keyword_is_case_insensitive_csv() {
        let t = trig("keyword", "giá, ship");
        assert!(trigger_matches(&t, "Cho hỏi GIÁ bao nhiêu ạ"));
        assert!(trigger_matches(&t, "shop có SHIP không"));
        assert!(!trigger_matches(&t, "sản phẩm đẹp quá"));
    }

    #[test]
    fn question_detects_marker_and_words() {
        let t = trig("question", "");
        assert!(trigger_matches(&t, "cái này còn hàng không?"));
        assert!(trigger_matches(&t, "bao nhiêu tiền vậy shop"));
        assert!(!trigger_matches(&t, "đẹp lắm luôn"));
    }

    #[test]
    fn disabled_trigger_never_matches() {
        let mut t = trig("all", "");
        t.enabled = false;
        assert!(!trigger_matches(&t, "gì cũng được"));
    }
}
