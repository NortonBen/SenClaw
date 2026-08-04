//! HTTP API for the Thinking app. Every handler funnels through small
//! `*_value` helpers that the MCP server ([`crate::mcp`]) reuses, so REST and
//! agent tools always behave identically. All analysis data stays in the local
//! SQLite DB; the only outbound call is the SenClaw LLM bridge.

use crate::db::Db;
use crate::llm;
use crate::logic;
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
    let db = Arc::new(Db::open_default().expect("open thinking db"));
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
        .route("/activity", get(activity))
        .route("/problems", get(list_problems).post(add_problem))
        .route("/problems/:id", get(get_problem).post(update_problem))
        .route("/problems/:id/delete", post(delete_problem))
        .route("/problems/:id/w", post(set_5w))
        .route("/problems/:id/w/generate", post(generate_5w))
        .route("/problems/:id/hats", post(set_hats))
        .route("/problems/:id/hats/generate", post(generate_hats))
        .route("/problems/:id/solutions", post(add_solution))
        .route("/problems/:id/solutions/generate", post(generate_solutions))
        .route("/problems/:id/compare", get(compare))
        .route("/problems/:id/decide", post(decide))
        .route("/problems/:id/analyze", post(analyze))
        .route("/problems/:id/report", get(report))
        .route("/solutions/:id", post(update_solution))
        .route("/solutions/:id/delete", post(delete_solution))
        .route("/solutions/:id/evaluate", post(evaluate_solution))
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
    let dash = s.db.dashboard();
    json!({
        "ok": true,
        "app": "thinking",
        "problems_total": dash["problems_total"],
        "open": dash["by_status"]["open"],
        "analyzing": dash["by_status"]["analyzing"],
        "decided": dash["by_status"]["decided"],
        "attention_count": dash["attention"].as_array().map(|a| a.len()).unwrap_or(0),
    })
}

async fn status(State(s): State<AppState>) -> Json<Value> {
    Json(status_value(&s))
}

pub(crate) fn dashboard_value(s: &AppState) -> Value {
    s.db.dashboard()
}

async fn dashboard(State(s): State<AppState>) -> Json<Value> {
    Json(dashboard_value(&s))
}

async fn activity(State(s): State<AppState>) -> Json<Value> {
    Json(json!({ "activity": s.db.recent_activity(50) }))
}

// ---- problems ----

#[derive(Deserialize, Default)]
pub(crate) struct ProblemIn {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub tags: String,
}

pub(crate) fn add_problem_value(s: &AppState, b: &ProblemIn) -> Value {
    match s.db.add_problem(
        &b.title,
        &b.description,
        &b.context,
        &b.goal,
        &b.priority,
        &b.tags,
    ) {
        Ok(id) => {
            s.db.log(
                "problem",
                &format!("tạo vấn đề \"{}\"", b.title.trim()),
                &id.to_string(),
            );
            json!({ "ok": true, "problem": s.db.problem_brief(id) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn add_problem(State(s): State<AppState>, Json(b): Json<ProblemIn>) -> Json<Value> {
    Json(add_problem_value(&s, &b))
}

#[derive(Deserialize)]
struct ProblemQuery {
    q: Option<String>,
    status: Option<String>,
    limit: Option<i64>,
}

pub(crate) fn list_problems_value(
    s: &AppState,
    q: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> Value {
    json!({ "problems": s.db.list_problems(q, status, limit) })
}

async fn list_problems(State(s): State<AppState>, Query(q): Query<ProblemQuery>) -> Json<Value> {
    Json(list_problems_value(
        &s,
        q.q.as_deref(),
        q.status.as_deref(),
        q.limit.unwrap_or(100),
    ))
}

pub(crate) fn get_problem_value(s: &AppState, id: i64) -> Value {
    match s.db.get_problem(id) {
        Some(d) => d,
        None => json!({ "error": format!("vấn đề #{id} không tồn tại") }),
    }
}

async fn get_problem(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(get_problem_value(&s, id))
}

pub(crate) fn update_problem_value(s: &AppState, id: i64, patch: &Value) -> Value {
    match s.db.update_problem(id, patch) {
        Ok(()) => {
            s.db.log(
                "problem",
                &format!("cập nhật vấn đề #{id}"),
                &id.to_string(),
            );
            json!({ "ok": true, "problem": s.db.problem_brief(id) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn update_problem(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<Value>,
) -> Json<Value> {
    Json(update_problem_value(&s, id, &patch))
}

pub(crate) fn delete_problem_value(s: &AppState, id: i64) -> Value {
    match s.db.delete_problem(id) {
        Ok(v) => {
            s.db.log(
                "problem",
                &format!("xoá vấn đề \"{}\"", v["deleted"].as_str().unwrap_or("?")),
                &id.to_string(),
            );
            v
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn delete_problem(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(delete_problem_value(&s, id))
}

// ---- 5W ----

/// Manual set: body chứa bất kỳ khóa nào trong who/what/when/where/why.
pub(crate) fn set_5w_value(s: &AppState, id: i64, body: &Value, source: &str) -> Value {
    let mut set = Vec::new();
    for w in logic::W_KEYS {
        if let Some(content) = body.get(w).and_then(|v| v.as_str()) {
            if let Err(e) = s.db.set_w(id, w, content, source) {
                return json!({ "error": e.to_string() });
            }
            set.push(w);
        }
    }
    if set.is_empty() {
        return json!({ "error": "không có khóa 5W nào trong body (who|what|when|where|why)" });
    }
    s.db.log(
        "5w",
        &format!("điền 5W [{}] cho vấn đề #{id}", set.join(", ")),
        &id.to_string(),
    );
    get_problem_value(s, id)
}

async fn set_5w(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Json<Value> {
    Json(set_5w_value(&s, id, &body, "user"))
}

#[derive(Deserialize, Default)]
pub(crate) struct GenerateIn {
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub hat: String,
    #[serde(default)]
    pub count: Option<usize>,
    #[serde(default)]
    pub question: String,
}

/// AI điền 5W. Mặc định chỉ điền ô TRỐNG (không ghi đè phân tích người dùng
/// đã viết); `force = true` ghi đè cả năm ô.
pub(crate) async fn generate_5w_value(s: &AppState, id: i64, force: bool) -> Value {
    let Some(detail) = s.db.get_problem(id) else {
        return json!({ "error": format!("vấn đề #{id} không tồn tại") });
    };
    let targets: Vec<&str> = logic::W_KEYS
        .iter()
        .copied()
        .filter(|w| {
            force
                || detail["five_w"][*w]["content"]
                    .as_str()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
        })
        .collect();
    if targets.is_empty() {
        return json!({ "ok": true, "filled": [], "note": "5W đã đầy đủ — dùng force=true nếu muốn AI viết lại", "detail": detail });
    }
    match llm::gen_5w(&s.sc, &detail).await {
        Ok((v, model)) => {
            for w in &targets {
                if let Some(c) = v[*w].as_str() {
                    if let Err(e) = s.db.set_w(id, w, c, "ai") {
                        return json!({ "error": e.to_string() });
                    }
                }
            }
            s.db.log(
                "ai",
                &format!("AI điền 5W [{}] cho vấn đề #{id}", targets.join(", ")),
                &id.to_string(),
            );
            json!({ "ok": true, "filled": targets, "model": model, "detail": get_problem_value(s, id) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn generate_5w(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<GenerateIn>>,
) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(generate_5w_value(&s, id, b.force).await)
}

// ---- hats ----

/// Manual set: body chứa bất kỳ khóa nào trong white/red/black/yellow/green/blue.
pub(crate) fn set_hats_value(s: &AppState, id: i64, body: &Value, source: &str) -> Value {
    let mut set = Vec::new();
    for h in logic::HAT_KEYS {
        if let Some(content) = body.get(h).and_then(|v| v.as_str()) {
            if let Err(e) = s.db.set_hat(id, h, content, source) {
                return json!({ "error": e.to_string() });
            }
            set.push(h);
        }
    }
    if set.is_empty() {
        return json!({ "error": "không có khóa mũ nào trong body (white|red|black|yellow|green|blue)" });
    }
    s.db.log(
        "hat",
        &format!("điền mũ [{}] cho vấn đề #{id}", set.join(", ")),
        &id.to_string(),
    );
    get_problem_value(s, id)
}

async fn set_hats(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Json<Value> {
    Json(set_hats_value(&s, id, &body, "user"))
}

/// AI chạy 6 mũ. `hat` rỗng → cả sáu; mặc định chỉ điền mũ trống, `force`
/// ghi đè.
pub(crate) async fn generate_hats_value(s: &AppState, id: i64, hat: &str, force: bool) -> Value {
    let Some(detail) = s.db.get_problem(id) else {
        return json!({ "error": format!("vấn đề #{id} không tồn tại") });
    };
    let hat = hat.trim();
    let only = if hat.is_empty() { None } else { Some(hat) };
    if let Some(h) = only {
        if !logic::HAT_KEYS.contains(&h) {
            return json!({ "error": format!("mũ không hợp lệ: {h} (white|red|black|yellow|green|blue)") });
        }
    }
    let targets: Vec<&str> = match only {
        Some(h) => vec![h],
        None => logic::HAT_KEYS
            .iter()
            .copied()
            .filter(|h| {
                force
                    || detail["hats"][*h]["content"]
                        .as_str()
                        .unwrap_or("")
                        .trim()
                        .is_empty()
            })
            .collect(),
    };
    if targets.is_empty() {
        return json!({ "ok": true, "filled": [], "note": "6 mũ đã đầy đủ — dùng force=true nếu muốn AI viết lại", "detail": detail });
    }
    match llm::gen_hats(&s.sc, &detail, only).await {
        Ok((v, model)) => {
            for h in &targets {
                if let Some(c) = v[*h].as_str() {
                    if let Err(e) = s.db.set_hat(id, h, c, "ai") {
                        return json!({ "error": e.to_string() });
                    }
                }
            }
            s.db.log(
                "ai",
                &format!("AI đội mũ [{}] cho vấn đề #{id}", targets.join(", ")),
                &id.to_string(),
            );
            json!({ "ok": true, "filled": targets, "model": model, "detail": get_problem_value(s, id) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn generate_hats(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<GenerateIn>>,
) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(generate_hats_value(&s, id, &b.hat, b.force).await)
}

// ---- solutions ----

#[derive(Deserialize, Default)]
pub(crate) struct SolutionIn {
    pub title: String,
    #[serde(default)]
    pub description: String,
}

pub(crate) fn add_solution_value(
    s: &AppState,
    problem_id: i64,
    b: &SolutionIn,
    source: &str,
) -> Value {
    match s
        .db
        .add_solution(problem_id, &b.title, &b.description, source)
    {
        Ok(id) => {
            s.db.log(
                "solution",
                &format!(
                    "thêm giải pháp \"{}\" cho vấn đề #{problem_id}",
                    b.title.trim()
                ),
                &id.to_string(),
            );
            json!({ "ok": true, "solution": s.db.get_solution(id) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn add_solution(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<SolutionIn>,
) -> Json<Value> {
    Json(add_solution_value(&s, id, &b, "user"))
}

pub(crate) async fn generate_solutions_value(s: &AppState, id: i64, count: usize) -> Value {
    let Some(detail) = s.db.get_problem(id) else {
        return json!({ "error": format!("vấn đề #{id} không tồn tại") });
    };
    match llm::gen_solutions(&s.sc, &detail, count).await {
        Ok((sols, model)) => {
            let mut added = Vec::new();
            for (title, desc) in sols {
                match s.db.add_solution(id, &title, &desc, "ai") {
                    Ok(sid) => added.push(json!({ "id": sid, "title": title })),
                    Err(e) => return json!({ "error": e.to_string() }),
                }
            }
            s.db.log(
                "ai",
                &format!("AI đề xuất {} giải pháp cho vấn đề #{id}", added.len()),
                &id.to_string(),
            );
            json!({ "ok": true, "added": added, "model": model, "solutions": s.db.list_solutions(id) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn generate_solutions(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<GenerateIn>>,
) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(generate_solutions_value(&s, id, b.count.unwrap_or(3)).await)
}

pub(crate) fn update_solution_value(s: &AppState, id: i64, patch: &Value) -> Value {
    match s.db.update_solution(id, patch) {
        Ok(()) => {
            s.db.log(
                "solution",
                &format!("cập nhật giải pháp #{id}"),
                &id.to_string(),
            );
            json!({ "ok": true, "solution": s.db.get_solution(id) })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn update_solution(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(patch): Json<Value>,
) -> Json<Value> {
    Json(update_solution_value(&s, id, &patch))
}

pub(crate) fn delete_solution_value(s: &AppState, id: i64) -> Value {
    match s.db.delete_solution(id) {
        Ok(v) => {
            s.db.log(
                "solution",
                &format!("xoá giải pháp \"{}\"", v["deleted"].as_str().unwrap_or("?")),
                &id.to_string(),
            );
            v
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn delete_solution(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(delete_solution_value(&s, id))
}

#[derive(Deserialize, Default)]
pub(crate) struct EvaluateIn {
    pub benefit: Option<f64>,
    pub risk: Option<f64>,
    pub feasibility: Option<f64>,
    pub effort: Option<f64>,
    #[serde(default)]
    pub verdict: String,
}

/// Đánh giá một giải pháp. Đủ cả 4 điểm trong body → chấm tay; thiếu bất kỳ
/// điểm nào → AI chấm qua bridge.
pub(crate) async fn evaluate_solution_value(s: &AppState, id: i64, b: &EvaluateIn) -> Value {
    let Some(sol) = s.db.get_solution(id) else {
        return json!({ "error": format!("giải pháp #{id} không tồn tại") });
    };
    if let (Some(ben), Some(risk), Some(feas), Some(eff)) =
        (b.benefit, b.risk, b.feasibility, b.effort)
    {
        return match s
            .db
            .set_evaluation(id, ben, risk, feas, eff, &b.verdict, "", "user")
        {
            Ok(v) => {
                s.db.log(
                    "eval",
                    &format!("chấm tay giải pháp #{id}"),
                    &id.to_string(),
                );
                json!({ "ok": true, "solution": v })
            }
            Err(e) => json!({ "error": e.to_string() }),
        };
    }
    let problem_id = sol["problem_id"].as_i64().unwrap_or(0);
    let Some(detail) = s.db.get_problem(problem_id) else {
        return json!({ "error": format!("vấn đề #{problem_id} không tồn tại") });
    };
    match llm::evaluate_solution(&s.sc, &detail, &sol).await {
        Ok(((ben, risk, feas, eff, verdict, det), model)) => {
            match s
                .db
                .set_evaluation(id, ben, risk, feas, eff, &verdict, &det, "ai")
            {
                Ok(v) => {
                    s.db.log(
                        "ai",
                        &format!("AI đánh giá giải pháp #{id}"),
                        &id.to_string(),
                    );
                    json!({ "ok": true, "solution": v, "model": model })
                }
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn evaluate_solution(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<EvaluateIn>>,
) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(evaluate_solution_value(&s, id, &b).await)
}

// ---- compare / decide / analyze / report ----

pub(crate) fn compare_value(s: &AppState, id: i64) -> Value {
    match s.db.compare(id) {
        Ok(v) => v,
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn compare(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(compare_value(&s, id))
}

#[derive(Deserialize, Default)]
pub(crate) struct DecideIn {
    pub solution_id: i64,
    #[serde(default)]
    pub rationale: String,
}

pub(crate) fn decide_value(s: &AppState, id: i64, b: &DecideIn) -> Value {
    match s.db.decide(id, b.solution_id, &b.rationale) {
        Ok(v) => {
            s.db.log(
                "decide",
                &format!("chốt giải pháp #{} cho vấn đề #{id}", b.solution_id),
                &id.to_string(),
            );
            v
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn decide(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(b): Json<DecideIn>,
) -> Json<Value> {
    Json(decide_value(&s, id, &b))
}

/// Pipeline "phân tích toàn diện": chạy tuần tự các bước còn thiếu —
/// 5W → 6 mũ → đề xuất giải pháp (nếu chưa có) → chấm mọi giải pháp chưa chấm
/// → mũ Xanh Dương tổng hợp (lưu vào `synthesis`). Vấn đề `open` được chuyển
/// sang `analyzing`. Từng bước lỗi thì dừng và báo rõ đã xong tới đâu.
pub(crate) async fn analyze_value(s: &AppState, id: i64, question: &str) -> Value {
    if s.db.get_problem(id).is_none() {
        return json!({ "error": format!("vấn đề #{id} không tồn tại") });
    }
    let mut steps: Vec<Value> = Vec::new();
    let fail = |steps: &[Value], stage: &str, e: String| {
        json!({
            "error": format!("lỗi ở bước {stage}: {e}"),
            "steps_done": steps,
            "note": "các bước đã xong vẫn được lưu — chạy lại think_analyze để tiếp tục từ chỗ dở",
        })
    };

    // 1. 5W (chỉ ô trống)
    let r = generate_5w_value(s, id, false).await;
    if let Some(e) = r["error"].as_str() {
        return fail(&steps, "5W", e.to_string());
    }
    steps.push(json!({ "step": "5w", "filled": r["filled"] }));

    // 2. 6 mũ (chỉ mũ trống)
    let r = generate_hats_value(s, id, "", false).await;
    if let Some(e) = r["error"].as_str() {
        return fail(&steps, "6 mũ", e.to_string());
    }
    steps.push(json!({ "step": "hats", "filled": r["filled"] }));

    // 3. Giải pháp (chỉ khi chưa có cái nào)
    let detail = s.db.get_problem(id).unwrap_or_default();
    let existing = detail["solutions"].as_array().map(|a| a.len()).unwrap_or(0);
    if existing == 0 {
        let r = generate_solutions_value(s, id, 3).await;
        if let Some(e) = r["error"].as_str() {
            return fail(&steps, "đề xuất giải pháp", e.to_string());
        }
        steps.push(json!({ "step": "solutions", "added": r["added"] }));
    } else {
        steps.push(
            json!({ "step": "solutions", "note": format!("giữ {existing} giải pháp sẵn có") }),
        );
    }

    // 4. Chấm điểm mọi giải pháp chưa có đánh giá
    let mut evaluated = 0;
    for sol in s.db.list_solutions(id) {
        if sol["evaluation"].is_null() {
            let sid = sol["id"].as_i64().unwrap_or(0);
            let r = evaluate_solution_value(s, sid, &EvaluateIn::default()).await;
            if let Some(e) = r["error"].as_str() {
                return fail(&steps, &format!("đánh giá giải pháp #{sid}"), e.to_string());
            }
            evaluated += 1;
        }
    }
    steps.push(json!({ "step": "evaluate", "evaluated": evaluated }));

    // 5. Tổng hợp mũ Xanh Dương + chuyển trạng thái
    let detail = s.db.get_problem(id).unwrap_or_default();
    let cmp = compare_value(s, id);
    let (synthesis, model) = llm::synthesize(&s.sc, &detail, &cmp, question).await;
    let mut patch = json!({ "synthesis": synthesis });
    if detail["problem"]["status"] == "open" {
        patch["status"] = json!("analyzing");
    }
    if let Err(e) = s.db.update_problem(id, &patch) {
        return fail(&steps, "lưu tổng hợp", e.to_string());
    }
    steps.push(json!({ "step": "synthesis", "model": model }));
    s.db.log(
        "ai",
        &format!("phân tích toàn diện vấn đề #{id}"),
        &id.to_string(),
    );

    json!({
        "ok": true,
        "steps": steps,
        "synthesis": synthesis,
        "compare": compare_value(s, id),
        "detail": get_problem_value(s, id),
    })
}

async fn analyze(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    body: Option<Json<GenerateIn>>,
) -> Json<Value> {
    let b = body.map(|Json(x)| x).unwrap_or_default();
    Json(analyze_value(&s, id, &b.question).await)
}

pub(crate) fn report_value(s: &AppState, id: i64) -> Value {
    match s.db.report(id) {
        Ok(md) => json!({ "report": md }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

async fn report(State(s): State<AppState>, Path(id): Path<i64>) -> Json<Value> {
    Json(report_value(&s, id))
}
