//! Domain operations shared by the REST API and the MCP tools. Each returns a
//! `serde_json::Value` shaped exactly like kaizen's NestJS responses
//! (camelCase), so the ported React frontend keeps working unchanged.

use anyhow::{anyhow, Result};
use chrono::{DateTime, Duration, Utc};
use rand::seq::SliceRandom;
use serde_json::{json, Map, Value};

use crate::db::{Card, Db};
use crate::srs::{self, ProgressData, ReviewAction};

fn card_json(card: &Card, progress: Option<Value>) -> Value {
    let mut v = serde_json::to_value(card).unwrap_or_default();
    if let Some(obj) = v.as_object_mut() {
        obj.insert("progress".into(), progress.unwrap_or(Value::Null));
    }
    v
}

fn progress_json(p: &ProgressData) -> Value {
    json!({
        "level": p.level,
        "isUrgent": p.is_urgent,
        "nextReview": srs::fmt(p.next_review),
    })
}

fn tz_and_slots(db: &Db) -> Result<(chrono_tz::Tz, Vec<String>)> {
    let s = db.settings()?;
    Ok((srs::parse_tz(&s.timezone), s.study_slots))
}

// ---- profile / settings ----

pub fn profile(db: &Db) -> Result<Value> {
    let s = db.settings()?;
    Ok(json!({
        // Identity stubs kept for the ported frontend; the app is single-user.
        "id": "local",
        "username": "kaen",
        "email": "",
        "fullName": Value::Null,
        "avatarUrl": Value::Null,
        "bio": Value::Null,
        "nativeLanguage": s.native_language,
        "studySlots": s.study_slots,
        "timezone": s.timezone,
        "dailyWordGoal": s.daily_word_goal,
        "currentStreak": s.current_streak,
        "lastStudyDate": s.last_study_date,
        "totalXP": s.total_xp,
        "snoozeUntil": s.snooze_until,
    }))
}

pub fn update_profile(db: &Db, body: &Value) -> Result<Value> {
    if let Some(slots) = body.get("studySlots") {
        if slots.is_array() {
            db.set_setting_field("study_slots", &slots.to_string())?;
        }
    }
    if let Some(tz) = body.get("timezone").and_then(Value::as_str) {
        if tz.parse::<chrono_tz::Tz>().is_err() {
            return Err(anyhow!("timezone không hợp lệ: {tz}"));
        }
        db.set_setting_field("timezone", tz)?;
    }
    if let Some(goal) = body.get("dailyWordGoal").and_then(Value::as_i64) {
        db.set_setting_field("daily_word_goal", &goal.to_string())?;
    }
    if let Some(lang) = body.get("nativeLanguage").and_then(Value::as_str) {
        db.set_setting_field("native_language", lang)?;
    }
    profile(db)
}

pub fn snooze(db: &Db, duration_hours: i64) -> Result<Value> {
    let until = Utc::now() + Duration::hours(duration_hours.clamp(0, 72));
    db.set_setting_field("snooze_until", &srs::fmt(until))?;
    Ok(json!({ "snoozeUntil": srs::fmt(until) }))
}

// ---- study ----

/// Port of `getSession`: 5 random new cards + 5 review cards mixed from a
/// low-level (1-2) pool of 10 and a high-level (3+) pool of 5.
pub fn session(db: &Db) -> Result<Value> {
    let now = Utc::now();
    let mut rng = rand::thread_rng();

    // Cards met in a story (viewed/listened) don't count as brand-new — kaizen
    // excluded them from the new-word pool too.
    let story_learned = crate::story::story_learned_card_ids(db)?;
    let mut new_cards: Vec<_> = db
        .cards_without_progress()?
        .into_iter()
        .filter(|c| !story_learned.contains(&c.id))
        .collect();
    let total_new = new_cards.len();
    new_cards.shuffle(&mut rng);
    let selected_new: Vec<Value> = new_cards
        .iter()
        .take(5)
        .map(|c| {
            card_json(
                c,
                Some(json!({ "level": 0, "isUrgent": true, "nextReview": srs::fmt(now) })),
            )
        })
        .collect();

    let learned = db.learned_count()?;
    if learned == 0 {
        return Ok(json!({ "cards": selected_new, "totalNew": total_new, "totalReview": 0 }));
    }

    let low = db.random_review_cards(1, 2, 10)?;
    let high = db.random_review_cards(3, srs::MAX_LEVEL, 5)?;
    let mut mixed: Vec<_> = low.into_iter().chain(high).collect();
    mixed.shuffle(&mut rng);

    let review: Vec<Value> = mixed
        .iter()
        .take(5)
        .map(|(c, p)| card_json(c, Some(progress_json(&p.data))))
        .collect();

    let mut cards = selected_new;
    cards.extend(review);
    Ok(json!({ "cards": cards, "totalNew": total_new, "totalReview": learned }))
}

/// Port of `getLessonReviewSession`: every card of the lesson, new-first, then
/// due (urgent first, earliest due first), then not-yet-due.
pub fn lesson_session(db: &Db, lesson_id: &str) -> Result<Value> {
    let lesson = db
        .get_lesson(lesson_id)?
        .ok_or_else(|| anyhow!("Không tìm thấy bài học"))?;
    let cards = db.cards_of_lesson(lesson_id)?;
    let now = Utc::now();

    if cards.is_empty() {
        return Ok(json!({
            "cards": [], "totalNew": 0, "totalReview": 0,
            "lesson": { "id": lesson.id, "title": lesson.title },
        }));
    }

    let ids: Vec<String> = cards.iter().map(|c| c.id.clone()).collect();
    let progress = db.progress_for_cards(&ids)?;
    let pmap: std::collections::HashMap<String, ProgressData> = progress
        .into_iter()
        .map(|row| (row.card_id.clone(), row.data))
        .collect();

    struct Entry {
        json: Value,
        p: Option<ProgressData>,
    }
    let mut entries: Vec<Entry> = cards
        .iter()
        .map(|c| {
            let p = pmap.get(&c.id).cloned();
            Entry {
                json: card_json(c, p.as_ref().map(progress_json)),
                p,
            }
        })
        .collect();

    entries.sort_by(|a, b| {
        use std::cmp::Ordering;
        match (&a.p, &b.p) {
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (None, None) => Ordering::Equal,
            (Some(pa), Some(pb)) => {
                let a_due = pa.next_review <= now;
                let b_due = pb.next_review <= now;
                match (a_due, b_due) {
                    (true, false) => Ordering::Less,
                    (false, true) => Ordering::Greater,
                    (true, true) => pb
                        .is_urgent
                        .cmp(&pa.is_urgent)
                        .then(pa.next_review.cmp(&pb.next_review)),
                    (false, false) => pa.next_review.cmp(&pb.next_review),
                }
            }
        }
    });

    let total_new = entries.iter().filter(|e| e.p.is_none()).count();
    let total_review = entries
        .iter()
        .filter(|e| e.p.as_ref().is_some_and(|p| p.next_review <= now))
        .count();

    Ok(json!({
        "cards": entries.into_iter().map(|e| e.json).collect::<Vec<_>>(),
        "totalNew": total_new,
        "totalReview": total_review,
        "lesson": { "id": lesson.id, "title": lesson.title },
    }))
}

/// Streak bookkeeping shared by every grading path.
fn update_streak(db: &Db) -> Result<()> {
    let s = db.settings()?;
    let tz = srs::parse_tz(&s.timezone);
    let last = s.last_study_date.as_deref().and_then(srs::parse);
    let (streak, update_last) = srs::next_streak(last, s.current_streak, tz, Utc::now());
    let last_str = update_last.then(|| srs::fmt(Utc::now()));
    db.set_streak(streak, last_str.as_deref())?;
    Ok(())
}

fn apply_and_persist(
    db: &Db,
    card_id: &str,
    is_correct: bool,
    slots: &[String],
    tz: chrono_tz::Tz,
) -> Result<Option<ProgressData>> {
    let existing = db.get_progress(card_id)?.map(|r| r.data);
    match srs::apply_review(existing.as_ref(), is_correct, slots, tz, Utc::now()) {
        ReviewAction::Create(p) | ReviewAction::Update(p) => {
            db.upsert_progress(card_id, &p)?;
            Ok(Some(p))
        }
        ReviewAction::Remove => {
            db.remove_progress(card_id)?;
            Ok(None)
        }
    }
}

/// Port of `submitReview` / `submitSpacedRepetitionReview` (identical logic in
/// the single-user port — the TS pair differed only in dead branches).
pub fn submit_review(db: &Db, card_id: &str, result: &str, mode: &str) -> Result<Value> {
    if db.get_card(card_id)?.is_none() {
        return Err(anyhow!("Không tìm thấy thẻ"));
    }
    let is_correct = result.eq_ignore_ascii_case("REMEMBER");
    let (tz, slots) = tz_and_slots(db)?;

    let progress = apply_and_persist(db, card_id, is_correct, &slots, tz)?;
    let xp = srs::xp_for(mode, is_correct);
    db.add_xp(xp)?;
    update_streak(db)?;

    Ok(json!({
        "xpEarned": xp,
        "progress": progress.map(|p| json!({ "level": p.level, "isUrgent": p.is_urgent })),
    }))
}

/// Port of `submitReviewBatch` — grades every review, then writes one study
/// log. Keeps kaizen's XP accounting: per-card XP AND the log's own XP
/// (new*10 + reviewed*5 + gameScore) are both added to the total.
pub fn review_batch(
    db: &Db,
    reviews: &[Value],
    duration_seconds: i64,
    new_words_learned: i64,
    cards_reviewed: i64,
    game_score: Option<i64>,
) -> Result<Value> {
    let (tz, slots) = tz_and_slots(db)?;
    let mut total_xp = 0i64;
    let mut processed = 0usize;

    for r in reviews {
        let card_id = r["cardId"].as_str().unwrap_or("");
        if card_id.is_empty() || db.get_card(card_id)?.is_none() {
            continue;
        }
        let is_correct = r["result"].as_str().unwrap_or("") == "REMEMBER";
        let mode = r["mode"].as_str().unwrap_or("FLIP");
        apply_and_persist(db, card_id, is_correct, &slots, tz)?;
        total_xp += srs::xp_for(mode, is_correct);
        processed += 1;
    }

    if total_xp > 0 {
        db.add_xp(total_xp)?;
    }
    let log_xp = new_words_learned * 10 + cards_reviewed * 5 + game_score.unwrap_or(0);
    update_streak(db)?;
    if log_xp > 0 {
        db.add_xp(log_xp)?;
    }
    let log_id = db.insert_study_log(
        duration_seconds,
        new_words_learned,
        cards_reviewed,
        game_score,
        log_xp,
    )?;

    Ok(json!({
        "totalXP": total_xp,
        "processed": processed,
        "studyLog": { "id": log_id, "xpEarned": log_xp },
    }))
}

/// Port of `createStudyLog` (POST /study/log).
pub fn study_log(
    db: &Db,
    duration_seconds: i64,
    new_words_learned: i64,
    cards_reviewed: i64,
    game_score: Option<i64>,
) -> Result<Value> {
    let xp = new_words_learned * 10 + cards_reviewed * 5 + game_score.unwrap_or(0);
    update_streak(db)?;
    db.add_xp(xp)?;
    let id = db.insert_study_log(
        duration_seconds,
        new_words_learned,
        cards_reviewed,
        game_score,
        xp,
    )?;
    Ok(json!({
        "id": id,
        "durationSeconds": duration_seconds,
        "newWordsLearned": new_words_learned,
        "cardsReviewed": cards_reviewed,
        "gameScore": game_score,
        "xpEarned": xp,
    }))
}

/// Port of `getLearnedCards`: level > 0 (or explicit range), optional
/// today-only and search filters, de-duplicated by word, newest review first.
pub fn learned_cards(
    db: &Db,
    page: i64,
    limit: i64,
    period: Option<&str>,
    min_level: Option<i64>,
    max_level: Option<i64>,
    search: Option<&str>,
) -> Result<Value> {
    let page = page.max(1);
    let limit = limit.clamp(1, 100);
    let s = db.settings()?;
    let tz = srs::parse_tz(&s.timezone);
    let now = Utc::now();

    let today_range =
        (period == Some("today")).then(|| (srs::start_of_day(tz, now), srs::end_of_day(tz, now)));

    let mut seen = std::collections::HashSet::new();
    let mut cards = Vec::new();
    for (card, prow) in db.progress_with_cards()? {
        let level_ok = match (min_level, max_level) {
            (None, None) => prow.data.level > 0,
            (min, max) => {
                prow.data.level >= min.unwrap_or(0) && prow.data.level <= max.unwrap_or(7)
            }
        };
        if !level_ok {
            continue;
        }
        if let Some((from, to)) = today_range {
            if prow.created_at < from || prow.created_at > to {
                continue;
            }
        }
        if let Some(q) = search {
            let q = q.to_lowercase();
            if !q.is_empty()
                && !card.word.to_lowercase().contains(&q)
                && !card.explain.to_lowercase().contains(&q)
            {
                continue;
            }
        }
        let key = card.word.trim().to_lowercase();
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        let mut v = serde_json::to_value(&card).unwrap_or_default();
        if let Some(obj) = v.as_object_mut() {
            obj.insert(
                "progress".into(),
                json!({
                    "level": prow.data.level,
                    "lastReviewed": srs::fmt(prow.data.last_reviewed),
                    "nextReview": srs::fmt(prow.data.next_review),
                }),
            );
        }
        cards.push(v);
    }

    let total = cards.len() as i64;
    let total_pages = if total == 0 {
        0
    } else {
        (total + limit - 1) / limit
    };
    let page = if total_pages > 0 {
        page.min(total_pages)
    } else {
        1
    };
    let start = ((page - 1) * limit) as usize;
    let paged: Vec<Value> = cards.into_iter().skip(start).take(limit as usize).collect();

    Ok(json!({
        "cards": paged,
        "page": page,
        "limit": limit,
        "total": total,
        "totalPages": total_pages,
        "hasNext": total_pages > 0 && page < total_pages,
        "hasPrevious": page > 1 && total_pages > 0,
    }))
}

pub fn stats_level(db: &Db) -> Result<Value> {
    let hist = db.level_histogram()?;
    let mut detailed = Map::new();
    let mut groups = [0i64; 6]; // levels 0..=5
    let mut level6_plus = 0i64;
    let mut total = 0i64;
    let mut learned = 0i64;
    for (level, count) in hist {
        detailed.insert(level.to_string(), json!(count));
        total += count;
        if level > 0 {
            learned += count;
        }
        if (0..=5).contains(&level) {
            groups[level as usize] = count;
        } else if level >= 6 {
            level6_plus += count;
        }
    }
    Ok(json!({
        "byLevel": {
            "level0": groups[0], "level1": groups[1], "level2": groups[2],
            "level3": groups[3], "level4": groups[4], "level5": groups[5],
            "level6Plus": level6_plus,
        },
        "totalWords": total,
        "totalLearned": learned,
        "newWords": groups[0],
        "detailed": detailed,
    }))
}

/// GET /study/overview — everything the home dashboard needs in ONE request.
///
/// The dashboard used to fan out 5 calls on mount; inside the Space App iframe
/// that is 5 round-trips before the first meaningful paint, and the numbers
/// could disagree with each other (each call sees a different `now`).
pub fn overview(db: &Db) -> Result<Value> {
    let now = Utc::now();
    let s = db.settings()?;
    let tz = srs::parse_tz(&s.timezone);

    let story_learned = crate::story::story_learned_card_ids(db)?;
    let new_available = db
        .cards_without_progress()?
        .iter()
        .filter(|c| !story_learned.contains(&c.id))
        .count() as i64;

    let lessons = db.list_lessons()?;
    let total_cards: i64 = lessons.iter().map(|l| l.card_count).sum();

    let count = |sql: &str| -> Result<i64> { db.with(|c| c.query_row(sql, [], |r| r.get(0))) };
    let stories = count("SELECT COUNT(*) FROM stories")?;
    let dictation_lessons = count("SELECT COUNT(*) FROM dictation_lessons")?;
    let dictation_in_progress =
        count("SELECT COUNT(*) FROM dictation_progress WHERE completion_percentage > 0")?;
    let grammar_total = count("SELECT COUNT(*) FROM grammars")?;

    let snoozed_until = s
        .snooze_until
        .as_deref()
        .and_then(srs::parse)
        .filter(|t| *t > now)
        .map(srs::fmt);

    Ok(json!({
        // What to do right now
        "dueNow": db.due_count(now)?,
        "newAvailable": new_available,
        "snoozedUntil": snoozed_until,
        // Momentum
        "currentStreak": s.current_streak,
        "totalXP": s.total_xp,
        "dailyWordGoal": s.daily_word_goal,
        "today": stats_today(db)?,
        // Memory state
        "levels": stats_level(db)?,
        "learnedWords": db.learned_count()?,
        // Library
        "library": {
            "lessons": lessons.len(),
            "cards": total_cards,
            "grammars": grammar_total,
            "grammarDue": crate::grammar::due_reminder_count(db)?,
            "stories": stories,
            "dictationLessons": dictation_lessons,
            "dictationInProgress": dictation_in_progress,
        },
        "timezone": s.timezone,
        "studySlots": s.study_slots,
        "nextSlot": next_study_slot(&s.study_slots, tz, now),
    }))
}

/// The next study slot as an ISO instant, so the UI can say "khung giờ tới: 20:00".
fn next_study_slot(slots: &[String], tz: chrono_tz::Tz, now: DateTime<Utc>) -> Value {
    let mut best: Option<DateTime<Utc>> = None;
    for slot in slots {
        let mut it = slot.split(':');
        let (Some(h), Some(m)) = (
            it.next().and_then(|v| v.parse::<u32>().ok()),
            it.next().and_then(|v| v.parse::<u32>().ok()),
        ) else {
            continue;
        };
        for day_offset in 0..2 {
            let date = (now.with_timezone(&tz) + Duration::days(day_offset)).date_naive();
            let cand = srs::local_instant(date, h.min(23), m.min(59), tz);
            if cand > now && best.map_or(true, |b| cand < b) {
                best = Some(cand);
            }
        }
    }
    best.map(|t| json!(srs::fmt(t))).unwrap_or(Value::Null)
}

pub fn stats_today(db: &Db) -> Result<Value> {
    let s = db.settings()?;
    let tz = srs::parse_tz(&s.timezone);
    let now = Utc::now();
    let from = srs::start_of_day(tz, now);
    let to = srs::end_of_day(tz, now);
    Ok(json!({
        "newWordsToday": db.count_created_between(from, to)?,
        "reviewedWordsToday": db.count_reviewed_between(from, to)?,
    }))
}

// ---- practice modes (Review / Matching / Listening / Writing) ----
//
// All four share kaizen's pool logic: learned cards (level > 0), minus cards
// seen in the last 24h (`review_sessions` marks), 10 from levels 1-3 plus 5
// from levels 4+, shuffled. Only the Review mode marks cards on fetch and
// feeds streak/study-log on submit; the three games record marks on submit
// only and never touch SRS progress.

fn practice_progress_json(p: &ProgressData) -> Value {
    json!({ "level": p.level, "lastReviewed": srs::fmt(p.last_reviewed) })
}

fn select_practice_pool(
    pool: &[(Card, crate::db::ProgressRow)],
    exclude: &std::collections::HashSet<String>,
) -> Vec<usize> {
    let priority: Vec<usize> = pool
        .iter()
        .enumerate()
        .filter(|(_, (c, p))| !exclude.contains(&c.id) && (1..=3).contains(&p.data.level))
        .map(|(i, _)| i)
        .take(10)
        .collect();
    let others: Vec<usize> = pool
        .iter()
        .enumerate()
        .filter(|(i, (c, p))| !exclude.contains(&c.id) && p.data.level > 3 && !priority.contains(i))
        .map(|(i, _)| i)
        .take(5)
        .collect();
    priority.into_iter().chain(others).take(15).collect()
}

/// GET /review/session (and /review/session/lesson/:id).
pub fn review_session(db: &Db, allow_repeat: bool, lesson_id: Option<&str>) -> Result<Value> {
    let now = Utc::now();
    let one_day_ago = now - Duration::days(1);

    let lesson = match lesson_id {
        Some(id) => Some(
            db.get_lesson(id)?
                .ok_or_else(|| anyhow!("Không tìm thấy bài học"))?,
        ),
        None => None,
    };

    if allow_repeat {
        db.delete_recent_review_sessions(one_day_ago, None, lesson_id)?;
    }

    // Learned cards, most recently reviewed first (db order), scoped to the
    // lesson when given.
    let pool: Vec<_> = db
        .progress_with_cards()?
        .into_iter()
        .filter(|(c, p)| p.data.level > 0 && lesson_id.map_or(true, |id| c.lesson_id == id))
        .collect();

    let exclude = if allow_repeat {
        Default::default()
    } else {
        db.recently_reviewed_ids(one_day_ago)?
    };

    let mut picked: Vec<&(Card, crate::db::ProgressRow)> = select_practice_pool(&pool, &exclude)
        .into_iter()
        .map(|i| &pool[i])
        .collect();

    // Everything was seen recently → fall back to the 15 longest-unreviewed
    // learned cards (kaizen's global-review fallback) so the page never dead-ends.
    if picked.is_empty() && !allow_repeat && !pool.is_empty() && lesson_id.is_none() {
        let mut oldest: Vec<&(Card, crate::db::ProgressRow)> = pool.iter().collect();
        oldest.sort_by_key(|(_, p)| p.data.last_reviewed);
        picked = oldest.into_iter().take(15).collect();
    }

    picked.shuffle(&mut rand::thread_rng());

    // Mark fetched cards so a re-opened session doesn't repeat them (Review
    // mode only, and never when the caller asked to repeat).
    if !allow_repeat {
        let marked = db.recently_reviewed_ids(one_day_ago)?;
        for (c, _) in &picked {
            if !marked.contains(&c.id) {
                db.insert_review_session(&c.id, false, now)?;
            }
        }
    }

    let cards: Vec<Value> = picked
        .iter()
        .map(|(c, p)| card_json(c, Some(practice_progress_json(&p.data))))
        .collect();
    let mut out = json!({ "cards": cards, "total": cards.len() });
    if let Some(l) = lesson {
        out.as_object_mut()
            .unwrap()
            .insert("lesson".into(), json!({ "id": l.id, "title": l.title }));
    }
    Ok(out)
}

/// POST /review/submit/:cardId — correct confirms the 24h mark, wrong deletes
/// it so the card re-enters the pool; either way streak + a study log tick.
pub fn review_submit(db: &Db, card_id: &str, is_correct: bool) -> Result<Value> {
    if db.get_card(card_id)?.is_none() {
        return Err(anyhow!("Không tìm thấy thẻ"));
    }
    let now = Utc::now();
    let one_day_ago = now - Duration::days(1);
    if is_correct {
        if db.confirm_review_sessions(card_id, one_day_ago, now)? == 0 {
            db.insert_review_session(card_id, true, now)?;
        }
    } else {
        db.delete_recent_review_sessions(one_day_ago, Some(card_id), None)?;
    }
    update_streak(db)?;
    let score = i64::from(is_correct);
    db.add_xp(5 + score)?; // cardsReviewed*5 + gameScore, as in kaizen's log
    db.insert_study_log(30, 0, 1, Some(score), 5 + score)?;
    Ok(json!({ "success": true, "isCorrect": is_correct }))
}

/// POST /review/submit/batch.
pub fn review_submit_batch(db: &Db, results: &[Value]) -> Result<Value> {
    let now = Utc::now();
    let one_day_ago = now - Duration::days(1);
    let mut submitted = 0i64;
    let mut correct = 0i64;
    for r in results {
        let card_id = r["cardId"].as_str().unwrap_or("");
        if card_id.is_empty() || db.get_card(card_id)?.is_none() {
            continue;
        }
        let is_correct = r["isCorrect"].as_bool().unwrap_or(false);
        if is_correct {
            correct += 1;
            if db.confirm_review_sessions(card_id, one_day_ago, now)? == 0 {
                db.insert_review_session(card_id, true, now)?;
            }
        } else {
            db.delete_recent_review_sessions(one_day_ago, Some(card_id), None)?;
        }
        submitted += 1;
    }
    if submitted > 0 {
        update_streak(db)?;
        let xp = submitted * 5 + correct;
        db.add_xp(xp)?;
        db.insert_study_log(30 * submitted, 0, submitted, Some(correct), xp)?;
    }
    Ok(json!({ "success": true, "submitted": submitted, "total": results.len() }))
}

/// GET /matching|listening|writing/session — same pool, no mark-on-fetch.
pub fn game_session(db: &Db) -> Result<Value> {
    let one_day_ago = Utc::now() - Duration::days(1);
    let pool: Vec<_> = db
        .progress_with_cards()?
        .into_iter()
        .filter(|(_, p)| p.data.level > 0)
        .collect();
    let exclude = db.recently_reviewed_ids(one_day_ago)?;
    let mut idx = select_practice_pool(&pool, &exclude);
    if idx.is_empty() {
        // kaizen's matching fallback: re-select ignoring the 24h marks.
        idx = select_practice_pool(&pool, &Default::default());
    }
    let mut picked: Vec<&(Card, crate::db::ProgressRow)> =
        idx.into_iter().map(|i| &pool[i]).collect();
    picked.shuffle(&mut rand::thread_rng());
    let cards: Vec<Value> = picked
        .iter()
        .map(|(c, p)| card_json(c, Some(practice_progress_json(&p.data))))
        .collect();
    Ok(json!({ "cards": cards, "total": cards.len() }))
}

/// POST /matching|listening|writing/submit/:cardId — records the sighting only;
/// never touches SRS progress (kaizen: "KHÔNG update user_card_progress").
pub fn game_submit(db: &Db, card_id: &str, is_correct: bool) -> Result<Value> {
    if db.get_card(card_id)?.is_none() {
        return Err(anyhow!("Không tìm thấy thẻ"));
    }
    db.insert_review_session(card_id, is_correct, Utc::now())?;
    Ok(json!({ "success": true, "isCorrect": is_correct }))
}

/// GET /study/spaced-repetition/:id — the notification deep-link. Kaen has no
/// notification rows yet; serve the cards actually due right now instead.
pub fn due_session(db: &Db) -> Result<Value> {
    let now = Utc::now();
    let pool = db.progress_with_cards()?;
    let mut due: Vec<_> = pool
        .iter()
        .filter(|(_, p)| p.data.next_review <= now)
        .collect();
    due.sort_by(|(_, a), (_, b)| {
        b.data
            .is_urgent
            .cmp(&a.data.is_urgent)
            .then(a.data.next_review.cmp(&b.data.next_review))
    });
    let cards: Vec<Value> = due
        .into_iter()
        .take(20)
        .map(|(c, p)| card_json(c, Some(progress_json(&p.data))))
        .collect();
    let total = cards.len();
    Ok(json!({ "cards": cards, "totalNew": 0, "totalReview": total }))
}

/// GET /lessons with kaizen's paginated envelope.
pub fn lessons_page(db: &Db, search: Option<&str>, page: i64, limit: i64) -> Result<Value> {
    let page = page.max(1);
    let limit = limit.clamp(1, 100);
    let all = db.list_lessons()?;
    let filtered: Vec<_> = match search.map(str::to_lowercase).filter(|q| !q.is_empty()) {
        Some(q) => all
            .into_iter()
            .filter(|l| {
                l.title.to_lowercase().contains(&q)
                    || l.description
                        .as_deref()
                        .is_some_and(|d| d.to_lowercase().contains(&q))
            })
            .collect(),
        None => all,
    };
    let total = filtered.len() as i64;
    let total_pages = if total == 0 {
        0
    } else {
        (total + limit - 1) / limit
    };
    let page = if total_pages > 0 {
        page.min(total_pages)
    } else {
        1
    };
    let start = ((page - 1) * limit) as usize;
    let items: Vec<_> = filtered
        .into_iter()
        .skip(start)
        .take(limit as usize)
        .collect();
    Ok(json!({
        "lessons": items,
        "total": total,
        "totalPages": total_pages,
        "hasNext": total_pages > 0 && page < total_pages,
        "hasPrevious": page > 1 && total_pages > 0,
        "page": page,
        "limit": limit,
    }))
}

// ---- lessons ----

pub fn lesson_json(db: &Db, lesson_id: &str, with_cards: bool) -> Result<Value> {
    let lesson = db
        .get_lesson(lesson_id)?
        .ok_or_else(|| anyhow!("Không tìm thấy bài học"))?;
    let mut v = serde_json::to_value(&lesson)?;
    if with_cards {
        let cards: Vec<Value> = db
            .cards_of_lesson(lesson_id)?
            .iter()
            .map(|c| card_json(c, None))
            .collect();
        v.as_object_mut()
            .unwrap()
            .insert("cards".into(), json!(cards));
    }
    Ok(v)
}

/// Port of `import`: one card per line,
/// `word|meaning|example|partOfSpeech|ipa|explain|other:mean,pairs`.
pub fn import_lesson(db: &Db, title: &str, raw_text: &str, separator: &str) -> Result<Value> {
    let separator = if separator.is_empty() { "|" } else { separator };
    let lesson = db.create_lesson(title, None)?;

    let mut count = 0usize;
    for line in raw_text.trim().lines() {
        let parts: Vec<&str> = line.split(separator).map(str::trim).collect();
        if parts.len() < 2 || parts[0].is_empty() {
            continue;
        }
        let word = parts[0];
        let meaning = parts[1];
        let example = parts.get(2).filter(|v| !v.is_empty());
        let part_of_speech = parts.get(3).filter(|v| !v.is_empty());
        let ipa = parts.get(4).filter(|v| !v.is_empty());
        let explain = parts.get(5).copied().unwrap_or("");

        let mut meanings = Map::new();
        if !meaning.is_empty() {
            meanings.insert("vi".into(), json!(meaning));
        }
        if let Some(extra) = parts.get(6) {
            for pair in extra.split(',') {
                if let Some((code, val)) = pair.split_once(':') {
                    let (code, val) = (code.trim(), val.trim());
                    if !code.is_empty() && !val.is_empty() {
                        meanings.insert(code.into(), json!(val));
                    }
                }
            }
        }

        let examples = example.map(|e| json!([e]));
        db.insert_card(
            &lesson.id,
            word,
            None,
            ipa.copied(),
            part_of_speech.copied(),
            examples.as_ref(),
            explain,
            (!meanings.is_empty())
                .then(|| Value::Object(meanings))
                .as_ref(),
        )?;
        count += 1;
    }
    let _ = count;
    lesson_json(db, &lesson.id, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_memory().unwrap()
    }

    fn seed_lesson(db: &Db, n: usize) -> String {
        let lesson = db.create_lesson("Test", None).unwrap();
        for i in 0..n {
            db.insert_card(
                &lesson.id,
                &format!("word{i}"),
                None,
                None,
                None,
                None,
                &format!("explain {i}"),
                None,
            )
            .unwrap();
        }
        lesson.id
    }

    #[test]
    fn import_parses_the_pipe_format() {
        let db = db();
        let out = import_lesson(
            &db,
            "Cơ bản",
            "Apple|Quả táo|I eat an apple|noun|/ˈæp.əl/|A round fruit|jp:りんご\nRun|Chạy\nbroken-line",
            "|",
        )
        .unwrap();
        let cards = out["cards"].as_array().unwrap();
        assert_eq!(cards.len(), 2, "the 1-column line is skipped");
        assert_eq!(cards[0]["word"], "Apple");
        assert_eq!(cards[0]["meanings"]["vi"], "Quả táo");
        assert_eq!(cards[0]["meanings"]["jp"], "りんご");
        assert_eq!(cards[0]["ipa"], "/ˈæp.əl/");
        assert_eq!(cards[0]["examples"][0], "I eat an apple");
        assert_eq!(cards[1]["explain"], "");
    }

    #[test]
    fn session_returns_only_new_cards_before_anything_is_learned() {
        let db = db();
        seed_lesson(&db, 8);
        let s = session(&db).unwrap();
        assert_eq!(s["cards"].as_array().unwrap().len(), 5);
        assert_eq!(s["totalNew"], 8);
        assert_eq!(s["totalReview"], 0);
        assert!(s["cards"][0]["progress"]["isUrgent"].as_bool().unwrap());
    }

    #[test]
    fn grading_a_new_card_creates_progress_and_awards_xp() {
        let db = db();
        let lesson_id = seed_lesson(&db, 1);
        let card_id = db.cards_of_lesson(&lesson_id).unwrap()[0].id.clone();

        let out = submit_review(&db, &card_id, "REMEMBER", "TYPING").unwrap();
        assert_eq!(out["xpEarned"], 15);
        assert_eq!(out["progress"]["level"], 1);

        let s = db.settings().unwrap();
        assert_eq!(s.total_xp, 15);
        assert_eq!(s.current_streak, 1);
        assert!(s.last_study_date.is_some());

        // Second grade the same day: XP accrues, streak holds.
        let out2 = submit_review(&db, &card_id, "FORGOT", "FLIP").unwrap();
        assert_eq!(out2["xpEarned"], 10);
        let s2 = db.settings().unwrap();
        assert_eq!(s2.total_xp, 25);
        assert_eq!(s2.current_streak, 1);
    }

    #[test]
    fn review_batch_writes_one_log_and_double_counts_xp_like_kaizen() {
        let db = db();
        let lesson_id = seed_lesson(&db, 2);
        let ids: Vec<String> = db
            .cards_of_lesson(&lesson_id)
            .unwrap()
            .iter()
            .map(|c| c.id.clone())
            .collect();
        let reviews = vec![
            json!({ "cardId": ids[0], "result": "REMEMBER", "mode": "FLIP" }),
            json!({ "cardId": ids[1], "result": "FORGOT", "mode": "TYPING" }),
            json!({ "cardId": "missing", "result": "REMEMBER", "mode": "FLIP" }),
        ];
        let out = review_batch(&db, &reviews, 360, 2, 1, Some(3)).unwrap();
        assert_eq!(out["processed"], 2);
        assert_eq!(out["totalXP"], 20);
        assert_eq!(out["studyLog"]["xpEarned"], 2 * 10 + 5 + 3);
        // Kaizen adds both pools to the user's total XP; the port preserves it.
        assert_eq!(db.settings().unwrap().total_xp, 20 + 28);
    }

    #[test]
    fn learned_cards_dedupes_by_word_and_paginates() {
        let db = db();
        let l1 = seed_lesson(&db, 1);
        let l2 = db.create_lesson("Dup", None).unwrap();
        db.insert_card(
            &l2.id,
            "word0",
            None,
            None,
            None,
            None,
            "dup of word0",
            None,
        )
        .unwrap();
        for lesson in [&l1, &l2.id] {
            for c in db.cards_of_lesson(lesson).unwrap() {
                submit_review(&db, &c.id, "REMEMBER", "FLIP").unwrap();
            }
        }
        let out = learned_cards(&db, 1, 20, None, None, None, None).unwrap();
        assert_eq!(out["total"], 1, "same word in two lessons counts once");
        let empty = learned_cards(&db, 1, 20, None, None, None, Some("zzz")).unwrap();
        assert_eq!(empty["total"], 0);
    }

    #[test]
    fn overview_answers_what_to_study_now_in_one_call() {
        let db = db();
        let lesson_id = seed_lesson(&db, 4);
        let cards = db.cards_of_lesson(&lesson_id).unwrap();

        let o = overview(&db).unwrap();
        assert_eq!(o["dueNow"], 0);
        assert_eq!(o["newAvailable"], 4);
        assert_eq!(o["currentStreak"], 0);
        assert_eq!(o["library"]["lessons"], 1);
        assert_eq!(o["library"]["cards"], 4);
        assert!(o["snoozedUntil"].is_null());
        // Study slots default to 08:00/20:00 → the next one is always resolvable.
        assert!(
            o["nextSlot"].is_string(),
            "next slot instant: {:?}",
            o["nextSlot"]
        );

        // Grading a card makes it due in 30 minutes (not now) and starts a streak.
        submit_review(&db, &cards[0].id, "REMEMBER", "FLIP").unwrap();
        let o = overview(&db).unwrap();
        assert_eq!(o["dueNow"], 0, "the 30-minute retry is not due yet");
        assert_eq!(o["newAvailable"], 3, "graded card left the new pool");
        assert_eq!(o["currentStreak"], 1);
        assert_eq!(o["today"]["newWordsToday"], 1);
        assert_eq!(o["levels"]["byLevel"]["level1"], 1);
        assert_eq!(o["learnedWords"], 1);

        // Snooze surfaces only while it is still in the future.
        snooze(&db, 2).unwrap();
        assert!(overview(&db).unwrap()["snoozedUntil"].is_string());
        snooze(&db, 0).unwrap();
        assert!(
            overview(&db).unwrap()["snoozedUntil"].is_null(),
            "expired snooze is hidden"
        );
    }

    #[test]
    fn stats_shapes_match_kaizen() {
        let db = db();
        let lesson_id = seed_lesson(&db, 3);
        for c in db.cards_of_lesson(&lesson_id).unwrap() {
            submit_review(&db, &c.id, "REMEMBER", "FLIP").unwrap();
        }
        let lv = stats_level(&db).unwrap();
        assert_eq!(lv["byLevel"]["level1"], 3);
        assert_eq!(lv["totalLearned"], 3);
        let today = stats_today(&db).unwrap();
        assert_eq!(today["newWordsToday"], 3);
        assert_eq!(today["reviewedWordsToday"], 3);
    }

    #[test]
    fn review_pool_hides_seen_cards_for_24h_and_wrong_answers_requeue() {
        let db = db();
        let lesson_id = seed_lesson(&db, 3);
        let cards = db.cards_of_lesson(&lesson_id).unwrap();
        for c in &cards {
            submit_review(&db, &c.id, "REMEMBER", "FLIP").unwrap();
        }

        // First fetch returns all 3 learned cards and marks them.
        let s1 = review_session(&db, false, None).unwrap();
        assert_eq!(s1["cards"].as_array().unwrap().len(), 3);

        // Re-opening without repeat falls back (nothing unseen) but batch-submit
        // decides what happens next: correct stays hidden, wrong re-enters.
        let results = vec![
            json!({ "cardId": cards[0].id, "isCorrect": true }),
            json!({ "cardId": cards[1].id, "isCorrect": false }),
        ];
        let out = review_submit_batch(&db, &results).unwrap();
        assert_eq!(out["submitted"], 2);

        let s2 = review_session(&db, false, None).unwrap();
        let words: Vec<String> = s2["cards"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["word"].as_str().unwrap().to_string())
            .collect();
        assert!(
            words.contains(&cards[1].word),
            "wrongly-answered card must re-enter the pool: {words:?}"
        );
        assert!(
            !words.contains(&cards[0].word),
            "confirmed card stays hidden"
        );

        // allowRepeat wipes the marks — everything is available again.
        let s3 = review_session(&db, true, None).unwrap();
        assert_eq!(s3["cards"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn game_submit_never_touches_srs_progress() {
        let db = db();
        let lesson_id = seed_lesson(&db, 1);
        let card_id = db.cards_of_lesson(&lesson_id).unwrap()[0].id.clone();
        submit_review(&db, &card_id, "REMEMBER", "FLIP").unwrap();
        let before = db.get_progress(&card_id).unwrap().unwrap();

        let s = game_session(&db).unwrap();
        assert_eq!(s["cards"].as_array().unwrap().len(), 1);
        game_submit(&db, &card_id, false).unwrap();

        let after = db.get_progress(&card_id).unwrap().unwrap();
        assert_eq!(
            before.data, after.data,
            "games must not alter SRS scheduling"
        );
    }

    #[test]
    fn lessons_page_searches_and_paginates() {
        let db = db();
        db.create_lesson("Du lịch", None).unwrap();
        db.create_lesson("Công sở", Some("từ vựng văn phòng"))
            .unwrap();
        let all = lessons_page(&db, None, 1, 1).unwrap();
        assert_eq!(all["total"], 2);
        assert_eq!(all["lessons"].as_array().unwrap().len(), 1);
        assert_eq!(all["hasNext"], true);
        let hit = lessons_page(&db, Some("văn phòng"), 1, 10).unwrap();
        assert_eq!(hit["total"], 1);
        assert_eq!(hit["lessons"][0]["title"], "Công sở");
    }

    #[test]
    fn lesson_session_orders_new_cards_first() {
        let db = db();
        let lesson_id = seed_lesson(&db, 3);
        let cards = db.cards_of_lesson(&lesson_id).unwrap();
        // Learn one card so it gets progress; it must sort after the new ones.
        submit_review(&db, &cards[0].id, "REMEMBER", "FLIP").unwrap();
        let s = lesson_session(&db, &lesson_id).unwrap();
        let out = s["cards"].as_array().unwrap();
        assert_eq!(out.len(), 3);
        assert!(out[0]["progress"].is_null());
        assert!(out[1]["progress"].is_null());
        assert!(!out[2]["progress"].is_null());
        assert_eq!(s["totalNew"], 2);
    }
}
