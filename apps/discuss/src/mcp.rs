//! MCP server `discuss-mcp` — JSON-RPC 2.0 tự viết (không rmcp).
//!
//! Transport: GET /api/mcp/sse phát event `endpoint` trỏ về /api/mcp/message;
//! client POST JSON-RPC vào đó và nhận reply trong HTTP body. Reply KHÔNG
//! mirror lên SSE (tránh lộ payload của caller này cho mọi client khác —
//! bài học apps/study). Mọi tool gọi đúng các hàm `crate::api::*_value` mà
//! REST dùng — agent và người dùng thấy hành vi y hệt.

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde_json::{json, Value};
use std::convert::Infallible;
use std::time::Duration;

use crate::api::AppState;

pub async fn mcp_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.mcp_tx.subscribe();
    let stream = async_stream::stream! {
        yield Ok(Event::default().event("endpoint").data("/api/mcp/message"));
        loop {
            match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
                Ok(Ok(msg)) => yield Ok(Event::default().event("message").data(msg)),
                Ok(Err(_)) => break,
                Err(_) => yield Ok(Event::default().comment("keepalive")),
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn text_result(t: String) -> Value {
    json!({ "content": [{ "type": "text", "text": t }] })
}
fn json_result(v: &Value) -> Value {
    text_result(serde_json::to_string_pretty(v).unwrap_or_default())
}
fn error_result(t: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": t }] })
}
fn wrap(r: Result<Value, String>) -> Value {
    match r {
        Ok(v) => json_result(&v),
        Err(e) => error_result(e),
    }
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required })
}

pub fn tools_list() -> Vec<Value> {
    let disc_p = json!({ "type": "integer", "description": "ID phiên thảo luận" });
    vec![
        json!({ "name": "discuss_status",
            "description": "Tình trạng app AI Discuss Team: model LLM đang dùng, số thành viên, các phiên thảo luận gần nhất kèm trạng thái và điểm tiến độ của Manager.",
            "inputSchema": obj(json!({}), &[]) }),
        json!({ "name": "discuss_members",
            "description": "Danh sách thành viên trong đội (Manager, Thư ký, các member AI): key, vai trò, chuyên môn, mũ tư duy thiên hướng, có dùng tool không, giới hạn tool nếu có.",
            "inputSchema": obj(json!({}), &[]) }),
        json!({ "name": "discuss_member_add",
            "description": "Thêm thành viên AI mới vào đội. role: member (thảo luận) | manager (điều phối, không bàn nội dung) | secretary (ghi biên bản). hat = mũ THIÊN HƯỚNG, được chọn NHIỀU (chuỗi phẩy, vd 'black,red') — mỗi phát biểu member sẽ dùng 1 mũ trong thiên hướng: white|red|black|yellow|green|blue. use_tools=false thì member chỉ suy luận không gọi tool. tools = mảng tên tool đầy đủ (mcp__server__tool) để giới hạn, bỏ trống = toàn bộ tool hệ thống.",
            "inputSchema": obj(json!({
                "name": {"type": "string", "description": "Tên hiển thị, ví dụ 'Hà • Dữ liệu'"},
                "role": {"type": "string", "enum": ["member", "manager", "secretary"]},
                "expertise": {"type": "string"}, "style": {"type": "string"},
                "hat": {"type": "string", "description": "một hoặc nhiều mũ, phân tách phẩy: white,red,black,yellow,green,blue"},
                "use_tools": {"type": "boolean"},
                "tools": {"type": "array", "items": {"type": "string"}},
                "model": {"type": "string", "description": "LLM profile (id hoặc label trong Settings → Models) — mỗi member một model khác nhau được (vd 1 Gemini, 1 Claude); member không dùng tool chạy đúng model này, member dùng tool hiện vẫn theo model active của daemon"}
            }), &["name"]) }),
        json!({ "name": "discuss_member_update",
            "description": "Sửa thành viên theo key hoặc id: đổi tên/chuyên môn/phong cách/mũ thiên hướng (chọn nhiều, chuỗi phẩy vd 'black,red'), bật tắt (enabled), bật tắt dùng tool, đặt giới hạn tool (tools=null xoá giới hạn = dùng toàn bộ).",
            "inputSchema": obj(json!({
                "key": {"type": "string"}, "id": {"type": "integer"},
                "name": {"type": "string"}, "expertise": {"type": "string"}, "style": {"type": "string"},
                "hat": {"type": "string", "description": "một hoặc nhiều mũ phân tách phẩy"}, "use_tools": {"type": "boolean"},
                "tools": {"type": ["array", "null"], "items": {"type": "string"}},
                "model": {"type": ["string", "null"], "description": "LLM profile id/label; null = dùng model active"},
                "enabled": {"type": "boolean"}
            }), &[]) }),
        json!({ "name": "discuss_create",
            "description": "BOSS mở phiên thảo luận mới. title = chủ đề; requirement = yêu cầu kết quả (tiêu chí để Manager biết khi nào ĐỦ và chốt) — bắt buộc. member_keys chọn member tham gia (bỏ trống = mọi member đang bật). mode: sequential (lần lượt, mặc định) | parallel (song song). pace_secs = giây nghỉ giữa các lượt để BOSS đọc kịp (mặc định 20). start=true chạy luôn.",
            "inputSchema": obj(json!({
                "title": {"type": "string"},
                "requirement": {"type": "string"},
                "member_keys": {"type": "array", "items": {"type": "string"}},
                "mode": {"type": "string", "enum": ["sequential", "parallel"]},
                "pace_secs": {"type": "integer"}, "max_rounds": {"type": "integer"},
                "start": {"type": "boolean"}
            }), &["title", "requirement"]) }),
        json!({ "name": "discuss_start",
            "description": "Bắt đầu chạy phiên thảo luận (từ draft hoặc paused). Đội sẽ tự thảo luận theo vòng cho tới khi Manager thấy đủ hoặc chạm trần vòng.",
            "inputSchema": obj(json!({ "discussion_id": disc_p }), &["discussion_id"]) }),
        json!({ "name": "discuss_pause",
            "description": "Tạm dừng phiên đang chạy (đội dừng sau lượt hiện tại).",
            "inputSchema": obj(json!({ "discussion_id": disc_p }), &["discussion_id"]) }),
        json!({ "name": "discuss_resume",
            "description": "Chạy tiếp phiên đang tạm dừng.",
            "inputSchema": obj(json!({ "discussion_id": disc_p }), &["discussion_id"]) }),
        json!({ "name": "discuss_say",
            "description": "BOSS phát biểu vào phòng họp — chen bất kỳ lúc nào. Tin BOSS là ưu tiên số 1: member kế tiếp bắt buộc trả lời trước khi làm việc khác.",
            "inputSchema": obj(json!({ "discussion_id": disc_p, "content": {"type": "string"} }), &["discussion_id", "content"]) }),
        json!({ "name": "discuss_messages",
            "description": "Đọc diễn biến phiên (feed tăng dần). after = id tin cuối đã đọc (0 = từ đầu). Mỗi tin có loại luận điểm (evidence/inference/creative), mức chứng minh (practical/theoretical), mũ tư duy, thái độ (agree/disagree), trích dẫn.",
            "inputSchema": obj(json!({ "discussion_id": disc_p, "after": {"type": "integer"}, "limit": {"type": "integer"} }), &["discussion_id"]) }),
        json!({ "name": "discuss_minutes",
            "description": "Biên bản mới nhất do Thư ký AI tổng hợp: diễn biến, bảng luận điểm, đồng thuận, bất đồng mở, việc còn thiếu.",
            "inputSchema": obj(json!({ "discussion_id": disc_p }), &["discussion_id"]) }),
        json!({ "name": "discuss_progress",
            "description": "Tiến độ theo Manager: điểm 0-100 so với yêu cầu BOSS, phần còn thiếu, thống kê tham gia từng member (ai im lặng mấy vòng), luận điểm chưa ai phản hồi, trạng thái live của member.",
            "inputSchema": obj(json!({ "discussion_id": disc_p }), &["discussion_id"]) }),
        json!({ "name": "discuss_conclude",
            "description": "BOSS ép chốt phiên ngay: Thư ký tổng hợp KẾT QUẢ (mỗi kết luận gắn loại luận điểm + mức THỰC TIỄN/LÝ THUYẾT + nguồn), phiên chuyển sang chờ nghiệm thu.",
            "inputSchema": obj(json!({ "discussion_id": disc_p }), &["discussion_id"]) }),
        json!({ "name": "discuss_result",
            "description": "Đọc bản KẾT QUẢ mới nhất của phiên (draft đang chờ BOSS nghiệm thu, hoặc bản đã duyệt).",
            "inputSchema": obj(json!({ "discussion_id": disc_p }), &["discussion_id"]) }),
        json!({ "name": "discuss_approve",
            "description": "BOSS DUYỆT kết quả: phiên kết thúc, kết quả lưu vào kho tài liệu chung, mỗi member ghi bài học vào bộ nhớ riêng.",
            "inputSchema": obj(json!({ "discussion_id": disc_p }), &["discussion_id"]) }),
        json!({ "name": "discuss_reject",
            "description": "BOSS TỪ CHỐI kết quả kèm góp ý bắt buộc — phiên mở lại, góp ý thành tin BOSS ưu tiên, trần vòng được nới thêm 4.",
            "inputSchema": obj(json!({ "discussion_id": disc_p, "feedback": {"type": "string"} }), &["discussion_id", "feedback"]) }),
        json!({ "name": "discuss_docs_add",
            "description": "Thêm tài liệu văn bản vào kho chung (mọi thành viên đọc được, trích dẫn bằng doc:<id>). discussion_id bỏ trống = kho chung toàn app (tự vật chất hoá vào các phiên đang mở).",
            "inputSchema": obj(json!({
                "title": {"type": "string"}, "content": {"type": "string"},
                "discussion_id": {"type": "integer"}
            }), &["title", "content"]) }),
        json!({ "name": "discuss_docs_search",
            "description": "Tìm trong kho tài liệu chung (FTS tiếng Việt, gõ không dấu vẫn khớp). Trả preview — đọc đầy đủ bằng discuss_docs_get.",
            "inputSchema": obj(json!({
                "q": {"type": "string"}, "discussion_id": {"type": "integer"},
                "limit": {"type": "integer"}
            }), &[]) }),
        json!({ "name": "discuss_docs_get",
            "description": "Đọc toàn văn một tài liệu theo doc_id.",
            "inputSchema": obj(json!({ "doc_id": {"type": "integer"} }), &["doc_id"]) }),
        json!({ "name": "discuss_member_memory",
            "description": "Xem bộ nhớ riêng + mạch suy nghĩ (thinking) đã lưu của một member (theo key hoặc id) — thứ member đó mang theo xuyên phiên.",
            "inputSchema": obj(json!({ "key": {"type": "string"}, "id": {"type": "integer"}, "discussion_id": {"type": "integer"} }), &[]) }),
    ]
}

async fn call_tool(state: &AppState, name: &str, args: &Value) -> Value {
    let disc = || {
        args.get("discussion_id")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| "discussion_id là bắt buộc".to_string())
    };
    match name {
        "discuss_status" => wrap(crate::api::status_value(state).await),
        "discuss_members" => wrap(crate::api::members_value(state)),
        "discuss_member_add" => wrap(crate::api::member_add_value(state, args)),
        "discuss_member_update" => wrap(crate::api::member_update_value(state, args)),
        "discuss_member_memory" => wrap(crate::api::member_memory_value(state, args)),
        "discuss_create" => wrap(crate::api::discussion_create_value(state, args)),
        "discuss_start" => wrap(disc().and_then(|id| crate::api::start_value(state, id))),
        "discuss_pause" => wrap(disc().and_then(|id| crate::api::pause_value(state, id))),
        "discuss_resume" => wrap(disc().and_then(|id| crate::api::resume_value(state, id))),
        "discuss_conclude" => wrap(disc().and_then(|id| crate::api::conclude_value(state, id))),
        "discuss_approve" => wrap(disc().and_then(|id| crate::api::approve_value(state, id))),
        "discuss_reject" => wrap(disc().and_then(|id| {
            let fb = args.get("feedback").and_then(|v| v.as_str()).unwrap_or("");
            crate::api::reject_value(state, id, fb)
        })),
        "discuss_say" => wrap(disc().and_then(|id| {
            let c = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            crate::api::say_value(state, id, c)
        })),
        "discuss_messages" => wrap(disc().and_then(|id| {
            let after = args.get("after").and_then(|v| v.as_i64()).unwrap_or(0);
            let limit = args.get("limit").and_then(|v| v.as_i64()).unwrap_or(100);
            crate::api::messages_value(state, id, after, limit)
        })),
        "discuss_minutes" => wrap(disc().and_then(|id| crate::api::minutes_value(state, id))),
        "discuss_progress" => wrap(disc().and_then(|id| crate::api::progress_value(state, id))),
        "discuss_result" => wrap(disc().and_then(|id| crate::api::result_value(state, id))),
        "discuss_docs_add" => wrap(crate::api::docs_add_text_value(state, args)),
        "discuss_docs_search" => wrap(crate::api::docs_list_value(
            state,
            args.get("q").and_then(|v| v.as_str()),
            args.get("discussion_id").and_then(|v| v.as_i64()),
            args.get("limit").and_then(|v| v.as_i64()).unwrap_or(20),
        )),
        "discuss_docs_get" => wrap(
            args.get("doc_id")
                .and_then(|v| v.as_i64())
                .ok_or_else(|| "doc_id là bắt buộc".to_string())
                .and_then(|id| crate::api::docs_get_value(state, id)),
        ),
        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

pub async fn mcp_message(State(state): State<AppState>, Json(req): Json<Value>) -> Json<Value> {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "discuss-mcp", "version": env!("CARGO_PKG_VERSION") },
        }),
        "ping" => json!({}),
        "notifications/initialized" => json!({}),
        "tools/list" => json!({ "tools": tools_list() }),
        "tools/call" => {
            let name = req
                .pointer("/params/name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let default_args = json!({});
            let args = req.pointer("/params/arguments").unwrap_or(&default_args);
            call_tool(&state, name, args).await
        }
        _ => json!({}),
    };
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_wellformed() {
        let tools = tools_list();
        assert_eq!(tools.len(), 20);
        let mut names = std::collections::HashSet::new();
        for t in &tools {
            let name = t["name"].as_str().unwrap();
            assert!(name.starts_with("discuss_"), "tool {name} phải prefix discuss_");
            assert!(names.insert(name.to_string()), "tool {name} trùng tên");
            assert!(
                t["description"].as_str().unwrap().chars().count() > 20,
                "description của {name} quá ngắn"
            );
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[tokio::test]
    async fn call_tool_full_flow_without_llm() {
        let state = crate::api::make_test_state();
        // tạo phiên qua đường MCP
        let v = call_tool(
            &state,
            "discuss_create",
            &json!({ "title": "Chọn thị trường 2027", "requirement": "3 kết luận có mức chứng minh", "start": true }),
        )
        .await;
        let text = v["content"][0]["text"].as_str().unwrap();
        assert!(!v["isError"].as_bool().unwrap_or(false), "lỗi: {text}");
        let parsed: Value = serde_json::from_str(text).unwrap();
        let id = parsed["discussion"]["id"].as_i64().unwrap();

        // BOSS nói
        let v = call_tool(&state, "discuss_say", &json!({ "discussion_id": id, "content": "ưu tiên Đông Nam Á" })).await;
        assert!(!v["isError"].as_bool().unwrap_or(false));

        // đọc feed
        let v = call_tool(&state, "discuss_messages", &json!({ "discussion_id": id })).await;
        let text = v["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("ưu tiên Đông Nam Á"));

        // thêm + tìm + đọc tài liệu
        let v = call_tool(
            &state,
            "discuss_docs_add",
            &json!({ "title": "Số liệu xuất khẩu", "content": "Kim ngạch sang ASEAN tăng 12%" }),
        )
        .await;
        assert!(!v["isError"].as_bool().unwrap_or(false));
        let v = call_tool(&state, "discuss_docs_search", &json!({ "q": "asean" })).await;
        assert!(v["content"][0]["text"].as_str().unwrap().contains("ASEAN"));

        // progress + pause
        let v = call_tool(&state, "discuss_progress", &json!({ "discussion_id": id })).await;
        assert!(v["content"][0]["text"].as_str().unwrap().contains("participation"));
        let v = call_tool(&state, "discuss_pause", &json!({ "discussion_id": id })).await;
        assert!(!v["isError"].as_bool().unwrap_or(false));

        // tool sai tên → isError
        let v = call_tool(&state, "discuss_nope", &json!({})).await;
        assert!(v["isError"].as_bool().unwrap());
    }

    #[tokio::test]
    async fn mcp_message_initialize_and_list() {
        let state = crate::api::make_test_state();
        let resp = mcp_message(
            axum::extract::State(state.clone()),
            Json(json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" })),
        )
        .await;
        assert_eq!(resp.0["result"]["serverInfo"]["name"], "discuss-mcp");
        let resp = mcp_message(
            axum::extract::State(state),
            Json(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" })),
        )
        .await;
        assert!(resp.0["result"]["tools"].as_array().unwrap().len() >= 19);
    }
}
