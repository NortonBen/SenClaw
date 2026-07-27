//! InnerTube (`youtubei/v1/*`) payload builders + light response parsing.
//!
//! InnerTube is the internal API youtube.com itself uses. It needs no API key and
//! no quota, but the request MUST be issued from a real, logged-in browser context
//! (BotGuard/PoToken) — which is why every call here is forwarded to the Chrome
//! extension (see `youtube.rs::proxy`) rather than made from this process.
//!
//! The `context.client` block below is the WEB client's default; the extension can
//! and should override `clientVersion`/`visitorData` with the fresher values it
//! scrapes from the live page (`ytcfg`) so the payload stays consistent with the
//! session's own headers.

use serde_json::{json, Value};

/// The well-known public WEB InnerTube key that ships in every youtube.com page.
/// Not a secret — it only selects the API surface; auth is the session cookie.
pub const WEB_API_KEY: &str = "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8";

/// A recent WEB client version. The extension overrides this from live `ytcfg`
/// when available; this is only the fallback for an un-augmented call.
pub const WEB_CLIENT_VERSION: &str = "2.20240620.05.00";

/// Build the `context.client` object for a WEB InnerTube request.
pub fn client_context() -> Value {
    json!({
        "client": {
            "clientName": "WEB",
            "clientVersion": WEB_CLIENT_VERSION,
            "hl": "vi",
            "gl": "VN"
        }
    })
}

/// Full endpoint URL, e.g. `search` → `https://www.youtube.com/youtubei/v1/search?...`.
pub fn endpoint_url(endpoint: &str) -> String {
    format!(
        "https://www.youtube.com/youtubei/v1/{endpoint}?key={WEB_API_KEY}&prettyPrint=false"
    )
}

/// `search` body for a text query.
pub fn search_body(query: &str) -> Value {
    json!({ "context": client_context(), "query": query })
}

/// `browse` body. `browse_id` is e.g. a channel id (`UC…`) or `FEwhat_to_watch`.
/// `params` optionally selects a sub-tab (e.g. the community tab token).
pub fn browse_body(browse_id: &str, params: Option<&str>) -> Value {
    let mut b = json!({ "context": client_context(), "browseId": browse_id });
    if let Some(p) = params {
        b["params"] = json!(p);
    }
    b
}

/// `next` body for a continuation token (comment pages, feed continuations).
#[allow(dead_code)] // wired for Phase 3 comment-paging
pub fn continuation_body(token: &str) -> Value {
    json!({ "context": client_context(), "continuation": token })
}

/// Best-effort flat extraction of `videoRenderer` items from a `search`/`browse`
/// response into `{ videoId, title, channel, published, views }`. InnerTube's shape
/// is deeply nested and changes often, so this walks the whole tree defensively and
/// never fails — callers can always fall back to the raw JSON.
pub fn parse_videos(v: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    collect_renderer(v, "videoRenderer", &mut |r| {
        let video_id = r.get("videoId").and_then(|x| x.as_str()).unwrap_or("");
        if video_id.is_empty() {
            return;
        }
        out.push(json!({
            "videoId": video_id,
            "title": runs_text(r.get("title")),
            "channel": runs_text(r.get("longBylineText").or_else(|| r.get("ownerText"))),
            "published": simple_text(r.get("publishedTimeText")),
            "views": simple_text(r.get("viewCountText")),
        }));
    });
    out
}

/// Extract community/backstage posts from a `browse` (community tab) response into
/// `{ postId, text }`. Same defensive tree-walk approach as `parse_videos`.
pub fn parse_posts(v: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    collect_renderer(v, "backstagePostRenderer", &mut |r| {
        let post_id = r.get("postId").and_then(|x| x.as_str()).unwrap_or("");
        out.push(json!({
            "postId": post_id,
            "text": runs_text(r.get("contentText")),
        }));
    });
    out
}

/// Extract comments from a `next` continuation response into rich rows:
/// `{ commentId, author, authorChannel, text, likeCount, replyCount, published,
///    replyParams, heartToken, likeToken }`. Defensive tree-walk — never fails.
pub fn parse_comments(v: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    collect_renderer(v, "commentRenderer", &mut |r| {
        let comment_id = r.get("commentId").and_then(|x| x.as_str()).unwrap_or("");
        if comment_id.is_empty() {
            return;
        }
        let published = runs_text(r.get("publishedTimeText"));
        out.push(json!({
            "commentId": comment_id,
            "author": runs_text(r.get("authorText").or_else(|| r.get("authorName"))),
            "authorChannel": find_object(r, "authorEndpoint").and_then(|e| find_key_str(e, "browseId")),
            "text": runs_text(r.get("contentText")),
            "likeCount": parse_count(&runs_text(r.get("voteCount"))),
            "replyCount": r.get("replyCount").and_then(|x| x.as_i64()),
            "published": if published.is_empty() { Value::Null } else { Value::String(published) },
            "replyParams": find_key_str(r, "createReplyParams"),
            // Action tokens for perform_comment_action (best-effort; may be absent or
            // expire quickly — meant to be used soon after sync by youtube_comment_action).
            "heartToken": find_object(r, "creatorHeartRenderer").and_then(|h| find_key_str(h, "action")),
            "likeToken": find_object(r, "likeButton").and_then(|b| find_key_str(b, "action")),
            // Moderation tokens live in the comment's overflow menu, labelled by icon.
            "removeToken": find_menu_token(r, &["DELETE", "TRASH"]),
            "reportToken": find_menu_token(r, &["FLAG"]),
            "pinToken": find_menu_token(r, &["KEEP", "PUSHPIN"]),
        }));
    });
    out
}

/// The continuation token that loads the NEXT page of comments, if any. Looks for a
/// `continuationItemRenderer` (the "show more" sentinel) and returns its token.
pub fn find_next_continuation(v: &Value) -> Option<String> {
    fn search(v: &Value) -> Option<String> {
        if let Value::Object(map) = v {
            if let Some(cir) = map.get("continuationItemRenderer") {
                if let Some(tok) = find_key_str(cir, "token") {
                    return Some(tok);
                }
            }
            for child in map.values() {
                if let Some(t) = search(child) {
                    return Some(t);
                }
            }
        } else if let Value::Array(arr) = v {
            for c in arr {
                if let Some(t) = search(c) {
                    return Some(t);
                }
            }
        }
        None
    }
    search(v)
}

/// Find a `perform_comment_action` token in the comment's overflow menu, selected by
/// the menu item's `icon.iconType` (e.g. DELETE→remove, FLAG→report, KEEP→pin).
pub fn find_menu_token(r: &Value, want_icons: &[&str]) -> Option<String> {
    fn search(v: &Value, want: &[&str]) -> Option<String> {
        if let Value::Object(map) = v {
            if let Some(item) = map.get("menuServiceItemRenderer") {
                let icon = item
                    .get("icon")
                    .and_then(|i| i.get("iconType"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                if want.contains(&icon) {
                    if let Some(tok) = find_key_str(item, "action") {
                        return Some(tok);
                    }
                }
            }
            for child in map.values() {
                if let Some(t) = search(child, want) {
                    return Some(t);
                }
            }
        } else if let Value::Array(arr) = v {
            for c in arr {
                if let Some(t) = search(c, want) {
                    return Some(t);
                }
            }
        }
        None
    }
    search(r, want_icons)
}

/// First object value found under a key named `key`, anywhere in the tree.
pub fn find_object<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if k == key && child.is_object() {
                    return Some(child);
                }
                if let Some(found) = find_object(child, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(|c| find_object(c, key)),
        _ => None,
    }
}

/// Parse a like/vote count like "1.2K" / "3,456" / "12" into an integer (best-effort).
fn parse_count(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    let mult = if t.ends_with('K') || t.ends_with('k') {
        1_000.0
    } else if t.ends_with('M') || t.ends_with('m') {
        1_000_000.0
    } else {
        1.0
    };
    let digits: String = t.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
    digits.parse::<f64>().ok().map(|n| (n * mult) as i64)
}

// ---- WRITE action bodies (InnerTube `comment/*` endpoints) ----

/// `comment/create_comment` body. `params` is the `createCommentParams` token
/// scraped from the video's comment box (see `youtube::create_comment_params`).
pub fn comment_create_body(params: &str, text: &str) -> Value {
    json!({ "context": client_context(), "commentText": text, "createCommentParams": params })
}

/// `comment/create_comment_reply` body. `params` is a `createReplyParams` token
/// (from `parse_comments`).
pub fn comment_reply_body(params: &str, text: &str) -> Value {
    json!({ "context": client_context(), "commentText": text, "createReplyParams": params })
}

/// Whether a `create_comment*` response reports success. InnerTube signals this
/// with a `STATUS_SUCCEEDED` action result; a `failed`/error node means rejected.
pub fn action_succeeded(v: &Value) -> bool {
    let mut ok = false;
    let mut failed = false;
    walk_strings(v, &mut |s| {
        if s == "STATUS_SUCCEEDED" {
            ok = true;
        }
        if s.contains("FAILED") || s == "actionResultError" {
            failed = true;
        }
    });
    ok && !failed
}

/// First string value found under a key named `key`, anywhere in the tree.
pub fn find_key_str(v: &Value, key: &str) -> Option<String> {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if k == key {
                    if let Some(s) = child.as_str() {
                        return Some(s.to_string());
                    }
                }
                if let Some(found) = find_key_str(child, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => arr.iter().find_map(|c| find_key_str(c, key)),
        _ => None,
    }
}

/// Find the continuation token that loads a video's COMMENT section. Looks for an
/// `itemSectionRenderer` flagged `sectionIdentifier: "comment-item-section"` and
/// returns the `token` beneath it.
pub fn find_comment_section_token(v: &Value) -> Option<String> {
    fn search(v: &Value) -> Option<String> {
        if let Value::Object(map) = v {
            if let Some(sec) = map.get("itemSectionRenderer") {
                let ident = sec.get("sectionIdentifier").and_then(|x| x.as_str()).unwrap_or("");
                if ident == "comment-item-section" {
                    if let Some(tok) = find_key_str(sec, "token") {
                        return Some(tok);
                    }
                }
            }
            for child in map.values() {
                if let Some(t) = search(child) {
                    return Some(t);
                }
            }
        } else if let Value::Array(arr) = v {
            for c in arr {
                if let Some(t) = search(c) {
                    return Some(t);
                }
            }
        }
        None
    }
    search(v)
}

fn walk_strings(v: &Value, f: &mut impl FnMut(&str)) {
    match v {
        Value::String(s) => f(s),
        Value::Object(map) => map.values().for_each(|c| walk_strings(c, f)),
        Value::Array(arr) => arr.iter().for_each(|c| walk_strings(c, f)),
        _ => {}
    }
}

/// Walk the JSON tree and invoke `f` on every object found under a key named `key`.
fn collect_renderer(v: &Value, key: &str, f: &mut impl FnMut(&Value)) {
    match v {
        Value::Object(map) => {
            for (k, child) in map {
                if k == key {
                    if let Value::Object(_) = child {
                        f(child);
                    }
                }
                collect_renderer(child, key, f);
            }
        }
        Value::Array(arr) => {
            for child in arr {
                collect_renderer(child, key, f);
            }
        }
        _ => {}
    }
}

/// Join a `{ runs: [{ text }] }` text object into a plain string.
fn runs_text(v: Option<&Value>) -> String {
    let Some(v) = v else { return String::new() };
    if let Some(runs) = v.get("runs").and_then(|r| r.as_array()) {
        return runs
            .iter()
            .filter_map(|r| r.get("text").and_then(|t| t.as_str()))
            .collect::<String>();
    }
    simple_text(Some(v))
}

/// Read a `{ simpleText }` value (or a bare string).
fn simple_text(v: Option<&Value>) -> String {
    match v {
        Some(v) => v
            .get("simpleText")
            .and_then(|s| s.as_str())
            .or_else(|| v.as_str())
            .unwrap_or("")
            .to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_videos_from_nested_search() {
        // A trimmed shape mirroring how `search` nests `videoRenderer` items.
        let resp = json!({
            "contents": { "sectionListRenderer": { "contents": [
                { "itemSectionRenderer": { "contents": [
                    { "videoRenderer": {
                        "videoId": "abc123",
                        "title": { "runs": [{ "text": "Học " }, { "text": "Rust" }] },
                        "ownerText": { "runs": [{ "text": "Kênh X" }] },
                        "viewCountText": { "simpleText": "1.2M views" },
                        "publishedTimeText": { "simpleText": "2 years ago" }
                    }},
                    { "videoRenderer": { "videoId": "", "title": { "runs": [] } } }
                ]}}
            ]}}
        });
        let items = parse_videos(&resp);
        assert_eq!(items.len(), 1, "empty-id renderer must be skipped");
        assert_eq!(items[0]["videoId"], "abc123");
        assert_eq!(items[0]["title"], "Học Rust");
        assert_eq!(items[0]["channel"], "Kênh X");
        assert_eq!(items[0]["views"], "1.2M views");
    }

    #[test]
    fn parses_comments_with_reply_params() {
        let resp = json!({ "onResponseReceivedEndpoints": [{ "appendContinuationItemsAction": {
            "continuationItems": [
                { "commentThreadRenderer": { "comment": { "commentRenderer": {
                    "commentId": "cid1",
                    "authorText": { "simpleText": "@nguoidung" },
                    "contentText": { "runs": [{ "text": "Bình " }, { "text": "luận hay" }] },
                    "replyCount": 3,
                    "actionButtons": { "commentActionButtonsRenderer": {
                        "replyButton": { "buttonRenderer": { "serviceEndpoint": {
                            "createCommentReplyDialogEndpoint": { "dialog": { "commentReplyDialogRenderer": {
                                "replyButton": { "buttonRenderer": { "serviceEndpoint": {
                                    "createCommentReplyEndpoint": { "createReplyParams": "REPLY_TOK" }
                                }}}
                            }}}
                        }}}
                    }}
                }}}}
            ]
        }}]});
        let items = parse_comments(&resp);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["commentId"], "cid1");
        assert_eq!(items[0]["author"], "@nguoidung");
        assert_eq!(items[0]["text"], "Bình luận hay");
        assert_eq!(items[0]["replyParams"], "REPLY_TOK");
    }

    #[test]
    fn finds_comment_section_token() {
        let resp = json!({ "contents": { "twoColumnWatchNextResults": { "results": { "results": {
            "contents": [
                { "itemSectionRenderer": {
                    "sectionIdentifier": "comment-item-section",
                    "contents": [{ "continuationItemRenderer": { "continuationEndpoint": {
                        "continuationCommand": { "token": "COMMENT_CONT" }
                    }}}]
                }}
            ]
        }}}}});
        assert_eq!(find_comment_section_token(&resp).as_deref(), Some("COMMENT_CONT"));
    }

    #[test]
    fn action_success_detection() {
        assert!(action_succeeded(&json!({ "actionResults": [{ "status": "STATUS_SUCCEEDED" }] })));
        assert!(!action_succeeded(&json!({ "actionResults": [{ "status": "STATUS_FAILED" }] })));
        assert!(!action_succeeded(&json!({ "foo": "bar" })));
    }

    #[test]
    fn action_bodies_carry_context_and_token() {
        let c = comment_create_body("PARAM", "xin chào");
        assert_eq!(c["createCommentParams"], "PARAM");
        assert_eq!(c["commentText"], "xin chào");
        assert!(c["context"]["client"]["clientName"] == "WEB");
        let r = comment_reply_body("RTOK", "trả lời");
        assert_eq!(r["createReplyParams"], "RTOK");
    }
}
