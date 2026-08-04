//! HTTP API — ported from cmd/server/*.go. Uniform `{success,msg,data}`
//! envelope via `ok` / `ok_msg` / `err`.

use crate::ai;
use crate::bridge::Bridge;
use crate::db::{gen_id, Db};
use crate::domain::*;
use crate::run_manager::{next_run_after, validate_schedule_input, RunManager};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub runs: Arc<RunManager>,
    pub bridge: Bridge,
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
    pub ext: crate::extbridge::ExtBridge,
}

// ---- envelope helpers ----

fn ok<T: serde::Serialize>(data: T) -> Response {
    Json(json!({ "success": true, "msg": "ok", "data": data })).into_response()
}

fn ok_msg(msg: &str, data: Value) -> Response {
    Json(json!({ "success": true, "msg": msg, "data": data })).into_response()
}

fn err(status: StatusCode, msg: impl std::fmt::Display) -> Response {
    (
        status,
        Json(json!({ "success": false, "msg": msg.to_string(), "data": Value::Null })),
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct PageQuery {
    #[serde(default)]
    page: Option<i64>,
    #[serde(default, rename = "pageSize")]
    page_size: Option<i64>,
    #[serde(default)]
    q: Option<String>,
}

fn page_offset(pq: &PageQuery, default_size: i64, max_size: i64) -> (i64, i64) {
    let page = pq.page.unwrap_or(1).max(1);
    let mut size = pq.page_size.unwrap_or(default_size);
    if size < 1 {
        size = default_size;
    }
    if size > max_size {
        size = max_size;
    }
    ((page - 1) * size, size)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/accounts", get(list_accounts).post(create_account))
        .route("/api/proxies", get(list_proxies).post(create_proxy))
        .route("/api/proxies/:id", delete(delete_proxy))
        .route(
            "/api/browser-profiles",
            get(list_profiles).post(create_profile),
        )
        .route("/api/browser-profiles/:id", delete(delete_profile))
        .route("/api/flows", get(list_flows).post(create_flow))
        .route("/api/flows/generate-ai", post(flow_generate_ai))
        .route(
            "/api/saved-flow-actions",
            get(list_saved).post(create_saved),
        )
        .route(
            "/api/saved-flow-actions/:id",
            get(get_saved).delete(delete_saved),
        )
        .route(
            "/api/saved-flow-actions/analyze-page",
            post(analyze_page_unavailable),
        )
        .route("/api/settings", get(get_settings).post(save_settings))
        .route("/api/settings/deepseek-models", post(deepseek_models))
        .route(
            "/api/engine/legacy-atomic-rules",
            get(get_legacy_rules)
                .put(put_legacy_rules)
                .delete(delete_legacy_rules),
        )
        .route("/api/notification-rules", get(list_rules).post(create_rule))
        .route("/api/notification-rules/:id", delete(delete_rule))
        .route("/api/notifications", get(list_notifications))
        .route("/api/notifications/:id/read", post(mark_read))
        .route("/api/notifications/read-all", post(mark_all_read))
        .route("/api/notifications/unread-count", get(unread_count))
        .route("/api/runs", get(list_runs))
        .route("/api/runs/:id", get(run_by_id))
        .route("/api/runs/start", post(start_run))
        .route("/api/runs/browser-preview", post(browser_preview_run))
        .route("/api/dashboard/run-stats", get(dashboard_stats))
        .route("/api/schedules", get(list_schedules).post(create_schedule))
        .route("/api/schedules/:id", delete(delete_schedule))
        .route("/api/schedules/:id/toggle", post(toggle_schedule))
        .route("/api/schedules/:id/run-now", post(run_now_schedule))
        .route("/api/agent/suggest", post(agent_suggest))
        .route("/api/agent/profiles/generate", post(generate_profile))
        .route("/api/agent-skills", get(list_skills).post(create_skill))
        .route("/api/agent-skills/import", post(import_skills))
        .route(
            "/api/agent-skills/analyze-page",
            post(analyze_page_unavailable),
        )
        .route(
            "/api/agent-skills/:id",
            get(skill_by_id).put(update_skill).delete(delete_skill),
        )
        // AI-memory: stored via bridge knowledge; the standalone panel is disabled here.
        .route("/api/aimemory/status", get(aimemory_status))
        // Browser extension bridge: connection status + RPC-reply callback.
        .route("/api/ext/status", get(ext_status))
        .route("/api/ext/callback", post(ext_callback))
}

async fn health() -> Response {
    ok(json!({ "ok": true, "time": crate::db::now_str() }))
}

// ---- accounts ----

async fn list_accounts(State(s): State<AppState>, Query(pq): Query<PageQuery>) -> Response {
    let (off, lim) = page_offset(&pq, 20, 500);
    match s
        .db
        .list_accounts_page(off, lim, pq.q.as_deref().unwrap_or(""))
    {
        Ok((items, total)) => ok(json!({ "items": items, "total": total })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn create_account(State(s): State<AppState>, Json(a): Json<TikTokAccount>) -> Response {
    ok(s.db.upsert_account(a))
}

// ---- proxies ----

async fn list_proxies(State(s): State<AppState>, Query(pq): Query<PageQuery>) -> Response {
    let (off, lim) = page_offset(&pq, 20, 500);
    match s
        .db
        .list_proxies_page(off, lim, pq.q.as_deref().unwrap_or(""))
    {
        Ok((items, total)) => ok(json!({ "items": items, "total": total })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn create_proxy(State(s): State<AppState>, Json(p): Json<ManagedProxy>) -> Response {
    ok(s.db.upsert_proxy(p))
}

async fn delete_proxy(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.delete_proxy(&id) {
        Ok(()) => ok_msg("deleted", Value::Null),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

// ---- browser profiles ----

async fn list_profiles(State(s): State<AppState>, Query(pq): Query<PageQuery>) -> Response {
    let (off, lim) = page_offset(&pq, 20, 500);
    match s
        .db
        .list_browser_profiles_page(off, lim, pq.q.as_deref().unwrap_or(""))
    {
        Ok((items, total)) => ok(json!({ "items": items, "total": total })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn create_profile(State(s): State<AppState>, Json(p): Json<BrowserProfile>) -> Response {
    ok(s.db.upsert_browser_profile(p))
}

async fn delete_profile(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.delete_browser_profile(&id) {
        Ok(()) => ok_msg("deleted", Value::Null),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

// ---- flows ----

async fn list_flows(State(s): State<AppState>) -> Response {
    ok(s.db.list_flows())
}

async fn create_flow(State(s): State<AppState>, Json(f): Json<Flow>) -> Response {
    ok(s.db.upsert_flow(f))
}

#[derive(Deserialize)]
struct FlowGenReq {
    #[serde(default)]
    prompt: String,
    #[serde(default, rename = "actionsCatalog")]
    actions_catalog: Vec<ai::FlowGenCatalogItem>,
    #[serde(default, rename = "accountId")]
    account_id: String,
    #[serde(default, rename = "pageUrl")]
    page_url: String,
}

async fn flow_generate_ai(State(s): State<AppState>, Json(in_): Json<FlowGenReq>) -> Response {
    if in_.prompt.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "thiếu prompt");
    }
    // Build account context from the resolved account (no passwords). The live
    // browser probe is a CDP-driver capability; when unavailable we still ground
    // the LLM with the account summary.
    let mut ctx = String::new();
    if !in_.account_id.trim().is_empty() {
        match s.runs.find_account(&in_.account_id) {
            Ok(acc) => {
                let r = s.db.resolve_account_for_run(&acc);
                ctx.push_str("Account context (no passwords):\n");
                ctx.push_str(&format!("- id: {}\n- username: {}\n", r.id, r.username));
                if !r.profile_path.trim().is_empty() {
                    ctx.push_str(&format!("- profile_path: {}\n", r.profile_path));
                }
                if !r.proxy.trim().is_empty() {
                    ctx.push_str("- proxy: đã cấu hình\n");
                }
                if r.viewport_width > 0 && r.viewport_height > 0 {
                    ctx.push_str(&format!(
                        "- viewport: {}x{}\n",
                        r.viewport_width, r.viewport_height
                    ));
                }
                if !r.locale.trim().is_empty() {
                    ctx.push_str(&format!("- locale: {}\n", r.locale));
                }
            }
            Err(_) => return err(StatusCode::NOT_FOUND, "account not found"),
        }
    }
    let page_url = if in_.page_url.trim().is_empty() {
        "https://www.tiktok.com/"
    } else {
        in_.page_url.trim()
    };
    ctx.push_str(&format!("\nTrang mục tiêu: {page_url}\n"));

    match ai::generate_flow_from_catalog(&s.bridge, &in_.prompt, &in_.actions_catalog, &ctx).await {
        Ok(out) => ok(out),
        Err(e) => {
            let msg = e.to_string();
            let status = if msg.contains("chưa cấu hình") || msg.contains("unavailable") {
                StatusCode::SERVICE_UNAVAILABLE
            } else {
                StatusCode::BAD_REQUEST
            };
            err(status, msg)
        }
    }
}

// ---- saved flow actions ----

async fn list_saved(State(s): State<AppState>) -> Response {
    ok(s.db.list_saved_flow_actions())
}

async fn create_saved(State(s): State<AppState>, Json(in_): Json<SavedFlowAction>) -> Response {
    match s.db.upsert_saved_flow_action(in_) {
        Ok(v) => ok(v),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn get_saved(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.get_saved_flow_action(&id) {
        Ok(v) => ok(v),
        Err(e) => err(StatusCode::NOT_FOUND, e),
    }
}

async fn delete_saved(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.delete_saved_flow_action(&id) {
        Ok(()) => ok_msg("deleted", Value::Null),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn analyze_page_unavailable() -> Response {
    err(
        StatusCode::SERVICE_UNAVAILABLE,
        "phân tích trang thật cần TIKTOK_USE_PLAYWRIGHT=1 (CDP driver) — chưa bật trên instance này",
    )
}

// ---- settings ----

fn redact_secret(s: &str) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    if s.len() <= 8 {
        return "••••".into();
    }
    format!("{}••••{}", &s[..4], &s[s.len() - 4..])
}

fn merge_secret(old: &str, incoming: &str) -> String {
    let incoming = incoming.trim();
    if incoming.is_empty() {
        return String::new();
    }
    if incoming.contains("••••") {
        return old.to_string();
    }
    incoming.to_string()
}

async fn get_settings(State(s): State<AppState>) -> Response {
    match s.db.get_app_settings() {
        Ok(mut v) => {
            v.openai_api_key = redact_secret(&v.openai_api_key);
            v.openrouter_api_key = redact_secret(&v.openrouter_api_key);
            v.deepseek_api_key = redact_secret(&v.deepseek_api_key);
            v.lm_studio_api_key = redact_secret(&v.lm_studio_api_key);
            v.ai_memory_embedding_api_key = redact_secret(&v.ai_memory_embedding_api_key);
            ok(v)
        }
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn save_settings(State(s): State<AppState>, Json(mut in_): Json<AppSettings>) -> Response {
    let cur = s.db.get_app_settings().unwrap_or_default();
    in_.openai_api_key = merge_secret(&cur.openai_api_key, &in_.openai_api_key);
    in_.openrouter_api_key = merge_secret(&cur.openrouter_api_key, &in_.openrouter_api_key);
    in_.deepseek_api_key = merge_secret(&cur.deepseek_api_key, &in_.deepseek_api_key);
    in_.lm_studio_api_key = merge_secret(&cur.lm_studio_api_key, &in_.lm_studio_api_key);
    in_.ai_memory_embedding_api_key = merge_secret(
        &cur.ai_memory_embedding_api_key,
        &in_.ai_memory_embedding_api_key,
    );
    match s.db.upsert_app_settings(&in_) {
        Ok(()) => ok_msg("settings updated", Value::Null),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
struct DeepSeekReq {
    #[serde(default, rename = "apiKey")]
    api_key: String,
    #[serde(default, rename = "baseUrl")]
    base_url: String,
}

async fn deepseek_models(State(s): State<AppState>, Json(in_): Json<DeepSeekReq>) -> Response {
    let cur = s.db.get_app_settings().unwrap_or_default();
    let api_key = merge_secret(&cur.deepseek_api_key, &in_.api_key);
    if api_key.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "missing DeepSeek API key");
    }
    let mut base = in_.base_url.trim().to_string();
    if base.is_empty() {
        base = cur.deepseek_base_url.trim().to_string();
    }
    if base.is_empty() {
        base = "https://api.deepseek.com/v1".into();
    }
    base = base.trim_end_matches('/').to_string();
    if base.to_lowercase().ends_with("/v1") {
        base.truncate(base.len() - 3);
    }
    let url = format!("{}/models", base.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .bearer_auth(&api_key)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await;
    match resp {
        Ok(r) if r.status().is_success() => match r.json::<Value>().await {
            Ok(v) => {
                let mut models: Vec<String> = v
                    .get("data")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|m| m.get("id").and_then(Value::as_str))
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .unwrap_or_default();
                models.sort();
                models.dedup();
                ok(json!({ "models": models }))
            }
            Err(_) => err(StatusCode::BAD_GATEWAY, "invalid response from DeepSeek"),
        },
        Ok(r) => err(
            StatusCode::BAD_GATEWAY,
            format!("deepseek models error: {}", r.status()),
        ),
        Err(e) => err(StatusCode::BAD_GATEWAY, e),
    }
}

// ---- legacy atomic rules ----

async fn get_legacy_rules(State(s): State<AppState>) -> Response {
    let raw = s.db.get_legacy_atomic_rules_json().unwrap_or_default();
    ok(json!({ "loaded": crate::engine::browser::legacy_loaded(), "json": raw }))
}

async fn put_legacy_rules(State(s): State<AppState>, body: String) -> Response {
    // Apply to the in-memory book first (validates), then persist — same order
    // as the Go handler.
    if let Err(e) = crate::engine::browser::apply_legacy_rules(&body) {
        return err(StatusCode::BAD_REQUEST, e);
    }
    match s.db.set_legacy_atomic_rules_json(&body) {
        Ok(()) => ok_msg(
            "legacy atomic rules saved",
            json!({ "loaded": crate::engine::browser::legacy_loaded() }),
        ),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn delete_legacy_rules(State(s): State<AppState>) -> Response {
    let _ = crate::engine::browser::apply_legacy_rules("");
    match s.db.set_legacy_atomic_rules_json("") {
        Ok(()) => ok_msg("legacy atomic rules cleared", json!({ "loaded": false })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

// ---- notification rules ----

async fn list_rules(State(s): State<AppState>) -> Response {
    ok(s.db.list_notification_rules())
}

async fn create_rule(State(s): State<AppState>, Json(r): Json<NotificationRule>) -> Response {
    ok(s.db.upsert_notification_rule(r))
}

async fn delete_rule(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.delete_notification_rule(&id) {
        Ok(()) => ok_msg("deleted", Value::Null),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

// ---- notifications ----

#[derive(Deserialize)]
struct UnreadQuery {
    #[serde(default)]
    unread: Option<String>,
}

async fn list_notifications(State(s): State<AppState>, Query(q): Query<UnreadQuery>) -> Response {
    let unread = q.unread.as_deref() == Some("1");
    ok(s.db.list_notifications(unread, 50))
}

async fn mark_read(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.mark_notification_read(&id) {
        Ok(()) => ok_msg("marked as read", Value::Null),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn mark_all_read(State(s): State<AppState>) -> Response {
    match s.db.mark_all_notifications_read() {
        Ok(()) => ok_msg("all notifications marked as read", Value::Null),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn unread_count(State(s): State<AppState>) -> Response {
    ok(json!({ "count": s.db.count_unread_notifications() }))
}

// ---- runs ----

async fn list_runs(State(s): State<AppState>, Query(pq): Query<PageQuery>) -> Response {
    let (off, lim) = page_offset(&pq, 20, 100);
    match s.db.list_runs_page(off, lim, pq.q.as_deref().unwrap_or("")) {
        Ok((items, total)) => ok(json!({ "items": items, "total": total })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn run_by_id(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.get_run(&id) {
        Ok(r) => ok(r),
        Err(e) => err(StatusCode::NOT_FOUND, e),
    }
}

async fn dashboard_stats(State(s): State<AppState>) -> Response {
    match s.db.dashboard_run_stats() {
        Ok(v) => ok(v),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
struct StartRunReq {
    #[serde(default, rename = "accountId")]
    account_id: String,
    #[serde(default, rename = "flowId")]
    flow_id: String,
    #[serde(default)]
    params: Option<StrMap>,
}

async fn start_run(State(s): State<AppState>, Json(in_): Json<StartRunReq>) -> Response {
    let account = match s.runs.find_account(&in_.account_id) {
        Ok(a) => a,
        Err(_) => return err(StatusCode::NOT_FOUND, "account not found"),
    };
    if s.db.get_flow(&in_.flow_id).is_err() {
        return err(StatusCode::NOT_FOUND, "flow not found");
    }
    match s.runs.start_flow_run(account, &in_.flow_id, "", in_.params) {
        Ok(run) => ok(run),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
struct BrowserPreviewReq {
    #[serde(default, rename = "accountId")]
    account_id: String,
}

async fn browser_preview_run(
    State(s): State<AppState>,
    Json(in_): Json<BrowserPreviewReq>,
) -> Response {
    let account = match s.runs.find_account(&in_.account_id) {
        Ok(a) => a,
        Err(_) => return err(StatusCode::NOT_FOUND, "account not found"),
    };
    match s.runs.start_browser_preview(account) {
        Ok(run) => ok(run),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

// ---- schedules ----

async fn list_schedules(State(s): State<AppState>) -> Response {
    ok(s.db.list_schedules())
}

async fn create_schedule(State(s): State<AppState>, Json(mut in_): Json<Schedule>) -> Response {
    if in_.id.trim().is_empty() {
        in_.id = gen_id("sch");
    }
    if let Err(e) = validate_schedule_input(&in_, &s.db) {
        return err(StatusCode::BAD_REQUEST, e);
    }
    match next_run_after(&in_, chrono::Utc::now()) {
        Ok((next, _)) => {
            in_.next_run_at = next;
            ok(s.db.upsert_schedule(in_))
        }
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn delete_schedule(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.delete_schedule(&id) {
        Ok(()) => ok_msg("deleted", Value::Null),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn toggle_schedule(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let mut sc = match s.db.get_schedule(&id) {
        Ok(v) => v,
        Err(e) => return err(StatusCode::NOT_FOUND, e),
    };
    sc.enabled = !sc.enabled;
    if sc.enabled {
        match next_run_after(&sc, chrono::Utc::now()) {
            Ok((next, _)) => sc.next_run_at = next,
            Err(e) => return err(StatusCode::BAD_REQUEST, e),
        }
    } else {
        sc.next_run_at = String::new();
    }
    ok(s.db.upsert_schedule(sc))
}

async fn run_now_schedule(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    let sc = match s.db.get_schedule(&id) {
        Ok(v) => v,
        Err(e) => return err(StatusCode::NOT_FOUND, e),
    };
    if !sc.enabled {
        return err(StatusCode::BAD_REQUEST, "schedule is disabled");
    }
    match s.runs.trigger_schedule(sc) {
        Ok(()) => ok_msg("scheduled run dispatched", Value::Null),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

// ---- agent ----

#[derive(Deserialize)]
struct SuggestReq {
    #[serde(default, rename = "flowId")]
    flow_id: String,
}

async fn agent_suggest(State(s): State<AppState>, Json(in_): Json<SuggestReq>) -> Response {
    let flow = match s.db.get_flow(&in_.flow_id) {
        Ok(f) => f,
        Err(e) => return err(StatusCode::NOT_FOUND, e),
    };
    match ai::suggest_next(&s.bridge, &flow).await {
        Ok(msg) => ok(json!({ "suggestion": msg })),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Deserialize)]
struct ProfileGenReq {
    #[serde(default, rename = "accountId")]
    account_id: String,
    #[serde(default)]
    note: String,
}

async fn generate_profile(State(s): State<AppState>, Json(in_): Json<ProfileGenReq>) -> Response {
    let acc = if in_.account_id.trim().is_empty() {
        None
    } else {
        match s.db.get_account(&in_.account_id) {
            Some(a) => Some(a),
            None => return err(StatusCode::NOT_FOUND, "account not found"),
        }
    };
    let draft = ai::generate_profile_draft(&s.bridge, acc.as_ref(), &in_.note).await;
    ok(draft)
}

// ---- agent skills ----

async fn list_skills(State(s): State<AppState>) -> Response {
    match s.db.list_agent_skills() {
        Ok(v) => ok(v),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

async fn create_skill(State(s): State<AppState>, Json(mut in_): Json<AgentSkill>) -> Response {
    if in_.id.trim().is_empty() {
        in_.id = gen_id("skill");
    }
    match s.db.create_agent_skill(&mut in_) {
        Ok(()) => ok(in_),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn skill_by_id(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.get_agent_skill(&id) {
        Ok(v) => ok(v),
        Err(e) => err(StatusCode::NOT_FOUND, e),
    }
}

async fn update_skill(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(mut in_): Json<AgentSkill>,
) -> Response {
    in_.id = id;
    match s.db.update_agent_skill(&mut in_) {
        Ok(()) => ok(in_),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

async fn delete_skill(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.delete_agent_skill(&id) {
        Ok(()) => ok_msg("deleted", Value::Null),
        Err(e) => err(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Deserialize)]
struct ImportSkillsReq {
    #[serde(default)]
    skills: Vec<AgentSkill>,
}

async fn import_skills(State(s): State<AppState>, Json(in_): Json<ImportSkillsReq>) -> Response {
    let mut imported = 0;
    for mut sk in in_.skills {
        if sk.id.trim().is_empty() {
            sk.id = gen_id("skill");
        }
        sk.enabled = true;
        if s.db.create_agent_skill(&mut sk).is_ok() {
            imported += 1;
        } else {
            let _ = s.db.update_agent_skill(&mut sk);
            imported += 1;
        }
    }
    ok_msg("imported", json!({ "imported": imported }))
}

async fn aimemory_status() -> Response {
    ok(
        json!({ "enabled": false, "note": "AI-memory dùng chung cognitive graph của SenClaw qua bridge knowledge.save" }),
    )
}

// ---- browser extension bridge ----

async fn ext_status(State(s): State<AppState>) -> Response {
    ok(json!({
        "connected": s.ext.is_connected(),
        "wsPort": crate::config::ext_ws_port(),
        "stats": s.ext.stats(),
    }))
}

/// The extension POSTs its RPC replies here (resilient to WS drops). Must
/// present the callback secret handed out on connect.
async fn ext_callback(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    if body.get("secret").and_then(Value::as_str) != Some(&s.ext.secret()) {
        return err(StatusCode::UNAUTHORIZED, "bad callback secret");
    }
    let id = body
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if id.is_empty() {
        return err(StatusCode::BAD_REQUEST, "missing id");
    }
    s.ext.complete_callback(&id, body);
    ok_msg("ok", Value::Null)
}
