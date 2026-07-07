//! MCP server (HTTP/SSE) exposing the lunar calendar to SenClaw agents.
//! Tools: view today's almanac, xem ngày tốt xấu for any date, convert
//! solar⇄lunar, list good/bad days in a month, good hours, and an AI advisory.

use axum::{
    extract::State,
    response::sse::{Event, Sse},
    Json,
};
use futures_util::stream::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{convert::Infallible, sync::Arc};

use crate::almanac::day_info;
use crate::api::{days_in_month, render_facts, today, AppState};
use crate::lunar::{lunar_to_solar, TZ_VN};

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
            "serverInfo": { "name": "luna-mcp", "version": "1.0.0" }
        })),
        "ping" => reply(json!({})),
        "notifications/initialized" => Json(json!({ "jsonrpc": "2.0", "id": req.id, "result": {} })),
        "tools/list" => reply(json!({ "tools": tools_list() })),
        "tools/call" => {
            let params = req.params.clone().unwrap_or_default();
            let name = params["name"].as_str().unwrap_or("").to_string();
            let args = params["arguments"].clone();
            reply(call_tool(&name, &args).await)
        }
        _ => Json(json!("ok")),
    }
}

const DATE_PROP: &str =
    "Solar date as YYYY-MM-DD. Omit or use \"today\" for the current day (Vietnam, UTC+7).";

fn tools_list() -> Value {
    json!([
        {
            "name": "luna_today",
            "description": "Xem ngày hôm nay: the full Vietnamese almanac for TODAY — lunar date, Can-Chi, tiết khí, whether it is a good (Hoàng Đạo) or bad (Hắc Đạo) day, the auspicious hours (giờ hoàng đạo), lucky directions, xuất hành fortune, and taboo warnings. Start here for 'hôm nay là ngày gì / ngày tốt hay xấu'.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "luna_day",
            "description": "Xem ngày tốt xấu for ANY solar date: the complete almanac (âm lịch, Can-Chi ngày/tháng/năm, Hoàng Đạo/Hắc Đạo verdict, giờ hoàng đạo, trực, sao, nạp âm, hướng xuất hành, ngày kỵ). Use for 'xem ngày <date>', 'ngày mai/ngày kia', or checking a specific date.",
            "inputSchema": { "type": "object", "properties": {
                "date": { "type": "string", "description": DATE_PROP }
            } }
        },
        {
            "name": "luna_solar_to_lunar",
            "description": "Convert a SOLAR (dương lịch) date to its LUNAR (âm lịch) date, with the day's Can-Chi and good/bad verdict. Use for 'ngày dương X là ngày âm bao nhiêu'.",
            "inputSchema": { "type": "object", "properties": {
                "date": { "type": "string", "description": DATE_PROP }
            } }
        },
        {
            "name": "luna_lunar_to_solar",
            "description": "Convert a LUNAR (âm lịch) date to its SOLAR (dương lịch) date, with the full almanac. Use for 'ngày âm X tháng Y năm Z rơi vào dương lịch nào' (e.g. finding the solar date of a giỗ / Tết / birthday).",
            "inputSchema": { "type": "object", "properties": {
                "lunar_day": { "type": "number" },
                "lunar_month": { "type": "number" },
                "lunar_year": { "type": "number" },
                "leap": { "type": "boolean", "description": "true if the lunar month is a leap (nhuận) month" }
            }, "required": ["lunar_day", "lunar_month", "lunar_year"] }
        },
        {
            "name": "luna_good_hours",
            "description": "The auspicious hours (giờ Hoàng Đạo) and inauspicious hours (giờ Hắc Đạo) for a date, with their clock ranges. Use for 'giờ tốt hôm nay / giờ hoàng đạo ngày X'.",
            "inputSchema": { "type": "object", "properties": {
                "date": { "type": "string", "description": DATE_PROP }
            } }
        },
        {
            "name": "luna_good_days",
            "description": "List the good (Hoàng Đạo) or bad (Hắc Đạo) days in a solar month — the quick 'những ngày tốt tháng này' answer. Each entry has solar+lunar date, Can-Chi, the controlling god, good hours, and any taboo.",
            "inputSchema": { "type": "object", "properties": {
                "year": { "type": "number" },
                "month": { "type": "number", "description": "solar month 1..12" },
                "kind": { "type": "string", "enum": ["hoang-dao", "hac-dao"], "description": "hoang-dao (good, default) or hac-dao (bad)" }
            }, "required": ["year", "month"] }
        },
        {
            "name": "luna_advise",
            "description": "AI luận giải: judge whether a date suits a specific việc (cưới hỏi, khai trương, xuất hành, động thổ, ký hợp đồng…), grounded in that day's deterministic almanac. Returns a short nên/không-nên reasoning. Requires the daemon's LLM.",
            "inputSchema": { "type": "object", "properties": {
                "date": { "type": "string", "description": DATE_PROP },
                "activity": { "type": "string", "description": "The việc to evaluate, e.g. 'cưới hỏi', 'khai trương cửa hàng'." }
            }, "required": ["activity"] }
        }
    ])
}

/// Parse an optional YYYY-MM-DD arg, default today.
fn arg_date(args: &Value) -> Result<(i64, i64, i64), String> {
    match args["date"].as_str() {
        None | Some("") | Some("today") => Ok(today()),
        Some(s) => {
            let p: Vec<&str> = s.split('-').collect();
            if p.len() != 3 {
                return Err("date must be YYYY-MM-DD".into());
            }
            let yy = p[0].parse::<i64>().map_err(|_| "bad year")?;
            let mm = p[1].parse::<i64>().map_err(|_| "bad month")?;
            let dd = p[2].parse::<i64>().map_err(|_| "bad day")?;
            Ok((dd, mm, yy))
        }
    }
}

async fn call_tool(name: &str, args: &Value) -> Value {
    match name {
        "luna_today" => {
            let (d, m, y) = today();
            let info = day_info(d, m, y);
            json_result(json!({ "summary": render_facts(&info), "info": info }))
        }
        "luna_day" | "luna_solar_to_lunar" | "luna_good_hours" => match arg_date(args) {
            Ok((d, m, y)) => {
                let info = day_info(d, m, y);
                match name {
                    "luna_solar_to_lunar" => json_result(json!({
                        "solar": info.solar_date,
                        "lunar": format!("{} tháng {} năm {} ({})", info.lunar_day, info.lunar_month, info.year_can_chi, info.year_animal),
                        "lunarDay": info.lunar_day, "lunarMonth": info.lunar_month, "lunarYear": info.lunar_year, "leap": info.lunar_leap,
                        "dayCanChi": info.day_can_chi, "verdict": info.verdict, "weekday": info.weekday,
                    })),
                    "luna_good_hours" => json_result(json!({
                        "date": info.solar_date, "dayCanChi": info.day_can_chi,
                        "goodHours": info.good_hours,
                        "hours": info.hours,
                    })),
                    _ => json_result(json!({ "summary": render_facts(&info), "info": info })),
                }
            }
            Err(e) => error_result(e),
        },
        "luna_lunar_to_solar" => {
            let ld = args["lunar_day"].as_i64().unwrap_or(0);
            let lm = args["lunar_month"].as_i64().unwrap_or(0);
            let ly = args["lunar_year"].as_i64().unwrap_or(0);
            let leap = args["leap"].as_bool().unwrap_or(false);
            if ld < 1 || lm < 1 || lm > 12 || ly < 1 {
                return error_result("lunar_day 1..30, lunar_month 1..12, lunar_year required".into());
            }
            let (dd, mm, yy) = lunar_to_solar(ld, lm, ly, leap, TZ_VN);
            if (dd, mm, yy) == (0, 0, 0) {
                return error_result("that leap month does not exist in that lunar year".into());
            }
            let info = day_info(dd, mm, yy);
            json_result(json!({ "solar": info.solar_date, "summary": render_facts(&info), "info": info }))
        }
        "luna_good_days" => {
            let year = args["year"].as_i64().unwrap_or(0);
            let month = args["month"].as_i64().unwrap_or(0);
            if !(1..=12).contains(&month) || year < 1 {
                return error_result("year and month (1..12) required".into());
            }
            let want_bad = args["kind"].as_str() == Some("hac-dao");
            let list: Vec<Value> = (1..=days_in_month(month, year))
                .map(|d| day_info(d, month, year))
                .filter(|i| if want_bad { !i.hoang_dao } else { i.hoang_dao })
                .map(|i| json!({
                    "solar": i.solar_date, "lunar": i.lunar_date, "weekday": i.weekday,
                    "dayCanChi": i.day_can_chi, "god": i.day_god, "goodHours": i.good_hours,
                    "warnings": i.warnings,
                }))
                .collect();
            json_result(json!({ "year": year, "month": month, "kind": if want_bad {"hac-dao"} else {"hoang-dao"}, "count": list.len(), "days": list }))
        }
        "luna_advise" => {
            let activity = args["activity"].as_str().unwrap_or("").trim();
            if activity.is_empty() {
                return error_result("activity is required".into());
            }
            let (d, m, y) = match arg_date(args) {
                Ok(t) => t,
                Err(e) => return error_result(e),
            };
            let info = day_info(d, m, y);
            let facts = render_facts(&info);
            match crate::llm::advise(&facts, activity).await {
                Ok((text, model)) => json_result(json!({ "advice": text, "model": model, "facts": facts })),
                Err(e) => error_result(e),
            }
        }
        _ => error_result(format!("Unknown tool: {name}")),
    }
}
