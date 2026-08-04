//! MCP server (HTTP + SSE) exposing the Thinking app to SenClaw agents.
//! Tool prefix `think_` (registered as `thinking-mcp` → full names
//! `mcp__thinking-mcp__think_*`); every tool calls the SAME `crate::api::*_value`
//! helpers the REST UI uses, so agents and humans see identical behavior.
//! All data is local — the app only records analysis and decisions on paper;
//! no tool executes any decision in the real world.

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
            "serverInfo": { "name": "thinking-mcp", "version": "1.0.0" }
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
            "name": "think_status",
            "description": "Trạng thái nhanh của app Tư Duy: tổng số vấn đề, số vấn đề mới/đang phân tích/đã quyết định, số vấn đề cần chú ý (phân tích dở dang).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "think_dashboard",
            "description": "Toàn cảnh app Tư Duy: đếm vấn đề theo trạng thái, danh sách vấn đề gần đây (kèm completeness % và số giải pháp), vấn đề cần chú ý (phân tích chưa xong hoặc chưa có giải pháp), hoạt động gần đây. Dùng tool này TRƯỚC khi trả lời câu hỏi tổng quan.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "think_problem_add",
            "description": "Tạo một vấn đề mới cần phân tích/ra quyết định. Nên điền description (chuyện gì đang xảy ra), context (bối cảnh: ai, quy mô, ràng buộc) và goal (kết quả mong muốn) càng cụ thể thì 5W/6 mũ càng chất lượng.",
            "inputSchema": { "type": "object", "properties": {
                "title":       { "type": "string", "description": "Tên vấn đề, ngắn gọn." },
                "description": { "type": "string", "description": "Chuyện gì đang xảy ra." },
                "context":     { "type": "string", "description": "Bối cảnh: ai liên quan, quy mô, ràng buộc." },
                "goal":        { "type": "string", "description": "Kết quả mong muốn sau khi giải quyết." },
                "priority":    { "type": "string", "enum": ["low","normal","high"] },
                "tags":        { "type": "string", "description": "Nhãn phân loại, phân cách bằng dấu phẩy." }
            }, "required": ["title"] }
        },
        {
            "name": "think_problem_list",
            "description": "Liệt kê vấn đề (mới cập nhật trước) kèm completeness % (5W chiếm 40, 6 mũ chiếm 60) và số giải pháp. Lọc: q (tìm trong tiêu đề/mô tả/tags), status (open|analyzing|decided|closed).",
            "inputSchema": { "type": "object", "properties": {
                "q":      { "type": "string" },
                "status": { "type": "string", "enum": ["open","analyzing","decided","closed"] },
                "limit":  { "type": "number", "description": "Mặc định 100." }
            } }
        },
        {
            "name": "think_problem_get",
            "description": "Chi tiết đầy đủ một vấn đề: thông tin + map 5W (who/what/when/where/why) + map 6 mũ (white/red/black/yellow/green/blue) + mọi giải pháp kèm đánh giá. Ô chưa điền có content rỗng.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id": { "type": "number" }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_problem_update",
            "description": "Sửa vấn đề (patch — chỉ trường truyền vào mới đổi): title, description, context, goal, priority (low|normal|high), status (open|analyzing|decided|closed), tags, synthesis, decision. Đóng vấn đề bằng status='closed'.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id":  { "type": "number" },
                "title":       { "type": "string" },
                "description": { "type": "string" },
                "context":     { "type": "string" },
                "goal":        { "type": "string" },
                "priority":    { "type": "string", "enum": ["low","normal","high"] },
                "status":      { "type": "string", "enum": ["open","analyzing","decided","closed"] },
                "tags":        { "type": "string" },
                "synthesis":   { "type": "string" },
                "decision":    { "type": "string" }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_problem_delete",
            "description": "Xoá hẳn một vấn đề cùng toàn bộ 5W, 6 mũ, giải pháp và đánh giá của nó. Không hoàn tác được — chỉ dùng khi người dùng xác nhận rõ.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id": { "type": "number" }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_5w_set",
            "description": "Ghi tay nội dung 5W cho một vấn đề. Truyền bất kỳ khóa nào trong who (ai liên quan), what (bản chất vấn đề), when (khi nào), where (ở đâu/khâu nào), why (nguyên nhân gốc) — khóa nào truyền thì ghi đè khóa đó.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id": { "type": "number" },
                "who":   { "type": "string" },
                "what":  { "type": "string" },
                "when":  { "type": "string" },
                "where": { "type": "string" },
                "why":   { "type": "string" }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_5w_generate",
            "description": "AI (qua bridge SenClaw) soạn nháp 5W từ mô tả vấn đề. Mặc định CHỈ điền ô trống — không ghi đè nội dung người dùng đã viết; force=true mới viết lại cả năm ô. Chỗ thiếu dữ kiện AI sẽ ghi 'Cần làm rõ: …'.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id": { "type": "number" },
                "force":      { "type": "boolean", "description": "true = ghi đè cả 5 ô." }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_hat_set",
            "description": "Ghi tay nội dung các mũ tư duy. Truyền bất kỳ khóa nào trong white (dữ kiện), red (cảm xúc), black (rủi ro), yellow (lợi ích), green (sáng tạo), blue (tổng kết) — khóa nào truyền thì ghi đè khóa đó.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id": { "type": "number" },
                "white":  { "type": "string" },
                "red":    { "type": "string" },
                "black":  { "type": "string" },
                "yellow": { "type": "string" },
                "green":  { "type": "string" },
                "blue":   { "type": "string" }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_hats_generate",
            "description": "AI (qua bridge SenClaw) chạy phiên 6 Mũ Tư Duy cho vấn đề: mũ Trắng dữ kiện, Đỏ cảm xúc, Đen rủi ro, Vàng lợi ích, Xanh Lá ý tưởng mới, Xanh Dương tổng kết quá trình. hat = một mũ cụ thể để chạy riêng mũ đó; mặc định chỉ điền mũ trống, force=true ghi đè.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id": { "type": "number" },
                "hat":        { "type": "string", "enum": ["white","red","black","yellow","green","blue"], "description": "Bỏ trống = cả sáu mũ." },
                "force":      { "type": "boolean" }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_solution_add",
            "description": "Thêm một giải pháp (do người dùng nghĩ ra) cho vấn đề. Sau khi thêm nên chấm điểm bằng think_solution_evaluate để so sánh được với các giải pháp khác.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id":  { "type": "number" },
                "title":       { "type": "string" },
                "description": { "type": "string" }
            }, "required": ["problem_id", "title"] }
        },
        {
            "name": "think_solutions_generate",
            "description": "AI (qua bridge SenClaw) đề xuất giải pháp mới theo tư duy mũ Xanh Lá — các hướng đi khác nhau rõ rệt, kèm cách làm và kết quả kỳ vọng. count mặc định 3 (2-6). Giải pháp sẵn có được giữ nguyên.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id": { "type": "number" },
                "count":      { "type": "number", "description": "Số giải pháp muốn AI đề xuất, 2-6, mặc định 3." }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_solution_update",
            "description": "Sửa một giải pháp (patch): title, description, status (proposed|chosen|rejected). Loại một giải pháp khỏi vòng xem xét bằng status='rejected'.",
            "inputSchema": { "type": "object", "properties": {
                "solution_id": { "type": "number" },
                "title":       { "type": "string" },
                "description": { "type": "string" },
                "status":      { "type": "string", "enum": ["proposed","chosen","rejected"] }
            }, "required": ["solution_id"] }
        },
        {
            "name": "think_solution_delete",
            "description": "Xoá một giải pháp cùng đánh giá của nó. Nếu nó đang là giải pháp được chọn của vấn đề thì tham chiếu quyết định cũng được gỡ.",
            "inputSchema": { "type": "object", "properties": {
                "solution_id": { "type": "number" }
            }, "required": ["solution_id"] }
        },
        {
            "name": "think_solution_evaluate",
            "description": "Đánh giá một giải pháp theo 4 tiêu chí 0-10: benefit (lợi ích — mũ Vàng), risk (rủi ro — mũ Đen, cao là XẤU), feasibility (khả thi), effort (công sức/chi phí, cao là XẤU). Truyền đủ cả 4 điểm = chấm tay; bỏ trống = AI chấm qua bridge kèm nhận xét từng mũ. Điểm tổng hợp 0-100 LUÔN do hệ thống tính (lợi ích 35% + an toàn 30% + khả thi 25% + nhẹ công 10%) — không tự bịa điểm tổng.",
            "inputSchema": { "type": "object", "properties": {
                "solution_id": { "type": "number" },
                "benefit":     { "type": "number" },
                "risk":        { "type": "number" },
                "feasibility": { "type": "number" },
                "effort":      { "type": "number" },
                "verdict":     { "type": "string", "description": "Một câu kết luận (tuỳ chọn, dùng khi chấm tay)." }
            }, "required": ["solution_id"] }
        },
        {
            "name": "think_compare",
            "description": "Bảng so sánh deterministic các giải pháp của một vấn đề: xếp hạng theo điểm tổng hợp giảm dần, chỉ ra giải pháp tốt nhất (best) và danh sách chưa được đánh giá. Dùng tool này khi được hỏi 'nên chọn phương án nào'.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id": { "type": "number" }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_decide",
            "description": "Chốt quyết định: chọn một giải pháp cho vấn đề kèm lý do (rationale). Vấn đề chuyển sang 'decided', giải pháp được đánh dấu 'chosen' (giải pháp từng chọn trước đó quay về 'proposed'). CHỈ gọi khi người dùng đã xác nhận lựa chọn — app không tự quyết thay người dùng.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id":  { "type": "number" },
                "solution_id": { "type": "number" },
                "rationale":   { "type": "string", "description": "Lý do chọn — nên tham chiếu điểm số và góc nhìn các mũ." }
            }, "required": ["problem_id", "solution_id"] }
        },
        {
            "name": "think_analyze",
            "description": "Chạy TRỌN GÓI một phiên phân tích cho vấn đề (qua bridge SenClaw), tuần tự các bước còn thiếu: 5W → 6 mũ → đề xuất 3 giải pháp (nếu chưa có) → AI chấm điểm mọi giải pháp chưa chấm → mũ Xanh Dương tổng hợp + khuyến nghị (lưu vào synthesis). Nội dung người dùng đã viết được GIỮ NGUYÊN. Bước nào lỗi thì dừng ở đó, các bước xong vẫn lưu — gọi lại để chạy tiếp. Kết quả kèm bảng điểm và khuyến nghị 'phân tích tham khảo'.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id": { "type": "number" },
                "question":   { "type": "string", "description": "Yêu cầu riêng cho phần tổng hợp, bỏ trống = khuyến nghị tổng quan." }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_report",
            "description": "Báo cáo markdown đầy đủ của một vấn đề theo đúng trình tự phương pháp: vấn đề → 5W → 6 mũ → bảng giải pháp & điểm → tổng hợp mũ Xanh Dương → quyết định. Dùng khi người dùng muốn xem/gửi bản phân tích hoàn chỉnh.",
            "inputSchema": { "type": "object", "properties": {
                "problem_id": { "type": "number" }
            }, "required": ["problem_id"] }
        },
        {
            "name": "think_activity",
            "description": "Nhật ký hành động gần đây của app (ai tạo vấn đề / điền phân tích / chốt quyết định khi nào).",
            "inputSchema": { "type": "object", "properties": {} }
        }
    ])
}

async fn call_tool(s: &AppState, name: &str, args: &Value) -> Value {
    let f64_arg = |k: &str| args.get(k).and_then(|x| x.as_f64());
    let i64_arg = |k: &str| args.get(k).and_then(|x| x.as_i64());
    let str_arg = |k: &str| {
        args.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };
    let opt_str = |k: &str| args.get(k).and_then(|x| x.as_str()).map(|v| v.to_string());
    let bool_arg = |k: &str| args.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    let need_problem = || -> Result<i64, Value> {
        i64_arg("problem_id").ok_or_else(|| error_result("thiếu 'problem_id'".into()))
    };
    let need_solution = || -> Result<i64, Value> {
        i64_arg("solution_id").ok_or_else(|| error_result("thiếu 'solution_id'".into()))
    };
    match name {
        "think_status" => json_result(&api::status_value(s)),
        "think_dashboard" => json_result(&api::dashboard_value(s)),
        "think_problem_add" => {
            let b = api::ProblemIn {
                title: str_arg("title"),
                description: str_arg("description"),
                context: str_arg("context"),
                goal: str_arg("goal"),
                priority: str_arg("priority"),
                tags: str_arg("tags"),
            };
            if b.title.trim().is_empty() {
                return error_result("thiếu 'title'".into());
            }
            json_result(&api::add_problem_value(s, &b))
        }
        "think_problem_list" => {
            let q = opt_str("q");
            let status = opt_str("status");
            json_result(&api::list_problems_value(
                s,
                q.as_deref(),
                status.as_deref(),
                i64_arg("limit").unwrap_or(100),
            ))
        }
        "think_problem_get" => match need_problem() {
            Ok(id) => json_result(&api::get_problem_value(s, id)),
            Err(e) => e,
        },
        "think_problem_update" => match need_problem() {
            Ok(id) => json_result(&api::update_problem_value(s, id, args)),
            Err(e) => e,
        },
        "think_problem_delete" => match need_problem() {
            Ok(id) => json_result(&api::delete_problem_value(s, id)),
            Err(e) => e,
        },
        "think_5w_set" => match need_problem() {
            Ok(id) => json_result(&api::set_5w_value(s, id, args, "user")),
            Err(e) => e,
        },
        "think_5w_generate" => match need_problem() {
            Ok(id) => json_result(&api::generate_5w_value(s, id, bool_arg("force")).await),
            Err(e) => e,
        },
        "think_hat_set" => match need_problem() {
            Ok(id) => json_result(&api::set_hats_value(s, id, args, "user")),
            Err(e) => e,
        },
        "think_hats_generate" => match need_problem() {
            Ok(id) => json_result(
                &api::generate_hats_value(s, id, &str_arg("hat"), bool_arg("force")).await,
            ),
            Err(e) => e,
        },
        "think_solution_add" => match need_problem() {
            Ok(id) => {
                let b = api::SolutionIn {
                    title: str_arg("title"),
                    description: str_arg("description"),
                };
                if b.title.trim().is_empty() {
                    return error_result("thiếu 'title'".into());
                }
                json_result(&api::add_solution_value(s, id, &b, "user"))
            }
            Err(e) => e,
        },
        "think_solutions_generate" => match need_problem() {
            Ok(id) => {
                let count = i64_arg("count").unwrap_or(3).clamp(2, 6) as usize;
                json_result(&api::generate_solutions_value(s, id, count).await)
            }
            Err(e) => e,
        },
        "think_solution_update" => match need_solution() {
            Ok(id) => json_result(&api::update_solution_value(s, id, args)),
            Err(e) => e,
        },
        "think_solution_delete" => match need_solution() {
            Ok(id) => json_result(&api::delete_solution_value(s, id)),
            Err(e) => e,
        },
        "think_solution_evaluate" => match need_solution() {
            Ok(id) => {
                let b = api::EvaluateIn {
                    benefit: f64_arg("benefit"),
                    risk: f64_arg("risk"),
                    feasibility: f64_arg("feasibility"),
                    effort: f64_arg("effort"),
                    verdict: str_arg("verdict"),
                };
                json_result(&api::evaluate_solution_value(s, id, &b).await)
            }
            Err(e) => e,
        },
        "think_compare" => match need_problem() {
            Ok(id) => json_result(&api::compare_value(s, id)),
            Err(e) => e,
        },
        "think_decide" => match need_problem() {
            Ok(id) => {
                let Some(sid) = i64_arg("solution_id") else {
                    return error_result("thiếu 'solution_id'".into());
                };
                let b = api::DecideIn {
                    solution_id: sid,
                    rationale: str_arg("rationale"),
                };
                json_result(&api::decide_value(s, id, &b))
            }
            Err(e) => e,
        },
        "think_analyze" => match need_problem() {
            Ok(id) => json_result(&api::analyze_value(s, id, &str_arg("question")).await),
            Err(e) => e,
        },
        "think_report" => match need_problem() {
            Ok(id) => {
                let v = api::report_value(s, id);
                match v["report"].as_str() {
                    Some(md) => text_result(md.to_string()),
                    None => json_result(&v),
                }
            }
            Err(e) => e,
        },
        "think_activity" => json_result(&json!({ "activity": s.db.recent_activity(50) })),
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
        assert_eq!(names.len(), 21);
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "tool names must be unique");
        assert!(
            names.iter().all(|n| n.starts_with("think_")),
            "all tools use the think_ prefix"
        );
    }

    #[test]
    fn every_tool_has_schema_and_description() {
        for t in tools_list().as_array().unwrap() {
            assert!(t["description"].as_str().unwrap().len() > 20);
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }

    #[test]
    fn evaluate_schema_has_four_criteria() {
        let tools = tools_list();
        let ev = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "think_solution_evaluate")
            .unwrap();
        for k in ["benefit", "risk", "feasibility", "effort"] {
            assert_eq!(ev["inputSchema"]["properties"][k]["type"], "number");
        }
    }

    #[test]
    fn hat_tools_enumerate_valid_hats() {
        let tools = tools_list();
        let gen = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "think_hats_generate")
            .unwrap();
        let hats: Vec<&str> = gen["inputSchema"]["properties"]["hat"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(hats, crate::logic::HAT_KEYS.to_vec());
    }
}
