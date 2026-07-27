//! Grammar lessons + AI-generated grammar tests — port of kaizen's `grammar`
//! and `grammar-test` modules (single-user: no visibility, no admin CMS).
//!
//! Response shapes stay camelCase-compatible with the kaizen frontend. AI
//! question generation goes through the daemon bridge (`llm.rs`) instead of
//! Dify.

use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use rusqlite::{params, OptionalExtension, Row};
use serde_json::{json, Value};

use crate::db::Db;
use crate::srs;

/// Days until the review reminder after submitting a linked test.
const REMINDER_DAYS: i64 = 7;

pub const LEVELS: [&str; 7] = ["A1", "A2", "B1", "B1-B2", "B2", "C1", "OTHER"];

// ---- helpers ----

/// ASCII slug (kaizen's `slugifyText`): lowercase, Vietnamese diacritics
/// folded, everything else collapsed to `-`.
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut prev_dash = true; // suppress leading dashes
    for c in text.to_lowercase().chars() {
        let folded = fold_char(c);
        for f in folded.chars() {
            if f.is_ascii_alphanumeric() {
                out.push(f);
                prev_dash = false;
            } else if !prev_dash {
                out.push('-');
                prev_dash = true;
            }
        }
        if folded.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Fold one Vietnamese/latin char to its ASCII base ("" = non-alphanumeric).
fn fold_char(c: char) -> String {
    const TABLE: &[(&str, char)] = &[
        ("aàáảãạăằắẳẵặâầấẩẫậ", 'a'),
        ("eèéẻẽẹêềếểễệ", 'e'),
        ("iìíỉĩị", 'i'),
        ("oòóỏõọôồốổỗộơờớởỡợ", 'o'),
        ("uùúủũụưừứửữự", 'u'),
        ("yỳýỷỹỵ", 'y'),
        ("dđ", 'd'),
    ];
    for (set, base) in TABLE {
        if set.contains(c) {
            return base.to_string();
        }
    }
    if c.is_ascii_alphanumeric() {
        c.to_string()
    } else {
        // Marker for "not alphanumeric" — slugify turns it into a dash.
        String::new()
    }
}

fn strip_level_prefix(slug: &str) -> String {
    for lv in ["a1-", "a2-", "b1-b2-", "b1-", "b2-", "c1-", "other-"] {
        if let Some(rest) = slug.strip_prefix(lv) {
            return rest.to_string();
        }
    }
    slug.to_string()
}

fn is_uuid(s: &str) -> bool {
    s.len() == 36 && s.chars().filter(|c| *c == '-').count() == 4
}

fn now_s() -> String {
    srs::fmt(Utc::now())
}

// ---- row mappers ----

fn grammar_row(row: &Row, with_content: bool) -> rusqlite::Result<Value> {
    let mut v = json!({
        "id": row.get::<_, String>("id")?,
        "title": row.get::<_, String>("title")?,
        "description": row.get::<_, Option<String>>("description")?,
        "level": row.get::<_, String>("level")?,
        "thumbnailUrl": row.get::<_, Option<String>>("thumbnail_url")?,
        "viewCount": row.get::<_, i64>("view_count")?,
        "index": row.get::<_, i64>("idx")?,
        "slug": row.get::<_, String>("slug")?,
        "createdAt": row.get::<_, String>("created_at")?,
        "updatedAt": row.get::<_, String>("updated_at")?,
        "visibility": "PUBLIC",
    });
    if with_content {
        v.as_object_mut()
            .unwrap()
            .insert("content".into(), json!(row.get::<_, String>("content")?));
    }
    Ok(v)
}

fn question_row(row: &Row, with_answer: bool) -> rusqlite::Result<Value> {
    let options: String = row.get("options")?;
    let mut v = json!({
        "id": row.get::<_, String>("id")?,
        "topicId": row.get::<_, String>("topic_id")?,
        "content": row.get::<_, String>("content")?,
        "options": serde_json::from_str::<Value>(&options).unwrap_or(json!([])),
        "source": row.get::<_, String>("source")?,
        "createdAt": row.get::<_, String>("created_at")?,
    });
    if with_answer {
        let o = v.as_object_mut().unwrap();
        o.insert(
            "correctAnswerId".into(),
            json!(row.get::<_, String>("correct_answer_id")?),
        );
        o.insert(
            "explanation".into(),
            json!(row.get::<_, Option<String>>("explanation")?),
        );
    }
    Ok(v)
}

fn study_progress_json(db: &Db, grammar_id: &str) -> Result<Value> {
    let row = db.with(|c| {
        c.query_row(
            "SELECT first_passed_at, last_test_at, next_reminder_at FROM grammar_progress WHERE grammar_id = ?1",
            params![grammar_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
    })?;
    Ok(match row {
        Some((first, last, next)) => {
            let due = next
                .as_deref()
                .and_then(srs::parse)
                .is_some_and(|t| t <= Utc::now());
            json!({
                "firstPassedAt": first,
                "lastTestAt": last,
                "nextReminderAt": next,
                "dueForReview": due,
            })
        }
        None => Value::Null,
    })
}

// ---- grammar CRUD / listing ----

pub fn create_grammar(
    db: &Db,
    title: &str,
    content: &str,
    description: Option<&str>,
    level: &str,
    index: i64,
) -> Result<Value> {
    let title = title.trim();
    if title.is_empty() {
        return Err(anyhow!("Thiếu tiêu đề bài ngữ pháp"));
    }
    let level = if LEVELS.contains(&level) { level } else { "B1" };
    let id = uuid::Uuid::new_v4().to_string();
    // kaizen appended Date.now() for uniqueness; a short uuid tail does the same.
    let slug = format!("{}-{}", slugify(title), &id[..8]);
    let now = now_s();
    db.with(|c| {
        c.execute(
            "INSERT INTO grammars (id, title, content, description, level, idx, slug, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![id, title, content, description, level, index, slug, now],
        )
    })?;
    get_grammar(db, &id, false)
}

pub fn update_grammar(db: &Db, id_or_slug: &str, body: &Value) -> Result<Value> {
    let g = get_grammar(db, id_or_slug, false)?;
    let id = g["id"].as_str().unwrap().to_string();
    let now = now_s();
    db.with(|c| {
        c.execute(
            "UPDATE grammars SET
               title = COALESCE(?2, title),
               content = COALESCE(?3, content),
               description = COALESCE(?4, description),
               level = COALESCE(?5, level),
               idx = COALESCE(?6, idx),
               updated_at = ?7
             WHERE id = ?1",
            params![
                id,
                body["title"].as_str(),
                body["content"].as_str(),
                body["description"].as_str(),
                body["level"].as_str().filter(|l| LEVELS.contains(l)),
                body["index"].as_i64(),
                now,
            ],
        )
    })?;
    get_grammar(db, &id, false)
}

pub fn delete_grammar(db: &Db, id_or_slug: &str) -> Result<Value> {
    let g = get_grammar(db, id_or_slug, false)?;
    let id = g["id"].as_str().unwrap();
    db.with(|c| c.execute("DELETE FROM grammars WHERE id = ?1", params![id]))?;
    Ok(json!({ "success": true }))
}

fn get_grammar(db: &Db, id_or_slug: &str, with_content: bool) -> Result<Value> {
    let where_clause = if is_uuid(id_or_slug) {
        "id = ?1"
    } else {
        "LOWER(slug) = LOWER(?1)"
    };
    db.with(|c| {
        c.query_row(
            &format!("SELECT * FROM grammars WHERE {where_clause}"),
            params![id_or_slug],
            |r| grammar_row(r, with_content),
        )
        .optional()
    })?
    .ok_or_else(|| anyhow!("Không tìm thấy bài ngữ pháp"))
}

/// GET /grammar/public (and /grammar): kaizen's paginated listing with
/// level/search/study filters and level→index→createdAt ordering.
pub fn list_grammars(
    db: &Db,
    page: i64,
    limit: i64,
    level: Option<&str>,
    search: Option<&str>,
    study: Option<&str>,
) -> Result<Value> {
    let page = page.max(1);
    let limit = limit.clamp(1, 100);

    let rows: Vec<Value> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT * FROM grammars ORDER BY level ASC, idx ASC, created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| grammar_row(r, false))?;
        rows.collect()
    })?;

    let q = search.map(str::to_lowercase).filter(|s| !s.is_empty());
    let mut filtered: Vec<Value> = Vec::new();
    for g in rows {
        if let Some(lv) = level {
            if g["level"].as_str() != Some(lv) {
                continue;
            }
        }
        if let Some(q) = &q {
            let title = g["title"].as_str().unwrap_or("").to_lowercase();
            let desc = g["description"].as_str().unwrap_or("").to_lowercase();
            if !title.contains(q) && !desc.contains(q) {
                continue;
            }
        }
        let progress = study_progress_json(db, g["id"].as_str().unwrap())?;
        match study {
            Some("completed") if progress.is_null() => continue,
            Some("pending") if !progress.is_null() => continue,
            _ => {}
        }
        let mut g = g;
        g.as_object_mut().unwrap().insert("studyProgress".into(), progress);
        filtered.push(g);
    }

    let total = filtered.len() as i64;
    let total_pages = if total == 0 { 0 } else { (total + limit - 1) / limit };
    let start = ((page - 1) * limit).max(0) as usize;
    let items: Vec<Value> = filtered.into_iter().skip(start).take(limit as usize).collect();
    Ok(json!({
        "items": items,
        "total": total,
        "page": page,
        "limit": limit,
        "totalPages": total_pages,
    }))
}

/// GET /grammar/:idOrSlug — full content + prev/next in the same level +
/// studyProgress; bumps viewCount.
pub fn view_grammar(db: &Db, id_or_slug: &str) -> Result<Value> {
    let mut g = get_grammar(db, id_or_slug, true)?;
    let id = g["id"].as_str().unwrap().to_string();
    let level = g["level"].as_str().unwrap_or("").to_string();
    let idx = g["index"].as_i64().unwrap_or(0);
    let created = g["createdAt"].as_str().unwrap_or("").to_string();

    db.with(|c| {
        c.execute(
            "UPDATE grammars SET view_count = view_count + 1 WHERE id = ?1",
            params![id],
        )
    })?;

    let prev: Option<String> = db.with(|c| {
        c.query_row(
            "SELECT slug FROM grammars WHERE level = ?1 AND (idx < ?2 OR (idx = ?2 AND created_at > ?3))
             ORDER BY idx DESC, created_at ASC LIMIT 1",
            params![level, idx, created],
            |r| r.get(0),
        )
        .optional()
    })?;
    let next: Option<String> = db.with(|c| {
        c.query_row(
            "SELECT slug FROM grammars WHERE level = ?1 AND (idx > ?2 OR (idx = ?2 AND created_at < ?3))
             ORDER BY idx ASC, created_at DESC LIMIT 1",
            params![level, idx, created],
            |r| r.get(0),
        )
        .optional()
    })?;

    let progress = study_progress_json(db, &id)?;
    let o = g.as_object_mut().unwrap();
    o.insert("prevSlug".into(), json!(prev));
    o.insert("nextSlug".into(), json!(next));
    o.insert("studyProgress".into(), progress);
    // Reflect the bump without a second read.
    if let Some(vc) = o.get("viewCount").and_then(Value::as_i64) {
        o.insert("viewCount".into(), json!(vc + 1));
    }
    Ok(g)
}

// ---- backup: export / import ----

/// GET /grammar/export — every lesson plus its linked test topic and questions,
/// in the exact shape `import_bulk` accepts, so a backup round-trips.
pub fn export_all(db: &Db) -> Result<Value> {
    let rows: Vec<Value> = db.with(|c| {
        let mut stmt = c.prepare("SELECT * FROM grammars ORDER BY level ASC, idx ASC")?;
        let rows = stmt.query_map([], |r| grammar_row(r, true))?;
        rows.collect()
    })?;

    let mut grammars = Vec::with_capacity(rows.len());
    for mut g in rows {
        let id = g["id"].as_str().unwrap_or("").to_string();
        let topic: Option<(String, String, Option<String>)> = db.with(|c| {
            c.query_row(
                "SELECT id, name, level FROM grammar_topics WHERE grammar_id = ?1",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
        })?;

        let topic_json = match topic {
            Some((topic_id, name, level)) => {
                let questions: Vec<Value> = db.with(|c| {
                    let mut stmt = c.prepare(
                        "SELECT content, options, correct_answer_id, explanation, source
                         FROM grammar_questions WHERE topic_id = ?1 ORDER BY created_at ASC",
                    )?;
                    let rows = stmt.query_map(params![topic_id], |r| {
                        let options: String = r.get(1)?;
                        Ok(json!({
                            "content": r.get::<_, String>(0)?,
                            "options": serde_json::from_str::<Value>(&options).unwrap_or(json!([])),
                            "correctAnswerId": r.get::<_, String>(2)?,
                            "explanation": r.get::<_, Option<String>>(3)?,
                            "source": r.get::<_, String>(4)?,
                        }))
                    })?;
                    rows.collect()
                })?;
                json!({ "name": name, "level": level, "questions": questions })
            }
            None => Value::Null,
        };

        let o = g.as_object_mut().unwrap();
        o.remove("id"); // ids are per-install; slug is the stable key
        o.remove("viewCount");
        o.remove("createdAt");
        o.remove("updatedAt");
        o.remove("visibility");
        o.insert("topic".into(), topic_json);
        grammars.push(g);
    }

    Ok(json!({
        "version": 1,
        "kind": "kaen-grammar",
        "exportedAt": now_s(),
        "grammars": grammars,
    }))
}

/// POST /grammar/import — accepts the export envelope or a bare array.
/// Upserts by slug so re-importing an edited backup updates in place.
pub fn import_bulk(db: &Db, payload: &Value) -> Result<Value> {
    let items = payload
        .get("grammars")
        .and_then(Value::as_array)
        .or_else(|| payload.as_array())
        .ok_or_else(|| anyhow!("Thiếu danh sách 'grammars' trong file import"))?;

    let (mut created, mut updated, mut questions_added, mut skipped) = (0, 0, 0, 0);

    for item in items {
        let title = item["title"].as_str().unwrap_or("").trim();
        if title.is_empty() {
            skipped += 1;
            continue;
        }
        let content = item["content"].as_str().unwrap_or("");
        let description = item["description"].as_str();
        let level = item["level"].as_str().filter(|l| LEVELS.contains(l)).unwrap_or("B1");
        let index = item["index"].as_i64().unwrap_or(0);
        let slug = item["slug"].as_str().map(str::trim).filter(|s| !s.is_empty());

        let existing: Option<String> = match slug {
            Some(s) => db.with(|c| {
                c.query_row(
                    "SELECT id FROM grammars WHERE LOWER(slug) = LOWER(?1)",
                    params![s],
                    |r| r.get::<_, String>(0),
                )
                .optional()
            })?,
            None => None,
        };

        let grammar_id = match existing {
            Some(id) => {
                db.with(|c| {
                    c.execute(
                        "UPDATE grammars SET title = ?2, content = ?3, description = ?4,
                           level = ?5, idx = ?6, updated_at = ?7 WHERE id = ?1",
                        params![id, title, content, description, level, index, now_s()],
                    )
                })?;
                updated += 1;
                id
            }
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                // Keep the incoming slug when it is free; otherwise derive a fresh one.
                let slug = slug
                    .map(String::from)
                    .unwrap_or_else(|| format!("{}-{}", slugify(title), &id[..8]));
                db.with(|c| {
                    c.execute(
                        "INSERT INTO grammars (id, title, content, description, level, idx, slug, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                        params![id, title, content, description, level, index, slug, now_s()],
                    )
                })?;
                created += 1;
                id
            }
        };

        // Questions travel with the lesson; replace them wholesale so an edited
        // backup is authoritative rather than accumulating duplicates.
        if let Some(topic) = item.get("topic").filter(|t| t.is_object()) {
            let topic_name = topic["name"].as_str().unwrap_or(title).trim().to_string();
            let topic_level = topic["level"].as_str().unwrap_or(level);
            let g = get_grammar(db, &grammar_id, false)?;
            let gslug = g["slug"].as_str().unwrap_or("").to_string();

            let topic_id: String = match db.with(|c| {
                c.query_row(
                    "SELECT id FROM grammar_topics WHERE grammar_id = ?1",
                    params![grammar_id],
                    |r| r.get::<_, String>(0),
                )
                .optional()
            })? {
                Some(id) => {
                    db.with(|c| {
                        c.execute(
                            "UPDATE grammar_topics SET name = ?2, level = ?3, grammar_slug = ?4 WHERE id = ?1",
                            params![id, topic_name, topic_level, gslug],
                        )
                    })?;
                    db.with(|c| {
                        c.execute("DELETE FROM grammar_questions WHERE topic_id = ?1", params![id])
                    })?;
                    id
                }
                None => {
                    let id = uuid::Uuid::new_v4().to_string();
                    db.with(|c| {
                        c.execute(
                            "INSERT INTO grammar_topics (id, name, level, grammar_id, grammar_slug, created_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            params![id, topic_name, topic_level, grammar_id, gslug, now_s()],
                        )
                    })?;
                    id
                }
            };

            for q in topic["questions"].as_array().unwrap_or(&Vec::new()) {
                let (Some(qc), Some(opts), Some(correct)) = (
                    q["content"].as_str(),
                    q["options"].as_array(),
                    q["correctAnswerId"].as_str(),
                ) else {
                    continue;
                };
                db.with(|c| {
                    c.execute(
                        "INSERT INTO grammar_questions (id, topic_id, content, options, correct_answer_id, explanation, source, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        params![
                            uuid::Uuid::new_v4().to_string(),
                            topic_id,
                            qc,
                            json!(opts).to_string(),
                            correct,
                            q["explanation"].as_str().unwrap_or(""),
                            q["source"].as_str().unwrap_or("MANUAL"),
                            now_s(),
                        ],
                    )
                })?;
                questions_added += 1;
            }
        }
    }

    Ok(json!({
        "created": created,
        "updated": updated,
        "questionsImported": questions_added,
        "skipped": skipped,
    }))
}

// ---- topics ----

pub fn list_topics(db: &Db, level: Option<&str>) -> Result<Value> {
    let rows: Vec<Value> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT t.*, (SELECT COUNT(*) FROM grammar_questions q WHERE q.topic_id = t.id) AS question_count
             FROM grammar_topics t ORDER BY t.created_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(json!({
                "id": r.get::<_, String>("id")?,
                "name": r.get::<_, String>("name")?,
                "level": r.get::<_, Option<String>>("level")?,
                "description": r.get::<_, Option<String>>("description")?,
                "grammarId": r.get::<_, Option<String>>("grammar_id")?,
                "grammarSlug": r.get::<_, Option<String>>("grammar_slug")?,
                "questionCount": r.get::<_, i64>("question_count")?,
                "createdAt": r.get::<_, String>("created_at")?,
            }))
        })?;
        rows.collect()
    })?;
    Ok(json!(rows
        .into_iter()
        .filter(|t| level.is_none() || t["level"].as_str() == level)
        .collect::<Vec<_>>()))
}

struct TopicRef {
    id: String,
    name: String,
    level: Option<String>,
    grammar_id: Option<String>,
    grammar_slug: Option<String>,
}

fn all_topics(db: &Db) -> Result<Vec<TopicRef>> {
    db.with(|c| {
        let mut stmt =
            c.prepare("SELECT id, name, level, grammar_id, grammar_slug FROM grammar_topics")?;
        let rows = stmt.query_map([], |r| {
            Ok(TopicRef {
                id: r.get(0)?,
                name: r.get(1)?,
                level: r.get(2)?,
                grammar_id: r.get(3)?,
                grammar_slug: r.get(4)?,
            })
        })?;
        rows.collect()
    })
}

fn question_count(db: &Db, topic_id: &str) -> Result<i64> {
    db.with(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM grammar_questions WHERE topic_id = ?1",
            params![topic_id],
            |r| r.get(0),
        )
    })
}

/// GET /grammar-topics/for-lesson/:grammarSlug — kaizen's ranked matcher:
/// explicitly linked topics win; otherwise fuzzy-match topic name against the
/// lesson title/slug (scores 100/95/92/90/75, min 75), best question count
/// breaks ties. Returns null when nothing matches.
pub fn topic_for_lesson(db: &Db, grammar_slug: &str) -> Result<Value> {
    let g = get_grammar(db, grammar_slug, false)?;
    let gid = g["id"].as_str().unwrap().to_string();
    let gslug = g["slug"].as_str().unwrap_or("").to_string();
    let gtitle = g["title"].as_str().unwrap_or("").to_string();
    let glevel = g["level"].as_str().unwrap_or("").to_string();

    let topics = all_topics(db)?;

    let linked: Vec<&TopicRef> = topics
        .iter()
        .filter(|t| {
            t.grammar_id.as_deref() == Some(gid.as_str())
                || t.grammar_slug
                    .as_deref()
                    .is_some_and(|s| s.trim().eq_ignore_ascii_case(gslug.trim()))
        })
        .collect();

    let pick_best = |cands: Vec<&TopicRef>| -> Result<Option<Value>> {
        let mut best: Option<(&TopicRef, i64)> = None;
        for t in cands {
            let cnt = question_count(db, &t.id)?;
            if best.as_ref().map_or(true, |(_, c)| cnt > *c) {
                best = Some((t, cnt));
            }
        }
        Ok(best.map(|(t, cnt)| {
            json!({
                "topicId": t.id,
                "name": t.name,
                "level": t.level.clone().unwrap_or_else(|| glevel.clone()),
                "questionCount": cnt,
            })
        }))
    };

    if !linked.is_empty() {
        return Ok(pick_best(linked)?.unwrap_or(Value::Null));
    }

    let title_slug = slugify(&gtitle);
    let slug_norm = slugify(&gslug);
    let slug_no_level = slugify(&strip_level_prefix(&gslug));
    let rank = |t: &TopicRef| -> i64 {
        let n = t.name.trim().to_lowercase();
        if n == gtitle.trim().to_lowercase() {
            return 100;
        }
        let ts = slugify(&t.name);
        if ts == title_slug {
            return 95;
        }
        if ts == slug_norm {
            return 92;
        }
        if ts == slug_no_level {
            return 90;
        }
        if slug_no_level.len() >= 10
            && ts.len() >= 10
            && (slug_no_level.contains(&ts) || ts.contains(&slug_no_level))
        {
            return 75;
        }
        0
    };

    // Same level first, then any level (kaizen's two-pass search).
    for pass in 0..2 {
        let cands: Vec<(&TopicRef, i64)> = topics
            .iter()
            .filter(|t| pass == 1 || t.level.as_deref() == Some(glevel.as_str()))
            .map(|t| (t, rank(t)))
            .filter(|(_, s)| *s >= 75)
            .collect();
        if !cands.is_empty() {
            let top_score = cands.iter().map(|(_, s)| *s).max().unwrap();
            let top: Vec<&TopicRef> = cands
                .into_iter()
                .filter(|(_, s)| *s == top_score)
                .map(|(t, _)| t)
                .collect();
            return Ok(pick_best(top)?.unwrap_or(Value::Null));
        }
    }
    Ok(Value::Null)
}

// ---- test flow ----

/// GET /grammar-test/:topicId — up to 10 questions, answers stripped.
pub fn questions_for_topic(db: &Db, topic_id: &str) -> Result<Value> {
    let exists: bool = db.with(|c| {
        c.query_row(
            "SELECT 1 FROM grammar_topics WHERE id = ?1",
            params![topic_id],
            |_| Ok(true),
        )
        .optional()
        .map(|v| v.unwrap_or(false))
    })?;
    if !exists {
        return Err(anyhow!("Không tìm thấy chủ đề ngữ pháp"));
    }
    let rows: Vec<Value> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT * FROM grammar_questions WHERE topic_id = ?1 ORDER BY RANDOM() LIMIT 10",
        )?;
        let rows = stmt.query_map(params![topic_id], |r| question_row(r, false))?;
        rows.collect()
    })?;
    Ok(json!(rows))
}

/// Persist AI-generated questions under a (found-or-created) topic.
/// `raw_questions` items must carry content/options/correctAnswerId.
pub fn save_generated_questions(
    db: &Db,
    topic_name: &str,
    level: &str,
    grammar_id_or_slug: Option<&str>,
    raw_questions: &[Value],
) -> Result<Value> {
    let topic_name = topic_name.trim();
    if topic_name.is_empty() {
        return Err(anyhow!("topic không được để trống"));
    }
    let grammar = match grammar_id_or_slug {
        Some(g) => Some(get_grammar(db, g, false)?),
        None => None,
    };
    let (gid, gslug) = (
        grammar.as_ref().and_then(|g| g["id"].as_str().map(String::from)),
        grammar.as_ref().and_then(|g| g["slug"].as_str().map(String::from)),
    );

    // Find-or-create topic: linked to the grammar first, then by (name, level).
    let existing: Option<String> = db.with(|c| {
        if let Some(gid) = &gid {
            if let Some(id) = c
                .query_row(
                    "SELECT id FROM grammar_topics WHERE grammar_id = ?1
                     OR LOWER(TRIM(grammar_slug)) = LOWER(TRIM(?2))",
                    params![gid, gslug.as_deref().unwrap_or("")],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
            {
                return Ok(Some(id));
            }
        }
        c.query_row(
            "SELECT id FROM grammar_topics WHERE name = ?1 AND level = ?2",
            params![topic_name, level],
            |r| r.get::<_, String>(0),
        )
        .optional()
    })?;

    let topic_id = match existing {
        Some(id) => {
            if gid.is_some() {
                db.with(|c| {
                    c.execute(
                        "UPDATE grammar_topics SET grammar_id = ?2, grammar_slug = ?3 WHERE id = ?1",
                        params![id, gid, gslug],
                    )
                })?;
            }
            id
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            db.with(|c| {
                c.execute(
                    "INSERT INTO grammar_topics (id, name, level, grammar_id, grammar_slug, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, topic_name, level, gid, gslug, now_s()],
                )
            })?;
            id
        }
    };

    let mut saved = Vec::new();
    for item in raw_questions {
        let (Some(content), Some(options), Some(correct)) = (
            item["content"].as_str(),
            item["options"].as_array(),
            item["correctAnswerId"].as_str(),
        ) else {
            continue;
        };
        let qid = uuid::Uuid::new_v4().to_string();
        db.with(|c| {
            c.execute(
                "INSERT INTO grammar_questions (id, topic_id, content, options, correct_answer_id, explanation, source, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'AI', ?7)",
                params![
                    qid,
                    topic_id,
                    content,
                    json!(options).to_string(),
                    correct,
                    item["explanation"].as_str().unwrap_or(""),
                    now_s(),
                ],
            )
        })?;
        saved.push(json!({
            "id": qid,
            "topicId": topic_id,
            "content": content,
            "options": options,
            "source": "AI",
        }));
    }
    if saved.is_empty() {
        return Err(anyhow!(
            "Không lưu được câu nào — object trong JSON thiếu content/options/correctAnswerId."
        ));
    }
    Ok(json!(saved))
}

/// POST /grammar-test/submit.
pub fn submit_test(db: &Db, topic_id: &str, answers: &[Value]) -> Result<Value> {
    let mut score = 0i64;
    let mut results = Vec::new();
    let mut rows: Vec<(String, Option<String>, bool)> = Vec::new();

    for a in answers {
        let qid = a["questionId"].as_str().unwrap_or("");
        let selected = a["selectedAnswerId"].as_str();
        let q: Option<Value> = db.with(|c| {
            c.query_row(
                "SELECT * FROM grammar_questions WHERE id = ?1",
                params![qid],
                |r| question_row(r, true),
            )
            .optional()
        })?;
        let Some(q) = q else { continue };
        let correct_id = q["correctAnswerId"].as_str().unwrap_or("");
        let is_correct = selected == Some(correct_id);
        if is_correct {
            score += 1;
        }
        rows.push((qid.to_string(), selected.map(String::from), is_correct));
        results.push(json!({
            "questionId": qid,
            "content": q["content"],
            "options": q["options"],
            "selectedAnswerId": selected,
            "isCorrect": is_correct,
            "correctAnswerId": correct_id,
            "explanation": q["explanation"],
        }));
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let total = answers.len() as i64;
    db.with(|c| {
        c.execute(
            "INSERT INTO grammar_test_sessions (id, topic_id, score, total_questions, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, topic_id, score, total, now_s()],
        )
    })?;
    for (qid, selected, is_correct) in &rows {
        db.with(|c| {
            c.execute(
                "INSERT INTO grammar_test_results (id, session_id, question_id, selected_answer_id, is_correct, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    session_id,
                    qid,
                    selected,
                    *is_correct as i64,
                    now_s()
                ],
            )
        })?;
    }

    record_progress_after_test(db, topic_id)?;

    Ok(json!({
        "sessionId": session_id,
        "score": score,
        "total": total,
        "results": results,
    }))
}

/// Linked-topic bookkeeping: mark learned + schedule the 7-day reminder.
fn record_progress_after_test(db: &Db, topic_id: &str) -> Result<()> {
    let grammar_id: Option<String> = db.with(|c| {
        c.query_row(
            "SELECT grammar_id FROM grammar_topics WHERE id = ?1",
            params![topic_id],
            |r| r.get(0),
        )
        .optional()
        .map(Option::flatten)
    })?;
    let Some(gid) = grammar_id else { return Ok(()) };
    let now = now_s();
    let next = srs::fmt(Utc::now() + Duration::days(REMINDER_DAYS));
    db.with(|c| {
        c.execute(
            "INSERT INTO grammar_progress (grammar_id, first_passed_at, last_test_at, next_reminder_at, created_at)
             VALUES (?1, ?2, ?2, ?3, ?2)
             ON CONFLICT(grammar_id) DO UPDATE SET
               last_test_at = excluded.last_test_at,
               next_reminder_at = excluded.next_reminder_at,
               first_passed_at = COALESCE(grammar_progress.first_passed_at, excluded.first_passed_at)",
            params![gid, now, next],
        )
    })?;
    Ok(())
}

/// GET /grammar-test/results/:sessionId.
pub fn session_result(db: &Db, session_id: &str) -> Result<Value> {
    let session: Option<(i64, i64)> = db.with(|c| {
        c.query_row(
            "SELECT score, total_questions FROM grammar_test_sessions WHERE id = ?1",
            params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    })?;
    let Some((score, total)) = session else {
        return Err(anyhow!("Không tìm thấy kết quả bài test"));
    };

    let results: Vec<Value> = db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT r.question_id, r.selected_answer_id, r.is_correct,
                    q.content, q.options, q.correct_answer_id, q.explanation
             FROM grammar_test_results r
             LEFT JOIN grammar_questions q ON q.id = r.question_id
             WHERE r.session_id = ?1",
        )?;
        let rows = stmt.query_map(params![session_id], |r| {
            let options: Option<String> = r.get(4)?;
            Ok(json!({
                "questionId": r.get::<_, String>(0)?,
                "selectedAnswerId": r.get::<_, Option<String>>(1)?,
                "isCorrect": r.get::<_, i64>(2)? != 0,
                "content": r.get::<_, Option<String>>(3)?,
                "options": options
                    .and_then(|o| serde_json::from_str::<Value>(&o).ok())
                    .unwrap_or(json!([])),
                "correctAnswerId": r.get::<_, Option<String>>(5)?,
                "explanation": r.get::<_, Option<String>>(6)?,
            }))
        })?;
        rows.collect()
    })?;

    Ok(json!({
        "sessionId": session_id,
        "score": score,
        "total": total,
        "results": results,
    }))
}

/// Grammars whose review reminder is due (for status/MCP).
pub fn due_reminder_count(db: &Db) -> Result<i64> {
    db.with(|c| {
        c.query_row(
            "SELECT COUNT(*) FROM grammar_progress WHERE next_reminder_at IS NOT NULL AND next_reminder_at <= ?1",
            params![now_s()],
            |r| r.get(0),
        )
    })
}

/// The lesson content for grounding AI generation (may be None).
pub fn grammar_content(db: &Db, id_or_slug: &str) -> Result<Option<(String, String, String)>> {
    match get_grammar(db, id_or_slug, true) {
        Ok(g) => Ok(Some((
            g["title"].as_str().unwrap_or("").to_string(),
            g["level"].as_str().unwrap_or("B1").to_string(),
            g["content"].as_str().unwrap_or("").to_string(),
        ))),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_memory().unwrap()
    }

    fn q(content: &str, correct: &str) -> Value {
        json!({
            "content": content,
            "options": [
                {"id": "A", "text": "a"}, {"id": "B", "text": "b"},
                {"id": "C", "text": "c"}, {"id": "D", "text": "d"}
            ],
            "correctAnswerId": correct,
            "explanation": "vì vậy",
        })
    }

    #[test]
    fn slugify_folds_vietnamese() {
        assert_eq!(slugify("Thì Hiện Tại Đơn"), "thi-hien-tai-don");
        assert_eq!(slugify("Present  Simple!"), "present-simple");
        assert_eq!(strip_level_prefix("b1-b2-passive-voice"), "passive-voice");
    }

    #[test]
    fn grammar_crud_and_listing_with_study_filter() {
        let db = db();
        let g = create_grammar(&db, "Thì hiện tại đơn", "# Nội dung", None, "A1", 1).unwrap();
        create_grammar(&db, "Passive voice", "# Bị động", None, "B1", 2).unwrap();

        let all = list_grammars(&db, 1, 10, None, None, None).unwrap();
        assert_eq!(all["total"], 2);
        let a1 = list_grammars(&db, 1, 10, Some("A1"), None, None).unwrap();
        assert_eq!(a1["total"], 1);
        // Search covers title + description (not the markdown content).
        let hit = list_grammars(&db, 1, 10, None, Some("passive"), None).unwrap();
        assert_eq!(hit["total"], 1);

        // Nothing studied yet → "completed" is empty, "pending" has both.
        assert_eq!(list_grammars(&db, 1, 10, None, None, Some("completed")).unwrap()["total"], 0);
        assert_eq!(list_grammars(&db, 1, 10, None, None, Some("pending")).unwrap()["total"], 2);

        // View by slug bumps count and resolves prev/next within the level.
        let slug = g["slug"].as_str().unwrap();
        let viewed = view_grammar(&db, slug).unwrap();
        assert_eq!(viewed["viewCount"], 1);
        assert!(viewed["content"].as_str().unwrap().contains("Nội dung"));
    }

    #[test]
    fn generated_questions_land_in_a_topic_linked_to_the_grammar() {
        let db = db();
        let g = create_grammar(&db, "Present Simple", "content", None, "A1", 0).unwrap();
        let slug = g["slug"].as_str().unwrap();

        let saved = save_generated_questions(
            &db,
            "Present Simple",
            "A1",
            Some(slug),
            &[q("Q1?", "A"), q("Q2?", "B"), json!({"broken": true})],
        )
        .unwrap();
        assert_eq!(saved.as_array().unwrap().len(), 2, "invalid item skipped");

        // for-lesson finds the linked topic immediately.
        let topic = topic_for_lesson(&db, slug).unwrap();
        assert_eq!(topic["questionCount"], 2);
        let topic_id = topic["topicId"].as_str().unwrap().to_string();

        // Re-generating appends to the SAME topic instead of forking a new one.
        save_generated_questions(&db, "Present Simple", "A1", Some(slug), &[q("Q3?", "C")]).unwrap();
        assert_eq!(topic_for_lesson(&db, slug).unwrap()["topicId"], topic_id.as_str());

        // Questions served to the client never leak the answer.
        let served = questions_for_topic(&db, &topic_id).unwrap();
        assert!(served[0].get("correctAnswerId").is_none());
        assert!(served[0].get("explanation").is_none());
    }

    #[test]
    fn fuzzy_topic_match_without_explicit_link() {
        let db = db();
        let g = create_grammar(&db, "Passive Voice", "content", None, "B1", 0).unwrap();
        // Topic created WITHOUT a grammar link, name matches the title.
        save_generated_questions(&db, "Passive Voice", "B1", None, &[q("Q?", "A")]).unwrap();
        let topic = topic_for_lesson(&db, g["slug"].as_str().unwrap()).unwrap();
        assert!(!topic.is_null(), "title-based match should find the topic");
        assert_eq!(topic["questionCount"], 1);
    }

    #[test]
    fn export_import_round_trips_lessons_and_questions() {
        let src = db();
        let g = create_grammar(&src, "Present Simple", "# Nội dung\n\nDùng cho thói quen.", Some("Mô tả"), "A1", 3)
            .unwrap();
        let slug = g["slug"].as_str().unwrap().to_string();
        save_generated_questions(&src, "Present Simple", "A1", Some(&slug), &[q("Q1?", "A"), q("Q2?", "B")])
            .unwrap();

        let dump = export_all(&src).unwrap();
        assert_eq!(dump["kind"], "kaen-grammar");
        assert_eq!(dump["grammars"].as_array().unwrap().len(), 1);
        // Per-install ids must not travel; the slug is the stable key.
        assert!(dump["grammars"][0].get("id").is_none());
        assert_eq!(dump["grammars"][0]["topic"]["questions"].as_array().unwrap().len(), 2);

        // Restore into an empty install.
        let dst = db();
        let out = import_bulk(&dst, &dump).unwrap();
        assert_eq!(out["created"], 1);
        assert_eq!(out["questionsImported"], 2);

        let restored = view_grammar(&dst, &slug).unwrap();
        assert_eq!(restored["title"], "Present Simple");
        assert_eq!(restored["level"], "A1");
        assert_eq!(restored["index"], 3);
        assert!(restored["content"].as_str().unwrap().contains("thói quen"));
        let topic = topic_for_lesson(&dst, &slug).unwrap();
        assert_eq!(topic["questionCount"], 2, "questions came back linked to the lesson");

        // Re-importing the same file updates in place instead of duplicating.
        let again = import_bulk(&dst, &dump).unwrap();
        assert_eq!(again["created"], 0);
        assert_eq!(again["updated"], 1);
        assert_eq!(list_grammars(&dst, 1, 10, None, None, None).unwrap()["total"], 1);
        assert_eq!(topic_for_lesson(&dst, &slug).unwrap()["questionCount"], 2, "questions replaced, not stacked");
    }

    #[test]
    fn import_accepts_a_bare_array_and_skips_untitled_rows() {
        let db = db();
        let out = import_bulk(
            &db,
            &json!([
                { "title": "Articles", "content": "a/an/the", "level": "A1" },
                { "content": "no title here" }
            ]),
        )
        .unwrap();
        assert_eq!(out["created"], 1);
        assert_eq!(out["skipped"], 1);
        // A missing slug is generated so the lesson is still addressable.
        let items = list_grammars(&db, 1, 10, None, None, None).unwrap();
        assert!(items["items"][0]["slug"].as_str().unwrap().starts_with("articles-"));
    }

    #[test]
    fn submit_grades_persists_and_schedules_the_reminder() {
        let db = db();
        let g = create_grammar(&db, "Articles", "content", None, "A2", 0).unwrap();
        let slug = g["slug"].as_str().unwrap();
        save_generated_questions(&db, "Articles", "A2", Some(slug), &[q("Q1?", "A"), q("Q2?", "B")])
            .unwrap();
        let topic = topic_for_lesson(&db, slug).unwrap();
        let topic_id = topic["topicId"].as_str().unwrap();
        let served = questions_for_topic(&db, topic_id).unwrap();
        let ids: Vec<&str> = served
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v["id"].as_str().unwrap())
            .collect();

        // Answer everything "A": one right, one wrong (correct ids A and B).
        let answers: Vec<Value> = ids
            .iter()
            .map(|id| json!({ "questionId": id, "selectedAnswerId": "A" }))
            .collect();
        let out = submit_test(&db, topic_id, &answers).unwrap();
        assert_eq!(out["score"], 1);
        assert_eq!(out["total"], 2);
        assert_eq!(out["results"].as_array().unwrap().len(), 2);
        assert!(out["results"][0]["correctAnswerId"].is_string(), "answers revealed after submit");

        // Progress row exists, reminder ~7 days out, not yet due.
        let progress = study_progress_json(&db, g["id"].as_str().unwrap()).unwrap();
        assert!(!progress.is_null());
        assert_eq!(progress["dueForReview"], false);
        assert_eq!(due_reminder_count(&db).unwrap(), 0);

        // Session result endpoint reconstructs the same review data.
        let sid = out["sessionId"].as_str().unwrap();
        let replay = session_result(&db, sid).unwrap();
        assert_eq!(replay["score"], 1);
        assert_eq!(replay["results"].as_array().unwrap().len(), 2);

        // "completed" study filter now returns the grammar.
        assert_eq!(list_grammars(&db, 1, 10, None, None, Some("completed")).unwrap()["total"], 1);
    }
}
