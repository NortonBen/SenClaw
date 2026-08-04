//! YouTube domain logic. Reads go through InnerTube (proxied via the Chrome
//! extension); writes are draft-first (stored, then explicitly approved & sent).

use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::db::{CommentIn, Db};
use crate::extbridge::ExtBridge;
use crate::innertube;

/// Forward an InnerTube call to the extension, which issues the real `fetch` from a
/// logged-in youtube.com context (attaching `SAPISIDHASH` auth + BotGuard/PoToken).
///
/// The extension replies (over WS or `POST /api/ext/callback`) with:
///   `{ id, status: "ok" | "error", data: { httpStatus, json }, message? }`
async fn proxy(bridge: &ExtBridge, endpoint: &str, body: Value) -> Result<Value, String> {
    let params = json!({
        "url": innertube::endpoint_url(endpoint),
        "method": "POST",
        "body": body,
    });
    let reply = bridge
        .call("yt_fetch", params, Duration::from_secs(30))
        .await?;
    let data = unwrap_reply(reply, "extension fetch error")?;

    let http = data.get("httpStatus").and_then(|s| s.as_i64()).unwrap_or(0);
    if http != 0 && !(200..300).contains(&http) {
        return Err(format!(
            "YouTube trả HTTP {http} — có thể bị chặn (BotGuard/PoToken) hoặc chưa đăng nhập"
        ));
    }
    data.get("json")
        .cloned()
        .ok_or_else(|| "extension không trả về JSON của YouTube".to_string())
}

/// Unwrap the extension's RPC reply envelope `{ status, data, message }` → `data`.
fn unwrap_reply(reply: Value, default_err: &str) -> Result<Value, String> {
    // A disconnect mid-flight surfaces as `{ error }`.
    if let Some(err) = reply.get("error").and_then(|e| e.as_str()) {
        return Err(err.to_string());
    }
    let status = reply.get("status").and_then(|s| s.as_str()).unwrap_or("ok");
    if status == "error" {
        let msg = reply
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or(default_err);
        return Err(msg.to_string());
    }
    Ok(reply.get("data").cloned().unwrap_or(Value::Null))
}

// ---- UI remote control (for surfaces InnerTube has no API for) ----
//
// The extension drives the page with `chrome.debugger` CDP input, so the events
// are TRUSTED (isTrusted=true) exactly like a human's — synthetic DOM events from
// a content script would be ignored by YouTube's composer.

async fn ui_call(bridge: &ExtBridge, method: &str, params: Value) -> Result<Value, String> {
    let reply = bridge.call(method, params, Duration::from_secs(45)).await?;
    unwrap_reply(reply, "extension UI error")
}

/// Open (or focus) a YouTube/Studio URL in a real tab.
pub async fn ui_open(bridge: &ExtBridge, url: &str) -> Result<Value, String> {
    ui_call(bridge, "yt_ui_open", json!({ "url": url })).await
}

/// Snapshot the active tab's interactive elements, each tagged with an `idx` that
/// `ui_act` targets.
pub async fn ui_snapshot(bridge: &ExtBridge) -> Result<Value, String> {
    ui_call(bridge, "yt_ui_snapshot", json!({})).await
}

/// Act on the page: `click` / `type` / `press` an element by index.
pub async fn ui_act(
    bridge: &ExtBridge,
    action: &str,
    index: Option<i64>,
    text: Option<&str>,
    key: Option<&str>,
) -> Result<Value, String> {
    ui_call(
        bridge,
        "yt_ui_act",
        json!({ "action": action, "index": index, "text": text, "key": key }),
    )
    .await
}

fn s<'a>(e: &'a Value, k: &str) -> &'a str {
    e.get(k).and_then(|x| x.as_str()).unwrap_or("")
}

/// Anything that looks like a site search box rather than a content editor.
/// Typing a post into YouTube's search bar is the classic failure of naive
/// "first textbox" matching — the search input sits above the composer in the DOM.
fn is_search_box(e: &Value) -> bool {
    let hay = format!("{} {} {}", s(e, "name"), s(e, "label"), s(e, "type")).to_lowercase();
    hay.contains("search") || hay.contains("tìm kiếm")
}

/// The composer text field: the LARGEST editable element that isn't a search box.
/// Area beats DOM order — a post composer is a big box, chrome inputs are small.
fn find_editor(snapshot: &Value) -> Option<i64> {
    let els = snapshot.get("elements")?.as_array()?;
    els.iter()
        .filter(|e| e.get("editable").and_then(|x| x.as_bool()).unwrap_or(false))
        .filter(|e| !is_search_box(e))
        .max_by_key(|e| e.get("area").and_then(|x| x.as_i64()).unwrap_or(0))
        .and_then(|e| e.get("idx").and_then(|x| x.as_i64()))
}

/// A clickable control whose *whole* label equals one of `labels` (after trimming).
/// Deliberately NOT a substring match and never a link (`<a>`): the Studio sidebar
/// has a "Posts" nav link that a substring/link-tolerant match would click,
/// navigating away and discarding the drafted text.
fn find_button(snapshot: &Value, labels: &[&str], allow_links: bool) -> Option<i64> {
    let els = snapshot.get("elements")?.as_array()?;
    els.iter()
        .filter(|e| {
            e.get("clickable")
                .and_then(|x| x.as_bool())
                .unwrap_or(false)
        })
        .filter(|e| allow_links || s(e, "tag") != "a")
        .find(|e| {
            let text = s(e, "text").trim().to_lowercase();
            let label = s(e, "label").trim().to_lowercase();
            labels.iter().any(|l| {
                let l = l.to_lowercase();
                text == l || label == l
            })
        })
        .and_then(|e| e.get("idx").and_then(|x| x.as_i64()))
}

/// Post a community post by driving the Studio/channel composer UI. There is no
/// stable InnerTube endpoint for this, so we automate the real composer with
/// trusted input. Returns a step trace so a failure is debuggable; when a step
/// can't find its target the agent can finish the job with the generic
/// `youtube_ui_snapshot` / `youtube_ui_act` tools.
pub async fn post_community(
    bridge: &ExtBridge,
    text: &str,
    composer_url: &str,
) -> Result<Value, String> {
    let mut trace: Vec<Value> = Vec::new();

    let opened = ui_open(bridge, composer_url).await?;
    trace.push(json!({ "step": "open", "url": composer_url, "result": opened }));

    // 1) Open the composer if it starts collapsed behind a "Create post" control.
    //    Exact-label + button-only so the sidebar "Posts" link can't be hit.
    let snap = ui_snapshot(bridge).await?;
    if let Some(i) = find_button(
        &snap,
        &["create post", "tạo bài viết", "new post", "tạo bài đăng"],
        false,
    ) {
        let r = ui_act(bridge, "click", Some(i), None, None).await?;
        trace.push(json!({ "step": "open_composer", "index": i, "result": r }));
    }

    // 2) Type into the composer — the largest non-search editable box.
    let snap = ui_snapshot(bridge).await?;
    let editor = find_editor(&snap).ok_or_else(|| {
        format!(
            "không tìm thấy ô soạn bài viết trên {composer_url}. \
             Dùng youtube_ui_snapshot + youtube_ui_act để thao tác thủ công."
        )
    })?;
    let r = ui_act(bridge, "type", Some(editor), Some(text), None).await?;
    trace.push(json!({ "step": "type", "index": editor, "result": r }));

    // 3) Submit.
    let snap = ui_snapshot(bridge).await?;
    let post_btn =
        find_button(&snap, &["post", "đăng", "publish", "xuất bản"], false).ok_or_else(|| {
            "không tìm thấy nút Đăng — nội dung ĐÃ được nhập vào ô soạn, hãy dùng \
         youtube_ui_snapshot/act để bấm gửi thủ công"
                .to_string()
        })?;
    let r = ui_act(bridge, "click", Some(post_btn), None, None).await?;
    trace.push(json!({ "step": "submit", "index": post_btn, "result": r }));

    // 4) Best-effort verification: after a successful post the composer clears /
    //    closes. We report what we saw instead of claiming success blindly.
    let verified = match ui_snapshot(bridge).await {
        Ok(after) => {
            let editor_gone_or_empty = find_editor(&after)
                .map(|i| {
                    after
                        .get("elements")
                        .and_then(|e| e.as_array())
                        .and_then(|els| {
                            els.iter()
                                .find(|e| e.get("idx").and_then(|x| x.as_i64()) == Some(i))
                        })
                        .map(|e| s(e, "text").trim().is_empty())
                        .unwrap_or(false)
                })
                .unwrap_or(true); // no editor at all = composer closed
            trace.push(json!({ "step": "verify", "editorClearedOrClosed": editor_gone_or_empty }));
            editor_gone_or_empty
        }
        Err(e) => {
            trace.push(json!({ "step": "verify", "error": e }));
            false
        }
    };

    // Drop the debugger attachment so Chrome stops showing the debugging banner.
    let _ = ui_call(bridge, "yt_ui_release", json!({})).await;

    Ok(json!({
        "submitted": true,
        "verified": verified,
        "kind": "community_post",
        "trace": trace
    }))
}

/// App + extension + auth status. Cheap; works even with no extension connected.
pub fn status(bridge: &ExtBridge, db: &Db) -> Value {
    json!({
        "app": "youtube",
        "extensionConnected": bridge.is_connected(),
        "bridge": bridge.stats(),
        "auth": db.auth_snapshot(),
    })
}

/// Search YouTube. Returns a light `{ items, count }` plus the raw response for
/// callers that want the full nested shape.
pub async fn search(bridge: &ExtBridge, query: &str) -> Result<Value, String> {
    let raw = proxy(bridge, "search", innertube::search_body(query)).await?;
    let items = innertube::parse_videos(&raw);
    Ok(json!({ "query": query, "count": items.len(), "items": items }))
}

/// Browse a channel by id (`UC…`). `params` optionally selects a tab (e.g. the
/// community-tab token) — omit for the channel home.
pub async fn browse(
    bridge: &ExtBridge,
    browse_id: &str,
    params: Option<&str>,
) -> Result<Value, String> {
    let raw = proxy(bridge, "browse", innertube::browse_body(browse_id, params)).await?;
    let videos = innertube::parse_videos(&raw);
    let posts = innertube::parse_posts(&raw);
    Ok(json!({
        "browseId": browse_id,
        "videos": videos,
        "posts": posts,
    }))
}

/// List comments given a comment-section continuation token (page-by-page).
pub async fn comments(bridge: &ExtBridge, continuation: &str) -> Result<Value, String> {
    let raw = proxy(bridge, "next", innertube::continuation_body(continuation)).await?;
    let items = innertube::parse_comments(&raw);
    Ok(json!({ "count": items.len(), "comments": items }))
}

/// List a video's comments: derive the comment-section token from the watch page,
/// then page it.
pub async fn comments_for_video(bridge: &ExtBridge, video_id: &str) -> Result<Value, String> {
    let n1 = proxy(
        bridge,
        "next",
        json!({ "context": innertube::client_context(), "videoId": video_id }),
    )
    .await?;
    let token = innertube::find_comment_section_token(&n1).ok_or_else(|| {
        "không tìm thấy phần bình luận (video tắt bình luận hoặc chưa đăng nhập)".to_string()
    })?;
    comments(bridge, &token).await
}

/// Pull a video's comments through InnerTube and cache them into the DB (up to
/// `max_pages` continuation pages). Returns `{ videoId, fetched, new, pages }` plus
/// the DB counts — this cache is the foundation for analytics (P7) and a CRM
/// pull-feed (P9), neither of which can run off a live-only fetch.
pub async fn sync_comments(
    bridge: &ExtBridge,
    db: &Db,
    video_id: &str,
    max_pages: u32,
    now: i64,
) -> Result<Value, String> {
    // Locate the comment section, then page it.
    let n1 = proxy(
        bridge,
        "next",
        json!({ "context": innertube::client_context(), "videoId": video_id }),
    )
    .await?;
    let mut token = innertube::find_comment_section_token(&n1).ok_or_else(|| {
        "không tìm thấy phần bình luận (video tắt bình luận hoặc chưa đăng nhập)".to_string()
    })?;

    let mut fetched = 0usize;
    let mut new = 0usize;
    let mut pages = 0u32;
    let cap = max_pages.max(1);

    while pages < cap {
        let page = proxy(bridge, "next", innertube::continuation_body(&token)).await?;
        for c in innertube::parse_comments(&page) {
            let row = comment_in(video_id, &c);
            if row.id.is_empty() {
                continue;
            }
            fetched += 1;
            if db.upsert_comment(&row, now).map_err(|e| e.to_string())? {
                new += 1;
            }
        }
        pages += 1;
        match innertube::find_next_continuation(&page) {
            Some(next) if next != token => token = next,
            _ => break, // no further page (or the same token → avoid a loop)
        }
    }

    db.log("sync_comments", video_id, now);
    Ok(json!({
        "videoId": video_id,
        "fetched": fetched,
        "new": new,
        "pages": pages,
        "counts": db.comment_counts(video_id).map_err(|e| e.to_string())?,
    }))
}

/// Map a parsed comment `Value` (from `innertube::parse_comments`) to a DB row.
fn comment_in(video_id: &str, c: &Value) -> CommentIn {
    let get_str = |k: &str| c.get(k).and_then(|x| x.as_str()).map(|s| s.to_string());
    // Bundle whatever action tokens we captured; None when we got nothing usable.
    let mut tokens = serde_json::Map::new();
    for (out_key, in_key) in [
        ("heart", "heartToken"),
        ("like", "likeToken"),
        ("pin", "pinToken"),
        ("remove", "removeToken"),
        ("report", "reportToken"),
    ] {
        if let Some(v) = c.get(in_key).and_then(|x| x.as_str()) {
            tokens.insert(out_key.into(), Value::String(v.to_string()));
        }
    }
    CommentIn {
        id: c
            .get("commentId")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        video_id: video_id.to_string(),
        parent_id: get_str("parentId"),
        author: get_str("author").unwrap_or_default(),
        author_channel: get_str("authorChannel"),
        text: get_str("text").unwrap_or_default(),
        like_count: c.get("likeCount").and_then(|x| x.as_i64()),
        reply_count: c.get("replyCount").and_then(|x| x.as_i64()),
        published: get_str("published"),
        reply_params: get_str("replyParams"),
        tokens_json: if tokens.is_empty() {
            None
        } else {
            Some(Value::Object(tokens).to_string())
        },
    }
}

/// P7 — analyse cached-but-unanalysed comments in batches via the LLM, writing
/// sentiment/intent/topic/spam into `comment_analysis`. Bounded by `max`.
pub async fn analyze_pending(db: &Db, max: usize, now: i64) -> Result<Value, String> {
    let mut analyzed = 0usize;
    let mut model = String::new();
    let batch_size = 15usize;

    while analyzed < max {
        let want = batch_size.min(max - analyzed) as i64;
        let batch = db.unanalyzed_comments(want).map_err(|e| e.to_string())?;
        if batch.is_empty() {
            break;
        }
        let (results, m) = crate::llm::analyze_batch(&batch).await?;
        if !m.is_empty() {
            model = m;
        }
        for (id, _text) in &batch {
            match results.iter().find(|a| &a.id == id) {
                Some(a) => {
                    let topics = serde_json::to_string(&a.topics).unwrap_or_else(|_| "[]".into());
                    db.save_analysis(
                        id,
                        &a.sentiment,
                        a.sentiment_score,
                        &a.intent,
                        &topics,
                        &a.lang,
                        a.is_spam,
                        a.toxicity,
                        &model,
                        now,
                    )
                    .map_err(|e| e.to_string())?;
                }
                // Model dropped this id → store a neutral placeholder so we don't
                // re-fetch it forever.
                None => {
                    db.save_analysis(id, "neu", 0.0, "other", "[]", "", false, 0.0, &model, now)
                        .map_err(|e| e.to_string())?;
                }
            }
            analyzed += 1;
        }
    }
    db.log("analyze", &analyzed.to_string(), now);
    Ok(json!({ "analyzed": analyzed, "model": model }))
}

/// P8 + moderation — perform a comment action using the token captured at sync
/// time: `heart`/`like`/`pin` (reversible) or `remove`/`report` (destructive, and
/// therefore gated behind `confirm=true`). Tokens are per-session and expire, so
/// sync close to the action.
pub async fn comment_action(
    bridge: &ExtBridge,
    db: &Db,
    comment_id: &str,
    action: &str,
    confirm: bool,
) -> Result<Value, String> {
    if !matches!(action, "heart" | "like" | "pin" | "remove" | "report") {
        return Err("action phải là heart | like | pin | remove | report".to_string());
    }
    let destructive = matches!(action, "remove" | "report");
    if destructive && !confirm {
        return Err(format!(
            "hành động '{action}' không thể hoàn tác — cần confirm=true"
        ));
    }
    let tokens = db
        .tokens_of(comment_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "chưa có action-token cho comment này — chạy youtube_sync_comments trước".to_string()
        })?;
    let token = tokens.get(action).and_then(|x| x.as_str()).ok_or_else(|| {
        format!(
            "YouTube không cấp token '{action}' cho comment này (chỉ chủ kênh mới có remove/pin)"
        )
    })?;

    throttle_write().await;
    let body = json!({ "context": innertube::client_context(), "actions": [token] });
    let resp = proxy(bridge, "comment/perform_comment_action", body).await?;
    if innertube::action_succeeded(&resp) {
        db.log(
            "comment_action",
            &format!("{action}:{comment_id}"),
            crate::api::now(),
        );
        Ok(json!({ "ok": true, "action": action, "commentId": comment_id }))
    } else {
        Err("YouTube không xác nhận hành động (token có thể đã hết hạn — sync lại)".to_string())
    }
}

/// P10 — index a video's cached comments into the app's private knowledge space
/// (`youtube-comments`) so drafts can recall context / build an FAQ.
pub async fn index_comments(db: &Db, video_id: &str, limit: i64) -> Result<Value, String> {
    let rows = db
        .list_comments(video_id, limit)
        .map_err(|e| e.to_string())?;
    let items: Vec<(String, String)> = rows
        .into_iter()
        .filter(|r| !r.text.trim().is_empty())
        .map(|r| (r.author, r.text))
        .collect();
    let n = crate::llm::knowledge_index(&items, video_id).await?;
    Ok(json!({ "indexed": n, "space": "youtube-comments" }))
}

/// Scrape the `createCommentParams` token needed to POST a top-level comment on a
/// video. It lives on the comment simplebox, which may only appear once the comment
/// section is loaded — so try the watch page first, then the comment continuation.
async fn create_comment_params(bridge: &ExtBridge, video_id: &str) -> Result<String, String> {
    let n1 = proxy(
        bridge,
        "next",
        json!({ "context": innertube::client_context(), "videoId": video_id }),
    )
    .await?;
    if let Some(p) = innertube::find_key_str(&n1, "createCommentParams") {
        return Ok(p);
    }
    if let Some(tok) = innertube::find_comment_section_token(&n1) {
        let n2 = proxy(bridge, "next", innertube::continuation_body(&tok)).await?;
        if let Some(p) = innertube::find_key_str(&n2, "createCommentParams") {
            return Ok(p);
        }
    }
    Err(
        "không lấy được createCommentParams — video có thể tắt bình luận hoặc phiên chưa đăng nhập"
            .to_string(),
    )
}

/// Minimum gap between two WRITE actions. Automated posting at machine speed is
/// the fastest way to get flagged as spam, so every write waits out the remainder
/// of this window (plus a little jitter) before firing.
#[cfg(not(test))]
const MIN_WRITE_GAP: Duration = Duration::from_secs(30);
#[cfg(test)] // no artificial delay in the fast test suite
const MIN_WRITE_GAP: Duration = Duration::from_millis(0);
static LAST_WRITE: Mutex<Option<Instant>> = Mutex::new(None);

/// Sleep until the write-rate window has elapsed, then claim the slot.
async fn throttle_write() {
    let wait = {
        let mut last = LAST_WRITE.lock().unwrap();
        let wait = match *last {
            Some(t) => MIN_WRITE_GAP.checked_sub(t.elapsed()).unwrap_or_default(),
            None => Duration::ZERO,
        };
        *last = Some(Instant::now() + wait);
        wait
    };
    if !wait.is_zero() {
        // Jitter 0-3s so the cadence isn't machine-regular.
        let jitter = Duration::from_millis(
            (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos() as u64)
                .unwrap_or(0))
                % 3000,
        );
        tokio::time::sleep(wait + jitter).await;
    }
}

/// Send an APPROVED write draft. Reads go straight through; writes hit the
/// InnerTube `comment/*` action endpoints via the extension (or, for community
/// posts, drive the real composer UI). The human approval gate (only approved
/// drafts reach here) plus `throttle_write` are the safety mechanisms.
pub async fn send_action(
    bridge: &ExtBridge,
    kind: &str,
    target: &str,
    body: &str,
) -> Result<Value, String> {
    let target = target.trim();
    match kind {
        "comment" => {
            if target.is_empty() {
                return Err("comment cần `target` = videoId".to_string());
            }
            let params = create_comment_params(bridge, target).await?;
            throttle_write().await;
            let resp = proxy(
                bridge,
                "comment/create_comment",
                innertube::comment_create_body(&params, body),
            )
            .await?;
            if innertube::action_succeeded(&resp) {
                Ok(json!({ "submitted": true, "kind": "comment", "target": target }))
            } else {
                Err("YouTube không xác nhận đã đăng bình luận (có thể bị lọc spam / chưa đăng nhập)".to_string())
            }
        }
        "reply" => {
            if target.is_empty() {
                return Err(
                    "reply cần `target` = createReplyParams token (lấy từ youtube_list_comments)"
                        .to_string(),
                );
            }
            throttle_write().await;
            let resp = proxy(
                bridge,
                "comment/create_comment_reply",
                innertube::comment_reply_body(target, body),
            )
            .await?;
            if innertube::action_succeeded(&resp) {
                Ok(json!({ "submitted": true, "kind": "reply" }))
            } else {
                Err("YouTube không xác nhận đã đăng trả lời".to_string())
            }
        }
        "community_post" => {
            // `target` may be a full composer URL or a channel id; default to Studio.
            let url = if target.starts_with("http") {
                target.to_string()
            } else if target.is_empty() {
                "https://studio.youtube.com/".to_string()
            } else {
                format!("https://studio.youtube.com/channel/{target}/posts")
            };
            throttle_write().await;
            post_community(bridge, body, &url).await
        }
        other => Err(format!("kind không hợp lệ: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::sync::mpsc::UnboundedReceiver;

    /// Read the next outbound RPC the code under test sent to the (fake) extension.
    async fn next_rpc(rx: &mut UnboundedReceiver<String>) -> Value {
        let msg = rx.recv().await.expect("expected an outbound RPC");
        serde_json::from_str(&msg).unwrap()
    }

    /// Reply to an RPC id as the fake extension would (HTTP-callback shape).
    fn reply_ok(bridge: &ExtBridge, id: &str, http: i64, body: Value) {
        bridge.complete_callback(
            id,
            json!({ "id": id, "status": "ok", "data": { "httpStatus": http, "json": body } }),
        );
    }

    // ---- P6: comment cache + sync ----

    fn tmp_db() -> Db {
        let p = std::env::temp_dir().join(format!("yt-test-{}.db", crate::db::new_id()));
        Db::open(&p).unwrap()
    }

    fn comment_node(id: &str, author: &str, text: &str, like: &str) -> Value {
        json!({ "commentThreadRenderer": { "comment": { "commentRenderer": {
            "commentId": id,
            "authorText": { "simpleText": author },
            "contentText": { "runs": [{ "text": text }] },
            "voteCount": { "simpleText": like }
        }}}})
    }
    fn cont_item(token: &str) -> Value {
        json!({ "continuationItemRenderer": { "continuationEndpoint": { "continuationCommand": { "token": token }}}})
    }
    fn comment_page(items: Value) -> Value {
        json!({ "onResponseReceivedEndpoints": [{ "appendContinuationItemsAction": { "continuationItems": items }}]})
    }
    fn watch_with_comment_token(token: &str) -> Value {
        json!({ "contents": { "twoColumnWatchNextResults": { "results": { "results": { "contents": [
            { "itemSectionRenderer": { "sectionIdentifier": "comment-item-section", "contents": [ cont_item(token) ]}}
        ]}}}}})
    }

    #[test]
    fn upsert_comment_is_idempotent_and_updates() {
        let db = tmp_db();
        let c = CommentIn {
            id: "x".into(),
            video_id: "v".into(),
            parent_id: None,
            author: "a".into(),
            author_channel: None,
            text: "hi".into(),
            like_count: Some(1),
            reply_count: None,
            published: None,
            reply_params: None,
            tokens_json: None,
        };
        assert!(db.upsert_comment(&c, 1).unwrap(), "first insert = new");
        let c2 = CommentIn {
            text: "hi edited".into(),
            like_count: Some(5),
            ..c
        };
        assert!(!db.upsert_comment(&c2, 2).unwrap(), "second = not new");
        let rows = db.list_comments("v", 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "hi edited");
        assert_eq!(rows[0].like_count, Some(5));
    }

    #[tokio::test]
    async fn sync_comments_pages_and_dedupes() {
        use std::sync::Arc;
        let bridge = ExtBridge::new();
        let db = Arc::new(tmp_db());
        let mut rx = bridge.test_connect();
        let (b2, dbc) = (bridge.clone(), db.clone());
        let handle = tokio::spawn(async move { sync_comments(&b2, &dbc, "vid9", 3, 1000).await });

        // 1) next{videoId} → comment-section token
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["params"]["body"]["videoId"], "vid9");
        reply_ok(
            &bridge,
            r["id"].as_str().unwrap(),
            200,
            watch_with_comment_token("C0"),
        );

        // 2) next{C0} → page 1 (2 comments + a next-page sentinel)
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["params"]["body"]["continuation"], "C0");
        reply_ok(
            &bridge,
            r["id"].as_str().unwrap(),
            200,
            comment_page(json!([
                comment_node("cid1", "a", "hay", "2"),
                comment_node("cid2", "b", "ok", "0"),
                cont_item("C1")
            ])),
        );

        // 3) next{C1} → page 2 (cid2 repeats, cid3 new, no sentinel → stop)
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["params"]["body"]["continuation"], "C1");
        reply_ok(
            &bridge,
            r["id"].as_str().unwrap(),
            200,
            comment_page(json!([
                comment_node("cid2", "b", "ok sửa", "0"),
                comment_node("cid3", "c", "mới", "5")
            ])),
        );

        let res = handle.await.unwrap().expect("sync ok");
        assert_eq!(res["fetched"], 4, "2 + 2 rows seen");
        assert_eq!(res["new"], 3, "cid1, cid2, cid3 distinct");
        assert_eq!(res["pages"], 2);
        assert_eq!(res["counts"]["total"], 3);

        let rows = db.list_comments("vid9", 100).unwrap();
        assert_eq!(rows.len(), 3);
        // The repeated comment was UPDATED, not duplicated.
        assert_eq!(rows.iter().find(|r| r.id == "cid2").unwrap().text, "ok sửa");
        assert_eq!(
            rows.iter().find(|r| r.id == "cid1").unwrap().like_count,
            Some(2)
        );
    }

    fn mk_comment(id: &str, author: &str, text: &str) -> CommentIn {
        CommentIn {
            id: id.into(),
            video_id: "v".into(),
            parent_id: None,
            author: author.into(),
            author_channel: None,
            text: text.into(),
            like_count: Some(1),
            reply_count: None,
            published: None,
            reply_params: Some(format!("rp-{id}")),
            tokens_json: Some(json!({ "heart": format!("h-{id}") }).to_string()),
        }
    }

    // ---- P7 stats / P9 feed / P10 scan (DB level) ----
    #[test]
    fn stats_feed_scan_and_tokens() {
        let db = tmp_db();
        db.upsert_comment(&mk_comment("c1", "alice", "sản phẩm tuyệt vời"), 1)
            .unwrap();
        db.upsert_comment(&mk_comment("c2", "bob", "giá bao nhiêu vậy?"), 1)
            .unwrap();
        db.upsert_comment(&mk_comment("c3", "alice", "tệ quá"), 1)
            .unwrap();
        db.save_analysis("c1", "pos", 0.9, "praise", "[]", "vi", false, 0.0, "m", 2)
            .unwrap();
        db.save_analysis("c2", "neu", 0.0, "question", "[]", "vi", false, 0.0, "m", 2)
            .unwrap();

        let stats = db.comment_stats("v").unwrap();
        assert_eq!(stats["total"], 3);
        assert_eq!(stats["analyzed"], 2);
        assert_eq!(stats["sentiment"]["pos"], 1);
        assert_eq!(stats["intent"]["question"], 1);
        assert_eq!(stats["topAuthors"]["alice"], 2);

        // CRM pull-feed cursor: newest-after-cursor, then empty.
        let feed = db.feed_since(0, 10).unwrap();
        assert_eq!(feed.len(), 3);
        assert_eq!(feed[0]["platform"], "youtube");
        assert_eq!(feed[0]["external_id"], "c1");
        let last = feed.last().unwrap()["id"].as_i64().unwrap();
        assert!(
            db.feed_since(last, 10).unwrap().is_empty(),
            "cursor must advance"
        );

        // reply token + action token lookups
        assert_eq!(db.reply_params_of("c2").unwrap().as_deref(), Some("rp-c2"));
        assert_eq!(db.tokens_of("c1").unwrap().unwrap()["heart"], "h-c1");

        // keyword scan
        let hits = db.search_comments(Some("v"), &["giá".into()], 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "c2");
        assert!(db
            .search_comments(Some("v"), &["khôngcótừnày".into()], 10)
            .unwrap()
            .is_empty());
    }

    // ---- P8 comment action ----
    #[tokio::test]
    async fn comment_action_hearts_via_cached_token() {
        use std::sync::Arc;
        let bridge = ExtBridge::new();
        let db = Arc::new(tmp_db());
        db.upsert_comment(&mk_comment("c1", "a", "hi"), 1).unwrap();
        let mut rx = bridge.test_connect();
        let (b2, dbc) = (bridge.clone(), db.clone());
        let handle =
            tokio::spawn(async move { comment_action(&b2, &dbc, "c1", "heart", false).await });

        let r = next_rpc(&mut rx).await;
        assert!(r["params"]["url"]
            .as_str()
            .unwrap()
            .contains("comment/perform_comment_action"));
        assert_eq!(r["params"]["body"]["actions"][0], "h-c1");
        reply_ok(
            &bridge,
            r["id"].as_str().unwrap(),
            200,
            json!({ "actionResults": [{ "status": "STATUS_SUCCEEDED" }] }),
        );

        let res = handle.await.unwrap().expect("action ok");
        assert_eq!(res["ok"], true);
        assert_eq!(res["action"], "heart");
    }

    #[tokio::test]
    async fn comment_action_errors_without_token() {
        let bridge = ExtBridge::new();
        let db = tmp_db();
        // A comment with no cached action tokens.
        db.upsert_comment(
            &CommentIn {
                tokens_json: None,
                ..mk_comment("c1", "a", "hi")
            },
            1,
        )
        .unwrap();
        let _rx = bridge.test_connect();
        let err = comment_action(&bridge, &db, "c1", "heart", false)
            .await
            .unwrap_err();
        assert!(err.contains("token"), "got: {err}");
    }

    // ---- moderation (remove/report via InnerTube menu tokens) ----
    #[test]
    fn parse_comments_extracts_moderation_menu_tokens() {
        let resp = json!({ "commentRenderer": {
            "commentId": "cid",
            "contentText": { "runs": [{ "text": "spam here" }] },
            "actionMenu": { "menuRenderer": { "items": [
                { "menuServiceItemRenderer": { "icon": { "iconType": "DELETE" },
                    "serviceEndpoint": { "performCommentActionEndpoint": { "action": "REMOVE_TOK" }}}},
                { "menuServiceItemRenderer": { "icon": { "iconType": "FLAG" },
                    "serviceEndpoint": { "performCommentActionEndpoint": { "action": "REPORT_TOK" }}}}
            ]}}
        }});
        let items = innertube::parse_comments(&resp);
        assert_eq!(items[0]["removeToken"], "REMOVE_TOK");
        assert_eq!(items[0]["reportToken"], "REPORT_TOK");
    }

    #[tokio::test]
    async fn remove_requires_confirm_then_succeeds() {
        use std::sync::Arc;
        let bridge = ExtBridge::new();
        let db = Arc::new(tmp_db());
        db.upsert_comment(
            &CommentIn {
                tokens_json: Some(json!({ "remove": "RM_TOK" }).to_string()),
                ..mk_comment("c1", "a", "spam")
            },
            1,
        )
        .unwrap();
        let _rx = bridge.test_connect();

        // Without confirm → refused before any RPC.
        let err = comment_action(&bridge, &db, "c1", "remove", false)
            .await
            .unwrap_err();
        assert!(err.contains("confirm=true"), "got: {err}");

        // With confirm → fires the action.
        let mut rx = bridge.test_connect();
        let (b2, dbc) = (bridge.clone(), db.clone());
        let handle =
            tokio::spawn(async move { comment_action(&b2, &dbc, "c1", "remove", true).await });
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["params"]["body"]["actions"][0], "RM_TOK");
        reply_ok(
            &bridge,
            r["id"].as_str().unwrap(),
            200,
            json!({ "actionResults": [{ "status": "STATUS_SUCCEEDED" }] }),
        );
        assert_eq!(handle.await.unwrap().unwrap()["ok"], true);
    }

    fn search_fixture() -> Value {
        json!({ "contents": { "sectionListRenderer": { "contents": [
            { "itemSectionRenderer": { "contents": [
                { "videoRenderer": {
                    "videoId": "vid9",
                    "title": { "runs": [{ "text": "Tự học Rust" }] },
                    "ownerText": { "runs": [{ "text": "Kênh Rust" }] }
                }}
            ]}}
        ]}}})
    }

    #[tokio::test]
    async fn search_round_trip_through_fake_extension() {
        let bridge = ExtBridge::new();
        let mut rx = bridge.test_connect();
        let b2 = bridge.clone();
        let handle = tokio::spawn(async move { search(&b2, "rust").await });

        let rpc = next_rpc(&mut rx).await;
        assert_eq!(rpc["method"], "yt_fetch");
        assert!(rpc["params"]["url"]
            .as_str()
            .unwrap()
            .contains("youtubei/v1/search"));
        assert_eq!(rpc["params"]["body"]["query"], "rust");
        reply_ok(&bridge, rpc["id"].as_str().unwrap(), 200, search_fixture());

        let res = handle.await.unwrap().expect("search ok");
        assert_eq!(res["count"], 1);
        assert_eq!(res["items"][0]["videoId"], "vid9");
        assert_eq!(res["items"][0]["title"], "Tự học Rust");
    }

    #[tokio::test]
    async fn http_error_surfaces_as_block() {
        let bridge = ExtBridge::new();
        let mut rx = bridge.test_connect();
        let b2 = bridge.clone();
        let handle = tokio::spawn(async move { search(&b2, "x").await });
        let rpc = next_rpc(&mut rx).await;
        reply_ok(&bridge, rpc["id"].as_str().unwrap(), 403, json!({}));
        let err = handle.await.unwrap().unwrap_err();
        assert!(err.contains("403"), "got: {err}");
    }

    #[tokio::test]
    async fn send_comment_two_step_then_success() {
        let bridge = ExtBridge::new();
        let mut rx = bridge.test_connect();
        let b2 = bridge.clone();
        let handle =
            tokio::spawn(async move { send_action(&b2, "comment", "vid9", "hay quá!").await });

        // 1) create_comment_params → next{videoId}; we return the params inline.
        let r1 = next_rpc(&mut rx).await;
        assert!(r1["params"]["url"]
            .as_str()
            .unwrap()
            .contains("youtubei/v1/next"));
        assert_eq!(r1["params"]["body"]["videoId"], "vid9");
        reply_ok(
            &bridge,
            r1["id"].as_str().unwrap(),
            200,
            json!({ "createCommentParams": "CCP" }),
        );

        // 2) create_comment with that token.
        let r2 = next_rpc(&mut rx).await;
        assert!(r2["params"]["url"]
            .as_str()
            .unwrap()
            .contains("comment/create_comment"));
        assert_eq!(r2["params"]["body"]["createCommentParams"], "CCP");
        assert_eq!(r2["params"]["body"]["commentText"], "hay quá!");
        reply_ok(
            &bridge,
            r2["id"].as_str().unwrap(),
            200,
            json!({ "actionResults": [{ "status": "STATUS_SUCCEEDED" }] }),
        );

        let res = handle.await.unwrap().expect("send ok");
        assert_eq!(res["submitted"], true);
        assert_eq!(res["kind"], "comment");
    }

    /// Reply to a UI RPC (no httpStatus envelope — `data` is returned as-is).
    fn reply_ui(bridge: &ExtBridge, id: &str, data: Value) {
        bridge.complete_callback(id, json!({ "id": id, "status": "ok", "data": data }));
    }

    fn snap(elements: Value) -> Value {
        json!({ "url": "https://studio.youtube.com/", "title": "Studio", "elements": elements })
    }

    /// YouTube's own search box: editable, but small and named "search".
    fn search_box(idx: i64) -> Value {
        json!({ "idx": idx, "tag": "input", "role": "combobox", "editable": true, "clickable": false,
                "name": "search", "label": "Tìm kiếm", "text": "", "area": 3_000 })
    }
    /// The composer: the big editable box.
    fn composer(idx: i64, text: &str) -> Value {
        json!({ "idx": idx, "tag": "div", "role": "textbox", "editable": true, "clickable": false,
                "name": "", "label": "", "text": text, "area": 60_000 })
    }
    fn button(idx: i64, text: &str) -> Value {
        json!({ "idx": idx, "tag": "button", "role": "button", "editable": false, "clickable": true,
                "name": "", "label": "", "text": text, "area": 2_000 })
    }
    /// Studio's sidebar nav link — the trap that a substring match would click.
    fn nav_link(idx: i64, text: &str) -> Value {
        json!({ "idx": idx, "tag": "a", "role": "", "editable": false, "clickable": true,
                "name": "", "label": "", "text": text, "area": 1_000 })
    }

    #[test]
    fn editor_pick_skips_the_search_box() {
        // Search box comes FIRST in DOM order; the composer must still win.
        let s = snap(json!([search_box(0), composer(5, "")]));
        assert_eq!(find_editor(&s), Some(5));
    }

    #[test]
    fn submit_pick_skips_the_posts_nav_link() {
        // "Posts" link precedes the real "Đăng" button and even matches "post".
        let s = snap(json!([nav_link(1, "Posts"), button(9, "Đăng")]));
        assert_eq!(
            find_button(&s, &["post", "đăng", "publish", "xuất bản"], false),
            Some(9)
        );
    }

    #[test]
    fn submit_pick_requires_a_whole_label_match() {
        // "Post settings" must NOT be treated as the submit button.
        let s = snap(json!([button(3, "Post settings")]));
        assert_eq!(find_button(&s, &["post", "đăng"], false), None);
    }

    #[tokio::test]
    async fn community_post_drives_the_composer_ui() {
        let bridge = ExtBridge::new();
        let mut rx = bridge.test_connect();
        let b2 = bridge.clone();
        let handle = tokio::spawn(async move {
            post_community(
                &b2,
                "chào cả nhà",
                "https://studio.youtube.com/channel/UC1/posts",
            )
            .await
        });

        // open
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["method"], "yt_ui_open");
        assert!(r["params"]["url"].as_str().unwrap().contains("UC1"));
        reply_ui(&bridge, r["id"].as_str().unwrap(), json!({ "tabId": 7 }));

        // snapshot 1 → click "Create post" (a nav link "Posts" must be ignored)
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["method"], "yt_ui_snapshot");
        reply_ui(
            &bridge,
            r["id"].as_str().unwrap(),
            snap(json!([nav_link(1, "Posts"), button(2, "Create post")])),
        );
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["method"], "yt_ui_act");
        assert_eq!(r["params"]["action"], "click");
        assert_eq!(
            r["params"]["index"], 2,
            "phải bấm nút Create post, không phải link Posts"
        );
        reply_ui(&bridge, r["id"].as_str().unwrap(), json!({ "ok": true }));

        // snapshot 2 → type into the composer, NOT the search box
        let r = next_rpc(&mut rx).await;
        reply_ui(
            &bridge,
            r["id"].as_str().unwrap(),
            snap(json!([search_box(0), composer(5, "")])),
        );
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["params"]["action"], "type");
        assert_eq!(
            r["params"]["index"], 5,
            "phải gõ vào composer, không phải ô tìm kiếm"
        );
        assert_eq!(r["params"]["text"], "chào cả nhà");
        reply_ui(&bridge, r["id"].as_str().unwrap(), json!({ "ok": true }));

        // snapshot 3 → click the real submit button, not the "Posts" nav link
        let r = next_rpc(&mut rx).await;
        reply_ui(
            &bridge,
            r["id"].as_str().unwrap(),
            snap(json!([
                nav_link(1, "Posts"),
                composer(5, "chào cả nhà"),
                button(9, "Đăng")
            ])),
        );
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["params"]["action"], "click");
        assert_eq!(
            r["params"]["index"], 9,
            "phải bấm nút Đăng, không phải link Posts"
        );
        reply_ui(&bridge, r["id"].as_str().unwrap(), json!({ "ok": true }));

        // snapshot 4 (verify) → composer gone ⇒ verified
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["method"], "yt_ui_snapshot");
        reply_ui(&bridge, r["id"].as_str().unwrap(), snap(json!([])));

        // release the debugger attachment
        let r = next_rpc(&mut rx).await;
        assert_eq!(r["method"], "yt_ui_release");
        reply_ui(
            &bridge,
            r["id"].as_str().unwrap(),
            json!({ "released": true }),
        );

        let res = handle.await.unwrap().expect("post ok");
        assert_eq!(res["submitted"], true);
        assert_eq!(res["verified"], true);
        assert_eq!(res["kind"], "community_post");
        assert_eq!(res["trace"].as_array().unwrap().len(), 5);
    }

    #[tokio::test]
    async fn community_post_not_verified_when_text_remains() {
        let bridge = ExtBridge::new();
        let mut rx = bridge.test_connect();
        let b2 = bridge.clone();
        let handle = tokio::spawn(async move {
            post_community(&b2, "xin chào", "https://studio.youtube.com/").await
        });

        let r = next_rpc(&mut rx).await; // open
        reply_ui(&bridge, r["id"].as_str().unwrap(), json!({ "tabId": 3 }));
        let r = next_rpc(&mut rx).await; // snapshot 1 — no "Create post" button ⇒ no click
        reply_ui(
            &bridge,
            r["id"].as_str().unwrap(),
            snap(json!([composer(5, "")])),
        );
        let r = next_rpc(&mut rx).await; // snapshot 2 — locate the editor
        reply_ui(
            &bridge,
            r["id"].as_str().unwrap(),
            snap(json!([composer(5, "")])),
        );
        let r = next_rpc(&mut rx).await; // type
        assert_eq!(r["params"]["action"], "type");
        reply_ui(&bridge, r["id"].as_str().unwrap(), json!({ "ok": true }));
        let r = next_rpc(&mut rx).await; // snapshot 3 — submit button
        reply_ui(
            &bridge,
            r["id"].as_str().unwrap(),
            snap(json!([button(9, "Đăng")])),
        );
        let r = next_rpc(&mut rx).await; // click submit
        reply_ui(&bridge, r["id"].as_str().unwrap(), json!({ "ok": true }));
        // verify: composer still holds the text ⇒ the post did NOT go through
        let r = next_rpc(&mut rx).await;
        reply_ui(
            &bridge,
            r["id"].as_str().unwrap(),
            snap(json!([composer(5, "xin chào")])),
        );
        let r = next_rpc(&mut rx).await; // release
        reply_ui(
            &bridge,
            r["id"].as_str().unwrap(),
            json!({ "released": true }),
        );

        let res = handle.await.unwrap().expect("flow completes");
        assert_eq!(
            res["verified"], false,
            "còn chữ trong composer ⇒ không được báo đã đăng"
        );
    }

    #[tokio::test]
    async fn community_post_reports_missing_editor() {
        let bridge = ExtBridge::new();
        let mut rx = bridge.test_connect();
        let b2 = bridge.clone();
        let handle =
            tokio::spawn(
                async move { post_community(&b2, "x", "https://studio.youtube.com/").await },
            );

        let r = next_rpc(&mut rx).await; // open
        reply_ui(&bridge, r["id"].as_str().unwrap(), json!({ "tabId": 1 }));
        let r = next_rpc(&mut rx).await; // snapshot 1 — nothing matches
        reply_ui(&bridge, r["id"].as_str().unwrap(), snap(json!([])));
        let r = next_rpc(&mut rx).await; // snapshot 2 — still no editor
        reply_ui(&bridge, r["id"].as_str().unwrap(), snap(json!([])));

        let err = handle.await.unwrap().unwrap_err();
        assert!(
            err.contains("youtube_ui_snapshot"),
            "phải chỉ đường fallback; got: {err}"
        );
    }
}
