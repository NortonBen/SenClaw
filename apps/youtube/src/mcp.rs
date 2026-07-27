use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::{norm_kind, now, AppState};
use crate::youtube;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.mcp_tx.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message".to_string()));
        while let Ok(msg) = rx.recv().await {
            yield Ok(Event::default().event("message").data(msg));
        }
    };
    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default())
}

fn text_result(text: String) -> Value {
    json!({ "content": [{ "type": "text", "text": text }] })
}
fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
}
fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<Arc<AppState>>,
    Json(req): Json<JsonRpcRequest>,
) -> Json<Value> {
    let reply = |result: Value| -> Json<Value> {
        let resp = json!({ "jsonrpc": "2.0", "id": req.id, "result": result });
        let _ = state.mcp_tx.send(resp.to_string());
        Json(resp)
    };

    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "youtube-mcp", "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or_default();
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params["arguments"].clone();
            reply(call_tool(&state, &name, &args).await)
        }
        _ => Json(json!("ok")),
    }
}

fn tools_list() -> Value {
    json!([
        {
            "name": "youtube_status",
            "description": "Check whether the YouTube Chrome extension is connected and the user is signed in. Call this FIRST — reads/writes only work once the extension is connected (it runs the real logged-in browser session that YouTube requires). Returns extensionConnected + auth snapshot.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "youtube_search",
            "description": "Search YouTube (videos) for a query, via the InnerTube API proxied through the signed-in browser extension. Returns a list of { videoId, title, channel, published, views }.",
            "inputSchema": { "type": "object", "properties": {
                "query": { "type": "string", "description": "Search text" }
            }, "required": ["query"] }
        },
        {
            "name": "youtube_browse",
            "description": "Browse a channel or feed by browseId (a channel id like UC…, or a feed id). Optionally pass a `params` token to select a sub-tab such as the Community tab. Returns { videos, posts }.",
            "inputSchema": { "type": "object", "properties": {
                "browse_id": { "type": "string", "description": "Channel id (UC…) or feed id" },
                "params": { "type": "string", "description": "Optional tab-selection token (e.g. community tab)" }
            }, "required": ["browse_id"] }
        },
        {
            "name": "youtube_list_comments",
            "description": "List comments on a video (pass video_id) or page an existing comment thread (pass a continuation token). Each comment has { commentId, author, text, replyParams }. Use replyParams as the target of a youtube_draft_comment kind=reply.",
            "inputSchema": { "type": "object", "properties": {
                "video_id": { "type": "string", "description": "Video id to load comments for" },
                "continuation": { "type": "string", "description": "A comment-section continuation token (alternative to video_id)" }
            } }
        },
        {
            "name": "youtube_sync_comments",
            "description": "Pull a video's comments through the signed-in browser and CACHE them locally (up to max_pages continuation pages). This is the foundation for comment analytics and routing — call it before analysing. Returns { fetched, new, pages, counts }.",
            "inputSchema": { "type": "object", "properties": {
                "video_id": { "type": "string" },
                "max_pages": { "type": "number", "description": "Continuation pages to pull (default 3, each ~20 comments)" }
            }, "required": ["video_id"] }
        },
        {
            "name": "youtube_cached_comments",
            "description": "List comments already cached for a video (from youtube_sync_comments), newest first, with any analysis attached. Reads the local DB — does not hit YouTube.",
            "inputSchema": { "type": "object", "properties": {
                "video_id": { "type": "string" },
                "limit": { "type": "number", "description": "Max rows (default 100)" }
            }, "required": ["video_id"] }
        },
        {
            "name": "youtube_analyze_comments",
            "description": "Run LLM analysis (sentiment/intent/topic/spam/lang) over cached comments that aren't analysed yet, writing results into the local DB. Call youtube_sync_comments first. Bounded by `max`. Then read youtube_comment_stats.",
            "inputSchema": { "type": "object", "properties": {
                "max": { "type": "number", "description": "Max comments to analyse this call (default 60)" }
            } }
        },
        {
            "name": "youtube_comment_stats",
            "description": "Aggregated statistics for a video's cached+analysed comments: totals, sentiment/intent/language breakdown, top authors, spam count, avg sentiment. Reads local DB.",
            "inputSchema": { "type": "object", "properties": {
                "video_id": { "type": "string" }
            }, "required": ["video_id"] }
        },
        {
            "name": "youtube_scan_keywords",
            "description": "Find cached comments containing any of the given keywords (case-insensitive) — the data source for keyword alerts. Optionally scoped to one video.",
            "inputSchema": { "type": "object", "properties": {
                "keywords": { "type": "array", "items": { "type": "string" } },
                "video_id": { "type": "string", "description": "Optional: scope to one video" }
            }, "required": ["keywords"] }
        },
        {
            "name": "youtube_index_comments",
            "description": "Save a video's cached comments into the app's private knowledge space so future draft replies can recall context / build an FAQ. Uses knowledge.save.",
            "inputSchema": { "type": "object", "properties": {
                "video_id": { "type": "string" },
                "limit": { "type": "number", "description": "Max comments to index (default 50)" }
            }, "required": ["video_id"] }
        },
        {
            "name": "youtube_comment_action",
            "description": "Perform an action on a comment using tokens captured at sync time. heart/like/pin are reversible; remove/report are DESTRUCTIVE and require confirm=true. remove/pin only work on comments on your own channel. Run youtube_sync_comments first so the token is fresh.",
            "inputSchema": { "type": "object", "properties": {
                "comment_id": { "type": "string" },
                "action": { "type": "string", "enum": ["heart", "like", "pin", "remove", "report"] },
                "confirm": { "type": "boolean", "description": "Required true for remove/report (irreversible)" }
            }, "required": ["comment_id", "action"] }
        },
        {
            "name": "youtube_oauth_status",
            "description": "Check whether Data-API OAuth is configured and authorized (needed for owner-level moderation: heldForReview / rejected / banAuthor). If not authorized, the user must configure a Desktop OAuth client and visit /api/oauth/start in the app UI.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "youtube_moderate",
            "description": "Owner-level moderation of a comment on YOUR channel via the YouTube Data API (requires OAuth — check youtube_oauth_status first). status: heldForReview | published | rejected. ban_author (only valid with rejected) also auto-rejects the author's future comments.",
            "inputSchema": { "type": "object", "properties": {
                "comment_id": { "type": "string" },
                "status": { "type": "string", "enum": ["heldForReview", "published", "rejected"] },
                "ban_author": { "type": "boolean", "description": "Ban the author (rejected only)" }
            }, "required": ["comment_id", "status"] }
        },
        {
            "name": "youtube_ui_open",
            "description": "Open (or focus) a YouTube / YouTube Studio URL in the user's real signed-in tab. Use this to reach surfaces InnerTube has no API for (e.g. the community-post composer) before driving them with youtube_ui_snapshot / youtube_ui_act.",
            "inputSchema": { "type": "object", "properties": {
                "url": { "type": "string", "description": "URL to open, e.g. https://studio.youtube.com/" }
            }, "required": ["url"] }
        },
        {
            "name": "youtube_ui_snapshot",
            "description": "Snapshot the open YouTube tab's visible interactive elements as a numbered list { idx, tag, role, text }. Call this before every youtube_ui_act — indexes are only valid for the latest snapshot.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "youtube_ui_act",
            "description": "Act on the page with human-like TRUSTED input (CDP via chrome.debugger): click an element by index, click-then-type text into it, or press a key. Take a youtube_ui_snapshot first to get valid indexes.",
            "inputSchema": { "type": "object", "properties": {
                "action": { "type": "string", "enum": ["click", "type", "press"] },
                "index": { "type": "number", "description": "Element idx from the latest snapshot (click/type)" },
                "text": { "type": "string", "description": "Text to type (action=type)" },
                "key": { "type": "string", "enum": ["Enter", "Tab", "Escape", "Backspace"], "description": "Key to press (action=press)" }
            }, "required": ["action"] }
        },
        {
            "name": "youtube_draft_comment",
            "description": "AI-write a comment (or community-post reply) about some context and store it as a DRAFT. Nothing is posted — drafts are the human-in-the-loop safety gate. Returns the draft id + body. Use youtube_list_drafts → youtube_approve_draft → youtube_send_draft to actually post.",
            "inputSchema": { "type": "object", "properties": {
                "kind": { "type": "string", "enum": ["comment", "reply", "community_post"] },
                "target": { "type": "string", "description": "video id / comment id / channel id the action targets" },
                "context": { "type": "string", "description": "What to write about (a video title, a post body, etc.)" },
                "instruction": { "type": "string", "description": "Optional guidance for tone/content" }
            }, "required": ["kind", "context"] }
        },
        {
            "name": "youtube_list_drafts",
            "description": "List stored WRITE drafts (comment/reply/community_post), optionally filtered by status: draft | approved | sent | failed.",
            "inputSchema": { "type": "object", "properties": {
                "status": { "type": "string", "enum": ["draft", "approved", "sent", "failed"] }
            } }
        },
        {
            "name": "youtube_approve_draft",
            "description": "Approve a draft so it becomes eligible to send. This is the explicit human-in-the-loop confirmation step; only approved drafts can be sent.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "string" }
            }, "required": ["id"] }
        },
        {
            "name": "youtube_send_draft",
            "description": "Send an APPROVED draft: post the comment (kind=comment, target=videoId) or reply (kind=reply, target=createReplyParams from youtube_list_comments) via the signed-in browser session. Refuses drafts that are not approved. community_post is not yet supported (needs YouTube Studio flow).",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "string" }
            }, "required": ["id"] }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let db = &state.db;
    let bridge = &state.bridge;
    match name {
        "youtube_status" => json_result(youtube::status(bridge, db)),

        "youtube_search" => {
            let query = args["query"].as_str().unwrap_or("").trim();
            if query.is_empty() {
                return error_result("query is required".into());
            }
            match youtube::search(bridge, query).await {
                Ok(v) => {
                    db.log("search", query, now());
                    json_result(v)
                }
                Err(e) => error_result(e),
            }
        }

        "youtube_browse" => {
            let browse_id = args["browse_id"].as_str().unwrap_or("").trim();
            if browse_id.is_empty() {
                return error_result("browse_id is required".into());
            }
            let params = args["params"].as_str();
            match youtube::browse(bridge, browse_id, params).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "youtube_list_comments" => {
            let vid = args["video_id"].as_str().unwrap_or("").trim();
            let cont = args["continuation"].as_str().unwrap_or("").trim();
            let res = if !cont.is_empty() {
                youtube::comments(bridge, cont).await
            } else if !vid.is_empty() {
                youtube::comments_for_video(bridge, vid).await
            } else {
                return error_result("cần video_id hoặc continuation".into());
            };
            match res {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "youtube_sync_comments" => {
            let vid = args["video_id"].as_str().unwrap_or("").trim();
            if vid.is_empty() {
                return error_result("video_id is required".into());
            }
            let max_pages = args["max_pages"].as_u64().unwrap_or(3) as u32;
            match youtube::sync_comments(bridge, db, vid, max_pages, now()).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "youtube_cached_comments" => {
            let vid = args["video_id"].as_str().unwrap_or("").trim();
            if vid.is_empty() {
                return error_result("video_id is required".into());
            }
            let limit = args["limit"].as_i64().unwrap_or(100).clamp(1, 1000);
            match db.list_comments(vid, limit) {
                Ok(rows) => json_result(json!({ "count": rows.len(), "comments": rows })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "youtube_analyze_comments" => {
            let max = args["max"].as_u64().unwrap_or(60) as usize;
            match youtube::analyze_pending(db, max, now()).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "youtube_comment_stats" => {
            let vid = args["video_id"].as_str().unwrap_or("").trim();
            if vid.is_empty() {
                return error_result("video_id is required".into());
            }
            match db.comment_stats(vid) {
                Ok(v) => json_result(v),
                Err(e) => error_result(e.to_string()),
            }
        }

        "youtube_scan_keywords" => {
            let kws: Vec<String> = args["keywords"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).map(|s| s.to_string()).collect())
                .unwrap_or_default();
            if kws.is_empty() {
                return error_result("keywords is required (non-empty array)".into());
            }
            let vid = args["video_id"].as_str().filter(|v| !v.trim().is_empty());
            match db.search_comments(vid, &kws, 200) {
                Ok(rows) => json_result(json!({ "count": rows.len(), "keywords": kws, "comments": rows })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "youtube_index_comments" => {
            let vid = args["video_id"].as_str().unwrap_or("").trim();
            if vid.is_empty() {
                return error_result("video_id is required".into());
            }
            let limit = args["limit"].as_i64().unwrap_or(50).clamp(1, 500);
            match youtube::index_comments(db, vid, limit).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "youtube_comment_action" => {
            let cid = args["comment_id"].as_str().unwrap_or("").trim();
            let action = args["action"].as_str().unwrap_or("").trim();
            if cid.is_empty() {
                return error_result("comment_id is required".into());
            }
            let confirm = args["confirm"].as_bool().unwrap_or(false);
            match youtube::comment_action(bridge, db, cid, action, confirm).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "youtube_oauth_status" => json_result(crate::oauth::status(db)),

        "youtube_moderate" => {
            let cid = args["comment_id"].as_str().unwrap_or("").trim();
            let status = args["status"].as_str().unwrap_or("").trim();
            let ban = args["ban_author"].as_bool().unwrap_or(false);
            if cid.is_empty() {
                return error_result("comment_id is required".into());
            }
            match crate::oauth::moderate(db, cid, status, ban).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "youtube_ui_open" => {
            let url = args["url"].as_str().unwrap_or("").trim();
            if url.is_empty() {
                return error_result("url is required".into());
            }
            match youtube::ui_open(bridge, url).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "youtube_ui_snapshot" => match youtube::ui_snapshot(bridge).await {
            Ok(v) => json_result(v),
            Err(e) => error_result(e),
        },

        "youtube_ui_act" => {
            let action = args["action"].as_str().unwrap_or("").trim();
            if !matches!(action, "click" | "type" | "press") {
                return error_result("action must be click | type | press".into());
            }
            let index = args["index"].as_i64();
            let text = args["text"].as_str();
            let key = args["key"].as_str();
            match youtube::ui_act(bridge, action, index, text, key).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }

        "youtube_draft_comment" => {
            let kind = match norm_kind(args["kind"].as_str().unwrap_or("")) {
                Some(k) => k,
                None => return error_result("kind must be comment | reply | community_post".into()),
            };
            let context = args["context"].as_str().unwrap_or("").trim();
            if context.is_empty() {
                return error_result("context is required".into());
            }
            let target = args["target"].as_str().unwrap_or("");
            let instruction = args["instruction"].as_str();
            match crate::llm::draft_body(kind, context, instruction).await {
                Ok((body, model)) => match db.create_draft(kind, target, body.trim(), now()) {
                    Ok(id) => json_result(json!({ "id": id, "status": "draft", "body": body.trim(), "model": model })),
                    Err(e) => error_result(e.to_string()),
                },
                Err(e) => error_result(e),
            }
        }

        "youtube_list_drafts" => {
            let status = args["status"].as_str();
            match db.list_drafts(status) {
                Ok(v) => json_result(json!(v)),
                Err(e) => error_result(e.to_string()),
            }
        }

        "youtube_approve_draft" => {
            let id = args["id"].as_str().unwrap_or("");
            match db.get_draft(id) {
                Ok(Some(d)) if d.status == "sent" => error_result("draft already sent".into()),
                Ok(Some(_)) => match db.set_draft_status(id, "approved", None, now()) {
                    Ok(()) => json_result(json!({ "id": id, "status": "approved" })),
                    Err(e) => error_result(e.to_string()),
                },
                Ok(None) => error_result(format!("draft {id} not found")),
                Err(e) => error_result(e.to_string()),
            }
        }

        "youtube_send_draft" => {
            let id = args["id"].as_str().unwrap_or("");
            let d = match db.get_draft(id) {
                Ok(Some(d)) => d,
                Ok(None) => return error_result(format!("draft {id} not found")),
                Err(e) => return error_result(e.to_string()),
            };
            if d.status != "approved" {
                return error_result("only APPROVED drafts can be sent (approve first)".into());
            }
            match youtube::send_action(bridge, &d.kind, &d.target, &d.body).await {
                Ok(res) => {
                    let _ = db.set_draft_status(id, "sent", Some(&res.to_string()), now());
                    json_result(json!({ "id": id, "status": "sent", "result": res }))
                }
                Err(e) => {
                    let _ = db.set_draft_status(id, "failed", Some(&json!({ "error": e.clone() }).to_string()), now());
                    error_result(e)
                }
            }
        }

        _ => error_result(format!("Unknown tool: {name}")),
    }
}
