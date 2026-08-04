//! MCP server (HTTP + SSE) exposing the Facebook Page operations to SenClaw
//! agents. Every write that creates public content goes through the SAME
//! draft-approve gate the UI uses ([`crate::api::enqueue_or_send`] /
//! [`crate::api::send_draft`]) so an agent can never bypass the human-approval
//! default: in `draft` mode a post/comment/reply becomes a queued draft, and only
//! `fb_approve` (or `live` mode) actually calls the Graph API. There is no
//! bulk/broadcast tool.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::convert::Infallible;

use crate::api::{self, AppState};
use crate::db::DraftInput;

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
fn json_result(v: &Value) -> Value {
    text_result(serde_json::to_string_pretty(v).unwrap_or_default())
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
            "serverInfo": { "name": "facebook-mcp", "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => {
            Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} }))
        }
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
        { "name": "fb_status", "description": "Trạng thái Facebook Pro: đã cấu hình App ID/Secret chưa, đã kết nối (user token) chưa, Trang đang chọn, số Trang, autonomy (observe/draft/live), số nháp chờ duyệt.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "fb_connect_link", "description": "Sinh link đăng nhập OAuth (Facebook Login dialog) để admin tự bấm cấp quyền cho Developer App. Trả về URL; con người phải tự mở & đồng ý. KHÔNG tự động hoá bước đồng ý.", "inputSchema": { "type": "object", "properties": { "redirect": { "type": "string", "description": "Redirect URI đã whitelist trong app (vd http://127.0.0.1:4590/api/oauth/callback)." } }, "required": ["redirect"] } },
        { "name": "fb_connect_token", "description": "Kết nối bằng User Access Token dán từ Graph API Explorer: đổi sang token dài hạn (~60 ngày) và lấy danh sách Trang + Page Access Token. Chỉ dùng token của chính admin.", "inputSchema": { "type": "object", "properties": { "user_token": { "type": "string" } }, "required": ["user_token"] } },
        { "name": "fb_pages", "description": "Danh sách các Trang (Fanpage) đã kết nối + Trang đang chọn. Làm mới từ Graph nếu có user token.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "fb_select_page", "description": "Chọn Trang active để các thao tác sau tác động vào (nếu không truyền page_id ở từng tool).", "inputSchema": { "type": "object", "properties": { "page_id": { "type": "string" } }, "required": ["page_id"] } },
        { "name": "fb_posts", "description": "Liệt kê bài đăng gần đây của Trang (kèm tóm tắt tương tác).", "inputSchema": { "type": "object", "properties": { "page_id": { "type": "string" }, "limit": { "type": "number" } } } },
        { "name": "fb_post_get", "description": "Chi tiết một bài viết theo post_id (nội dung + tương tác).", "inputSchema": { "type": "object", "properties": { "post_id": { "type": "string" }, "page_id": { "type": "string" } }, "required": ["post_id"] } },
        { "name": "fb_post_create", "description": "SOẠN một bài đăng (chữ/link, hoặc ẢNH nếu truyền image_url) vào hàng chờ duyệt (draft-first). KHÔNG đăng ngay trừ khi autonomy=live.", "inputSchema": { "type": "object", "properties": { "page_id": { "type": "string" }, "message": { "type": "string" }, "link": { "type": "string" }, "image_url": { "type": "string", "description": "URL ảnh công khai để đăng bài ảnh." } }, "required": ["message"] } },
        { "name": "fb_post_edit", "description": "SOẠN chỉnh sửa nội dung một bài viết (draft-first).", "inputSchema": { "type": "object", "properties": { "post_id": { "type": "string" }, "message": { "type": "string" }, "page_id": { "type": "string" } }, "required": ["post_id", "message"] } },
        { "name": "fb_post_delete", "description": "XOÁ một bài viết của Trang (thao tác tức thời, không tự động hoá — chỉ chạy khi được yêu cầu rõ ràng).", "inputSchema": { "type": "object", "properties": { "post_id": { "type": "string" }, "page_id": { "type": "string" } }, "required": ["post_id"] } },
        { "name": "fb_comments", "description": "Đọc bình luận của một bài viết (hoặc reply của một bình luận) theo object_id.", "inputSchema": { "type": "object", "properties": { "object_id": { "type": "string" }, "page_id": { "type": "string" }, "limit": { "type": "number" } }, "required": ["object_id"] } },
        { "name": "fb_comment_create", "description": "SOẠN một bình luận lên một bài viết (draft-first).", "inputSchema": { "type": "object", "properties": { "object_id": { "type": "string", "description": "post_id để bình luận." }, "message": { "type": "string" }, "page_id": { "type": "string" } }, "required": ["object_id", "message"] } },
        { "name": "fb_comment_reply", "description": "SOẠN một câu TRẢ LỜI cho một bình luận (draft-first). Nếu bỏ trống 'message' thì AI tự soạn từ 'comment_text' + 'hint'.", "inputSchema": { "type": "object", "properties": { "comment_id": { "type": "string" }, "message": { "type": "string" }, "comment_text": { "type": "string", "description": "Nội dung bình luận gốc để AI soạn." }, "hint": { "type": "string", "description": "Định hướng trả lời." }, "page_id": { "type": "string" } }, "required": ["comment_id"] } },
        { "name": "fb_like", "description": "Thả like một đối tượng (bài viết/bình luận) — thao tác tức thời, không tự động hoá.", "inputSchema": { "type": "object", "properties": { "object_id": { "type": "string" }, "page_id": { "type": "string" } }, "required": ["object_id"] } },
        { "name": "fb_overview", "description": "Tổng quan tương tác Trang: tổng reactions/comments/shares của các bài gần đây, top bài theo tương tác, và số nháp chờ. Dùng để thống kê nhanh.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "fb_conversations", "description": "Danh sách hội thoại (tin nhắn Messenger) của Trang: snippet, số tin, chưa đọc, người tham gia. Cần quyền pages_messaging.", "inputSchema": { "type": "object", "properties": { "page_id": { "type": "string" }, "limit": { "type": "number" } } } },
        { "name": "fb_conversation_messages", "description": "Các tin nhắn trong một hội thoại (thread) theo conversation id (t_...).", "inputSchema": { "type": "object", "properties": { "conversation_id": { "type": "string" }, "page_id": { "type": "string" }, "limit": { "type": "number" } }, "required": ["conversation_id"] } },
        { "name": "fb_message_reply", "description": "SOẠN một tin nhắn trả lời người dùng (draft-first, gửi qua Send API dạng RESPONSE — không broadcast). Bỏ trống 'message' thì AI tự soạn từ 'customer_msg' + 'hint'. recipient_id là PSID của người dùng.", "inputSchema": { "type": "object", "properties": { "recipient_id": { "type": "string" }, "message": { "type": "string" }, "customer_msg": { "type": "string" }, "hint": { "type": "string" }, "page_id": { "type": "string" } }, "required": ["recipient_id"] } },
        { "name": "fb_analyze", "description": "Phân tích một bài viết bằng AI (điểm mạnh/yếu, gợi ý, mức tương tác). Truyền post_id để lấy dữ liệu thật, hoặc message để phân tích bản nháp.", "inputSchema": { "type": "object", "properties": { "post_id": { "type": "string" }, "message": { "type": "string" }, "page_id": { "type": "string" } } } },
        { "name": "fb_page_insights", "description": "Thống kê cấp Trang (Insights API). metric mặc định: page_impressions,page_post_engagements,page_fans. period: day/week/days_28.", "inputSchema": { "type": "object", "properties": { "page_id": { "type": "string" }, "metric": { "type": "string" }, "period": { "type": "string" } } } },
        { "name": "fb_post_insights", "description": "Thống kê cấp bài viết theo post_id (Insights API).", "inputSchema": { "type": "object", "properties": { "post_id": { "type": "string" }, "metric": { "type": "string" }, "page_id": { "type": "string" } }, "required": ["post_id"] } },
        { "name": "fb_ad_accounts", "description": "Liệt kê Tài khoản quảng cáo (ad accounts) truy cập được + tài khoản đang chọn. Cần user token có quyền ads_read.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "fb_ad_select_account", "description": "Chọn Tài khoản quảng cáo active cho các phân tích ads sau (tự thêm tiền tố act_).", "inputSchema": { "type": "object", "properties": { "account_id": { "type": "string" } }, "required": ["account_id"] } },
        { "name": "fb_ad_campaigns", "description": "Liệt kê chiến dịch của tài khoản quảng cáo (id, tên, trạng thái, mục tiêu, ngân sách).", "inputSchema": { "type": "object", "properties": { "account_id": { "type": "string" } } } },
        { "name": "fb_ads_insights", "description": "Chỉ số quảng cáo (CTR, CPC, CPM, chi tiêu, reach, kết quả, ROAS) theo level. object_id: act_<id> | campaign | adset | ad (bỏ trống = tài khoản đang chọn). level: account|campaign|adset|ad. date_preset: last_7d|last_30d|today|maximum...", "inputSchema": { "type": "object", "properties": { "object_id": { "type": "string" }, "level": { "type": "string" }, "date_preset": { "type": "string" } } } },
        { "name": "fb_ads_analyze", "description": "Phân tích hiệu quả quảng cáo bằng AI dựa trên số liệu thật: đọc CTR/CPC/CPM/chi tiêu/kết quả/ROAS, kết luận HIỆU QUẢ/THEO DÕI/ĐỐT TIỀN cho từng chiến dịch, và có NÊN TẮT hay không. Trả về cả bảng rows + verdict.", "inputSchema": { "type": "object", "properties": { "object_id": { "type": "string" }, "level": { "type": "string" }, "date_preset": { "type": "string" }, "currency": { "type": "string" } } } },
        { "name": "fb_ad_status", "description": "TẮT/BẬT một chiến dịch/nhóm QC/quảng cáo (PAUSED|ACTIVE) — thao tác tức thời trên tài khoản QC của bạn, chỉ chạy khi được yêu cầu rõ ràng (vd khi ad đang đốt tiền).", "inputSchema": { "type": "object", "properties": { "entity_id": { "type": "string", "description": "campaign_id / adset_id / ad_id." }, "status": { "type": "string", "description": "PAUSED hoặc ACTIVE." } }, "required": ["entity_id", "status"] } },
        { "name": "fb_drafts", "description": "Liệt kê các bản nháp (bài/bình luận/trả lời) đang chờ duyệt.", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "fb_approve", "description": "DUYỆT & ĐĂNG một bản nháp — cổng DUY NHẤT thực sự gọi Graph API để đăng/trả lời. Chỉ dùng khi con người đã đồng ý.", "inputSchema": { "type": "object", "properties": { "draft_id": { "type": "number" } }, "required": ["draft_id"] } },
        { "name": "fb_reject", "description": "Bỏ một bản nháp mà không đăng.", "inputSchema": { "type": "object", "properties": { "draft_id": { "type": "number" } }, "required": ["draft_id"] } },
        { "name": "fb_triggers", "description": "Liệt kê các trigger theo luật (new_comment → draft_reply/notify).", "inputSchema": { "type": "object", "properties": {} } },
        { "name": "fb_trigger_create", "description": "Tạo một trigger: khi có bình luận mới khớp luật thì soạn nháp trả lời hoặc ghi thông báo. match_type: all|keyword|question. action: draft_reply|notify.", "inputSchema": { "type": "object", "properties": { "name": { "type": "string" }, "page_id": { "type": "string", "description": "Bỏ trống = áp cho mọi Trang." }, "match_type": { "type": "string" }, "match_value": { "type": "string", "description": "CSV từ khoá khi match_type=keyword." }, "action": { "type": "string" }, "reply_hint": { "type": "string" } }, "required": ["name"] } },
        { "name": "fb_trigger_delete", "description": "Xoá một trigger theo id.", "inputSchema": { "type": "object", "properties": { "id": { "type": "number" } }, "required": ["id"] } },
        { "name": "fb_autonomy_set", "description": "Đặt chế độ tự chủ: observe (chỉ đọc) | draft (soạn nháp, mặc định) | live (tự đăng).", "inputSchema": { "type": "object", "properties": { "mode": { "type": "string" } }, "required": ["mode"] } },
        { "name": "fb_tick", "description": "Chạy một nhịp heartbeat ngay: quét bình luận mới, áp trigger, soạn nháp/ghi thông báo (không đăng trừ live). Tôn trọng autonomy gate.", "inputSchema": { "type": "object", "properties": {} } }
    ])
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
}

async fn call_tool(s: &AppState, name: &str, args: &Value) -> Value {
    match name {
        "fb_status" => json_result(&api::status_value(s)),
        "fb_connect_link" => {
            let Some(redirect) = str_arg(args, "redirect") else {
                return error_result("thiếu 'redirect'".into());
            };
            match api::client_from_settings(&s.db) {
                Some(client) => json_result(&json!({ "url": client.connect_url(redirect) })),
                None => error_result("chưa cấu hình App ID/App Secret".into()),
            }
        }
        "fb_connect_token" => {
            let Some(tok) = str_arg(args, "user_token") else {
                return error_result("thiếu 'user_token'".into());
            };
            json_result(&api::connect_with_token(s, tok).await)
        }
        "fb_pages" => json_result(&api::pages_value(s).await),
        "fb_select_page" => {
            let Some(pid) = str_arg(args, "page_id") else {
                return error_result("thiếu 'page_id'".into());
            };
            if s.db.page_token(pid).is_none() {
                return error_result("page_id không có trong danh sách đã kết nối".into());
            }
            let _ = s.db.set_setting("active_page_id", pid);
            json_result(&json!({ "ok": true, "active_page_id": pid }))
        }
        "fb_posts" => {
            let limit = args.get("limit").and_then(|x| x.as_i64()).unwrap_or(15);
            json_result(&api::posts_value(s, str_arg(args, "page_id"), limit).await)
        }
        "fb_post_get" => {
            let Some(id) = str_arg(args, "post_id") else {
                return error_result("thiếu 'post_id'".into());
            };
            json_result(&api::post_get_value(s, id, str_arg(args, "page_id")).await)
        }
        "fb_post_create" => {
            let Some(message) = str_arg(args, "message") else {
                return error_result("thiếu 'message'".into());
            };
            let image_url = str_arg(args, "image_url").unwrap_or("");
            let pid = str_arg(args, "page_id")
                .map(|s| s.to_string())
                .or_else(|| s.db.active_page_id())
                .unwrap_or_default();
            let d = DraftInput {
                kind: if image_url.is_empty() {
                    "post".into()
                } else {
                    "photo".into()
                },
                page_id: pid,
                message: message.into(),
                link: str_arg(args, "link").unwrap_or("").into(),
                image_url: image_url.into(),
                source: "agent".into(),
                ..Default::default()
            };
            json_result(&api::enqueue_or_send(s, d).await)
        }
        "fb_post_edit" => {
            let (Some(post_id), Some(message)) =
                (str_arg(args, "post_id"), str_arg(args, "message"))
            else {
                return error_result("cần 'post_id' và 'message'".into());
            };
            let pid = str_arg(args, "page_id")
                .map(|s| s.to_string())
                .or_else(|| s.db.active_page_id())
                .unwrap_or_default();
            let d = DraftInput {
                kind: "edit".into(),
                page_id: pid,
                target_id: post_id.into(),
                message: message.into(),
                source: "agent".into(),
                ..Default::default()
            };
            json_result(&api::enqueue_or_send(s, d).await)
        }
        "fb_post_delete" => {
            let Some(post_id) = str_arg(args, "post_id") else {
                return error_result("thiếu 'post_id'".into());
            };
            json_result(&api::delete_post_value(s, post_id, str_arg(args, "page_id")).await)
        }
        "fb_comments" => {
            let Some(object_id) = str_arg(args, "object_id") else {
                return error_result("thiếu 'object_id'".into());
            };
            let limit = args.get("limit").and_then(|x| x.as_i64()).unwrap_or(25);
            json_result(&api::comments_value(s, object_id, str_arg(args, "page_id"), limit).await)
        }
        "fb_comment_create" => {
            let (Some(object_id), Some(message)) =
                (str_arg(args, "object_id"), str_arg(args, "message"))
            else {
                return error_result("cần 'object_id' và 'message'".into());
            };
            let pid = str_arg(args, "page_id")
                .map(|s| s.to_string())
                .or_else(|| s.db.active_page_id())
                .unwrap_or_default();
            let d = DraftInput {
                kind: "comment".into(),
                page_id: pid,
                target_id: object_id.into(),
                message: message.into(),
                source: "agent".into(),
                ..Default::default()
            };
            json_result(&api::enqueue_or_send(s, d).await)
        }
        "fb_comment_reply" => {
            let Some(comment_id) = str_arg(args, "comment_id") else {
                return error_result("thiếu 'comment_id'".into());
            };
            let pid = str_arg(args, "page_id")
                .map(|s| s.to_string())
                .or_else(|| s.db.active_page_id())
                .unwrap_or_default();
            let (message, model) = match str_arg(args, "message") {
                Some(m) => (m.to_string(), String::new()),
                None => {
                    let page_name =
                        s.db.list_pages()
                            .into_iter()
                            .find(|p| p.get("page_id").and_then(|x| x.as_str()) == Some(&pid))
                            .and_then(|p| {
                                p.get("name")
                                    .and_then(|x| x.as_str())
                                    .map(|s| s.to_string())
                            })
                            .unwrap_or_else(|| "Trang".into());
                    crate::llm::compose_reply(
                        &s.sc,
                        &page_name,
                        str_arg(args, "comment_text").unwrap_or(""),
                        str_arg(args, "hint").unwrap_or(""),
                    )
                    .await
                }
            };
            let d = DraftInput {
                kind: "reply".into(),
                page_id: pid,
                target_id: comment_id.into(),
                message,
                model,
                source: "agent".into(),
                ..Default::default()
            };
            json_result(&api::enqueue_or_send(s, d).await)
        }
        "fb_like" => {
            let Some(object_id) = str_arg(args, "object_id") else {
                return error_result("thiếu 'object_id'".into());
            };
            json_result(&api::like_value(s, object_id, str_arg(args, "page_id")).await)
        }
        "fb_overview" => json_result(&api::overview_value(s).await),
        "fb_conversations" => {
            let limit = args.get("limit").and_then(|x| x.as_i64()).unwrap_or(25);
            json_result(&api::conversations_value(s, str_arg(args, "page_id"), limit).await)
        }
        "fb_conversation_messages" => {
            let Some(cid) = str_arg(args, "conversation_id") else {
                return error_result("thiếu 'conversation_id'".into());
            };
            let limit = args.get("limit").and_then(|x| x.as_i64()).unwrap_or(25);
            json_result(
                &api::conversation_messages_value(s, cid, str_arg(args, "page_id"), limit).await,
            )
        }
        "fb_message_reply" => {
            let Some(recipient) = str_arg(args, "recipient_id") else {
                return error_result("thiếu 'recipient_id'".into());
            };
            json_result(
                &api::message_reply_value(
                    s,
                    str_arg(args, "page_id"),
                    recipient,
                    str_arg(args, "message"),
                    str_arg(args, "customer_msg"),
                    str_arg(args, "hint"),
                    "agent",
                )
                .await,
            )
        }
        "fb_analyze" => json_result(
            &api::analyze_value(
                s,
                str_arg(args, "post_id"),
                str_arg(args, "message"),
                str_arg(args, "page_id"),
            )
            .await,
        ),
        "fb_page_insights" => json_result(
            &api::page_insights_value(
                s,
                str_arg(args, "page_id"),
                str_arg(args, "metric"),
                str_arg(args, "period"),
            )
            .await,
        ),
        "fb_post_insights" => {
            let Some(post_id) = str_arg(args, "post_id") else {
                return error_result("thiếu 'post_id'".into());
            };
            json_result(
                &api::post_insights_value(
                    s,
                    post_id,
                    str_arg(args, "page_id"),
                    str_arg(args, "metric"),
                )
                .await,
            )
        }
        "fb_ad_accounts" => json_result(&api::ad_accounts_value(s).await),
        "fb_ad_select_account" => {
            let Some(id) = str_arg(args, "account_id") else {
                return error_result("thiếu 'account_id'".into());
            };
            let full = if id.starts_with("act_") {
                id.to_string()
            } else {
                format!("act_{id}")
            };
            let _ = s.db.set_setting("active_ad_account", &full);
            json_result(&json!({ "ok": true, "active_ad_account": full }))
        }
        "fb_ad_campaigns" => {
            json_result(&api::ad_campaigns_value(s, str_arg(args, "account_id")).await)
        }
        "fb_ads_insights" => json_result(
            &api::ads_insights_value(
                s,
                str_arg(args, "object_id"),
                str_arg(args, "level"),
                str_arg(args, "date_preset"),
            )
            .await,
        ),
        "fb_ads_analyze" => json_result(
            &api::ads_analyze_value(
                s,
                str_arg(args, "object_id"),
                str_arg(args, "level"),
                str_arg(args, "date_preset"),
                str_arg(args, "currency"),
            )
            .await,
        ),
        "fb_ad_status" => {
            let (Some(entity_id), Some(status)) =
                (str_arg(args, "entity_id"), str_arg(args, "status"))
            else {
                return error_result("cần 'entity_id' và 'status'".into());
            };
            json_result(&api::ad_status_value(s, entity_id, status).await)
        }
        "fb_drafts" => json_result(&json!({ "pending": s.db.list_drafts("pending") })),
        "fb_approve" => {
            let Some(id) = args.get("draft_id").and_then(|x| x.as_i64()) else {
                return error_result("thiếu 'draft_id'".into());
            };
            json_result(&api::send_draft(s, id).await)
        }
        "fb_reject" => {
            let Some(id) = args.get("draft_id").and_then(|x| x.as_i64()) else {
                return error_result("thiếu 'draft_id'".into());
            };
            let _ = s.db.decide_draft(id, "rejected", "", "");
            json_result(&json!({ "ok": true, "status": "rejected" }))
        }
        "fb_triggers" => json_result(&json!({ "triggers": s.db.list_triggers(None) })),
        "fb_trigger_create" => {
            let Some(tname) = str_arg(args, "name") else {
                return error_result("thiếu 'name'".into());
            };
            let t = crate::db::TriggerInput {
                name: tname.into(),
                page_id: str_arg(args, "page_id").unwrap_or("").into(),
                event: "new_comment".into(),
                match_type: api::normalize_match_type(str_arg(args, "match_type")),
                match_value: str_arg(args, "match_value").unwrap_or("").into(),
                action: if str_arg(args, "action") == Some("notify") {
                    "notify".into()
                } else {
                    "draft_reply".into()
                },
                reply_hint: str_arg(args, "reply_hint").unwrap_or("").into(),
                enabled: true,
            };
            match s.db.add_trigger(&t) {
                Ok(id) => json_result(&json!({ "ok": true, "id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }
        "fb_trigger_delete" => {
            let Some(id) = args.get("id").and_then(|x| x.as_i64()) else {
                return error_result("thiếu 'id'".into());
            };
            let _ = s.db.delete_trigger(id);
            json_result(&json!({ "ok": true }))
        }
        "fb_autonomy_set" => {
            let mode = match str_arg(args, "mode") {
                Some("observe") => "observe",
                Some("live") => "live",
                Some("draft") => "draft",
                _ => return error_result("mode phải là observe|draft|live".into()),
            };
            let _ = s.db.set_setting("autonomy", mode);
            json_result(&json!({ "ok": true, "autonomy": mode }))
        }
        "fb_tick" => json_result(&crate::engine::tick(s).await),
        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_list_has_expected_tools() {
        let tools = tools_list();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|t| t["name"].as_str())
            .collect();
        for expected in [
            "fb_status",
            "fb_connect_link",
            "fb_connect_token",
            "fb_pages",
            "fb_select_page",
            "fb_post_create",
            "fb_post_edit",
            "fb_post_delete",
            "fb_posts",
            "fb_post_get",
            "fb_comments",
            "fb_comment_create",
            "fb_comment_reply",
            "fb_like",
            "fb_overview",
            "fb_conversations",
            "fb_conversation_messages",
            "fb_message_reply",
            "fb_analyze",
            "fb_page_insights",
            "fb_post_insights",
            "fb_ad_accounts",
            "fb_ad_select_account",
            "fb_ad_campaigns",
            "fb_ads_insights",
            "fb_ads_analyze",
            "fb_ad_status",
            "fb_drafts",
            "fb_approve",
            "fb_reject",
            "fb_triggers",
            "fb_trigger_create",
            "fb_trigger_delete",
            "fb_autonomy_set",
            "fb_tick",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
        // Every tool must declare an inputSchema object.
        for t in tools.as_array().unwrap() {
            assert_eq!(
                t["inputSchema"]["type"], "object",
                "tool {} bad schema",
                t["name"]
            );
        }
    }
}
