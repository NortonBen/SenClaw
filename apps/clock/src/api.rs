use axum::{
    extract::Query,
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Shared state: only the MCP SSE broadcast channel (the clock itself is
/// stateless — every value is computed from the system clock on demand).
pub struct AppState {
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> Arc<AppState> {
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    Arc::new(AppState { mcp_tx })
}

#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
    app: &'static str,
}

async fn status() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "ok",
        app: "clock",
    })
}

#[derive(Serialize)]
struct TimeResponse {
    utc: String,
    unix: i64,
    zones: Vec<ZoneTime>,
}

#[derive(Serialize, Clone)]
pub struct ZoneTime {
    pub zone: String,
    pub label: String,
    pub time: String,
    pub date: String,
    pub offset: String,
}

#[derive(Deserialize)]
struct TimeQuery {
    #[serde(default = "default_zones")]
    zones: String,
}

pub const DEFAULT_ZONES: &str = "Asia/Ho_Chi_Minh,America/New_York,Europe/London,Asia/Tokyo";

fn default_zones() -> String {
    DEFAULT_ZONES.to_string()
}

/// Compute the current wall-clock for each zone in a comma-separated list.
/// Unparseable zones are silently skipped.
pub fn compute_zones(zones_csv: &str) -> Vec<ZoneTime> {
    let now = Utc::now();
    zones_csv
        .split(',')
        .filter_map(|z| {
            let tz: Tz = z.trim().parse().ok()?;
            let local = now.with_timezone(&tz);
            Some(ZoneTime {
                zone: z.trim().to_string(),
                label: friendly_label(z.trim()),
                time: local.format("%H:%M:%S").to_string(),
                date: local.format("%Y-%m-%d").to_string(),
                offset: local.format("%:z").to_string(),
            })
        })
        .collect()
}

async fn get_time(Query(q): Query<TimeQuery>) -> Json<TimeResponse> {
    let now = Utc::now();
    Json(TimeResponse {
        utc: now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
        unix: now.timestamp(),
        zones: compute_zones(&q.zones),
    })
}

pub fn friendly_label(zone: &str) -> String {
    match zone {
        "Asia/Ho_Chi_Minh" => "Hà Nội".to_string(),
        "America/New_York" => "New York".to_string(),
        "Europe/London" => "London".to_string(),
        "Asia/Tokyo" => "Tokyo".to_string(),
        "Asia/Shanghai" => "Thượng Hải".to_string(),
        "America/Los_Angeles" => "Los Angeles".to_string(),
        "Europe/Paris" => "Paris".to_string(),
        "Australia/Sydney" => "Sydney".to_string(),
        _ => zone.rsplit('/').next().unwrap_or(zone).replace('_', " "),
    }
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/time", get(get_time))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}
