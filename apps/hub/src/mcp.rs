//! MCP server (HTTP/SSE) exposing Dipper Hub device monitoring & control to
//! SenClaw agents. Tools: hub_status, hub_list_devices, hub_device_status,
//! hub_telemetry, hub_send_command, hub_alerts.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::AppState;

#[derive(Deserialize, Debug)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

pub async fn mcp_sse(
    State(state): State<Arc<AppState>>,
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
fn json_result(v: Value) -> Value {
    text_result(serde_json::to_string_pretty(&v).unwrap_or_default())
}
fn error_result(text: String) -> Value {
    json!({ "isError": true, "content": [{ "type": "text", "text": text }] })
}

pub async fn mcp_message(
    State(state): State<Arc<AppState>>,
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
            "serverInfo": { "name": "hub-mcp", "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or(json!({}));
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            reply(call_tool(&state, &name, &args).await)
        }
        _ => Json(json!("ok")),
    }
}

fn tools_list() -> Value {
    json!([
        {
            "name": "hub_status",
            "description": "Trạng thái kết nối tới Dipper IoT Hub: đã cấu hình chưa, đã đăng nhập chưa, URL đang dùng. Gọi tool này ĐẦU TIÊN nếu tool khác báo lỗi chưa kết nối.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "hub_list_devices",
            "description": "Danh sách thiết bị IoT trên Dipper Hub: id, tên, model, online/offline (suy từ telemetry gần nhất), thuộc tính. Dùng cho 'có những thiết bị nào / thiết bị nào online' và để tìm device_id theo tên trước khi điều khiển.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "Lọc theo tên thiết bị (tìm gần đúng, không phân biệt hoa thường). Bỏ trống để lấy tất cả." }
                }
            }
        },
        {
            "name": "hub_device_status",
            "description": "Chi tiết một thiết bị: online/offline, lần cuối gửi dữ liệu, model, thuộc tính (properties). Dùng trước khi gửi lệnh để kiểm tra thiết bị có online không.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "device_id": { "type": "string", "description": "ID thiết bị (lấy từ hub_list_devices)." }
                },
                "required": ["device_id"]
            }
        },
        {
            "name": "hub_telemetry",
            "description": "Dữ liệu telemetry (cảm biến) của thiết bị: các bản ghi mới nhất theo từng trường (nhiệt độ, độ ẩm...). Dùng cho 'nhiệt độ hiện tại bao nhiêu / dữ liệu cảm biến'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "device_id": { "type": "string", "description": "ID thiết bị." },
                    "field": { "type": "string", "description": "Chỉ lấy một trường (key) cụ thể, vd 'temperature'. Bỏ trống để lấy mọi trường." },
                    "limit": { "type": "number", "description": "Số bản ghi tối đa (mặc định 20)." }
                },
                "required": ["device_id"]
            }
        },
        {
            "name": "hub_send_command",
            "description": "GỬI LỆNH điều khiển xuống thiết bị thật qua Dipper Hub (Redis → MQTT v1/action). Đây là hành động có tác dụng vật lý — chỉ gọi khi người dùng đã yêu cầu/xác nhận rõ ràng. Sau khi gửi, kiểm chứng bằng hub_device_status / hub_telemetry.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "device_id": { "type": "string", "description": "ID thiết bị." },
                    "command": { "type": "string", "description": "Tên action trên Dipper Hub. Mặc định 'sendMsgToDevice' (gửi payload thẳng xuống thiết bị). Các action khác: 'updateServerPropertyDevice', 'switchServerPropertyDevice', hoặc action tự đặt tên của thiết bị." },
                    "params": { "type": "object", "description": "Payload JSON gửi kèm, vd {\"on\": true} hay {\"pump\": \"start\"} — tuỳ firmware thiết bị định nghĩa." }
                },
                "required": ["device_id", "command"]
            }
        },
        {
            "name": "hub_alerts",
            "description": "Danh sách cảnh báo (alert) gần đây từ Dipper Hub — thiết bị vượt ngưỡng, rule cảnh báo đã kích hoạt.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "number", "description": "Số cảnh báo tối đa (mặc định 20)." }
                }
            }
        }
    ])
}

async fn call_tool(state: &Arc<AppState>, name: &str, args: &Value) -> Value {
    let s = |key: &str| -> String {
        args.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    };

    match name {
        "hub_status" => json_result(state.client.conn_status().await),
        "hub_list_devices" => match state.client.list_devices(&s("q")).await {
            Ok(devices) => json_result(json!({ "count": devices.len(), "devices": devices })),
            Err(e) => error_result(format!("Lỗi lấy danh sách thiết bị: {e:#}")),
        },
        "hub_device_status" => {
            let id = s("device_id");
            if id.is_empty() {
                return error_result("Thiếu device_id (lấy từ hub_list_devices).".into());
            }
            match state.client.get_device(&id).await {
                Ok(d) => json_result(json!(d)),
                Err(e) => error_result(format!("Lỗi lấy thiết bị {id}: {e:#}")),
            }
        }
        "hub_telemetry" => {
            let id = s("device_id");
            if id.is_empty() {
                return error_result("Thiếu device_id (lấy từ hub_list_devices).".into());
            }
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as u32;
            match state.client.telemetry(&id, &s("field"), limit).await {
                Ok(points) => json_result(json!({ "count": points.len(), "telemetry": points })),
                Err(e) => error_result(format!("Lỗi lấy telemetry {id}: {e:#}")),
            }
        }
        "hub_send_command" => {
            let id = s("device_id");
            let command = s("command");
            if id.is_empty() || command.is_empty() {
                return error_result("Cần cả device_id và command.".into());
            }
            let params = args.get("params").cloned().unwrap_or(json!({}));
            match state.client.send_command(&id, &command, &params).await {
                Ok((ok, detail)) => json_result(json!({
                    "sent": ok,
                    "detail": detail,
                    "note": "Kiểm chứng lại bằng hub_device_status / hub_telemetry sau vài giây."
                })),
                Err(e) => error_result(format!("Gửi lệnh thất bại: {e:#}")),
            }
        }
        "hub_alerts" => {
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as u32;
            match state.client.alerts(limit).await {
                Ok(list) => json_result(json!({ "count": list.len(), "alerts": list })),
                Err(e) => error_result(format!("Lỗi lấy cảnh báo: {e:#}")),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
