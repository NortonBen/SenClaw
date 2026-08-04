//! MCP server (HTTP + SSE) exposing warehouse management to SenClaw agents.
//! Tool prefix `wh_` (registered as `warehouse-mcp` → full names
//! `mcp__warehouse-mcp__wh_*`); every tool calls the SAME `crate::api::*_value`
//! helpers the REST UI uses, so agents and humans see identical behavior.
//! All data is local — the app only KEEPS the stock ledger; no tool ships,
//! sells or orders anything in the real world.

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
            "serverInfo": { "name": "warehouse-mcp", "version": "1.0.0" }
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
            "name": "wh_status",
            "description": "Trạng thái nhanh của kho: số sản phẩm đang bán, tổng giá trị tồn kho, số mặt hàng dưới tồn tối thiểu.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "wh_dashboard",
            "description": "Toàn cảnh kho hàng: giá trị tồn kho, danh sách hàng dưới tồn tối thiểu, số mặt hàng hết hàng, nhập/xuất 30 ngày, biểu đồ nhập-xuất 12 tháng, top sản phẩm theo giá trị tồn, các kho kèm giá trị, phiếu gần đây. Dùng tool này TRƯỚC khi phân tích hay trả lời câu hỏi tổng quan.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "wh_product_add",
            "description": "Thêm sản phẩm vào danh mục. min_stock = tồn tối thiểu để cảnh báo sắp hết hàng; cost_price = giá vốn tham khảo (khi đã có phiếu nhập, giá vốn dùng bình quân gia quyền theo phiếu); sell_price = giá bán mặc định.",
            "inputSchema": { "type": "object", "properties": {
                "name":       { "type": "string" },
                "sku":        { "type": "string", "description": "Mã hàng, duy nhất (tuỳ chọn)." },
                "unit":       { "type": "string", "description": "Đơn vị tính, mặc định 'cái'." },
                "category":   { "type": "string" },
                "barcode":    { "type": "string" },
                "cost_price": { "type": "number" },
                "sell_price": { "type": "number" },
                "min_stock":  { "type": "number", "description": "Cảnh báo khi tồn < mức này (0 = không cảnh báo)." },
                "note":       { "type": "string" }
            }, "required": ["name"] }
        },
        {
            "name": "wh_product_update",
            "description": "Sửa sản phẩm (patch — chỉ các trường truyền vào mới đổi). Ngừng kinh doanh bằng status='inactive' (sản phẩm inactive bị loại khỏi dashboard).",
            "inputSchema": { "type": "object", "properties": {
                "product_id": { "type": "number" },
                "sku":        { "type": "string" },
                "name":       { "type": "string" },
                "unit":       { "type": "string" },
                "category":   { "type": "string" },
                "barcode":    { "type": "string" },
                "cost_price": { "type": "number" },
                "sell_price": { "type": "number" },
                "min_stock":  { "type": "number" },
                "status":     { "type": "string", "enum": ["active","inactive"] },
                "note":       { "type": "string" }
            }, "required": ["product_id"] }
        },
        {
            "name": "wh_product_list",
            "description": "Liệt kê sản phẩm kèm số liệu suy ra: on_hand (tồn tổng các kho), avg_cost (giá vốn bình quân), stock_value, low_stock. Lọc: q (tìm theo tên/SKU/barcode), category, status, low_stock=true (chỉ hàng sắp hết).",
            "inputSchema": { "type": "object", "properties": {
                "q":         { "type": "string" },
                "category":  { "type": "string" },
                "status":    { "type": "string", "enum": ["active","inactive"] },
                "low_stock": { "type": "boolean", "description": "true = chỉ hàng dưới tồn tối thiểu." }
            } }
        },
        {
            "name": "wh_product_get",
            "description": "Chi tiết một sản phẩm: thông tin + tồn theo từng kho + 50 dòng thẻ kho gần nhất.",
            "inputSchema": { "type": "object", "properties": {
                "product_id": { "type": "number" }
            }, "required": ["product_id"] }
        },
        {
            "name": "wh_warehouse_add",
            "description": "Thêm một kho / chi nhánh mới.",
            "inputSchema": { "type": "object", "properties": {
                "name":     { "type": "string" },
                "location": { "type": "string" },
                "note":     { "type": "string" }
            }, "required": ["name"] }
        },
        {
            "name": "wh_warehouse_list",
            "description": "Liệt kê các kho kèm sku_count (số mặt hàng đang có tồn) và stock_value (giá trị tồn tại kho đó). Lọc theo status: active|inactive.",
            "inputSchema": { "type": "object", "properties": {
                "status": { "type": "string", "enum": ["active","inactive"] }
            } }
        },
        {
            "name": "wh_warehouse_update",
            "description": "Sửa thông tin một kho (patch). Ngừng dùng kho bằng status='inactive'.",
            "inputSchema": { "type": "object", "properties": {
                "warehouse_id": { "type": "number" },
                "name":         { "type": "string" },
                "location":     { "type": "string" },
                "note":         { "type": "string" },
                "status":       { "type": "string", "enum": ["active","inactive"] }
            }, "required": ["warehouse_id"] }
        },
        {
            "name": "wh_partner_add",
            "description": "Thêm đối tác. kind: supplier (nhà cung cấp) | customer (khách hàng) | other. Gắn vào phiếu nhập/xuất bằng partner_id.",
            "inputSchema": { "type": "object", "properties": {
                "name":    { "type": "string" },
                "kind":    { "type": "string", "enum": ["supplier","customer","other"] },
                "phone":   { "type": "string" },
                "address": { "type": "string" },
                "note":    { "type": "string" }
            }, "required": ["name"] }
        },
        {
            "name": "wh_partner_list",
            "description": "Liệt kê đối tác, lọc theo kind: supplier|customer|other.",
            "inputSchema": { "type": "object", "properties": {
                "kind": { "type": "string", "enum": ["supplier","customer","other"] }
            } }
        },
        {
            "name": "wh_move_create",
            "description": "Tạo phiếu kho (chỉ GHI SỔ cục bộ). kind: receipt (nhập kho — unit_price là giá vốn nhập) | issue (xuất kho — unit_price là giá bán/xuất, tồn phải đủ) | transfer (chuyển kho — cần to_warehouse_id khác kho đi) | adjust (điều chỉnh kiểm kê — qty là DELTA có dấu: thừa dương, thiếu âm). Một phiếu nhiều dòng hàng được. Mã phiếu tự sinh: NK-/XK-/CK-/DC-.",
            "inputSchema": { "type": "object", "properties": {
                "kind":            { "type": "string", "enum": ["receipt","issue","transfer","adjust"] },
                "warehouse_id":    { "type": "number", "description": "Kho thao tác (kho đi nếu là transfer)." },
                "to_warehouse_id": { "type": "number", "description": "Kho đến, chỉ dùng cho transfer." },
                "partner_id":      { "type": "number", "description": "Nhà cung cấp (nhập) / khách hàng (xuất), tuỳ chọn." },
                "move_date":       { "type": "string", "description": "YYYY-MM-DD, bỏ trống = hôm nay." },
                "note":            { "type": "string" },
                "lines":           { "type": "array", "description": "Các dòng hàng của phiếu.", "items": {
                    "type": "object", "properties": {
                        "product_id": { "type": "number" },
                        "qty":        { "type": "number", "description": "> 0; riêng adjust là delta có dấu, ≠ 0." },
                        "unit_price": { "type": "number", "description": "Đơn giá trên phiếu, mặc định 0." }
                    }, "required": ["product_id", "qty"]
                } }
            }, "required": ["kind", "warehouse_id", "lines"] }
        },
        {
            "name": "wh_move_list",
            "description": "Liệt kê phiếu kho (mới nhất trước) kèm tổng số lượng/giá trị mỗi phiếu. Lọc: kind, warehouse_id (khớp cả kho đi lẫn kho đến), product_id, date_from/date_to (YYYY-MM-DD).",
            "inputSchema": { "type": "object", "properties": {
                "kind":         { "type": "string", "enum": ["receipt","issue","transfer","adjust"] },
                "warehouse_id": { "type": "number" },
                "product_id":   { "type": "number" },
                "date_from":    { "type": "string" },
                "date_to":      { "type": "string" },
                "limit":        { "type": "number", "description": "Mặc định 100." }
            } }
        },
        {
            "name": "wh_move_get",
            "description": "Chi tiết một phiếu kho: header + toàn bộ dòng hàng (tên sản phẩm, số lượng, đơn giá, thành tiền).",
            "inputSchema": { "type": "object", "properties": {
                "move_id": { "type": "number" }
            }, "required": ["move_id"] }
        },
        {
            "name": "wh_move_delete",
            "description": "Xoá một phiếu kho (huỷ chứng từ ghi nhầm). Bị TỪ CHỐI nếu xoá xong tồn kho của mặt hàng nào đó bị âm (ví dụ xoá phiếu nhập khi hàng đã xuất đi rồi).",
            "inputSchema": { "type": "object", "properties": {
                "move_id": { "type": "number" }
            }, "required": ["move_id"] }
        },
        {
            "name": "wh_stock_onhand",
            "description": "Tồn kho hiện tại theo từng cặp (sản phẩm, kho) kèm giá vốn bình quân và giá trị. Lọc theo product_id / warehouse_id. Tổng giá trị trả về trong total_value.",
            "inputSchema": { "type": "object", "properties": {
                "product_id":   { "type": "number" },
                "warehouse_id": { "type": "number" }
            } }
        },
        {
            "name": "wh_stock_card",
            "description": "Thẻ kho của một sản phẩm: từng phiếu nhập/xuất/chuyển/điều chỉnh theo thời gian với số dư luỹ kế (balance). Có warehouse_id → thẻ kho của riêng kho đó; không có → toàn công ty (chuyển kho không đổi số dư).",
            "inputSchema": { "type": "object", "properties": {
                "product_id":   { "type": "number" },
                "warehouse_id": { "type": "number" },
                "limit":        { "type": "number", "description": "Số dòng cuối cùng, mặc định 200." }
            }, "required": ["product_id"] }
        },
        {
            "name": "wh_report_inout",
            "description": "Báo cáo nhập-xuất theo tháng: in_qty/in_value (nhập), out_qty/out_value (xuất), adjust_qty (điều chỉnh), net_qty. months mặc định 12.",
            "inputSchema": { "type": "object", "properties": {
                "months": { "type": "number" }
            } }
        },
        {
            "name": "wh_low_stock",
            "description": "Danh sách hàng dưới tồn tối thiểu (cần nhập thêm): on_hand hiện tại so với min_stock.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "wh_product_insight",
            "description": "Hiệu suất từng sản phẩm trong cửa sổ N ngày (mặc định 90) với phân loại tự động: potential (TIỀM NĂNG — đang bán tốt, tồn chỉ đủ ≤45 ngày, nên nhập thêm) | steady (ổn định) | slow (bán chậm — tồn đủ bán >180 ngày) | dead (TỒN ĐỌNG — có tồn mà không bán được đơn nào) | idle (chưa kinh doanh). Kèm sold_qty/sold_value, velocity_30d, days_of_stock, margin_pct, sell_through_pct, last_sale_date và summary (số lượng từng nhóm, giá trị vốn chôn trong hàng tồn đọng, top bán chạy). Dùng tool này khi được hỏi sản phẩm nào bán chạy/tiềm năng/ế.",
            "inputSchema": { "type": "object", "properties": {
                "days": { "type": "number", "description": "Cửa sổ phân tích theo ngày, 7–365, mặc định 90." }
            } }
        },
        {
            "name": "wh_analyze_products",
            "description": "AI đánh giá danh mục sản phẩm (qua bridge SenClaw) dựa trên số liệu wh_product_insight: sản phẩm TIỀM NĂNG nhất nên nhập thêm bao nhiêu, sản phẩm KHÔNG BÁN ĐƯỢC cần xả/ngừng nhập, hàng bán chậm cần theo dõi. Kết quả kèm cả JSON hiệu suất và luôn có lưu ý 'phân tích tham khảo'.",
            "inputSchema": { "type": "object", "properties": {
                "question": { "type": "string", "description": "Câu hỏi cụ thể, bỏ trống = đánh giá tổng quan danh mục." },
                "days":     { "type": "number", "description": "Cửa sổ phân tích theo ngày, mặc định 90." }
            } }
        },
        {
            "name": "wh_analyze",
            "description": "AI phân tích tồn kho (qua bridge SenClaw): hàng sắp hết, hàng tồn đọng, lệch nhập-xuất, khuyến nghị. Chỉ dựa trên số liệu trong app; kết quả luôn kèm lưu ý 'phân tích tham khảo'.",
            "inputSchema": { "type": "object", "properties": {
                "question": { "type": "string", "description": "Câu hỏi cụ thể, bỏ trống = phân tích tổng quan." }
            } }
        },
        {
            "name": "wh_activity",
            "description": "Nhật ký hành động gần đây của app (ai thêm sản phẩm/phiếu khi nào).",
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
    match name {
        "wh_status" => json_result(&api::status_value(s)),
        "wh_dashboard" => json_result(&api::dashboard_value(s)),
        "wh_product_add" => {
            let b = api::ProductIn {
                sku: str_arg("sku"),
                name: str_arg("name"),
                unit: str_arg("unit"),
                category: str_arg("category"),
                barcode: str_arg("barcode"),
                cost_price: f64_arg("cost_price").unwrap_or(0.0),
                sell_price: f64_arg("sell_price").unwrap_or(0.0),
                min_stock: f64_arg("min_stock").unwrap_or(0.0),
                note: str_arg("note"),
            };
            if b.name.is_empty() {
                return error_result("thiếu 'name'".into());
            }
            json_result(&api::add_product_value(s, &b))
        }
        "wh_product_update" => {
            let Some(id) = i64_arg("product_id") else {
                return error_result("thiếu 'product_id'".into());
            };
            json_result(&api::update_product_value(s, id, args))
        }
        "wh_product_list" => {
            let q = opt_str("q");
            let category = opt_str("category");
            let status = opt_str("status");
            json_result(&api::list_products_value(
                s,
                q.as_deref(),
                category.as_deref(),
                status.as_deref(),
                args.get("low_stock")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false),
            ))
        }
        "wh_product_get" => {
            let Some(id) = i64_arg("product_id") else {
                return error_result("thiếu 'product_id'".into());
            };
            json_result(&api::get_product_value(s, id))
        }
        "wh_warehouse_add" => {
            let b = api::WarehouseIn {
                name: str_arg("name"),
                location: str_arg("location"),
                note: str_arg("note"),
            };
            if b.name.is_empty() {
                return error_result("thiếu 'name'".into());
            }
            json_result(&api::add_warehouse_value(s, &b))
        }
        "wh_warehouse_list" => {
            let st = opt_str("status");
            json_result(&api::list_warehouses_value(s, st.as_deref()))
        }
        "wh_warehouse_update" => {
            let Some(id) = i64_arg("warehouse_id") else {
                return error_result("thiếu 'warehouse_id'".into());
            };
            json_result(&api::update_warehouse_value(s, id, args))
        }
        "wh_partner_add" => {
            let b = api::PartnerIn {
                name: str_arg("name"),
                kind: str_arg("kind"),
                phone: str_arg("phone"),
                address: str_arg("address"),
                note: str_arg("note"),
            };
            if b.name.is_empty() {
                return error_result("thiếu 'name'".into());
            }
            json_result(&api::add_partner_value(s, &b))
        }
        "wh_partner_list" => {
            let k = opt_str("kind");
            json_result(&api::list_partners_value(s, k.as_deref()))
        }
        "wh_move_create" => {
            let Some(warehouse_id) = i64_arg("warehouse_id") else {
                return error_result("thiếu 'warehouse_id'".into());
            };
            let kind = str_arg("kind");
            if kind.is_empty() {
                return error_result("thiếu 'kind'".into());
            }
            let Some(raw_lines) = args.get("lines").and_then(|x| x.as_array()) else {
                return error_result("thiếu 'lines'".into());
            };
            let mut lines = Vec::new();
            for l in raw_lines {
                let (Some(pid), Some(qty)) = (
                    l.get("product_id").and_then(|x| x.as_i64()),
                    l.get("qty").and_then(|x| x.as_f64()),
                ) else {
                    return error_result("mỗi dòng cần 'product_id' và 'qty'".into());
                };
                lines.push(api::MoveLineIn {
                    product_id: pid,
                    qty,
                    unit_price: l.get("unit_price").and_then(|x| x.as_f64()).unwrap_or(0.0),
                });
            }
            let b = api::MoveIn {
                kind,
                warehouse_id,
                to_warehouse_id: i64_arg("to_warehouse_id"),
                partner_id: i64_arg("partner_id"),
                move_date: str_arg("move_date"),
                note: str_arg("note"),
                lines,
            };
            json_result(&api::create_move_value(s, &b))
        }
        "wh_move_list" => {
            let kind = opt_str("kind");
            let date_from = opt_str("date_from");
            let date_to = opt_str("date_to");
            json_result(&api::list_moves_value(
                s,
                kind.as_deref(),
                i64_arg("warehouse_id"),
                i64_arg("product_id"),
                date_from.as_deref(),
                date_to.as_deref(),
                i64_arg("limit").unwrap_or(100),
            ))
        }
        "wh_move_get" => {
            let Some(id) = i64_arg("move_id") else {
                return error_result("thiếu 'move_id'".into());
            };
            json_result(&api::get_move_value(s, id))
        }
        "wh_move_delete" => {
            let Some(id) = i64_arg("move_id") else {
                return error_result("thiếu 'move_id'".into());
            };
            json_result(&api::delete_move_value(s, id))
        }
        "wh_stock_onhand" => json_result(&api::stock_onhand_value(
            s,
            i64_arg("product_id"),
            i64_arg("warehouse_id"),
        )),
        "wh_stock_card" => {
            let Some(pid) = i64_arg("product_id") else {
                return error_result("thiếu 'product_id'".into());
            };
            json_result(&api::stock_card_value(
                s,
                pid,
                i64_arg("warehouse_id"),
                i64_arg("limit").unwrap_or(200),
            ))
        }
        "wh_report_inout" => {
            json_result(&api::report_inout_value(s, i64_arg("months").unwrap_or(12)))
        }
        "wh_product_insight" => json_result(&api::product_insight_value(
            s,
            i64_arg("days").unwrap_or(90),
        )),
        "wh_analyze_products" => {
            let q = str_arg("question");
            json_result(&api::analyze_products_value(s, &q, i64_arg("days").unwrap_or(90)).await)
        }
        "wh_low_stock" => json_result(&api::list_products_value(
            s,
            None,
            None,
            Some("active"),
            true,
        )),
        "wh_analyze" => {
            let q = str_arg("question");
            json_result(&api::analyze_value(s, &q).await)
        }
        "wh_activity" => json_result(&json!({ "activity": s.db.recent_activity(50) })),
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
        assert_eq!(names.len(), 23);
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "tool names must be unique");
        assert!(
            names.iter().all(|n| n.starts_with("wh_")),
            "all tools use the wh_ prefix"
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
    fn move_create_schema_declares_lines_array() {
        let tools = tools_list();
        let mv = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "wh_move_create")
            .unwrap();
        assert_eq!(mv["inputSchema"]["properties"]["lines"]["type"], "array");
        let req = mv["inputSchema"]["required"].as_array().unwrap();
        assert!(req.iter().any(|r| r == "lines"));
    }
}
