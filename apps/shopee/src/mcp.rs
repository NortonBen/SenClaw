//! MCP server (HTTP + SSE) exposing the Shopee shop's read + reply operations to
//! SenClaw agents. Every write goes through the SAME draft-approve gate the UI
//! uses ([`crate::api::enqueue_or_send`] / [`crate::api::send_draft`]) so an
//! agent can never bypass the human-approval default: in `draft` mode a reply
//! becomes a queued draft, and only `shopee_approve_draft` (or `live` mode)
//! actually calls Shopee `send_message`. There is no bulk/broadcast tool.

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
            "serverInfo": { "name": "shopee-mcp", "version": "1.0.0" }
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
            "name": "shopee_status",
            "description": "Trạng thái kết nối Shopee: đã cấu hình partner chưa, đã authorize shop chưa, chế độ autonomy (observe/draft/live), số bản nháp đang chờ duyệt.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "shopee_oauth_link",
            "description": "Sinh link authorize (sống 5 phút) để seller tự bấm đồng ý cấp quyền cho partner app. Trả về URL; con người phải tự mở và đồng ý. KHÔNG tự động hoá bước đồng ý này.",
            "inputSchema": { "type": "object", "properties": {
                "redirect": { "type": "string", "description": "URL callback Shopee sẽ redirect về (kèm ?code&shop_id)." }
            }, "required": ["redirect"] }
        },
        {
            "name": "shopee_shop_info",
            "description": "Thông tin cơ bản của shop đã kết nối (cách rẻ để xác nhận token còn hiệu lực).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "shopee_orders",
            "description": "Danh sách đơn hàng 14 ngày gần nhất của shop (Order API chính thức).",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "shopee_conversations",
            "description": "Danh sách hội thoại buyer↔seller của shop (Chat API). Dùng để xem khách đang nhắn gì trước khi soạn trả lời.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "shopee_draft_reply",
            "description": "SOẠN một câu trả lời khách và đưa vào hàng chờ duyệt (draft-first). KHÔNG gửi ngay (trừ khi autonomy=live). Nếu bỏ trống 'content' thì LLM tự soạn từ 'customer_msg'. Nếu truyền 'order_sn' thì tra đơn thật (trạng thái/sản phẩm/vận đơn) để trả lời đúng số liệu. Chỉ trả lời khách của shop này — không có gửi hàng loạt.",
            "inputSchema": { "type": "object", "properties": {
                "conversation_id": { "type": "string" },
                "to_id":           { "type": "number", "description": "id người nhận (khách)." },
                "to_name":         { "type": "string" },
                "content":         { "type": "string", "description": "Nội dung tự viết. Bỏ trống để LLM soạn." },
                "customer_msg":    { "type": "string", "description": "Tin của khách, để LLM soạn dựa vào." },
                "context":         { "type": "string", "description": "Bối cảnh thêm (chính sách...)." },
                "order_sn":        { "type": "string", "description": "order_sn để ground câu trả lời vào đơn thật." }
            }, "required": ["conversation_id", "to_id"] }
        },
        {
            "name": "shopee_order_detail",
            "description": "Chi tiết một hay nhiều đơn theo order_sn (trạng thái, tổng tiền, sản phẩm, mã vận đơn). Dùng để trả lời 'đơn của tôi tới đâu rồi'.",
            "inputSchema": { "type": "object", "properties": {
                "order_sn": { "type": "array", "items": { "type": "string" } }
            }, "required": ["order_sn"] }
        },
        {
            "name": "shopee_list_drafts",
            "description": "Liệt kê các bản nháp trả lời đang chờ duyệt.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "shopee_approve_draft",
            "description": "DUYỆT và GỬI một bản nháp cho khách — đây là cổng DUY NHẤT thực sự gọi Shopee send_message. Chỉ dùng khi con người đã đồng ý nội dung.",
            "inputSchema": { "type": "object", "properties": {
                "draft_id": { "type": "number" }
            }, "required": ["draft_id"] }
        },
        {
            "name": "shopee_reject_draft",
            "description": "Bỏ một bản nháp mà không gửi.",
            "inputSchema": { "type": "object", "properties": {
                "draft_id": { "type": "number" }
            }, "required": ["draft_id"] }
        },
        {
            "name": "shopee_tick",
            "description": "Chạy một nhịp heartbeat ngay: đọc hội thoại chưa đọc và SOẠN nháp trả lời (không gửi trừ live). Tôn trọng autonomy gate.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "shopee_products",
            "description": "Danh sách sản phẩm của shop (Product API). status: NORMAL/BANNED/UNLIST/REVIEWING (mặc định NORMAL).",
            "inputSchema": { "type": "object", "properties": {
                "status": { "type": "string" }
            } }
        },
        {
            "name": "shopee_product_info",
            "description": "Thông tin chi tiết của các sản phẩm theo item_id (tối đa 50).",
            "inputSchema": { "type": "object", "properties": {
                "item_ids": { "type": "array", "items": { "type": "number" } }
            }, "required": ["item_ids"] }
        },
        {
            "name": "shopee_update_stock",
            "description": "Cập nhật TỒN KHO một sản phẩm của shop bạn (variant đơn). Thao tác ghi lên shop của chính bạn — không tự động hoá, chỉ chạy khi được yêu cầu rõ ràng.",
            "inputSchema": { "type": "object", "properties": {
                "item_id": { "type": "number" },
                "stock":   { "type": "number" }
            }, "required": ["item_id", "stock"] }
        },
        {
            "name": "shopee_update_price",
            "description": "Cập nhật GIÁ một sản phẩm của shop bạn (variant đơn). Thao tác ghi — chỉ chạy khi được yêu cầu rõ ràng.",
            "inputSchema": { "type": "object", "properties": {
                "item_id": { "type": "number" },
                "price":   { "type": "number" }
            }, "required": ["item_id", "price"] }
        }
    ])
}

async fn call_tool(s: &AppState, name: &str, args: &Value) -> Value {
    match name {
        "shopee_status" => json_result(&api::status_value(s)),
        "shopee_oauth_link" => {
            let Some(redirect) = args.get("redirect").and_then(|x| x.as_str()) else {
                return error_result("thiếu 'redirect'".into());
            };
            match api::client_from_settings(&s.db) {
                Some(client) => json_result(&json!({ "url": client.authorize_link(redirect) })),
                None => error_result("chưa cấu hình partner_id/partner_key".into()),
            }
        }
        "shopee_shop_info" => {
            let (Some(client), Some(sid)) = (api::client_from_settings(&s.db), api::shop_id(&s.db))
            else {
                return error_result("chưa kết nối shop".into());
            };
            match api::fresh_token(&s.db, &client, sid).await {
                Ok(tok) => match client.get_shop_info(&tok).await {
                    Ok(v) => json_result(&v),
                    Err(e) => error_result(e.to_string()),
                },
                Err(e) => error_result(e),
            }
        }
        "shopee_orders" => json_result(&api::orders_value(s).await),
        "shopee_conversations" => json_result(&api::conversations_value(s).await),
        "shopee_draft_reply" => {
            let conversation_id = args
                .get("conversation_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let to_id = args.get("to_id").and_then(|x| x.as_i64()).unwrap_or(0);
            if conversation_id.is_empty() || to_id == 0 {
                return error_result("cần 'conversation_id' và 'to_id'".into());
            }
            let to_name = args
                .get("to_name")
                .and_then(|x| x.as_str())
                .unwrap_or("khách")
                .to_string();
            let content = args
                .get("content")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string());
            let customer_msg = args
                .get("customer_msg")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let extra = args.get("context").and_then(|x| x.as_str());
            let order_sn = args.get("order_sn").and_then(|x| x.as_str());
            let context = api::grounded_context(s, order_sn, extra).await;
            let res = api::enqueue_or_send(
                s,
                &conversation_id,
                to_id,
                &to_name,
                content,
                &customer_msg,
                &context,
                "agent",
            )
            .await;
            json_result(&res)
        }
        "shopee_order_detail" => {
            let sns: Vec<String> = args
                .get("order_sn")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            if sns.is_empty() {
                return error_result("cần 'order_sn'".into());
            }
            json_result(&api::order_detail_value(s, &sns).await)
        }
        "shopee_list_drafts" => json_result(&json!({ "pending": s.db.list_drafts("pending") })),
        "shopee_approve_draft" => {
            let Some(id) = args.get("draft_id").and_then(|x| x.as_i64()) else {
                return error_result("thiếu 'draft_id'".into());
            };
            json_result(&api::send_draft(s, id).await)
        }
        "shopee_reject_draft" => {
            let Some(id) = args.get("draft_id").and_then(|x| x.as_i64()) else {
                return error_result("thiếu 'draft_id'".into());
            };
            let _ = s.db.decide_draft(id, "rejected", "");
            json_result(&json!({ "ok": true, "status": "rejected" }))
        }
        "shopee_tick" => json_result(&crate::engine::tick(s).await),
        "shopee_products" => {
            let status = args
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("NORMAL");
            json_result(&api::products_value(s, status).await)
        }
        "shopee_product_info" => {
            let ids: Vec<i64> = args
                .get("item_ids")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
                .unwrap_or_default();
            if ids.is_empty() {
                return error_result("cần 'item_ids'".into());
            }
            json_result(&api::product_info_value(s, &ids).await)
        }
        "shopee_update_stock" => {
            let (Some(item_id), Some(stock)) = (
                args.get("item_id").and_then(|x| x.as_i64()),
                args.get("stock").and_then(|x| x.as_i64()),
            ) else {
                return error_result("cần 'item_id' và 'stock'".into());
            };
            json_result(&api::update_stock_value(s, item_id, stock).await)
        }
        "shopee_update_price" => {
            let (Some(item_id), Some(price)) = (
                args.get("item_id").and_then(|x| x.as_i64()),
                args.get("price").and_then(|x| x.as_f64()),
            ) else {
                return error_result("cần 'item_id' và 'price'".into());
            };
            json_result(&api::update_price_value(s, item_id, price).await)
        }
        other => error_result(format!("tool không tồn tại: {other}")),
    }
}
