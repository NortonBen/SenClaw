//! HTTP API. Paths are registered WITHOUT the `/api` prefix; `main.rs` nests
//! this router under `/api`. Paths and response shapes mirror kaizen's NestJS
//! backend so the ported React frontend works unchanged. `/health` and
//! `/status` both serve the health JSON (manifest `healthPath` = `/api/status`).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ops;
use crate::state::AppState;

pub fn root_router(state: AppState) -> Router {
    Router::new().route("/health", get(health)).with_state(state)
}

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/status", get(health))
        // Profile / settings (kaizen: user module, minus auth)
        .route("/users/profile", get(get_profile).patch(patch_profile))
        .route("/users/snooze", post(post_snooze))
        // Study
        .route("/study/session", get(get_session))
        .route("/study/review/:card_id", post(post_review))
        .route("/study/review-batch", post(post_review_batch))
        .route("/study/spaced-repetition/review/:card_id", post(post_review))
        .route("/study/log", post(post_study_log))
        .route("/study/snooze", post(post_snooze))
        .route("/study/spaced-repetition/:notification_id", get(get_due_session))
        .route("/study/overview", get(get_overview))
        .route("/study/learned-cards", get(get_learned_cards))
        .route("/study/statistics/level", get(get_stats_level))
        .route("/study/statistics/today", get(get_stats_today))
        // Review practice (24h anti-repeat pool)
        .route("/review/session", get(get_review_session))
        .route("/review/session/lesson/:lesson_id", get(get_review_session_lesson))
        .route("/review/submit/batch", post(post_review_submit_batch))
        .route("/review/submit/:card_id", post(post_review_submit))
        // Matching / Listening / Writing games (record-only submits)
        .route("/matching/session", get(get_game_session))
        .route("/listening/session", get(get_game_session))
        .route("/writing/session", get(get_game_session))
        .route("/matching/submit/:card_id", post(post_game_submit))
        .route("/listening/submit/:card_id", post(post_game_submit))
        .route("/writing/submit/:card_id", post(post_game_submit))
        // Lessons
        .route("/lessons", get(list_lessons).post(create_lesson))
        .route("/lessons/import", post(import_lesson))
        .route("/lessons/ai-draft", post(ai_draft_vocab))
        .route("/lessons/my", get(list_lessons))
        .route("/lessons/my-and-marked", get(list_lessons))
        .route("/lessons/:id", get(get_lesson).patch(patch_lesson).delete(delete_lesson))
        .route("/lessons/:id/cards", get(get_lesson_cards).post(add_card))
        .route("/lessons/:id/cards/:card_id", axum::routing::patch(patch_card).delete(remove_card))
        // Stories (Phase 3)
        .route("/stories", get(list_stories).post(create_story))
        .route("/stories/public", get(list_stories))
        .route("/stories/generate", post(generate_story))
        .route("/stories/:id", get(get_story).patch(patch_story).delete(delete_story))
        .route("/stories/:id/progress", get(get_story_progress).post(post_story_progress))
        // Dictation (Phase 3)
        .route("/dictation-lessons", get(list_dictation).post(create_dictation_lesson))
        .route("/dictation-lessons/topics", get(dictation_topics).post(create_dictation_topic))
        .route(
            "/dictation-topics/:id",
            axum::routing::patch(patch_dictation_topic).delete(delete_dictation_topic),
        )
        .route("/dictation-lessons/export", get(export_dictation))
        .route("/dictation-lessons/import", post(import_dictation))
        .route("/dictation-lessons/ai-draft", post(ai_draft_dictation))
        .route("/dictation-lessons/history/me", get(dictation_history))
        .route(
            "/dictation-lessons/:id",
            get(get_dictation_lesson)
                .patch(patch_dictation_lesson)
                .delete(delete_dictation_lesson),
        )
        .route("/dictation-lessons/:id/progress", get(get_dictation_progress).post(post_dictation_progress))
        .route("/dictation-lessons/:id/audio/segment", get(dictation_audio_segment))
        // Dictionary (Phase 3)
        .route("/dictionary/lookup", get(dictionary_lookup))
        .route("/dictionary/audio", get(dictionary_audio))
        // Grammar (Phase 2) + admin backup/AI (Phase 5)
        .route("/grammar", get(list_grammar).post(create_grammar))
        .route("/grammar/public", get(list_grammar))
        .route("/grammar/export", get(export_grammar))
        .route("/grammar/import", post(import_grammar))
        .route("/grammar/ai-draft", post(ai_draft_grammar))
        .route("/grammar/:id_or_slug", get(view_grammar).patch(patch_grammar).delete(delete_grammar))
        .route("/grammar-topics", get(list_grammar_topics))
        .route("/grammar-topics/for-lesson/:grammar_slug", get(topic_for_lesson))
        .route("/grammar-test/results/:session_id", get(grammar_test_result))
        .route("/grammar-test/generate", post(generate_grammar_test))
        .route("/grammar-test/submit", post(submit_grammar_test))
        .route("/grammar-test/:topic_id", get(grammar_test_questions))
        // MCP
        .route("/mcp/sse", get(crate::mcp::mcp_sse).post(crate::mcp::mcp_message))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

// ---- helpers ----

fn ok(v: Value) -> Response {
    Json(v).into_response()
}

fn fail(e: anyhow::Error) -> Response {
    let msg = e.to_string();
    let code = if msg.contains("Không tìm thấy") {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::BAD_REQUEST
    };
    (code, Json(json!({ "error": msg, "message": msg }))).into_response()
}

fn respond(r: anyhow::Result<Value>) -> Response {
    match r {
        Ok(v) => ok(v),
        Err(e) => fail(e),
    }
}

async fn health() -> Response {
    ok(json!({
        "ok": true,
        "name": "kaen",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

// ---- profile ----

async fn get_profile(State(s): State<AppState>) -> Response {
    respond(ops::profile(&s.db))
}

async fn patch_profile(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    respond(ops::update_profile(&s.db, &body))
}

async fn post_snooze(State(s): State<AppState>, body: Option<Json<Value>>) -> Response {
    let hours = body
        .as_ref()
        .and_then(|b| b.0["durationHours"].as_i64())
        .unwrap_or(1);
    respond(ops::snooze(&s.db, hours))
}

// ---- study ----

#[derive(Deserialize)]
struct SessionQuery {
    #[serde(rename = "lessonId")]
    lesson_id: Option<String>,
}

async fn get_session(State(s): State<AppState>, Query(q): Query<SessionQuery>) -> Response {
    respond(match q.lesson_id.as_deref().filter(|v| !v.is_empty()) {
        Some(id) => ops::lesson_session(&s.db, id),
        None => ops::session(&s.db),
    })
}

async fn post_review(
    State(s): State<AppState>,
    Path(card_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let result = body["result"].as_str().unwrap_or("");
    let mode = body["mode"].as_str().unwrap_or("FLIP");
    respond(ops::submit_review(&s.db, &card_id, result, mode))
}

async fn post_review_batch(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let empty = Vec::new();
    let reviews = body["reviews"].as_array().unwrap_or(&empty);
    respond(ops::review_batch(
        &s.db,
        reviews,
        body["durationSeconds"].as_i64().unwrap_or(0),
        body["newWordsLearned"].as_i64().unwrap_or(0),
        body["cardsReviewed"].as_i64().unwrap_or(0),
        body["gameScore"].as_i64(),
    ))
}

async fn post_study_log(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    respond(ops::study_log(
        &s.db,
        body["durationSeconds"].as_i64().unwrap_or(0),
        body["newWordsLearned"].as_i64().unwrap_or(0),
        body["cardsReviewed"].as_i64().unwrap_or(0),
        body["gameScore"].as_i64(),
    ))
}

#[derive(Deserialize)]
struct LearnedQuery {
    page: Option<i64>,
    limit: Option<i64>,
    period: Option<String>,
    #[serde(rename = "minLevel")]
    min_level: Option<i64>,
    #[serde(rename = "maxLevel")]
    max_level: Option<i64>,
    search: Option<String>,
}

async fn get_learned_cards(State(s): State<AppState>, Query(q): Query<LearnedQuery>) -> Response {
    respond(ops::learned_cards(
        &s.db,
        q.page.unwrap_or(1),
        q.limit.unwrap_or(20),
        q.period.as_deref(),
        q.min_level,
        q.max_level,
        q.search.as_deref(),
    ))
}

async fn get_overview(State(s): State<AppState>) -> Response {
    respond(ops::overview(&s.db))
}

async fn get_stats_level(State(s): State<AppState>) -> Response {
    respond(ops::stats_level(&s.db))
}

async fn get_stats_today(State(s): State<AppState>) -> Response {
    respond(ops::stats_today(&s.db))
}

// ---- practice ----

#[derive(Deserialize)]
struct AllowRepeatQuery {
    #[serde(rename = "allowRepeat")]
    allow_repeat: Option<String>,
}

fn is_true(v: &Option<String>) -> bool {
    v.as_deref() == Some("true")
}

async fn get_review_session(State(s): State<AppState>, Query(q): Query<AllowRepeatQuery>) -> Response {
    respond(ops::review_session(&s.db, is_true(&q.allow_repeat), None))
}

async fn get_review_session_lesson(
    State(s): State<AppState>,
    Path(lesson_id): Path<String>,
    Query(q): Query<AllowRepeatQuery>,
) -> Response {
    respond(ops::review_session(&s.db, is_true(&q.allow_repeat), Some(&lesson_id)))
}

async fn post_review_submit(
    State(s): State<AppState>,
    Path(card_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    respond(ops::review_submit(&s.db, &card_id, body["isCorrect"].as_bool().unwrap_or(false)))
}

async fn post_review_submit_batch(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let empty = Vec::new();
    let results = body["results"].as_array().unwrap_or(&empty);
    respond(ops::review_submit_batch(&s.db, results))
}

async fn get_game_session(State(s): State<AppState>) -> Response {
    respond(ops::game_session(&s.db))
}

async fn post_game_submit(
    State(s): State<AppState>,
    Path(card_id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    respond(ops::game_submit(&s.db, &card_id, body["isCorrect"].as_bool().unwrap_or(false)))
}

async fn get_due_session(State(s): State<AppState>, Path(_id): Path<String>) -> Response {
    respond(ops::due_session(&s.db))
}

// ---- stories (Phase 3) ----

async fn list_stories(State(s): State<AppState>) -> Response {
    respond(crate::story::list_stories(&s.db))
}

async fn create_story(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    respond(crate::story::create_story(&s.db, &body))
}

async fn get_story(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    respond(crate::story::get_story(&s.db, &id))
}

async fn patch_story(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    respond(crate::story::update_story(&s.db, &id, &body))
}

async fn delete_story(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    respond(crate::story::delete_story(&s.db, &id))
}

async fn get_story_progress(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    respond(crate::story::get_progress(&s.db, &id))
}

async fn post_story_progress(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    respond(crate::story::update_progress(&s.db, &id, &body))
}

async fn generate_story(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let lesson_id = body["lessonId"].as_str().unwrap_or("");
    if lesson_id.is_empty() {
        return fail(anyhow::anyhow!("Thiếu lessonId"));
    }
    let native = s
        .db
        .settings()
        .map(|st| st.native_language)
        .unwrap_or_else(|_| "vi".into());
    respond(
        crate::story::generate_story(
            &s.db,
            lesson_id,
            body["title"].as_str().unwrap_or(""),
            body["description"].as_str().unwrap_or(""),
            body["nativeLanguage"].as_str().unwrap_or(&native),
        )
        .await,
    )
}

// ---- dictation (Phase 3) ----

#[derive(Deserialize)]
struct DictationListQuery {
    topic: Option<String>,
    level: Option<String>,
    limit: Option<i64>,
    page: Option<i64>,
}

async fn list_dictation(State(s): State<AppState>, Query(q): Query<DictationListQuery>) -> Response {
    respond(crate::dictation::list_lessons(
        &s.db,
        q.topic.as_deref().filter(|v| !v.is_empty()),
        q.level.as_deref().filter(|v| !v.is_empty()),
        q.limit.unwrap_or(20),
        q.page.unwrap_or(1),
    ))
}

async fn dictation_topics(State(s): State<AppState>) -> Response {
    respond(crate::dictation::list_topics(&s.db))
}

async fn dictation_history(State(s): State<AppState>) -> Response {
    respond(crate::dictation::history(&s.db))
}

async fn get_dictation_lesson(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    respond(crate::dictation::get_lesson(&s.db, id))
}

async fn get_dictation_progress(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    respond(crate::dictation::get_progress(&s.db, id))
}

async fn post_dictation_progress(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Response {
    respond(crate::dictation::save_progress(
        &s.db,
        id,
        body["currentIndex"].as_i64().unwrap_or(0),
        body.get("segmentStatus").unwrap_or(&json!({})),
    ))
}

/// Compat endpoint: kaizen sliced local audio server-side; Kaen redirects to
/// the lesson audio and the client seeks by start/end time.
async fn dictation_audio_segment(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    match crate::dictation::lesson_audio_url(&s.db, id) {
        Ok(Some(url)) => axum::response::Redirect::temporary(&url).into_response(),
        Ok(None) => fail(anyhow::anyhow!("Bài dictation không có audio")),
        Err(e) => fail(e),
    }
}

// ---- dictionary (Phase 3) ----

#[derive(Deserialize)]
struct DictLookupQuery {
    word: Option<String>,
    #[serde(rename = "targetLang")]
    target_lang: Option<String>,
}

async fn dictionary_lookup(State(s): State<AppState>, Query(q): Query<DictLookupQuery>) -> Response {
    let Some(word) = q.word.filter(|w| !w.trim().is_empty()) else {
        return fail(anyhow::anyhow!("Thiếu word"));
    };
    respond(crate::dictionary::lookup(&s.db, &word, q.target_lang.as_deref().unwrap_or("vi")).await)
}

async fn dictionary_audio(State(s): State<AppState>, Query(q): Query<DictLookupQuery>) -> Response {
    let Some(word) = q.word.filter(|w| !w.trim().is_empty()) else {
        return fail(anyhow::anyhow!("Thiếu word"));
    };
    respond(crate::dictionary::audio_url(&s.db, &word).await)
}

// ---- admin: backup + AI drafting (Phase 5) ----
//
// kaizen kept all of this in a separate NestJS CMS with its own login. Kaen is
// a single-user local app, so content management lives in the app itself and
// needs no auth; the AI drafting that used to call Dify now goes through the
// SenClaw bridge like every other completion in this app.

async fn export_grammar(State(s): State<AppState>) -> Response {
    respond(crate::grammar::export_all(&s.db))
}

async fn import_grammar(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    respond(crate::grammar::import_bulk(&s.db, &body))
}

async fn ai_draft_grammar(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let _ = &s;
    let topic = body["topic"].as_str().unwrap_or("").trim().to_string();
    if topic.is_empty() {
        return fail(anyhow::anyhow!("Thiếu chủ đề ngữ pháp"));
    }
    let level = body["level"].as_str().unwrap_or("B1");
    let note = body["note"].as_str().unwrap_or("");
    match crate::llm::draft_grammar_lesson(&topic, level, note).await {
        Ok(v) => ok(v),
        Err(e) => fail(anyhow::anyhow!(e)),
    }
}

async fn ai_draft_vocab(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let _ = &s;
    let topic = body["topic"].as_str().unwrap_or("").trim().to_string();
    if topic.is_empty() {
        return fail(anyhow::anyhow!("Thiếu chủ đề"));
    }
    let level = body["level"].as_str().unwrap_or("A2");
    let count = body["count"].as_u64().unwrap_or(10) as u32;
    match crate::llm::draft_vocab_list(&topic, level, count).await {
        Ok(items) => {
            // Also hand back the pipe format so the UI can drop it straight into
            // the existing importer without a second round of mapping.
            let raw_text = items
                .iter()
                .map(|i| {
                    let f = |k: &str| i[k].as_str().unwrap_or("").replace('|', "/");
                    format!(
                        "{}|{}|{}|{}|{}|{}",
                        f("word"),
                        f("meaning"),
                        f("example"),
                        f("partOfSpeech"),
                        f("ipa"),
                        f("explain")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            ok(json!({ "items": items, "rawText": raw_text }))
        }
        Err(e) => fail(anyhow::anyhow!(e)),
    }
}

async fn export_dictation(State(s): State<AppState>) -> Response {
    respond(crate::dictation::export_all(&s.db))
}

async fn import_dictation(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    respond(crate::dictation::import_json(&s.db, &body))
}

/// Build dictation segments from a pasted transcript, or let the AI write the
/// passage first. Timings are spread across `durationSeconds` when given.
async fn ai_draft_dictation(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let _ = &s;
    let duration = body["durationSeconds"].as_f64().unwrap_or(0.0);
    let text = body["text"].as_str().unwrap_or("").trim().to_string();

    let passage = if !text.is_empty() {
        text
    } else {
        let topic = body["topic"].as_str().unwrap_or("").trim().to_string();
        if topic.is_empty() {
            return fail(anyhow::anyhow!("Cần 'text' để tách đoạn hoặc 'topic' để AI viết bài"));
        }
        let level = body["level"].as_str().unwrap_or("A2");
        let sentences = body["sentences"].as_u64().unwrap_or(6) as u32;
        match crate::llm::draft_dictation_passage(&topic, level, sentences).await {
            Ok(p) => p,
            Err(e) => return fail(anyhow::anyhow!(e)),
        }
    };

    let segments = crate::llm::split_into_segments(&passage, duration);
    ok(json!({ "text": passage, "segments": segments }))
}

async fn create_dictation_topic(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    respond(crate::dictation::create_topic(&s.db, &body))
}

async fn patch_dictation_topic(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Response {
    respond(crate::dictation::update_topic(&s.db, id, &body))
}

async fn delete_dictation_topic(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    respond(crate::dictation::delete_topic(&s.db, id))
}

async fn create_dictation_lesson(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    respond(crate::dictation::create_lesson(&s.db, &body))
}

async fn patch_dictation_lesson(
    State(s): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Response {
    respond(crate::dictation::update_lesson(&s.db, id, &body))
}

async fn delete_dictation_lesson(State(s): State<AppState>, Path(id): Path<i64>) -> Response {
    respond(crate::dictation::delete_lesson(&s.db, id))
}

// ---- grammar (Phase 2) ----

#[derive(Deserialize)]
struct GrammarListQuery {
    page: Option<i64>,
    limit: Option<i64>,
    level: Option<String>,
    search: Option<String>,
    study: Option<String>,
}

async fn list_grammar(State(s): State<AppState>, Query(q): Query<GrammarListQuery>) -> Response {
    respond(crate::grammar::list_grammars(
        &s.db,
        q.page.unwrap_or(1),
        q.limit.unwrap_or(10),
        q.level.as_deref().filter(|v| !v.is_empty()),
        q.search.as_deref(),
        q.study.as_deref(),
    ))
}

async fn create_grammar(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    respond(crate::grammar::create_grammar(
        &s.db,
        body["title"].as_str().unwrap_or(""),
        body["content"].as_str().unwrap_or(""),
        body["description"].as_str(),
        body["level"].as_str().unwrap_or("B1"),
        body["index"].as_i64().unwrap_or(0),
    ))
}

async fn view_grammar(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    respond(crate::grammar::view_grammar(&s.db, &id))
}

async fn patch_grammar(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    respond(crate::grammar::update_grammar(&s.db, &id, &body))
}

async fn delete_grammar(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    respond(crate::grammar::delete_grammar(&s.db, &id))
}

#[derive(Deserialize)]
struct TopicQuery {
    level: Option<String>,
}

async fn list_grammar_topics(State(s): State<AppState>, Query(q): Query<TopicQuery>) -> Response {
    respond(crate::grammar::list_topics(&s.db, q.level.as_deref().filter(|v| !v.is_empty())))
}

async fn topic_for_lesson(State(s): State<AppState>, Path(slug): Path<String>) -> Response {
    respond(crate::grammar::topic_for_lesson(&s.db, &slug))
}

async fn grammar_test_questions(State(s): State<AppState>, Path(topic_id): Path<String>) -> Response {
    respond(crate::grammar::questions_for_topic(&s.db, &topic_id))
}

async fn grammar_test_result(State(s): State<AppState>, Path(session_id): Path<String>) -> Response {
    respond(crate::grammar::session_result(&s.db, &session_id))
}

async fn generate_grammar_test(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let topic = body["topic"].as_str().unwrap_or("").trim().to_string();
    if topic.is_empty() {
        return fail(anyhow::anyhow!("topic không được để trống"));
    }
    let level = body["level"].as_str().unwrap_or("A1").to_string();
    let count = body["count"].as_u64().unwrap_or(10) as u32;
    let link = body["grammarSlug"]
        .as_str()
        .or_else(|| body["grammarId"].as_str())
        .map(str::to_string);

    // Ground generation in the lesson text when the test is linked to one.
    let content = link
        .as_deref()
        .and_then(|l| crate::grammar::grammar_content(&s.db, l).ok().flatten())
        .map(|(_, _, c)| c);

    match crate::llm::generate_grammar_questions(&topic, &level, count, content.as_deref()).await {
        Ok(items) => respond(crate::grammar::save_generated_questions(
            &s.db,
            &topic,
            &level,
            link.as_deref(),
            &items,
        )),
        Err(e) => fail(anyhow::anyhow!(e)),
    }
}

async fn submit_grammar_test(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let topic_id = body["topicId"].as_str().unwrap_or("");
    let empty = Vec::new();
    let answers = body["answers"].as_array().unwrap_or(&empty);
    if topic_id.is_empty() {
        return fail(anyhow::anyhow!("Thiếu topicId"));
    }
    respond(crate::grammar::submit_test(&s.db, topic_id, answers))
}

// ---- lessons ----

#[derive(Deserialize)]
struct LessonListQuery {
    search: Option<String>,
    page: Option<i64>,
    limit: Option<i64>,
}

async fn list_lessons(State(s): State<AppState>, Query(q): Query<LessonListQuery>) -> Response {
    respond(ops::lessons_page(
        &s.db,
        q.search.as_deref(),
        q.page.unwrap_or(1),
        q.limit.unwrap_or(50),
    ))
}

async fn create_lesson(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let title = body["title"].as_str().unwrap_or("").trim().to_string();
    if title.is_empty() {
        return fail(anyhow::anyhow!("Thiếu tiêu đề bài học"));
    }
    respond(
        s.db.create_lesson(&title, body["description"].as_str())
            .map(|l| json!(l)),
    )
}

async fn import_lesson(State(s): State<AppState>, Json(body): Json<Value>) -> Response {
    let title = body["title"].as_str().unwrap_or("").trim().to_string();
    let raw = body["rawText"].as_str().unwrap_or("");
    if title.is_empty() || raw.trim().is_empty() {
        return fail(anyhow::anyhow!("Thiếu tiêu đề hoặc nội dung import"));
    }
    respond(ops::import_lesson(
        &s.db,
        &title,
        raw,
        body["separator"].as_str().unwrap_or("|"),
    ))
}

async fn get_lesson(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    respond(ops::lesson_json(&s.db, &id, true))
}

async fn get_lesson_cards(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    // Contract: { cards (with progress), lesson: { title } } — same shape the
    // lesson study session uses.
    respond(ops::lesson_session(&s.db, &id))
}

async fn patch_lesson(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    match s.db.update_lesson(&id, body["title"].as_str(), body["description"].as_str()) {
        Ok(true) => respond(ops::lesson_json(&s.db, &id, false)),
        Ok(false) => fail(anyhow::anyhow!("Không tìm thấy bài học")),
        Err(e) => fail(e),
    }
}

async fn delete_lesson(State(s): State<AppState>, Path(id): Path<String>) -> Response {
    match s.db.delete_lesson(&id) {
        Ok(true) => ok(json!({ "success": true })),
        Ok(false) => fail(anyhow::anyhow!("Không tìm thấy bài học")),
        Err(e) => fail(e),
    }
}

async fn add_card(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    if s.db.get_lesson(&id).ok().flatten().is_none() {
        return fail(anyhow::anyhow!("Không tìm thấy bài học"));
    }
    let word = body["word"].as_str().unwrap_or("").trim().to_string();
    if word.is_empty() {
        return fail(anyhow::anyhow!("Thiếu từ vựng (word)"));
    }
    respond(
        s.db.insert_card(
            &id,
            &word,
            body["imageUrl"].as_str(),
            body["ipa"].as_str(),
            body["partOfSpeech"].as_str(),
            body.get("examples").filter(|v| v.is_array()),
            body["explain"].as_str().unwrap_or(""),
            body.get("meanings").filter(|v| v.is_object()),
        )
        .map(|c| json!(c)),
    )
}

async fn patch_card(
    State(s): State<AppState>,
    Path((_lesson_id, card_id)): Path<(String, String)>,
    Json(body): Json<Value>,
) -> Response {
    let Some(fields) = body.as_object() else {
        return fail(anyhow::anyhow!("Body không hợp lệ"));
    };
    match s.db.update_card_fields(&card_id, fields) {
        Ok(_) => match s.db.get_card(&card_id) {
            Ok(Some(c)) => ok(json!(c)),
            Ok(None) => fail(anyhow::anyhow!("Không tìm thấy thẻ")),
            Err(e) => fail(e),
        },
        Err(e) => fail(e),
    }
}

async fn remove_card(
    State(s): State<AppState>,
    Path((_lesson_id, card_id)): Path<(String, String)>,
) -> Response {
    match s.db.delete_card(&card_id) {
        Ok(true) => ok(json!({ "success": true })),
        Ok(false) => fail(anyhow::anyhow!("Không tìm thấy thẻ")),
        Err(e) => fail(e),
    }
}
