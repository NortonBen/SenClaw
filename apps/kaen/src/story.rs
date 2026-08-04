//! Stories — 3-step AI reading practice built from a lesson's vocabulary.
//! Port of kaizen's `story` module + `story-ai.service` (Dify → daemon bridge).
//!
//! Steps: STEP1 = English story using every vocab word, STEP2 = mixed
//! (English with native-language hints), STEP3 = full native-language version.
//! kaizen also generated ElevenLabs audio per step; Kaen leaves `audioUrl`
//! null — the web UI reads steps aloud with the browser's speechSynthesis.

use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use crate::db::Db;
use crate::srs;

fn now_s() -> String {
    srs::fmt(Utc::now())
}

fn story_json(db: &Db, id: &str, with_steps: bool) -> Result<Value> {
    let base: Option<Value> = db.with(|c| {
        c.query_row(
            "SELECT s.id, s.title, s.topic, s.description, s.lesson_id, s.created_at, l.title AS lesson_title
             FROM stories s JOIN lessons l ON l.id = s.lesson_id WHERE s.id = ?1",
            params![id],
            |r| {
                Ok(json!({
                    "id": r.get::<_, String>(0)?,
                    "title": r.get::<_, String>(1)?,
                    "topic": r.get::<_, Option<String>>(2)?,
                    "description": r.get::<_, Option<String>>(3)?,
                    "lessonId": r.get::<_, String>(4)?,
                    "createdAt": r.get::<_, String>(5)?,
                    "visibility": "PRIVATE",
                    "isOwner": true,
                    "lesson": { "id": r.get::<_, String>(4)?, "title": r.get::<_, String>(6)? },
                }))
            },
        )
        .optional()
    })?;
    let mut v = base.ok_or_else(|| anyhow!("Không tìm thấy story"))?;

    if with_steps {
        let steps: Vec<Value> = db.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, step_type, primary_language, content, ord, audio_url
                 FROM story_steps WHERE story_id = ?1 ORDER BY ord ASC",
            )?;
            let rows = stmt.query_map(params![id], |r| {
                Ok(json!({
                    "id": r.get::<_, String>(0)?,
                    "stepType": r.get::<_, String>(1)?,
                    "primaryLanguage": r.get::<_, String>(2)?,
                    "content": r.get::<_, String>(3)?,
                    "order": r.get::<_, i64>(4)?,
                    "audioUrl": r.get::<_, Option<String>>(5)?,
                }))
            })?;
            rows.collect()
        })?;
        let lesson_id = v["lessonId"].as_str().unwrap().to_string();
        let cards: Vec<Value> = db
            .cards_of_lesson(&lesson_id)?
            .iter()
            .map(|c| serde_json::to_value(c).unwrap_or_default())
            .collect();
        let o = v.as_object_mut().unwrap();
        o.insert("steps".into(), json!(steps));
        o["lesson"]
            .as_object_mut()
            .unwrap()
            .insert("cards".into(), json!(cards));
        o.insert("progress".into(), progress_json(db, id)?);
    }
    Ok(v)
}

pub fn list_stories(db: &Db) -> Result<Value> {
    let ids: Vec<String> = db.with(|c| {
        let mut stmt = c.prepare("SELECT id FROM stories ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    })?;
    let mut out = Vec::new();
    for id in ids {
        out.push(story_json(db, &id, false)?);
    }
    Ok(json!(out))
}

pub fn get_story(db: &Db, id: &str) -> Result<Value> {
    story_json(db, id, true)
}

pub fn create_story(db: &Db, body: &Value) -> Result<Value> {
    let title = body["title"].as_str().unwrap_or("").trim().to_string();
    let lesson_id = body["lessonId"].as_str().unwrap_or("");
    if title.is_empty() || lesson_id.is_empty() {
        return Err(anyhow!("Thiếu title hoặc lessonId"));
    }
    if db.get_lesson(lesson_id)?.is_none() {
        return Err(anyhow!("Không tìm thấy lesson nguồn"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    db.with(|c| {
        c.execute(
            "INSERT INTO stories (id, title, topic, description, lesson_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                title,
                body["topic"].as_str(),
                body["description"].as_str(),
                lesson_id,
                now_s()
            ],
        )
    })?;
    if let Some(steps) = body["steps"].as_array() {
        replace_steps(db, &id, steps)?;
    }
    story_json(db, &id, true)
}

fn replace_steps(db: &Db, story_id: &str, steps: &[Value]) -> Result<()> {
    db.with(|c| {
        c.execute(
            "DELETE FROM story_steps WHERE story_id = ?1",
            params![story_id],
        )
    })?;
    for s in steps {
        let step_type = s["stepType"].as_str().unwrap_or("STEP1");
        let lang = s["primaryLanguage"].as_str().unwrap_or("en");
        db.with(|c| {
            c.execute(
                "INSERT INTO story_steps (id, story_id, step_type, primary_language, content, ord, audio_url)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    story_id,
                    step_type,
                    lang,
                    s["content"].as_str().unwrap_or(""),
                    s["order"].as_i64().unwrap_or(1),
                    s["audioUrl"].as_str(),
                ],
            )
        })?;
    }
    Ok(())
}

pub fn update_story(db: &Db, id: &str, body: &Value) -> Result<Value> {
    story_json(db, id, false)?;
    db.with(|c| {
        c.execute(
            "UPDATE stories SET
               title = COALESCE(?2, title),
               topic = COALESCE(?3, topic),
               description = COALESCE(?4, description)
             WHERE id = ?1",
            params![
                id,
                body["title"].as_str(),
                body["topic"].as_str(),
                body["description"].as_str()
            ],
        )
    })?;
    if let Some(steps) = body["steps"].as_array().filter(|s| !s.is_empty()) {
        replace_steps(db, id, steps)?;
    }
    story_json(db, id, true)
}

pub fn delete_story(db: &Db, id: &str) -> Result<Value> {
    story_json(db, id, false)?;
    db.with(|c| c.execute("DELETE FROM stories WHERE id = ?1", params![id]))?;
    Ok(json!({ "message": "Đã xóa story thành công" }))
}

// ---- progress ----

fn progress_json(db: &Db, story_id: &str) -> Result<Value> {
    let row: Option<Value> = db.with(|c| {
        c.query_row(
            "SELECT current_step, completed_steps, viewed_vocab_ids, listened_vocab_ids,
                    total_reading_time, tts_sessions_count, last_accessed_at, completed_at
             FROM story_progress WHERE story_id = ?1",
            params![story_id],
            |r| {
                let arr = |s: String| serde_json::from_str::<Value>(&s).unwrap_or(json!([]));
                Ok(json!({
                    "currentStep": r.get::<_, i64>(0)?,
                    "completedSteps": arr(r.get(1)?),
                    "viewedVocabIds": arr(r.get(2)?),
                    "listenedVocabIds": arr(r.get(3)?),
                    "totalReadingTime": r.get::<_, i64>(4)?,
                    "ttsSessionsCount": r.get::<_, i64>(5)?,
                    "lastAccessedAt": r.get::<_, Option<String>>(6)?,
                    "completedAt": r.get::<_, Option<String>>(7)?,
                }))
            },
        )
        .optional()
    })?;
    Ok(row.unwrap_or(json!({
        "currentStep": 1,
        "completedSteps": [],
        "viewedVocabIds": [],
        "listenedVocabIds": [],
        "totalReadingTime": 0,
        "ttsSessionsCount": 0,
    })))
}

pub fn get_progress(db: &Db, story_id: &str) -> Result<Value> {
    story_json(db, story_id, false)?;
    progress_json(db, story_id)
}

/// POST /stories/:id/progress — kaizen semantics: currentStep/completedSteps
/// replace, vocab-id lists MERGE, reading time and TTS count accumulate;
/// completing all 3 steps stamps completedAt.
pub fn update_progress(db: &Db, story_id: &str, dto: &Value) -> Result<Value> {
    story_json(db, story_id, false)?;
    let existing = progress_json(db, story_id)?;
    let is_new = db.with(|c| {
        c.query_row(
            "SELECT 1 FROM story_progress WHERE story_id = ?1",
            params![story_id],
            |_| Ok(()),
        )
        .optional()
        .map(|v| v.is_none())
    })?;

    let current_step = dto["currentStep"]
        .as_i64()
        .unwrap_or_else(|| existing["currentStep"].as_i64().unwrap_or(1));
    let completed: Vec<i64> = dto["completedSteps"]
        .as_array()
        .or_else(|| existing["completedSteps"].as_array())
        .map(|a| a.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();

    let merge = |key: &str| -> Vec<String> {
        let mut set: Vec<String> = existing[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(new) = dto[key].as_array() {
            for v in new.iter().filter_map(Value::as_str) {
                if !set.iter().any(|x| x == v) {
                    set.push(v.to_string());
                }
            }
        }
        set
    };
    let viewed = merge("viewedVocabIds");
    let listened = merge("listenedVocabIds");

    let reading = existing["totalReadingTime"].as_i64().unwrap_or(0)
        + dto["additionalReadingTime"].as_i64().unwrap_or(0);
    let tts = existing["ttsSessionsCount"].as_i64().unwrap_or(0)
        + dto["incrementTtsCount"].as_i64().unwrap_or(0);
    let completed_at = (completed.len() >= 3).then(now_s);
    let now = now_s();

    db.with(|c| {
        c.execute(
            "INSERT INTO story_progress
               (story_id, current_step, completed_steps, viewed_vocab_ids, listened_vocab_ids,
                total_reading_time, tts_sessions_count, started_at, last_accessed_at, completed_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?8)
             ON CONFLICT(story_id) DO UPDATE SET
               current_step = excluded.current_step,
               completed_steps = excluded.completed_steps,
               viewed_vocab_ids = excluded.viewed_vocab_ids,
               listened_vocab_ids = excluded.listened_vocab_ids,
               total_reading_time = excluded.total_reading_time,
               tts_sessions_count = excluded.tts_sessions_count,
               last_accessed_at = excluded.last_accessed_at,
               completed_at = COALESCE(excluded.completed_at, story_progress.completed_at)",
            params![
                story_id,
                current_step,
                json!(completed).to_string(),
                json!(viewed).to_string(),
                json!(listened).to_string(),
                reading,
                tts,
                now,
                completed_at,
            ],
        )
    })?;
    let _ = is_new;
    progress_json(db, story_id)
}

/// Card ids "learned in stories" — kaizen's session generator excludes these
/// from the new-word pool (a word met in a story isn't brand-new anymore).
pub fn story_learned_card_ids(db: &Db) -> Result<std::collections::HashSet<String>> {
    let rows: Vec<(String, String)> = db.with(|c| {
        let mut stmt =
            c.prepare("SELECT viewed_vocab_ids, listened_vocab_ids FROM story_progress")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    })?;
    let mut out = std::collections::HashSet::new();
    for (viewed, listened) in rows {
        for raw in [viewed, listened] {
            if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(&raw) {
                out.extend(a.iter().filter_map(|v| v.as_str().map(String::from)));
            }
        }
    }
    Ok(out)
}

// ---- AI generation ----

/// Extract kaizen's `{step1:{content},step2,step3}` object; same fallback:
/// unparseable output becomes the content of every step.
pub fn parse_story_response(text: &str) -> (String, String, String) {
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        if start < end {
            if let Ok(v) = serde_json::from_str::<Value>(&text[start..=end]) {
                if v.get("step1").is_some() {
                    let get = |k: &str| v[k]["content"].as_str().unwrap_or("").to_string();
                    let (s1, s2, s3) = (get("step1"), get("step2"), get("step3"));
                    if !s1.is_empty() {
                        return (s1, s2, s3);
                    }
                }
            }
        }
    }
    (text.to_string(), text.to_string(), text.to_string())
}

pub fn build_story_prompt(
    title: &str,
    description: &str,
    vocab: &[(String, String, String)],
    native_language: &str,
) -> String {
    let vocab_lines: String = vocab
        .iter()
        .map(|(w, m, pos)| format!("- {w} ({pos}): {m}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Write a short learning story titled \"{title}\" for an English learner. {description}\n\
         Use EVERY word from this vocabulary list at least once:\n{vocab_lines}\n\n\
         Return ONLY a JSON object (no markdown fences) with this exact structure:\n\
         {{\"step1\": {{\"content\": \"...\"}}, \"step2\": {{\"content\": \"...\"}}, \"step3\": {{\"content\": \"...\"}}}}\n\
         - step1: the story fully in English (~150-250 words), simple HTML paragraphs (<p>).\n\
         - step2: the same story in English, but right after each vocabulary word add its {native_language} meaning in parentheses.\n\
         - step3: the same story fully translated into {native_language}.\n\
         Keep vocabulary words in their original English form (no inflection changes) so they can be highlighted."
    )
}

/// POST /stories/generate — build vocab from the lesson, one bridge call,
/// persist story + 3 steps.
pub async fn generate_story(
    db: &Db,
    lesson_id: &str,
    title: &str,
    description: &str,
    native_language: &str,
) -> Result<Value> {
    let lesson = db
        .get_lesson(lesson_id)?
        .ok_or_else(|| anyhow!("Không tìm thấy lesson nguồn"))?;
    let cards = db.cards_of_lesson(lesson_id)?;
    if cards.is_empty() {
        return Err(anyhow!("Lesson chưa có thẻ từ vựng nào"));
    }
    let vocab: Vec<(String, String, String)> = cards
        .iter()
        .map(|c| {
            let meaning = c
                .meanings
                .as_ref()
                .and_then(|m| m.get(native_language).or_else(|| m.get("vi")))
                .and_then(Value::as_str)
                .map(String::from)
                .unwrap_or_else(|| c.explain.clone());
            (
                c.word.clone(),
                meaning,
                c.part_of_speech.clone().unwrap_or_default(),
            )
        })
        .collect();

    let title = if title.trim().is_empty() {
        lesson.title.clone()
    } else {
        title.trim().to_string()
    };
    let prompt = build_story_prompt(&title, description, &vocab, native_language);
    let system = "You are a language-learning story writer. Answer with raw JSON only.";
    let (text, finish) = crate::llm::bridge_llm(system, &prompt, 16_000)
        .await
        .map_err(|e| anyhow!(e))?;
    if finish == "length" {
        return Err(anyhow!(
            "model cắt output giữa chừng (finish=length) — thử lesson ít từ hơn"
        ));
    }
    let (s1, s2, s3) = parse_story_response(&text);

    create_story(
        db,
        &json!({
            "title": title,
            "topic": title,
            "lessonId": lesson_id,
            "description": if description.is_empty() {
                format!("AI-generated story using vocabulary from \"{}\"", lesson.title)
            } else {
                description.to_string()
            },
            "steps": [
                { "stepType": "STEP1", "content": s1, "order": 1, "primaryLanguage": "en" },
                { "stepType": "STEP2", "content": s2, "order": 2, "primaryLanguage": "mixed" },
                { "stepType": "STEP3", "content": s3, "order": 3, "primaryLanguage": native_language },
            ],
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded_db() -> (Db, String) {
        let db = Db::open_memory().unwrap();
        let lesson = db.create_lesson("Truyện test", None).unwrap();
        for w in ["apple", "run"] {
            db.insert_card(&lesson.id, w, None, None, None, None, "explain", None)
                .unwrap();
        }
        (db, lesson.id)
    }

    fn make_story(db: &Db, lesson_id: &str) -> String {
        let s = create_story(
            db,
            &json!({
                "title": "A day",
                "lessonId": lesson_id,
                "steps": [
                    { "stepType": "STEP1", "content": "<p>en</p>", "order": 1 },
                    { "stepType": "STEP2", "content": "<p>mix</p>", "order": 2 },
                    { "stepType": "STEP3", "content": "<p>vi</p>", "order": 3 },
                ],
            }),
        )
        .unwrap();
        s["id"].as_str().unwrap().to_string()
    }

    #[test]
    fn story_crud_carries_steps_and_lesson_cards() {
        let (db, lesson_id) = seeded_db();
        let id = make_story(&db, &lesson_id);
        let s = get_story(&db, &id).unwrap();
        assert_eq!(s["steps"].as_array().unwrap().len(), 3);
        assert_eq!(s["lesson"]["cards"].as_array().unwrap().len(), 2);
        assert_eq!(s["progress"]["currentStep"], 1, "default progress served");

        assert_eq!(list_stories(&db).unwrap().as_array().unwrap().len(), 1);
        delete_story(&db, &id).unwrap();
        assert!(get_story(&db, &id).is_err());
    }

    #[test]
    fn progress_merges_vocab_ids_and_stamps_completion() {
        let (db, lesson_id) = seeded_db();
        let id = make_story(&db, &lesson_id);

        update_progress(
            &db,
            &id,
            &json!({ "currentStep": 2, "viewedVocabIds": ["c1"] }),
        )
        .unwrap();
        let p = update_progress(
            &db,
            &id,
            &json!({ "viewedVocabIds": ["c2", "c1"], "additionalReadingTime": 30 }),
        )
        .unwrap();
        assert_eq!(
            p["viewedVocabIds"].as_array().unwrap().len(),
            2,
            "merged, deduped"
        );
        assert_eq!(p["currentStep"], 2, "unspecified fields keep prior value");
        assert_eq!(p["totalReadingTime"], 30);

        let done = update_progress(&db, &id, &json!({ "completedSteps": [1, 2, 3] })).unwrap();
        assert!(
            done["completedAt"].is_string(),
            "3 steps done → completedAt"
        );

        // The learned-in-story set feeds the vocab session generator.
        let learned = story_learned_card_ids(&db).unwrap();
        assert!(learned.contains("c1") && learned.contains("c2"));
    }

    #[test]
    fn parse_story_response_extracts_or_falls_back() {
        let good = r#"Sure! {"step1":{"content":"<p>A</p>"},"step2":{"content":"<p>B</p>"},"step3":{"content":"<p>C</p>"}}"#;
        assert_eq!(
            parse_story_response(good),
            ("<p>A</p>".into(), "<p>B</p>".into(), "<p>C</p>".into())
        );
        let bad = "just prose";
        let (a, b, c) = parse_story_response(bad);
        assert_eq!(a, bad);
        assert_eq!(b, bad);
        assert_eq!(c, bad);
    }

    #[test]
    fn story_prompt_lists_every_vocab_word() {
        let vocab = vec![
            (
                "apple".to_string(),
                "quả táo".to_string(),
                "noun".to_string(),
            ),
            ("run".to_string(), "chạy".to_string(), "verb".to_string()),
        ];
        let p = build_story_prompt("My day", "", &vocab, "vi");
        assert!(p.contains("- apple (noun): quả táo"));
        assert!(p.contains("- run (verb): chạy"));
        assert!(p.contains("\"step1\""));
    }
}
