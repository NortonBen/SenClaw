//! MCP server `cafe-mcp` — HTTP JSON-RPC + SSE, cùng khuôn với các Space App
//! khác. Tool nào cũng gọi lại `api::*_value` để REST và MCP không lệch nhau.

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
            "serverInfo": { "name": "cafe-mcp", "version": "1.0.0" }
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
            "name": "cafe_status",
            "description": "Trạng thái nhanh của quán: số món, số nguyên liệu, đơn và doanh thu hôm nay, số nguyên liệu dưới tồn tối thiểu, giá trị tồn kho.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "cafe_dashboard",
            "description": "Toàn cảnh quán cafe: doanh thu / đơn / lãi gộp hôm nay và 7 ngày, biểu đồ doanh thu 14 ngày, top món, nguyên liệu sắp hết, kho âm, món chưa có công thức, đơn gần đây, cảnh báo. Dùng tool này TRƯỚC khi phân tích hay trả lời câu hỏi tổng quan.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "cafe_ingredient_add",
            "description": "Thêm nguyên liệu vào kho. unit là ĐƠN VỊ GỐC (g | ml | cái) — tồn kho và công thức luôn tính theo đơn vị này; nhập hàng có thể khai kg/lít sẽ tự quy đổi. min_stock = ngưỡng cảnh báo sắp hết (theo đơn vị gốc). Giá vốn sẽ tự tính bình quân gia quyền từ phiếu nhập.",
            "inputSchema": { "type": "object", "properties": {
                "name":      { "type": "string" },
                "unit":      { "type": "string", "enum": ["g", "ml", "cái"], "description": "Đơn vị gốc của nguyên liệu." },
                "min_stock": { "type": "number", "description": "Cảnh báo khi tồn < mức này (0 = không cảnh báo)." },
                "note":      { "type": "string" }
            }, "required": ["name", "unit"] }
        },
        {
            "name": "cafe_ingredient_update",
            "description": "Sửa nguyên liệu: name, min_stock, note, active (false = ngừng dùng). unit chỉ đổi được khi nguyên liệu CHƯA có biến động kho.",
            "inputSchema": { "type": "object", "properties": {
                "ingredient_id": { "type": "number" },
                "name":          { "type": "string" },
                "unit":          { "type": "string", "enum": ["g", "ml", "cái"] },
                "min_stock":     { "type": "number" },
                "note":          { "type": "string" },
                "active":        { "type": "boolean" }
            }, "required": ["ingredient_id"] }
        },
        {
            "name": "cafe_ingredient_list",
            "description": "Liệt kê nguyên liệu kèm số liệu suy ra: stock (tồn theo đơn vị gốc), stock_display (tự quy kg/lít), avg_cost (giá vốn bình quân/đơn vị gốc), stock_value, low_stock, avg_daily_14d (tiêu hao trung bình/ngày 14 ngày), days_left. Lọc: q (tìm không dấu), low_only=true (chỉ hàng sắp hết), include_inactive.",
            "inputSchema": { "type": "object", "properties": {
                "q":                { "type": "string" },
                "low_only":         { "type": "boolean", "description": "true = chỉ nguyên liệu dưới tồn tối thiểu." },
                "include_inactive": { "type": "boolean" }
            } }
        },
        {
            "name": "cafe_stock_adjust",
            "description": "Điều chỉnh kiểm kê MỘT nguyên liệu: delta (chênh lệch có dấu — đếm thừa dương, thiếu âm) HOẶC set_qty (đặt thẳng số đếm thực tế, theo đơn vị gốc). Luôn kèm reason (đợt kiểm kê, rơi vãi, hết hạn...).",
            "inputSchema": { "type": "object", "properties": {
                "ingredient_id": { "type": "number" },
                "delta":         { "type": "number", "description": "Chênh lệch có dấu theo đơn vị gốc." },
                "set_qty":       { "type": "number", "description": "Đặt tồn = số đếm thực tế (thay cho delta)." },
                "reason":        { "type": "string" }
            }, "required": ["ingredient_id"] }
        },
        {
            "name": "cafe_stock_card",
            "description": "Thẻ kho một nguyên liệu: từng biến động (nhập / bán / điều chỉnh / hoàn kho khi huỷ đơn) với số dư luỹ kế, số dư đầu-cuối kỳ. Lọc from/to (YYYY-MM-DD), limit dòng cuối.",
            "inputSchema": { "type": "object", "properties": {
                "ingredient_id": { "type": "number" },
                "from":          { "type": "string", "description": "YYYY-MM-DD" },
                "to":            { "type": "string", "description": "YYYY-MM-DD" },
                "limit":         { "type": "number" }
            }, "required": ["ingredient_id"] }
        },
        {
            "name": "cafe_purchase_create",
            "description": "Tạo phiếu nhập hàng nhiều dòng (mã NH- tự sinh). Mỗi dòng: ingredient_id; qty theo unit khai (g | kg | ml | l | lít | cái — kg/lít tự quy đổi về đơn vị gốc); unit_price là giá cho MỘT unit đó (nhập 5 kg giá 90000 đ/kg → qty=5, unit=\"kg\", unit_price=90000). Giá vốn bình quân gia quyền của nguyên liệu tự cập nhật.",
            "inputSchema": { "type": "object", "properties": {
                "supplier": { "type": "string", "description": "Nhà cung cấp (tuỳ chọn)." },
                "date":     { "type": "string", "description": "YYYY-MM-DD, bỏ trống = hôm nay." },
                "note":     { "type": "string" },
                "lines":    { "type": "array", "description": "Các dòng nhập của phiếu.", "items": {
                    "type": "object", "properties": {
                        "ingredient_id": { "type": "number" },
                        "qty":           { "type": "number", "description": "> 0, theo unit khai." },
                        "unit":          { "type": "string", "enum": ["g", "kg", "ml", "l", "lít", "cái"] },
                        "unit_price":    { "type": "number", "description": "Giá cho MỘT unit khai, mặc định 0." }
                    }, "required": ["ingredient_id", "qty", "unit"]
                } }
            }, "required": ["lines"] }
        },
        {
            "name": "cafe_purchase_list",
            "description": "Liệt kê phiếu nhập hàng (mã, nhà cung cấp, ngày, tổng tiền, số dòng). Lọc from/to (YYYY-MM-DD), supplier (chứa chuỗi), limit.",
            "inputSchema": { "type": "object", "properties": {
                "from":     { "type": "string" },
                "to":       { "type": "string" },
                "supplier": { "type": "string" },
                "limit":    { "type": "number" }
            } }
        },
        {
            "name": "cafe_purchase_get",
            "description": "Xem chi tiết một phiếu nhập hàng: đầy đủ các dòng nguyên liệu, số lượng đã quy đổi về đơn vị gốc và thành tiền.",
            "inputSchema": { "type": "object", "properties": {
                "purchase_id": { "type": "number" }
            }, "required": ["purchase_id"] }
        },
        {
            "name": "cafe_report_purchases",
            "description": "Báo cáo nhập hàng trong khoảng from..to, nhóm theo group_by: supplier (nhà cung cấp) | ingredient (nguyên liệu, mặc định) | day (ngày). Kèm tổng số phiếu và tổng tiền nhập.",
            "inputSchema": { "type": "object", "properties": {
                "from":     { "type": "string", "description": "YYYY-MM-DD, bỏ trống = từ đầu." },
                "to":       { "type": "string", "description": "YYYY-MM-DD, bỏ trống = đến nay." },
                "group_by": { "type": "string", "enum": ["supplier", "ingredient", "day"] }
            } }
        },
        {
            "name": "cafe_purchase_suggest",
            "description": "Đề xuất nhập hàng cho N ngày tới (mặc định 7): cần nhập = tiêu hao dự kiến (dự báo lượng bán × công thức) + tồn tối thiểu − tồn hiện tại, kèm chi phí ước tính theo giá vốn bình quân. Dùng khi được hỏi \"tuần tới cần mua gì\".",
            "inputSchema": { "type": "object", "properties": {
                "days": { "type": "number", "description": "Số ngày cần trù bị, 1-30, mặc định 7." }
            } }
        },
        {
            "name": "cafe_menu_add",
            "description": "Thêm món vào thực đơn: name, category (nhóm: Cà phê / Trà / Sinh tố...), price (giá bán VND), instructions (cách pha chế). Sau khi thêm nên đặt công thức bằng cafe_recipe_set để tính được giá vốn và trừ kho khi bán.",
            "inputSchema": { "type": "object", "properties": {
                "name":         { "type": "string" },
                "category":     { "type": "string" },
                "price":        { "type": "number", "description": "Giá bán (VND)." },
                "instructions": { "type": "string", "description": "Cách pha chế / làm món." }
            }, "required": ["name", "price"] }
        },
        {
            "name": "cafe_menu_update",
            "description": "Sửa món: name, category, price, instructions, active (false = ngừng bán — món ngừng bán không lên đơn được).",
            "inputSchema": { "type": "object", "properties": {
                "menu_id":      { "type": "number" },
                "name":         { "type": "string" },
                "category":     { "type": "string" },
                "price":        { "type": "number" },
                "instructions": { "type": "string" },
                "active":       { "type": "boolean" }
            }, "required": ["menu_id"] }
        },
        {
            "name": "cafe_menu_list",
            "description": "Liệt kê thực đơn kèm giá vốn hiện tại (từ công thức × giá vốn nguyên liệu), lãi gộp, margin_pct và cờ has_recipe. Lọc: q (tìm không dấu), category, include_inactive.",
            "inputSchema": { "type": "object", "properties": {
                "q":                { "type": "string" },
                "category":         { "type": "string" },
                "include_inactive": { "type": "boolean" }
            } }
        },
        {
            "name": "cafe_menu_get",
            "description": "Xem chi tiết một món: giá bán, cách pha chế, công thức từng dòng (nguyên liệu, định lượng theo đơn vị gốc, giá vốn dòng), tổng giá vốn, lãi gộp, margin.",
            "inputSchema": { "type": "object", "properties": {
                "menu_id": { "type": "number" }
            }, "required": ["menu_id"] }
        },
        {
            "name": "cafe_recipe_set",
            "description": "Đặt CÔNG THỨC cho món — THAY THẾ toàn bộ công thức cũ. items: mỗi dòng {ingredient_id, qty} với qty theo ĐƠN VỊ GỐC của nguyên liệu (vd 25 g cafe, 30 ml sữa đặc, 1 cái ly). items rỗng = xoá công thức. Không lặp nguyên liệu — gộp định lượng vào một dòng.",
            "inputSchema": { "type": "object", "properties": {
                "menu_id": { "type": "number" },
                "items":   { "type": "array", "description": "Toàn bộ công thức mới.", "items": {
                    "type": "object", "properties": {
                        "ingredient_id": { "type": "number" },
                        "qty":           { "type": "number", "description": "> 0, theo đơn vị gốc của nguyên liệu." }
                    }, "required": ["ingredient_id", "qty"]
                } }
            }, "required": ["menu_id", "items"] }
        },
        {
            "name": "cafe_sale_create",
            "description": "Ghi đơn bán nhiều dòng (mã BH- tự sinh). Mỗi dòng: menu_id, qty, unit_price (bỏ trống = giá thực đơn). Kho nguyên liệu TỰ TRỪ theo công thức từng món và giá vốn được chốt tại thời điểm bán. Kết quả có thể kèm warnings (món chưa có công thức, nguyên liệu bị âm kho) — PHẢI nhắc lại các cảnh báo đó.",
            "inputSchema": { "type": "object", "properties": {
                "date":  { "type": "string", "description": "YYYY-MM-DD, bỏ trống = hôm nay." },
                "note":  { "type": "string" },
                "lines": { "type": "array", "description": "Các dòng món của đơn.", "items": {
                    "type": "object", "properties": {
                        "menu_id":    { "type": "number" },
                        "qty":        { "type": "number", "description": "Số ly/phần, > 0." },
                        "unit_price": { "type": "number", "description": "Giá bán ghi đè, bỏ trống = giá thực đơn." }
                    }, "required": ["menu_id", "qty"]
                } }
            }, "required": ["lines"] }
        },
        {
            "name": "cafe_sale_list",
            "description": "Liệt kê đơn bán (mã, ngày, món trong đơn, tổng tiền, giá vốn, lãi, trạng thái done/void). Lọc from/to (YYYY-MM-DD), status, limit.",
            "inputSchema": { "type": "object", "properties": {
                "from":   { "type": "string" },
                "to":     { "type": "string" },
                "status": { "type": "string", "enum": ["done", "void"] },
                "limit":  { "type": "number" }
            } }
        },
        {
            "name": "cafe_sale_get",
            "description": "Xem chi tiết một đơn bán: các dòng món, đơn giá, thành tiền, giá vốn từng dòng và tổng lãi gộp của đơn.",
            "inputSchema": { "type": "object", "properties": {
                "sale_id": { "type": "number" }
            }, "required": ["sale_id"] }
        },
        {
            "name": "cafe_sale_void",
            "description": "Huỷ một đơn bán ghi nhầm: hoàn toàn bộ nguyên liệu về kho (ghi move hoàn kho) và loại đơn khỏi mọi báo cáo doanh thu. Không xoá dữ liệu — đơn vẫn tra cứu được với trạng thái void.",
            "inputSchema": { "type": "object", "properties": {
                "sale_id": { "type": "number" },
                "reason":  { "type": "string" }
            }, "required": ["sale_id"] }
        },
        {
            "name": "cafe_report_revenue",
            "description": "Báo cáo doanh thu – giá vốn – lãi gộp trong khoảng from..to, nhóm theo group_by: day (theo ngày, mặc định) | item (từng món) | category (nhóm món). Kèm tổng đơn, tổng ly bán, tổng doanh thu / giá vốn / lãi.",
            "inputSchema": { "type": "object", "properties": {
                "from":     { "type": "string", "description": "YYYY-MM-DD, bỏ trống = từ đầu." },
                "to":       { "type": "string", "description": "YYYY-MM-DD, bỏ trống = đến nay." },
                "group_by": { "type": "string", "enum": ["day", "item", "category"] }
            } }
        },
        {
            "name": "cafe_report_inventory",
            "description": "Báo cáo tồn kho nguyên liệu hiện tại: tồn + giá trị từng nguyên liệu (tồn × giá vốn bình quân), tổng giá trị kho, danh sách sắp hết và danh sách kho âm cần kiểm kê.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "cafe_forecast_sales",
            "description": "Dự đoán N ngày tới (mặc định 7, tối đa 30): lượng bán từng món và doanh thu / lãi gộp từng ngày, theo trung bình cùng thứ trong 4 tuần gần nhất (lịch sử 28 ngày). Chỉ là ước tính từ dữ liệu bán cũ.",
            "inputSchema": { "type": "object", "properties": {
                "days": { "type": "number", "description": "1-30, mặc định 7." }
            } }
        },
        {
            "name": "cafe_forecast_ingredients",
            "description": "Dự báo tiêu hao nguyên liệu N ngày tới (dự báo lượng bán × công thức hiện tại): tổng tiêu hao dự kiến, số ngày còn cầm cự với tồn hiện tại (days_left), ngày dự kiến hết (stockout_date) và lượng cần nhập.",
            "inputSchema": { "type": "object", "properties": {
                "days": { "type": "number", "description": "1-30, mặc định 7." }
            } }
        },
        {
            "name": "cafe_ai_analyze",
            "description": "AI phân tích kinh doanh của quán qua bridge SenClaw: doanh thu, món lãi tốt / kém, nguyên liệu cần nhập, bất thường — dựa TRÊN số liệu dashboard + doanh thu 30 ngày + dự báo 7 ngày. Kết quả luôn kèm dòng lưu ý \"phân tích tham khảo…\" — giữ nguyên dòng đó khi trả lời.",
            "inputSchema": { "type": "object", "properties": {
                "question": { "type": "string", "description": "Câu hỏi cụ thể, bỏ trống = phân tích tổng quan." }
            } }
        },
        {
            "name": "cafe_ai_menu_suggest",
            "description": "AI gợi ý công thức đồ uống mới từ nguyên liệu ĐANG CÓ trong kho (định lượng g/ml, cách pha, giá vốn ước tính, giá bán gợi ý theo biên lãi mục tiêu). Chỉ là gợi ý — người dùng chốt rồi mới thêm vào thực đơn bằng cafe_menu_add + cafe_recipe_set.",
            "inputSchema": { "type": "object", "properties": {
                "idea":              { "type": "string", "description": "Ý tưởng món / yêu cầu, bỏ trống = gợi ý từ nguyên liệu sẵn có." },
                "target_margin_pct": { "type": "number", "description": "Biên lãi gộp mục tiêu %, mặc định 70." }
            } }
        }
    ])
}

async fn call_tool(s: &AppState, name: &str, args: &Value) -> Value {
    let f64_arg = |k: &str| args.get(k).and_then(|x| x.as_f64());
    let i64_arg = |k: &str| args.get(k).and_then(|x| x.as_i64());
    let bool_arg = |k: &str| args.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    let str_arg = |k: &str| {
        args.get(k)
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()
    };
    let opt_str = |k: &str| args.get(k).and_then(|x| x.as_str()).map(|v| v.to_string());
    match name {
        "cafe_status" => json_result(&api::status_value(s)),
        "cafe_dashboard" => json_result(&api::dashboard_value(s)),
        "cafe_ingredient_add" => {
            let b = api::IngredientIn {
                name: str_arg("name"),
                unit: str_arg("unit"),
                min_stock: f64_arg("min_stock").unwrap_or(0.0),
                note: str_arg("note"),
            };
            if b.name.is_empty() {
                return error_result("thiếu 'name'".into());
            }
            json_result(&api::add_ingredient_value(s, &b))
        }
        "cafe_ingredient_update" => {
            let Some(id) = i64_arg("ingredient_id") else {
                return error_result("thiếu 'ingredient_id'".into());
            };
            json_result(&api::update_ingredient_value(s, id, args))
        }
        "cafe_ingredient_list" => {
            let q = opt_str("q");
            json_result(&api::list_ingredients_value(
                s,
                q.as_deref(),
                bool_arg("low_only"),
                bool_arg("include_inactive"),
            ))
        }
        "cafe_stock_adjust" => {
            let Some(id) = i64_arg("ingredient_id") else {
                return error_result("thiếu 'ingredient_id'".into());
            };
            let b = api::AdjustIn {
                ingredient_id: id,
                delta: f64_arg("delta"),
                set_qty: f64_arg("set_qty"),
                reason: str_arg("reason"),
            };
            json_result(&api::adjust_stock_value(s, &b))
        }
        "cafe_stock_card" => {
            let Some(id) = i64_arg("ingredient_id") else {
                return error_result("thiếu 'ingredient_id'".into());
            };
            let from = opt_str("from");
            let to = opt_str("to");
            json_result(&api::stock_card_value(
                s,
                id,
                from.as_deref(),
                to.as_deref(),
                i64_arg("limit").unwrap_or(200),
            ))
        }
        "cafe_purchase_create" => {
            let Some(raw_lines) = args.get("lines").and_then(|x| x.as_array()) else {
                return error_result("thiếu 'lines'".into());
            };
            let mut lines = Vec::new();
            for l in raw_lines {
                let (Some(iid), Some(qty), Some(unit)) = (
                    l.get("ingredient_id").and_then(|x| x.as_i64()),
                    l.get("qty").and_then(|x| x.as_f64()),
                    l.get("unit").and_then(|x| x.as_str()),
                ) else {
                    return error_result(
                        "mỗi dòng cần 'ingredient_id', 'qty' và 'unit'".into(),
                    );
                };
                lines.push(api::PLineIn {
                    ingredient_id: iid,
                    qty,
                    unit: unit.to_string(),
                    unit_price: l.get("unit_price").and_then(|x| x.as_f64()).unwrap_or(0.0),
                });
            }
            let b = api::PurchaseIn {
                supplier: str_arg("supplier"),
                date: str_arg("date"),
                note: str_arg("note"),
                lines,
            };
            json_result(&api::create_purchase_value(s, &b))
        }
        "cafe_purchase_list" => {
            let from = opt_str("from");
            let to = opt_str("to");
            let supplier = opt_str("supplier");
            json_result(&api::list_purchases_value(
                s,
                from.as_deref(),
                to.as_deref(),
                supplier.as_deref(),
                i64_arg("limit").unwrap_or(100),
            ))
        }
        "cafe_purchase_get" => {
            let Some(id) = i64_arg("purchase_id") else {
                return error_result("thiếu 'purchase_id'".into());
            };
            json_result(&api::get_purchase_value(s, id))
        }
        "cafe_report_purchases" => json_result(&api::report_purchases_value(
            s,
            &str_arg("from"),
            &str_arg("to"),
            &{
                let g = str_arg("group_by");
                if g.is_empty() { "ingredient".to_string() } else { g }
            },
        )),
        "cafe_purchase_suggest" => {
            json_result(&api::purchase_suggest_value(s, i64_arg("days").unwrap_or(7)))
        }
        "cafe_menu_add" => {
            let b = api::MenuIn {
                name: str_arg("name"),
                category: str_arg("category"),
                price: f64_arg("price").unwrap_or(0.0),
                instructions: str_arg("instructions"),
            };
            if b.name.is_empty() {
                return error_result("thiếu 'name'".into());
            }
            json_result(&api::add_menu_value(s, &b))
        }
        "cafe_menu_update" => {
            let Some(id) = i64_arg("menu_id") else {
                return error_result("thiếu 'menu_id'".into());
            };
            json_result(&api::update_menu_value(s, id, args))
        }
        "cafe_menu_list" => {
            let q = opt_str("q");
            let category = opt_str("category");
            json_result(&api::list_menu_value(
                s,
                q.as_deref(),
                category.as_deref(),
                bool_arg("include_inactive"),
            ))
        }
        "cafe_menu_get" => {
            let Some(id) = i64_arg("menu_id") else {
                return error_result("thiếu 'menu_id'".into());
            };
            json_result(&api::get_menu_value(s, id))
        }
        "cafe_recipe_set" => {
            let Some(id) = i64_arg("menu_id") else {
                return error_result("thiếu 'menu_id'".into());
            };
            let Some(raw) = args.get("items").and_then(|x| x.as_array()) else {
                return error_result("thiếu 'items' (mảng rỗng = xoá công thức)".into());
            };
            let mut items = Vec::new();
            for it in raw {
                let (Some(iid), Some(qty)) = (
                    it.get("ingredient_id").and_then(|x| x.as_i64()),
                    it.get("qty").and_then(|x| x.as_f64()),
                ) else {
                    return error_result("mỗi dòng cần 'ingredient_id' và 'qty'".into());
                };
                items.push(api::RItemIn {
                    ingredient_id: iid,
                    qty,
                });
            }
            json_result(&api::set_recipe_value(s, id, &items))
        }
        "cafe_sale_create" => {
            let Some(raw_lines) = args.get("lines").and_then(|x| x.as_array()) else {
                return error_result("thiếu 'lines'".into());
            };
            let mut lines = Vec::new();
            for l in raw_lines {
                let (Some(mid), Some(qty)) = (
                    l.get("menu_id").and_then(|x| x.as_i64()),
                    l.get("qty").and_then(|x| x.as_f64()),
                ) else {
                    return error_result("mỗi dòng cần 'menu_id' và 'qty'".into());
                };
                lines.push(api::SLineIn {
                    menu_id: mid,
                    qty,
                    unit_price: l.get("unit_price").and_then(|x| x.as_f64()),
                });
            }
            let b = api::SaleIn {
                date: str_arg("date"),
                note: str_arg("note"),
                lines,
            };
            json_result(&api::create_sale_value(s, &b))
        }
        "cafe_sale_list" => {
            let from = opt_str("from");
            let to = opt_str("to");
            let status = opt_str("status");
            json_result(&api::list_sales_value(
                s,
                from.as_deref(),
                to.as_deref(),
                status.as_deref(),
                i64_arg("limit").unwrap_or(100),
            ))
        }
        "cafe_sale_get" => {
            let Some(id) = i64_arg("sale_id") else {
                return error_result("thiếu 'sale_id'".into());
            };
            json_result(&api::get_sale_value(s, id))
        }
        "cafe_sale_void" => {
            let Some(id) = i64_arg("sale_id") else {
                return error_result("thiếu 'sale_id'".into());
            };
            json_result(&api::void_sale_value(s, id, &str_arg("reason")))
        }
        "cafe_report_revenue" => json_result(&api::report_revenue_value(
            s,
            &str_arg("from"),
            &str_arg("to"),
            &{
                let g = str_arg("group_by");
                if g.is_empty() { "day".to_string() } else { g }
            },
        )),
        "cafe_report_inventory" => json_result(&api::report_inventory_value(s)),
        "cafe_forecast_sales" => {
            json_result(&api::forecast_sales_value(s, i64_arg("days").unwrap_or(7)))
        }
        "cafe_forecast_ingredients" => json_result(&api::forecast_ingredients_value(
            s,
            i64_arg("days").unwrap_or(7),
        )),
        "cafe_ai_analyze" => {
            let q = str_arg("question");
            json_result(&api::analyze_value(s, &q).await)
        }
        "cafe_ai_menu_suggest" => {
            let idea = str_arg("idea");
            json_result(&api::menu_suggest_value(s, &idea, f64_arg("target_margin_pct")).await)
        }
        other => error_result(format!("tool không tồn tại: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_have_unique_prefixed_names() {
        let tools = tools_list();
        let arr = tools.as_array().unwrap();
        assert_eq!(arr.len(), 27);
        let mut seen = std::collections::BTreeSet::new();
        for t in arr {
            let name = t["name"].as_str().unwrap();
            assert!(name.starts_with("cafe_"), "tool {name} thiếu tiền tố cafe_");
            assert!(seen.insert(name.to_string()), "tool {name} bị trùng");
        }
    }

    #[test]
    fn every_tool_has_schema_and_description() {
        for t in tools_list().as_array().unwrap() {
            let name = t["name"].as_str().unwrap();
            let desc = t["description"].as_str().unwrap();
            assert!(desc.len() > 20, "mô tả tool {name} quá ngắn");
            assert_eq!(t["inputSchema"]["type"], "object", "tool {name} thiếu inputSchema object");
        }
    }

    #[test]
    fn line_schemas_declare_required_fields() {
        let tools = tools_list();
        let arr = tools.as_array().unwrap();
        let find = |n: &str| arr.iter().find(|t| t["name"] == n).unwrap();
        let purchase = find("cafe_purchase_create");
        let req = purchase["inputSchema"]["properties"]["lines"]["items"]["required"]
            .as_array()
            .unwrap();
        assert!(req.contains(&json!("ingredient_id")) && req.contains(&json!("unit")));
        let sale = find("cafe_sale_create");
        let req = sale["inputSchema"]["properties"]["lines"]["items"]["required"]
            .as_array()
            .unwrap();
        assert!(req.contains(&json!("menu_id")) && req.contains(&json!("qty")));
        let recipe = find("cafe_recipe_set");
        assert_eq!(recipe["inputSchema"]["properties"]["items"]["type"], "array");
    }
}
