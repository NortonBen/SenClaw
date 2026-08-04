//! HTTP API cho app AutoTest. Mọi handler đi qua các helper `*_value` mà MCP
//! server ([`crate::mcp`]) dùng lại — REST và tool agent luôn hành xử giống
//! hệt nhau. Dữ liệu test nằm trong SQLite local; outbound duy nhất là bridge
//! LLM (AI sinh case / chẩn đoán), API đích mà test gọi, và Mini Browser cho
//! test web.

use crate::db::Db;
use crate::llm;
use crate::runner::Runner;
use app_space_sdk::SpaceClient;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub runner: Arc<Runner>,
    pub sc: SpaceClient,
    /// Fan-out MCP JSON-RPC responses tới SSE client đang nối.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("open autotest db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        runner: Arc::new(Runner::new(db.clone())),
        db,
        sc: SpaceClient::from_env(),
        mcp_tx,
    }
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/dashboard", get(dashboard))
        .route("/suites", get(list_suites).post(add_suite))
        .route("/suites/:id", get(get_suite).post(update_suite))
        .route("/suites/:id/delete", post(delete_suite))
        .route("/cases", post(add_case))
        .route("/cases/:id", post(update_case))
        .route("/cases/:id/delete", post(delete_case))
        .route("/environments", get(list_envs).post(set_env))
        .route("/environments/:id/delete", post(delete_env))
        .route("/run/suite", post(run_suite_h))
        .route("/run/case", post(run_case_h))
        .route("/runs", get(list_runs))
        .route("/runs/:id", get(get_run))
        .route("/runs/:id/cancel", post(cancel_run))
        .route("/report", get(report_h))
        .route("/schedules", get(list_schedules).post(set_schedule))
        .route("/schedules/:suite_id/delete", post(delete_schedule))
        .route("/ai/generate", post(ai_generate))
        .route("/ai/diagnose", post(ai_diagnose))
        .route("/activity", get(activity))
        .route("/settings", get(get_settings).post(set_settings))
        // MCP (HTTP + SSE), cùng shape với các Space App khác.
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

fn err(e: impl std::fmt::Display) -> Value {
    json!({ "ok": false, "error": e.to_string() })
}

// ---- status / dashboard ----

pub(crate) fn status_value(s: &AppState) -> Value {
    let d = s.db.dashboard();
    json!({
        "ok": true,
        "app": "autotest",
        "suites": d["suites"],
        "cases": d["cases"],
        "running": d["running"],
        "runs_today": d["runs_today"],
        "pass_rate_recent": d["pass_rate_recent"],
        "schedules_enabled": d["schedules_enabled"],
    })
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

async fn dashboard(State(s): State<AppState>) -> Json<Value> {
    Json(s.db.dashboard())
}

// ---- suites ----

#[derive(Deserialize, Default)]
pub(crate) struct SuiteIn {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub env_id: Option<i64>,
}

pub(crate) fn add_suite_value(s: &AppState, b: &SuiteIn) -> Value {
    match s.db.add_suite(&b.name, &b.description, b.env_id) {
        Ok(id) => {
            s.db.log(
                "suite",
                &format!("tạo suite \"{}\"", b.name.trim()),
                &id.to_string(),
            );
            json!({ "ok": true, "suite": s.db.get_suite(id) })
        }
        Err(e) => err(e),
    }
}

async fn add_suite(State(s): State<AppState>, Json(b): Json<SuiteIn>) -> Json<Value> {
    Json(add_suite_value(&s, &b))
}

pub(crate) fn list_suites_value(s: &AppState, include_archived: bool) -> Value {
    json!({ "ok": true, "suites": s.db.list_suites(include_archived) })
}

#[derive(Deserialize, Default)]
struct SuiteListQ {
    #[serde(default)]
    all: Option<bool>,
}

async fn list_suites(State(s): State<AppState>, Query(q): Query<SuiteListQ>) -> Json<Value> {
    Json(list_suites_value(&s, q.all.unwrap_or(false)))
}

/// Suite + danh sách case + schedule (màn chi tiết).
pub(crate) fn get_suite_value(s: &AppState, id: i64) -> Value {
    match s.db.get_suite(id) {
        None => err(format!("không có suite #{id}")),
        Some(mut suite) => {
            suite["cases"] =
                Value::Array(s.db.list_cases(id).iter().map(|c| c.to_value()).collect());
            suite["schedule"] =
                s.db.schedule_list()
                    .into_iter()
                    .find(|x| x["suite_id"].as_i64() == Some(id))
                    .unwrap_or(Value::Null);
            json!({ "ok": true, "suite": suite })
        }
    }
}

async fn get_suite(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_suite_value(&s, id))
}

#[derive(Deserialize, Default)]
pub(crate) struct SuiteUpdateIn {
    pub name: Option<String>,
    pub description: Option<String>,
    /// Some(None) không biểu diễn được qua JSON phẳng — dùng env_id: 0 để gỡ env.
    pub env_id: Option<i64>,
    pub status: Option<String>,
}

pub(crate) fn update_suite_value(s: &AppState, id: i64, b: &SuiteUpdateIn) -> Value {
    if s.db.get_suite(id).is_none() {
        return err(format!("không có suite #{id}"));
    }
    let env = b.env_id.map(|e| if e <= 0 { None } else { Some(e) });
    match s.db.update_suite(
        id,
        b.name.as_deref(),
        b.description.as_deref(),
        env,
        b.status.as_deref(),
    ) {
        Ok(()) => json!({ "ok": true, "suite": s.db.get_suite(id) }),
        Err(e) => err(e),
    }
}

async fn update_suite(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<SuiteUpdateIn>,
) -> Json<Value> {
    Json(update_suite_value(&s, id, &b))
}

pub(crate) fn delete_suite_value(s: &AppState, id: i64) -> Value {
    match s.db.get_suite(id) {
        None => err(format!("không có suite #{id}")),
        Some(suite) => match s.db.delete_suite(id) {
            Ok(()) => {
                s.db.log(
                    "suite",
                    &format!("xoá suite \"{}\"", suite["name"].as_str().unwrap_or("")),
                    &id.to_string(),
                );
                json!({ "ok": true })
            }
            Err(e) => err(e),
        },
    }
}

async fn delete_suite(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(delete_suite_value(&s, id))
}

// ---- cases ----

#[derive(Deserialize, Default)]
pub(crate) struct CaseIn {
    pub suite_id: i64,
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub position: Option<i64>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_timeout")]
    pub timeout_ms: i64,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub assertions: Value,
    #[serde(default)]
    pub extract: Value,
}

fn default_kind() -> String {
    "http".into()
}
fn default_true() -> bool {
    true
}
fn default_timeout() -> i64 {
    30000
}

/// config/assertions/extract nhận cả object/array JSON lẫn chuỗi JSON text.
fn json_arg(v: &Value, fallback: &str) -> String {
    match v {
        Value::Null => fallback.to_string(),
        Value::String(s) if s.trim().is_empty() => fallback.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub(crate) fn add_case_value(s: &AppState, b: &CaseIn) -> Value {
    if s.db.get_suite(b.suite_id).is_none() {
        return err(format!("không có suite #{}", b.suite_id));
    }
    match s.db.add_case(
        b.suite_id,
        &b.name,
        &b.kind,
        b.position,
        b.enabled,
        b.timeout_ms,
        &json_arg(&b.config, "{}"),
        &json_arg(&b.assertions, "[]"),
        &json_arg(&b.extract, "[]"),
    ) {
        Ok(id) => {
            s.db.log(
                "case",
                &format!("thêm case \"{}\" ({})", b.name.trim(), b.kind),
                &id.to_string(),
            );
            json!({ "ok": true, "case": s.db.get_case(id).map(|c| c.to_value()) })
        }
        Err(e) => err(e),
    }
}

async fn add_case(State(s): State<AppState>, Json(b): Json<CaseIn>) -> Json<Value> {
    Json(add_case_value(&s, &b))
}

#[derive(Deserialize, Default)]
pub(crate) struct CaseUpdateIn {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub position: Option<i64>,
    pub enabled: Option<bool>,
    pub timeout_ms: Option<i64>,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub assertions: Value,
    #[serde(default)]
    pub extract: Value,
}

pub(crate) fn update_case_value(s: &AppState, id: i64, b: &CaseUpdateIn) -> Value {
    if s.db.get_case(id).is_none() {
        return err(format!("không có case #{id}"));
    }
    let opt = |v: &Value| -> Option<String> {
        match v {
            Value::Null => None,
            other => Some(json_arg(other, "")),
        }
    };
    match s.db.update_case(
        id,
        b.name.as_deref(),
        b.kind.as_deref(),
        b.position,
        b.enabled,
        b.timeout_ms,
        opt(&b.config).as_deref(),
        opt(&b.assertions).as_deref(),
        opt(&b.extract).as_deref(),
    ) {
        Ok(()) => json!({ "ok": true, "case": s.db.get_case(id).map(|c| c.to_value()) }),
        Err(e) => err(e),
    }
}

async fn update_case(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<CaseUpdateIn>,
) -> Json<Value> {
    Json(update_case_value(&s, id, &b))
}

pub(crate) fn delete_case_value(s: &AppState, id: i64) -> Value {
    match s.db.get_case(id) {
        None => err(format!("không có case #{id}")),
        Some(c) => match s.db.delete_case(id) {
            Ok(()) => {
                s.db.log("case", &format!("xoá case \"{}\"", c.name), &id.to_string());
                json!({ "ok": true })
            }
            Err(e) => err(e),
        },
    }
}

async fn delete_case(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(delete_case_value(&s, id))
}

// ---- environments ----

#[derive(Deserialize, Default)]
pub(crate) struct EnvIn {
    pub name: String,
    #[serde(default)]
    pub vars: Value,
}

pub(crate) fn set_env_value(s: &AppState, b: &EnvIn) -> Value {
    match s.db.env_set(&b.name, &json_arg(&b.vars, "{}")) {
        Ok(id) => {
            s.db.log(
                "env",
                &format!("lưu environment \"{}\"", b.name.trim()),
                &id.to_string(),
            );
            json!({ "ok": true, "id": id })
        }
        Err(e) => err(e),
    }
}

async fn set_env(State(s): State<AppState>, Json(b): Json<EnvIn>) -> Json<Value> {
    Json(set_env_value(&s, &b))
}

pub(crate) fn list_envs_value(s: &AppState) -> Value {
    json!({ "ok": true, "environments": s.db.env_list() })
}

async fn list_envs(State(s): State<AppState>) -> Json<Value> {
    Json(list_envs_value(&s))
}

async fn delete_env(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    match s.db.env_delete(id) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(err(e)),
    }
}

// ---- chạy test ----

#[derive(Deserialize, Default)]
pub(crate) struct RunSuiteIn {
    pub suite_id: i64,
    #[serde(default)]
    pub env_id: Option<i64>,
    /// true → đợi chạy xong và trả kết quả đầy đủ (MCP dùng); false → chạy nền.
    #[serde(default)]
    pub wait: bool,
}

pub(crate) async fn run_suite_value(s: &AppState, b: &RunSuiteIn, trigger: &str) -> Value {
    if s.db.get_suite(b.suite_id).is_none() {
        return err(format!("không có suite #{}", b.suite_id));
    }
    if b.wait {
        match s.runner.run_suite(b.suite_id, b.env_id, trigger).await {
            Ok(run_id) => json!({ "ok": true, "run": s.db.get_run(run_id) }),
            Err(e) => err(e),
        }
    } else {
        let runner = s.runner.clone();
        let (suite_id, env_id, trigger) = (b.suite_id, b.env_id, trigger.to_string());
        tokio::spawn(async move {
            let _ = runner.run_suite(suite_id, env_id, &trigger).await;
        });
        json!({ "ok": true, "started": true })
    }
}

async fn run_suite_h(State(s): State<AppState>, Json(b): Json<RunSuiteIn>) -> Json<Value> {
    Json(run_suite_value(&s, &b, "manual").await)
}

#[derive(Deserialize, Default)]
pub(crate) struct RunCaseIn {
    pub case_id: i64,
    #[serde(default)]
    pub env_id: Option<i64>,
}

pub(crate) async fn run_case_value(s: &AppState, b: &RunCaseIn, trigger: &str) -> Value {
    match s.runner.run_case_solo(b.case_id, b.env_id, trigger).await {
        Ok(run_id) => json!({ "ok": true, "run": s.db.get_run(run_id) }),
        Err(e) => err(e),
    }
}

async fn run_case_h(State(s): State<AppState>, Json(b): Json<RunCaseIn>) -> Json<Value> {
    Json(run_case_value(&s, &b, "manual").await)
}

#[derive(Deserialize, Default)]
struct RunsQ {
    suite_id: Option<i64>,
    limit: Option<i64>,
}

pub(crate) fn list_runs_value(s: &AppState, suite_id: Option<i64>, limit: i64) -> Value {
    json!({ "ok": true, "runs": s.db.list_runs(suite_id, limit) })
}

async fn list_runs(State(s): State<AppState>, Query(q): Query<RunsQ>) -> Json<Value> {
    Json(list_runs_value(&s, q.suite_id, q.limit.unwrap_or(50)))
}

pub(crate) fn get_run_value(s: &AppState, id: i64) -> Value {
    match s.db.get_run(id) {
        Some(run) => json!({ "ok": true, "run": run }),
        None => err(format!("không có run #{id}")),
    }
}

async fn get_run(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_run_value(&s, id))
}

async fn cancel_run(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    s.runner.request_cancel(id);
    s.db.log("run", &format!("yêu cầu hủy run #{id}"), &id.to_string());
    Json(json!({ "ok": true }))
}

// ---- báo cáo ----

pub(crate) fn report_value(s: &AppState, suite_id: Option<i64>) -> Value {
    json!({
        "ok": true,
        "trend": s.db.pass_trend(suite_id, 30),
        "flaky": s.db.flaky_cases(10),
        "top_failing": s.db.top_failing(30, 10),
    })
}

#[derive(Deserialize, Default)]
struct ReportQ {
    suite_id: Option<i64>,
}

async fn report_h(State(s): State<AppState>, Query(q): Query<ReportQ>) -> Json<Value> {
    Json(report_value(&s, q.suite_id))
}

// ---- schedules ----

#[derive(Deserialize, Default)]
pub(crate) struct ScheduleIn {
    pub suite_id: i64,
    #[serde(default = "default_interval")]
    pub interval_min: i64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_interval() -> i64 {
    60
}

pub(crate) fn set_schedule_value(s: &AppState, b: &ScheduleIn) -> Value {
    if s.db.get_suite(b.suite_id).is_none() {
        return err(format!("không có suite #{}", b.suite_id));
    }
    match s.db.schedule_set(b.suite_id, b.interval_min, b.enabled) {
        Ok(()) => {
            s.db.log(
                "schedule",
                &format!(
                    "lịch suite #{}: mỗi {} phút, {}",
                    b.suite_id,
                    b.interval_min,
                    if b.enabled { "bật" } else { "tắt" }
                ),
                &b.suite_id.to_string(),
            );
            json!({ "ok": true, "schedules": s.db.schedule_list() })
        }
        Err(e) => err(e),
    }
}

async fn set_schedule(State(s): State<AppState>, Json(b): Json<ScheduleIn>) -> Json<Value> {
    Json(set_schedule_value(&s, &b))
}

pub(crate) fn list_schedules_value(s: &AppState) -> Value {
    json!({ "ok": true, "schedules": s.db.schedule_list() })
}

async fn list_schedules(State(s): State<AppState>) -> Json<Value> {
    Json(list_schedules_value(&s))
}

async fn delete_schedule(State(s): State<AppState>, Path(suite_id): Path<i64>) -> Json<Value> {
    match s.db.schedule_delete(suite_id) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(err(e)),
    }
}

// ---- AI ----

#[derive(Deserialize, Default)]
pub(crate) struct GenerateIn {
    pub suite_id: i64,
    pub description: String,
    /// false → chỉ trả về case đề xuất, không ghi DB (xem trước).
    #[serde(default = "default_true")]
    pub apply: bool,
}

pub(crate) async fn ai_generate_value(s: &AppState, b: &GenerateIn) -> Value {
    if s.db.get_suite(b.suite_id).is_none() {
        return err(format!("không có suite #{}", b.suite_id));
    }
    if b.description.trim().is_empty() {
        return err("description không được rỗng");
    }
    // Gợi ý cho model các biến environment đang có.
    let mut var_names: Vec<String> = vec![];
    for env in s.db.env_list() {
        if let Some(obj) = env["vars"].as_object() {
            for k in obj.keys() {
                if !var_names.contains(k) {
                    var_names.push(k.clone());
                }
            }
        }
    }
    let (cases, model) = match llm::generate(&s.sc, &b.description, &var_names).await {
        Ok(x) => x,
        Err(e) => return err(e),
    };
    let mut added = vec![];
    let mut rejected = vec![];
    for c in &cases {
        match llm::normalize_case(c) {
            Ok((name, kind, timeout_ms, config, assertions, extract)) => {
                if b.apply {
                    match s.db.add_case(
                        b.suite_id,
                        &name,
                        &kind,
                        None,
                        true,
                        timeout_ms,
                        &config,
                        &assertions,
                        &extract,
                    ) {
                        Ok(id) => added.push(
                            s.db.get_case(id)
                                .map(|x| x.to_value())
                                .unwrap_or(json!({"id": id})),
                        ),
                        Err(e) => rejected.push(json!({ "case": name, "error": e.to_string() })),
                    }
                } else {
                    added.push(c.clone());
                }
            }
            Err(e) => rejected.push(json!({ "case": c["name"], "error": e })),
        }
    }
    if b.apply {
        s.db.log(
            "ai",
            &format!("AI sinh {} case cho suite #{}", added.len(), b.suite_id),
            &b.suite_id.to_string(),
        );
    }
    json!({ "ok": true, "applied": b.apply, "model": model, "cases": added, "rejected": rejected })
}

async fn ai_generate(State(s): State<AppState>, Json(b): Json<GenerateIn>) -> Json<Value> {
    Json(ai_generate_value(&s, &b).await)
}

#[derive(Deserialize, Default)]
pub(crate) struct DiagnoseIn {
    pub run_id: i64,
    #[serde(default)]
    pub question: String,
}

pub(crate) async fn ai_diagnose_value(s: &AppState, b: &DiagnoseIn) -> Value {
    let run = match s.db.get_run(b.run_id) {
        Some(r) => r,
        None => return err(format!("không có run #{}", b.run_id)),
    };
    let (analysis, model) = llm::diagnose(&s.sc, &run, &b.question).await;
    json!({ "ok": true, "run_id": b.run_id, "model": model, "analysis": analysis })
}

async fn ai_diagnose(State(s): State<AppState>, Json(b): Json<DiagnoseIn>) -> Json<Value> {
    Json(ai_diagnose_value(&s, &b).await)
}

// ---- activity / settings ----

async fn activity(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "ok": true, "activity": s.db.activity(100) }))
}

async fn get_settings(State(s): State<AppState>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "browser_url": s.db.get_setting("browser_url").unwrap_or_default(),
    }))
}

#[derive(Deserialize, Default)]
struct SettingsIn {
    #[serde(default)]
    browser_url: String,
}

async fn set_settings(State(s): State<AppState>, Json(b): Json<SettingsIn>) -> Json<Value> {
    s.db.set_setting("browser_url", b.browser_url.trim());
    Json(json!({ "ok": true }))
}
