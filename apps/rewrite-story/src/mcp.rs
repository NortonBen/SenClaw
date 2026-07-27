//! MCP server — hand-rolled JSON-RPC over HTTP + SSE, matching the other Space
//! Apps (the `rmcp` crate is not used here).
//!
//! Tools are prefixed `rs_`. Agents reach them as `mcp__rewrite-story-mcp__rs_*`.

use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::Json;
use futures_util::Stream;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::db::{status, NewProcess};
use crate::state::AppState;
use crate::text;

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
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message"));
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
    // The result goes back in the HTTP response only. Mirroring it onto the SSE
    // fan-out — as the sibling apps do — sends every caller's payload to every
    // other connected client, so one agent's paginated story page lands in
    // another's stream, and a large result evicts frames for lagging readers.
    let reply = |result: Value| -> Json<Value> {
        Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": result }))
    };
    match req.method.as_str() {
        "initialize" => reply(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "rewrite-story-mcp", "version": "1.0.0" }
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

// ---- arg helpers ----

fn s(args: &Value, k: &str) -> String {
    args[k].as_str().unwrap_or("").trim().to_string()
}

fn opt_s(args: &Value, k: &str) -> Option<String> {
    let v = s(args, k);
    (!v.is_empty()).then_some(v)
}

fn int(args: &Value, k: &str, d: i64) -> i64 {
    args[k].as_i64().unwrap_or(d)
}

fn opt_int(args: &Value, k: &str) -> Option<i64> {
    args[k].as_i64()
}

fn tools_list() -> Value {
    json!([
        {
            "name": "rs_status",
            "description": "Tổng quan kho truyện và hàng chờ viết lại: số truyện, số tiến trình đang chờ / đang chạy / đã xong, và cấu hình chia chunk hiện tại. Gọi tool này TRƯỚC TIÊN khi người dùng nhắc tới viết lại truyện, để biết có việc đang chạy không. Dùng cho 'tình hình viết lại truyện', 'có truyện nào đang chạy không', 'rewrite status'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "rs_story_list",
            "description": "Liệt kê các truyện gốc đã nhập, mới nhất trước. Mỗi dòng gồm id, tên, độ dài (ký tự), số bản viết lại đã có và một đoạn xem trước. KHÔNG trả về toàn văn — dùng rs_story_get để đọc nội dung. Dùng cho 'danh sách truyện', 'kho truyện của tôi', 'list stories'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "rs_story_import",
            "description": "Nhập một truyện mới vào kho từ văn bản thô. Trả về id truyện — dùng id đó cho rs_rewrite_start. Truyện dài hàng triệu ký tự đều nhập được; việc chia chunk diễn ra tự động khi bắt đầu viết lại. Dùng cho 'thêm truyện', 'nhập truyện', 'import story'.",
            "inputSchema": { "type": "object", "properties": {
                "name": { "type": "string", "description": "Tên truyện. Bỏ trống sẽ đặt 'Truyện chưa đặt tên'." },
                "text": { "type": "string", "description": "Toàn văn truyện gốc." }
            }, "required": ["text"] }
        },
        {
            "name": "rs_story_get",
            "description": "Đọc nội dung một truyện (bản gốc hoặc bản đã viết lại) theo từng khoảng ký tự. LUÔN dùng offset/limit — truyện có thể dài hàng triệu ký tự và trả hết sẽ tràn ngữ cảnh. Trả về đoạn văn bản cùng tổng độ dài để biết còn bao nhiêu.",
            "inputSchema": { "type": "object", "properties": {
                "story_id": { "type": "integer", "description": "Id truyện." },
                "offset":   { "type": "integer", "description": "Vị trí ký tự bắt đầu (mặc định 0)." },
                "limit":    { "type": "integer", "description": "Số ký tự cần đọc (mặc định 4000, tối đa 20000)." }
            }, "required": ["story_id"] }
        },
        {
            "name": "rs_story_versions",
            "description": "Liệt kê các bản viết lại (version) của một truyện gốc, cũ nhất trước. Mỗi bản là một truyện độc lập có id riêng — đọc bằng rs_story_get. Dùng cho 'các bản viết lại', 'phiên bản của truyện này', 'story versions'.",
            "inputSchema": { "type": "object", "properties": {
                "story_id": { "type": "integer", "description": "Id truyện gốc." }
            }, "required": ["story_id"] }
        },
        {
            "name": "rs_story_chunks",
            "description": "Xem truyện sẽ được cắt thành bao nhiêu chunk và mỗi chunk dài bao nhiêu, TRƯỚC khi chạy viết lại. Hữu ích để ước lượng thời gian/chi phí và để chỉnh hybrid_split_max_size nếu chunk quá dài. Nếu truyện chưa từng được cắt, tool chỉ xem thử chứ không lưu.",
            "inputSchema": { "type": "object", "properties": {
                "story_id": { "type": "integer", "description": "Id truyện." }
            }, "required": ["story_id"] }
        },
        {
            "name": "rs_story_export",
            "description": "Xuất một truyện thành KỊCH BẢN (screenplay markdown) để chuyển sang app làm video. Đây là cầu nối chính sang Video Flow: chuỗi markdown trả về nạp thẳng được vào mcp__video-flow-mcp__vf_pipeline_create với mode='production', hoặc POST /api/script/parse của Video Flow. Truyện được cắt thành các cảnh (mỗi cảnh ~1 khung hình 8 giây) bằng bộ chia hiểu tiếng Việt, mỗi cảnh là một heading '# Cảnh N'. Truyện dài thì LẤY THEO KHOẢNG bằng from_scene/to_scene — đừng kéo cả nghìn cảnh vào ngữ cảnh. Tool cũng ghi bản đầy đủ ra file trên đĩa và trả về đường dẫn.",
            "inputSchema": { "type": "object", "properties": {
                "story_id":    { "type": "integer", "description": "Id truyện cần xuất (thường là id bản ĐÃ VIẾT LẠI — xem rs_rewrite_status.result_story_id)." },
                "scene_chars": { "type": "integer", "description": "Số ký tự mỗi cảnh, mặc định 900 (~8 giây video). Nhỏ hơn = nhiều cảnh ngắn hơn." },
                "from_scene":  { "type": "integer", "description": "Cảnh bắt đầu (1-based, mặc định 1)." },
                "to_scene":    { "type": "integer", "description": "Cảnh kết thúc (mặc định: hết, nhưng tự giới hạn ~40 cảnh mỗi lần gọi)." },
                "write_file":  { "type": "boolean", "description": "Ghi bản kịch bản ĐẦY ĐỦ ra file (mặc định true) và trả về đường dẫn." }
            }, "required": ["story_id"] }
        },
        {
            "name": "rs_story_delete",
            "description": "Xoá một truyện cùng toàn bộ bản viết lại, chunk và tiến trình của nó. Không thể hoàn tác — hãy xác nhận với người dùng trước khi gọi.",
            "inputSchema": { "type": "object", "properties": {
                "story_id": { "type": "integer", "description": "Id truyện cần xoá." }
            }, "required": ["story_id"] }
        },
        {
            "name": "rs_rewrite_start",
            "description": "Đưa một truyện vào hàng chờ viết lại và trả về NGAY (không chờ chạy xong — truyện dài mất hàng chục phút). Trả về process_id; theo dõi bằng rs_rewrite_status. Việc viết lại chạy nền theo từng chunk và lưu lại từng chunk, nên nếu hỏng có thể rs_rewrite_retry để chạy tiếp từ chỗ dở. Dùng cho 'viết lại truyện', 'rewrite truyện này', 'làm bản mới của truyện'.",
            "inputSchema": { "type": "object", "properties": {
                "story_id":     { "type": "integer", "description": "Id truyện gốc cần viết lại." },
                "version_plan": { "type": "string",  "description": "Phong cách / kế hoạch cho bản mới, ví dụ 'giọng cổ trang, tiết tấu nhanh hơn'. Đây là chỉ dẫn quan trọng nhất." },
                "user_prompt":  { "type": "string",  "description": "Yêu cầu thêm, ví dụ 'giữ nguyên tên nhân vật, bỏ cảnh bạo lực'." },
                "system_instruction": { "type": "string", "description": "Ghi đè vai trò hệ thống. Bỏ trống dùng mặc định (biên tập viên viết lại truyện)." },
                "creativity_ratio": { "type": "integer", "description": "0-100. Càng cao càng sáng tạo/xa bản gốc. Mặc định 40." },
                "target_length_variance": { "type": "integer", "description": "Dung sai độ dài theo %, mặc định 5." },
                "model": { "type": "string", "description": "Bỏ trống để dùng model đang bật của SenClaw." }
            }, "required": ["story_id"] }
        },
        {
            "name": "rs_rewrite_status",
            "description": "Xem tiến độ một tiến trình viết lại: trạng thái (queued/processing/completed/failed/cancelled), phần trăm, đang ở chunk mấy trên mấy, lỗi nếu có, và id truyện kết quả khi xong. Đây là tool để poll sau khi gọi rs_rewrite_start — đừng chờ đồng bộ.",
            "inputSchema": { "type": "object", "properties": {
                "process_id": { "type": "integer", "description": "Id tiến trình." }
            }, "required": ["process_id"] }
        },
        {
            "name": "rs_rewrite_list",
            "description": "Liệt kê các tiến trình viết lại, mới nhất trước, lọc theo trạng thái nếu cần. Dùng để tìm process_id khi người dùng nói 'cái đang chạy' mà không nhớ id.",
            "inputSchema": { "type": "object", "properties": {
                "status": { "type": "string", "description": "Lọc: queued | processing | completed | failed | cancelled." }
            } }
        },
        {
            "name": "rs_rewrite_cancel",
            "description": "Dừng một tiến trình đang chờ hoặc đang chạy. Các chunk đã viết xong vẫn được giữ, nên sau này có thể rs_rewrite_retry để chạy tiếp thay vì làm lại từ đầu.",
            "inputSchema": { "type": "object", "properties": {
                "process_id": { "type": "integer", "description": "Id tiến trình cần dừng." }
            }, "required": ["process_id"] }
        },
        {
            "name": "rs_rewrite_retry",
            "description": "Chạy lại một tiến trình đã thất bại hoặc bị hủy. Đây là CHẠY TIẾP, không phải làm lại: các chunk đã xong được giữ nguyên và tiến trình bắt đầu từ chunk dở dang đầu tiên. Trả về số chunk sẽ được bỏ qua.",
            "inputSchema": { "type": "object", "properties": {
                "process_id": { "type": "integer", "description": "Id tiến trình cần chạy tiếp." }
            }, "required": ["process_id"] }
        },
        {
            "name": "rs_settings_get",
            "description": "Đọc cấu hình hiện tại: kích thước chunk (hybrid_split_min_size / max_size / threshold), mức sáng tạo mặc định, số tiến trình chạy song song, profile LLM.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "rs_settings_set",
            "description": "Chỉnh cấu hình. Hay dùng nhất là hybrid_split_max_size khi chunk quá dài làm model cắt output giữa chừng. Lưu ý: đổi kích thước chunk KHÔNG cắt lại các truyện đã cắt trước đó.",
            "inputSchema": { "type": "object", "properties": {
                "hybrid_split_min_size":  { "type": "integer", "description": "Ngưỡng tối thiểu (ký tự) trước khi cho phép ngắt theo ngữ nghĩa." },
                "hybrid_split_max_size":  { "type": "integer", "description": "Trần cứng mỗi chunk (ký tự)." },
                "hybrid_split_threshold": { "type": "number",  "description": "0-1. Dưới ngưỡng tương đồng này thì coi là chuyển cảnh và ngắt chunk." },
                "default_creativity_ratio": { "type": "integer", "description": "0-100." },
                "default_length_variance":  { "type": "integer", "description": "Dung sai độ dài %, mặc định 5." },
                "max_concurrent_processes": { "type": "integer", "description": "Số TRUYỆN chạy song song." },
                "parallel_chunks": { "type": "integer", "description": "1-8. Số chunk viết song song TRONG MỘT truyện. 1 = tuần tự, mỗi chunk nối tiếp đuôi bản đã viết lại của chunk trước (chất lượng mạch văn cao nhất). Lớn hơn 1 thì các chunk cùng lô dùng đuôi bản GỐC làm cầu nối — nhanh gần tuyến tính nhưng mối nối hơi kém mượt. Truyện rất dài nên đặt 3-4." },
                "llm_profile": { "type": "string", "description": "Profile LLM của SenClaw. Bỏ trống = theo model đang bật." }
            } }
        }
    ])
}

async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    let db = &state.core.db;

    match name {
        "rs_status" => {
            let stories = db.list_stories().map(|s| s.len()).unwrap_or(0);
            json_result(json!({
                "stories": stories,
                "queued": db.count_by_status(status::QUEUED).unwrap_or(0),
                "processing": db.count_by_status(status::PROCESSING).unwrap_or(0),
                "completed": db.count_by_status(status::COMPLETED).unwrap_or(0),
                "failed": db.count_by_status(status::FAILED).unwrap_or(0),
                "running_here": state.core.running_count(),
                "chunk_settings": {
                    "min_size": db.setting_i64("hybrid_split_min_size", (crate::llm::MAX_CHUNK_CHARS as i64) * 3 / 5),
                    "max_size": db.setting_i64("hybrid_split_max_size", crate::llm::MAX_CHUNK_CHARS as i64),
                    "threshold": db.setting_f64("hybrid_split_threshold", 0.2),
                },
                "next": "Dùng rs_story_list để xem kho truyện, rs_rewrite_start để bắt đầu viết lại."
            }))
        }

        "rs_story_list" => match db.list_stories() {
            Ok(rows) => json_result(json!({ "total": rows.len(), "stories": rows })),
            Err(e) => error_result(e.to_string()),
        },

        "rs_story_import" => {
            let text_body = s(args, "text");
            if text_body.is_empty() {
                return error_result("text là bắt buộc".into());
            }
            let name = opt_s(args, "name").unwrap_or_else(|| "Truyện chưa đặt tên".to_string());
            match db.create_story(&name, &text_body) {
                Ok(id) => json_result(json!({
                    "ok": true,
                    "story_id": id,
                    "name": name,
                    "length": text_body.chars().count(),
                    "next": "Xem trước cách cắt chunk bằng rs_story_chunks, rồi rs_rewrite_start để viết lại."
                })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rs_story_get" => {
            let id = int(args, "story_id", 0);
            let Ok(Some(meta)) = db.story_meta(id) else {
                return error_result(format!("không tìm thấy truyện {id}"));
            };
            let offset = int(args, "offset", 0).max(0);
            let limit = int(args, "limit", 4000).clamp(1, 20_000);
            // Sliced in SQL. Reading the whole novel and then `.skip().take()`
            // made every 4 000-character page cost a full decode of the text.
            let Ok(Some((slice, total))) = db.story_slice(id, offset, limit) else {
                return error_result(format!("không đọc được truyện {id}"));
            };
            json_result(json!({
                "story_id": meta.id,
                "name": meta.name,
                "source_type": meta.source_type,
                "version_number": meta.version_number,
                "total_length": total,
                "offset": offset,
                "returned": slice.chars().count(),
                "has_more": offset + limit < total,
                "text": slice,
            }))
        }

        "rs_story_versions" => {
            let id = int(args, "story_id", 0);
            match db.list_versions(id) {
                Ok(rows) => json_result(json!({ "total": rows.len(), "versions": rows })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rs_story_chunks" => {
            let id = int(args, "story_id", 0);
            let stored = db.get_chunks(id).unwrap_or_default();
            let (chunks, persisted) = if stored.is_empty() {
                let Ok(Some(story)) = db.get_story(id) else {
                    return error_result(format!("không tìm thấy truyện {id}"));
                };
                let min = db.setting_i64("hybrid_split_min_size", (crate::llm::MAX_CHUNK_CHARS as i64) * 3 / 5).max(1) as usize;
                let max = db.setting_i64("hybrid_split_max_size", crate::llm::MAX_CHUNK_CHARS as i64).max(1) as usize;
                let (min, max) = if min > max { (max, min) } else { (min, max) };
                let th = db.setting_f64("hybrid_split_threshold", 0.2).clamp(0.0, 1.0);
                (text::hybrid_split(&story.original_text, min, max, th), false)
            } else {
                (stored, true)
            };
            let lengths: Vec<usize> = chunks.iter().map(|c| c.chars().count()).collect();
            json_result(json!({
                "persisted": persisted,
                "total_chunks": chunks.len(),
                "longest_chunk": lengths.iter().max().copied().unwrap_or(0),
                "chunk_lengths": lengths,
                "note": if persisted { "Đã cắt và lưu — đổi cấu hình sẽ không cắt lại." }
                        else { "Mới chỉ xem thử; chunk sẽ được lưu khi bắt đầu viết lại." }
            }))
        }

        "rs_story_export" => {
            let id = int(args, "story_id", 0);
            let Ok(Some(meta)) = db.story_meta(id) else {
                return error_result(format!("không tìm thấy truyện {id}"));
            };
            let Ok(Some(text_body)) = db.story_text(id) else {
                return error_result(format!("không đọc được nội dung truyện {id}"));
            };

            let scene_chars =
                int(args, "scene_chars", crate::export::DEFAULT_SCENE_CHARS as i64).max(1) as usize;
            let bundle = crate::export::bundle(
                meta.id,
                &meta.name,
                &meta.source_type,
                meta.version_number,
                &text_body,
                scene_chars,
            );
            if bundle.total_scenes == 0 {
                return error_result("truyện rỗng, không có cảnh nào để xuất".into());
            }

            // Write the complete screenplay to disk first, so a novel that is far
            // too big to return still leaves something the user (or another app)
            // can pick up.
            let mut file_path = String::new();
            if args["write_file"].as_bool().unwrap_or(true) {
                let dir = crate::config::data_dir().join("exports");
                std::fs::create_dir_all(&dir).ok();
                let path = dir.join(format!(
                    "{}-{}.md",
                    crate::export::slug(&meta.name),
                    meta.id
                ));
                match std::fs::write(&path, crate::export::to_screenplay(&bundle)) {
                    Ok(_) => file_path = path.to_string_lossy().to_string(),
                    Err(e) => return error_result(format!("không ghi được file kịch bản: {e}")),
                }
            }

            // Returning a whole novel would blow the caller's context, so the
            // inline copy is a window. The file above always holds everything.
            const MAX_SCENES_INLINE: usize = 40;
            let from = int(args, "from_scene", 1).max(1) as usize;
            let requested_to = int(args, "to_scene", 0);
            let to = if requested_to > 0 {
                (requested_to as usize).min(bundle.total_scenes)
            } else {
                bundle.total_scenes
            };
            let to = to.min(from + MAX_SCENES_INLINE - 1);
            if from > bundle.total_scenes {
                return error_result(format!(
                    "from_scene {from} vượt quá số cảnh ({})",
                    bundle.total_scenes
                ));
            }

            let window = crate::export::ExportBundle {
                scenes: bundle.scenes[from - 1..to].to_vec(),
                ..bundle.clone()
            };
            let screenplay = crate::export::to_screenplay(&window);

            json_result(json!({
                "story_id": meta.id,
                "name": meta.name,
                "total_scenes": bundle.total_scenes,
                "total_chars": bundle.total_chars,
                "scene_chars": scene_chars,
                "returned_scenes": [from, to],
                "has_more": to < bundle.total_scenes,
                "file": file_path,
                "screenplay": screenplay,
                "next": format!(
                    "Kịch bản này nạp thẳng vào Video Flow: mcp__video-flow-mcp__vf_project_create (name, story) → vf_video_create (orientation) → vf_pipeline_create (project_id, script=<screenplay>, mode='production'). {}",
                    if to < bundle.total_scenes {
                        format!("Còn cảnh {}..{} — gọi lại với from_scene, hoặc dùng file đầy đủ ở trên.", to + 1, bundle.total_scenes)
                    } else {
                        "Đã lấy hết cảnh.".to_string()
                    }
                ),
            }))
        }

        "rs_story_delete" => {
            let id = int(args, "story_id", 0);
            // The cascade would pull a running process out from under its worker.
            match db.active_processes_for_story(id) {
                Ok(active) if !active.is_empty() => {
                    return error_result(format!(
                        "Truyện đang có {} tiến trình chạy/chờ ({:?}). Dùng rs_rewrite_cancel trước khi xoá.",
                        active.len(),
                        active
                    ))
                }
                Ok(_) => {}
                Err(e) => return error_result(e.to_string()),
            }
            match db.delete_story(id) {
                Ok(0) => error_result(format!("không tìm thấy truyện {id}")),
                Ok(_) => json_result(json!({ "ok": true, "deleted_story_id": id })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rs_rewrite_start" => {
            let story_id = int(args, "story_id", 0);
            if !db.story_exists(story_id).unwrap_or(false) {
                return error_result(format!("không tìm thấy truyện {story_id}"));
            }
            let in_flight = db.count_by_status(status::QUEUED).unwrap_or(0)
                + db.count_by_status(status::PROCESSING).unwrap_or(0);
            if in_flight >= 10 {
                return error_result(
                    "Đang có quá nhiều tiến trình trong hàng chờ (tối đa 10). Dùng rs_rewrite_list để xem và rs_rewrite_cancel để dọn bớt.".into(),
                );
            }

            let p = NewProcess {
                story_id,
                creativity_ratio: opt_int(args, "creativity_ratio")
                    .unwrap_or_else(|| db.setting_i64("default_creativity_ratio", 40))
                    .clamp(0, 100),
                target_length_variance: opt_int(args, "target_length_variance")
                    .unwrap_or_else(|| db.setting_i64("default_length_variance", 5))
                    .clamp(0, 100),
                system_instruction: opt_s(args, "system_instruction"),
                user_prompt: opt_s(args, "user_prompt"),
                version_plan: opt_s(args, "version_plan"),
                model: opt_s(args, "model"),
            };
            match db.create_process(&p) {
                Ok(id) => json_result(json!({
                    "ok": true,
                    "process_id": id,
                    "status": "queued",
                    "next": "Tiến trình chạy nền. Poll bằng rs_rewrite_status; truyện dài có thể mất hàng chục phút. ĐỪNG chờ đồng bộ."
                })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rs_rewrite_status" => {
            let id = int(args, "process_id", 0);
            let Ok(Some(p)) = db.get_process(id) else {
                return error_result(format!("không tìm thấy tiến trình {id}"));
            };
            let done = db.get_rewrite_chunks(id).map(|c| c.len()).unwrap_or(0);
            let next = match p.status.as_str() {
                status::COMPLETED => format!(
                    "Xong. Đọc bản mới bằng rs_story_get với story_id={}.",
                    p.result_story_id.unwrap_or(0)
                ),
                status::FAILED | status::CANCELLED => {
                    format!("Dùng rs_rewrite_retry để chạy tiếp từ chunk {done}.")
                }
                _ => "Đang chạy — poll lại sau.".to_string(),
            };
            json_result(json!({ "process": p, "chunks_done": done, "next": next }))
        }

        "rs_rewrite_list" => {
            let filter = opt_s(args, "status");
            match db.list_processes(filter.as_deref()) {
                Ok(rows) => json_result(json!({ "total": rows.len(), "processes": rows })),
                Err(e) => error_result(e.to_string()),
            }
        }

        "rs_rewrite_cancel" => {
            let id = int(args, "process_id", 0);
            let Ok(Some(p)) = db.get_process(id) else {
                return error_result(format!("không tìm thấy tiến trình {id}"));
            };
            if !status::is_active(&p.status) {
                return error_result(format!(
                    "Tiến trình đang ở trạng thái '{}', không thể hủy.",
                    p.status
                ));
            }
            state.core.cancel_job(id);
            if let Err(e) = db.update_progress(
                id,
                status::CANCELLED,
                crate::db::stage::CANCELLED,
                p.progress_percentage,
                0,
                0,
                Some("Bị hủy bởi người dùng"),
                None,
            ) {
                return error_result(e.to_string());
            }
            if let Ok(Some(row)) = db.get_process(id) {
                state
                    .core
                    .dash
                    .emit(crate::dashws::event::PROCESS_CANCELLED, json!(row));
            }
            let done = db.get_rewrite_chunks(id).map(|c| c.len()).unwrap_or(0);
            json_result(json!({
                "ok": true,
                "process_id": id,
                "chunks_kept": done,
                "next": "Các chunk đã xong vẫn được giữ — rs_rewrite_retry sẽ chạy tiếp từ đó."
            }))
        }

        "rs_rewrite_retry" => {
            let id = int(args, "process_id", 0);
            let Ok(Some(p)) = db.get_process(id) else {
                return error_result(format!("không tìm thấy tiến trình {id}"));
            };
            if !matches!(p.status.as_str(), status::FAILED | status::CANCELLED) {
                return error_result(format!(
                    "Chỉ chạy tiếp được tiến trình failed/cancelled; tiến trình này đang '{}'.",
                    p.status
                ));
            }
            if let Err(e) = db.requeue_process(id) {
                return error_result(e.to_string());
            }
            let done = db.get_rewrite_chunks(id).map(|c| c.len()).unwrap_or(0);
            json_result(json!({
                "ok": true,
                "process_id": id,
                "status": "queued",
                "resuming_from_chunk": done,
                "next": "Poll bằng rs_rewrite_status."
            }))
        }

        "rs_settings_get" => match db.all_settings() {
            Ok(kv) => {
                let map: serde_json::Map<String, Value> =
                    kv.into_iter().map(|(k, v)| (k, json!(v))).collect();
                json_result(Value::Object(map))
            }
            Err(e) => error_result(e.to_string()),
        },

        "rs_settings_set" => {
            let Some(obj) = args.as_object() else {
                return error_result("cần ít nhất một cấu hình để đặt".into());
            };
            // Validate everything before writing anything, so a bad key can't
            // leave settings half-applied.
            let mut writes = Vec::new();
            for (k, v) in obj {
                let val = match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if let Err(e) = crate::db::validate_setting(k, &val) {
                    return error_result(e.to_string());
                }
                writes.push((k.clone(), val));
            }
            let mut applied = Vec::new();
            for (k, val) in writes {
                if let Err(e) = db.set_setting(&k, &val) {
                    return error_result(e.to_string());
                }
                if k == "llm_profile" {
                    crate::llm::set_profile(&val);
                }
                applied.push(k);
            }
            if applied.is_empty() {
                return error_result("cần ít nhất một cấu hình để đặt".into());
            }
            json_result(json!({ "ok": true, "applied": applied }))
        }

        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every advertised tool must have a dispatch arm. A typo in either list
    /// would otherwise only surface when an agent called the tool.
    #[tokio::test]
    async fn every_advertised_tool_is_dispatchable() {
        let core = {
            // An in-memory DB is enough; we only care that dispatch resolves.
            let db = crate::db::Db::open_memory().unwrap();
            std::sync::Arc::new(crate::state::Core::for_test(db))
        };
        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        let state = AppState { core, mcp_tx };

        for tool in tools_list().as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            let result = call_tool(&state, name, &json!({})).await;
            let text = result["content"][0]["text"].as_str().unwrap_or("");
            assert!(
                !text.contains("tool không tồn tại"),
                "{name} is advertised but has no dispatch arm"
            );
        }
    }

    #[tokio::test]
    async fn settings_set_rejects_an_unknown_key() {
        let db = crate::db::Db::open_memory().unwrap();
        let core = std::sync::Arc::new(crate::state::Core::for_test(db));
        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        let state = AppState { core, mcp_tx };

        let out = call_tool(&state, "rs_settings_set", &json!({ "nonsense": 1 })).await;
        assert_eq!(out["isError"], json!(true));
    }

    #[tokio::test]
    async fn story_get_paginates_instead_of_dumping_the_whole_novel() {
        let db = crate::db::Db::open_memory().unwrap();
        let body = "à".repeat(50_000);
        let sid = db
            .create_story("T", &body).unwrap();
        let core = std::sync::Arc::new(crate::state::Core::for_test(db));
        let (mcp_tx, _) = tokio::sync::broadcast::channel(8);
        let state = AppState { core, mcp_tx };

        let out = call_tool(&state, "rs_story_get", &json!({ "story_id": sid })).await;
        let payload: Value =
            serde_json::from_str(out["content"][0]["text"].as_str().unwrap()).unwrap();

        assert_eq!(payload["total_length"], json!(50_000));
        assert_eq!(payload["returned"], json!(4000), "must default to a window");
        assert_eq!(payload["has_more"], json!(true));
    }
}
