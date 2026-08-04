//! HTTP API for the Luna Calendar app. Stateless — every endpoint is a pure
//! function of the requested date, computed by `almanac` / `lunar`.

use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::almanac::{day_info, DayInfo, Verdict};
use crate::lunar::{jd_to_ymd, lunar_to_solar, TZ_VN};

pub struct AppState {
    /// Broadcasts raw JSON-RPC responses to any connected MCP SSE clients.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> Arc<AppState> {
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState { mcp_tx })
}

pub struct ApiError(pub StatusCode, pub String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}
fn bad(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, e.to_string())
}

/// Today's Gregorian date in Vietnam (UTC+7): `(dd, mm, yy)`.
pub fn today() -> (i64, i64, i64) {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Unix epoch 1970-01-01 = JD 2440588; shift into +7 local time.
    let jd = 2440588 + (secs + (TZ_VN * 3600.0) as i64).div_euclid(86400);
    jd_to_ymd(jd)
}

/// Parse an optional `date=YYYY-MM-DD` query, defaulting to today.
fn parse_date(date: Option<&str>) -> Result<(i64, i64, i64), ApiError> {
    match date {
        None | Some("") | Some("today") => Ok(today()),
        Some(s) => {
            let p: Vec<&str> = s.split('-').collect();
            if p.len() != 3 {
                return Err(bad("date must be YYYY-MM-DD"));
            }
            let yy = p[0].parse::<i64>().map_err(bad)?;
            let mm = p[1].parse::<i64>().map_err(bad)?;
            let dd = p[2].parse::<i64>().map_err(bad)?;
            if !(1..=12).contains(&mm) || !(1..=31).contains(&dd) {
                return Err(bad("invalid month/day"));
            }
            Ok((dd, mm, yy))
        }
    }
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/day", get(get_day))
        .route("/month", get(get_month))
        .route("/lunar-to-solar", get(get_lunar_to_solar))
        .route("/good-days", get(get_good_days))
        .route("/advise", post(post_advise))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

async fn status() -> Json<Value> {
    let (d, m, y) = today();
    Json(json!({ "ok": true, "app": "luna-calendar", "today": format!("{y:04}-{m:02}-{d:02}") }))
}

#[derive(Deserialize)]
struct DateQuery {
    #[serde(default)]
    date: Option<String>,
}

/// Full almanac ("xem ngày tốt xấu") for one solar date (default today).
async fn get_day(Query(q): Query<DateQuery>) -> Result<Json<DayInfo>, ApiError> {
    let (dd, mm, yy) = parse_date(q.date.as_deref())?;
    Ok(Json(day_info(dd, mm, yy)))
}

#[derive(Deserialize)]
struct MonthQuery {
    year: i64,
    month: i64,
}

/// A month's worth of day cells for the calendar grid: solar day, lunar day/month,
/// day Can-Chi, and the tốt/xấu verdict (so the UI can dot each cell).
async fn get_month(Query(q): Query<MonthQuery>) -> Result<Json<Value>, ApiError> {
    if !(1..=12).contains(&q.month) {
        return Err(bad("month must be 1..12"));
    }
    let days = days_in_month(q.month, q.year);
    let cells: Vec<Value> = (1..=days)
        .map(|d| {
            let info = day_info(d, q.month, q.year);
            json!({
                "solarDay": d,
                "lunarDay": info.lunar_day,
                "lunarMonth": info.lunar_month,
                "lunarLeap": info.lunar_leap,
                "dayCanChi": info.day_can_chi,
                "weekday": info.weekday,
                "verdict": info.verdict,
                "hoangDao": info.hoang_dao,
                "warnings": info.warnings,
                // First lunar day of a month → show the month label like "1/6".
                "isLunarMonthStart": info.lunar_day == 1,
            })
        })
        .collect();
    // Weekday (Mon=0) of the 1st, for grid alignment.
    let first = day_info(1, q.month, q.year);
    Ok(Json(json!({
        "year": q.year,
        "month": q.month,
        "firstWeekday": crate::lunar::weekday_mon0(first.jd),
        "days": cells,
    })))
}

#[derive(Deserialize)]
struct LunarQuery {
    /// lunar day
    ld: i64,
    /// lunar month
    lm: i64,
    /// lunar year
    ly: i64,
    #[serde(default)]
    leap: bool,
}

/// Convert a lunar date to its solar date + full almanac.
async fn get_lunar_to_solar(Query(q): Query<LunarQuery>) -> Result<Json<Value>, ApiError> {
    let (dd, mm, yy) = lunar_to_solar(q.ld, q.lm, q.ly, q.leap, TZ_VN);
    if (dd, mm, yy) == (0, 0, 0) {
        return Err(bad("that leap month does not exist in that lunar year"));
    }
    let info = day_info(dd, mm, yy);
    Ok(Json(
        json!({ "solar": { "day": dd, "month": mm, "year": yy }, "info": info }),
    ))
}

#[derive(Deserialize)]
struct GoodDaysQuery {
    year: i64,
    month: i64,
    /// Optional: only "hoang-dao" good days, or "hac-dao" bad days. Default good.
    #[serde(default)]
    kind: Option<String>,
}

/// List the auspicious (or inauspicious) days of a solar month — the quick
/// "which days this month are good?" answer.
async fn get_good_days(Query(q): Query<GoodDaysQuery>) -> Result<Json<Value>, ApiError> {
    if !(1..=12).contains(&q.month) {
        return Err(bad("month must be 1..12"));
    }
    let want_bad = q.kind.as_deref() == Some("hac-dao");
    let days = days_in_month(q.month, q.year);
    let list: Vec<Value> = (1..=days)
        .map(|d| day_info(d, q.month, q.year))
        .filter(|i| if want_bad { !i.hoang_dao } else { i.hoang_dao })
        .map(|i| {
            json!({
                "solarDate": i.solar_date,
                "lunarDate": i.lunar_date,
                "weekday": i.weekday,
                "dayCanChi": i.day_can_chi,
                "dayGod": i.day_god,
                "verdict": i.verdict,
                "warnings": i.warnings,
                "goodHours": i.good_hours,
            })
        })
        .collect();
    Ok(Json(
        json!({ "year": q.year, "month": q.month, "kind": if want_bad {"hac-dao"} else {"hoang-dao"}, "days": list }),
    ))
}

#[derive(Deserialize)]
struct AdviseBody {
    #[serde(default)]
    date: Option<String>,
    /// The việc to evaluate, e.g. "cưới hỏi", "khai trương", "xuất hành".
    activity: String,
}

/// AI interpretation of whether a day suits an activity (grounded in the
/// deterministic almanac). Optional — requires the daemon's bridge LLM.
async fn post_advise(Json(b): Json<AdviseBody>) -> Result<Json<Value>, ApiError> {
    let (dd, mm, yy) = parse_date(b.date.as_deref())?;
    if b.activity.trim().is_empty() {
        return Err(bad("activity is required"));
    }
    let info = day_info(dd, mm, yy);
    let facts = render_facts(&info);
    match crate::llm::advise(&facts, b.activity.trim()).await {
        Ok((text, model)) => Ok(Json(
            json!({ "text": text, "model": model, "facts": facts }),
        )),
        Err(e) => Err(ApiError(StatusCode::BAD_GATEWAY, e)),
    }
}

/// A compact text rendering of the day's almanac for the LLM / MCP text results.
pub fn render_facts(i: &DayInfo) -> String {
    let verdict = match i.verdict {
        Verdict::Tot => "Tốt (Hoàng Đạo)",
        Verdict::Binh => "Bình thường",
        Verdict::Xau => "Xấu (Hắc Đạo)",
    };
    let warn = if i.warnings.is_empty() {
        "không".to_string()
    } else {
        i.warnings.join(", ")
    };
    format!(
        "Dương lịch: {} ({})\nÂm lịch: {} tháng {} năm {}\nNgày {} — tháng {} — năm {} ({})\n\
Tiết khí: {}\nTrực: {} | Sao (Nhị Thập Bát Tú): {} ({})\nNgũ hành (nạp âm): {} ({})\n\
Đánh giá: {} — thần {}\nGiờ Hoàng Đạo: {}\nHướng xuất hành: Hỷ Thần {}, Tài Thần {}\n\
Xuất hành (Lý Thuần Phong): {} — {}\nNgày kỵ: {}",
        i.solar_date,
        i.weekday,
        i.lunar_date,
        i.lunar_month,
        i.year_can_chi,
        i.day_can_chi,
        i.month_can_chi,
        i.year_can_chi,
        i.year_animal,
        i.tiet_khi,
        i.truc,
        i.tu,
        if i.tu_good { "tốt" } else { "xấu" },
        i.nap_am,
        i.ngu_hanh,
        verdict,
        i.day_god,
        i.good_hours,
        i.directions.hy_than,
        i.directions.tai_than,
        i.xuat_hanh,
        i.xuat_hanh_detail,
        warn,
    )
}

/// Days in a Gregorian month (handles leap years).
pub fn days_in_month(month: i64, year: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}
