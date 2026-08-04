//! Dictation practice — port of kaizen's `dictation-lesson` module.
//! Content (topics → lessons → timed segments) is seeded via JSON import from
//! the dailydictation crawler output; audio stays a remote/served URL and the
//! client seeks segments by start/end time.

use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};

use crate::db::Db;
use crate::srs;

fn now_s() -> String {
    srs::fmt(Utc::now())
}

fn parse_json(s: Option<String>, default: Value) -> Value {
    s.and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or(default)
}

// ---- topics ----

pub fn list_topics(db: &Db) -> Result<Value> {
    db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT t.id, t.name, t.slug, t.description, t.level,
                    (SELECT COUNT(*) FROM dictation_lessons dl WHERE dl.topic_id = t.id) AS lesson_count
             FROM dictation_topics t ORDER BY t.name ASC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "name": r.get::<_, String>(1)?,
                "slug": r.get::<_, String>(2)?,
                "description": r.get::<_, Option<String>>(3)?,
                "level": r.get::<_, Option<String>>(4)?,
                "lessonCount": r.get::<_, i64>(5)?,
            }))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map(|v| json!(v))
    })
    .map_err(Into::into)
}

// ---- lessons ----

fn lesson_summary(db: &Db, id: i64) -> Result<Value> {
    let base: Option<Value> = db.with(|c| {
        c.query_row(
            "SELECT l.id, l.title, l.topic, l.description, l.level, l.audio_url,
                    l.youtube_video_id, l.mode, l.topic_id, l.created_at,
                    t.name, t.slug
             FROM dictation_lessons l LEFT JOIN dictation_topics t ON t.id = l.topic_id
             WHERE l.id = ?1",
            params![id],
            |r| {
                let topic_id: Option<i64> = r.get(8)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "title": r.get::<_, String>(1)?,
                    "topic": r.get::<_, String>(2)?,
                    "description": r.get::<_, Option<String>>(3)?,
                    "level": r.get::<_, Option<String>>(4)?,
                    "audioUrl": r.get::<_, Option<String>>(5)?,
                    "youtubeVideoId": r.get::<_, Option<String>>(6)?,
                    "mode": r.get::<_, String>(7)?,
                    "topicId": topic_id,
                    "createdAt": r.get::<_, String>(9)?,
                    "dictationTopic": topic_id.map(|tid| json!({
                        "id": tid,
                        "name": r.get::<_, Option<String>>(10).ok().flatten(),
                        "slug": r.get::<_, Option<String>>(11).ok().flatten(),
                    })),
                }))
            },
        )
        .optional()
    })?;
    base.ok_or_else(|| anyhow!("Không tìm thấy bài dictation"))
}

fn progress_row(db: &Db, lesson_id: i64) -> Result<Option<(i64, i64, Value, Option<String>)>> {
    db.with(|c| {
        c.query_row(
            "SELECT current_index, completion_percentage, segment_status, last_practiced_at
             FROM dictation_progress WHERE lesson_id = ?1",
            params![lesson_id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    parse_json(r.get(2)?, json!({})),
                    r.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()
    })
    .map_err(Into::into)
}

/// GET /dictation-lessons?topic&level&limit&page.
pub fn list_lessons(
    db: &Db,
    topic: Option<&str>,
    level: Option<&str>,
    limit: i64,
    page: i64,
) -> Result<Value> {
    let limit = limit.clamp(1, 100);
    let page = page.max(1);
    let ids: Vec<i64> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT l.id FROM dictation_lessons l LEFT JOIN dictation_topics t ON t.id = l.topic_id
             WHERE (?1 IS NULL OR t.slug = ?1 OR l.topic = ?1 OR t.name = ?1)
               AND (?2 IS NULL OR l.level = ?2)
             ORDER BY l.id ASC",
        )?;
        let rows = stmt.query_map(params![topic, level], |r| r.get(0))?;
        rows.collect()
    })?;

    let total = ids.len() as i64;
    let mut data = Vec::new();
    for id in ids
        .into_iter()
        .skip(((page - 1) * limit) as usize)
        .take(limit as usize)
    {
        let mut l = lesson_summary(db, id)?;
        let progress = progress_row(db, id)?;
        let user_progress = progress.map(|(_, pct, status, _)| {
            let has_mark = status
                .as_object()
                .is_some_and(|m| m.values().any(|v| v.as_str() == Some("marked")));
            json!({ "percentage": pct, "hasMark": has_mark })
        });
        l.as_object_mut()
            .unwrap()
            .insert("userProgress".into(), user_progress.unwrap_or(Value::Null));
        data.push(l);
    }
    Ok(json!({ "data": data, "total": total, "page": page, "limit": limit }))
}

/// GET /dictation-lessons/:id — full lesson with segments and challenges.
pub fn get_lesson(db: &Db, id: i64) -> Result<Value> {
    let mut l = lesson_summary(db, id)?;
    let segments: Vec<Value> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, content, solutions, start_time, end_time, order_index
             FROM dictation_segments WHERE lesson_id = ?1 ORDER BY order_index ASC",
        )?;
        let rows = stmt.query_map(params![id], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "content": r.get::<_, Option<String>>(1)?,
                "solutions": parse_json(r.get(2)?, json!([])),
                "startTime": r.get::<_, f64>(3)?,
                "endTime": r.get::<_, f64>(4)?,
                "orderIndex": r.get::<_, i64>(5)?,
            }))
        })?;
        rows.collect()
    })?;
    let challenges: Vec<Value> = db.with(|c| {
        let mut stmt =
            c.prepare("SELECT id, options, voices FROM dictation_challenges WHERE lesson_id = ?1")?;
        let rows = stmt.query_map(params![id], |r| {
            Ok(json!({
                "id": r.get::<_, i64>(0)?,
                "options": parse_json(r.get(1)?, json!([])),
                "voices": parse_json(r.get(2)?, json!([])),
            }))
        })?;
        rows.collect()
    })?;
    let o = l.as_object_mut().unwrap();
    o.insert("segments".into(), json!(segments));
    o.insert("pronunciationChallenges".into(), json!(challenges));
    Ok(l)
}

/// Audio for a segment: the client seeks inside the full audio; this endpoint
/// exists for URL-compat with kaizen and just points at the lesson audio.
pub fn lesson_audio_url(db: &Db, id: i64) -> Result<Option<String>> {
    Ok(lesson_summary(db, id)?["audioUrl"]
        .as_str()
        .map(String::from))
}

// ---- progress ----

pub fn get_progress(db: &Db, lesson_id: i64) -> Result<Value> {
    lesson_summary(db, lesson_id)?;
    Ok(match progress_row(db, lesson_id)? {
        Some((idx, pct, status, last)) => json!({
            "currentIndex": idx,
            "completionPercentage": pct,
            "segmentStatus": status,
            "lastPracticedAt": last,
        }),
        None => json!({ "currentIndex": 0, "completionPercentage": 0, "segmentStatus": {} }),
    })
}

/// POST /dictation-lessons/:id/progress — percentage recomputed from the
/// learned segment count over the lesson's item total (kaizen logic).
pub fn save_progress(
    db: &Db,
    lesson_id: i64,
    current_index: i64,
    segment_status: &Value,
) -> Result<Value> {
    let lesson = get_lesson(db, lesson_id)?;
    let total_items = if lesson["mode"] == "pronunciation" {
        lesson["pronunciationChallenges"]
            .as_array()
            .map_or(0, Vec::len)
    } else {
        lesson["segments"].as_array().map_or(0, Vec::len)
    };
    let learned = segment_status.as_object().map_or(0, |m| {
        m.values().filter(|v| v.as_str() == Some("learned")).count()
    });
    let pct = if total_items == 0 {
        0
    } else {
        ((learned as f64 / total_items as f64) * 100.0).round() as i64
    };

    db.with(|c| {
        c.execute(
            "INSERT INTO dictation_progress (lesson_id, current_index, completion_percentage, segment_status, last_practiced_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT(lesson_id) DO UPDATE SET
               current_index = excluded.current_index,
               completion_percentage = excluded.completion_percentage,
               segment_status = excluded.segment_status,
               last_practiced_at = excluded.last_practiced_at",
            params![
                lesson_id,
                current_index,
                pct,
                segment_status.to_string(),
                now_s()
            ],
        )
    })?;
    get_progress(db, lesson_id)
}

/// GET /dictation-lessons/history/me — practiced lessons, most recent first.
pub fn history(db: &Db) -> Result<Value> {
    let rows: Vec<(i64, i64, Option<String>)> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT lesson_id, completion_percentage, last_practiced_at
             FROM dictation_progress ORDER BY last_practiced_at DESC",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        rows.collect()
    })?;
    let mut out = Vec::new();
    for (lesson_id, pct, last) in rows {
        if let Ok(mut l) = lesson_summary(db, lesson_id) {
            let o = l.as_object_mut().unwrap();
            o.insert("completionPercentage".into(), json!(pct));
            o.insert("lastPracticedAt".into(), json!(last));
            out.push(l);
        }
    }
    Ok(json!(out))
}

// ---- admin CRUD ----

fn slugify_simple(text: &str) -> String {
    crate::grammar::slugify(text)
}

pub fn create_topic(db: &Db, body: &Value) -> Result<Value> {
    let name = body["name"].as_str().unwrap_or("").trim().to_string();
    if name.is_empty() {
        return Err(anyhow!("Thiếu tên chủ đề"));
    }
    let slug = match body["slug"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(s) => s.to_string(),
        None => slugify_simple(&name),
    };
    db.with(|c| {
        c.execute(
            "INSERT INTO dictation_topics (name, slug, description, level, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                name,
                slug,
                body["description"].as_str(),
                body["level"].as_str(),
                now_s()
            ],
        )
    })
    .map_err(|e| anyhow!("Không tạo được chủ đề (slug '{slug}' có thể đã tồn tại): {e}"))?;
    list_topics(db)
}

pub fn update_topic(db: &Db, id: i64, body: &Value) -> Result<Value> {
    let n = db.with(|c| {
        c.execute(
            "UPDATE dictation_topics SET
               name = COALESCE(?2, name),
               description = COALESCE(?3, description),
               level = COALESCE(?4, level)
             WHERE id = ?1",
            params![
                id,
                body["name"].as_str(),
                body["description"].as_str(),
                body["level"].as_str()
            ],
        )
    })?;
    if n == 0 {
        return Err(anyhow!("Không tìm thấy chủ đề"));
    }
    list_topics(db)
}

pub fn delete_topic(db: &Db, id: i64) -> Result<Value> {
    let n = db.with(|c| c.execute("DELETE FROM dictation_topics WHERE id = ?1", params![id]))?;
    if n == 0 {
        return Err(anyhow!("Không tìm thấy chủ đề"));
    }
    Ok(json!({ "success": true }))
}

/// Write a lesson's segments, replacing whatever was there.
fn replace_segments(db: &Db, lesson_id: i64, segments: &[Value]) -> Result<()> {
    db.with(|c| {
        c.execute(
            "DELETE FROM dictation_segments WHERE lesson_id = ?1",
            params![lesson_id],
        )
    })?;
    for (i, s) in segments.iter().enumerate() {
        db.with(|c| {
            c.execute(
                "INSERT INTO dictation_segments (lesson_id, content, solutions, start_time, end_time, order_index)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    lesson_id,
                    s["content"].as_str(),
                    s.get("solutions").filter(|v| v.is_array()).map(|v| v.to_string()).unwrap_or_else(|| "[]".into()),
                    s["startTime"].as_f64().unwrap_or(0.0),
                    s["endTime"].as_f64().unwrap_or(0.0),
                    s["orderIndex"].as_i64().unwrap_or(i as i64),
                ],
            )
        })?;
    }
    Ok(())
}

fn topic_id_for(db: &Db, body: &Value) -> Result<Option<i64>> {
    if let Some(id) = body["topicId"].as_i64() {
        return Ok(Some(id));
    }
    let slug = body["topicSlug"].as_str().unwrap_or("");
    if slug.is_empty() {
        return Ok(None);
    }
    db.with(|c| {
        c.query_row(
            "SELECT id FROM dictation_topics WHERE slug = ?1",
            params![slug],
            |r| r.get(0),
        )
        .optional()
    })
    .map_err(Into::into)
}

pub fn create_lesson(db: &Db, body: &Value) -> Result<Value> {
    let title = body["title"].as_str().unwrap_or("").trim().to_string();
    if title.is_empty() {
        return Err(anyhow!("Thiếu tiêu đề bài"));
    }
    let topic_id = topic_id_for(db, body)?;
    let topic_label = body["topic"]
        .as_str()
        .or(body["topicSlug"].as_str())
        .unwrap_or("")
        .to_string();
    db.with(|c| {
        c.execute(
            "INSERT INTO dictation_lessons (title, topic, description, level, audio_url, youtube_video_id, mode, topic_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                title,
                topic_label,
                body["description"].as_str(),
                body["level"].as_str(),
                body["audioUrl"].as_str(),
                body["youtubeVideoId"].as_str(),
                body["mode"].as_str().unwrap_or("dictation"),
                topic_id,
                now_s()
            ],
        )
    })?;
    let id: i64 = db.with(|c| c.query_row("SELECT last_insert_rowid()", [], |r| r.get(0)))?;
    if let Some(segments) = body["segments"].as_array() {
        replace_segments(db, id, segments)?;
    }
    get_lesson(db, id)
}

pub fn update_lesson(db: &Db, id: i64, body: &Value) -> Result<Value> {
    lesson_summary(db, id)?;
    let topic_id = topic_id_for(db, body)?;
    db.with(|c| {
        c.execute(
            "UPDATE dictation_lessons SET
               title = COALESCE(?2, title),
               topic = COALESCE(?3, topic),
               description = COALESCE(?4, description),
               level = COALESCE(?5, level),
               audio_url = COALESCE(?6, audio_url),
               youtube_video_id = COALESCE(?7, youtube_video_id),
               mode = COALESCE(?8, mode),
               topic_id = COALESCE(?9, topic_id)
             WHERE id = ?1",
            params![
                id,
                body["title"].as_str(),
                body["topic"].as_str(),
                body["description"].as_str(),
                body["level"].as_str(),
                body["audioUrl"].as_str(),
                body["youtubeVideoId"].as_str(),
                body["mode"].as_str(),
                topic_id,
            ],
        )
    })?;
    if let Some(segments) = body["segments"].as_array() {
        replace_segments(db, id, segments)?;
    }
    get_lesson(db, id)
}

pub fn delete_lesson(db: &Db, id: i64) -> Result<Value> {
    let n = db.with(|c| c.execute("DELETE FROM dictation_lessons WHERE id = ?1", params![id]))?;
    if n == 0 {
        return Err(anyhow!("Không tìm thấy bài dictation"));
    }
    Ok(json!({ "success": true }))
}

/// GET /dictation-lessons/export — the exact payload `import_json` accepts.
pub fn export_all(db: &Db) -> Result<Value> {
    let topics: Vec<Value> = db.with(|c| {
        let mut stmt =
            c.prepare("SELECT name, slug, description, level FROM dictation_topics ORDER BY name")?;
        let rows = stmt.query_map([], |r| {
            Ok(json!({
                "name": r.get::<_, String>(0)?,
                "slug": r.get::<_, String>(1)?,
                "description": r.get::<_, Option<String>>(2)?,
                "level": r.get::<_, Option<String>>(3)?,
            }))
        })?;
        rows.collect()
    })?;

    let ids: Vec<i64> = db.with(|c| {
        let mut stmt = c.prepare("SELECT id FROM dictation_lessons ORDER BY id")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect()
    })?;

    let mut lessons = Vec::with_capacity(ids.len());
    for id in ids {
        let mut l = get_lesson(db, id)?;
        let topic_slug = l["dictationTopic"]["slug"].clone();
        let o = l.as_object_mut().unwrap();
        o.remove("id");
        o.remove("createdAt");
        o.remove("topicId");
        o.remove("dictationTopic");
        o.insert("topicSlug".into(), topic_slug);
        // Round-trip name: import reads `challenges`, get_lesson emits the long form.
        if let Some(ch) = o.remove("pronunciationChallenges") {
            o.insert("challenges".into(), ch);
        }
        lessons.push(l);
    }

    Ok(json!({
        "version": 1,
        "kind": "kaen-dictation",
        "exportedAt": now_s(),
        "topics": topics,
        "lessons": lessons,
    }))
}

// ---- import ----

/// Import topics + lessons from crawler JSON:
/// `{ topics: [{name, slug, description?, level?}],
///    lessons: [{title, topicSlug?, topic?, level?, audioUrl?, youtubeVideoId?, mode?,
///               segments: [{content, solutions?, startTime, endTime}],
///               challenges?: [{options, voices}] }] }`
pub fn import_json(db: &Db, payload: &Value) -> Result<Value> {
    let mut topics_created = 0usize;
    let mut lessons_created = 0usize;
    let mut lessons_updated = 0usize;

    if let Some(topics) = payload["topics"].as_array() {
        for t in topics {
            let name = t["name"].as_str().unwrap_or("").trim();
            let slug = t["slug"].as_str().unwrap_or("").trim();
            if name.is_empty() || slug.is_empty() {
                continue;
            }
            let n = db.with(|c| {
                c.execute(
                    "INSERT OR IGNORE INTO dictation_topics (name, slug, description, level, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![name, slug, t["description"].as_str(), t["level"].as_str(), now_s()],
                )
            })?;
            topics_created += n;
        }
    }

    if let Some(lessons) = payload["lessons"].as_array() {
        for l in lessons {
            let title = l["title"].as_str().unwrap_or("").trim();
            if title.is_empty() {
                continue;
            }
            let topic_slug = l["topicSlug"].as_str().unwrap_or("");
            let topic_id: Option<i64> = db.with(|c| {
                c.query_row(
                    "SELECT id FROM dictation_topics WHERE slug = ?1",
                    params![topic_slug],
                    |r| r.get(0),
                )
                .optional()
            })?;
            // Upsert by (title, topic): re-importing a backup must update the
            // lesson in place, not stack a second copy next to it.
            let existing: Option<i64> = db.with(|c| {
                c.query_row(
                    "SELECT id FROM dictation_lessons
                     WHERE title = ?1 AND (topic_id IS ?2 OR (topic_id IS NULL AND ?2 IS NULL))",
                    params![title, topic_id],
                    |r| r.get(0),
                )
                .optional()
            })?;

            let lesson_id = match existing {
                Some(id) => {
                    db.with(|c| {
                        c.execute(
                            "UPDATE dictation_lessons SET topic = ?2, description = ?3, level = ?4,
                               audio_url = ?5, youtube_video_id = ?6, mode = ?7 WHERE id = ?1",
                            params![
                                id,
                                l["topic"].as_str().unwrap_or(topic_slug),
                                l["description"].as_str(),
                                l["level"].as_str(),
                                l["audioUrl"].as_str(),
                                l["youtubeVideoId"].as_str(),
                                l["mode"].as_str().unwrap_or("dictation"),
                            ],
                        )
                    })?;
                    lessons_updated += 1;
                    id
                }
                None => {
                    db.with(|c| {
                        c.execute(
                            "INSERT INTO dictation_lessons (title, topic, description, level, audio_url, youtube_video_id, mode, topic_id, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                            params![
                                title,
                                l["topic"].as_str().unwrap_or(topic_slug),
                                l["description"].as_str(),
                                l["level"].as_str(),
                                l["audioUrl"].as_str(),
                                l["youtubeVideoId"].as_str(),
                                l["mode"].as_str().unwrap_or("dictation"),
                                topic_id,
                                now_s()
                            ],
                        )
                    })?;
                    lessons_created += 1;
                    db.with(|c| c.query_row("SELECT last_insert_rowid()", [], |r| r.get(0)))?
                }
            };

            if let Some(segments) = l["segments"].as_array() {
                replace_segments(db, lesson_id, segments)?;
            }
            if let Some(challenges) = l["challenges"].as_array() {
                db.with(|c| {
                    c.execute(
                        "DELETE FROM dictation_challenges WHERE lesson_id = ?1",
                        params![lesson_id],
                    )
                })?;
                for ch in challenges {
                    db.with(|c| {
                        c.execute(
                            "INSERT INTO dictation_challenges (lesson_id, options, voices) VALUES (?1, ?2, ?3)",
                            params![
                                lesson_id,
                                ch.get("options").map(|v| v.to_string()),
                                ch.get("voices").map(|v| v.to_string())
                            ],
                        )
                    })?;
                }
            }
        }
    }

    Ok(json!({
        "topicsCreated": topics_created,
        "lessonsCreated": lessons_created,
        "lessonsUpdated": lessons_updated,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> Db {
        let db = Db::open_memory().unwrap();
        import_json(
            &db,
            &json!({
                "topics": [{ "name": "Short Stories", "slug": "short-stories", "level": "A1-B1" }],
                "lessons": [{
                    "title": "First Snowfall",
                    "topicSlug": "short-stories",
                    "level": "A1",
                    "audioUrl": "https://example.com/a.mp3",
                    "segments": [
                        { "content": "It snowed last night.", "solutions": [["it","snowed","last","night"]], "startTime": 0.0, "endTime": 3.2 },
                        { "content": "The kids were happy.", "startTime": 3.2, "endTime": 6.0 }
                    ]
                }]
            }),
        )
        .unwrap();
        db
    }

    #[test]
    fn import_then_list_and_read_back() {
        let db = seeded();
        let topics = list_topics(&db).unwrap();
        assert_eq!(topics[0]["slug"], "short-stories");
        assert_eq!(topics[0]["lessonCount"], 1);

        let page = list_lessons(&db, Some("short-stories"), None, 20, 1).unwrap();
        assert_eq!(page["total"], 1);
        assert!(page["data"][0]["userProgress"].is_null());

        let lesson = get_lesson(&db, page["data"][0]["id"].as_i64().unwrap()).unwrap();
        assert_eq!(lesson["segments"].as_array().unwrap().len(), 2);
        assert_eq!(lesson["segments"][0]["solutions"][0][1], "snowed");
        assert_eq!(lesson["dictationTopic"]["slug"], "short-stories");

        // Re-import same topic slug is idempotent for topics.
        import_json(
            &db,
            &json!({ "topics": [{ "name": "Short Stories", "slug": "short-stories" }] }),
        )
        .unwrap();
        assert_eq!(list_topics(&db).unwrap().as_array().unwrap().len(), 1);
    }

    #[test]
    fn export_import_round_trips_and_reimport_updates_in_place() {
        let src = seeded();
        let dump = export_all(&src).unwrap();
        assert_eq!(dump["kind"], "kaen-dictation");
        assert_eq!(dump["topics"].as_array().unwrap().len(), 1);
        assert_eq!(dump["lessons"].as_array().unwrap().len(), 1);
        assert_eq!(dump["lessons"][0]["topicSlug"], "short-stories");
        assert_eq!(dump["lessons"][0]["segments"].as_array().unwrap().len(), 2);
        assert!(
            dump["lessons"][0].get("id").is_none(),
            "ids are per-install"
        );

        // Restore into an empty install.
        let dst = Db::open_memory().unwrap();
        let out = import_json(&dst, &dump).unwrap();
        assert_eq!(out["lessonsCreated"], 1);
        let page = list_lessons(&dst, Some("short-stories"), None, 20, 1).unwrap();
        assert_eq!(page["total"], 1);
        let lesson = get_lesson(&dst, page["data"][0]["id"].as_i64().unwrap()).unwrap();
        assert_eq!(lesson["segments"].as_array().unwrap().len(), 2);
        assert_eq!(lesson["audioUrl"], "https://example.com/a.mp3");

        // Re-import must not double the library (kaizen's importer appended).
        let again = import_json(&dst, &dump).unwrap();
        assert_eq!(again["lessonsCreated"], 0);
        assert_eq!(again["lessonsUpdated"], 1);
        assert_eq!(list_lessons(&dst, None, None, 20, 1).unwrap()["total"], 1);
        let lesson = get_lesson(&dst, page["data"][0]["id"].as_i64().unwrap()).unwrap();
        assert_eq!(
            lesson["segments"].as_array().unwrap().len(),
            2,
            "segments replaced, not appended"
        );
    }

    #[test]
    fn admin_crud_covers_topics_lessons_and_segments() {
        let db = Db::open_memory().unwrap();

        let topics = create_topic(&db, &json!({ "name": "TOEIC Part 1", "level": "B1" })).unwrap();
        assert_eq!(
            topics[0]["slug"], "toeic-part-1",
            "slug derived from the name"
        );
        let topic_id = topics[0]["id"].as_i64().unwrap();

        let lesson = create_lesson(
            &db,
            &json!({
                "title": "Photo description",
                "topicId": topic_id,
                "topic": "toeic-part-1",
                "audioUrl": "https://x/a.mp3",
                "segments": [
                    { "content": "A man is walking.", "startTime": 0, "endTime": 2.5 },
                    { "content": "He carries a bag.", "startTime": 2.5, "endTime": 5 }
                ]
            }),
        )
        .unwrap();
        let lesson_id = lesson["id"].as_i64().unwrap();
        assert_eq!(lesson["segments"].as_array().unwrap().len(), 2);
        assert_eq!(lesson["dictationTopic"]["slug"], "toeic-part-1");

        // Editing replaces the segment list wholesale.
        let edited = update_lesson(
            &db,
            lesson_id,
            &json!({
                "title": "Photo description (v2)",
                "segments": [{ "content": "Only one now.", "startTime": 0, "endTime": 3 }]
            }),
        )
        .unwrap();
        assert_eq!(edited["title"], "Photo description (v2)");
        assert_eq!(edited["segments"].as_array().unwrap().len(), 1);
        assert_eq!(
            edited["audioUrl"], "https://x/a.mp3",
            "unspecified fields survive"
        );

        update_topic(&db, topic_id, &json!({ "name": "TOEIC Nghe ảnh" })).unwrap();
        assert_eq!(list_topics(&db).unwrap()[0]["name"], "TOEIC Nghe ảnh");

        delete_lesson(&db, lesson_id).unwrap();
        assert_eq!(list_lessons(&db, None, None, 20, 1).unwrap()["total"], 0);
        delete_topic(&db, topic_id).unwrap();
        assert_eq!(list_topics(&db).unwrap().as_array().unwrap().len(), 0);
        assert!(
            delete_topic(&db, topic_id).is_err(),
            "deleting twice is an error"
        );
    }

    #[test]
    fn progress_percentage_counts_learned_segments() {
        let db = seeded();
        let id = list_lessons(&db, None, None, 20, 1).unwrap()["data"][0]["id"]
            .as_i64()
            .unwrap();
        let p = save_progress(&db, id, 1, &json!({ "0": "learned", "1": "marked" })).unwrap();
        assert_eq!(p["completionPercentage"], 50, "1 of 2 segments learned");
        assert_eq!(p["currentIndex"], 1);

        // The listing surfaces percentage + marked flag; history lists the lesson.
        let page = list_lessons(&db, None, None, 20, 1).unwrap();
        assert_eq!(page["data"][0]["userProgress"]["percentage"], 50);
        assert_eq!(page["data"][0]["userProgress"]["hasMark"], true);
        let h = history(&db).unwrap();
        assert_eq!(h.as_array().unwrap().len(), 1);
        assert_eq!(h[0]["completionPercentage"], 50);
    }
}
