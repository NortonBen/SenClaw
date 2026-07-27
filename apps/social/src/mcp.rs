//! MCP server — hand-rolled JSON-RPC over HTTP + SSE, matching the other Space
//! Apps (the `rmcp` crate is not used here).

use crate::channels::Platform;
use crate::state::AppState;
use crate::web_ops;
use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<AppState>,
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
    State(state): State<AppState>,
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
            "serverInfo": { "name": "social-mcp", "version": "1.0.0" }
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

// ---- argument helpers ----

fn s(args: &Value, key: &str) -> String {
    args.get(key).and_then(|v| v.as_str()).unwrap_or_default().to_string()
}

fn opt_s(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn int(args: &Value, key: &str) -> i64 {
    args.get(key)
        .and_then(|v| v.as_i64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
        .unwrap_or(0)
}

fn parse_platform(args: &Value) -> Result<Platform, Value> {
    let p = s(args, "platform");
    Platform::parse(&p).ok_or_else(|| {
        error_result(format!(
            "platform '{p}' không hợp lệ — chọn: facebook | tiktok | x | instagram | youtube"
        ))
    })
}

fn tools_list() -> Value {
    json!([
      {
        "name": "social_status",
        "description": "Trạng thái tổng quan Social: có bao nhiêu tài khoản đã kết nối theo nền tảng, extension đã kết nối chưa và đang có phiên đăng nhập cho những nền tảng nào. GỌI TOOL NÀY TRƯỚC — nếu extension chưa kết nối thì mọi thao tác tìm kiếm/duyệt/nt đều sẽ báo lỗi.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      {
        "name": "social_ext_status",
        "description": "Chi tiết kết nối của Chrome extension (đã kết nối chưa, uptime, số lần kết nối/ngắt, danh sách host đang có phiên). Dùng để chẩn đoán khi thao tác web báo 'extension chưa kết nối'.",
        "inputSchema": { "type": "object", "properties": {} }
      },
      {
        "name": "social_accounts",
        "description": "Liệt kê các tài khoản mạng xã hội đã kết nối (nền tảng, handle, tên hiển thị, đã có cấu hình API chính thức chưa).",
        "inputSchema": { "type": "object", "properties": {} }
      },
      {
        "name": "social_connect",
        "description": "Khai báo/cập nhật một tài khoản và cấu hình API chính thức của nó (dùng cho đăng bài hợp lệ). KHÔNG lưu token của phiên web — token đó do extension giữ. official_config là JSON tuỳ nền tảng (vd TikTok: {access_token}, Facebook: {page_id, access_token}).",
        "inputSchema": {
          "type": "object",
          "properties": {
            "platform": { "type": "string", "description": "facebook|tiktok|x|instagram|youtube" },
            "handle": { "type": "string", "description": "@username hoặc tên Page" },
            "display_name": { "type": "string" },
            "official_config": { "type": "object", "description": "cấu hình API chính thức (tuỳ nền tảng)" }
          },
          "required": ["platform", "handle"]
        }
      },
      {
        "name": "social_post",
        "description": "Đăng bài qua API CHÍNH THỨC của nền tảng (an toàn nhất, có hạn mức). Cần đã social_connect với official_config hợp lệ. Nếu nền tảng chưa được nối API, tool trả lỗi rõ ràng nêu cần gì.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "platform": { "type": "string" },
            "handle": { "type": "string" },
            "text": { "type": "string", "description": "nội dung bài/caption" }
          },
          "required": ["platform", "handle", "text"]
        }
      },
      {
        "name": "social_search",
        "description": "Tìm kiếm trên nền tảng qua extension (phiên đăng nhập thật). Qua bộ điều tiết nhịp người. Trả kết quả thô do extension replay web-API.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "platform": { "type": "string" },
            "handle": { "type": "string", "description": "tài khoản dùng để tìm (khớp phiên đăng nhập)" },
            "query": { "type": "string" }
          },
          "required": ["platform", "handle", "query"]
        }
      },
      {
        "name": "social_feed",
        "description": "Duyệt feed / bài viết qua extension. Qua bộ điều tiết nhịp người.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "platform": { "type": "string" },
            "handle": { "type": "string" },
            "target": { "type": "string", "description": "feed nguồn: 'home' | @user | url" },
            "limit": { "type": "integer" }
          },
          "required": ["platform", "handle"]
        }
      },
      {
        "name": "social_groups",
        "description": "Duyệt các hội nhóm Facebook mà tài khoản tham gia (chỉ Facebook — các nền tảng khác không có nhóm). Qua extension + bộ điều tiết nhịp.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "handle": { "type": "string", "description": "tài khoản Facebook" },
            "query": { "type": "string", "description": "lọc theo tên nhóm (tuỳ chọn)" }
          },
          "required": ["handle"]
        }
      },
      {
        "name": "social_page_scan",
        "description": "Quét thông tin một Facebook PAGE mà Sếp QUẢN TRỊ, qua Graph API chính thức (ổn định, đúng ToS). kind='info' → tên/hạng mục/số follower/like/website/địa chỉ; kind='feed' → các bài đăng gần đây kèm số reaction/comment/share; kind='insights' → chỉ số thống kê (cần quyền read_insights). CẦN social_connect facebook với official_config {page_id, access_token} (Page token). KHÔNG đọc được Page/profile mà Sếp không quản trị (Meta chặn nếu chưa duyệt PPCA).",
        "inputSchema": {
          "type": "object",
          "properties": {
            "handle": { "type": "string", "description": "tài khoản Facebook đã connect (khớp Page)" },
            "kind": { "type": "string", "description": "info | feed | insights (mặc định info)" },
            "fields": { "type": "string", "description": "info: danh sách field Graph tuỳ chọn" },
            "metric": { "type": "string", "description": "insights: tên metric, phẩy ngăn cách (lưu ý impressions→views từ 15/11/2025)" },
            "period": { "type": "string", "description": "insights: day|week|days_28 (mặc định day)" },
            "limit": { "type": "integer", "description": "feed: số bài (mặc định 10)" }
          },
          "required": ["handle"]
        }
      },
      {
        "name": "social_inbox_poll",
        "description": "Đọc hộp thư đến của tài khoản qua extension (hoặc API Page/Business nơi có). Qua bộ điều tiết nhịp.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "platform": { "type": "string" },
            "handle": { "type": "string" }
          },
          "required": ["platform", "handle"]
        }
      },
      {
        "name": "social_send_dm",
        "description": "Trả lời một tin nhắn (CHỈ phản hồi — không nhắn nguội, theo quy tắc an toàn). Qua extension + bộ điều tiết nhịp. Lưu ý DM là thao tác rủi ro cao nhất về gắn cờ spam.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "platform": { "type": "string" },
            "handle": { "type": "string" },
            "thread_id": { "type": "string", "description": "hội thoại đang trả lời" },
            "text": { "type": "string" }
          },
          "required": ["platform", "handle", "thread_id", "text"]
        }
      },
      {
        "name": "social_autonomy",
        "description": "Xem/đổi chế độ tự chủ: observe (chỉ đọc) | draft (mọi bài/nt thành NHÁP chờ Sếp duyệt — mặc định, an toàn) | live (gửi ngay, vẫn qua nhịp). Không truyền mode = chỉ xem.",
        "inputSchema": {
          "type": "object",
          "properties": { "mode": { "type": "string", "description": "observe|draft|live" } }
        }
      },
      {
        "name": "social_drafts",
        "description": "Liệt kê các nháp (bài/trả lời) chờ duyệt. Lọc theo status (pending|sent|rejected) tuỳ chọn.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "status": { "type": "string" },
            "limit": { "type": "integer" }
          }
        }
      },
      {
        "name": "social_approve",
        "description": "Duyệt và GỬI một nháp (đăng bài qua API chính thức, hoặc gửi trả lời qua extension). Qua bộ điều tiết nhịp. Nếu lỗi (vd thiếu cấu hình), nháp vẫn giữ pending để sửa rồi duyệt lại.",
        "inputSchema": {
          "type": "object",
          "properties": { "draft_id": { "type": "integer" } },
          "required": ["draft_id"]
        }
      },
      {
        "name": "social_reject",
        "description": "Bỏ một nháp (không gửi).",
        "inputSchema": {
          "type": "object",
          "properties": { "draft_id": { "type": "integer" } },
          "required": ["draft_id"]
        }
      },
      {
        "name": "social_post_log",
        "description": "Lịch sử các lần đăng bài (nền tảng, ref_id, thành công/lỗi, thời điểm). Dùng để kiểm chứng đã đăng thật chưa thay vì tin lời.",
        "inputSchema": {
          "type": "object",
          "properties": { "limit": { "type": "integer", "description": "mặc định 20" } }
        }
      },
      {
        "name": "social_action_log",
        "description": "Nhật ký AUDIT các hành động API: mỗi lần đăng/tìm/duyệt/nt được điều tiết (reserved) hay bị chặn (blocked) vì chạm hạn mức, kèm lý do. Dùng để soi app đã/đang làm gì với nền tảng.",
        "inputSchema": {
          "type": "object",
          "properties": { "limit": { "type": "integer", "description": "mặc định 30" } }
        }
      },
      {
        "name": "social_sessions",
        "description": "Lịch sử phiên đăng nhập: mỗi lần một nền tảng có phiên (online) hoặc mất phiên (offline) theo báo cáo của extension. Dùng để biết khi nào Sếp đã đăng nhập nền tảng nào.",
        "inputSchema": {
          "type": "object",
          "properties": { "limit": { "type": "integer", "description": "mặc định 30" } }
        }
      },
      {
        "name": "social_inbox_list",
        "description": "Đọc các tin nhắn đã lưu (vào/ra) — kể cả các câu trả lời đã gửi. Lọc theo platform tuỳ chọn.",
        "inputSchema": {
          "type": "object",
          "properties": {
            "platform": { "type": "string" },
            "limit": { "type": "integer", "description": "mặc định 20" }
          }
        }
      }
    ])
}

async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    match name {
        "social_status" => {
            let accounts = state.core.db.list_accounts().unwrap_or_default();
            let mut by_platform = serde_json::Map::new();
            for a in &accounts {
                let e = by_platform.entry(a.platform.clone()).or_insert(json!(0));
                *e = json!(e.as_i64().unwrap_or(0) + 1);
            }
            let db = &state.core.db;
            let pending = db.list_drafts(Some("pending"), 1000).map(|d| d.len()).unwrap_or(0);
            // What each platform can actually do, and how — so the agent never
            // asks for a capability a platform doesn't have (e.g. DM on Threads).
            let mut caps = serde_json::Map::new();
            for p in Platform::ALL {
                let mut m = serde_json::Map::new();
                for c in Platform::CAPS {
                    m.insert(c.to_string(), json!(p.capability(c).as_str()));
                }
                caps.insert(p.as_str().to_string(), Value::Object(m));
            }
            json_result(json!({
                "platforms": Platform::ALL.iter().map(|p| p.as_str()).collect::<Vec<_>>(),
                "capabilities": caps,
                "autonomy": db.autonomy(),
                "accounts_total": accounts.len(),
                "accounts_by_platform": by_platform,
                "drafts_pending": pending,
                "posts_logged": db.recent_posts(100000).map(|p| p.len()).unwrap_or(0),
                "actions_logged": db.recent_actions(100000).map(|a| a.len()).unwrap_or(0),
                "extension_connected": state.ext.is_connected(),
                "extension_hosts_ready": state.ext.hosts_ready(),
                "note": "Đăng bài ưu tiên API chính thức (social_post). Tìm kiếm/duyệt/nt cần extension kết nối + đăng nhập nền tảng. Chế độ draft: bài/nt thành nháp chờ social_approve.",
            }))
        }
        "social_ext_status" => json_result(state.ext.stats()),
        "social_accounts" => {
            let accounts = state.core.db.list_accounts().unwrap_or_default();
            let view: Vec<Value> = accounts
                .iter()
                .map(|a| {
                    json!({
                        "id": a.id,
                        "platform": a.platform,
                        "handle": a.handle,
                        "display_name": a.display_name,
                        "official_configured": a.official_config.as_object().map(|m| !m.is_empty()).unwrap_or(false),
                        "enabled": a.enabled,
                    })
                })
                .collect();
            json_result(json!({ "accounts": view }))
        }
        "social_connect" => {
            let platform = match parse_platform(args) {
                Ok(p) => p,
                Err(e) => return e,
            };
            let handle = s(args, "handle");
            if handle.is_empty() {
                return error_result("thiếu 'handle'".into());
            }
            let display = opt_s(args, "display_name").unwrap_or_else(|| handle.clone());
            let cfg = args.get("official_config").cloned().unwrap_or(json!({}));
            match state.core.db.upsert_account(platform.as_str(), &handle, &display, &cfg) {
                Ok(id) => json_result(json!({
                    "ok": true, "id": id, "platform": platform.as_str(), "handle": handle,
                    "official_note": platform.official_note(),
                })),
                Err(e) => error_result(format!("lưu tài khoản lỗi: {e}")),
            }
        }
        "social_post" => {
            let platform = match parse_platform(args) {
                Ok(p) => p,
                Err(e) => return e,
            };
            let handle = s(args, "handle");
            let text = s(args, "text");
            if text.is_empty() {
                return error_result("thiếu 'text'".into());
            }
            // Through the autonomy gate (draft→approve→live) + cadence.
            match crate::gate::submit(state, "post", platform, &handle, &text, "", &json!([])).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }
        "social_search" => {
            let platform = match parse_platform(args) {
                Ok(p) => p,
                Err(e) => return e,
            };
            // Route by the platform's declared strategy: Threads/YouTube search
            // is an OFFICIAL API path, not an extension replay.
            if platform.capability("search") == crate::channels::Capability::Official {
                let handle = s(args, "handle");
                let query = s(args, "query");
                if query.is_empty() {
                    return error_result("thiếu 'query'".into());
                }
                let cfg = state.core.db.official_config(platform.as_str(), &handle);
                state.core.db.log_action(platform.as_str(), "search", "official", &query);
                return match crate::channels::official_search(platform, &cfg, &query).await {
                    Ok(v) => json_result(v),
                    Err(e) => error_result(e),
                };
            }
            web_op(state, args, "search", "search").await
        }
        "social_feed" => web_op(state, args, "feed", "feed").await,
        "social_inbox_poll" => {
            let platform = match parse_platform(args) {
                Ok(p) => p,
                Err(e) => return e,
            };
            let handle = s(args, "handle");
            if handle.is_empty() {
                return error_result("thiếu 'handle'".into());
            }
            match web_ops::run(state, platform, &handle, "inbox", "inbox_poll", args.clone()).await {
                Ok(v) => {
                    // Parse the extension's reply into structured inbound rows and
                    // persist them (direction "in"), so they become a real inbox
                    // an operator — or a downstream app like CRM — can pull.
                    let stored = persist_inbound(state, platform.as_str(), &v);
                    json_result(json!({ "stored": stored, "raw": v }))
                }
                Err(e) => error_result(e),
            }
        }
        "social_send_dm" => {
            let platform = match parse_platform(args) {
                Ok(p) => p,
                Err(e) => return e,
            };
            let handle = s(args, "handle");
            let thread_id = s(args, "thread_id");
            let text = s(args, "text");
            if thread_id.is_empty() || text.is_empty() {
                return error_result("cần 'thread_id' và 'text' (DM chỉ để trả lời)".into());
            }
            // Reactive DM through the autonomy gate too.
            match crate::gate::submit(state, "reply", platform, &handle, &text, &thread_id, &json!([])).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }
        "social_autonomy" => {
            if let Some(mode) = opt_s(args, "mode") {
                if !matches!(mode.as_str(), "observe" | "draft" | "live") {
                    return error_result("mode phải là observe | draft | live".into());
                }
                if let Err(e) = state.core.db.set_setting("autonomy", &mode) {
                    return error_result(format!("lưu lỗi: {e}"));
                }
            }
            json_result(json!({
                "autonomy": state.core.db.autonomy(),
                "meaning": "observe=chỉ đọc · draft=tạo nháp chờ duyệt · live=gửi ngay (vẫn qua nhịp)"
            }))
        }
        "social_drafts" => {
            let status = opt_s(args, "status");
            let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20).clamp(1, 200);
            json_result(json!({
                "drafts": state.core.db.list_drafts(status.as_deref(), limit).unwrap_or_default()
            }))
        }
        "social_approve" => {
            let id = int(args, "draft_id");
            let draft = match state.core.db.get_draft(id) {
                Ok(Some(d)) => d,
                _ => return error_result(format!("không thấy nháp #{id}")),
            };
            if draft["status"] != "pending" {
                return error_result(format!("nháp #{id} không còn ở trạng thái pending"));
            }
            let platform = match crate::channels::Platform::parse(draft["platform"].as_str().unwrap_or("")) {
                Some(p) => p,
                None => return error_result("nháp có platform không hợp lệ".into()),
            };
            let handle = draft["handle"].as_str().unwrap_or("");
            let kind = draft["kind"].as_str().unwrap_or("post");
            let text = draft["text"].as_str().unwrap_or("");
            let thread_id = draft["thread_id"].as_str().unwrap_or("");
            match crate::gate::execute_write(state, kind, platform, handle, text, thread_id).await {
                Ok(ref_id) => {
                    let _ = state.core.db.set_draft_status(id, "sent", &ref_id, "");
                    json_result(json!({ "ok": true, "draft_id": id, "ref_id": ref_id }))
                }
                Err(e) => {
                    // Leave the draft pending so it can be retried after fixing config.
                    let _ = state.core.db.set_draft_status(id, "pending", "", &e);
                    error_result(format!("gửi nháp #{id} lỗi: {e}"))
                }
            }
        }
        "social_reject" => {
            let id = int(args, "draft_id");
            match state.core.db.set_draft_status(id, "rejected", "", "rejected by user") {
                Ok(()) => json_result(json!({ "ok": true, "draft_id": id, "status": "rejected" })),
                Err(e) => error_result(format!("lỗi: {e}")),
            }
        }
        "social_post_log" => {
            let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20).clamp(1, 200);
            json_result(json!({ "posts": state.core.db.recent_posts(limit).unwrap_or_default() }))
        }
        "social_action_log" => {
            let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(30).clamp(1, 200);
            json_result(json!({ "actions": state.core.db.recent_actions(limit).unwrap_or_default() }))
        }
        "social_sessions" => {
            let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(30).clamp(1, 200);
            json_result(json!({ "sessions": state.core.db.recent_sessions(limit).unwrap_or_default() }))
        }
        "social_inbox_list" => {
            let platform = opt_s(args, "platform");
            let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20).clamp(1, 200);
            json_result(json!({
                "messages": state.core.db.list_inbox(platform.as_deref(), limit).unwrap_or_default()
            }))
        }
        "social_groups" => {
            // Facebook-only; force the platform.
            let handle = s(args, "handle");
            if handle.is_empty() {
                return error_result("thiếu 'handle'".into());
            }
            let params = json!({ "query": opt_s(args, "query").unwrap_or_default() });
            match web_ops::run(state, Platform::Facebook, &handle, "groups", "groups", params).await {
                Ok(v) => json_result(v),
                Err(e) => error_result(e),
            }
        }
        "social_page_scan" => {
            // Reliable, ToS-clean read of a Page the user MANAGES, via Graph API.
            let handle = s(args, "handle");
            if handle.is_empty() {
                return error_result("thiếu 'handle'".into());
            }
            let cfg = state.core.db.official_config("facebook", &handle);
            let kind = opt_s(args, "kind").unwrap_or_else(|| "info".into());
            let res = match kind.as_str() {
                "feed" => {
                    let limit = if int(args, "limit") > 0 { int(args, "limit") } else { 10 };
                    crate::channels::facebook::page_feed(&cfg, limit).await
                }
                "insights" => {
                    crate::channels::facebook::page_insights(
                        &cfg,
                        &opt_s(args, "metric").unwrap_or_default(),
                        &opt_s(args, "period").unwrap_or_default(),
                    )
                    .await
                }
                _ => crate::channels::facebook::page_info(&cfg, &opt_s(args, "fields").unwrap_or_default()).await,
            };
            match res {
                Ok(v) => {
                    state.core.db.log_action("facebook", "page_scan", "ok", &kind);
                    json_result(v)
                }
                Err(e) => {
                    state.core.db.log_action("facebook", "page_scan", "error", &e);
                    error_result(e)
                }
            }
        }
        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

/// Parse an extension inbox reply into structured inbound rows and persist the
/// new ones (direction "in"). Accepts a `messages` array where each item has an
/// `external_id` (or `thread_id`), a `sender` (or `sender_name`/`from`), and
/// `text`. Returns how many new rows were stored.
fn persist_inbound(state: &AppState, platform: &str, v: &Value) -> usize {
    let msgs = v.get("messages").and_then(|m| m.as_array());
    let Some(msgs) = msgs else { return 0 };
    let pick = |m: &Value, keys: &[&str]| -> String {
        for k in keys {
            if let Some(s) = m.get(*k).and_then(|x| x.as_str()) {
                if !s.is_empty() {
                    return s.to_string();
                }
            }
        }
        String::new()
    };
    let mut stored = 0;
    for m in msgs {
        let text = pick(m, &["text", "message", "content"]);
        if text.is_empty() {
            continue;
        }
        let external_id = pick(m, &["external_id", "thread_id", "conversation_id", "chat_id"]);
        let sender = pick(m, &["sender", "sender_name", "from", "author"]);
        if state.core.db.inbox_contains(platform, &external_id, &text) {
            continue; // idempotent across repeated polls
        }
        if state.core.db.insert_inbox(platform, &external_id, &sender, "in", &text).is_ok() {
            stored += 1;
        }
    }
    stored
}

/// Shared body for extension-backed tools that take (platform, handle, ...).
async fn web_op(state: &AppState, args: &Value, action: &str, op: &str) -> Value {
    let platform = match parse_platform(args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let handle = s(args, "handle");
    if handle.is_empty() {
        return error_result("thiếu 'handle'".into());
    }
    // Pass all args through as params (extension picks what it needs).
    let params = args.clone();
    match web_ops::run(state, platform, &handle, action, op, params).await {
        Ok(v) => json_result(v),
        Err(e) => error_result(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cadence::Cadence;
    use crate::db::Db;
    use crate::extbridge::ExtBridge;
    use crate::state::{AppState, Core};
    use std::sync::Arc;

    fn state() -> AppState {
        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        AppState {
            core: Arc::new(Core { db: Db::open_memory().unwrap() }),
            mcp_tx,
            ext: ExtBridge::new(),
            cadence: Arc::new(Cadence::new()),
        }
    }

    fn tool_names() -> Vec<String> {
        tools_list()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    /// Every tool advertised by tools/list must actually route in call_tool —
    /// i.e. none may fall through to the "tool không tồn tại" arm. Guards against
    /// listing a tool whose match arm was removed/renamed.
    #[tokio::test]
    async fn every_listed_tool_is_routed() {
        let st = state();
        let names = tool_names();
        assert!(names.len() >= 18, "expected the full tool set, got {}", names.len());
        for name in names {
            let out = call_tool(&st, &name, &json!({})).await;
            let text = out["content"][0]["text"].as_str().unwrap_or_default();
            assert!(
                !text.contains(&format!("tool không tồn tại: {name}")),
                "tool '{name}' được liệt kê nhưng không có match arm trong call_tool"
            );
        }
    }

    #[test]
    fn persist_inbound_stores_new_and_dedups_repeats() {
        let st = state();
        let reply = json!({ "messages": [
            { "external_id": "t1", "sender_name": "Khách A", "text": "giá bao nhiêu?" },
            { "thread_id": "t2", "from": "Khách B", "message": "còn hàng không?" },
            { "external_id": "t3", "text": "" },            // empty text → skipped
        ]});
        assert_eq!(persist_inbound(&st, "facebook", &reply), 2);
        // Re-polling the same batch stores nothing new (idempotent).
        assert_eq!(persist_inbound(&st, "facebook", &reply), 0);

        let feed = st.core.db.inbox_since(0, 100).unwrap();
        assert_eq!(feed.len(), 2);
        assert_eq!(feed[0]["sender"], "Khách A");
        assert_eq!(feed[0]["external_id"], "t1");
        assert_eq!(feed[1]["text"], "còn hàng không?");
    }

    #[test]
    fn persist_inbound_ignores_a_reply_without_messages() {
        let st = state();
        assert_eq!(persist_inbound(&st, "x", &json!({ "not_wired": true })), 0);
    }

    /// `social_search` must route by the platform's declared strategy: Threads
    /// and YouTube go to their OFFICIAL search API (so with no extension
    /// connected they still report a config problem, never an extension one),
    /// while replay-based platforms are sent to the extension.
    #[tokio::test]
    async fn search_routes_official_vs_extension_per_platform() {
        let st = state(); // extension never connected
        for p in ["threads", "youtube"] {
            let out = call_tool(&st, "social_search", &json!({"platform": p, "handle": "@a", "query": "áo"})).await;
            let text = out["content"][0]["text"].as_str().unwrap();
            assert!(text.contains("API chính thức"), "{p} should use the official API: {text}");
            assert!(!text.contains("Extension chưa kết nối"), "{p} must not go to the extension");
        }
        for p in ["facebook", "x", "instagram", "tiktok"] {
            let out = call_tool(&st, "social_search", &json!({"platform": p, "handle": "@a", "query": "áo"})).await;
            let text = out["content"][0]["text"].as_str().unwrap();
            assert!(text.contains("Extension chưa kết nối"), "{p} should use the extension: {text}");
        }
    }

    #[tokio::test]
    async fn unknown_tool_is_reported() {
        let st = state();
        let out = call_tool(&st, "social_bogus", &json!({})).await;
        assert_eq!(out["isError"], json!(true));
        assert!(out["content"][0]["text"].as_str().unwrap().contains("không tồn tại"));
    }
}
