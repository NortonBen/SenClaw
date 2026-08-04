//! MCP server (HTTP + SSE) exposing the downloader to SenClaw agents. Tool
//! prefix `tdl_` (registered as `tiktok-dl-mcp` → full names
//! `mcp__tiktok-dl-mcp__tdl_*`); every tool calls the SAME `crate::api::*_value`
//! helpers the REST UI uses, so agents and humans see identical behavior.
//! Side effects stay on this machine: files land in the configured download
//! folder — nothing is ever uploaded or posted anywhere.

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
/// Loopback base URL for this app's own HTTP surface. The daemon, the chat UI
/// and this app all run on the same machine, and `PORT` is what `main` binds.
fn base_url() -> &'static str {
    static BASE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    BASE.get_or_init(|| {
        let port = std::env::var("PORT").unwrap_or_else(|_| "4670".into());
        format!("http://127.0.0.1:{port}")
    })
}

/// Hang playable URLs off every download row (any object carrying both an `id`
/// and a non-empty `files` array). Rows otherwise expose filesystem paths only,
/// and a path can't be played — the chat's `video` widget needs an http(s) URL,
/// so without this an agent asked to "mở video" has nothing to hand it.
fn add_media_urls(v: &mut Value) {
    match v {
        Value::Array(items) => items.iter_mut().for_each(add_media_urls),
        Value::Object(map) => {
            let id = map.get("id").and_then(|x| x.as_i64());
            let n = map
                .get("files")
                .and_then(|f| f.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            if let (Some(id), true) = (id, n > 0) {
                let base = base_url();
                let urls: Vec<Value> = (0..n)
                    .map(|i| json!(format!("{base}/api/downloads/{id}/file?i={i}")))
                    .collect();
                map.insert("file_urls".into(), Value::Array(urls));
                map.insert(
                    "thumb_url".into(),
                    json!(format!("{base}/api/downloads/{id}/thumb")),
                );
            }
            map.values_mut().for_each(add_media_urls);
        }
        _ => {}
    }
}

fn json_result(v: &Value) -> Value {
    let mut enriched = v.clone();
    add_media_urls(&mut enriched);
    text_result(serde_json::to_string_pretty(&enriched).unwrap_or_default())
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
            "serverInfo": { "name": "tiktok-dl-mcp", "version": "1.0.0" }
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
        {
            "name": "tdl_status",
            "description": "Trạng thái app TikTok Downloader: số job đang tải / đang chờ / đã xong / lỗi, tổng dung lượng đã tải, thư mục lưu và cài đặt chính.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "tdl_resolve",
            "description": "PHÂN TÍCH một link TikTok (không tải): trả về caption, tác giả, thời lượng, lượt xem/tim/bình luận, link video các bản (không logo / HD / có logo), nhạc nền, và danh sách ảnh nếu là post ảnh. Nhận cả link rút gọn vm.tiktok.com / vt.tiktok.com lẫn link dán kèm chữ. Dùng khi người dùng muốn xem thông tin video trước khi tải.",
            "inputSchema": { "type": "object", "properties": {
                "url": { "type": "string", "description": "Link TikTok (hoặc đoạn text có chứa link)." }
            }, "required": ["url"] }
        },
        {
            "name": "tdl_download",
            "description": "TẢI một video/post TikTok về máy (xếp vào hàng đợi, chạy nền). quality: nowm = không logo (mặc định) | hd = HD không logo | wm = bản gốc có logo | audio = chỉ tách nhạc MP3. Post ảnh tự động tải trọn bộ ảnh (+ nhạc nền theo cài đặt). Link đã tải xong trước đó sẽ bị bỏ qua trừ khi force=true. Trả về bản ghi kèm id — dùng tdl_queue/tdl_history_get để theo dõi tiến trình.",
            "inputSchema": { "type": "object", "properties": {
                "url":     { "type": "string", "description": "Link TikTok." },
                "quality": { "type": "string", "enum": ["nowm","hd","wm","audio"], "description": "Bỏ trống = dùng chất lượng mặc định trong cài đặt." },
                "force":   { "type": "boolean", "description": "true = tải lại dù link này đã tải xong trước đó." }
            }, "required": ["url"] }
        },
        {
            "name": "tdl_download_batch",
            "description": "TẢI HÀNG LOẠT: nhận một đoạn text chứa nhiều link TikTok (mỗi dòng một link, hoặc lẫn trong chữ đều được), lọc link hợp lệ, bỏ link đã tải xong, xếp tất cả vào hàng đợi. Tối đa 200 link một lần. Trả về số link đã xếp / bị bỏ qua.",
            "inputSchema": { "type": "object", "properties": {
                "text":    { "type": "string", "description": "Đoạn text chứa các link TikTok." },
                "quality": { "type": "string", "enum": ["nowm","hd","wm","audio"] },
                "force":   { "type": "boolean" }
            }, "required": ["text"] }
        },
        {
            "name": "tdl_profile_feed",
            "description": "Liệt kê video MỚI NHẤT của một tài khoản TikTok (không tải): video_id, link, caption, thời lượng, lượt xem. Phân trang bằng cursor. LƯU Ý: nguồn dữ liệu profile hay bị Cloudflare chặn hơn link lẻ — khi lỗi hãy khuyên người dùng dán link video và dùng tdl_download_batch.",
            "inputSchema": { "type": "object", "properties": {
                "unique_id": { "type": "string", "description": "Tên tài khoản, có hoặc không có @ (ví dụ '@tiktok')." },
                "count":     { "type": "number", "description": "Số video mỗi trang, tối đa 34." },
                "cursor":    { "type": "string", "description": "Cursor trang kế (lấy từ lần gọi trước)." }
            }, "required": ["unique_id"] }
        },
        {
            "name": "tdl_profile_download",
            "description": "TẢI CẢ TRANG CÁ NHÂN: lấy tối đa `max` video mới nhất của một tài khoản rồi xếp tất cả vào hàng đợi tải. Trả về số video tìm thấy / đã xếp hàng. Cùng lưu ý Cloudflare như tdl_profile_feed.",
            "inputSchema": { "type": "object", "properties": {
                "unique_id": { "type": "string", "description": "Tên tài khoản, ví dụ '@tiktok'." },
                "max":       { "type": "number", "description": "Số video tối đa (mặc định theo cài đặt profile_max, trần 200)." },
                "quality":   { "type": "string", "enum": ["nowm","hd","wm","audio"] }
            }, "required": ["unique_id"] }
        },
        {
            "name": "tdl_avatar",
            "description": "Tải ẢNH ĐẠI DIỆN của tác giả một post TikTok bất kỳ (đưa link post của người đó; không có API profile trực tiếp).",
            "inputSchema": { "type": "object", "properties": {
                "url": { "type": "string", "description": "Link một post TikTok của tác giả cần lấy avatar." }
            }, "required": ["url"] }
        },
        {
            "name": "tdl_queue",
            "description": "Hàng đợi hiện tại: các job đang tải (kèm % tiến trình bytes) và đang chờ. Dùng để trả lời 'tải xong chưa'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "tdl_cancel",
            "description": "Hủy một job đang chờ hoặc đang tải. File tải dở bị xoá; bản ghi chuyển trạng thái canceled (có thể tdl_retry sau).",
            "inputSchema": { "type": "object", "properties": {
                "download_id": { "type": "number" }
            }, "required": ["download_id"] }
        },
        {
            "name": "tdl_retry",
            "description": "Tải lại một job lỗi/đã hủy/đã xong (xếp lại vào hàng đợi; link được phân giải mới nên vẫn chạy dù link CDN cũ hết hạn).",
            "inputSchema": { "type": "object", "properties": {
                "download_id": { "type": "number" }
            }, "required": ["download_id"] }
        },
        {
            "name": "tdl_history",
            "description": "LỊCH SỬ tải: tìm kiếm theo caption/tác giả/link (FTS — gõ không dấu vẫn khớp, 'am thuc' tìm được 'ẩm thực'), lọc theo status (active|queued|downloading|done|error|canceled) và kind (video|images|audio|avatar), phân trang limit/offset. Trả kèm bộ đếm tổng.",
            "inputSchema": { "type": "object", "properties": {
                "q":      { "type": "string", "description": "Từ khóa tìm kiếm." },
                "status": { "type": "string", "enum": ["active","queued","downloading","done","error","canceled"] },
                "kind":   { "type": "string", "enum": ["video","images","audio","avatar"] },
                "limit":  { "type": "number", "description": "Mặc định 50, tối đa 500." },
                "offset": { "type": "number" }
            } }
        },
        {
            "name": "tdl_history_get",
            "description": "Chi tiết một bản ghi tải: trạng thái, tiến trình bytes, danh sách file đã lưu trên đĩa, thống kê video (view/tim/bình luận), lỗi nếu có.",
            "inputSchema": { "type": "object", "properties": {
                "download_id": { "type": "number" }
            }, "required": ["download_id"] }
        },
        {
            "name": "tdl_history_delete",
            "description": "Xoá MỘT bản ghi lịch sử. with_file=true xoá luôn file trên đĩa (mặc định chỉ xoá bản ghi, file giữ nguyên). Job đang chạy phải hủy trước.",
            "inputSchema": { "type": "object", "properties": {
                "download_id": { "type": "number" },
                "with_file":   { "type": "boolean" }
            }, "required": ["download_id"] }
        },
        {
            "name": "tdl_history_clear",
            "description": "Dọn lịch sử hàng loạt: xoá mọi bản ghi đã kết thúc (done/error/canceled), hoặc chỉ một trạng thái. with_files=true xoá cả file trên đĩa — hành động không hoàn tác, chỉ dùng khi người dùng nói rõ. Job đang chạy không bị đụng tới.",
            "inputSchema": { "type": "object", "properties": {
                "status":     { "type": "string", "enum": ["done","error","canceled"], "description": "Bỏ trống = cả ba trạng thái đã kết thúc." },
                "with_files": { "type": "boolean" }
            } }
        },
        {
            "name": "tdl_open",
            "description": "Mở thư mục chứa file đã tải trong Finder/File manager trên máy đang chạy SenClaw (reveal=true thì chọn thẳng file).",
            "inputSchema": { "type": "object", "properties": {
                "download_id": { "type": "number" },
                "reveal":      { "type": "boolean" }
            }, "required": ["download_id"] }
        },
        {
            "name": "tdl_settings_get",
            "description": "Đọc cài đặt hiện tại: thư mục lưu (download_dir), chất lượng mặc định (default_quality), mẫu tên file (filename_template với {author} {id} {title} {date} {quality}), số tải đồng thời (max_concurrent 1-4), tải nhạc kèm post ảnh (photo_audio), ghi metadata JSON (save_meta_json), trần video khi tải profile (profile_max).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "tdl_settings_set",
            "description": "Đổi cài đặt (patch — chỉ key truyền vào mới đổi). Ví dụ {\"settings\": {\"default_quality\": \"hd\", \"max_concurrent\": 3}}. Key hợp lệ: download_dir, default_quality (nowm|hd|wm|audio), filename_template, max_concurrent (1-4), photo_audio (0/1), save_meta_json (0/1), profile_max.",
            "inputSchema": { "type": "object", "properties": {
                "settings": { "type": "object", "description": "Object các cài đặt cần đổi." }
            }, "required": ["settings"] }
        },
        {
            "name": "tdl_activity",
            "description": "Nhật ký hoạt động gần đây của app (xếp hàng, tải xong, lỗi, đổi cài đặt).",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

async fn call_tool(s: &AppState, name: &str, args: &Value) -> Value {
    let i64_arg = |k: &str| args.get(k).and_then(|x| x.as_i64());
    let str_arg = |k: &str| {
        args.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };
    let bool_arg = |k: &str| args.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    match name {
        "tdl_status" => json_result(&api::status_value(s)),
        "tdl_resolve" => {
            let url = str_arg("url");
            if url.is_empty() {
                return error_result("thiếu 'url'".into());
            }
            json_result(&api::resolve_value(s, &url).await)
        }
        "tdl_download" => {
            let url = str_arg("url");
            if url.is_empty() {
                return error_result("thiếu 'url'".into());
            }
            json_result(&api::download_value(
                s,
                &url,
                &str_arg("quality"),
                bool_arg("force"),
                None,
            ))
        }
        "tdl_download_batch" => {
            let text = str_arg("text");
            if text.is_empty() {
                return error_result("thiếu 'text'".into());
            }
            json_result(&api::batch_value(s, &text, &str_arg("quality"), bool_arg("force")))
        }
        "tdl_profile_feed" => {
            let uid = str_arg("unique_id");
            if uid.is_empty() {
                return error_result("thiếu 'unique_id'".into());
            }
            json_result(
                &api::profile_feed_value(s, &uid, i64_arg("count").unwrap_or(30), &str_arg("cursor"))
                    .await,
            )
        }
        "tdl_profile_download" => {
            let uid = str_arg("unique_id");
            if uid.is_empty() {
                return error_result("thiếu 'unique_id'".into());
            }
            json_result(
                &api::profile_download_value(
                    s,
                    &uid,
                    i64_arg("max").unwrap_or(0),
                    &str_arg("quality"),
                )
                .await,
            )
        }
        "tdl_avatar" => {
            let url = str_arg("url");
            if url.is_empty() {
                return error_result("thiếu 'url'".into());
            }
            json_result(&api::avatar_value(s, &url))
        }
        "tdl_queue" => {
            let q = api::ListQuery {
                status: Some("active".into()),
                limit: Some(100),
                ..Default::default()
            };
            let mut v = api::list_value(s, &q);
            let queued = s.db.list_downloads(None, Some("queued"), None, 100, 0);
            v["queued_jobs"] = serde_json::json!(queued);
            json_result(&v)
        }
        "tdl_cancel" => {
            let Some(id) = i64_arg("download_id") else {
                return error_result("thiếu 'download_id'".into());
            };
            json_result(&api::cancel_value(s, id))
        }
        "tdl_retry" => {
            let Some(id) = i64_arg("download_id") else {
                return error_result("thiếu 'download_id'".into());
            };
            json_result(&api::retry_value(s, id))
        }
        "tdl_history" => {
            let q = api::ListQuery {
                q: args.get("q").and_then(|x| x.as_str()).map(str::to_string),
                status: args
                    .get("status")
                    .and_then(|x| x.as_str())
                    .map(str::to_string),
                kind: args.get("kind").and_then(|x| x.as_str()).map(str::to_string),
                limit: i64_arg("limit"),
                offset: i64_arg("offset"),
            };
            json_result(&api::list_value(s, &q))
        }
        "tdl_history_get" => {
            let Some(id) = i64_arg("download_id") else {
                return error_result("thiếu 'download_id'".into());
            };
            json_result(&api::get_value(s, id))
        }
        "tdl_history_delete" => {
            let Some(id) = i64_arg("download_id") else {
                return error_result("thiếu 'download_id'".into());
            };
            json_result(&api::delete_value(s, id, bool_arg("with_file")))
        }
        "tdl_history_clear" => {
            let st = args
                .get("status")
                .and_then(|x| x.as_str())
                .map(str::to_string);
            json_result(&api::clear_value(s, st.as_deref(), bool_arg("with_files")))
        }
        "tdl_open" => {
            let Some(id) = i64_arg("download_id") else {
                return error_result("thiếu 'download_id'".into());
            };
            json_result(&api::open_value(s, id, bool_arg("reveal")))
        }
        "tdl_settings_get" => json_result(&api::settings_value(s)),
        "tdl_settings_set" => {
            let patch = args.get("settings").cloned().unwrap_or(Value::Null);
            if !patch.is_object() {
                return error_result("thiếu 'settings' (object)".into());
            }
            json_result(&api::set_settings_value(s, &patch))
        }
        "tdl_activity" => json_result(&json!({ "activity": s.db.recent_activity(50) })),
        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_urls_added_to_download_rows_at_any_depth() {
        let base = base_url();
        let mut v = json!({
            "downloads": [
                { "id": 7, "files": ["/tmp/a.mp4", "/tmp/a.mp3"], "status": "done" },
                { "id": 8, "files": [], "status": "queued" },
            ],
            "settings": { "download_dir": "/tmp" }
        });
        add_media_urls(&mut v);

        let done = &v["downloads"][0];
        assert_eq!(done["file_urls"][0], json!(format!("{base}/api/downloads/7/file?i=0")));
        assert_eq!(done["file_urls"][1], json!(format!("{base}/api/downloads/7/file?i=1")));
        assert_eq!(done["thumb_url"], json!(format!("{base}/api/downloads/7/thumb")));
        // No files yet → nothing to link.
        assert!(v["downloads"][1].get("file_urls").is_none());
        // Unrelated objects are untouched.
        assert!(v["settings"].get("file_urls").is_none());
    }

    #[test]
    fn media_urls_are_absolute_http_so_the_video_widget_accepts_them() {
        let mut v = json!({ "id": 1, "files": ["/tmp/clip.mp4"] });
        add_media_urls(&mut v);
        let url = v["file_urls"][0].as_str().unwrap();
        assert!(url.starts_with("http://"), "{url}");
    }

    #[test]
    fn tools_have_unique_prefixed_names() {
        let tools = tools_list();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 18);
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "tool names must be unique");
        assert!(
            names.iter().all(|n| n.starts_with("tdl_")),
            "all tools use the tdl_ prefix"
        );
    }

    #[test]
    fn every_tool_has_schema_and_description() {
        for t in tools_list().as_array().unwrap() {
            assert!(
                t["description"].as_str().unwrap().len() > 20,
                "{} needs a real description",
                t["name"]
            );
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn required_fields_declared_for_detail_tools() {
        let tools = tools_list();
        for (tool, field) in [
            ("tdl_resolve", "url"),
            ("tdl_download", "url"),
            ("tdl_download_batch", "text"),
            ("tdl_profile_feed", "unique_id"),
            ("tdl_profile_download", "unique_id"),
            ("tdl_avatar", "url"),
            ("tdl_cancel", "download_id"),
            ("tdl_retry", "download_id"),
            ("tdl_history_get", "download_id"),
            ("tdl_history_delete", "download_id"),
            ("tdl_open", "download_id"),
            ("tdl_settings_set", "settings"),
        ] {
            let t = tools
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == tool)
                .unwrap();
            let req = t["inputSchema"]["required"].as_array().unwrap();
            assert!(req.iter().any(|r| r == field), "{tool} must require {field}");
        }
    }
}
