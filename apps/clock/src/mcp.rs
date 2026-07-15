//! MCP server (HTTP/SSE) exposing the clock to SenClaw agents.
//! Tools: current time (any zone), world clock, timezone conversion, and
//! countdown/end-time computation. Everything is derived from the system
//! clock — deterministic, no state.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use chrono::{Duration, TimeZone, Utc};
use chrono_tz::Tz;
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::api::{compute_zones, friendly_label, AppState, DEFAULT_ZONES};

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
            "serverInfo": { "name": "clock-mcp", "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or(json!({}));
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            reply(call_tool(&name, &args))
        }
        _ => Json(json!("ok")),
    }
}

fn tools_list() -> Value {
    json!([
        {
            "name": "clock_now",
            "description": "Giờ hiện tại ở một múi giờ (mặc định Asia/Ho_Chi_Minh), kèm giờ UTC và thứ trong tuần. Dùng cho 'bây giờ mấy giờ / mấy giờ rồi / current time'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "zone": { "type": "string", "description": "IANA timezone, e.g. 'Asia/Ho_Chi_Minh', 'America/New_York'. Mặc định Asia/Ho_Chi_Minh." }
                }
            }
        },
        {
            "name": "clock_world",
            "description": "Giờ hiện tại ở NHIỀU múi giờ cùng lúc. Dùng cho 'giờ thế giới / mấy giờ ở New York/Tokyo/London'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "zones": { "type": "string", "description": "Danh sách IANA timezone ngăn cách bằng dấu phẩy. Mặc định: Asia/Ho_Chi_Minh,America/New_York,Europe/London,Asia/Tokyo." }
                }
            }
        },
        {
            "name": "clock_convert",
            "description": "Đổi một giờ trong ngày từ múi giờ này sang múi giờ khác. Dùng cho 'X giờ ở A là mấy giờ ở B'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "time": { "type": "string", "description": "Giờ theo định dạng HH:MM (24h), ví dụ '14:30'." },
                    "from": { "type": "string", "description": "Múi giờ nguồn (IANA)." },
                    "to": { "type": "string", "description": "Múi giờ đích (IANA)." }
                },
                "required": ["time", "from", "to"]
            }
        },
        {
            "name": "clock_countdown",
            "description": "Tính thời điểm KẾT THÚC của một bộ đếm ngược tính từ bây giờ. Dùng cho 'hẹn giờ X phút / đếm ngược Y giây thì mấy giờ'.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "minutes": { "type": "number", "description": "Số phút đếm ngược." },
                    "seconds": { "type": "number", "description": "Số giây đếm ngược (cộng thêm)." },
                    "zone": { "type": "string", "description": "Múi giờ hiển thị kết quả. Mặc định Asia/Ho_Chi_Minh." }
                }
            }
        }
    ])
}

fn arg_zone(args: &Value, key: &str) -> Tz {
    args.get(key)
        .and_then(|v| v.as_str())
        .and_then(|s| s.trim().parse::<Tz>().ok())
        .unwrap_or(chrono_tz::Asia::Ho_Chi_Minh)
}

fn call_tool(name: &str, args: &Value) -> Value {
    match name {
        "clock_now" => {
            let tz = arg_zone(args, "zone");
            let now = Utc::now();
            let local = now.with_timezone(&tz);
            json_result(json!({
                "zone": tz.name(),
                "label": friendly_label(tz.name()),
                "time": local.format("%H:%M:%S").to_string(),
                "date": local.format("%Y-%m-%d").to_string(),
                "weekday": local.format("%A").to_string(),
                "offset": local.format("%:z").to_string(),
                "utc": now.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
                "summary": format!(
                    "{} {} ({})",
                    local.format("%H:%M"),
                    local.format("%d/%m/%Y"),
                    friendly_label(tz.name())
                ),
            }))
        }
        "clock_world" => {
            let zones = args
                .get("zones")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(DEFAULT_ZONES);
            let list = compute_zones(zones);
            json_result(json!({ "count": list.len(), "zones": list }))
        }
        "clock_convert" => {
            let time = args.get("time").and_then(|v| v.as_str()).unwrap_or("");
            let from = match args.get("from").and_then(|v| v.as_str()).and_then(|s| s.parse::<Tz>().ok()) {
                Some(t) => t,
                None => return error_result("`from` không phải múi giờ IANA hợp lệ".into()),
            };
            let to = match args.get("to").and_then(|v| v.as_str()).and_then(|s| s.parse::<Tz>().ok()) {
                Some(t) => t,
                None => return error_result("`to` không phải múi giờ IANA hợp lệ".into()),
            };
            // Parse HH:MM, applied to TODAY's date in the `from` zone.
            let parts: Vec<&str> = time.split(':').collect();
            let (h, m) = match (
                parts.first().and_then(|s| s.trim().parse::<u32>().ok()),
                parts.get(1).and_then(|s| s.trim().parse::<u32>().ok()),
            ) {
                (Some(h), Some(m)) if h < 24 && m < 60 => (h, m),
                _ => return error_result("`time` phải theo định dạng HH:MM (24h)".into()),
            };
            let today_from = Utc::now().with_timezone(&from).date_naive();
            let naive = today_from.and_hms_opt(h, m, 0).unwrap();
            let src = match from.from_local_datetime(&naive).single() {
                Some(dt) => dt,
                None => return error_result("Giờ không tồn tại ở múi giờ nguồn (DST).".into()),
            };
            let dst = src.with_timezone(&to);
            json_result(json!({
                "from": { "zone": from.name(), "label": friendly_label(from.name()), "time": src.format("%H:%M").to_string(), "date": src.format("%Y-%m-%d").to_string(), "offset": src.format("%:z").to_string() },
                "to":   { "zone": to.name(),   "label": friendly_label(to.name()),   "time": dst.format("%H:%M").to_string(), "date": dst.format("%Y-%m-%d").to_string(), "offset": dst.format("%:z").to_string() },
                "summary": format!(
                    "{} ở {} = {} ở {} ({})",
                    src.format("%H:%M"), friendly_label(from.name()),
                    dst.format("%H:%M"), friendly_label(to.name()),
                    if dst.date_naive() != src.date_naive() { "khác ngày" } else { "cùng ngày" }
                ),
            }))
        }
        "clock_countdown" => {
            let minutes = args.get("minutes").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let seconds = args.get("seconds").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let total_secs = (minutes * 60.0 + seconds).round() as i64;
            if total_secs <= 0 {
                return error_result("Cần `minutes` hoặc `seconds` > 0.".into());
            }
            let tz = arg_zone(args, "zone");
            let now = Utc::now();
            let end = now + Duration::seconds(total_secs);
            let end_local = end.with_timezone(&tz);
            let now_local = now.with_timezone(&tz);
            let mm = total_secs / 60;
            let ss = total_secs % 60;
            json_result(json!({
                "duration_seconds": total_secs,
                "duration_label": if ss == 0 { format!("{mm} phút") } else { format!("{mm} phút {ss} giây") },
                "start": now_local.format("%H:%M:%S").to_string(),
                "end": end_local.format("%H:%M:%S").to_string(),
                "end_date": end_local.format("%Y-%m-%d").to_string(),
                "zone": tz.name(),
                "summary": format!(
                    "Đếm ngược {} bắt đầu {} → kết thúc lúc {} ({})",
                    if ss == 0 { format!("{mm} phút") } else { format!("{mm}p{ss}s") },
                    now_local.format("%H:%M:%S"),
                    end_local.format("%H:%M:%S"),
                    friendly_label(tz.name())
                ),
                "note": "Bộ hẹn giờ trực quan nằm ở tab 'Hẹn giờ' của app Đồng hồ; app sẽ báo (thông báo hệ thống + tiếng chuông) khi hết giờ.",
            }))
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
