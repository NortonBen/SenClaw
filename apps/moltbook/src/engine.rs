//! The OpenClaw-style heartbeat. On a cadence it reads the feed and, driven by
//! the molty persona, decides what to engage with. In the default **draft** mode
//! nothing is published — every engagement lands in the approval queue. In
//! **live** mode it publishes immediately (with rate-limit guards). In
//! **observe** mode it only refreshes the local feed cache.
//!
//! [`execute_draft`] is the single code path that actually writes to Moltbook —
//! shared by the approve button (draft mode) and the live heartbeat — so the
//! publish semantics (and the anti-human verification handshake) live in one
//! place.

use crate::api::{client, now_ts, voice, AppState};
use crate::db::{CachedPost, Draft, DraftCreate};
use crate::llm::{self, FeedItem};
use crate::moltbook::Moltbook;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

/// Moltbook allows 1 post / 30 minutes. Keep the engine under that.
const POST_COOLDOWN_SECS: i64 = 30 * 60;
/// The heartbeat wakes this often to check whether the cadence has elapsed.
const TICK_SECS: u64 = 60;
/// Never let the configured cadence drop below this (avoid hammering the API).
const MIN_CADENCE_MINUTES: i64 = 5;

/// Spawn the background heartbeat. It is a no-op until the user connects an API
/// key AND enables the heartbeat in Settings.
pub fn spawn_heartbeat(state: Arc<AppState>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(TICK_SECS)).await;
            let db = &state.db;
            if !db.get_bool("heartbeat_enabled", false) || !db.connected() {
                continue;
            }
            let cadence = db.get_i64("heartbeat_minutes", 60).max(MIN_CADENCE_MINUTES);
            let last = db.get_i64("last_heartbeat_at", 0);
            if now_ts() - last < cadence * 60 {
                continue;
            }
            let _ = run_once(&state, "engine").await;
        }
    });
}

/// Run exactly one heartbeat tick now. Returns a summary Value for the UI/MCP.
pub async fn run_once(state: &Arc<AppState>, source: &str) -> Value {
    let db = &state.db;
    let now = now_ts();
    db.set_i64("last_heartbeat_at", now).ok();

    if !db.connected() {
        let msg = "Chưa kết nối agent — thêm API key ở Cài đặt trước.";
        db.log("heartbeat", msg, "", now).ok();
        return json!({ "ok": false, "reason": msg });
    }

    let autonomy = db.autonomy();
    let client = client(db);

    // 1. Pull a feed slice (personalised feed first, global posts as fallback).
    let feed = match fetch_feed(&client).await {
        Ok(f) => f,
        Err(e) => {
            db.log("error", &format!("heartbeat: {e}"), "", now).ok();
            return json!({ "ok": false, "reason": e });
        }
    };
    // Cache for the UI (only real posts — never overwrite with nothing).
    if !feed.is_empty() {
        db.clear_live_cache().ok();
        db.upsert_posts(&feed.iter().map(|f| to_cache(f, now)).collect::<Vec<_>>()).ok();
    }

    if autonomy == "observe" {
        let msg = format!("Quan sát: đã làm mới {} bài trên feed.", feed.len());
        db.log("heartbeat", &msg, source, now).ok();
        return json!({ "ok": true, "mode": "observe", "fetched": feed.len(), "note": msg });
    }

    // 2. Only consider posts we haven't already engaged with.
    let fresh: Vec<&FeedItem> = feed.iter().filter(|f| !db.already_targeting(&f.id)).collect();
    if fresh.is_empty() {
        let msg = "Không có bài mới nào để tương tác.";
        db.log("heartbeat", msg, source, now).ok();
        return json!({ "ok": true, "mode": autonomy, "fetched": feed.len(), "note": msg });
    }

    // 3. Ask the persona to plan engagements within budget.
    let voice = voice(db);
    let budget = db.get_i64("engage_limit", 2).clamp(0, 10);
    let allow_new_post = now - db.get_i64("last_post_at", 0) > POST_COOLDOWN_SECS;
    let default_submolt = db.get_str("default_submolt", "general");
    let items: Vec<FeedItem> = fresh.into_iter().map(clone_item).collect();

    // Moltbook's #1 heartbeat action (per heartbeat.md): reply to molties who
    // replied to YOU. `/home` surfaces that under `activity_on_your_posts`.
    // Best-effort — an old/unauth daemon just yields an empty priority list.
    let priority: Vec<(String, String)> = client
        .home()
        .await
        .ok()
        .map(|h| extract_home_activity(&h))
        .unwrap_or_default()
        .into_iter()
        .filter(|(pid, _)| !db.already_targeting(pid))
        .collect();

    let (plan, model) = match llm::plan_engagements(&voice, &items, &priority, budget, &default_submolt, allow_new_post).await {
        Ok(v) => v,
        Err(e) => {
            db.log("error", &format!("heartbeat plan: {e}"), source, now).ok();
            return json!({ "ok": false, "reason": e });
        }
    };

    let title_of = |id: &str| {
        items
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.title.clone())
            .or_else(|| priority.iter().find(|(p, _)| p == id).map(|(_, s)| crate::llm::truncate(s, 60)))
            .unwrap_or_default()
    };

    // 4. Materialise the plan — as drafts (draft mode) or live writes (live mode).
    let mut drafted = 0usize;
    let mut published = 0usize;
    let mut errors = 0usize;
    let live = autonomy == "live";

    // upvotes
    for pid in &plan.upvotes {
        let d = DraftCreate {
            kind: "vote".into(),
            vote_dir: "up".into(),
            target_post_id: pid.clone(),
            target_title: title_of(pid),
            reason: "heartbeat: bài đáng chú ý".into(),
            source: "engine".into(),
            model: model.clone(),
            ..Default::default()
        };
        apply(state, &d, live, now, &mut drafted, &mut published, &mut errors).await;
    }
    // comments
    for c in &plan.comments {
        let d = DraftCreate {
            kind: "comment".into(),
            target_post_id: c.post_id.clone(),
            target_title: title_of(&c.post_id),
            content: c.content.clone(),
            reason: if c.why.trim().is_empty() { "heartbeat".into() } else { c.why.clone() },
            source: "engine".into(),
            model: model.clone(),
            ..Default::default()
        };
        apply(state, &d, live, now, &mut drafted, &mut published, &mut errors).await;
    }
    // one new post
    if let Some(p) = &plan.new_post {
        if !p.title.trim().is_empty() {
            let d = DraftCreate {
                kind: "post".into(),
                submolt: if p.submolt.trim().is_empty() { default_submolt.clone() } else { p.submolt.trim_start_matches("m/").to_string() },
                title: p.title.clone(),
                content: p.content.clone(),
                reason: if p.why.trim().is_empty() { "heartbeat".into() } else { p.why.clone() },
                source: "engine".into(),
                model: model.clone(),
                ..Default::default()
            };
            apply(state, &d, live, now, &mut drafted, &mut published, &mut errors).await;
        }
    }

    let note = if live {
        format!("Heartbeat (live): đã đăng {published}, lỗi {errors}. {}", plan.note)
    } else {
        format!("Heartbeat (draft): đã soạn {drafted} mục chờ duyệt. {}", plan.note)
    };
    db.log("heartbeat", &note, source, now).ok();
    json!({
        "ok": true,
        "mode": autonomy,
        "fetched": feed.len(),
        "considered": items.len(),
        "replies_to_you": priority.len(),
        "drafted": drafted,
        "published": published,
        "errors": errors,
        "note": note,
        "model": model,
    })
}

/// Draft-or-publish one planned engagement.
async fn apply(
    state: &Arc<AppState>,
    d: &DraftCreate,
    live: bool,
    now: i64,
    drafted: &mut usize,
    published: &mut usize,
    errors: &mut usize,
) {
    let db = &state.db;
    if live {
        // Persist a draft row first so we have something to record the result on.
        match db.create_draft(d, now) {
            Ok(id) => {
                if let Ok(Some(draft)) = db.get_draft(id) {
                    match execute_draft(state, &draft).await {
                        Ok(reference) => {
                            db.set_draft_result(id, "posted", &reference, "", now_ts()).ok();
                            db.log(&draft.kind, &format!("live: {}", describe_draft(&draft)), &reference, now_ts()).ok();
                            *published += 1;
                        }
                        Err(e) => {
                            db.set_draft_result(id, "error", "", &e, now_ts()).ok();
                            db.log("error", &format!("live {}: {e}", draft.kind), "", now_ts()).ok();
                            *errors += 1;
                        }
                    }
                }
            }
            Err(_) => *errors += 1,
        }
    } else {
        match db.create_draft(d, now) {
            Ok(id) => {
                db.log("draft", &format!("soạn {}: {}", d.kind, first_line(&draft_summary(d))), &id.to_string(), now).ok();
                *drafted += 1;
            }
            Err(_) => *errors += 1,
        }
    }
}

/// Execute a queued draft against Moltbook. THE single publish path (approve
/// button + live heartbeat both call this). Returns a reference id on success.
pub async fn execute_draft(state: &Arc<AppState>, draft: &Draft) -> Result<String, String> {
    let db = &state.db;
    let client = client(db);
    if !client.is_authenticated() {
        return Err("chưa cấu hình API key".into());
    }
    match draft.kind.as_str() {
        "vote" => {
            let v = if draft.vote_dir == "down" {
                client.downvote_post(&draft.target_post_id).await
            } else {
                client.upvote_post(&draft.target_post_id).await
            };
            v.map(|_| draft.target_post_id.clone()).map_err(|e| e.to_string())
        }
        "comment" => {
            let parent = if draft.parent_id.is_empty() { None } else { Some(draft.parent_id.as_str()) };
            let v = client
                .create_comment(&draft.target_post_id, &draft.content, parent)
                .await
                .map_err(|e| e.to_string())?;
            Ok(extract_id(&v, "comment").unwrap_or_else(|| draft.target_post_id.clone()))
        }
        "post" => {
            let submolt = if draft.submolt.is_empty() { db.get_str("default_submolt", "general") } else { draft.submolt.clone() };
            let url = if draft.url.is_empty() { None } else { Some(draft.url.as_str()) };
            let reference = create_post_verified(&client, db, &submolt, &draft.title, &draft.content, url).await?;
            db.set_i64("last_post_at", now_ts()).ok();
            Ok(reference)
        }
        "submolt" => {
            let name = if draft.submolt.is_empty() { draft.target_name.clone() } else { draft.submolt.clone() };
            client
                .create_submolt(&name, &draft.title, &draft.content, false)
                .await
                .map(|_| name)
                .map_err(|e| e.to_string())
        }
        "follow" => client.follow(&draft.target_name).await.map(|_| draft.target_name.clone()).map_err(|e| e.to_string()),
        "subscribe" => client.subscribe(&draft.target_name).await.map(|_| draft.target_name.clone()).map_err(|e| e.to_string()),
        other => Err(format!("loại nháp không hỗ trợ: {other}")),
    }
}

/// Create a post and, if Moltbook demands the anti-human math challenge, solve
/// it via the daemon LLM and submit the answer.
async fn create_post_verified(
    client: &Moltbook,
    db: &crate::db::Db,
    submolt: &str,
    title: &str,
    content: &str,
    url: Option<&str>,
) -> Result<String, String> {
    let resp = client.create_post(submolt, title, content, url, "text").await.map_err(|e| e.to_string())?;
    let post = resp.get("post").cloned().unwrap_or(resp.clone());
    let post_id = extract_id(&post, "post").unwrap_or_default();

    let status = post.get("verification_status").and_then(|s| s.as_str()).unwrap_or("");
    if status == "pending" {
        let verification = post.get("verification").cloned().unwrap_or_default();
        let code = verification.get("verification_code").and_then(|s| s.as_str()).unwrap_or("");
        let challenge = verification.get("challenge_text").and_then(|s| s.as_str()).unwrap_or("");
        if code.is_empty() || challenge.is_empty() {
            return Err("bài cần xác minh nhưng thiếu challenge — thử lại sau".into());
        }
        let (answer, _model) = llm::solve_challenge(challenge).await.map_err(|e| format!("giải challenge thất bại: {e}"))?;
        client.verify(code, &answer).await.map_err(|e| format!("nộp đáp án challenge thất bại: {e}"))?;
        db.log("verify", &format!("giải challenge xác minh cho bài '{}' → {}", llm::truncate(title, 60), answer), &post_id, now_ts()).ok();
    }
    Ok(if post_id.is_empty() { "ok".into() } else { post_id })
}

// ---- feed fetch + shaping ----

async fn fetch_feed(client: &Moltbook) -> Result<Vec<FeedItem>, String> {
    // Personalised feed first; fall back to the global hot feed.
    let v = match client.feed("hot", "all", None).await {
        Ok(v) => v,
        Err(e1) => client.posts("hot", None, None).await.map_err(|e2| format!("{e1}; fallback: {e2}"))?,
    };
    Ok(extract_posts(&v))
}

/// Pull the posts array out of any of the feed response shapes Moltbook uses.
pub fn extract_posts(v: &Value) -> Vec<FeedItem> {
    let arr = v
        .get("posts")
        .or_else(|| v.get("results"))
        .or_else(|| v.get("data"))
        .and_then(|p| p.as_array())
        .cloned()
        .unwrap_or_default();
    arr.iter().filter_map(post_item).collect()
}

/// Pull `(post_id, snippet)` pairs from `/home`'s "activity on your posts"
/// section — the replies/comments other molties left on YOUR posts. Tolerant of
/// the exact shape (snake/camelCase, nested `post`), so a newer/older Moltbook
/// response still yields the priority list.
pub fn extract_home_activity(home: &Value) -> Vec<(String, String)> {
    let arr = home
        .get("activity_on_your_posts")
        .or_else(|| home.get("activityOnYourPosts"))
        .or_else(|| home.get("activity"))
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for it in arr.iter().take(10) {
        let pid = it
            .get("post_id")
            .or_else(|| it.get("postId"))
            .or_else(|| it.get("post").and_then(|p| p.get("id")))
            .or_else(|| it.get("id"))
            .and_then(|x| x.as_str().map(String::from).or_else(|| x.as_i64().map(|n| n.to_string())));
        let snippet = it
            .get("content")
            .or_else(|| it.get("comment"))
            .or_else(|| it.get("text"))
            .or_else(|| it.get("body"))
            .or_else(|| it.get("title"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(pid) = pid.filter(|p| !p.is_empty()) {
            out.push((pid, snippet));
        }
    }
    out
}

fn post_item(p: &Value) -> Option<FeedItem> {
    let id = p
        .get("id")
        .and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string())))?;
    let submolt = p
        .get("submolt_name")
        .or_else(|| p.get("submolt"))
        .and_then(|v| v.as_str())
        .map(|s| if s.starts_with("m/") { s.to_string() } else { format!("m/{s}") })
        .unwrap_or_else(|| "m/general".into());
    let author = p
        .get("author")
        .and_then(|a| a.get("name").and_then(|n| n.as_str()).or_else(|| a.as_str()))
        .or_else(|| p.get("author_name").and_then(|v| v.as_str()))
        .unwrap_or("unknown")
        .to_string();
    let title = p.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let content = p
        .get("content")
        .or_else(|| p.get("body"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let score = p.get("score").or_else(|| p.get("upvotes")).and_then(|v| v.as_i64()).unwrap_or(0);
    Some(FeedItem { id, submolt, author, title, content, score })
}

fn to_cache(f: &FeedItem, now: i64) -> CachedPost {
    CachedPost {
        post_id: f.id.clone(),
        submolt: f.submolt.clone(),
        author: f.author.clone(),
        title: f.title.clone(),
        content: f.content.clone(),
        url: String::new(),
        score: f.score,
        comment_count: 0,
        posted_at: now,
        cached_at: now,
        demo: false,
    }
}

fn clone_item(f: &FeedItem) -> FeedItem {
    FeedItem {
        id: f.id.clone(),
        submolt: f.submolt.clone(),
        author: f.author.clone(),
        title: f.title.clone(),
        content: f.content.clone(),
        score: f.score,
    }
}

/// Best-effort id extraction from a create response.
fn extract_id(v: &Value, nested: &str) -> Option<String> {
    v.get("id")
        .or_else(|| v.get(nested).and_then(|n| n.get("id")))
        .and_then(|x| x.as_str().map(String::from).or_else(|| x.as_i64().map(|n| n.to_string())))
}

fn describe_draft(d: &Draft) -> String {
    match d.kind.as_str() {
        "post" => format!("bài '{}' vào m/{}", llm::truncate(&d.title, 60), d.submolt.trim_start_matches("m/")),
        "comment" => format!("bình luận trên '{}'", llm::truncate(&d.target_title, 50)),
        "vote" => format!("{}vote '{}'", if d.vote_dir == "down" { "down" } else { "up" }, llm::truncate(&d.target_title, 50)),
        "submolt" => format!("submolt m/{}", d.submolt),
        "follow" => format!("theo dõi {}", d.target_name),
        "subscribe" => format!("đăng ký m/{}", d.target_name),
        k => k.to_string(),
    }
}

fn draft_summary(d: &DraftCreate) -> String {
    match d.kind.as_str() {
        "post" => format!("{} — {}", d.title, d.content),
        "comment" => d.content.clone(),
        "vote" => d.target_title.clone(),
        _ => d.target_name.clone(),
    }
}

fn first_line(s: &str) -> String {
    llm::truncate(s.lines().next().unwrap_or("").trim(), 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_posts_handles_moltbook_shape() {
        let v = json!({
            "posts": [
                { "id": "p1", "submolt_name": "existential", "author": { "name": "molty-a" },
                  "title": "hi", "content": "world", "score": 9 },
                { "id": 2, "submolt": "m/general", "author_name": "molty-b", "title": "t2", "body": "b2", "upvotes": 3 }
            ]
        });
        let items = extract_posts(&v);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "p1");
        assert_eq!(items[0].submolt, "m/existential");
        assert_eq!(items[0].author, "molty-a");
        assert_eq!(items[1].id, "2");
        assert_eq!(items[1].submolt, "m/general");
        assert_eq!(items[1].author, "molty-b");
        assert_eq!(items[1].content, "b2");
        assert_eq!(items[1].score, 3);
    }

    #[test]
    fn extract_id_from_nested_and_flat() {
        assert_eq!(extract_id(&json!({ "id": "x" }), "post"), Some("x".into()));
        assert_eq!(extract_id(&json!({ "post": { "id": 7 } }), "post"), Some("7".into()));
        assert_eq!(extract_id(&json!({ "nope": 1 }), "post"), None);
    }

    #[test]
    fn extract_home_activity_tolerant() {
        let h = json!({
            "activity_on_your_posts": [
                { "post_id": "p1", "content": "great point, but what about X?" },
                { "post": { "id": 2 }, "comment": "disagree" },
                { "id": "p3", "text": "nice" },
                { "nope": true }
            ]
        });
        let a = extract_home_activity(&h);
        assert_eq!(a.len(), 3);
        assert_eq!(a[0], ("p1".to_string(), "great point, but what about X?".to_string()));
        assert_eq!(a[1].0, "2");
        assert_eq!(a[1].1, "disagree");
        assert_eq!(a[2].0, "p3");
    }
}
