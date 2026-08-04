//! MCP server (HTTP + SSE) đưa Sentinel cho agent SenClaw.
//!
//! Tiền tố tool `sen_` (đăng ký là `sentinel-mcp` → tên đầy đủ
//! `mcp__sentinel-mcp__sen_*`). Mọi tool gọi CÙNG các hàm `crate::api::*_value`
//! mà giao diện web dùng, nên agent và người thấy hệt nhau.
//!
//! **Toàn bộ tool ở đây là chỉ-đọc và phân tích.** Không có tool nào sửa được
//! trạng thái daemon: không tạm dừng lịch, không tắt MCP server, không xoá luật.
//! Lý do: nếu chính agent đang bị chiếm quyền, nó không được phép dùng công cụ
//! điều tra để tự dọn dấu vết hay tự nới quyền. Việc đáp ứng nằm trên giao diện,
//! nơi con người bấm.

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
            "serverInfo": { "name": "sentinel-mcp", "version": "1.0.0" }
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

fn s(args: &Value, k: &str) -> Option<String> {
    args[k].as_str().map(|v| v.to_string())
}
fn i(args: &Value, k: &str) -> Option<i64> {
    args[k].as_i64()
}

fn tools_list() -> Value {
    json!([
        {
            "name": "sen_status",
            "description": "Trạng thái nhanh của Sentinel: số sự kiện đã thu, khoảng thời gian bao phủ, số phát hiện theo mức, tình trạng chuỗi băm chống sửa vết, và tình trạng từng nguồn trích xuất.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "sen_dashboard",
            "description": "Bảng tổng quan an ninh: thẻ tư thế (human-in-the-loop có đang tắt không, số luật auto-accept wildcard, có app nào mở ra LAN không, số lịch chạy shell), đếm phát hiện theo mức, hoạt động 14 ngày, và các phát hiện điểm cao nhất.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "sen_sources",
            "description": "Tình trạng từng nguồn chứng cứ — trả lời câu hỏi 'Sentinel đang mù chỗ nào'. Gồm: DB daemon (đọc được không, còn bao nhiêu dòng, đã bị FIFO xoá mất bao nhiêu), thư mục llm_logs, con trỏ trích xuất của từng nguồn kèm lỗi nếu có.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "sen_events",
            "description": "Truy vấn dòng thời gian hoạt động đã chuẩn hoá: tool đã chạy, yêu cầu và kết quả phê duyệt, lần chạy của lịch, tin nhắn. Lọc theo khoảng thời gian, đối tượng (actor), loại sự kiện, tên tool, và tìm toàn văn.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "from": { "type": "string", "description": "Mốc đầu, RFC3339 (ví dụ 2026-07-01T00:00:00Z)" },
                    "to": { "type": "string", "description": "Mốc cuối, RFC3339" },
                    "actor": { "type": "string", "description": "chat_jid, schedule:<id>, hoặc bg:<task_id>" },
                    "kind": { "type": "string", "description": "tool_call | permission_request | permission_resolved | schedule_run | message" },
                    "tool": { "type": "string", "description": "Khớp một phần tên tool" },
                    "q": { "type": "string", "description": "Tìm toàn văn trong tóm tắt và chi tiết" },
                    "limit": { "type": "number", "description": "Mặc định 200, tối đa 2000" }
                }
            }
        },
        {
            "name": "sen_event_detail",
            "description": "Chi tiết đầy đủ của một sự kiện kèm danh sách phát hiện đang trích dẫn nó làm chứng cứ. Dùng khi cần đọc nguyên văn phần đã lưu của một lượt gọi tool.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "number", "description": "id sự kiện" } },
                "required": ["id"]
            }
        },
        {
            "name": "sen_pivot",
            "description": "Xoay quanh một sự kiện để trả lời 'cái gì dẫn tới việc này'. Bốn kiểu: actor (mọi việc cùng đối tượng trong cửa sổ thời gian), tool (mọi lần dùng cùng tool), schedule (mọi sự kiện của cùng một lịch), preceding (những gì xảy ra NGAY TRƯỚC — dùng để tìm nguồn injection).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "number", "description": "id sự kiện làm mốc" },
                    "mode": { "type": "string", "enum": ["actor", "tool", "schedule", "preceding"], "description": "Kiểu xoay, mặc định actor" },
                    "minutes": { "type": "number", "description": "Nửa cửa sổ thời gian, mặc định 30 phút" }
                },
                "required": ["id"]
            }
        },
        {
            "name": "sen_findings",
            "description": "Danh sách phát hiện an ninh, xếp theo điểm giảm dần. Lọc theo trạng thái phân loại, mức nghiêm trọng, hoặc mã luật.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": ["open", "triaged", "accepted_risk", "false_positive", "resolved"] },
                    "severity": { "type": "string", "enum": ["critical", "high", "medium", "low", "info"] },
                    "rule": { "type": "string", "description": "Mã luật, ví dụ SEN-CTRL-01" },
                    "limit": { "type": "number", "description": "Mặc định 100" }
                }
            }
        },
        {
            "name": "sen_finding_detail",
            "description": "Chi tiết một phát hiện: mô tả đầy đủ, mô tả của luật đã sinh ra nó, và toàn bộ sự kiện chứng cứ được trích dẫn.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "number" } },
                "required": ["id"]
            }
        },
        {
            "name": "sen_finding_explain",
            "description": "Nhờ AI giải thích một phát hiện bằng lời thường: chuyện gì đã xảy ra, vì sao đáng quan tâm, và các bước kiểm chứng tiếp theo. AI chỉ diễn giải — mức nghiêm trọng vẫn do hệ thống chấm.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "number" } },
                "required": ["id"]
            }
        },
        {
            "name": "sen_finding_status",
            "description": "Đổi trạng thái phân loại của một phát hiện kèm ghi chú. Dùng để dọn hàng đợi: đánh dấu đã xem, chấp nhận rủi ro, hoặc kết luận dương tính giả.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "number" },
                    "status": { "type": "string", "enum": ["open", "triaged", "accepted_risk", "false_positive", "resolved"] },
                    "note": { "type": "string", "description": "Lý do — nên ghi để sau này còn hiểu vì sao" }
                },
                "required": ["id", "status"]
            }
        },
        {
            "name": "sen_scan",
            "description": "Chạy ngay toàn bộ luật phát hiện đang bật trên dữ liệu hiện có. Trả về số luật đã chạy, số phát hiện, số bị suppression che, và phân bố theo luật. Chạy lại không nhân bản phát hiện cũ.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "sen_ingest",
            "description": "Trích xuất ngay dấu vết mới từ DB daemon vào kho chỉ-thêm của Sentinel. Bình thường chạy tự động mỗi phút; gọi tool này khi cần dữ liệu mới nhất trước lúc điều tra.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "sen_rules",
            "description": "Danh mục toàn bộ luật phát hiện: mã, nhóm, mức mặc định, mức đang dùng, ánh xạ sang chuẩn OWASP LLM / Agentic, mô tả tín hiệu, trạng thái bật-tắt và tham số.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "sen_rule_config",
            "description": "Bật/tắt một luật, đổi mức nghiêm trọng, hoặc chỉnh tham số (ví dụ cửa sổ thời gian, ngưỡng tối thiểu). Dùng để giảm nhiễu cho môi trường cụ thể.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rule_id": { "type": "string", "description": "Ví dụ SEN-ANOM-01" },
                    "enabled": { "type": "boolean" },
                    "severity": { "type": "string", "enum": ["critical", "high", "medium", "low", "info"] },
                    "params": { "type": "object", "description": "Tham số riêng của luật, ví dụ {\"window_minutes\": 60}" }
                },
                "required": ["rule_id"]
            }
        },
        {
            "name": "sen_snapshots",
            "description": "Lịch sử ảnh chụp cấu hình. Daemon ghi đè cấu hình không kèm lịch sử, nên đây là nơi duy nhất biết được cấu hình đã đổi lúc nào. Chín nhóm: MCP server, manifest tool, luật auto-accept, nhóm, hook, cờ phê duyệt, skill, plugin, lịch.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "description": "Lọc theo nhóm, ví dụ tool_rules" },
                    "limit": { "type": "number" }
                }
            }
        },
        {
            "name": "sen_snapshot_take",
            "description": "Chụp ngay toàn bộ cấu hình và so với ảnh trước. Nội dung không đổi thì không lưu thêm bản ghi; có đổi thì sinh diff để các luật đọc.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "sen_snapshot_diff",
            "description": "Các thay đổi cấu hình đã phát hiện, dạng thêm/xoá/sửa theo khoá. Đây là chứng cứ cho những việc như một luật auto-accept mới xuất hiện hay mô tả tool của MCP server bị đổi.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string" },
                    "limit": { "type": "number" }
                }
            }
        },
        {
            "name": "sen_cases",
            "description": "Danh sách hồ sơ vụ việc đang điều tra, kèm số phát hiện đã gắn vào mỗi vụ.",
            "inputSchema": {
                "type": "object",
                "properties": { "status": { "type": "string", "enum": ["open", "investigating", "closed"] } }
            }
        },
        {
            "name": "sen_case_open",
            "description": "Mở một hồ sơ vụ việc mới và gắn sẵn các phát hiện liên quan. Dùng khi nhiều phát hiện rời rạc thật ra là cùng một sự việc cần điều tra chung.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "summary": { "type": "string" },
                    "severity": { "type": "string", "enum": ["critical", "high", "medium", "low"] },
                    "finding_ids": { "type": "array", "items": { "type": "number" } }
                },
                "required": ["title"]
            }
        },
        {
            "name": "sen_case_detail",
            "description": "Chi tiết một vụ việc: các phát hiện đã gắn, dòng thời gian hợp nhất từ chứng cứ của chúng, giả thuyết hiện tại và toàn bộ ghi chú điều tra.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "number" } },
                "required": ["id"]
            }
        },
        {
            "name": "sen_case_note",
            "description": "Thêm một ghi chú điều tra vào hồ sơ vụ việc. Ghi chú là nơi lưu những gì con người đã kiểm chứng bên ngoài hệ thống.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "number" },
                    "body": { "type": "string" },
                    "author": { "type": "string", "description": "user hoặc ai; mặc định user" }
                },
                "required": ["id", "body"]
            }
        },
        {
            "name": "sen_case_hypothesis",
            "description": "Nhờ AI dựng giả thuyết cho một vụ việc: các chuỗi nhân quả khả dĩ xếp theo mức khớp chứng cứ, một giả thuyết vô hại để đối chứng, và danh sách chứng cứ còn thiếu. Kết quả là bản nháp cho người sửa.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "number" } },
                "required": ["id"]
            }
        },
        {
            "name": "sen_case_report",
            "description": "Sinh báo cáo điều tra Markdown đầy đủ cho một vụ việc: tóm tắt điều hành, diễn biến, phát hiện, đánh giá, khuyến nghị, và phần hạn chế nói rõ dữ liệu nào không có.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "number" } },
                "required": ["id"]
            }
        },
        {
            "name": "sen_ask",
            "description": "Hỏi bằng lời thường về hoạt động trong một khoảng thời gian, ví dụ 'tuần này có gì bất thường không'. App tự lọc dữ liệu theo khoảng thời gian rồi mới đưa cho AI tóm tắt — câu hỏi không được dùng để sinh truy vấn.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "question": { "type": "string" },
                    "from": { "type": "string", "description": "Mốc đầu, RFC3339" },
                    "to": { "type": "string", "description": "Mốc cuối, RFC3339" }
                },
                "required": ["question"]
            }
        },
        {
            "name": "sen_suppress",
            "description": "Tạo một ngoại lệ có chủ đích cho một luật, bắt buộc kèm lý do và nên kèm hạn dùng. Dùng để giảm nhiễu mà vẫn giữ được vết vì sao đã bỏ qua.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rule_id": { "type": "string" },
                    "reason": { "type": "string", "description": "Bắt buộc — sáu tháng sau còn phải hiểu vì sao" },
                    "actor": { "type": "string", "description": "Chỉ bỏ qua với đối tượng này" },
                    "contains": { "type": "string", "description": "Chỉ bỏ qua khi tiêu đề/mô tả chứa chuỗi này" },
                    "until": { "type": "string", "description": "Hạn dùng, RFC3339; bỏ trống là vĩnh viễn" }
                },
                "required": ["rule_id", "reason"]
            }
        },
        {
            "name": "sen_tool_args",
            "description": "Khôi phục ĐỐI SỐ của các lượt gọi tool trong một ngày từ ~/.senclaw/llm_logs. Cần thiết vì bảng tool_executions của daemon chỉ lưu kết quả chứ không lưu tham số — không có đường dẫn file, URL hay tham số MCP nào (Bash là ngoại lệ, và cũng chỉ 100 ký tự đầu). Đối số trả về đã được lọc bí mật.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "date": { "type": "string", "description": "Ngày dạng YYYY-MM-DD; mặc định hôm nay" },
                    "limit": { "type": "number", "description": "Số lượt tối đa, mặc định 200" }
                }
            }
        },
        {
            "name": "sen_verify_chain",
            "description": "Kiểm tra chuỗi băm của kho sự kiện. Nếu ai đó sửa hoặc xoá một bản ghi quá khứ, chuỗi gãy và tool này chỉ ra đúng vị trí. Đây là kiểm chứng tính toàn vẹn của chính chứng cứ.",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

async fn call_tool(st: &AppState, name: &str, args: &Value) -> Value {
    match name {
        "sen_status" => json_result(&api::status_value(st)),
        "sen_dashboard" => json_result(&api::dashboard_value(st)),
        "sen_sources" => json_result(&api::sources_value(st).await),

        "sen_events" => {
            let q = api::EventQuery {
                from: s(args, "from"),
                to: s(args, "to"),
                actor: s(args, "actor"),
                kind: s(args, "kind"),
                tool: s(args, "tool"),
                q: s(args, "q"),
                limit: i(args, "limit"),
                before_id: None,
            };
            json_result(&api::events_value(st, &q))
        }
        "sen_event_detail" => match i(args, "id") {
            Some(id) => json_result(&api::event_detail_value(st, id)),
            None => error_result("thiếu tham số id".into()),
        },
        "sen_pivot" => match i(args, "id") {
            Some(id) => json_result(&api::pivot_value(
                st,
                id,
                &s(args, "mode").unwrap_or_else(|| "actor".into()),
                i(args, "minutes").unwrap_or(30),
            )),
            None => error_result("thiếu tham số id".into()),
        },

        "sen_findings" => {
            let q = api::FindingQuery {
                status: s(args, "status"),
                severity: s(args, "severity"),
                rule: s(args, "rule"),
                limit: i(args, "limit"),
            };
            json_result(&api::findings_value(st, &q))
        }
        "sen_finding_detail" => match i(args, "id") {
            Some(id) => json_result(&api::finding_detail_value(st, id)),
            None => error_result("thiếu tham số id".into()),
        },
        "sen_finding_explain" => match i(args, "id") {
            Some(id) => json_result(&api::finding_explain_value(st, id).await),
            None => error_result("thiếu tham số id".into()),
        },
        "sen_finding_status" => match (i(args, "id"), s(args, "status")) {
            (Some(id), Some(status)) => {
                let body = api::StatusBody {
                    status,
                    note: s(args, "note"),
                };
                json_result(&api::finding_status_value(st, id, &body))
            }
            _ => error_result("cần cả id và status".into()),
        },

        "sen_scan" => json_result(&api::scan_value(st).await),
        "sen_ingest" => json_result(&api::ingest_value(st)),

        "sen_rules" => json_result(&api::rules_value(st)),
        "sen_rule_config" => match s(args, "rule_id") {
            Some(id) => {
                let body = api::RuleBody {
                    enabled: args["enabled"].as_bool(),
                    severity: s(args, "severity"),
                    params: if args["params"].is_object() {
                        Some(args["params"].clone())
                    } else {
                        None
                    },
                };
                json_result(&api::rule_update_value(st, &id, &body))
            }
            None => error_result("thiếu tham số rule_id".into()),
        },

        "sen_snapshots" => {
            let q = api::SnapQuery {
                kind: s(args, "kind"),
                limit: i(args, "limit"),
            };
            json_result(&api::snapshots_value(st, &q))
        }
        "sen_snapshot_take" => json_result(&api::snapshots_take_value(st).await),
        "sen_snapshot_diff" => {
            let q = api::SnapQuery {
                kind: s(args, "kind"),
                limit: i(args, "limit"),
            };
            json_result(&api::snapshot_diffs_value(st, &q))
        }

        "sen_cases" => {
            let q = api::CaseQuery {
                status: s(args, "status"),
            };
            json_result(&api::cases_value(st, &q))
        }
        "sen_case_open" => match s(args, "title") {
            Some(title) => {
                let body = api::CaseCreate {
                    title,
                    summary: s(args, "summary"),
                    severity: s(args, "severity"),
                    finding_ids: args["finding_ids"].as_array().map(|a| {
                        a.iter().filter_map(|v| v.as_i64()).collect::<Vec<_>>()
                    }),
                };
                json_result(&api::case_create_value(st, &body))
            }
            None => error_result("thiếu tham số title".into()),
        },
        "sen_case_detail" => match i(args, "id") {
            Some(id) => json_result(&api::case_get_value(st, id)),
            None => error_result("thiếu tham số id".into()),
        },
        "sen_case_note" => match (i(args, "id"), s(args, "body")) {
            (Some(id), Some(body)) => {
                let b = api::NoteBody {
                    body,
                    author: s(args, "author"),
                };
                json_result(&api::case_note_value(st, id, &b))
            }
            _ => error_result("cần cả id và body".into()),
        },
        "sen_case_hypothesis" => match i(args, "id") {
            Some(id) => json_result(&api::case_hypothesis_value(st, id).await),
            None => error_result("thiếu tham số id".into()),
        },
        "sen_case_report" => match i(args, "id") {
            Some(id) => json_result(&api::case_report_value(st, id).await),
            None => error_result("thiếu tham số id".into()),
        },

        "sen_ask" => match s(args, "question") {
            Some(question) => {
                let b = api::AskBody {
                    question,
                    from: s(args, "from"),
                    to: s(args, "to"),
                };
                json_result(&api::ask_value(st, &b).await)
            }
            None => error_result("thiếu tham số question".into()),
        },
        "sen_suppress" => match (s(args, "rule_id"), s(args, "reason")) {
            (Some(rule_id), Some(reason)) => {
                let mut m = json!({});
                if let Some(a) = s(args, "actor") {
                    m["actor"] = json!(a);
                }
                if let Some(c) = s(args, "contains") {
                    m["contains"] = json!(c);
                }
                let b = api::SuppressBody {
                    rule_id,
                    r#match: m,
                    reason,
                    until: s(args, "until"),
                };
                json_result(&api::suppression_add_value(st, &b))
            }
            _ => error_result("cần cả rule_id và reason".into()),
        },

        "sen_tool_args" => {
            let q = api::ToolArgsQuery {
                date: s(args, "date"),
                limit: i(args, "limit"),
            };
            json_result(&api::tool_args_value(&q))
        }
        "sen_verify_chain" => json_result(&api::verify_chain_value(st)),

        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_have_unique_prefixed_names() {
        let tools = tools_list();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 27);
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "tên tool phải duy nhất");
        assert!(
            names.iter().all(|n| n.starts_with("sen_")),
            "mọi tool dùng tiền tố sen_"
        );
    }

    #[test]
    fn every_tool_has_schema_and_description() {
        for t in tools_list().as_array().unwrap() {
            assert!(
                t["description"].as_str().unwrap().chars().count() > 20,
                "{} thiếu mô tả",
                t["name"]
            );
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        }
    }

    #[test]
    fn required_params_are_declared_in_schema() {
        let tools = tools_list();
        let arr = tools.as_array().unwrap();
        for (name, req) in [
            ("sen_event_detail", "id"),
            ("sen_pivot", "id"),
            ("sen_finding_detail", "id"),
            ("sen_case_open", "title"),
            ("sen_ask", "question"),
            ("sen_rule_config", "rule_id"),
        ] {
            let t = arr.iter().find(|t| t["name"] == name).unwrap();
            let required: Vec<&str> = t["inputSchema"]["required"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|v| v.as_str())
                .collect();
            assert!(required.contains(&req), "{name} phải yêu cầu {req}");
        }
    }

    #[test]
    fn pivot_enum_matches_implementation() {
        let tools = tools_list();
        let t = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "sen_pivot")
            .unwrap();
        let modes: Vec<&str> = t["inputSchema"]["properties"]["mode"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(modes, vec!["actor", "tool", "schedule", "preceding"]);
    }

    #[test]
    fn finding_status_enum_matches_db_whitelist() {
        let tools = tools_list();
        let t = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "sen_finding_status")
            .unwrap();
        let vals: Vec<&str> = t["inputSchema"]["properties"]["status"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            vals,
            vec!["open", "triaged", "accepted_risk", "false_positive", "resolved"]
        );
    }

    /// Bất biến an ninh: agent không được dùng công cụ điều tra để sửa trạng thái
    /// daemon. Test này gãy nếu ai đó thêm tool ghi vào MCP surface.
    #[test]
    fn no_tool_mutates_daemon_state() {
        let tools = tools_list();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for banned in [
            "pause", "resume", "delete", "disable", "enable_server", "kill", "stop",
        ] {
            assert!(
                !names.iter().any(|n| n.contains(banned)),
                "MCP của Sentinel không được có tool sửa daemon ({banned})"
            );
        }
    }

    #[tokio::test]
    async fn unknown_tool_returns_error_result() {
        let st = AppState {
            db: std::sync::Arc::new(crate::db::Db::open_memory().unwrap()),
            sc: app_space_sdk::SpaceClient::new("http://127.0.0.1:1", "sentinel"),
            mcp_tx: tokio::sync::broadcast::channel(10).0,
            ticks: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };
        let r = call_tool(&st, "sen_khong_co", &json!({})).await;
        assert_eq!(r["isError"], true);
    }

    #[tokio::test]
    async fn missing_required_arg_is_a_clean_error() {
        let st = AppState {
            db: std::sync::Arc::new(crate::db::Db::open_memory().unwrap()),
            sc: app_space_sdk::SpaceClient::new("http://127.0.0.1:1", "sentinel"),
            mcp_tx: tokio::sync::broadcast::channel(10).0,
            ticks: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };
        let r = call_tool(&st, "sen_event_detail", &json!({})).await;
        assert_eq!(r["isError"], true);
        assert!(r["content"][0]["text"].as_str().unwrap().contains("id"));
    }

    #[tokio::test]
    async fn status_tool_returns_json_payload() {
        let st = AppState {
            db: std::sync::Arc::new(crate::db::Db::open_memory().unwrap()),
            sc: app_space_sdk::SpaceClient::new("http://127.0.0.1:1", "sentinel"),
            mcp_tx: tokio::sync::broadcast::channel(10).0,
            ticks: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        };
        let r = call_tool(&st, "sen_status", &json!({})).await;
        let text = r["content"][0]["text"].as_str().unwrap();
        let v: Value = serde_json::from_str(text).unwrap();
        assert_eq!(v["app"], "sentinel");
    }
}
