//! GraphQL client for a Dipper IoT Hub instance.
//!
//! Dipper Hub exposes a single gqlgen endpoint at `POST {base_url}/query`.
//! Auth: `mutation login` returns an opaque UUID token, then every request
//! carries `authorization: Bearer <token>` + `device_id` + `device: web`.
//! Auth inputs are camelCase; all device/log/alert inputs are snake_case.
//! The hub has NO client-facing online/offline API — we derive it from the
//! device's most recent log timestamp (window: HUB_ONLINE_WINDOW_SECS, 300s).

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub model: String,
    pub online: bool,
    pub last_seen: Option<String>,
    pub attributes: serde_json::Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryPoint {
    pub ts: String,
    pub field: String,
    pub value: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct AlertItem {
    pub id: String,
    pub device_id: String,
    pub device_name: String,
    pub level: String,
    pub message: String,
    pub ts: String,
}

struct Session {
    settings: crate::store::HubSettings,
    token: Option<String>,
}

pub struct HubClient {
    http: reqwest::Client,
    session: RwLock<Option<Session>>,
    /// Per-boot pseudo device id required by the hub's Login/TokenInfo inputs.
    device_id: String,
    /// device numeric id -> (fetched_at, latest log time) — avoids hammering
    /// getDeviceLogLastTime for every device on every 5s dashboard poll.
    last_seen_cache: Mutex<HashMap<u64, (Instant, Option<DateTime<Utc>>)>>,
}

const LAST_SEEN_TTL_SECS: u64 = 20;

fn online_window() -> i64 {
    std::env::var("HUB_ONLINE_WINDOW_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(300)
}

impl HubClient {
    pub fn new() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("build http client"),
            session: RwLock::new(None),
            device_id: format!("senclaw-hub-{nanos:x}"),
            last_seen_cache: Mutex::new(HashMap::new()),
        }
    }

    pub async fn conn_status(&self) -> Value {
        let guard = self.session.read().await;
        match guard.as_ref() {
            None => json!({
                "configured": false,
                "connected": false,
                "base_url": "",
                "username": "",
                "message": "Chưa cấu hình địa chỉ máy chủ Dipper Hub — vào Cài đặt.",
            }),
            Some(s) => json!({
                "configured": !s.settings.base_url.is_empty(),
                "connected": s.token.is_some(),
                "base_url": s.settings.base_url,
                "username": s.settings.username,
                "message": if s.token.is_some() {
                    format!("Đã đăng nhập {} ({})", s.settings.base_url, s.settings.username)
                } else if s.settings.base_url.is_empty() {
                    "Chưa cấu hình địa chỉ máy chủ Dipper Hub — vào Cài đặt.".to_string()
                } else {
                    "Chưa đăng nhập.".to_string()
                },
            }),
        }
    }

    /// Drop the session token but keep the configured server address.
    pub async fn logout(&self) {
        let mut guard = self.session.write().await;
        if let Some(s) = guard.as_mut() {
            s.token = None;
        }
    }

    /// Store settings without attempting a login (server address change).
    pub async fn set_settings(&self, settings: crate::store::HubSettings) {
        let mut guard = self.session.write().await;
        *guard = Some(Session {
            settings,
            token: None,
        });
    }

    /// Store settings and log in. On login failure the settings are kept
    /// (status shows configured-but-disconnected) and the error is returned.
    pub async fn connect(&self, settings: crate::store::HubSettings) -> Result<()> {
        {
            let mut guard = self.session.write().await;
            *guard = Some(Session {
                settings,
                token: None,
            });
        }
        self.login().await
    }

    async fn login(&self) -> Result<()> {
        let settings = {
            let guard = self.session.read().await;
            guard
                .as_ref()
                .map(|s| s.settings.clone())
                .context("chưa cấu hình kết nối")?
        };
        let query = r#"mutation Login($input: LoginInput!) {
            login(input: $input) { success reason token { token userId } }
        }"#;
        let variables = json!({
            "input": {
                "email": settings.username,
                "password": settings.password,
                "deviceType": "WEB",
                "deviceId": self.device_id,
            }
        });
        let data = self
            .raw_gql(&settings.base_url, None, query, variables)
            .await?;
        let login = &data["login"];
        if !login["success"].as_bool().unwrap_or(false) {
            bail!(
                "đăng nhập bị từ chối: {}",
                login["reason"].as_str().unwrap_or("không rõ lý do")
            );
        }
        let token = login["token"]["token"]
            .as_str()
            .context("hub không trả về token")?
            .to_string();
        let mut guard = self.session.write().await;
        if let Some(s) = guard.as_mut() {
            s.token = Some(token);
        }
        Ok(())
    }

    /// POST one GraphQL operation; returns the `data` object.
    async fn raw_gql(
        &self,
        base_url: &str,
        token: Option<&str>,
        query: &str,
        variables: Value,
    ) -> Result<Value> {
        if base_url.is_empty() {
            bail!("chưa cấu hình URL Dipper Hub");
        }
        let mut req = self
            .http
            .post(format!("{base_url}/query"))
            .json(&json!({ "query": query, "variables": variables }))
            .header("device", "web")
            .header("device_id", &self.device_id);
        if let Some(t) = token {
            req = req.header("authorization", format!("Bearer {t}"));
        }
        let res = req.send().await.context("không gọi được Dipper Hub")?;
        let status = res.status();
        let body: Value = res
            .json()
            .await
            .with_context(|| format!("Dipper Hub trả về không phải JSON (HTTP {status})"))?;
        if let Some(errors) = body.get("errors").and_then(|e| e.as_array()) {
            if !errors.is_empty() {
                let msgs: Vec<String> = errors
                    .iter()
                    .map(|e| e["message"].as_str().unwrap_or("?").to_string())
                    .collect();
                bail!("GraphQL error: {}", msgs.join("; "));
            }
        }
        body.get("data")
            .cloned()
            .filter(|d| !d.is_null())
            .ok_or_else(|| anyhow!("Dipper Hub không trả về data (HTTP {status})"))
    }

    /// Authenticated GraphQL call with one automatic re-login on auth errors
    /// (the UUID token can be invalidated server-side at any time).
    async fn gql(&self, query: &str, variables: Value) -> Result<Value> {
        let (base_url, token) = {
            let guard = self.session.read().await;
            let s = guard.as_ref().context(
                "chưa kết nối Dipper Hub — cấu hình URL + tài khoản trong app Device Hub trước",
            )?;
            (s.settings.base_url.clone(), s.token.clone())
        };
        if token.is_none() {
            self.login().await?;
        }
        let token = {
            let guard = self.session.read().await;
            guard.as_ref().and_then(|s| s.token.clone())
        };
        match self
            .raw_gql(&base_url, token.as_deref(), query, variables.clone())
            .await
        {
            Ok(v) => Ok(v),
            Err(e) => {
                let msg = format!("{e:#}").to_lowercase();
                if msg.contains("unauth") || msg.contains("token") || msg.contains("access denied")
                {
                    self.login().await?;
                    let token = {
                        let guard = self.session.read().await;
                        guard.as_ref().and_then(|s| s.token.clone())
                    };
                    self.raw_gql(&base_url, token.as_deref(), query, variables)
                        .await
                } else {
                    Err(e)
                }
            }
        }
    }

    fn parse_id(id: &str) -> Result<u64> {
        id.trim()
            .parse::<u64>()
            .with_context(|| format!("device_id không hợp lệ: {id:?} (phải là số)"))
    }

    /// Latest log timestamp for a device, cached ~20s.
    async fn last_seen(self: &Arc<Self>, device_id: u64) -> Option<DateTime<Utc>> {
        {
            let cache = self.last_seen_cache.lock().await;
            if let Some((at, v)) = cache.get(&device_id) {
                if at.elapsed().as_secs() < LAST_SEEN_TTL_SECS {
                    return *v;
                }
            }
        }
        let query = r#"query Last($input: LastTimeInput!) {
            getDeviceLogLastTime(input: $input) { data { time } }
        }"#;
        let result = self
            .gql(query, json!({ "input": { "device_id": device_id } }))
            .await
            .ok()
            .and_then(|d| {
                d["getDeviceLogLastTime"]["data"]
                    .as_array()
                    .map(|rows| {
                        rows.iter()
                            .filter_map(|r| {
                                r["time"]
                                    .as_str()
                                    .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                            })
                            .map(|t| t.with_timezone(&Utc))
                            .max()
                    })
                    .unwrap_or(None)
            });
        let mut cache = self.last_seen_cache.lock().await;
        cache.insert(device_id, (Instant::now(), result));
        result
    }

    fn device_from_detail(detail: &Value) -> Device {
        let mut attributes = serde_json::Map::new();
        if let Some(props) = detail["properties"].as_array() {
            for p in props {
                let key = p["key"].as_str().unwrap_or("").to_string();
                if key.is_empty() {
                    continue;
                }
                let value = if p["value_n"].is_number() {
                    p["value_n"].clone()
                } else {
                    p["value"].clone()
                };
                attributes.insert(key, value);
            }
        }
        Device {
            id: id_to_string(&detail["id"]),
            name: detail["name"].as_str().unwrap_or("(không tên)").to_string(),
            model: detail["description"]
                .as_str()
                .filter(|s| !s.is_empty())
                .or_else(|| detail["key"].as_str())
                .unwrap_or("")
                .to_string(),
            online: false,
            last_seen: None,
            attributes,
        }
    }

    async fn fill_liveness(self: &Arc<Self>, devices: &mut [Device]) {
        let window = Duration::seconds(online_window());
        let now = Utc::now();
        // Small concurrent fan-out; last_seen() caches per device for 20s.
        let futures: Vec<_> = devices
            .iter()
            .filter_map(|d| d.id.parse::<u64>().ok())
            .map(|id| {
                let me = self.clone();
                async move { (id, me.last_seen(id).await) }
            })
            .collect();
        let results = futures_util::future::join_all(futures).await;
        let map: HashMap<u64, Option<DateTime<Utc>>> = results.into_iter().collect();
        for d in devices.iter_mut() {
            if let Ok(id) = d.id.parse::<u64>() {
                if let Some(Some(t)) = map.get(&id) {
                    d.online = now.signed_duration_since(*t) <= window;
                    d.last_seen = Some(t.to_rfc3339_opts(SecondsFormat::Secs, true));
                }
            }
        }
    }

    pub async fn list_devices(self: &Arc<Self>, q: &str) -> Result<Vec<Device>> {
        let query = r#"query Devices($input: ListPaginationDeviceInput!) {
            getListPaginationDevice(input: $input) {
                total
                data {
                    id key name description is_gateway namespace_id tags
                    properties { key value value_n type }
                }
            }
        }"#;
        let mut input = json!({ "limit": 200, "skip": 0 });
        if !q.trim().is_empty() {
            input["search"] = json!(q.trim());
        }
        let data = self.gql(query, json!({ "input": input })).await?;
        let rows = data["getListPaginationDevice"]["data"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let mut devices: Vec<Device> = rows.iter().map(Self::device_from_detail).collect();
        // `search` behaviour is server-defined; also filter locally so name
        // matching is predictable for the agent ("máy bơm" → fuzzy contains).
        if !q.trim().is_empty() {
            let needle = q.trim().to_lowercase();
            let filtered: Vec<Device> = devices
                .iter()
                .filter(|d| {
                    d.name.to_lowercase().contains(&needle)
                        || d.model.to_lowercase().contains(&needle)
                        || d.id == needle
                })
                .cloned()
                .collect();
            if !filtered.is_empty() {
                devices = filtered;
            }
        }
        self.fill_liveness(&mut devices).await;
        Ok(devices)
    }

    pub async fn get_device(self: &Arc<Self>, id: &str) -> Result<Device> {
        let num_id = Self::parse_id(id)?;
        let query = r#"query DeviceById($id: Uint64!) {
            getDeviceById(id: $id) {
                id key name description is_gateway namespace_id tags
                properties { key value value_n type }
            }
        }"#;
        let data = self.gql(query, json!({ "id": num_id })).await?;
        let mut device = Self::device_from_detail(&data["getDeviceById"]);
        self.fill_liveness(std::slice::from_mut(&mut device)).await;
        Ok(device)
    }

    pub async fn telemetry(
        self: &Arc<Self>,
        id: &str,
        field: &str,
        limit: u32,
    ) -> Result<Vec<TelemetryPoint>> {
        let num_id = Self::parse_id(id)?;
        let now = Utc::now();
        let start = now - Duration::days(7);
        let query = r#"query Logs($input: ListPaginationDeviceLogInput!) {
            getListPaginationDeviceLog(input: $input) {
                data { key value value_n ts time is_number }
            }
        }"#;
        let input = json!({
            "device_id": num_id,
            "limit": limit.clamp(1, 500),
            "skip": 0,
            "start_time": start.to_rfc3339_opts(SecondsFormat::Secs, true),
            "end_time": now.to_rfc3339_opts(SecondsFormat::Secs, true),
        });
        let data = self.gql(query, json!({ "input": input })).await?;
        let mut points: Vec<TelemetryPoint> = data["getListPaginationDeviceLog"]["data"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|r| field.is_empty() || r["key"].as_str().map(|k| k == field).unwrap_or(false))
            .map(|r| TelemetryPoint {
                ts: r["time"].as_str().unwrap_or("").to_string(),
                field: r["key"].as_str().unwrap_or("").to_string(),
                value: if r["is_number"].as_bool().unwrap_or(false) && r["value_n"].is_number() {
                    r["value_n"].clone()
                } else {
                    r["value"].clone()
                },
            })
            .collect();
        points.sort_by(|a, b| b.ts.cmp(&a.ts));
        points.truncate(limit as usize);
        Ok(points)
    }

    pub async fn send_command(
        self: &Arc<Self>,
        id: &str,
        command: &str,
        params: &Value,
    ) -> Result<(bool, String)> {
        let num_id = Self::parse_id(id)?;
        let payload = if params.is_null() {
            String::new()
        } else if let Some(s) = params.as_str() {
            s.to_string()
        } else {
            serde_json::to_string(params)?
        };
        let query = r#"mutation Exec($deviceId: Uint64!, $name: String!, $payload: String) {
            executeActionDeviceByName(deviceId: $deviceId, name: $name, payload: $payload)
        }"#;
        let data = self
            .gql(
                query,
                json!({ "deviceId": num_id, "name": command, "payload": payload }),
            )
            .await?;
        let ok = data["executeActionDeviceByName"].as_bool().unwrap_or(false);
        // The command was accepted by the hub; delivery to the device happens
        // asynchronously (Redis device/command → MQTT v1/action).
        let detail = if ok {
            format!(
                "Hub đã nhận lệnh '{command}' cho thiết bị {id} (đẩy xuống qua MQTT v1/action)."
            )
        } else {
            format!("Hub từ chối lệnh '{command}' cho thiết bị {id}.")
        };
        Ok((ok, detail))
    }

    pub async fn alerts(self: &Arc<Self>, limit: u32) -> Result<Vec<AlertItem>> {
        let now = Utc::now();
        let start = now - Duration::days(7);
        let query = r#"query AlertLogs($input: ListPaginationAlertLogInput!) {
            getListPaginationAlertLog(input: $input) {
                data { id device_id alert_id message action_status source ts device { name } }
            }
        }"#;
        let input = json!({
            "limit": limit.clamp(1, 200),
            "skip": 0,
            "start_time": start.to_rfc3339_opts(SecondsFormat::Secs, true),
            "end_time": now.to_rfc3339_opts(SecondsFormat::Secs, true),
        });
        let data = self.gql(query, json!({ "input": input })).await?;
        let list = data["getListPaginationAlertLog"]["data"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|r| AlertItem {
                id: id_to_string(&r["id"]),
                device_id: id_to_string(&r["device_id"]),
                device_name: r["device"]["name"].as_str().unwrap_or("").to_string(),
                level: match r["action_status"].as_str().unwrap_or("") {
                    "" => "warning".to_string(),
                    s => s.to_lowercase(),
                },
                message: r["message"].as_str().unwrap_or("").to_string(),
                ts: r["ts"].as_str().unwrap_or("").to_string(),
            })
            .collect();
        Ok(list)
    }
}

/// Uint64 scalars arrive as JSON numbers (possibly > 2^53) — keep them as
/// strings on our own API so the JS side never loses precision.
fn id_to_string(v: &Value) -> String {
    if let Some(u) = v.as_u64() {
        u.to_string()
    } else if let Some(s) = v.as_str() {
        s.to_string()
    } else {
        v.to_string()
    }
}
