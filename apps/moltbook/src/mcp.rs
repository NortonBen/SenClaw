//! MCP server (HTTP + SSE) exposing Moltbook read + participate operations to
//! SenClaw agents. Writes go through the SAME autonomy gate the UI uses
//! ([`crate::api::enqueue_or_publish`]) so an agent can never bypass the
//! human-approval default — in `draft` mode a write becomes a queued draft, and
//! only `moltbook_approve_draft` (or `live` mode) actually publishes.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::{account_summary, client, enqueue_or_publish, now_ts, voice, AppState};
use crate::db::DraftCreate;
use crate::engine;
use crate::llm;

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
            "serverInfo": { "name": "moltbook-mcp", "version": "1.0.0" }
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

fn need_connection() -> Value {
    error_result("Chưa kết nối agent Moltbook. Dùng moltbook_connect (đã có API key) hoặc moltbook_register để tạo mới.".into())
}

fn tools_list() -> Value {
    json!([
        // ---- setup ----
        {
            "name": "moltbook_register",
            "description": "Register a BRAND-NEW agent ('molty') on Moltbook. Returns a claim_url the human must open and verify with their X account to activate the agent. The API key is stored locally. Use only once, when the user has no Moltbook agent yet.",
            "inputSchema": { "type": "object", "properties": {
                "name":        { "type": "string", "description": "The molty's display name." },
                "description": { "type": "string", "description": "One line on what this agent does." }
            }, "required": ["name"] }
        },
        {
            "name": "moltbook_connect",
            "description": "Connect an EXISTING Moltbook API key so the app can read & participate. Verifies the key by fetching your profile and caches it. base_url defaults to https://www.moltbook.com and should not be changed.",
            "inputSchema": { "type": "object", "properties": {
                "api_key":  { "type": "string", "description": "Your Moltbook API key (Bearer token)." },
                "base_url": { "type": "string", "description": "Override base URL (default https://www.moltbook.com — leave unset)." }
            }, "required": ["api_key"] }
        },
        {
            "name": "moltbook_account",
            "description": "Show local connection + autonomy status: whether an agent is connected, the autonomy mode (observe/draft/live), heartbeat settings, cached profile/karma, and the number of drafts waiting for approval.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        // ---- read ----
        {
            "name": "moltbook_feed",
            "description": "Read the Moltbook feed (the agent internet). When connected, fetches live and caches; otherwise returns the cached/demo feed. Use for 'moltbook có gì mới', 'what's on the agent internet'. Returns posts with post_id, submolt, author, title, content, score.",
            "inputSchema": { "type": "object", "properties": {
                "sort":   { "type": "string", "enum": ["hot","new","top","rising"], "description": "Default hot." },
                "filter": { "type": "string", "enum": ["all","following"], "description": "Default all." },
                "limit":  { "type": "number", "description": "Max posts (default 50)." }
            } }
        },
        {
            "name": "moltbook_home",
            "description": "Your Moltbook dashboard in one call: your account/karma, activity on your posts, unread notifications, posts from molties you follow, announcements, and suggested next steps. The best starting point for a check-in. Requires a connected agent.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "moltbook_get_post",
            "description": "Fetch one post plus its comment thread by post_id. Use before drafting a reply so you have the full context.",
            "inputSchema": { "type": "object", "properties": {
                "post_id": { "type": "string" }
            }, "required": ["post_id"] }
        },
        {
            "name": "moltbook_search",
            "description": "Semantic search over Moltbook posts/comments. Use for 'ai đang nói về X trên moltbook', 'find discussions about Y'.",
            "inputSchema": { "type": "object", "properties": {
                "q":     { "type": "string" },
                "type":  { "type": "string", "enum": ["all","posts","comments"], "description": "Default all." },
                "limit": { "type": "number", "description": "Default 20." }
            }, "required": ["q"] }
        },
        {
            "name": "moltbook_list_submolts",
            "description": "List Moltbook communities (submolts). Requires a connected agent.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "moltbook_profile",
            "description": "Fetch a molty's profile + karma. Omit `name` for your own profile. Requires a connected agent.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "Another molty's name; omit for yourself." }
            } }
        },
        {
            "name": "moltbook_notifications",
            "description": "List your Moltbook notifications (replies, follows, mentions). Requires a connected agent.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "moltbook_activity",
            "description": "The LOCAL activity log of everything this app/engine did (heartbeats, drafts, posts, votes, errors). Not the Moltbook feed. Use to explain 'what has my agent been doing on Moltbook'.",
            "inputSchema": { "type": "object", "properties": {
                "limit": { "type": "number", "description": "Default 50." }
            } }
        },
        {
            "name": "moltbook_list_drafts",
            "description": "List the approval queue — engagements the engine/you drafted that are waiting to be approved (or already posted/rejected). Filter by status.",
            "inputSchema": { "type": "object", "properties": {
                "status": { "type": "string", "enum": ["pending","posted","rejected","error"], "description": "Default: all." },
                "limit":  { "type": "number", "description": "Default 100." }
            } }
        },
        // ---- participate (draft-first; honour the autonomy gate) ----
        {
            "name": "moltbook_draft_post",
            "description": "Queue a NEW post for approval (in 'draft' mode → waits for approval; 'live' mode → publishes now; 'observe' mode → refused). Use for 'đăng bài X lên moltbook'. Respects the 300-char title limit and the 1-post/30-min rule at publish time.",
            "inputSchema": { "type": "object", "properties": {
                "submolt": { "type": "string", "description": "Community (without 'm/'). Defaults to the configured default submolt." },
                "title":   { "type": "string", "description": "Post title (max 300 chars)." },
                "content": { "type": "string", "description": "Post body." },
                "url":     { "type": "string", "description": "Optional link." }
            }, "required": ["title"] }
        },
        {
            "name": "moltbook_draft_comment",
            "description": "Queue a comment/reply for approval on a given post. Pass parent_id for a threaded reply. Honours the autonomy gate.",
            "inputSchema": { "type": "object", "properties": {
                "post_id":   { "type": "string" },
                "content":   { "type": "string" },
                "parent_id": { "type": "string", "description": "Comment id to reply under (optional)." }
            }, "required": ["post_id", "content"] }
        },
        {
            "name": "moltbook_compose_reply",
            "description": "Use the daemon LLM + your molty persona to DRAFT a substantive reply to a post, then queue it for approval. Provide post_id; the post text is pulled from cache if you don't pass it. Optional `instruction` steers the tone/point.",
            "inputSchema": { "type": "object", "properties": {
                "post_id":     { "type": "string" },
                "post_title":  { "type": "string", "description": "Optional — else read from cache." },
                "post_content":{ "type": "string", "description": "Optional — else read from cache." },
                "instruction": { "type": "string", "description": "Optional extra steer, e.g. 'push back gently'." }
            }, "required": ["post_id"] }
        },
        {
            "name": "moltbook_upvote",
            "description": "Upvote a post (honours the autonomy gate: queued in draft mode, applied now in live mode).",
            "inputSchema": { "type": "object", "properties": {
                "post_id": { "type": "string" }
            }, "required": ["post_id"] }
        },
        {
            "name": "moltbook_downvote",
            "description": "Downvote a post (honours the autonomy gate).",
            "inputSchema": { "type": "object", "properties": {
                "post_id": { "type": "string" }
            }, "required": ["post_id"] }
        },
        {
            "name": "moltbook_follow",
            "description": "Follow another molty (honours the autonomy gate).",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "The molty's name." }
            }, "required": ["name"] }
        },
        {
            "name": "moltbook_subscribe",
            "description": "Subscribe to a submolt (community). Honours the autonomy gate.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "Submolt name (without 'm/')." }
            }, "required": ["name"] }
        },
        {
            "name": "moltbook_create_submolt",
            "description": "Create a new submolt (community). Honours the autonomy gate. name is lowercase, hyphens, 2-30 chars.",
            "inputSchema": { "type": "object", "properties": {
                "name":         { "type": "string" },
                "display_name": { "type": "string" },
                "description":  { "type": "string" }
            }, "required": ["name"] }
        },
        // ---- approve / run ----
        {
            "name": "moltbook_approve_draft",
            "description": "THE PUBLISH GATE. Approve a queued draft by id — this actually calls Moltbook to post/comment/vote/etc. Solves the anti-human verification challenge automatically for new posts. Only approve drafts the user has confirmed.",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number", "description": "Draft id from moltbook_list_drafts." }
            }, "required": ["id"] }
        },
        {
            "name": "moltbook_reject_draft",
            "description": "Reject a queued draft by id so it's never published (and the engine won't re-draft that post).",
            "inputSchema": { "type": "object", "properties": {
                "id": { "type": "number" }
            }, "required": ["id"] }
        },
        {
            "name": "moltbook_run_heartbeat",
            "description": "Run ONE OpenClaw-style heartbeat tick now: read the feed, and (per autonomy mode) draft or publish a small set of genuine engagements. Returns a summary. Use for 'cho agent tham gia moltbook một vòng', 'run the moltbook heartbeat'.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let db = &state.db;
    match name {
        // ---- setup ----
        "moltbook_register" => {
            let aname = args["name"].as_str().unwrap_or("").trim();
            if aname.is_empty() {
                return error_result("name là bắt buộc".into());
            }
            let desc = args["description"].as_str().unwrap_or("").trim();
            let base = db.get_str("base_url", crate::moltbook::DEFAULT_BASE);
            let mb = crate::moltbook::Moltbook::new(Some(&base), None);
            match mb.register(aname, desc).await {
                Ok(v) => {
                    let (key, claim, vcode) = crate::moltbook::extract_register_fields(&v);
                    if !key.is_empty() {
                        db.set_str("api_key", &key).ok();
                    }
                    db.set_str("agent_name", aname).ok();
                    db.set_str("claim_url", &claim).ok();
                    db.set_str("verification_code", &vcode).ok();
                    db.set_json("last_register_response", &v).ok();
                    db.log("register", &format!("đăng ký agent '{aname}'"), "", now_ts()).ok();
                    json_result(json!({
                        "ok": true, "claim_url": claim, "verification_code": vcode, "raw": v,
                        "note": "Mở claim_url và xác nhận bằng tài khoản X để kích hoạt agent. Nếu claim_url rỗng, xem 'raw'."
                    }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "moltbook_connect" => {
            let key = args["api_key"].as_str().unwrap_or("").trim();
            if key.is_empty() {
                return error_result("api_key là bắt buộc".into());
            }
            if let Some(base) = args["base_url"].as_str().map(str::trim).filter(|b| !b.is_empty()) {
                db.set_str("base_url", base).ok();
            }
            db.set_str("api_key", key).ok();
            match client(db).me().await {
                Ok(me) => {
                    if let Some(n) = me.get("name").and_then(|x| x.as_str()) {
                        db.set_str("agent_name", n).ok();
                    }
                    db.set_json("profile", &me).ok();
                    db.set_bool("claimed", true).ok();
                    db.log("connect", "kết nối agent thành công", "", now_ts()).ok();
                    json_result(json!({ "ok": true, "profile": me }))
                }
                Err(e) => error_result(format!("lưu key nhưng xác minh thất bại: {e}")),
            }
        }
        "moltbook_account" => json_result(account_summary(db)),

        // ---- read ----
        "moltbook_feed" => {
            let limit = args["limit"].as_i64().unwrap_or(50).clamp(1, 200);
            let mut source = if db.connected() { "cache" } else { "demo" };
            if db.connected() {
                let sort = args["sort"].as_str().unwrap_or("hot");
                let filter = args["filter"].as_str().unwrap_or("all");
                if let Ok(v) = client(db).feed(sort, filter, None).await {
                    let items = engine::extract_posts(&v);
                    if !items.is_empty() {
                        db.clear_live_cache().ok();
                        let now = now_ts();
                        let rows: Vec<_> = items
                            .iter()
                            .map(|f| crate::db::CachedPost {
                                post_id: f.id.clone(), submolt: f.submolt.clone(), author: f.author.clone(),
                                title: f.title.clone(), content: f.content.clone(), url: String::new(),
                                score: f.score, comment_count: 0, posted_at: now, cached_at: now, demo: false,
                            })
                            .collect();
                        db.upsert_posts(&rows).ok();
                        source = "live";
                    }
                }
            }
            let posts = db.list_cached(limit).unwrap_or_default();
            json_result(json!({ "source": source, "count": posts.len(), "posts": posts }))
        }
        "moltbook_home" => {
            if !db.connected() {
                return need_connection();
            }
            match client(db).home().await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e.to_string()),
            }
        }
        "moltbook_get_post" => {
            let id = args["post_id"].as_str().unwrap_or("").trim();
            if id.is_empty() {
                return error_result("post_id là bắt buộc".into());
            }
            if !db.connected() {
                return need_connection();
            }
            let mb = client(db);
            match mb.get_post(id).await {
                Ok(post) => {
                    let comments = mb.comments(id, "best", None).await.unwrap_or(json!({}));
                    json_result(json!({ "post": post, "comments": comments }))
                }
                Err(e) => error_result(e.to_string()),
            }
        }
        "moltbook_search" => {
            let q = args["q"].as_str().unwrap_or("").trim();
            if q.is_empty() {
                return error_result("q là bắt buộc".into());
            }
            if !db.connected() {
                return need_connection();
            }
            let kind = args["type"].as_str().unwrap_or("all");
            let limit = args["limit"].as_i64().unwrap_or(20);
            match client(db).search(q, kind, limit).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e.to_string()),
            }
        }
        "moltbook_list_submolts" => {
            if !db.connected() {
                return need_connection();
            }
            match client(db).submolts(None).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e.to_string()),
            }
        }
        "moltbook_profile" => {
            if !db.connected() {
                return need_connection();
            }
            let mb = client(db);
            let r = match args["name"].as_str().map(str::trim).filter(|n| !n.is_empty()) {
                Some(n) => mb.profile_of(n).await,
                None => mb.me().await,
            };
            match r {
                Ok(v) => json_result(v),
                Err(e) => error_result(e.to_string()),
            }
        }
        "moltbook_notifications" => {
            if !db.connected() {
                return need_connection();
            }
            match client(db).notifications().await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e.to_string()),
            }
        }
        "moltbook_activity" => {
            let limit = args["limit"].as_i64().unwrap_or(50);
            match db.list_activity(limit) {
                Ok(items) => json_result(json!({ "count": items.len(), "items": items })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "moltbook_list_drafts" => {
            let status = args["status"].as_str();
            let limit = args["limit"].as_i64().unwrap_or(100);
            match db.list_drafts(status, limit) {
                Ok(drafts) => json_result(json!({ "count": drafts.len(), "drafts": drafts })),
                Err(e) => error_result(e.to_string()),
            }
        }

        // ---- participate (gated) ----
        "moltbook_draft_post" => {
            let title = args["title"].as_str().unwrap_or("").trim();
            if title.is_empty() {
                return error_result("title là bắt buộc".into());
            }
            let submolt = args["submolt"]
                .as_str()
                .map(|s| s.trim().trim_start_matches("m/").to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| db.get_str("default_submolt", "general"));
            let dc = DraftCreate {
                kind: "post".into(),
                submolt,
                title: title.to_string(),
                content: args["content"].as_str().unwrap_or("").to_string(),
                url: args["url"].as_str().unwrap_or("").to_string(),
                source: "agent".into(),
                ..Default::default()
            };
            json_result(enqueue_or_publish(state, dc).await)
        }
        "moltbook_draft_comment" => {
            let post_id = args["post_id"].as_str().unwrap_or("").trim();
            let content = args["content"].as_str().unwrap_or("").trim();
            if post_id.is_empty() || content.is_empty() {
                return error_result("post_id và content là bắt buộc".into());
            }
            let dc = DraftCreate {
                kind: "comment".into(),
                target_post_id: post_id.to_string(),
                content: content.to_string(),
                parent_id: args["parent_id"].as_str().unwrap_or("").to_string(),
                source: "agent".into(),
                ..Default::default()
            };
            json_result(enqueue_or_publish(state, dc).await)
        }
        "moltbook_compose_reply" => {
            let post_id = args["post_id"].as_str().unwrap_or("").trim();
            if post_id.is_empty() {
                return error_result("post_id là bắt buộc".into());
            }
            let (mut title, mut content) = (
                args["post_title"].as_str().unwrap_or("").to_string(),
                args["post_content"].as_str().unwrap_or("").to_string(),
            );
            if title.is_empty() && content.is_empty() {
                if let Some(p) = db.list_cached(500).unwrap_or_default().into_iter().find(|p| p.post_id == post_id) {
                    title = p.title;
                    content = p.content;
                }
            }
            let instruction = args["instruction"].as_str().unwrap_or("");
            match llm::compose_reply(&voice(db), &title, &content, instruction).await {
                Ok((text, model)) => {
                    let dc = DraftCreate {
                        kind: "comment".into(),
                        target_post_id: post_id.to_string(),
                        target_title: title,
                        content: text,
                        reason: if instruction.is_empty() { "compose_reply".into() } else { instruction.to_string() },
                        source: "agent".into(),
                        model,
                        ..Default::default()
                    };
                    match db.create_draft(&dc, now_ts()) {
                        Ok(id) => json_result(json!({ "ok": true, "queued": true, "draft": db.get_draft(id).ok().flatten() })),
                        Err(e) => error_result(e.to_string()),
                    }
                }
                Err(e) => error_result(e),
            }
        }
        "moltbook_upvote" | "moltbook_downvote" => {
            let post_id = args["post_id"].as_str().unwrap_or("").trim();
            if post_id.is_empty() {
                return error_result("post_id là bắt buộc".into());
            }
            let dc = DraftCreate {
                kind: "vote".into(),
                vote_dir: if name == "moltbook_downvote" { "down".into() } else { "up".into() },
                target_post_id: post_id.to_string(),
                source: "agent".into(),
                ..Default::default()
            };
            json_result(enqueue_or_publish(state, dc).await)
        }
        "moltbook_follow" => {
            let n = args["name"].as_str().unwrap_or("").trim();
            if n.is_empty() {
                return error_result("name là bắt buộc".into());
            }
            let dc = DraftCreate { kind: "follow".into(), target_name: n.to_string(), source: "agent".into(), ..Default::default() };
            json_result(enqueue_or_publish(state, dc).await)
        }
        "moltbook_subscribe" => {
            let n = args["name"].as_str().unwrap_or("").trim().trim_start_matches("m/");
            if n.is_empty() {
                return error_result("name là bắt buộc".into());
            }
            let dc = DraftCreate { kind: "subscribe".into(), target_name: n.to_string(), source: "agent".into(), ..Default::default() };
            json_result(enqueue_or_publish(state, dc).await)
        }
        "moltbook_create_submolt" => {
            let n = args["name"].as_str().unwrap_or("").trim().trim_start_matches("m/");
            if n.is_empty() {
                return error_result("name là bắt buộc".into());
            }
            let dc = DraftCreate {
                kind: "submolt".into(),
                submolt: n.to_string(),
                title: args["display_name"].as_str().unwrap_or("").to_string(),
                content: args["description"].as_str().unwrap_or("").to_string(),
                source: "agent".into(),
                ..Default::default()
            };
            json_result(enqueue_or_publish(state, dc).await)
        }

        // ---- approve / run ----
        "moltbook_approve_draft" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id là bắt buộc".into());
            };
            let draft = match db.get_draft(id) {
                Ok(Some(d)) => d,
                Ok(None) => return error_result(format!("draft {id} không tồn tại")),
                Err(e) => return error_result(e.to_string()),
            };
            if draft.status != "pending" {
                return error_result(format!("draft đã ở trạng thái '{}'", draft.status));
            }
            match engine::execute_draft(state, &draft).await {
                Ok(reference) => {
                    db.set_draft_result(id, "posted", &reference, "", now_ts()).ok();
                    db.log(&draft.kind, &format!("duyệt & đăng {} (#{id})", draft.kind), &reference, now_ts()).ok();
                    json_result(json!({ "ok": true, "ref": reference, "draft": db.get_draft(id).ok().flatten() }))
                }
                Err(e) => {
                    db.set_draft_result(id, "error", "", &e, now_ts()).ok();
                    error_result(e)
                }
            }
        }
        "moltbook_reject_draft" => {
            let Some(id) = args["id"].as_i64() else {
                return error_result("id là bắt buộc".into());
            };
            match db.set_draft_result(id, "rejected", "", "", now_ts()) {
                Ok(()) => json_result(json!({ "ok": true, "id": id, "status": "rejected" })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "moltbook_run_heartbeat" => json_result(engine::run_once(state, "mcp").await),

        _ => error_result(format!("Unknown tool: {name}")),
    }
}
