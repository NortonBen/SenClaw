//! SQLite layer — one serialized connection behind a `Mutex` with WAL, matching
//! the other Space Apps.

use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;

use crate::corpus::{self, Chunk};

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

// ── Row types ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocRow {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub ext: String,
    pub bytes: i64,
    pub chars: i64,
    pub extract_note: Option<String>,
    pub summary: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub added_at: i64,
    pub updated_at: i64,
    pub section_count: i64,
    pub chunk_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionRow {
    pub id: String,
    pub doc_id: String,
    pub ord: i64,
    pub title: String,
    pub level: i64,
    pub char_start: i64,
    pub char_end: i64,
    pub summary: Option<String>,
    pub key_points: Vec<String>,
    pub difficulty: i64,
    pub est_minutes: i64,
    pub prereq: Vec<String>,
    pub enriched_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkRow {
    pub id: i64,
    pub doc_id: String,
    pub section_id: Option<String>,
    pub ord: i64,
    pub char_start: i64,
    pub char_end: i64,
    pub text: String,
}

/// A section as produced by the outliner, before it has an id.
#[derive(Debug, Clone)]
pub struct NewSection {
    pub title: String,
    pub level: i64,
    pub char_start: usize,
    pub char_end: usize,
}

fn json_str_array(raw: Option<String>) -> Vec<String> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.seed_templates()?;
        Ok(db)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.seed_templates()?;
        Ok(db)
    }

    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self.conn.lock().map_err(|_| anyhow!("db mutex poisoned"))?;
        f(&guard)
    }

    pub fn with_conn_mut<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut guard = self.conn.lock().map_err(|_| anyhow!("db mutex poisoned"))?;
        f(&mut guard)
    }

    // ── Settings ────────────────────────────────────────────────────────────

    pub fn setting(&self, k: &str) -> Option<String> {
        self.with_conn(|c| {
            Ok(c.query_row("SELECT v FROM settings WHERE k = ?1", params![k], |r| {
                r.get::<_, String>(0)
            })
            .optional()?)
        })
        .ok()
        .flatten()
    }

    pub fn set_setting(&self, k: &str, v: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO settings (k, v) VALUES (?1, ?2)
                 ON CONFLICT(k) DO UPDATE SET v = excluded.v",
                params![k, v],
            )?;
            Ok(())
        })
    }

    // ── Documents ───────────────────────────────────────────────────────────

    pub fn doc_insert(
        &self,
        title: &str,
        filename: &str,
        ext: &str,
        bytes: i64,
        note: &str,
        body: &str,
    ) -> Result<String> {
        let id = new_id();
        let ts = now_ms();
        let chars = body.chars().count() as i64;
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO docs (id, title, filename, ext, bytes, chars, extract_note, body,
                                   status, added_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'new', ?9, ?9)",
                params![id, title, filename, ext, bytes, chars, note, body, ts],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn doc_list(&self) -> Result<Vec<DocRow>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT d.id, d.title, d.filename, d.ext, d.bytes, d.chars, d.extract_note,
                        d.summary, d.status, d.error, d.added_at, d.updated_at,
                        (SELECT COUNT(*) FROM sections s WHERE s.doc_id = d.id),
                        (SELECT COUNT(*) FROM chunks k WHERE k.doc_id = d.id)
                 FROM docs d ORDER BY d.added_at DESC",
            )?;
            let rows = st
                .query_map([], map_doc)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn doc_get(&self, id: &str) -> Result<Option<DocRow>> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT d.id, d.title, d.filename, d.ext, d.bytes, d.chars, d.extract_note,
                        d.summary, d.status, d.error, d.added_at, d.updated_at,
                        (SELECT COUNT(*) FROM sections s WHERE s.doc_id = d.id),
                        (SELECT COUNT(*) FROM chunks k WHERE k.doc_id = d.id)
                 FROM docs d WHERE d.id = ?1",
                params![id],
                map_doc,
            )
            .optional()?)
        })
    }

    pub fn doc_body(&self, id: &str) -> Result<Option<String>> {
        self.with_conn(|c| {
            Ok(c.query_row("SELECT body FROM docs WHERE id = ?1", params![id], |r| {
                r.get::<_, String>(0)
            })
            .optional()?)
        })
    }

    pub fn doc_set_status(&self, id: &str, status: &str, error: Option<&str>) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE docs SET status = ?2, error = ?3, updated_at = ?4 WHERE id = ?1",
                params![id, status, error, now_ms()],
            )?;
            Ok(())
        })
    }

    pub fn doc_set_summary(&self, id: &str, summary: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE docs SET summary = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, summary, now_ms()],
            )?;
            Ok(())
        })
    }

    pub fn doc_rename(&self, id: &str, title: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE docs SET title = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, title, now_ms()],
            )?;
            Ok(())
        })
    }

    /// Delete a document and everything derived from it, including the FTS
    /// rows — an orphaned FTS row keeps answering searches for text the user
    /// deleted.
    pub fn doc_delete(&self, id: &str) -> Result<()> {
        self.with_conn_mut(|c| {
            let tx = c.transaction()?;
            {
                let mut st = tx.prepare("SELECT id FROM chunks WHERE doc_id = ?1")?;
                let ids: Vec<i64> = st
                    .query_map(params![id], |r| r.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for cid in ids {
                    tx.execute("DELETE FROM chunks_fts WHERE rowid = ?1", params![cid])?;
                }
            }
            tx.execute("DELETE FROM docs WHERE id = ?1", params![id])?;
            tx.commit()?;
            Ok(())
        })
    }

    // ── Sections ────────────────────────────────────────────────────────────

    pub fn sections_replace(&self, doc_id: &str, list: &[NewSection]) -> Result<Vec<String>> {
        self.with_conn_mut(|c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM sections WHERE doc_id = ?1", params![doc_id])?;
            let mut ids = Vec::with_capacity(list.len());
            for (i, s) in list.iter().enumerate() {
                let id = new_id();
                tx.execute(
                    "INSERT INTO sections (id, doc_id, ord, title, level, char_start, char_end)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        id,
                        doc_id,
                        i as i64,
                        s.title,
                        s.level,
                        s.char_start as i64,
                        s.char_end as i64
                    ],
                )?;
                ids.push(id);
            }
            tx.commit()?;
            Ok(ids)
        })
    }

    pub fn sections_of(&self, doc_id: &str) -> Result<Vec<SectionRow>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id, doc_id, ord, title, level, char_start, char_end, summary, key_points,
                        difficulty, est_minutes, prereq, enriched_at
                 FROM sections WHERE doc_id = ?1 ORDER BY ord",
            )?;
            let rows = st
                .query_map(params![doc_id], map_section)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn section_get(&self, id: &str) -> Result<Option<SectionRow>> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT id, doc_id, ord, title, level, char_start, char_end, summary, key_points,
                        difficulty, est_minutes, prereq, enriched_at
                 FROM sections WHERE id = ?1",
                params![id],
                map_section,
            )
            .optional()?)
        })
    }

    /// Sections for a set of documents, document order preserved.
    pub fn sections_for_docs(&self, doc_ids: &[String]) -> Result<Vec<SectionRow>> {
        let mut out = Vec::new();
        for d in doc_ids {
            out.extend(self.sections_of(d)?);
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn section_enrich(
        &self,
        id: &str,
        summary: &str,
        key_points: &[String],
        difficulty: i64,
        est_minutes: i64,
        prereq: &[String],
    ) -> Result<()> {
        let kp = serde_json::to_string(key_points).unwrap_or_else(|_| "[]".into());
        let pr = serde_json::to_string(prereq).unwrap_or_else(|_| "[]".into());
        self.with_conn(|c| {
            c.execute(
                "UPDATE sections SET summary = ?2, key_points = ?3, difficulty = ?4,
                        est_minutes = ?5, prereq = ?6, enriched_at = ?7 WHERE id = ?1",
                params![
                    id,
                    summary,
                    kp,
                    difficulty.clamp(1, 5),
                    est_minutes.clamp(1, 600),
                    pr,
                    now_ms()
                ],
            )?;
            Ok(())
        })
    }

    // ── Chunks + FTS ────────────────────────────────────────────────────────

    /// Replace every chunk of a document, keeping the FTS index in step.
    ///
    /// `section_of` is asked about the chunk itself, not about its start
    /// offset: a chunk begins with an overlap prefix reaching back into the
    /// previous section, so keying off `char_start` labels chunks with the
    /// wrong chapter — and a citation carrying the wrong chapter name is worse
    /// than one carrying none.
    pub fn chunks_replace(
        &self,
        doc_id: &str,
        chunks: &[Chunk],
        section_of: impl Fn(&Chunk) -> Option<String>,
    ) -> Result<usize> {
        self.with_conn_mut(|c| {
            let tx = c.transaction()?;
            {
                let mut st = tx.prepare("SELECT id FROM chunks WHERE doc_id = ?1")?;
                let ids: Vec<i64> = st
                    .query_map(params![doc_id], |r| r.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                for cid in ids {
                    tx.execute("DELETE FROM chunks_fts WHERE rowid = ?1", params![cid])?;
                }
            }
            tx.execute("DELETE FROM chunks WHERE doc_id = ?1", params![doc_id])?;
            for (i, ch) in chunks.iter().enumerate() {
                tx.execute(
                    "INSERT INTO chunks (doc_id, section_id, ord, char_start, char_end, text)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        doc_id,
                        section_of(ch),
                        i as i64,
                        ch.char_start as i64,
                        ch.char_end as i64,
                        ch.text
                    ],
                )?;
                let rowid = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO chunks_fts (rowid, fold) VALUES (?1, ?2)",
                    params![rowid, corpus::fold(&ch.text)],
                )?;
            }
            tx.commit()?;
            Ok(chunks.len())
        })
    }

    pub fn chunk_get(&self, id: i64) -> Result<Option<ChunkRow>> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT id, doc_id, section_id, ord, char_start, char_end, text
                 FROM chunks WHERE id = ?1",
                params![id],
                map_chunk,
            )
            .optional()?)
        })
    }

    pub fn chunks_of_section(&self, section_id: &str) -> Result<Vec<ChunkRow>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id, doc_id, section_id, ord, char_start, char_end, text
                 FROM chunks WHERE section_id = ?1 ORDER BY ord",
            )?;
            let rows = st
                .query_map(params![section_id], map_chunk)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// BM25 search over chunks. `doc_ids` empty = every document.
    ///
    /// Returns `(chunk, score)` with a *higher is better* score — bm25() in
    /// SQLite returns a negative number where more negative is better, which
    /// flips every downstream comparison if passed through raw.
    pub fn search_chunks(
        &self,
        query: &str,
        doc_ids: &[String],
        limit: usize,
    ) -> Result<Vec<(ChunkRow, f64)>> {
        let Some(match_expr) = corpus::fts_query(query) else {
            return Ok(vec![]);
        };
        self.with_conn(|c| {
            let mut sql = String::from(
                "SELECT k.id, k.doc_id, k.section_id, k.ord, k.char_start, k.char_end, k.text,
                        bm25(chunks_fts) AS score
                 FROM chunks_fts f JOIN chunks k ON k.id = f.rowid
                 WHERE chunks_fts MATCH ?1",
            );
            if !doc_ids.is_empty() {
                let placeholders = doc_ids
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", i + 3))
                    .collect::<Vec<_>>()
                    .join(",");
                sql.push_str(&format!(" AND k.doc_id IN ({placeholders})"));
            }
            sql.push_str(" ORDER BY score LIMIT ?2");

            let mut st = c.prepare(&sql)?;
            let mut binds: Vec<&dyn rusqlite::ToSql> = vec![&match_expr, &limit];
            for d in doc_ids {
                binds.push(d);
            }
            let rows = st
                .query_map(binds.as_slice(), |r| {
                    let score: f64 = r.get(7)?;
                    Ok((map_chunk(r)?, -score))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    // ── Concepts ────────────────────────────────────────────────────────────

    pub fn concept_upsert(&self, doc_id: &str, name: &str) -> Result<String> {
        let norm = corpus::fold(name.trim());
        if norm.is_empty() {
            return Err(anyhow!("khái niệm rỗng"));
        }
        self.with_conn(|c| {
            if let Some(id) = c
                .query_row(
                    "SELECT id FROM concepts WHERE doc_id = ?1 AND norm = ?2",
                    params![doc_id, norm],
                    |r| r.get::<_, String>(0),
                )
                .optional()?
            {
                return Ok(id);
            }
            let id = new_id();
            c.execute(
                "INSERT INTO concepts (id, doc_id, name, norm, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, doc_id, name.trim(), norm, now_ms()],
            )?;
            Ok(id)
        })
    }

    pub fn concept_link(&self, concept_id: &str, section_id: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT OR IGNORE INTO concept_sections (concept_id, section_id) VALUES (?1, ?2)",
                params![concept_id, section_id],
            )?;
            Ok(())
        })
    }

    /// Concepts of a document with the sections that teach each one.
    pub fn concept_map(&self, doc_id: &str) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT c.id, c.name,
                        (SELECT GROUP_CONCAT(cs.section_id)
                         FROM concept_sections cs WHERE cs.concept_id = c.id)
                 FROM concepts c WHERE c.doc_id = ?1 ORDER BY c.name",
            )?;
            let rows = st
                .query_map(params![doc_id], |r| {
                    let secs: Option<String> = r.get(2)?;
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "sectionIds": secs
                            .map(|s| s.split(',').map(str::to_string).collect::<Vec<_>>())
                            .unwrap_or_default(),
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn concepts_of_section(&self, section_id: &str) -> Result<Vec<(String, String)>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT c.id, c.name FROM concepts c
                 JOIN concept_sections cs ON cs.concept_id = c.id
                 WHERE cs.section_id = ?1 ORDER BY c.name",
            )?;
            let rows = st
                .query_map(params![section_id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    // ── Templates ───────────────────────────────────────────────────────────

    fn seed_templates(&self) -> Result<()> {
        self.with_conn(|c| {
            for t in crate::planner::BUILTIN_TEMPLATES {
                c.execute(
                    "INSERT INTO plan_templates
                        (key, label, detail, days, min_per_day, review_offsets, blocks,
                         content_ratio, builtin, sort)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)
                     ON CONFLICT(key) DO UPDATE SET
                        label = excluded.label, detail = excluded.detail,
                        days = CASE WHEN plan_templates.builtin = 1 THEN excluded.days ELSE plan_templates.days END,
                        min_per_day = CASE WHEN plan_templates.builtin = 1 THEN excluded.min_per_day ELSE plan_templates.min_per_day END,
                        review_offsets = CASE WHEN plan_templates.builtin = 1 THEN excluded.review_offsets ELSE plan_templates.review_offsets END,
                        blocks = CASE WHEN plan_templates.builtin = 1 THEN excluded.blocks ELSE plan_templates.blocks END,
                        content_ratio = CASE WHEN plan_templates.builtin = 1 THEN excluded.content_ratio ELSE plan_templates.content_ratio END,
                        sort = excluded.sort",
                    params![
                        t.key,
                        t.label,
                        t.detail,
                        t.days,
                        t.min_per_day,
                        serde_json::to_string(t.review_offsets).unwrap_or_default(),
                        serde_json::to_string(t.blocks).unwrap_or_default(),
                        t.content_ratio,
                        t.sort
                    ],
                )?;
            }
            Ok(())
        })
    }

    pub fn templates(&self) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT key, label, detail, days, min_per_day, review_offsets, blocks,
                        content_ratio, builtin
                 FROM plan_templates ORDER BY sort, key",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(serde_json::json!({
                        "key": r.get::<_, String>(0)?,
                        "label": r.get::<_, String>(1)?,
                        "detail": r.get::<_, Option<String>>(2)?,
                        "days": r.get::<_, i64>(3)?,
                        "minPerDay": r.get::<_, i64>(4)?,
                        "reviewOffsets": serde_json::from_str::<Value>(&r.get::<_, String>(5)?)
                            .unwrap_or(Value::Null),
                        "blocks": serde_json::from_str::<Value>(&r.get::<_, String>(6)?)
                            .unwrap_or(Value::Null),
                        "contentRatio": r.get::<_, f64>(7)?,
                        "builtin": r.get::<_, i64>(8)? != 0,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    // ── Plans ───────────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn plan_insert(
        &self,
        title: &str,
        goal: &str,
        doc_ids: &[String],
        template_key: &str,
        start_date: &str,
        days: i64,
        min_per_day: i64,
        weekdays: &str,
        slot_hm: &str,
        tz: &str,
        note: &str,
        preview: &crate::planner::PlanPreview,
    ) -> Result<String> {
        let id = new_id();
        let ts = now_ms();
        let docs = serde_json::to_string(doc_ids).unwrap_or_else(|_| "[]".into());
        self.with_conn_mut(|c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO plans (id, title, goal, doc_ids, template_key, start_date, days,
                                    min_per_day, weekdays, slot_hm, tz, note, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)",
                params![
                    id, title, goal, docs, template_key, start_date, days, min_per_day, weekdays,
                    slot_hm, tz, note, ts
                ],
            )?;
            for s in &preview.sessions {
                let sid = new_id();
                tx.execute(
                    "INSERT INTO sessions (id, plan_id, ord, date, start_hm, minutes, title)
                     VALUES (?1,?2,?3,?4,?5,?6,?7)",
                    params![sid, id, s.ord, s.date, s.start_hm, s.minutes, s.title],
                )?;
                for (i, it) in s.items.iter().enumerate() {
                    tx.execute(
                        "INSERT INTO session_items
                            (id, session_id, ord, kind, section_id, section_title, est_minutes, part, parts)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                        params![
                            new_id(),
                            sid,
                            i as i64,
                            it.kind,
                            it.section_id,
                            it.section_title,
                            it.est_minutes,
                            it.part,
                            it.parts
                        ],
                    )?;
                }
            }
            tx.commit()?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn plan_list(&self) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT p.id, p.title, p.goal, p.doc_ids, p.template_key, p.start_date, p.days,
                        p.min_per_day, p.weekdays, p.slot_hm, p.tz, p.status, p.note, p.created_at,
                        (SELECT COUNT(*) FROM sessions s WHERE s.plan_id = p.id),
                        (SELECT COUNT(*) FROM sessions s WHERE s.plan_id = p.id AND s.status = 'done'),
                        (SELECT COUNT(*) FROM sessions s WHERE s.plan_id = p.id AND s.event_id IS NOT NULL)
                 FROM plans p ORDER BY p.created_at DESC",
            )?;
            let rows = st
                .query_map([], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "title": r.get::<_, String>(1)?,
                        "goal": r.get::<_, Option<String>>(2)?,
                        "docIds": serde_json::from_str::<Value>(&r.get::<_, String>(3)?)
                            .unwrap_or(Value::Null),
                        "templateKey": r.get::<_, Option<String>>(4)?,
                        "startDate": r.get::<_, String>(5)?,
                        "days": r.get::<_, i64>(6)?,
                        "minPerDay": r.get::<_, i64>(7)?,
                        "weekdays": r.get::<_, String>(8)?,
                        "slotHm": r.get::<_, String>(9)?,
                        "tz": r.get::<_, String>(10)?,
                        "status": r.get::<_, String>(11)?,
                        "note": r.get::<_, Option<String>>(12)?,
                        "createdAt": r.get::<_, i64>(13)?,
                        "sessionCount": r.get::<_, i64>(14)?,
                        "doneCount": r.get::<_, i64>(15)?,
                        "syncedCount": r.get::<_, i64>(16)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn plan_get(&self, id: &str) -> Result<Option<Value>> {
        let list = self.plan_list()?;
        Ok(list.into_iter().find(|p| p["id"] == id))
    }

    pub fn plan_delete(&self, id: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM plans WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    pub fn sessions_of_plan(&self, plan_id: &str) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id, ord, date, start_hm, minutes, title, status, event_id, completed_at
                 FROM sessions WHERE plan_id = ?1 ORDER BY ord",
            )?;
            let sessions = st
                .query_map(params![plan_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        serde_json::json!({
                            "id": r.get::<_, String>(0)?,
                            "ord": r.get::<_, i64>(1)?,
                            "date": r.get::<_, String>(2)?,
                            "startHm": r.get::<_, String>(3)?,
                            "minutes": r.get::<_, i64>(4)?,
                            "title": r.get::<_, String>(5)?,
                            "status": r.get::<_, String>(6)?,
                            "eventId": r.get::<_, Option<String>>(7)?,
                            "completedAt": r.get::<_, Option<i64>>(8)?,
                        }),
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let mut out = Vec::with_capacity(sessions.len());
            for (sid, mut v) in sessions {
                let mut si = c.prepare(
                    "SELECT id, ord, kind, section_id, section_title, est_minutes, part, parts, done_at
                     FROM session_items WHERE session_id = ?1 ORDER BY ord",
                )?;
                let items = si
                    .query_map(params![sid], |r| {
                        Ok(serde_json::json!({
                            "id": r.get::<_, String>(0)?,
                            "ord": r.get::<_, i64>(1)?,
                            "kind": r.get::<_, String>(2)?,
                            "sectionId": r.get::<_, Option<String>>(3)?,
                            "sectionTitle": r.get::<_, String>(4)?,
                            "estMinutes": r.get::<_, i64>(5)?,
                            "part": r.get::<_, i64>(6)?,
                            "parts": r.get::<_, i64>(7)?,
                            "doneAt": r.get::<_, Option<i64>>(8)?,
                        }))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                v["items"] = Value::Array(items);
                out.push(v);
            }
            Ok(out)
        })
    }

    pub fn session_get(&self, id: &str) -> Result<Option<Value>> {
        let plan_id: Option<String> = self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT plan_id FROM sessions WHERE id = ?1",
                params![id],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
        })?;
        let Some(pid) = plan_id else { return Ok(None) };
        let mut found = self
            .sessions_of_plan(&pid)?
            .into_iter()
            .find(|s| s["id"] == id);
        if let Some(v) = found.as_mut() {
            v["planId"] = Value::String(pid);
        }
        Ok(found)
    }

    pub fn session_set_event(&self, session_id: &str, event_id: Option<&str>) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE sessions SET event_id = ?2 WHERE id = ?1",
                params![session_id, event_id],
            )?;
            Ok(())
        })
    }

    pub fn session_complete(&self, session_id: &str, done: bool) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE sessions SET status = ?2, completed_at = ?3 WHERE id = ?1",
                params![
                    session_id,
                    if done { "done" } else { "planned" },
                    if done { Some(now_ms()) } else { None }
                ],
            )?;
            Ok(())
        })
    }

    pub fn item_complete(&self, item_id: &str, done: bool) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE session_items SET done_at = ?2 WHERE id = ?1",
                params![item_id, if done { Some(now_ms()) } else { None }],
            )?;
            Ok(())
        })
    }

    /// Sessions scheduled on `date` (YYYY-MM-DD) across every active plan.
    pub fn sessions_on(&self, date: &str) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT s.id, s.plan_id, p.title, s.ord, s.date, s.start_hm, s.minutes, s.title,
                        s.status, s.event_id
                 FROM sessions s JOIN plans p ON p.id = s.plan_id
                 WHERE s.date = ?1 AND p.status = 'active' ORDER BY s.start_hm",
            )?;
            let rows = st
                .query_map(params![date], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "planId": r.get::<_, String>(1)?,
                        "planTitle": r.get::<_, String>(2)?,
                        "ord": r.get::<_, i64>(3)?,
                        "date": r.get::<_, String>(4)?,
                        "startHm": r.get::<_, String>(5)?,
                        "minutes": r.get::<_, i64>(6)?,
                        "title": r.get::<_, String>(7)?,
                        "status": r.get::<_, String>(8)?,
                        "eventId": r.get::<_, Option<String>>(9)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Planned sessions in the past that were never completed.
    pub fn sessions_missed_before(&self, date: &str) -> Result<Vec<(String, String)>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT s.id, s.plan_id FROM sessions s JOIN plans p ON p.id = s.plan_id
                 WHERE s.date < ?1 AND s.status = 'planned' AND p.status = 'active'
                 ORDER BY s.date",
            )?;
            let rows = st
                .query_map(params![date], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn template_get(&self, key: &str) -> Result<Option<crate::planner::Template>> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT key, label, detail, days, min_per_day, review_offsets, blocks, content_ratio
                 FROM plan_templates WHERE key = ?1",
                params![key],
                |r| {
                    Ok(crate::planner::Template {
                        key: r.get::<_, String>(0)?,
                        label: r.get::<_, String>(1)?,
                        detail: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        days: r.get::<_, i64>(3)?,
                        min_per_day: r.get::<_, i64>(4)?,
                        review_offsets: serde_json::from_str(&r.get::<_, String>(5)?)
                            .unwrap_or_default(),
                        blocks: serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or_default(),
                        content_ratio: r.get::<_, f64>(7)?,
                    })
                },
            )
            .optional()?)
        })
    }
}

// ── Cards, quiz, ask ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardRow {
    pub id: String,
    pub doc_id: Option<String>,
    pub section_id: Option<String>,
    pub chunk_id: Option<i64>,
    pub concept_id: Option<String>,
    pub front: String,
    pub back: String,
    pub kind: String,
    pub source: String,
    pub level: i64,
    pub next_review: Option<String>,
    pub is_urgent: bool,
    pub reviews: i64,
    pub lapses: i64,
}

fn map_card(r: &Row) -> rusqlite::Result<CardRow> {
    Ok(CardRow {
        id: r.get(0)?,
        doc_id: r.get(1)?,
        section_id: r.get(2)?,
        chunk_id: r.get(3)?,
        concept_id: r.get(4)?,
        front: r.get(5)?,
        back: r.get(6)?,
        kind: r.get(7)?,
        source: r.get(8)?,
        level: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
        next_review: r.get(10)?,
        is_urgent: r.get::<_, Option<i64>>(11)?.unwrap_or(0) != 0,
        reviews: r.get::<_, Option<i64>>(12)?.unwrap_or(0),
        lapses: r.get::<_, Option<i64>>(13)?.unwrap_or(0),
    })
}

const CARD_SELECT: &str = "SELECT c.id, c.doc_id, c.section_id, c.chunk_id, c.concept_id,
        c.front, c.back, c.kind, c.source,
        p.level, p.next_review, p.is_urgent, p.reviews, p.lapses
 FROM cards c LEFT JOIN card_progress p ON p.card_id = c.id";

impl Db {
    #[allow(clippy::too_many_arguments)]
    pub fn card_insert(
        &self,
        doc_id: Option<&str>,
        section_id: Option<&str>,
        chunk_id: Option<i64>,
        concept_id: Option<&str>,
        front: &str,
        back: &str,
        kind: &str,
        source: &str,
    ) -> Result<String> {
        let id = new_id();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO cards (id, doc_id, section_id, chunk_id, concept_id, front, back,
                                    kind, source, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    id,
                    doc_id,
                    section_id,
                    chunk_id,
                    concept_id,
                    front.trim(),
                    back.trim(),
                    kind,
                    source,
                    now_ms()
                ],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    /// True when a card with the same front already exists for this section —
    /// re-running generation must not pile up duplicates the learner has to
    /// grade twice.
    pub fn card_exists(&self, section_id: Option<&str>, front: &str) -> Result<bool> {
        let norm = crate::corpus::squash_ws(&crate::corpus::fold(front));
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT front FROM cards WHERE (?1 IS NULL OR section_id = ?1)",
            )?;
            let existing: Vec<String> = st
                .query_map(params![section_id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(existing
                .iter()
                .any(|f| crate::corpus::squash_ws(&crate::corpus::fold(f)) == norm))
        })
    }

    pub fn card_get(&self, id: &str) -> Result<Option<CardRow>> {
        self.with_conn(|c| {
            Ok(c.query_row(
                &format!("{CARD_SELECT} WHERE c.id = ?1"),
                params![id],
                map_card,
            )
            .optional()?)
        })
    }

    pub fn cards_of_section(&self, section_id: &str) -> Result<Vec<CardRow>> {
        self.with_conn(|c| {
            let mut st = c.prepare(&format!(
                "{CARD_SELECT} WHERE c.section_id = ?1 ORDER BY c.created_at"
            ))?;
            let rows = st
                .query_map(params![section_id], map_card)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn cards_of_doc(&self, doc_id: &str) -> Result<Vec<CardRow>> {
        self.with_conn(|c| {
            let mut st = c.prepare(&format!(
                "{CARD_SELECT} WHERE c.doc_id = ?1 ORDER BY c.created_at"
            ))?;
            let rows = st
                .query_map(params![doc_id], map_card)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Cards due now: never-reviewed cards first, then overdue ones.
    pub fn cards_due(&self, now_iso: &str, limit: usize) -> Result<Vec<CardRow>> {
        self.with_conn(|c| {
            let mut st = c.prepare(&format!(
                "{CARD_SELECT}
                 WHERE p.card_id IS NULL OR p.next_review <= ?1
                 ORDER BY (p.card_id IS NULL) DESC, p.is_urgent DESC, p.next_review ASC
                 LIMIT ?2"
            ))?;
            let rows = st
                .query_map(params![now_iso, limit], map_card)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn card_due_count(&self, now_iso: &str) -> Result<i64> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM cards c LEFT JOIN card_progress p ON p.card_id = c.id
                 WHERE p.card_id IS NULL OR p.next_review <= ?1",
                params![now_iso],
                |r| r.get::<_, i64>(0),
            )?)
        })
    }

    pub fn card_delete(&self, id: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM cards WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    pub fn card_progress_get(&self, card_id: &str) -> Result<Option<crate::srs::Progress>> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT level, next_review, is_urgent, last_reviewed, first_due_at, reviews, lapses
                 FROM card_progress WHERE card_id = ?1",
                params![card_id],
                |r| {
                    Ok(crate::srs::Progress {
                        level: r.get(0)?,
                        next_review: crate::srs::parse(&r.get::<_, String>(1)?)
                            .unwrap_or_else(chrono::Utc::now),
                        is_urgent: r.get::<_, i64>(2)? != 0,
                        last_reviewed: crate::srs::parse(&r.get::<_, String>(3)?)
                            .unwrap_or_else(chrono::Utc::now),
                        first_due_at: r
                            .get::<_, Option<String>>(4)?
                            .and_then(|s| crate::srs::parse(&s)),
                        reviews: r.get(5)?,
                        lapses: r.get(6)?,
                    })
                },
            )
            .optional()?)
        })
    }

    pub fn card_progress_put(&self, card_id: &str, p: &crate::srs::Progress) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO card_progress
                    (card_id, level, next_review, is_urgent, last_reviewed, first_due_at,
                     reviews, lapses)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(card_id) DO UPDATE SET
                    level = excluded.level, next_review = excluded.next_review,
                    is_urgent = excluded.is_urgent, last_reviewed = excluded.last_reviewed,
                    first_due_at = excluded.first_due_at, reviews = excluded.reviews,
                    lapses = excluded.lapses",
                params![
                    card_id,
                    p.level,
                    crate::srs::fmt(p.next_review),
                    p.is_urgent as i64,
                    crate::srs::fmt(p.last_reviewed),
                    p.first_due_at.map(crate::srs::fmt),
                    p.reviews,
                    p.lapses
                ],
            )?;
            Ok(())
        })
    }

    // ── Questions ───────────────────────────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn question_insert(
        &self,
        doc_id: &str,
        section_id: Option<&str>,
        concept_id: Option<&str>,
        kind: &str,
        stem: &str,
        options: &Value,
        answer: &Value,
        explain: &str,
        chunk_id: i64,
        quote: &str,
        difficulty: i64,
    ) -> Result<String> {
        let id = new_id();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO questions (id, doc_id, section_id, concept_id, kind, stem, options,
                                        answer, explain, chunk_id, quote, difficulty, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                params![
                    id,
                    doc_id,
                    section_id,
                    concept_id,
                    kind,
                    stem,
                    options.to_string(),
                    answer.to_string(),
                    explain,
                    chunk_id,
                    quote,
                    difficulty.clamp(1, 5),
                    now_ms()
                ],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn question_get(&self, id: &str) -> Result<Option<Value>> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT id, doc_id, section_id, kind, stem, options, answer, explain,
                        chunk_id, quote, difficulty
                 FROM questions WHERE id = ?1",
                params![id],
                map_question,
            )
            .optional()?)
        })
    }

    /// Pick questions for a quiz. Sections narrow the pool; weak concepts are
    /// preferred, and within a difficulty tier the least-recently-attempted
    /// question wins so a quiz doesn't repeat the same three items.
    pub fn questions_pick(
        &self,
        doc_id: &str,
        section_ids: &[String],
        limit: usize,
    ) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut sql = String::from(
                "SELECT q.id, q.doc_id, q.section_id, q.kind, q.stem, q.options, q.answer,
                        q.explain, q.chunk_id, q.quote, q.difficulty
                 FROM questions q WHERE q.doc_id = ?1",
            );
            if !section_ids.is_empty() {
                let ph = section_ids
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", i + 3))
                    .collect::<Vec<_>>()
                    .join(",");
                sql.push_str(&format!(" AND q.section_id IN ({ph})"));
            }
            sql.push_str(
                " ORDER BY
                    (SELECT COUNT(*) FROM attempts a WHERE a.question_id = q.id AND a.correct = 0) DESC,
                    (SELECT MAX(a.answered_at) FROM attempts a WHERE a.question_id = q.id) ASC,
                    q.created_at
                  LIMIT ?2",
            );
            let mut st = c.prepare(&sql)?;
            let mut binds: Vec<&dyn rusqlite::ToSql> = vec![&doc_id, &limit];
            for s in section_ids {
                binds.push(s);
            }
            let rows = st
                .query_map(binds.as_slice(), map_question)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn question_count(&self, doc_id: &str, section_id: Option<&str>) -> Result<i64> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM questions
                 WHERE doc_id = ?1 AND (?2 IS NULL OR section_id = ?2)",
                params![doc_id, section_id],
                |r| r.get::<_, i64>(0),
            )?)
        })
    }

    pub fn attempt_insert(
        &self,
        question_id: &str,
        quiz_id: &str,
        chosen: &Value,
        correct: bool,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO attempts (id, question_id, quiz_id, chosen, correct, answered_at)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    new_id(),
                    question_id,
                    quiz_id,
                    chosen.to_string(),
                    correct as i64,
                    now_ms()
                ],
            )?;
            Ok(())
        })
    }

    /// Concepts the learner keeps getting wrong, worst first.
    pub fn weak_concepts(&self, doc_id: &str, limit: usize) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT co.id, co.name,
                        SUM(CASE WHEN a.correct = 0 THEN 1 ELSE 0 END) AS wrong,
                        COUNT(a.id) AS total
                 FROM attempts a
                 JOIN questions q ON q.id = a.question_id
                 JOIN concept_sections cs ON cs.section_id = q.section_id
                 JOIN concepts co ON co.id = cs.concept_id
                 WHERE q.doc_id = ?1
                 GROUP BY co.id
                 HAVING total > 0
                 ORDER BY (CAST(wrong AS REAL) / total) DESC, wrong DESC
                 LIMIT ?2",
            )?;
            let rows = st
                .query_map(params![doc_id, limit], |r| {
                    let wrong: i64 = r.get(2)?;
                    let total: i64 = r.get(3)?;
                    Ok(serde_json::json!({
                        "conceptId": r.get::<_, String>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "wrong": wrong,
                        "total": total,
                        "wrongRate": if total > 0 { wrong as f64 / total as f64 } else { 0.0 },
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    // ── Ask history ─────────────────────────────────────────────────────────

    pub fn ask_insert(
        &self,
        question: &str,
        scope: &Value,
        answer_md: &str,
        evidence: &Value,
        external: bool,
    ) -> Result<String> {
        let id = new_id();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO asks (id, question, scope, answer_md, evidence, external, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    id,
                    question,
                    scope.to_string(),
                    answer_md,
                    evidence.to_string(),
                    external as i64,
                    now_ms()
                ],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn ask_list(&self, limit: usize) -> Result<Vec<Value>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id, question, answer_md, evidence, external, created_at
                 FROM asks ORDER BY created_at DESC LIMIT ?1",
            )?;
            let rows = st
                .query_map(params![limit], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, String>(0)?,
                        "question": r.get::<_, String>(1)?,
                        "answerMd": r.get::<_, String>(2)?,
                        "evidence": serde_json::from_str::<Value>(&r.get::<_, String>(3)?)
                            .unwrap_or(Value::Null),
                        "external": r.get::<_, i64>(4)? != 0,
                        "createdAt": r.get::<_, i64>(5)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    // ── TTS cache ───────────────────────────────────────────────────────────

    pub fn tts_cached(&self, hash: &str) -> Result<Option<String>> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT path FROM tts_cache WHERE hash = ?1",
                params![hash],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
        })
    }

    pub fn tts_put(
        &self,
        hash: &str,
        voice: Option<&str>,
        speed: f64,
        path: &str,
        bytes: i64,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO tts_cache (hash, voice, speed, path, bytes, created_at)
                 VALUES (?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(hash) DO UPDATE SET path = excluded.path, bytes = excluded.bytes",
                params![hash, voice, speed, path, bytes, now_ms()],
            )?;
            Ok(())
        })
    }
}

fn map_question(r: &Row) -> rusqlite::Result<Value> {
    Ok(serde_json::json!({
        "id": r.get::<_, String>(0)?,
        "docId": r.get::<_, String>(1)?,
        "sectionId": r.get::<_, Option<String>>(2)?,
        "kind": r.get::<_, String>(3)?,
        "stem": r.get::<_, String>(4)?,
        "options": serde_json::from_str::<Value>(&r.get::<_, String>(5)?).unwrap_or(Value::Null),
        "answer": serde_json::from_str::<Value>(&r.get::<_, String>(6)?).unwrap_or(Value::Null),
        "explain": r.get::<_, Option<String>>(7)?,
        "chunkId": r.get::<_, i64>(8)?,
        "quote": r.get::<_, String>(9)?,
        "difficulty": r.get::<_, i64>(10)?,
    }))
}

/// Columns added after the first release. `CREATE TABLE IF NOT EXISTS` does not
/// touch an existing table, so new columns need an explicit pass.
fn migrate(conn: &Connection) -> Result<()> {
    let mut have: Vec<String> = Vec::new();
    {
        let mut st = conn.prepare("PRAGMA table_info(docs)")?;
        let names = st.query_map([], |r| r.get::<_, String>(1))?;
        for n in names {
            have.push(n?);
        }
    }
    if !have.iter().any(|c| c == "suspects") {
        conn.execute("ALTER TABLE docs ADD COLUMN suspects TEXT", [])?;
    }
    Ok(())
}

impl Db {
    /// Store the repeated lines the cleaner flagged (it does not remove them).
    pub fn set_suspects(&self, doc_id: &str, suspects: &[crate::corpus::Suspect]) -> Result<()> {
        let json = serde_json::to_string(suspects).unwrap_or_else(|_| "[]".into());
        self.with_conn(|c| {
            c.execute(
                "UPDATE docs SET suspects = ?2 WHERE id = ?1",
                params![doc_id, json],
            )?;
            Ok(())
        })
    }

    pub fn suspects(&self, doc_id: &str) -> Result<Vec<crate::corpus::Suspect>> {
        self.with_conn(|c| {
            let raw: Option<String> = c
                .query_row("SELECT suspects FROM docs WHERE id = ?1", params![doc_id], |r| {
                    r.get(0)
                })
                .optional()?;
            Ok(raw
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default())
        })
    }

    /// Remove user-confirmed page furniture, then rebuild everything derived
    /// from the text.
    ///
    /// Re-indexing invalidates chunk ids, so each stored question is re-pointed
    /// at the chunk that now holds its (already verified) quote. A question
    /// whose quote no longer exists — because the user stripped the line it sat
    /// on — is reported rather than silently left dangling.
    pub fn strip_lines(&self, doc_id: &str, lines: &[String]) -> Result<Value> {
        let body = self
            .doc_body(doc_id)?
            .ok_or_else(|| anyhow!("không tìm thấy tài liệu"))?;
        let (next, removed) = crate::corpus::strip_lines(&body, lines);
        if next.trim().is_empty() {
            return Err(anyhow!(
                "bỏ những dòng này sẽ xoá sạch tài liệu — hãy chọn lại"
            ));
        }
        self.with_conn(|c| {
            c.execute(
                "UPDATE docs SET body = ?2, chars = ?3, updated_at = ?4 WHERE id = ?1",
                params![doc_id, next, next.chars().count() as i64, now_ms()],
            )?;
            Ok(())
        })?;

        // Whatever is left may still repeat; recompute the flags.
        let cleaned = crate::corpus::dedupe(&next);
        self.set_suspects(doc_id, &cleaned.suspects)?;
        Ok(serde_json::json!({
            "removedLines": removed,
            "suspectedFurniture": cleaned.suspects,
        }))
    }

    /// Re-point questions at the chunks that now contain their quotes.
    /// Returns `(repointed, orphaned)`.
    pub fn repoint_questions(&self, doc_id: &str) -> Result<(usize, usize)> {
        let questions: Vec<(String, String)> = self.with_conn(|c| {
            let mut st = c.prepare("SELECT id, quote FROM questions WHERE doc_id = ?1")?;
            let rows = st
                .query_map(params![doc_id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;
        if questions.is_empty() {
            return Ok((0, 0));
        }
        let chunks: Vec<ChunkRow> = self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id, doc_id, section_id, ord, char_start, char_end, text
                 FROM chunks WHERE doc_id = ?1 ORDER BY ord",
            )?;
            let rows = st
                .query_map(params![doc_id], map_chunk)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;

        let mut ok = 0;
        let mut orphan = 0;
        for (qid, quote) in questions {
            let needle = corpus::squash_ws(&corpus::fold(&quote));
            let hit = chunks
                .iter()
                .find(|c| corpus::squash_ws(&corpus::fold(&c.text)).contains(&needle));
            match hit {
                Some(c) => {
                    self.with_conn(|conn| {
                        conn.execute(
                            "UPDATE questions SET chunk_id = ?2, section_id = ?3 WHERE id = ?1",
                            params![qid, c.id, c.section_id],
                        )?;
                        Ok(())
                    })?;
                    ok += 1;
                }
                None => orphan += 1,
            }
        }
        Ok((ok, orphan))
    }
}

// ── Row mappers ─────────────────────────────────────────────────────────────

fn map_doc(r: &Row) -> rusqlite::Result<DocRow> {
    Ok(DocRow {
        id: r.get(0)?,
        title: r.get(1)?,
        filename: r.get(2)?,
        ext: r.get(3)?,
        bytes: r.get(4)?,
        chars: r.get(5)?,
        extract_note: r.get(6)?,
        summary: r.get(7)?,
        status: r.get(8)?,
        error: r.get(9)?,
        added_at: r.get(10)?,
        updated_at: r.get(11)?,
        section_count: r.get(12)?,
        chunk_count: r.get(13)?,
    })
}

fn map_section(r: &Row) -> rusqlite::Result<SectionRow> {
    Ok(SectionRow {
        id: r.get(0)?,
        doc_id: r.get(1)?,
        ord: r.get(2)?,
        title: r.get(3)?,
        level: r.get(4)?,
        char_start: r.get(5)?,
        char_end: r.get(6)?,
        summary: r.get(7)?,
        key_points: json_str_array(r.get(8)?),
        difficulty: r.get(9)?,
        est_minutes: r.get(10)?,
        prereq: json_str_array(r.get(11)?),
        enriched_at: r.get(12)?,
    })
}

fn map_chunk(r: &Row) -> rusqlite::Result<ChunkRow> {
    Ok(ChunkRow {
        id: r.get(0)?,
        doc_id: r.get(1)?,
        section_id: r.get(2)?,
        ord: r.get(3)?,
        char_start: r.get(4)?,
        char_end: r.get(5)?,
        text: r.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded() -> (Db, String) {
        let db = Db::open_memory().unwrap();
        let body = "Chương 1: Lãi suất\n\nLãi suất điều hành do NHNN công bố.\n\n\
                    Chương 2: Tỷ giá\n\nTỷ giá trung tâm được công bố hằng ngày.";
        let id = db
            .doc_insert("Kinh tế", "kt.md", "md", 100, "ok", body)
            .unwrap();
        let chunks = corpus::chunk(body);
        db.chunks_replace(&id, &chunks, |_| None).unwrap();
        (db, id)
    }

    #[test]
    fn a_document_round_trips_with_its_counts() {
        let (db, id) = seeded();
        let d = db.doc_get(&id).unwrap().unwrap();
        assert_eq!(d.title, "Kinh tế");
        assert!(d.chunk_count > 0);
        assert_eq!(db.doc_list().unwrap().len(), 1);
    }

    #[test]
    fn search_finds_text_without_diacritics() {
        let (db, _) = seeded();
        let hits = db.search_chunks("lai suat", &[], 10).unwrap();
        assert!(!hits.is_empty(), "undiacriticised query must still hit");
    }

    #[test]
    fn search_scores_are_higher_is_better() {
        let (db, _) = seeded();
        let hits = db.search_chunks("lãi suất", &[], 10).unwrap();
        assert!(hits[0].1 > 0.0, "bm25 must be flipped for the caller");
    }

    #[test]
    fn deleting_a_document_also_removes_it_from_the_search_index() {
        let (db, id) = seeded();
        assert!(!db.search_chunks("lãi suất", &[], 10).unwrap().is_empty());
        db.doc_delete(&id).unwrap();
        assert!(
            db.search_chunks("lãi suất", &[], 10).unwrap().is_empty(),
            "an orphaned FTS row keeps answering for deleted text"
        );
    }

    #[test]
    fn reindexing_does_not_duplicate_fts_rows() {
        let (db, id) = seeded();
        let body = db.doc_body(&id).unwrap().unwrap();
        let before = db.search_chunks("lãi suất", &[], 50).unwrap().len();
        db.chunks_replace(&id, &corpus::chunk(&body), |_| None)
            .unwrap();
        let after = db.search_chunks("lãi suất", &[], 50).unwrap().len();
        assert_eq!(before, after);
    }

    #[test]
    fn a_search_scoped_to_another_document_returns_nothing() {
        let (db, _) = seeded();
        let hits = db
            .search_chunks("lãi suất", &["khong-ton-tai".to_string()], 10)
            .unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn concepts_dedupe_by_folded_name() {
        let (db, id) = seeded();
        let a = db.concept_upsert(&id, "Lãi suất").unwrap();
        let b = db.concept_upsert(&id, "lãi  suất").unwrap();
        assert_ne!(a, b, "different whitespace is a different raw name");
        let c = db.concept_upsert(&id, "Lãi suất").unwrap();
        assert_eq!(a, c);
    }

    #[test]
    fn builtin_templates_are_seeded() {
        let db = Db::open_memory().unwrap();
        let t = db.templates().unwrap();
        assert!(t.len() >= 5, "five study templates ship with the app");
        assert!(db.template_get("standard").unwrap().is_some());
    }
}
