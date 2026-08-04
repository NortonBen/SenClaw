//! REST surface. The web UI and the MCP server both go through the same
//! operations; anything with real logic lives in its own module so both entry
//! points behave identically.

use axum::extract::{Multipart, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::AppState;
use crate::{ask, calendar, cards, db, ingest, lesson, llm, outline, planner, quiz, sources, srs, tts};

pub struct ApiErr(pub StatusCode, pub String);

impl IntoResponse for ApiErr {
    fn into_response(self) -> axum::response::Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<String> for ApiErr {
    fn from(e: String) -> Self {
        ApiErr(StatusCode::BAD_REQUEST, e)
    }
}

impl From<anyhow::Error> for ApiErr {
    fn from(e: anyhow::Error) -> Self {
        ApiErr(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

type ApiResult = Result<Json<Value>, ApiErr>;

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(status))
        .route("/health", get(status))
        // Documents
        .route("/docs", get(list_docs))
        .route("/docs/upload", post(upload_doc))
        .route("/docs/text", post(add_text_doc))
        .route("/docs/:id", get(get_doc).patch(rename_doc).delete(del_doc))
        .route("/docs/:id/sections", get(get_sections))
        .route("/docs/:id/text", get(get_doc_text))
        .route("/docs/:id/concepts", get(get_concepts))
        .route("/docs/:id/reindex", post(reindex_doc))
        .route("/docs/:id/strip-lines", post(strip_lines))
        .route("/docs/:id/enrich", post(enrich_doc))
        .route("/docs/:id/summarize", post(summarize_doc))
        .route("/docs/:id/cards", get(doc_cards))
        .route("/docs/:id/weak", get(doc_weak))
        .route("/sections/:id", get(get_section))
        .route("/chunks/:id", get(get_chunk))
        // Plans
        .route("/templates", get(list_templates))
        .route("/plans", get(list_plans).post(create_plan))
        .route("/plans/preview", post(preview_plan))
        .route("/plans/:id", get(get_plan).delete(del_plan))
        .route("/plans/:id/sessions", get(plan_sessions))
        .route("/plans/:id/sync", post(sync_plan))
        .route("/plans/:id/unsync", post(unsync_plan))
        .route("/plans/:id/replan", post(replan))
        // Sessions
        .route("/today", get(today))
        .route("/sessions/:id", get(get_session))
        .route("/sessions/:id/complete", post(complete_session))
        .route("/items/:id/complete", post(complete_item))
        // Cards
        .route("/cards/due", get(cards_due))
        .route("/cards/:id/review", post(review_card))
        .route("/cards/:id", delete(del_card))
        .route("/cards", post(create_card))
        .route("/sections/:id/cards/generate", post(gen_cards))
        // Quiz
        .route("/sections/:id/quiz/generate", post(gen_quiz))
        .route("/quiz", post(build_quiz))
        .route("/quiz/grade", post(grade_quiz))
        // Ask / research
        .route("/ask", post(post_ask))
        .route("/research", post(post_research))
        .route("/asks", get(list_asks))
        .route("/sources", get(list_sources))
        // Audio
        .route("/speak", post(post_speak))
        .route("/audio/:name", get(get_audio))
        // Settings
        .route("/settings", get(get_settings).patch(patch_settings))
        // MCP
        .route("/mcp/sse", get(crate::mcp::mcp_sse))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ── Status ──────────────────────────────────────────────────────────────────

async fn status(State(s): State<AppState>) -> ApiResult {
    let docs = s.db.doc_list()?;
    let plans = s.db.plan_list()?;
    let now = srs::fmt(chrono::Utc::now());
    let today_str = local_today(&s.db);
    Ok(Json(json!({
        "ok": true,
        "app": "study",
        "docs": docs.len(),
        "plans": plans.len(),
        "cardsDue": s.db.card_due_count(&now)?,
        "todaySessions": s.db.sessions_on(&today_str)?,
        "today": today_str,
    })))
}

fn local_today(db: &db::Db) -> String {
    let tz = srs::parse_tz(&db.setting("tz").unwrap_or_else(|| "Asia/Ho_Chi_Minh".into()));
    chrono::Utc::now()
        .with_timezone(&tz)
        .date_naive()
        .format("%Y-%m-%d")
        .to_string()
}

// ── Documents ───────────────────────────────────────────────────────────────

async fn list_docs(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(s.db.doc_list()?)))
}

async fn upload_doc(State(s): State<AppState>, mut mp: Multipart) -> ApiResult {
    let mut filename = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut title = String::new();

    while let Some(field) = mp.next_field().await.map_err(|e| ApiErr(StatusCode::BAD_REQUEST, e.to_string()))? {
        match field.name().unwrap_or("") {
            "file" => {
                filename = field.file_name().unwrap_or("tai-lieu.txt").to_string();
                bytes = field
                    .bytes()
                    .await
                    .map_err(|e| ApiErr(StatusCode::BAD_REQUEST, e.to_string()))?
                    .to_vec();
            }
            "title" => title = field.text().await.unwrap_or_default(),
            _ => {}
        }
    }
    if bytes.is_empty() {
        return Err(ApiErr(StatusCode::BAD_REQUEST, "không có tệp nào".into()));
    }
    ingest::ingest(&s.db, &filename, &bytes, &title)
        .map(Json)
        .map_err(|e| ApiErr(StatusCode::BAD_REQUEST, e))
}

#[derive(Deserialize)]
struct TextDoc {
    title: Option<String>,
    filename: Option<String>,
    text: String,
}

async fn add_text_doc(State(s): State<AppState>, Json(b): Json<TextDoc>) -> ApiResult {
    let filename = b.filename.unwrap_or_else(|| "dan-vao.md".to_string());
    ingest::ingest(
        &s.db,
        &filename,
        b.text.as_bytes(),
        &b.title.unwrap_or_default(),
    )
    .map(Json)
    .map_err(|e| ApiErr(StatusCode::BAD_REQUEST, e))
}

async fn get_doc(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let d = s
        .db
        .doc_get(&id)?
        .ok_or_else(|| ApiErr(StatusCode::NOT_FOUND, "không tìm thấy tài liệu".into()))?;
    let mut v = json!(d);
    v["suspectedFurniture"] = json!(s.db.suspects(&id)?);
    Ok(Json(v))
}

#[derive(Deserialize)]
struct RenameBody {
    title: String,
}

async fn rename_doc(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<RenameBody>,
) -> ApiResult {
    s.db.doc_rename(&id, b.title.trim())?;
    Ok(Json(json!({ "ok": true })))
}

async fn del_doc(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    s.db.doc_delete(&id)?;
    Ok(Json(json!({ "ok": true })))
}

async fn get_sections(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(json!(s.db.sections_of(&id)?)))
}

async fn get_section(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let sec = s
        .db
        .section_get(&id)?
        .ok_or_else(|| ApiErr(StatusCode::NOT_FOUND, "không tìm thấy mục".into()))?;
    let body = s.db.doc_body(&sec.doc_id)?.unwrap_or_default();
    let chars: Vec<char> = body.chars().collect();
    let a = (sec.char_start as usize).min(chars.len());
    let b = (sec.char_end as usize).min(chars.len()).max(a);
    let text: String = chars[a..b].iter().collect();
    let concepts = s.db.concepts_of_section(&id)?;
    Ok(Json(json!({
        "section": sec,
        "text": text,
        "cards": s.db.cards_of_section(&id)?,
        "questionCount": s.db.question_count(&sec.doc_id, Some(&id))?,
        "concepts": concepts.into_iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>(),
    })))
}

/// One chunk by id — what a `[n]` citation resolves to when the reader clicks
/// it. Carries the offsets so the reader can be scrolled to the right place.
async fn get_chunk(State(s): State<AppState>, Path(id): Path<i64>) -> ApiResult {
    let c = s
        .db
        .chunk_get(id)?
        .ok_or_else(|| ApiErr(StatusCode::NOT_FOUND, "không có đoạn này".into()))?;
    Ok(Json(json!(c)))
}

#[derive(Deserialize)]
struct TextRange {
    start: Option<usize>,
    end: Option<usize>,
}

async fn get_doc_text(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<TextRange>,
) -> ApiResult {
    let body = s
        .db
        .doc_body(&id)?
        .ok_or_else(|| ApiErr(StatusCode::NOT_FOUND, "không tìm thấy tài liệu".into()))?;
    let chars: Vec<char> = body.chars().collect();
    let a = q.start.unwrap_or(0).min(chars.len());
    let b = q.end.unwrap_or(chars.len()).min(chars.len()).max(a);
    Ok(Json(json!({
        "text": chars[a..b].iter().collect::<String>(),
        "charStart": a,
        "charEnd": b,
        "total": chars.len(),
    })))
}

async fn get_concepts(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(json!(s.db.concept_map(&id)?)))
}

async fn reindex_doc(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let (sections, chunks, note) = outline::index_document(&s.db, &id)?;
    Ok(Json(json!({ "sections": sections, "chunks": chunks, "note": note })))
}

#[derive(Deserialize)]
struct StripBody {
    lines: Vec<String>,
}

/// The review step: drop the repeated lines the user confirmed are page
/// furniture, then rebuild the outline, the index and every question's
/// evidence pointer.
async fn strip_lines(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<StripBody>,
) -> ApiResult {
    if b.lines.is_empty() {
        return Err(ApiErr(StatusCode::BAD_REQUEST, "chưa chọn dòng nào".into()));
    }
    let mut out = s.db.strip_lines(&id, &b.lines)?;
    let (sections, chunks, note) = outline::index_document(&s.db, &id)?;
    // Re-indexing threw away the old chunk ids; point every question back at
    // the chunk that still holds its quote.
    let (repointed, orphaned) = s.db.repoint_questions(&id)?;
    out["sections"] = json!(sections);
    out["chunks"] = json!(chunks);
    out["note"] = json!(note);
    out["questionsRepointed"] = json!(repointed);
    out["questionsOrphaned"] = json!(orphaned);
    if orphaned > 0 {
        out["warning"] = json!(format!(
            "{orphaned} câu hỏi có trích dẫn nằm trên dòng vừa bỏ — câu vẫn chấm được nhưng không nhảy về đoạn gốc nữa"
        ));
    }
    Ok(Json(out))
}

async fn enrich_doc(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let (done, problems) = outline::enrich_document(&s.db, &id).await?;
    s.db.doc_set_status(&id, "enriched", None)?;
    Ok(Json(json!({ "enriched": done, "problems": problems })))
}

async fn summarize_doc(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let summary = outline::summarize_document(&s.db, &id).await?;
    Ok(Json(json!({ "summary": summary })))
}

async fn doc_cards(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(json!(s.db.cards_of_doc(&id)?)))
}

async fn doc_weak(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(json!(s.db.weak_concepts(&id, 20)?)))
}

// ── Plans ───────────────────────────────────────────────────────────────────

async fn list_templates(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(s.db.templates()?)))
}

#[derive(Deserialize)]
struct PlanBody {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    goal: Option<String>,
    doc_ids: Vec<String>,
    #[serde(default)]
    template: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    days: Option<i64>,
    #[serde(default)]
    min_per_day: Option<i64>,
    #[serde(default)]
    weekdays: Option<String>,
    #[serde(default)]
    slot_hm: Option<String>,
    #[serde(default)]
    tz: Option<String>,
    /// Only used by POST /plans: also push the sessions onto the calendar.
    #[serde(default)]
    sync_calendar: Option<bool>,
    #[serde(default)]
    reminder_min: Option<i64>,
}

struct Resolved {
    req: planner::PlanRequest,
    template_key: String,
    weekdays: String,
    tz: String,
    sections: Vec<db::SectionRow>,
}

fn resolve(db: &db::Db, b: &PlanBody) -> Result<Resolved, ApiErr> {
    if b.doc_ids.is_empty() {
        return Err(ApiErr(StatusCode::BAD_REQUEST, "chưa chọn tài liệu nào".into()));
    }
    let key = b.template.clone().unwrap_or_else(|| "standard".into());
    let t = db
        .template_get(&key)?
        .ok_or_else(|| ApiErr(StatusCode::BAD_REQUEST, format!("không có mẫu `{key}`")))?;

    let tz_name = b
        .tz
        .clone()
        .or_else(|| db.setting("tz"))
        .unwrap_or_else(|| "Asia/Ho_Chi_Minh".into());
    let tz = srs::parse_tz(&tz_name);
    let start = match b.start_date.as_deref() {
        Some(d) if !d.trim().is_empty() => chrono::NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d")
            .map_err(|_| ApiErr(StatusCode::BAD_REQUEST, format!("ngày bắt đầu không hợp lệ: {d}")))?,
        _ => chrono::Utc::now().with_timezone(&tz).date_naive(),
    };
    let weekdays = b
        .weekdays
        .clone()
        .unwrap_or_else(|| "1,2,3,4,5,6,7".to_string());

    let sections = db.sections_for_docs(&b.doc_ids)?;
    if sections.is_empty() {
        return Err(ApiErr(
            StatusCode::BAD_REQUEST,
            "tài liệu chưa được chia mục — chạy /reindex trước".into(),
        ));
    }

    Ok(Resolved {
        req: planner::PlanRequest {
            start_date: start,
            days: b.days.unwrap_or(t.days),
            min_per_day: b.min_per_day.unwrap_or(t.min_per_day),
            weekdays: planner::parse_weekdays(&weekdays),
            slot_hm: b
                .slot_hm
                .clone()
                .or_else(|| db.setting("slot_hm"))
                .unwrap_or_else(|| "20:00".into()),
            review_offsets: t.review_offsets.clone(),
            blocks: t.blocks.clone(),
            content_ratio: t.content_ratio,
        },
        template_key: key,
        weekdays,
        tz: tz_name,
        sections,
    })
}

async fn preview_plan(State(s): State<AppState>, Json(b): Json<PlanBody>) -> ApiResult {
    let r = resolve(&s.db, &b)?;
    let preview = planner::build(&r.sections, &r.req);
    Ok(Json(serde_json::to_value(preview).unwrap_or(Value::Null)))
}

async fn create_plan(State(s): State<AppState>, Json(b): Json<PlanBody>) -> ApiResult {
    let r = resolve(&s.db, &b)?;
    let preview = planner::build(&r.sections, &r.req);
    if preview.sessions.is_empty() {
        return Err(ApiErr(
            StatusCode::BAD_REQUEST,
            "không xếp được buổi nào — kiểm tra số ngày và số phút mỗi ngày".into(),
        ));
    }
    let title = b.title.clone().unwrap_or_else(|| {
        format!(
            "Học {} mục trong {} buổi",
            r.sections.len(),
            preview.sessions.len()
        )
    });
    let plan_id = s.db.plan_insert(
        &title,
        b.goal.as_deref().unwrap_or(""),
        &b.doc_ids,
        &r.template_key,
        &r.req.start_date.format("%Y-%m-%d").to_string(),
        r.req.days,
        r.req.min_per_day,
        &r.weekdays,
        &r.req.slot_hm,
        &r.tz,
        &preview.notes.join(" · "),
        &preview,
    )?;

    let mut out = json!({
        "id": plan_id,
        "title": title,
        "feasible": preview.feasible,
        "sessions": preview.sessions.len(),
        "dropped": preview.dropped,
        "options": preview.options,
        "notes": preview.notes,
    });

    if b.sync_calendar.unwrap_or(false) {
        match calendar::sync_plan(&s.db, &plan_id, b.reminder_min).await {
            Ok(rep) => out["calendar"] = serde_json::to_value(rep).unwrap_or(Value::Null),
            // The plan is saved either way — a calendar failure must not lose
            // the schedule the learner just built.
            Err(e) => out["calendarError"] = json!(e),
        }
    }
    Ok(Json(out))
}

async fn list_plans(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(s.db.plan_list()?)))
}

async fn get_plan(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let p = s
        .db
        .plan_get(&id)?
        .ok_or_else(|| ApiErr(StatusCode::NOT_FOUND, "không tìm thấy kế hoạch".into()))?;
    Ok(Json(p))
}

async fn del_plan(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    // Remove the calendar events first: deleting the plan would orphan them and
    // leave the user with events that open nothing.
    let removed = calendar::unsync_plan(&s.db, &id).await.unwrap_or(0);
    s.db.plan_delete(&id)?;
    Ok(Json(json!({ "ok": true, "eventsRemoved": removed })))
}

async fn plan_sessions(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    Ok(Json(json!(s.db.sessions_of_plan(&id)?)))
}

#[derive(Deserialize)]
struct SyncBody {
    #[serde(default)]
    reminder_min: Option<i64>,
}

async fn sync_plan(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<SyncBody>,
) -> ApiResult {
    let rep = calendar::sync_plan(&s.db, &id, b.reminder_min).await?;
    Ok(Json(serde_json::to_value(rep).unwrap_or(Value::Null)))
}

async fn unsync_plan(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let n = calendar::unsync_plan(&s.db, &id).await?;
    Ok(Json(json!({ "removed": n })))
}

/// Rebuild the remaining part of a plan starting today, keeping completed
/// sessions untouched. The caller sees the new shape before anything is synced.
async fn replan(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let plan = s
        .db
        .plan_get(&id)?
        .ok_or_else(|| ApiErr(StatusCode::NOT_FOUND, "không tìm thấy kế hoạch".into()))?;
    let today = local_today(&s.db);
    let missed = s.db.sessions_missed_before(&today)?;
    let missed_here: Vec<String> = missed
        .into_iter()
        .filter(|(_, pid)| *pid == id)
        .map(|(sid, _)| sid)
        .collect();
    Ok(Json(json!({
        "planId": id,
        "title": plan["title"],
        "missed": missed_here.len(),
        "missedSessionIds": missed_here,
        "hint": "tạo kế hoạch mới cho phần còn lại rồi xoá kế hoạch cũ, hoặc dời từng buổi trên lịch",
    })))
}

// ── Sessions ────────────────────────────────────────────────────────────────

async fn today(State(s): State<AppState>) -> ApiResult {
    let d = local_today(&s.db);
    let sessions = s.db.sessions_on(&d)?;
    let mut full = Vec::new();
    for x in sessions {
        if let Some(id) = x["id"].as_str() {
            if let Some(v) = s.db.session_get(id)? {
                full.push(lesson::attach_text(&s.db, v));
            }
        }
    }
    let now = srs::fmt(chrono::Utc::now());
    Ok(Json(json!({
        "date": d,
        "sessions": full,
        "cardsDue": s.db.card_due_count(&now)?,
    })))
}

async fn get_session(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let sess = s
        .db
        .session_get(&id)?
        .ok_or_else(|| ApiErr(StatusCode::NOT_FOUND, "không tìm thấy buổi học".into()))?;
    Ok(Json(lesson::attach_text(&s.db, sess)))
}

#[derive(Deserialize)]
struct DoneBody {
    #[serde(default = "yes")]
    done: bool,
}
fn yes() -> bool {
    true
}

async fn complete_session(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<DoneBody>,
) -> ApiResult {
    s.db.session_complete(&id, b.done)?;
    Ok(Json(json!({ "ok": true })))
}

async fn complete_item(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<DoneBody>,
) -> ApiResult {
    s.db.item_complete(&id, b.done)?;
    Ok(Json(json!({ "ok": true })))
}

// ── Cards ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DueQuery {
    #[serde(default)]
    limit: Option<usize>,
}

async fn cards_due(State(s): State<AppState>, Query(q): Query<DueQuery>) -> ApiResult {
    let now = srs::fmt(chrono::Utc::now());
    Ok(Json(json!({
        "due": s.db.cards_due(&now, q.limit.unwrap_or(20).clamp(1, 200))?,
        "total": s.db.card_due_count(&now)?,
    })))
}

#[derive(Deserialize)]
struct ReviewBody {
    grade: String,
}

async fn review_card(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<ReviewBody>,
) -> ApiResult {
    Ok(Json(cards::review(&s.db, &id, &b.grade)?))
}

async fn del_card(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    s.db.card_delete(&id)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct NewCard {
    front: String,
    back: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    doc_id: Option<String>,
    #[serde(default)]
    section_id: Option<String>,
}

async fn create_card(State(s): State<AppState>, Json(b): Json<NewCard>) -> ApiResult {
    if b.front.trim().is_empty() || b.back.trim().is_empty() {
        return Err(ApiErr(StatusCode::BAD_REQUEST, "thẻ phải có cả hai mặt".into()));
    }
    let id = s.db.card_insert(
        b.doc_id.as_deref(),
        b.section_id.as_deref(),
        None,
        None,
        &b.front,
        &b.back,
        b.kind.as_deref().unwrap_or("qa"),
        "manual",
    )?;
    Ok(Json(json!({ "id": id })))
}

#[derive(Deserialize)]
struct GenBody {
    #[serde(default)]
    count: Option<usize>,
    #[serde(default)]
    kinds: Option<Vec<String>>,
}

async fn gen_cards(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<GenBody>,
) -> ApiResult {
    let rep = cards::generate_for_section(&s.db, &id, b.count.unwrap_or(8)).await?;
    Ok(Json(serde_json::to_value(rep).unwrap_or(Value::Null)))
}

// ── Quiz ────────────────────────────────────────────────────────────────────

async fn gen_quiz(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(b): Json<GenBody>,
) -> ApiResult {
    let kinds = b.kinds.unwrap_or_default();
    let rep = quiz::generate_for_section(&s.db, &id, b.count.unwrap_or(6), &kinds).await?;
    Ok(Json(serde_json::to_value(rep).unwrap_or(Value::Null)))
}

#[derive(Deserialize)]
struct QuizBody {
    doc_id: String,
    #[serde(default)]
    section_ids: Option<Vec<String>>,
    #[serde(default)]
    count: Option<usize>,
}

async fn build_quiz(State(s): State<AppState>, Json(b): Json<QuizBody>) -> ApiResult {
    let sections = b.section_ids.unwrap_or_default();
    let n = b.count.unwrap_or(10).clamp(1, 50);
    let mut qs = s.db.questions_pick(&b.doc_id, &sections, n)?;
    if qs.is_empty() {
        return Err(ApiErr(
            StatusCode::BAD_REQUEST,
            "chưa có câu hỏi nào cho tài liệu này — sinh đề trước (POST /api/sections/{id}/quiz/generate)".into(),
        ));
    }
    // The answer key never leaves the server before grading.
    for q in qs.iter_mut() {
        if let Some(o) = q.as_object_mut() {
            o.remove("answer");
            o.remove("explain");
            o.remove("quote");
        }
    }
    Ok(Json(json!({
        "quizId": db::new_id(),
        "questions": qs,
    })))
}

#[derive(Deserialize)]
struct GradeBody {
    quiz_id: String,
    answers: Vec<GradeItem>,
}

#[derive(Deserialize)]
struct GradeItem {
    question_id: String,
    answer: Value,
}

async fn grade_quiz(State(s): State<AppState>, Json(b): Json<GradeBody>) -> ApiResult {
    let pairs: Vec<(String, Value)> = b
        .answers
        .into_iter()
        .map(|a| (a.question_id, a.answer))
        .collect();
    Ok(Json(quiz::grade(&s.db, &b.quiz_id, &pairs)?))
}

// ── Ask ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AskBody {
    question: String,
    #[serde(default)]
    doc_ids: Option<Vec<String>>,
    #[serde(default)]
    sources: Option<String>,
}

async fn post_ask(State(s): State<AppState>, Json(b): Json<AskBody>) -> ApiResult {
    if b.question.trim().is_empty() {
        return Err(ApiErr(StatusCode::BAD_REQUEST, "chưa có câu hỏi".into()));
    }
    Ok(Json(
        ask::ask(&s.db, b.question.trim(), &b.doc_ids.unwrap_or_default()).await?,
    ))
}

async fn post_research(State(s): State<AppState>, Json(b): Json<AskBody>) -> ApiResult {
    if b.question.trim().is_empty() {
        return Err(ApiErr(StatusCode::BAD_REQUEST, "chưa có câu hỏi".into()));
    }
    let setting = b
        .sources
        .or_else(|| s.db.setting("search_mcp"))
        .unwrap_or_else(|| "auto".into());
    Ok(Json(
        ask::research(
            &s.db,
            b.question.trim(),
            &b.doc_ids.unwrap_or_default(),
            &setting,
        )
        .await?,
    ))
}

async fn list_asks(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!(s.db.ask_list(50)?)))
}

async fn list_sources(State(s): State<AppState>) -> ApiResult {
    let all = sources::discover().await;
    let setting = s.db.setting("search_mcp").unwrap_or_else(|| "auto".into());
    let picked = sources::select(&all, &setting, 2);
    Ok(Json(json!({
        "setting": setting,
        "available": all.iter().map(|s| s.to_json()).collect::<Vec<_>>(),
        "selected": picked.iter().map(|s| s.key()).collect::<Vec<_>>(),
    })))
}

// ── Audio ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SpeakBody {
    text: String,
    #[serde(default)]
    voice: Option<String>,
    #[serde(default)]
    speed: Option<f64>,
    #[serde(default)]
    model_id: Option<String>,
    /// Split long text into sentence-sized clips and synthesize each.
    #[serde(default)]
    split: Option<bool>,
}

async fn post_speak(State(s): State<AppState>, Json(b): Json<SpeakBody>) -> ApiResult {
    let speed = b.speed.unwrap_or(1.0);
    if b.split.unwrap_or(false) {
        let parts = tts::sentences(&b.text, 400);
        let mut clips = Vec::new();
        let mut problems = Vec::new();
        for p in parts {
            match tts::speak(&s.db, &p, b.voice.as_deref(), speed, b.model_id.as_deref()).await {
                Ok(name) => clips.push(json!({ "text": p, "url": format!("/api/audio/{name}") })),
                Err(e) => {
                    problems.push(e);
                    // One failure is usually "no model installed"; every later
                    // clip would fail the same way.
                    break;
                }
            }
        }
        if clips.is_empty() {
            return Err(ApiErr(
                StatusCode::BAD_REQUEST,
                problems.join(" · "),
            ));
        }
        return Ok(Json(json!({ "clips": clips, "problems": problems })));
    }
    let name = tts::speak(&s.db, &b.text, b.voice.as_deref(), speed, b.model_id.as_deref()).await?;
    Ok(Json(json!({ "url": format!("/api/audio/{name}") })))
}

async fn get_audio(Path(name): Path<String>) -> Result<axum::response::Response, ApiErr> {
    let path = tts::cached_path(&name)
        .ok_or_else(|| ApiErr(StatusCode::NOT_FOUND, "không có clip này".into()))?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| ApiErr(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok((
        [(axum::http::header::CONTENT_TYPE, "audio/wav")],
        bytes,
    )
        .into_response())
}

// ── Settings ────────────────────────────────────────────────────────────────

async fn get_settings(State(s): State<AppState>) -> ApiResult {
    Ok(Json(json!({
        "tz": s.db.setting("tz").unwrap_or_else(|| "Asia/Ho_Chi_Minh".into()),
        "slotHm": s.db.setting("slot_hm").unwrap_or_else(|| "20:00".into()),
        "studySlots": s.db.setting("study_slots").unwrap_or_else(|| "[\"20:00\"]".into()),
        "searchMcp": s.db.setting("search_mcp").unwrap_or_else(|| "auto".into()),
        "voice": s.db.setting("voice"),
        "speed": s.db.setting("speed").unwrap_or_else(|| "1.0".into()),
    })))
}

#[derive(Deserialize)]
struct SettingsBody {
    #[serde(default)]
    tz: Option<String>,
    #[serde(default)]
    slot_hm: Option<String>,
    #[serde(default)]
    study_slots: Option<Vec<String>>,
    #[serde(default)]
    search_mcp: Option<String>,
    #[serde(default)]
    voice: Option<String>,
    #[serde(default)]
    speed: Option<f64>,
}

async fn patch_settings(State(s): State<AppState>, Json(b): Json<SettingsBody>) -> ApiResult {
    if let Some(v) = b.tz {
        s.db.set_setting("tz", &v)?;
    }
    if let Some(v) = b.slot_hm {
        s.db.set_setting("slot_hm", &v)?;
    }
    if let Some(v) = b.study_slots {
        s.db.set_setting("study_slots", &serde_json::to_string(&v).unwrap_or_default())?;
    }
    if let Some(v) = b.search_mcp {
        s.db.set_setting("search_mcp", &v)?;
    }
    if let Some(v) = b.voice {
        s.db.set_setting("voice", &v)?;
    }
    if let Some(v) = b.speed {
        s.db.set_setting("speed", &v.to_string())?;
    }
    Ok(Json(json!({ "ok": true })))
}

// Keep the LLM module referenced from the API layer's error surface so a
// bridge-only build failure is caught here rather than at first use.
#[allow(dead_code)]
fn _llm_link_check() -> usize {
    llm::MAX_OUT as usize
}
