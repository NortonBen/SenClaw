//! HTTP API for the Capital app. Every handler funnels through small
//! `*_value` helpers that the MCP server ([`crate::mcp`]) reuses, so REST and
//! agent tools always behave identically. All money data stays in the local
//! SQLite DB; the only outbound call is the LLM bridge for `/analyze`.

use crate::db::Db;
use crate::finance::{self, generate_schedule};
use crate::llm;
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
    pub sc: SpaceClient,
    /// Fan-out of MCP JSON-RPC responses to any connected SSE client.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("open capital db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        db,
        sc: SpaceClient::from_env(),
        mcp_tx,
    }
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/dashboard", get(dashboard))
        .route("/sources", get(list_sources).post(add_source))
        .route("/sources/:id", get(get_source).post(update_source))
        .route("/transactions", get(list_tx).post(add_tx))
        .route("/transactions/:id/delete", post(delete_tx))
        .route("/schedule", get(list_schedule))
        .route("/schedule/generate", post(generate_schedule_h))
        .route("/schedule/:id/pay", post(pay_schedule))
        .route("/allocations", get(list_allocs).post(add_alloc))
        .route("/allocations/:id", post(update_alloc))
        .route("/report/cashflow", get(cashflow))
        .route("/report/usage", get(usage_h))
        .route("/report/source-ratings", get(ratings_h))
        .route("/goals", get(goals_list).post(goal_add))
        .route("/goals/:id", post(goal_update))
        .route("/goals/:id/plan", post(goal_plan))
        .route("/goals/:id/steps", post(goal_steps))
        .route("/insight", get(insight_h))
        .route("/simulate", post(simulate_h))
        .route("/analyze", post(analyze))
        .route("/activity", get(activity))
        // MCP (HTTP + SSE), same shape as the other Space Apps.
        .route(
            "/mcp/sse",
            get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message),
        )
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ---- status / dashboard ----

pub(crate) fn status_value(s: &AppState) -> Value {
    let today = finance::today();
    let overdue = s.db.list_schedule(None, Some("overdue"), &today, 500);
    let sources = s.db.list_sources(Some("active"));
    json!({
        "ok": true,
        "app": "capital",
        "sources_active": sources.len(),
        "debt_outstanding": sources.iter()
            .filter(|x| finance::is_debt_kind(&x.kind))
            .map(|x| x.outstanding()).sum::<f64>(),
        "overdue_count": overdue.len(),
    })
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

pub(crate) fn dashboard_value(s: &AppState) -> Value {
    s.db.dashboard(&finance::today())
}

async fn dashboard(State(s): State<AppState>) -> Json<Value> {
    Json(dashboard_value(&s))
}

// ---- sources ----

#[derive(Deserialize, Default)]
pub(crate) struct SourceIn {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub total_amount: f64,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub interest_rate: f64,
    #[serde(default)]
    pub rate_type: String,
    #[serde(default)]
    pub start_date: String,
    #[serde(default)]
    pub end_date: String,
    #[serde(default)]
    pub note: String,
}

pub(crate) fn add_source_value(s: &AppState, b: &SourceIn) -> Value {
    let rate_type = if b.rate_type.is_empty() {
        "fixed"
    } else {
        &b.rate_type
    };
    match s.db.add_source(
        &b.name,
        &b.kind,
        &b.provider,
        b.total_amount,
        &b.currency,
        b.interest_rate,
        rate_type,
        &b.start_date,
        &b.end_date,
        &b.note,
    ) {
        Ok(id) => {
            s.db.log(
                "source",
                &format!("thêm nguồn vốn \"{}\" ({})", b.name.trim(), b.kind),
                &id.to_string(),
            );
            match s.db.get_source(id) {
                Some(row) => json!({ "ok": true, "source": row.to_value() }),
                None => json!({ "ok": true, "id": id }),
            }
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn add_source(State(s): State<AppState>, Json(b): Json<SourceIn>) -> Json<Value> {
    Json(add_source_value(&s, &b))
}

#[derive(Deserialize)]
struct StatusQuery {
    status: Option<String>,
}

pub(crate) fn list_sources_value(s: &AppState, status: Option<&str>) -> Value {
    json!({ "sources": s.db.list_sources(status).iter().map(|x| x.to_value()).collect::<Vec<_>>() })
}

async fn list_sources(State(s): State<AppState>, Query(q): Query<StatusQuery>) -> Json<Value> {
    Json(list_sources_value(&s, q.status.as_deref()))
}

/// One source with its ledger and schedule attached — the detail view.
pub(crate) fn get_source_value(s: &AppState, id: i64) -> Value {
    let Some(row) = s.db.get_source(id) else {
        return json!({ "error": format!("nguồn vốn #{id} không tồn tại") });
    };
    let today = finance::today();
    json!({
        "source": row.to_value(),
        "transactions": s.db.list_tx(Some(id), None, None, 200),
        "schedule": s.db.list_schedule(Some(id), None, &today, 500),
    })
}

async fn get_source(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_source_value(&s, id))
}

pub(crate) fn update_source_value(s: &AppState, id: i64, patch: &Value) -> Value {
    match s.db.update_source(id, patch) {
        Ok(()) => {
            s.db.log(
                "source",
                &format!("cập nhật nguồn vốn #{id}"),
                &id.to_string(),
            );
            match s.db.get_source(id) {
                Some(row) => json!({ "ok": true, "source": row.to_value() }),
                None => json!({ "ok": true }),
            }
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn update_source(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<Value>,
) -> Json<Value> {
    Json(update_source_value(&s, id, &patch))
}

// ---- transactions ----

#[derive(Deserialize)]
pub(crate) struct TxIn {
    pub source_id: i64,
    pub kind: String,
    pub amount: f64,
    #[serde(default)]
    pub alloc_id: Option<i64>,
    #[serde(default)]
    pub tx_date: String,
    #[serde(default)]
    pub note: String,
}

pub(crate) fn add_tx_value(s: &AppState, b: &TxIn) -> Value {
    match s.db.add_tx(
        b.source_id,
        b.alloc_id,
        &b.kind,
        b.amount,
        &b.tx_date,
        &b.note,
    ) {
        Ok(id) => {
            s.db.log(
                "tx",
                &format!(
                    "giao dịch {} {} vào nguồn #{}",
                    b.kind, b.amount, b.source_id
                ),
                &id.to_string(),
            );
            json!({ "ok": true, "tx_id": id })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn add_tx(State(s): State<AppState>, Json(b): Json<TxIn>) -> Json<Value> {
    Json(add_tx_value(&s, &b))
}

#[derive(Deserialize)]
struct TxQuery {
    source_id: Option<i64>,
    kind: Option<String>,
    alloc_id: Option<i64>,
    limit: Option<i64>,
}

async fn list_tx(State(s): State<AppState>, Query(q): Query<TxQuery>) -> Json<Value> {
    Json(json!({
        "transactions": s.db.list_tx(q.source_id, q.kind.as_deref(), q.alloc_id, q.limit.unwrap_or(200))
    }))
}

async fn delete_tx(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    match s.db.delete_tx(id) {
        Ok(true) => {
            s.db.log("tx", &format!("xoá giao dịch #{id}"), &id.to_string());
            Json(json!({ "ok": true }))
        }
        Ok(false) => Json(json!({ "error": format!("giao dịch #{id} không tồn tại") })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

// ---- schedule ----

#[derive(Deserialize)]
struct ScheduleQuery {
    source_id: Option<i64>,
    status: Option<String>,
    limit: Option<i64>,
}

pub(crate) fn list_schedule_value(
    s: &AppState,
    source_id: Option<i64>,
    status: Option<&str>,
    limit: i64,
) -> Value {
    let today = finance::today();
    json!({ "schedule": s.db.list_schedule(source_id, status, &today, limit) })
}

async fn list_schedule(State(s): State<AppState>, Query(q): Query<ScheduleQuery>) -> Json<Value> {
    Json(list_schedule_value(
        &s,
        q.source_id,
        q.status.as_deref(),
        q.limit.unwrap_or(500),
    ))
}

#[derive(Deserialize)]
pub(crate) struct GenerateIn {
    pub source_id: i64,
    /// annuity | equal_principal | interest_only
    #[serde(default)]
    pub method: String,
    /// Principal to amortize. 0/absent → dư nợ hiện tại của nguồn.
    #[serde(default)]
    pub principal: f64,
    /// %/năm. Absent → lãi suất của nguồn.
    #[serde(default)]
    pub annual_rate: Option<f64>,
    pub periods: u32,
    /// First installment = start_date + 1 kỳ. Absent → hôm nay.
    #[serde(default)]
    pub start_date: String,
    /// 1 = tháng, 3 = quý… (mặc định 1)
    #[serde(default)]
    pub freq_months: u32,
}

pub(crate) fn generate_schedule_value(s: &AppState, b: &GenerateIn) -> Value {
    let Some(src) = s.db.get_source(b.source_id) else {
        return json!({ "error": format!("nguồn vốn #{} không tồn tại", b.source_id) });
    };
    let principal = if b.principal > 0.0 {
        b.principal
    } else {
        src.outstanding()
    };
    if principal <= 0.0 {
        return json!({ "error": "principal = 0 — nguồn chưa có dư nợ, hãy truyền 'principal' hoặc ghi nhận giải ngân trước" });
    }
    if b.periods == 0 {
        return json!({ "error": "periods phải ≥ 1" });
    }
    let rate = b.annual_rate.unwrap_or(src.interest_rate);
    let start = if b.start_date.trim().is_empty() {
        finance::today()
    } else {
        b.start_date.trim().to_string()
    };
    let freq = if b.freq_months == 0 { 1 } else { b.freq_months };
    let method = if b.method.is_empty() {
        "annuity"
    } else {
        &b.method
    };
    let items = generate_schedule(method, principal, rate, b.periods, &start, freq);
    match s.db.replace_schedule(b.source_id, &items) {
        Ok(n) => {
            s.db.log(
                "schedule",
                &format!(
                    "sinh lịch trả nợ {n} kỳ ({method}) cho nguồn #{}",
                    b.source_id
                ),
                &b.source_id.to_string(),
            );
            list_schedule_with_summary(s, b.source_id)
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

fn list_schedule_with_summary(s: &AppState, source_id: i64) -> Value {
    let today = finance::today();
    let items = s.db.list_schedule(Some(source_id), None, &today, 2000);
    let total_p: f64 = items
        .iter()
        .filter(|i| i["status"] != "paid")
        .map(|i| i["principal_due"].as_f64().unwrap_or(0.0))
        .sum();
    let total_i: f64 = items
        .iter()
        .filter(|i| i["status"] != "paid")
        .map(|i| i["interest_due"].as_f64().unwrap_or(0.0))
        .sum();
    json!({
        "ok": true,
        "schedule": items,
        "unpaid_principal": finance::round2(total_p),
        "unpaid_interest": finance::round2(total_i),
    })
}

async fn generate_schedule_h(State(s): State<AppState>, Json(b): Json<GenerateIn>) -> Json<Value> {
    Json(generate_schedule_value(&s, &b))
}

#[derive(Deserialize, Default)]
pub(crate) struct PayIn {
    /// Ghi luôn giao dịch trả gốc/lãi vào sổ cái (mặc định true).
    #[serde(default)]
    pub create_tx: Option<bool>,
    #[serde(default)]
    pub pay_date: String,
}

pub(crate) fn pay_schedule_value(s: &AppState, id: i64, b: &PayIn) -> Value {
    match s
        .db
        .pay_schedule(id, b.create_tx.unwrap_or(true), &b.pay_date)
    {
        Ok(v) => {
            s.db.log(
                "schedule",
                &format!("thanh toán kỳ trả nợ #{id}"),
                &id.to_string(),
            );
            v
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn pay_schedule(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<PayIn>>,
) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(pay_schedule_value(&s, id, &b))
}

// ---- allocations ----

#[derive(Deserialize)]
pub(crate) struct AllocIn {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub target_amount: f64,
}

pub(crate) fn add_alloc_value(s: &AppState, b: &AllocIn) -> Value {
    match s.db.add_alloc(&b.name, &b.description, b.target_amount) {
        Ok(id) => {
            s.db.log(
                "alloc",
                &format!("thêm phân bổ \"{}\"", b.name.trim()),
                &id.to_string(),
            );
            json!({ "ok": true, "alloc_id": id })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn add_alloc(State(s): State<AppState>, Json(b): Json<AllocIn>) -> Json<Value> {
    Json(add_alloc_value(&s, &b))
}

pub(crate) fn list_allocs_value(s: &AppState) -> Value {
    json!({ "allocations": s.db.list_allocs() })
}

async fn list_allocs(State(s): State<AppState>) -> Json<Value> {
    Json(list_allocs_value(&s))
}

async fn update_alloc(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<Value>,
) -> Json<Value> {
    match s.db.update_alloc(id, &patch) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "error": e.to_string() })),
    }
}

// ---- reports / AI ----

#[derive(Deserialize)]
struct CashflowQuery {
    months: Option<i64>,
}

pub(crate) fn cashflow_value(s: &AppState, months: i64) -> Value {
    json!({ "cashflow": s.db.cashflow(months) })
}

async fn cashflow(State(s): State<AppState>, Query(q): Query<CashflowQuery>) -> Json<Value> {
    Json(cashflow_value(&s, q.months.unwrap_or(12)))
}

// ---- smart: đánh giá + mô phỏng ----

pub(crate) fn insight_value(s: &AppState) -> Value {
    crate::insight::evaluate_db(&s.db)
}

async fn insight_h(State(s): State<AppState>) -> Json<Value> {
    Json(insight_value(&s))
}

#[derive(Deserialize, Default)]
pub(crate) struct SimulateIn {
    /// new_loan | early_repay
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub amount: f64,
    #[serde(default)]
    pub annual_rate: f64,
    #[serde(default)]
    pub periods: u32,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub freq_months: u32,
    #[serde(default)]
    pub source_id: Option<i64>,
}

pub(crate) fn simulate_value(s: &AppState, b: &SimulateIn) -> Value {
    let snap = crate::insight::Snapshot::from_db(&s.db, &finance::today());
    match b.scenario.as_str() {
        "new_loan" => crate::insight::simulate_new_loan(
            &snap,
            &crate::insight::NewLoanParams {
                amount: b.amount,
                annual_rate: b.annual_rate,
                periods: b.periods,
                method: b.method.clone(),
                freq_months: b.freq_months,
            },
        ),
        "early_repay" => {
            let Some(sid) = b.source_id else {
                return json!({ "error": "early_repay cần 'source_id'" });
            };
            crate::insight::simulate_early_repay(&snap, sid, b.amount)
        }
        other => {
            json!({ "error": format!("scenario không hợp lệ: '{other}' (new_loan | early_repay)") })
        }
    }
}

async fn simulate_h(State(s): State<AppState>, Json(b): Json<SimulateIn>) -> Json<Value> {
    Json(simulate_value(&s, &b))
}

// ---- mục tiêu & kế hoạch ----

pub(crate) fn goals_list_value(s: &AppState, status: Option<&str>) -> Value {
    let snap = crate::insight::Snapshot::from_db(&s.db, &finance::today());
    let goals: Vec<Value> =
        s.db.list_goals(status)
            .iter()
            .map(|g| {
                let mut e = crate::goals::evaluate_goal(&snap, g);
                let id = g["id"].as_i64().unwrap_or(0);
                e.as_object_mut()
                    .unwrap()
                    .insert("steps".into(), json!(s.db.list_steps(id)));
                e
            })
            .collect();
    json!({ "goals": goals })
}

#[derive(Deserialize)]
pub(crate) struct GoalIn {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub target_amount: f64,
    #[serde(default)]
    pub source_id: Option<i64>,
    #[serde(default)]
    pub deadline: String,
    #[serde(default)]
    pub note: String,
}

pub(crate) fn goal_add_value(s: &AppState, b: &GoalIn) -> Value {
    let today = finance::today();
    let snap = crate::insight::Snapshot::from_db(&s.db, &today);
    // Baseline = the metric's value right now; progress is measured from here.
    let baseline = crate::goals::metric(&snap, &b.kind, b.source_id);
    match s.db.add_goal(
        &b.name,
        &b.kind,
        b.target_amount,
        baseline,
        b.source_id,
        &b.deadline,
        &b.note,
        &today,
    ) {
        Ok(id) => {
            s.db.log(
                "goal",
                &format!("thêm mục tiêu \"{}\" ({})", b.name.trim(), b.kind),
                &id.to_string(),
            );
            match s.db.get_goal(id) {
                Some(g) => json!({ "ok": true, "goal": crate::goals::evaluate_goal(&snap, &g) }),
                None => json!({ "ok": true, "goal_id": id }),
            }
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub(crate) fn goal_update_value(s: &AppState, id: i64, patch: &Value) -> Value {
    match s.db.update_goal(id, patch) {
        Ok(()) => {
            s.db.log("goal", &format!("cập nhật mục tiêu #{id}"), &id.to_string());
            let snap = crate::insight::Snapshot::from_db(&s.db, &finance::today());
            match s.db.get_goal(id) {
                Some(g) => json!({ "ok": true, "goal": crate::goals::evaluate_goal(&snap, &g) }),
                None => json!({ "ok": true }),
            }
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

/// Generate (or regenerate) a plan for a goal: try the AI planner through the
/// bridge, fall back to the deterministic monthly-milestone plan. Open
/// machine-generated steps are replaced; manual and completed steps stay.
pub(crate) async fn goal_plan_value(s: &AppState, id: i64, use_ai: bool) -> Value {
    let Some(goal) = s.db.get_goal(id) else {
        return json!({ "error": format!("mục tiêu #{id} không tồn tại") });
    };
    let snap = crate::insight::Snapshot::from_db(&s.db, &finance::today());
    let eval = crate::goals::evaluate_goal(&snap, &goal);

    let mut source = "auto";
    let mut model = String::new();
    let mut steps: Option<Vec<Value>> = None;
    if use_ai {
        if let Ok((text, m)) = llm::plan_goal(&s.sc, &eval, &insight_value(s)).await {
            if let Some(parsed) = crate::goals::parse_ai_plan(&text) {
                steps = Some(parsed);
                source = "ai";
                model = m;
            }
        }
    }
    let steps = steps.unwrap_or_else(|| crate::goals::fallback_plan(&snap, &eval));

    if let Err(e) = s.db.clear_generated_todo_steps(id) {
        return json!({ "error": e.to_string() });
    }
    for st in &steps {
        let _ = s.db.add_step(
            id,
            st["title"].as_str().unwrap_or(""),
            st["due_date"].as_str().unwrap_or(""),
            st["amount"].as_f64().unwrap_or(0.0),
            source,
        );
    }
    s.db.log(
        "goal",
        &format!(
            "lên kế hoạch {} bước ({source}) cho mục tiêu #{id}",
            steps.len()
        ),
        &id.to_string(),
    );
    json!({ "ok": true, "source": source, "model": model, "steps": s.db.list_steps(id), "goal": eval })
}

#[derive(Deserialize)]
pub(crate) struct StepIn {
    /// add | done | todo | delete
    pub action: String,
    #[serde(default)]
    pub step_id: Option<i64>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub due_date: String,
    #[serde(default)]
    pub amount: f64,
}

pub(crate) fn goal_steps_value(s: &AppState, goal_id: i64, b: &StepIn) -> Value {
    let res = match b.action.as_str() {
        "add" => {
            s.db.add_step(goal_id, &b.title, &b.due_date, b.amount, "manual")
                .map(|_| ())
        }
        "done" | "todo" => match b.step_id {
            Some(sid) => s.db.set_step_status(sid, &b.action),
            None => Err(anyhow::anyhow!("cần 'step_id'")),
        },
        "delete" => match b.step_id {
            Some(sid) => s.db.delete_step(sid).map(|_| ()),
            None => Err(anyhow::anyhow!("cần 'step_id'")),
        },
        other => Err(anyhow::anyhow!(
            "action không hợp lệ: {other} (add|done|todo|delete)"
        )),
    };
    match res {
        Ok(()) => json!({ "ok": true, "steps": s.db.list_steps(goal_id) }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn goals_list(State(s): State<AppState>, Query(q): Query<StatusQuery>) -> Json<Value> {
    Json(goals_list_value(&s, q.status.as_deref()))
}

async fn goal_add(State(s): State<AppState>, Json(b): Json<GoalIn>) -> Json<Value> {
    Json(goal_add_value(&s, &b))
}

async fn goal_update(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<Value>,
) -> Json<Value> {
    Json(goal_update_value(&s, id, &patch))
}

#[derive(Deserialize, Default)]
struct PlanIn {
    #[serde(default)]
    ai: Option<bool>,
}

async fn goal_plan(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<PlanIn>>,
) -> Json<Value> {
    let ai = body.map(|Json(b)| b.ai.unwrap_or(true)).unwrap_or(true);
    Json(goal_plan_value(&s, id, ai).await)
}

async fn goal_steps(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<StepIn>,
) -> Json<Value> {
    Json(goal_steps_value(&s, id, &b))
}

// ---- phân tích sử dụng + đánh giá nguồn ----

pub(crate) fn usage_value(s: &AppState) -> Value {
    crate::insight::usage_analysis(&s.db)
}

async fn usage_h(State(s): State<AppState>) -> Json<Value> {
    Json(usage_value(&s))
}

pub(crate) fn ratings_value(s: &AppState) -> Value {
    crate::insight::source_ratings(&s.db, &finance::today())
}

async fn ratings_h(State(s): State<AppState>) -> Json<Value> {
    Json(ratings_value(&s))
}

#[derive(Deserialize, Default)]
pub(crate) struct AnalyzeIn {
    #[serde(default)]
    pub question: String,
}

pub(crate) async fn analyze_value(s: &AppState, question: &str) -> Value {
    let dash = dashboard_value(s);
    let insight = insight_value(s);
    let (text, model) = llm::analyze(&s.sc, &dash, &insight, question).await;
    s.db.log("ai", "phân tích nguồn vốn", "");
    json!({ "analysis": text, "model": model, "insight": insight })
}

async fn analyze(State(s): State<AppState>, body: Option<Json<AnalyzeIn>>) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(analyze_value(&s, &b.question).await)
}

async fn activity(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "activity": s.db.recent_activity(50) }))
}
