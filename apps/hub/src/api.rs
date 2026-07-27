//! REST proxy the web HMI talks to. Every endpoint delegates to the shared
//! `HubClient`, which holds the Dipper Hub session (GraphQL + token).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

use crate::client::HubClient;
use crate::store::Store;

pub struct AppState {
    pub client: std::sync::Arc<HubClient>,
    pub store: Store,
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> Arc<AppState> {
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    let store = Store::open().expect("open settings db");
    let client = std::sync::Arc::new(HubClient::new());
    let state = Arc::new(AppState {
        client,
        store,
        mcp_tx,
    });
    // Reconnect with persisted settings in the background at boot.
    if let Some(settings) = state.store.load_settings() {
        let st = state.clone();
        tokio::spawn(async move {
            if let Err(e) = st.client.connect(settings).await {
                eprintln!("hub: initial connect failed: {e:#}");
            }
        });
    }
    state
}

#[derive(Serialize)]
struct StatusResponse {
    status: &'static str,
    app: &'static str,
}

async fn status() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "ok",
        app: "hub",
    })
}

type ApiError = (StatusCode, Json<Value>);

fn err(e: anyhow::Error) -> ApiError {
    (
        StatusCode::BAD_GATEWAY,
        Json(json!({ "error": format!("{e:#}") })),
    )
}

async fn hub_status(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(st.client.conn_status().await)
}

#[derive(Deserialize)]
struct ServerSettingsBody {
    base_url: String,
    #[serde(default)]
    namespace: String,
}

/// Settings = server address only. Credentials live on the login screen.
async fn get_settings(State(st): State<Arc<AppState>>) -> Json<Value> {
    let s = st.store.load_settings().unwrap_or_default();
    Json(json!({
        "base_url": s.base_url,
        "namespace": s.namespace,
        "username": s.username,
    }))
}

async fn save_settings(
    State(st): State<Arc<AppState>>,
    Json(body): Json<ServerSettingsBody>,
) -> Result<Json<Value>, ApiError> {
    let mut s = st.store.load_settings().unwrap_or_default();
    s.base_url = body.base_url.trim().trim_end_matches('/').to_string();
    s.namespace = body.namespace.trim().to_string();
    st.store.save_settings(&s).map_err(err)?;
    // Changing the server invalidates the current session.
    st.client.set_settings(s).await;
    Ok(Json(st.client.conn_status().await))
}

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

async fn login(
    State(st): State<Arc<AppState>>,
    Json(body): Json<LoginBody>,
) -> Result<Json<Value>, ApiError> {
    let mut s = st.store.load_settings().unwrap_or_default();
    if s.base_url.is_empty() {
        return Ok(Json(json!({
            "configured": false,
            "connected": false,
            "base_url": "",
            "username": "",
            "message": "Chưa cấu hình địa chỉ máy chủ — vào Cài đặt trước.",
        })));
    }
    s.username = body.username.trim().to_string();
    s.password = body.password;
    match st.client.connect(s.clone()).await {
        Ok(()) => {
            // Persist credentials only after a successful login so the daemon
            // can re-authenticate by itself after a restart.
            st.store.save_settings(&s).map_err(err)?;
            Ok(Json(st.client.conn_status().await))
        }
        Err(e) => Ok(Json(json!({
            "configured": true,
            "connected": false,
            "base_url": s.base_url,
            "username": s.username,
            "message": format!("Đăng nhập thất bại: {e:#}"),
        }))),
    }
}

async fn logout(State(st): State<Arc<AppState>>) -> Json<Value> {
    st.client.logout().await;
    Json(st.client.conn_status().await)
}

#[derive(Deserialize)]
struct PanelBody {
    id: Option<i64>,
    name: String,
    #[serde(default)]
    html: String,
}

async fn list_panels(State(st): State<Arc<AppState>>) -> Result<Json<Value>, ApiError> {
    let panels = st.store.list_panels().map_err(err)?;
    Ok(Json(json!(panels)))
}

async fn save_panel(
    State(st): State<Arc<AppState>>,
    Json(body): Json<PanelBody>,
) -> Result<Json<Value>, ApiError> {
    if body.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Panel cần có tên" })),
        ));
    }
    let panel = st
        .store
        .save_panel(body.id, body.name.trim(), &body.html)
        .map_err(err)?;
    Ok(Json(json!(panel)))
}

async fn delete_panel(
    State(st): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<Value>, ApiError> {
    st.store.delete_panel(id).map_err(err)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct DevicesQuery {
    #[serde(default)]
    q: String,
}

async fn list_devices(
    State(st): State<Arc<AppState>>,
    Query(p): Query<DevicesQuery>,
) -> Result<Json<Value>, ApiError> {
    let devices = st.client.list_devices(&p.q).await.map_err(err)?;
    Ok(Json(json!(devices)))
}

async fn get_device(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let device = st.client.get_device(&id).await.map_err(err)?;
    Ok(Json(json!(device)))
}

#[derive(Deserialize)]
struct TelemetryQuery {
    #[serde(default)]
    field: String,
    #[serde(default = "default_limit")]
    limit: u32,
}

fn default_limit() -> u32 {
    50
}

async fn telemetry(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(p): Query<TelemetryQuery>,
) -> Result<Json<Value>, ApiError> {
    let points = st
        .client
        .telemetry(&id, &p.field, p.limit)
        .await
        .map_err(err)?;
    Ok(Json(json!(points)))
}

#[derive(Deserialize)]
struct CommandBody {
    command: String,
    #[serde(default)]
    params: Value,
}

async fn send_command(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<CommandBody>,
) -> Result<Json<Value>, ApiError> {
    let (ok, detail) = st
        .client
        .send_command(&id, &body.command, &body.params)
        .await
        .map_err(err)?;
    Ok(Json(json!({ "ok": ok, "detail": detail })))
}

#[derive(Deserialize)]
struct AlertsQuery {
    #[serde(default = "default_alert_limit")]
    limit: u32,
}

fn default_alert_limit() -> u32 {
    30
}

async fn alerts(
    State(st): State<Arc<AppState>>,
    Query(p): Query<AlertsQuery>,
) -> Result<Json<Value>, ApiError> {
    let list = st.client.alerts(p.limit).await.map_err(err)?;
    Ok(Json(json!(list)))
}

pub fn api_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/hub/status", get(hub_status))
        .route("/hub/settings", get(get_settings).post(save_settings))
        .route("/hub/login", post(login))
        .route("/hub/logout", post(logout))
        .route("/hub/panels", get(list_panels).post(save_panel))
        .route("/hub/panels/:id", axum::routing::delete(delete_panel))
        .route("/hub/devices", get(list_devices))
        .route("/hub/devices/:id", get(get_device))
        .route("/hub/devices/:id/telemetry", get(telemetry))
        .route("/hub/devices/:id/command", post(send_command))
        .route("/hub/alerts", get(alerts))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}
