//! MCP server (HTTP + SSE) exposing capital management to SenClaw agents.
//! Tool prefix `capital_` per the SenClaw naming convention; every tool calls
//! the SAME `crate::api::*_value` helpers the REST UI uses, so agents and
//! humans see identical behavior. All data is local — there is no tool that
//! moves real money anywhere; "pay" only RECORDS a payment the human made.

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
            "serverInfo": { "name": "capital-mcp", "version": "1.0.0" }
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
            "name": "capital_status",
            "description": "Trạng thái nhanh của sổ nguồn vốn: số nguồn đang hoạt động, tổng dư nợ, số kỳ trả nợ quá hạn.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "capital_dashboard",
            "description": "Toàn cảnh nguồn vốn: vốn chủ đã góp, dư nợ vay, hạn mức khả dụng, lãi đã trả, lãi suất nợ bình quân gia quyền, hệ số nợ/vốn chủ (D/E), kỳ trả nợ 30 ngày tới, kỳ quá hạn, dòng tiền 12 tháng và danh sách nguồn. Dùng tool này TRƯỚC khi phân tích hay trả lời câu hỏi tổng quan.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "capital_source_add",
            "description": "Thêm một nguồn vốn. kind: equity (vốn chủ) | investor (vốn góp NĐT) | bank_loan (vay ngân hàng) | credit_line (hạn mức tín dụng quay vòng) | personal_loan (vay cá nhân) | bond (trái phiếu) | grant (tài trợ) | other. total_amount = tổng cam kết/hạn mức; interest_rate = %/năm.",
            "inputSchema": { "type": "object", "properties": {
                "name":          { "type": "string" },
                "kind":          { "type": "string", "enum": ["equity","investor","bank_loan","credit_line","personal_loan","bond","grant","other"] },
                "provider":      { "type": "string", "description": "Ngân hàng/nhà đầu tư/bên cho vay." },
                "total_amount":  { "type": "number" },
                "currency":      { "type": "string", "description": "Mặc định VND." },
                "interest_rate": { "type": "number", "description": "%/năm, 0 nếu không lãi." },
                "rate_type":     { "type": "string", "enum": ["fixed","floating"] },
                "start_date":    { "type": "string", "description": "YYYY-MM-DD" },
                "end_date":      { "type": "string", "description": "YYYY-MM-DD (đáo hạn)" },
                "note":          { "type": "string" }
            }, "required": ["name", "kind"] }
        },
        {
            "name": "capital_source_list",
            "description": "Liệt kê nguồn vốn kèm số liệu: đã giải ngân, đã trả gốc, dư nợ (outstanding), còn rút được (available), lãi/phí đã trả. Lọc theo status: active|closed|pending.",
            "inputSchema": { "type": "object", "properties": {
                "status": { "type": "string", "enum": ["active","closed","pending"] }
            } }
        },
        {
            "name": "capital_source_get",
            "description": "Chi tiết một nguồn vốn: thông tin + toàn bộ giao dịch + lịch trả nợ của nguồn đó.",
            "inputSchema": { "type": "object", "properties": {
                "source_id": { "type": "number" }
            }, "required": ["source_id"] }
        },
        {
            "name": "capital_source_update",
            "description": "Sửa một nguồn vốn (patch — chỉ các trường truyền vào mới đổi). Đóng nguồn bằng status='closed'; nguồn đóng bị loại khỏi các chỉ số dashboard.",
            "inputSchema": { "type": "object", "properties": {
                "source_id":     { "type": "number" },
                "name":          { "type": "string" },
                "kind":          { "type": "string" },
                "provider":      { "type": "string" },
                "total_amount":  { "type": "number" },
                "currency":      { "type": "string" },
                "interest_rate": { "type": "number" },
                "rate_type":     { "type": "string" },
                "start_date":    { "type": "string" },
                "end_date":      { "type": "string" },
                "status":        { "type": "string", "enum": ["active","closed","pending"] },
                "note":          { "type": "string" }
            }, "required": ["source_id"] }
        },
        {
            "name": "capital_tx_add",
            "description": "Ghi một giao dịch vào sổ cái (chỉ GHI SỔ cục bộ — không chuyển tiền thật). kind: disburse (giải ngân/nhận vốn về) | repay_principal (trả gốc) | repay_interest (trả lãi) | fee (phí). alloc_id gắn giải ngân vào một phân bổ/dự án.",
            "inputSchema": { "type": "object", "properties": {
                "source_id": { "type": "number" },
                "kind":      { "type": "string", "enum": ["disburse","repay_principal","repay_interest","fee"] },
                "amount":    { "type": "number", "description": "> 0" },
                "tx_date":   { "type": "string", "description": "YYYY-MM-DD, bỏ trống = hôm nay." },
                "alloc_id":  { "type": "number", "description": "Phân bổ/dự án nhận vốn (tuỳ chọn)." },
                "note":      { "type": "string" }
            }, "required": ["source_id", "kind", "amount"] }
        },
        {
            "name": "capital_tx_list",
            "description": "Liệt kê giao dịch, lọc theo source_id / kind / alloc_id. Trả về kèm tên nguồn và tên phân bổ.",
            "inputSchema": { "type": "object", "properties": {
                "source_id": { "type": "number" },
                "kind":      { "type": "string", "enum": ["disburse","repay_principal","repay_interest","fee"] },
                "alloc_id":  { "type": "number" },
                "limit":     { "type": "number", "description": "Mặc định 200." }
            } }
        },
        {
            "name": "capital_schedule_generate",
            "description": "Sinh lịch trả nợ cho một nguồn (thay thế các kỳ CHƯA trả; kỳ đã trả giữ nguyên). method: annuity (niên kim — tổng trả mỗi kỳ bằng nhau, mặc định) | equal_principal (gốc chia đều) | interest_only (trả lãi định kỳ, gốc cuối kỳ). principal bỏ trống = dư nợ hiện tại; annual_rate bỏ trống = lãi suất của nguồn; freq_months: 1=tháng, 3=quý.",
            "inputSchema": { "type": "object", "properties": {
                "source_id":   { "type": "number" },
                "method":      { "type": "string", "enum": ["annuity","equal_principal","interest_only"] },
                "periods":     { "type": "number", "description": "Số kỳ trả." },
                "principal":   { "type": "number" },
                "annual_rate": { "type": "number", "description": "%/năm" },
                "start_date":  { "type": "string", "description": "Kỳ đầu = start_date + 1 kỳ. Bỏ trống = hôm nay." },
                "freq_months": { "type": "number", "description": "1=tháng (mặc định), 3=quý, 6, 12." }
            }, "required": ["source_id", "periods"] }
        },
        {
            "name": "capital_schedule_list",
            "description": "Xem lịch trả nợ. status: upcoming (sắp tới) | overdue (quá hạn) | paid (đã trả); bỏ trống = tất cả. Lọc thêm theo source_id.",
            "inputSchema": { "type": "object", "properties": {
                "source_id": { "type": "number" },
                "status":    { "type": "string", "enum": ["upcoming","overdue","paid"] },
                "limit":     { "type": "number" }
            } }
        },
        {
            "name": "capital_schedule_pay",
            "description": "Đánh dấu một kỳ trả nợ ĐÃ được con người thanh toán (ghi sổ — app không chuyển tiền). Mặc định tự ghi giao dịch trả gốc + trả lãi tương ứng vào sổ cái để dư nợ cập nhật; đặt create_tx=false nếu đã ghi tay.",
            "inputSchema": { "type": "object", "properties": {
                "schedule_id": { "type": "number" },
                "create_tx":   { "type": "boolean", "description": "Mặc định true." },
                "pay_date":    { "type": "string", "description": "YYYY-MM-DD, bỏ trống = hôm nay." }
            }, "required": ["schedule_id"] }
        },
        {
            "name": "capital_alloc_add",
            "description": "Thêm một phân bổ vốn (mục đích sử dụng / dự án) với hạn mức dự kiến target_amount. Gắn giải ngân vào phân bổ bằng alloc_id trong capital_tx_add.",
            "inputSchema": { "type": "object", "properties": {
                "name":          { "type": "string" },
                "description":   { "type": "string" },
                "target_amount": { "type": "number" }
            }, "required": ["name"] }
        },
        {
            "name": "capital_alloc_list",
            "description": "Liệt kê phân bổ vốn kèm used (đã rót) và remaining (còn lại so với target).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "capital_report_cashflow",
            "description": "Dòng tiền theo tháng: inflow (giải ngân/nhận vốn), outflow (trả gốc + lãi + phí), net. months mặc định 12.",
            "inputSchema": { "type": "object", "properties": {
                "months": { "type": "number" }
            } }
        },
        {
            "name": "capital_goal_add",
            "description": "Thêm MỤC TIÊU tài chính đo được từ sổ. kind: reduce_debt (đưa dư nợ về ≤ target, thêm source_id để giới hạn 1 nguồn) | payoff_source (tất toán 1 nguồn, cần source_id, target=0) | raise_equity (vốn chủ đã góp ≥ target) | raise_funding (tổng vốn huy động ≥ target) | build_reserve (nguồn còn rút được ≥ target). Baseline tự chụp tại thời điểm tạo — tiến độ đo tự động từ sổ cái, không cần cập nhật tay.",
            "inputSchema": { "type": "object", "properties": {
                "name":          { "type": "string" },
                "kind":          { "type": "string", "enum": ["reduce_debt","payoff_source","raise_equity","raise_funding","build_reserve"] },
                "target_amount": { "type": "number" },
                "deadline":      { "type": "string", "description": "YYYY-MM-DD (khuyến nghị — có deadline mới đánh giá được đúng tiến độ)." },
                "source_id":     { "type": "number", "description": "Bắt buộc với payoff_source; tuỳ chọn với reduce_debt." },
                "note":          { "type": "string" }
            }, "required": ["name", "kind"] }
        },
        {
            "name": "capital_goal_list",
            "description": "Liệt kê mục tiêu KÈM ĐÁNH GIÁ PHÁT TRIỂN tự động: giá trị hiện tại, % tiến độ so với % thời gian đã trôi, trạng thái (on_track/behind/at_risk/achieved/overdue), số còn thiếu, tốc độ cần mỗi tháng để kịp hạn, và các bước kế hoạch. Gọi tool này khi người dùng hỏi 'mục tiêu đến đâu rồi'.",
            "inputSchema": { "type": "object", "properties": {
                "status": { "type": "string", "enum": ["active","done","cancelled"] }
            } }
        },
        {
            "name": "capital_goal_update",
            "description": "Sửa mục tiêu (patch): name/target_amount/deadline/note, hoặc chốt trạng thái status=done|cancelled khi người dùng xác nhận.",
            "inputSchema": { "type": "object", "properties": {
                "goal_id":       { "type": "number" },
                "name":          { "type": "string" },
                "target_amount": { "type": "number" },
                "deadline":      { "type": "string" },
                "status":        { "type": "string", "enum": ["active","done","cancelled"] },
                "note":          { "type": "string" }
            }, "required": ["goal_id"] }
        },
        {
            "name": "capital_goal_plan",
            "description": "LÊN KẾ HOẠCH cho một mục tiêu: AI soạn các bước hành động cụ thể (bám số còn thiếu + tình trạng sổ), nếu AI không khả dụng thì tự chia mốc đều theo tháng/quý. Chạy lại sẽ thay các bước máy-tạo còn mở; bước tự thêm tay và bước đã xong giữ nguyên.",
            "inputSchema": { "type": "object", "properties": {
                "goal_id": { "type": "number" },
                "ai":      { "type": "boolean", "description": "false = bỏ qua AI, dùng chia mốc tự động. Mặc định true." }
            }, "required": ["goal_id"] }
        },
        {
            "name": "capital_goal_steps",
            "description": "Quản lý bước kế hoạch của một mục tiêu: action='add' (thêm bước tay: title, due_date, amount), 'done'/'todo' (đánh dấu bước, cần step_id), 'delete' (xoá bước, cần step_id).",
            "inputSchema": { "type": "object", "properties": {
                "goal_id":  { "type": "number" },
                "action":   { "type": "string", "enum": ["add","done","todo","delete"] },
                "step_id":  { "type": "number" },
                "title":    { "type": "string" },
                "due_date": { "type": "string" },
                "amount":   { "type": "number" }
            }, "required": ["goal_id", "action"] }
        },
        {
            "name": "capital_usage",
            "description": "PHÂN TÍCH SỬ DỤNG nguồn tiền: tổng đã giải ngân, phần đã gắn mục đích theo từng phân bổ (share %, so ngân sách, cờ vượt ngân sách), phần CHƯA phân loại, mức tận dụng từng nguồn (đã rút / cam kết, vốn nhàn rỗi) và các tín hiệu cảnh báo. Gọi khi người dùng hỏi 'tiền đã dùng vào đâu / dùng có hiệu quả không'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "capital_source_rate",
            "description": "ĐÁNH GIÁ TỪNG NGUỒN TIỀN (rule engine, giải thích được): mỗi nguồn một scorecard 0–100 + hạng A/B/C/D + verdict + danh sách yếu tố cộng/trừ điểm — chi phí so mặt bằng sổ, kỷ luật trả đúng hạn từ lịch sử, đáo hạn gần, lãi thả nổi, room hạn mức, thiếu lịch trả nợ; nguồn vốn chủ đánh giá mức thực hiện cam kết góp. Dùng để trả lời 'nguồn nào tốt, nguồn nào nên bỏ/đảo nợ'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "capital_evaluate",
            "description": "ĐÁNH GIÁ sức khoẻ nguồn vốn bằng rule engine (không LLM, tức thời, giải thích được): điểm 0–100 + hạng A/B/C/D + danh sách phát hiện kèm mức độ (good/warn/crit) — kỷ luật trả nợ (quá hạn), thanh khoản 30 ngày so với nguồn còn rút được, đòn bẩy D/E, chi phí vốn & khoản vay đắt bất thường, tập trung chủ nợ, áp lực đáo hạn 90 ngày, hạn mức gần cạn, nợ chưa có lịch trả. Gọi tool này khi người dùng hỏi 'vốn có ổn không / rủi ro gì'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "capital_simulate",
            "description": "MÔ PHỎNG what-if để hỗ trợ quyết định — KHÔNG ghi gì vào sổ. scenario='new_loan': vay thêm (amount, annual_rate, periods, method, freq_months) → kỳ trả đầu tiên, tổng lãi phải trả, lịch mẫu, và so sánh TRƯỚC/SAU (dư nợ, D/E, lãi suất bq, điểm sức khoẻ, nghĩa vụ theo tháng 12 tháng). scenario='early_repay': trả trước hạn (source_id, amount) → lãi tiết kiệm ước tính + so sánh trước/sau.",
            "inputSchema": { "type": "object", "properties": {
                "scenario":    { "type": "string", "enum": ["new_loan", "early_repay"] },
                "amount":      { "type": "number" },
                "annual_rate": { "type": "number", "description": "new_loan: %/năm." },
                "periods":     { "type": "number", "description": "new_loan: số kỳ trả." },
                "method":      { "type": "string", "enum": ["annuity","equal_principal","interest_only"] },
                "freq_months": { "type": "number", "description": "new_loan: 1=tháng (mặc định), 3=quý." },
                "source_id":   { "type": "number", "description": "early_repay: nguồn nợ muốn trả trước." }
            }, "required": ["scenario", "amount"] }
        },
        {
            "name": "capital_analyze",
            "description": "AI phân tích cơ cấu nguồn vốn (qua bridge SenClaw): rủi ro thanh khoản, cơ cấu nợ/vốn, chi phí lãi, khuyến nghị. Chỉ dựa trên số liệu trong app; kết quả luôn kèm lưu ý 'phân tích tham khảo'.",
            "inputSchema": { "type": "object", "properties": {
                "question": { "type": "string", "description": "Câu hỏi cụ thể, bỏ trống = phân tích tổng quan." }
            } }
        },
        {
            "name": "capital_activity",
            "description": "Nhật ký hành động gần đây của app (ai thêm nguồn/giao dịch/lịch khi nào).",
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
    match name {
        "capital_status" => json_result(&api::status_value(s)),
        "capital_dashboard" => json_result(&api::dashboard_value(s)),
        "capital_source_add" => {
            let b = api::SourceIn {
                name: str_arg("name"),
                kind: str_arg("kind"),
                provider: str_arg("provider"),
                total_amount: f64_arg("total_amount").unwrap_or(0.0),
                currency: str_arg("currency"),
                interest_rate: f64_arg("interest_rate").unwrap_or(0.0),
                rate_type: str_arg("rate_type"),
                start_date: str_arg("start_date"),
                end_date: str_arg("end_date"),
                note: str_arg("note"),
            };
            if b.name.is_empty() || b.kind.is_empty() {
                return error_result("cần 'name' và 'kind'".into());
            }
            json_result(&api::add_source_value(s, &b))
        }
        "capital_source_list" => {
            let st = args.get("status").and_then(|x| x.as_str());
            json_result(&api::list_sources_value(s, st))
        }
        "capital_source_get" => {
            let Some(id) = i64_arg("source_id") else {
                return error_result("thiếu 'source_id'".into());
            };
            json_result(&api::get_source_value(s, id))
        }
        "capital_source_update" => {
            let Some(id) = i64_arg("source_id") else {
                return error_result("thiếu 'source_id'".into());
            };
            json_result(&api::update_source_value(s, id, args))
        }
        "capital_tx_add" => {
            let (Some(source_id), Some(amount)) = (i64_arg("source_id"), f64_arg("amount")) else {
                return error_result("cần 'source_id' và 'amount'".into());
            };
            let b = api::TxIn {
                source_id,
                kind: str_arg("kind"),
                amount,
                alloc_id: i64_arg("alloc_id"),
                tx_date: str_arg("tx_date"),
                note: str_arg("note"),
            };
            json_result(&api::add_tx_value(s, &b))
        }
        "capital_tx_list" => {
            let kind = args.get("kind").and_then(|x| x.as_str());
            json_result(&json!({
                "transactions": s.db.list_tx(i64_arg("source_id"), kind, i64_arg("alloc_id"), i64_arg("limit").unwrap_or(200))
            }))
        }
        "capital_schedule_generate" => {
            let (Some(source_id), Some(periods)) = (i64_arg("source_id"), i64_arg("periods"))
            else {
                return error_result("cần 'source_id' và 'periods'".into());
            };
            let b = api::GenerateIn {
                source_id,
                method: str_arg("method"),
                principal: f64_arg("principal").unwrap_or(0.0),
                annual_rate: f64_arg("annual_rate"),
                periods: periods.max(0) as u32,
                start_date: str_arg("start_date"),
                freq_months: i64_arg("freq_months").unwrap_or(1).max(1) as u32,
            };
            json_result(&api::generate_schedule_value(s, &b))
        }
        "capital_schedule_list" => {
            let st = args.get("status").and_then(|x| x.as_str());
            json_result(&api::list_schedule_value(
                s,
                i64_arg("source_id"),
                st,
                i64_arg("limit").unwrap_or(500),
            ))
        }
        "capital_schedule_pay" => {
            let Some(id) = i64_arg("schedule_id") else {
                return error_result("thiếu 'schedule_id'".into());
            };
            let b = api::PayIn {
                create_tx: args.get("create_tx").and_then(|x| x.as_bool()),
                pay_date: str_arg("pay_date"),
            };
            json_result(&api::pay_schedule_value(s, id, &b))
        }
        "capital_alloc_add" => {
            let b = api::AllocIn {
                name: str_arg("name"),
                description: str_arg("description"),
                target_amount: f64_arg("target_amount").unwrap_or(0.0),
            };
            if b.name.is_empty() {
                return error_result("thiếu 'name'".into());
            }
            json_result(&api::add_alloc_value(s, &b))
        }
        "capital_alloc_list" => json_result(&api::list_allocs_value(s)),
        "capital_report_cashflow" => {
            json_result(&api::cashflow_value(s, i64_arg("months").unwrap_or(12)))
        }
        "capital_goal_add" => {
            let b = api::GoalIn {
                name: str_arg("name"),
                kind: str_arg("kind"),
                target_amount: f64_arg("target_amount").unwrap_or(0.0),
                source_id: i64_arg("source_id"),
                deadline: str_arg("deadline"),
                note: str_arg("note"),
            };
            if b.name.is_empty() || b.kind.is_empty() {
                return error_result("cần 'name' và 'kind'".into());
            }
            json_result(&api::goal_add_value(s, &b))
        }
        "capital_goal_list" => {
            let st = args.get("status").and_then(|x| x.as_str());
            json_result(&api::goals_list_value(s, st))
        }
        "capital_goal_update" => {
            let Some(id) = i64_arg("goal_id") else {
                return error_result("thiếu 'goal_id'".into());
            };
            json_result(&api::goal_update_value(s, id, args))
        }
        "capital_goal_plan" => {
            let Some(id) = i64_arg("goal_id") else {
                return error_result("thiếu 'goal_id'".into());
            };
            let ai = args.get("ai").and_then(|x| x.as_bool()).unwrap_or(true);
            json_result(&api::goal_plan_value(s, id, ai).await)
        }
        "capital_goal_steps" => {
            let Some(id) = i64_arg("goal_id") else {
                return error_result("thiếu 'goal_id'".into());
            };
            let b = api::StepIn {
                action: str_arg("action"),
                step_id: i64_arg("step_id"),
                title: str_arg("title"),
                due_date: str_arg("due_date"),
                amount: f64_arg("amount").unwrap_or(0.0),
            };
            json_result(&api::goal_steps_value(s, id, &b))
        }
        "capital_usage" => json_result(&api::usage_value(s)),
        "capital_source_rate" => json_result(&api::ratings_value(s)),
        "capital_evaluate" => json_result(&api::insight_value(s)),
        "capital_simulate" => {
            let Some(amount) = f64_arg("amount") else {
                return error_result("thiếu 'amount'".into());
            };
            let b = api::SimulateIn {
                scenario: str_arg("scenario"),
                amount,
                annual_rate: f64_arg("annual_rate").unwrap_or(0.0),
                periods: i64_arg("periods").unwrap_or(0).max(0) as u32,
                method: str_arg("method"),
                freq_months: i64_arg("freq_months").unwrap_or(1).max(1) as u32,
                source_id: i64_arg("source_id"),
            };
            json_result(&api::simulate_value(s, &b))
        }
        "capital_analyze" => {
            let q = str_arg("question");
            json_result(&api::analyze_value(s, &q).await)
        }
        "capital_activity" => json_result(&json!({ "activity": s.db.recent_activity(50) })),
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
        assert_eq!(names.len(), 25);
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "tool names must be unique");
        assert!(
            names.iter().all(|n| n.starts_with("capital_")),
            "all tools use the capital_ prefix"
        );
    }

    #[test]
    fn every_tool_has_schema_and_description() {
        for t in tools_list().as_array().unwrap() {
            assert!(t["description"].as_str().unwrap().len() > 20);
            assert_eq!(t["inputSchema"]["type"], "object");
        }
    }
}
