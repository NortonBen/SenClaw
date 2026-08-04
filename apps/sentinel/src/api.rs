//! HTTP API. Mọi handler đi qua các hàm `*_value` mà [`crate::mcp`] dùng lại,
//! nên agent và người dùng luôn thấy hành vi giống hệt nhau.
//!
//! App chỉ ĐỌC nguồn; không có đường ghi nào tới daemon trong toàn bộ file này.

use crate::db::Db;
use crate::{ingest, llm, rules, snapshot};
use app_space_sdk::SpaceClient;
use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// Chu kỳ nền: trích xuất + chụp cấu hình + quét luật.
pub const TICK_SECS: u64 = 60;
/// Số lần tick giữa hai lần chụp cấu hình (15 phút).
const SNAPSHOT_EVERY: u64 = 15;
/// Trần sự kiện nạp vào một lượt quét.
const SCAN_EVENT_LIMIT: i64 = 20_000;

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub sc: SpaceClient,
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
    pub ticks: Arc<std::sync::atomic::AtomicU64>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("mở sentinel db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        db,
        sc: SpaceClient::from_env(),
        mcp_tx,
        ticks: Arc::new(std::sync::atomic::AtomicU64::new(0)),
    }
}

/// Một nhịp nền. Ingest chạy mỗi lần; chụp cấu hình thưa hơn vì nó gọi REST.
pub async fn tick(s: &AppState) -> Value {
    let n = s.ticks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let ing = ingest::run_all(&s.db);
    let snap = if n % SNAPSHOT_EVERY == 0 {
        Some(snapshot::take_all(&s.db).await.to_value())
    } else {
        None
    };
    let ctx = rules::RuleCtx::gather(&s.db, SCAN_EVENT_LIMIT).await;
    let scan = rules::scan(&s.db, &ctx);
    json!({ "ingest": ing.to_value(), "snapshot": snap, "scan": scan.to_value() })
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/dashboard", get(dashboard))
        .route("/ingest/run", post(ingest_run))
        .route("/scan", post(scan_now))
        .route("/events", get(events))
        .route("/events/:id", get(event_detail))
        .route("/events/:id/pivot", get(pivot))
        .route("/findings", get(findings))
        .route("/findings/:id", get(finding_detail))
        .route("/findings/:id/status", post(finding_status))
        .route("/findings/:id/explain", post(finding_explain))
        .route("/rules", get(rules_list))
        .route("/rules/:id", post(rule_update))
        .route("/snapshots", get(snapshots))
        .route("/snapshots/take", post(snapshots_take))
        .route("/snapshots/diff", get(snapshot_diffs))
        .route("/cases", get(cases).post(case_create))
        .route("/cases/:id", get(case_get).post(case_update))
        .route("/cases/:id/notes", post(case_note))
        .route("/cases/:id/attach", post(case_attach))
        .route("/cases/:id/hypothesis", post(case_hypothesis))
        .route("/cases/:id/report", post(case_report))
        .route("/ask", post(ask))
        .route("/verify-chain", get(verify_chain))
        .route("/suppressions", get(suppressions).post(suppression_add))
        .route("/suppressions/:id/delete", post(suppression_delete))
        .route("/sources", get(sources))
        .route("/tool-args", get(tool_args))
        .route("/settings", get(settings_get).post(settings_set))
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ---------------------------------------------------------------- status

pub(crate) fn status_value(s: &AppState) -> Value {
    let (lo, hi) = s.db.event_span();
    let counts = s.db.finding_counts().unwrap_or(json!({}));
    let (checked, broken) = s.db.verify_chain().unwrap_or((0, None));
    json!({
        "ok": true,
        "app": "sentinel",
        "events": s.db.event_count(),
        "event_span": { "from": lo, "to": hi },
        "findings": counts,
        "chain": { "checked": checked, "broken_at": broken, "intact": broken.is_none() },
        "sources": s.db.cursors().unwrap_or_default(),
        "rules": rules::RULES.len(),
    })
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

/// Tình trạng từng nguồn chứng cứ — trả lời "app đang mù chỗ nào".
pub(crate) async fn sources_value(s: &AppState) -> Value {
    let daemon = match crate::source::DaemonDb::open() {
        Ok(d) => json!({ "ok": true, "stats": d.stats() }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    };
    let rest = crate::source::DaemonRest::new();
    json!({
        "daemon_db": daemon,
        "llm_logs": crate::source::llm_log_index(),
        "cursors": s.db.cursors().unwrap_or_default(),
        "daemon_rest": {
            "base": crate::source::daemon_base_url(),
            "reachable": rest.config().await.is_some(),
        },
    })
}

async fn sources(State(s): State<AppState>) -> Json<Value> {
    Json(sources_value(&s).await)
}

#[derive(Deserialize)]
pub struct ToolArgsQuery {
    pub date: Option<String>,
    pub limit: Option<i64>,
}

/// Khôi phục **đối số tool** từ `llm_logs`.
///
/// `tool_executions` của daemon chỉ lưu *kết quả*; mọi `gen_tool_result_message`
/// bỏ `input`, nên trong DB không có đường dẫn file, URL hay tham số MCP nào
/// (Bash là ngoại lệ duy nhất, và cũng chỉ có 100 ký tự đầu của lệnh).
/// `llm_logs` là chỗ duy nhất còn giữ chúng.
///
/// App **không** chép nội dung này vào kho của mình — đọc trực tiếp từ file theo
/// yêu cầu, đã lọc bí mật, để không nhân đôi bề mặt lộ dữ liệu.
pub(crate) fn tool_args_value(q: &ToolArgsQuery) -> Value {
    let dir = crate::source::llm_log_dir();
    let date = q.date.clone().unwrap_or_else(|| {
        chrono::Utc::now().format("%Y-%m-%d").to_string()
    });
    let path = dir.join(format!("{date}.log"));
    if !path.exists() {
        return json!({
            "ok": false,
            "error": format!("không có nhật ký cho ngày {date}"),
            "available": crate::source::llm_log_index(),
        });
    }
    let calls = crate::source::tool_calls_in_log(&path, q.limit.unwrap_or(200) as usize);
    let masked: usize = calls
        .iter()
        .map(|c| crate::redact::count_secrets(&c["args"].to_string()))
        .sum();
    json!({
        "ok": true,
        "date": date,
        "file": path.display().to_string(),
        "count": calls.len(),
        "secrets_masked": masked,
        "tool_calls": calls,
        "note": "Đối số lấy trực tiếp từ ~/.senclaw/llm_logs và đã lọc bí mật. Sentinel không lưu bản sao."
    })
}

async fn tool_args(Query(q): Query<ToolArgsQuery>) -> Json<Value> {
    Json(tool_args_value(&q))
}

#[derive(Deserialize)]
pub struct SettingBody {
    pub key: String,
    pub value: String,
}

pub(crate) fn settings_value(s: &AppState) -> Value {
    json!({
        "ok": true,
        "theme": s.db.get_setting("theme").unwrap_or_else(|| "system".into()),
        "quiet_hours_from": s.db.get_setting("quiet_hours_from"),
        "quiet_hours_to": s.db.get_setting("quiet_hours_to"),
    })
}

async fn settings_get(State(s): State<AppState>) -> Json<Value> {
    Json(settings_value(&s))
}

async fn settings_set(State(s): State<AppState>, Json(b): Json<SettingBody>) -> Json<Value> {
    s.db.set_setting(&b.key, &b.value);
    Json(settings_value(&s))
}

pub(crate) fn dashboard_value(s: &AppState) -> Value {
    let counts = s.db.finding_counts().unwrap_or(json!({}));
    let top = s.db.findings(Some("open"), None, None, 8).unwrap_or_default();
    let (checked, broken) = s.db.verify_chain().unwrap_or((0, None));
    let (lo, hi) = s.db.event_span();

    // Thẻ tư thế: câu trả lời cho "hệ thống đang ở trạng thái nào" — đọc được
    // ngay mà không cần lật qua danh sách phát hiện.
    let open_all = s.db.findings(Some("open"), None, None, 500).unwrap_or_default();
    let hitl_off = open_all.iter().any(|f| f["rule_id"] == "SEN-CTRL-01");
    let wildcard = open_all
        .iter()
        .filter(|f| f["rule_id"] == "SEN-CTRL-03")
        .count();
    let lan_exposed = open_all.iter().any(|f| f["rule_id"] == "SEN-POSTURE-03");
    let script_tasks = open_all
        .iter()
        .filter(|f| f["rule_id"] == "SEN-PERSIST-02")
        .count();

    json!({
        "events": s.db.event_count(),
        "event_span": { "from": lo, "to": hi },
        "findings": counts,
        "top_findings": top,
        "activity": s.db.activity_by_day(14).unwrap_or_default(),
        "chain": { "checked": checked, "intact": broken.is_none(), "broken_at": broken },
        "posture": {
            "hitl_disabled": hitl_off,
            "wildcard_autoaccept_rules": wildcard,
            "apps_exposed_on_lan": lan_exposed,
            "shell_schedules": script_tasks,
        },
        "cases_open": s.db.cases(Some("open")).unwrap_or_default().len(),
    })
}

async fn dashboard(State(s): State<AppState>) -> Json<Value> {
    Json(dashboard_value(&s))
}

// ---------------------------------------------------------------- ingest/scan

pub(crate) fn ingest_value(s: &AppState) -> Value {
    ingest::run_all(&s.db).to_value()
}

async fn ingest_run(State(s): State<AppState>) -> Json<Value> {
    Json(ingest_value(&s))
}

pub(crate) async fn scan_value(s: &AppState) -> Value {
    let ctx = rules::RuleCtx::gather(&s.db, SCAN_EVENT_LIMIT).await;
    rules::scan(&s.db, &ctx).to_value()
}

async fn scan_now(State(s): State<AppState>) -> Json<Value> {
    Json(scan_value(&s).await)
}

// ---------------------------------------------------------------- events

#[derive(Deserialize, Default)]
pub struct EventQuery {
    pub from: Option<String>,
    pub to: Option<String>,
    pub actor: Option<String>,
    pub kind: Option<String>,
    pub tool: Option<String>,
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub before_id: Option<i64>,
}

pub(crate) fn events_value(s: &AppState, q: &EventQuery) -> Value {
    match s.db.events(
        q.from.as_deref(),
        q.to.as_deref(),
        q.actor.as_deref(),
        q.kind.as_deref(),
        q.tool.as_deref(),
        q.q.as_deref(),
        // Trần dành cho người dùng/agent nằm ở đây, không ở tầng DB — tầng DB
        // phải cho rule engine nạp cả kho.
        q.limit.unwrap_or(200).clamp(1, 2000),
        q.before_id,
    ) {
        Ok(rows) => json!({ "ok": true, "count": rows.len(), "events": rows }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn events(State(s): State<AppState>, Query(q): Query<EventQuery>) -> Json<Value> {
    Json(events_value(&s, &q))
}

pub(crate) fn event_detail_value(s: &AppState, id: i64) -> Value {
    match s.db.event(id) {
        Ok(Some(e)) => {
            // Phát hiện nào đang trích dẫn sự kiện này làm chứng cứ.
            let related: Vec<Value> = s
                .db
                .findings(None, None, None, 500)
                .unwrap_or_default()
                .into_iter()
                .filter(|f| {
                    f["evidence"]
                        .as_array()
                        .map(|a| a.iter().any(|v| v.as_i64() == Some(id)))
                        .unwrap_or(false)
                })
                .collect();
            json!({ "ok": true, "event": e, "findings": related })
        }
        Ok(None) => json!({ "ok": false, "error": format!("không có sự kiện id={id}") }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn event_detail(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(event_detail_value(&s, id))
}

#[derive(Deserialize)]
pub struct PivotQuery {
    pub mode: Option<String>,
    pub minutes: Option<i64>,
}

/// Pivot là thao tác điều tra thật sự: từ một sự kiện, hỏi "cái gì dẫn tới nó"
/// và "còn gì cùng lúc đó".
pub(crate) fn pivot_value(s: &AppState, id: i64, mode: &str, minutes: i64) -> Value {
    let Ok(Some(e)) = s.db.event(id) else {
        return json!({ "ok": false, "error": format!("không có sự kiện id={id}") });
    };
    let actor = e["actor"].as_str().unwrap_or("");
    let ts = e["ts"].as_str().unwrap_or("");

    let rows = match mode {
        "tool" => {
            let tool = e["tool_name"].as_str().unwrap_or("");
            if tool.is_empty() {
                vec![]
            } else {
                s.db.events(None, None, None, None, Some(tool), None, 200, None)
                    .unwrap_or_default()
            }
        }
        "schedule" => {
            // actor dạng `schedule:<id>` → mọi sự kiện của chính lịch đó.
            s.db.events(None, None, Some(actor), None, None, None, 200, None)
                .unwrap_or_default()
        }
        "preceding" => {
            // Tin nhắn và kết quả tool ngay TRƯỚC mốc này — ứng viên nguồn injection.
            let from = rules::parse_ts(ts)
                .map(|t| (t - chrono::Duration::minutes(minutes)).to_rfc3339())
                .unwrap_or_default();
            s.db.events(Some(&from), Some(ts), Some(actor), None, None, None, 200, None)
                .unwrap_or_default()
        }
        _ => s.db.events_near(actor, ts, minutes).unwrap_or_default(),
    };
    json!({ "ok": true, "mode": mode, "anchor": e, "count": rows.len(), "events": rows })
}

async fn pivot(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Query(q): Query<PivotQuery>,
) -> Json<Value> {
    Json(pivot_value(
        &s,
        id,
        q.mode.as_deref().unwrap_or("actor"),
        q.minutes.unwrap_or(30),
    ))
}

// ---------------------------------------------------------------- findings

#[derive(Deserialize, Default)]
pub struct FindingQuery {
    pub status: Option<String>,
    pub severity: Option<String>,
    pub rule: Option<String>,
    pub limit: Option<i64>,
}

pub(crate) fn findings_value(s: &AppState, q: &FindingQuery) -> Value {
    match s.db.findings(
        q.status.as_deref(),
        q.severity.as_deref(),
        q.rule.as_deref(),
        q.limit.unwrap_or(100),
    ) {
        Ok(rows) => json!({ "ok": true, "count": rows.len(), "findings": rows }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn findings(State(s): State<AppState>, Query(q): Query<FindingQuery>) -> Json<Value> {
    Json(findings_value(&s, &q))
}

pub(crate) fn finding_detail_value(s: &AppState, id: i64) -> Value {
    match s.db.finding(id) {
        Ok(Some(f)) => {
            let ids: Vec<i64> = f["evidence"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
                .unwrap_or_default();
            let ev = s.db.events_by_ids(&ids).unwrap_or_default();
            let about = rules::RULES
                .iter()
                .find(|r| Some(r.id) == f["rule_id"].as_str())
                .map(|r| json!({ "title": r.title, "about": r.about, "group": r.group }));
            json!({ "ok": true, "finding": f, "evidence": ev, "rule": about })
        }
        Ok(None) => json!({ "ok": false, "error": format!("không có phát hiện id={id}") }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn finding_detail(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(finding_detail_value(&s, id))
}

#[derive(Deserialize)]
pub struct StatusBody {
    pub status: String,
    pub note: Option<String>,
}

pub(crate) fn finding_status_value(s: &AppState, id: i64, body: &StatusBody) -> Value {
    match s
        .db
        .set_finding_status(id, &body.status, body.note.as_deref())
    {
        Ok(()) => json!({ "ok": true, "finding": s.db.finding(id).ok().flatten() }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn finding_status(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<StatusBody>,
) -> Json<Value> {
    Json(finding_status_value(&s, id, &body))
}

pub(crate) async fn finding_explain_value(s: &AppState, id: i64) -> Value {
    let Ok(Some(f)) = s.db.finding(id) else {
        return json!({ "ok": false, "error": format!("không có phát hiện id={id}") });
    };
    let ids: Vec<i64> = f["evidence"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
        .unwrap_or_default();
    let ev = s.db.events_by_ids(&ids).unwrap_or_default();
    match llm::explain(&s.sc, &f, &ev).await {
        Ok((text, model)) => json!({ "ok": true, "explanation": text, "model": model }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn finding_explain(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(finding_explain_value(&s, id).await)
}

// ---------------------------------------------------------------- rules

pub(crate) fn rules_value(s: &AppState) -> Value {
    json!({ "ok": true, "rules": rules::rules_catalog(&s.db) })
}

async fn rules_list(State(s): State<AppState>) -> Json<Value> {
    Json(rules_value(&s))
}

#[derive(Deserialize)]
pub struct RuleBody {
    pub enabled: Option<bool>,
    pub severity: Option<String>,
    pub params: Option<Value>,
}

pub(crate) fn rule_update_value(s: &AppState, id: &str, body: &RuleBody) -> Value {
    if !rules::RULES.iter().any(|r| r.id == id) {
        return json!({ "ok": false, "error": format!("không có luật {id}") });
    }
    match s.db.set_rule_config(
        id,
        body.enabled,
        body.severity.as_deref(),
        body.params.as_ref(),
    ) {
        Ok(()) => json!({ "ok": true, "rules": rules::rules_catalog(&s.db) }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn rule_update(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<RuleBody>,
) -> Json<Value> {
    Json(rule_update_value(&s, &id, &body))
}

// ---------------------------------------------------------------- snapshots

#[derive(Deserialize, Default)]
pub struct SnapQuery {
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

pub(crate) fn snapshots_value(s: &AppState, q: &SnapQuery) -> Value {
    json!({
        "ok": true,
        "kinds": snapshot::KINDS,
        "snapshots": s.db.snapshots(q.kind.as_deref(), q.limit.unwrap_or(50)).unwrap_or_default(),
    })
}

async fn snapshots(State(s): State<AppState>, Query(q): Query<SnapQuery>) -> Json<Value> {
    Json(snapshots_value(&s, &q))
}

pub(crate) async fn snapshots_take_value(s: &AppState) -> Value {
    let rep = snapshot::take_all(&s.db).await;
    let mut v = rep.to_value();
    v["ok"] = json!(true);
    v
}

async fn snapshots_take(State(s): State<AppState>) -> Json<Value> {
    Json(snapshots_take_value(&s).await)
}

pub(crate) fn snapshot_diffs_value(s: &AppState, q: &SnapQuery) -> Value {
    // Kèm ảnh mới nhất của nhóm đang xem, để giao diện hiện được "hiện trạng"
    // cạnh "đã đổi gì" mà không phải gọi thêm một vòng.
    let current = q
        .kind
        .as_deref()
        .and_then(|k| s.db.latest_snapshot(k).ok().flatten())
        .map(|(id, body)| json!({ "snapshot_id": id, "body": body }));
    json!({
        "ok": true,
        "current": current,
        "diffs": s.db.diffs(q.kind.as_deref(), q.limit.unwrap_or(50)).unwrap_or_default(),
    })
}

async fn snapshot_diffs(State(s): State<AppState>, Query(q): Query<SnapQuery>) -> Json<Value> {
    Json(snapshot_diffs_value(&s, &q))
}

// ---------------------------------------------------------------- cases

#[derive(Deserialize, Default)]
pub struct CaseQuery {
    pub status: Option<String>,
}

pub(crate) fn cases_value(s: &AppState, q: &CaseQuery) -> Value {
    json!({ "ok": true, "cases": s.db.cases(q.status.as_deref()).unwrap_or_default() })
}

async fn cases(State(s): State<AppState>, Query(q): Query<CaseQuery>) -> Json<Value> {
    Json(cases_value(&s, &q))
}

#[derive(Deserialize)]
pub struct CaseCreate {
    pub title: String,
    pub summary: Option<String>,
    pub severity: Option<String>,
    pub finding_ids: Option<Vec<i64>>,
}

pub(crate) fn case_create_value(s: &AppState, b: &CaseCreate) -> Value {
    if b.title.trim().is_empty() {
        return json!({ "ok": false, "error": "vụ việc phải có tiêu đề" });
    }
    match s.db.create_case(
        b.title.trim(),
        b.summary.as_deref().unwrap_or(""),
        b.severity.as_deref().unwrap_or("medium"),
    ) {
        Ok(id) => {
            for fid in b.finding_ids.clone().unwrap_or_default() {
                let _ = s.db.attach_finding_to_case(fid, id);
            }
            json!({ "ok": true, "case": s.db.case_detail(id).ok().flatten() })
        }
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn case_create(State(s): State<AppState>, Json(b): Json<CaseCreate>) -> Json<Value> {
    Json(case_create_value(&s, &b))
}

pub(crate) fn case_get_value(s: &AppState, id: i64) -> Value {
    match s.db.case_detail(id) {
        Ok(Some(mut c)) => {
            // Dòng thời gian của vụ việc = mọi chứng cứ của mọi phát hiện đã gắn.
            let ids: Vec<i64> = c["findings"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .flat_map(|f| {
                            f["evidence"]
                                .as_array()
                                .map(|e| e.iter().filter_map(|v| v.as_i64()).collect::<Vec<_>>())
                                .unwrap_or_default()
                        })
                        .collect()
                })
                .unwrap_or_default();
            c["timeline"] = json!(s.db.events_by_ids(&ids).unwrap_or_default());
            json!({ "ok": true, "case": c })
        }
        Ok(None) => json!({ "ok": false, "error": format!("không có vụ việc id={id}") }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn case_get(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(case_get_value(&s, id))
}

pub(crate) fn case_update_value(s: &AppState, id: i64, patch: &Value) -> Value {
    match s.db.update_case(id, patch) {
        Ok(()) => case_get_value(s, id),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn case_update(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<Value>,
) -> Json<Value> {
    Json(case_update_value(&s, id, &patch))
}

#[derive(Deserialize)]
pub struct NoteBody {
    pub body: String,
    pub author: Option<String>,
}

pub(crate) fn case_note_value(s: &AppState, id: i64, b: &NoteBody) -> Value {
    match s
        .db
        .add_case_note(id, b.author.as_deref().unwrap_or("user"), &b.body)
    {
        Ok(_) => case_get_value(s, id),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn case_note(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<NoteBody>,
) -> Json<Value> {
    Json(case_note_value(&s, id, &b))
}

#[derive(Deserialize)]
pub struct AttachBody {
    pub finding_ids: Vec<i64>,
}

pub(crate) fn case_attach_value(s: &AppState, id: i64, b: &AttachBody) -> Value {
    for fid in &b.finding_ids {
        if let Err(e) = s.db.attach_finding_to_case(*fid, id) {
            return json!({ "ok": false, "error": e.to_string() });
        }
    }
    case_get_value(s, id)
}

async fn case_attach(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<AttachBody>,
) -> Json<Value> {
    Json(case_attach_value(&s, id, &b))
}

/// Gom vụ việc + phát hiện + dòng thời gian rồi hỏi AI. Dùng chung cho cả giả
/// thuyết lẫn báo cáo để hai đường không lệch dữ liệu.
async fn case_bundle(s: &AppState, id: i64) -> Option<(Value, Vec<Value>, Vec<Value>)> {
    let c = s.db.case_detail(id).ok().flatten()?;
    let findings = s.db.findings_of_case(id).unwrap_or_default();
    let ids: Vec<i64> = findings
        .iter()
        .flat_map(|f| {
            f["evidence"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_i64()).collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .collect();
    let events = s.db.events_by_ids(&ids).unwrap_or_default();
    Some((c, findings, events))
}

pub(crate) async fn case_hypothesis_value(s: &AppState, id: i64) -> Value {
    let Some((c, f, e)) = case_bundle(s, id).await else {
        return json!({ "ok": false, "error": format!("không có vụ việc id={id}") });
    };
    match llm::hypothesize(&s.sc, &c, &f, &e).await {
        Ok((text, model)) => {
            let _ = s.db.update_case(id, &json!({ "hypothesis": text }));
            json!({ "ok": true, "hypothesis": text, "model": model })
        }
        Err(err) => json!({ "ok": false, "error": err.to_string() }),
    }
}

async fn case_hypothesis(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(case_hypothesis_value(&s, id).await)
}

pub(crate) async fn case_report_value(s: &AppState, id: i64) -> Value {
    let Some((c, f, e)) = case_bundle(s, id).await else {
        return json!({ "ok": false, "error": format!("không có vụ việc id={id}") });
    };
    match llm::case_report(&s.sc, &c, &f, &e).await {
        Ok((text, model)) => json!({ "ok": true, "report": text, "model": model }),
        Err(err) => json!({ "ok": false, "error": err.to_string() }),
    }
}

async fn case_report(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(case_report_value(&s, id).await)
}

// ---------------------------------------------------------------- ask

#[derive(Deserialize)]
pub struct AskBody {
    pub question: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// Hỏi bằng lời thường. Câu hỏi KHÔNG được dùng để sinh truy vấn — app tự lọc
/// theo khoảng thời gian rồi mới đưa dữ liệu cho mô hình tóm tắt.
pub(crate) async fn ask_value(s: &AppState, b: &AskBody) -> Value {
    if b.question.trim().is_empty() {
        return json!({ "ok": false, "error": "cần một câu hỏi" });
    }
    let events = s
        .db
        .events(
            b.from.as_deref(),
            b.to.as_deref(),
            None,
            None,
            None,
            None,
            300,
            None,
        )
        .unwrap_or_default();
    let findings = s.db.findings(Some("open"), None, None, 50).unwrap_or_default();
    let stats = json!({
        "tổng sự kiện đã thu": s.db.event_count(),
        "sự kiện trong khoảng hỏi": events.len(),
        "phát hiện đang mở": findings.len(),
    });
    match llm::ask_about(&s.sc, &b.question, &findings, &events, &stats).await {
        Ok((text, model)) => json!({
            "ok": true, "answer": text, "model": model,
            "events_considered": events.len()
        }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn ask(State(s): State<AppState>, Json(b): Json<AskBody>) -> Json<Value> {
    Json(ask_value(&s, &b).await)
}

// ---------------------------------------------------------------- chain

pub(crate) fn verify_chain_value(s: &AppState) -> Value {
    match s.db.verify_chain() {
        Ok((n, None)) => json!({
            "ok": true, "intact": true, "checked": n,
            "message": format!("Đã kiểm tra {n} sự kiện, chuỗi băm nguyên vẹn.")
        }),
        Ok((n, Some(id))) => json!({
            "ok": true, "intact": false, "checked": n, "broken_at": id,
            "message": format!("Chuỗi băm GÃY tại sự kiện id={id} (đã kiểm tra {n}). Một bản ghi trong quá khứ đã bị sửa hoặc xoá sau khi được ghi.")
        }),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn verify_chain(State(s): State<AppState>) -> Json<Value> {
    Json(verify_chain_value(&s))
}

// ---------------------------------------------------------------- suppressions

pub(crate) fn suppressions_value(s: &AppState) -> Value {
    json!({ "ok": true, "suppressions": s.db.suppressions().unwrap_or_default() })
}

async fn suppressions(State(s): State<AppState>) -> Json<Value> {
    Json(suppressions_value(&s))
}

#[derive(Deserialize)]
pub struct SuppressBody {
    pub rule_id: String,
    #[serde(default)]
    pub r#match: Value,
    pub reason: String,
    pub until: Option<String>,
}

pub(crate) fn suppression_add_value(s: &AppState, b: &SuppressBody) -> Value {
    match s
        .db
        .add_suppression(&b.rule_id, &b.r#match, &b.reason, b.until.as_deref())
    {
        Ok(_) => suppressions_value(s),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    }
}

async fn suppression_add(State(s): State<AppState>, Json(b): Json<SuppressBody>) -> Json<Value> {
    Json(suppression_add_value(&s, &b))
}

async fn suppression_delete(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(match s.db.delete_suppression(id) {
        Ok(()) => suppressions_value(&s),
        Err(e) => json!({ "ok": false, "error": e.to_string() }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// State test: bridge trỏ vào cổng không ai nghe, nên mọi lời gọi AI đều lỗi
    /// — đúng như mong muốn, phần logic không được phụ thuộc vào mạng.
    fn test_state() -> AppState {
        AppState {
            db: Arc::new(Db::open_memory().unwrap()),
            sc: SpaceClient::new("http://127.0.0.1:1", "sentinel"),
            mcp_tx: tokio::sync::broadcast::channel(10).0,
            ticks: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    fn seed(s: &AppState) {
        use crate::db::NewEvent;
        for (i, (kind, actor, tool, ts)) in [
            ("tool_call", "chat:a", "Read", "2026-07-01T10:00:00Z"),
            ("tool_call", "chat:a", "Bash", "2026-07-01T10:01:00Z"),
            ("tool_call", "chat:b", "mcp__senclaw-send__send_message", "2026-07-01T11:00:00Z"),
            ("permission_request", "chat:a", "Bash", "2026-07-01T09:59:00Z"),
        ]
        .iter()
        .enumerate()
        {
            let e = NewEvent::new("test", kind, actor, ts)
                .tool(tool)
                .ok(true)
                .summary(format!("sự kiện {i}"))
                .key(format!("test:{i}"));
            s.db.append_event(&e).unwrap();
        }
    }

    #[test]
    fn status_reports_counts_and_chain() {
        let s = test_state();
        seed(&s);
        let v = status_value(&s);
        assert_eq!(v["ok"], true);
        assert_eq!(v["events"], 4);
        assert_eq!(v["chain"]["intact"], true);
        assert_eq!(v["rules"], rules::RULES.len());
    }

    #[test]
    fn events_filter_by_actor_and_tool() {
        let s = test_state();
        seed(&s);
        let all = events_value(&s, &EventQuery::default());
        assert_eq!(all["count"], 4);

        let by_actor = events_value(
            &s,
            &EventQuery {
                actor: Some("chat:a".into()),
                ..Default::default()
            },
        );
        assert_eq!(by_actor["count"], 3);

        let by_tool = events_value(
            &s,
            &EventQuery {
                tool: Some("send_message".into()),
                ..Default::default()
            },
        );
        assert_eq!(by_tool["count"], 1);
    }

    #[test]
    fn pivot_actor_returns_neighbours_within_window() {
        let s = test_state();
        seed(&s);
        let all = events_value(&s, &EventQuery::default());
        let id = all["events"][0]["id"].as_i64().unwrap();
        let p = pivot_value(&s, id, "actor", 30);
        assert_eq!(p["ok"], true);
        assert!(p["count"].as_i64().unwrap() >= 1);
    }

    #[test]
    fn pivot_unknown_event_is_a_clean_error() {
        let s = test_state();
        let p = pivot_value(&s, 9999, "actor", 30);
        assert_eq!(p["ok"], false);
        assert!(p["error"].as_str().unwrap().contains("9999"));
    }

    #[test]
    fn finding_detail_joins_evidence_and_rule_text() {
        let s = test_state();
        seed(&s);
        let f = json!({
            "rule_id": "SEN-CTRL-01", "severity": "critical", "score": 90,
            "title": "HITL tắt", "detail": "d", "actor": null,
            "first_ts": "a", "last_ts": "b", "evidence": [1, 2], "standards": ["LLM06"],
            "dedupe_key": "k1"
        });
        let id = s.db.upsert_finding(&f).unwrap();
        let d = finding_detail_value(&s, id);
        assert_eq!(d["ok"], true);
        assert_eq!(d["evidence"].as_array().unwrap().len(), 2);
        assert!(d["rule"]["about"].as_str().unwrap().len() > 20);
    }

    #[test]
    fn event_detail_lists_findings_citing_it() {
        let s = test_state();
        seed(&s);
        let f = json!({
            "rule_id": "SEN-EXFIL-02", "severity": "high", "score": 70,
            "title": "gửi file", "detail": "", "actor": "chat:b",
            "first_ts": "a", "last_ts": "b", "evidence": [3], "standards": [],
            "dedupe_key": "k2"
        });
        s.db.upsert_finding(&f).unwrap();
        let d = event_detail_value(&s, 3);
        assert_eq!(d["findings"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn finding_status_rejects_bad_value_and_accepts_good() {
        let s = test_state();
        let f = json!({
            "rule_id": "R", "severity": "low", "score": 10, "title": "t", "detail": "",
            "actor": null, "first_ts": "a", "last_ts": "b", "evidence": [], "standards": [],
            "dedupe_key": "k3"
        });
        let id = s.db.upsert_finding(&f).unwrap();
        let bad = finding_status_value(&s, id, &StatusBody { status: "xyz".into(), note: None });
        assert_eq!(bad["ok"], false);
        let good = finding_status_value(
            &s,
            id,
            &StatusBody { status: "false_positive".into(), note: Some("tự chạy".into()) },
        );
        assert_eq!(good["ok"], true);
        assert_eq!(good["finding"]["status"], "false_positive");
    }

    #[test]
    fn rule_update_rejects_unknown_rule() {
        let s = test_state();
        let v = rule_update_value(
            &s,
            "SEN-KHONG-CO",
            &RuleBody { enabled: Some(false), severity: None, params: None },
        );
        assert_eq!(v["ok"], false);
    }

    #[test]
    fn rule_update_persists_and_shows_in_catalog() {
        let s = test_state();
        let v = rule_update_value(
            &s,
            "SEN-ANOM-01",
            &RuleBody { enabled: Some(false), severity: None, params: Some(json!({"from_hour": 2})) },
        );
        assert_eq!(v["ok"], true);
        let r = v["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "SEN-ANOM-01")
            .unwrap()
            .clone();
        assert_eq!(r["enabled"], false);
        assert_eq!(r["params"]["from_hour"], 2);
    }

    #[test]
    fn case_lifecycle_create_attach_note_close() {
        let s = test_state();
        seed(&s);
        let f = json!({
            "rule_id": "SEN-PERSIST-02", "severity": "critical", "score": 90,
            "title": "lịch shell", "detail": "", "actor": "schedule:x",
            "first_ts": "a", "last_ts": "b", "evidence": [1, 2], "standards": [],
            "dedupe_key": "k4"
        });
        let fid = s.db.upsert_finding(&f).unwrap();

        let c = case_create_value(
            &s,
            &CaseCreate {
                title: "Nghi vấn persistence".into(),
                summary: None,
                severity: Some("high".into()),
                finding_ids: Some(vec![fid]),
            },
        );
        assert_eq!(c["ok"], true);
        let cid = c["case"]["id"].as_i64().unwrap();

        let got = case_get_value(&s, cid);
        assert_eq!(got["case"]["findings"].as_array().unwrap().len(), 1);
        assert_eq!(
            got["case"]["timeline"].as_array().unwrap().len(),
            2,
            "dòng thời gian phải gom chứng cứ của phát hiện đã gắn"
        );

        let noted = case_note_value(&s, cid, &NoteBody { body: "đã hỏi chủ máy".into(), author: None });
        assert_eq!(noted["case"]["notes"].as_array().unwrap().len(), 1);

        let closed = case_update_value(&s, cid, &json!({"status": "closed"}));
        assert_eq!(closed["case"]["status"], "closed");
        assert!(closed["case"]["closed_at"].is_string());
    }

    #[test]
    fn case_create_requires_title() {
        let s = test_state();
        let v = case_create_value(
            &s,
            &CaseCreate { title: "   ".into(), summary: None, severity: None, finding_ids: None },
        );
        assert_eq!(v["ok"], false);
    }

    #[test]
    fn suppression_needs_reason_and_then_lists() {
        let s = test_state();
        let bad = suppression_add_value(
            &s,
            &SuppressBody {
                rule_id: "SEN-ANOM-01".into(),
                r#match: json!({}),
                reason: "".into(),
                until: None,
            },
        );
        assert_eq!(bad["ok"], false);

        let good = suppression_add_value(
            &s,
            &SuppressBody {
                rule_id: "SEN-ANOM-01".into(),
                r#match: json!({}),
                reason: "máy này vốn chạy tác vụ đêm".into(),
                until: None,
            },
        );
        assert_eq!(good["suppressions"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn verify_chain_reports_intact_and_break() {
        let s = test_state();
        seed(&s);
        let v = verify_chain_value(&s);
        assert_eq!(v["intact"], true);
        assert!(v["message"].as_str().unwrap().contains("nguyên vẹn"));
    }

    #[test]
    fn dashboard_exposes_posture_flags() {
        let s = test_state();
        let f = json!({
            "rule_id": "SEN-CTRL-01", "severity": "critical", "score": 90,
            "title": "HITL tắt", "detail": "", "actor": null,
            "first_ts": "a", "last_ts": "b", "evidence": [], "standards": [],
            "dedupe_key": "k5"
        });
        s.db.upsert_finding(&f).unwrap();
        let d = dashboard_value(&s);
        assert_eq!(d["posture"]["hitl_disabled"], true);
        assert_eq!(d["findings"]["by_severity"]["critical"], 1);
    }

    #[tokio::test]
    async fn ask_requires_a_question() {
        let s = test_state();
        let v = ask_value(
            &s,
            &AskBody { question: "  ".into(), from: None, to: None },
        )
        .await;
        assert_eq!(v["ok"], false);
    }

    #[tokio::test]
    async fn ai_failure_is_reported_not_panicked() {
        // Bridge không tồn tại → phải trả lỗi gọn, không sập.
        let s = test_state();
        let f = json!({
            "rule_id": "R", "severity": "low", "score": 1, "title": "t", "detail": "",
            "actor": null, "first_ts": "a", "last_ts": "b", "evidence": [], "standards": [],
            "dedupe_key": "k6"
        });
        let id = s.db.upsert_finding(&f).unwrap();
        let v = finding_explain_value(&s, id).await;
        assert_eq!(v["ok"], false);
        assert!(v["error"].is_string());
    }
}
