//! SQLite layer — single serialized connection behind a `Mutex` with WAL,
//! matching the other Space Apps. Typed structs; the schema is small and fixed.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::srs::{self, ProgressData};

const SCHEMA: &str = include_str!("schema.sql");

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub native_language: String,
    pub study_slots: Vec<String>,
    pub timezone: String,
    pub daily_word_goal: i64,
    pub current_streak: i64,
    pub last_study_date: Option<String>,
    pub total_xp: i64,
    pub snooze_until: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub created_at: String,
    pub card_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub lesson_id: String,
    pub word: String,
    pub image_url: Option<String>,
    pub ipa: Option<String>,
    pub part_of_speech: Option<String>,
    pub examples: Option<serde_json::Value>,
    pub explain: String,
    pub meanings: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct ProgressRow {
    pub card_id: String,
    pub data: ProgressData,
    pub created_at: DateTime<Utc>,
}

fn parse_json_opt(s: Option<String>) -> Option<serde_json::Value> {
    s.and_then(|v| serde_json::from_str(&v).ok())
}

fn card_from_row(row: &Row) -> rusqlite::Result<Card> {
    Ok(Card {
        id: row.get("id")?,
        lesson_id: row.get("lesson_id")?,
        word: row.get("word")?,
        image_url: row.get("image_url")?,
        ipa: row.get("ipa")?,
        part_of_speech: row.get("part_of_speech")?,
        examples: parse_json_opt(row.get("examples")?),
        explain: row.get("explain")?,
        meanings: parse_json_opt(row.get("meanings")?),
    })
}

fn progress_from_row(row: &Row) -> rusqlite::Result<ProgressRow> {
    let t = |name: &str| -> rusqlite::Result<Option<DateTime<Utc>>> {
        let v: Option<String> = row.get(name)?;
        Ok(v.as_deref().and_then(srs::parse))
    };
    Ok(ProgressRow {
        card_id: row.get("card_id")?,
        data: ProgressData {
            level: row.get("level")?,
            next_review: t("next_review")?.unwrap_or_else(Utc::now),
            is_urgent: row.get::<_, i64>("is_urgent")? != 0,
            last_reviewed: t("last_reviewed")?.unwrap_or_else(Utc::now),
            first_due_at: t("first_due_at")?,
            notification_sent_at: t("notification_sent_at")?,
        },
        created_at: t("created_at")?.unwrap_or_else(Utc::now),
    })
}

const CARD_COLS: &str =
    "id, lesson_id, word, image_url, ipa, part_of_speech, examples, explain, meanings";
const PROGRESS_COLS: &str =
    "card_id, level, next_review, is_urgent, last_reviewed, first_due_at, notification_sent_at, created_at";

impl Db {
    pub fn open(path: &str) -> Result<Db> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db> {
        conn.pragma_update(None, "journal_mode", "WAL").ok();
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        Ok(f(&conn)?)
    }

    // ---- settings ----

    pub fn settings(&self) -> Result<Settings> {
        self.with(|c| {
            c.query_row("SELECT * FROM settings WHERE id = 1", [], |row| {
                let slots_raw: String = row.get("study_slots")?;
                Ok(Settings {
                    native_language: row.get("native_language")?,
                    study_slots: serde_json::from_str(&slots_raw).unwrap_or_default(),
                    timezone: row.get("timezone")?,
                    daily_word_goal: row.get("daily_word_goal")?,
                    current_streak: row.get("current_streak")?,
                    last_study_date: row.get("last_study_date")?,
                    total_xp: row.get("total_xp")?,
                    snooze_until: row.get("snooze_until")?,
                })
            })
        })
    }

    pub fn set_setting_field(&self, field: &str, value: &str) -> Result<()> {
        // Column names are fixed by callers; never interpolate user input here.
        assert!(matches!(
            field,
            "native_language" | "study_slots" | "timezone" | "daily_word_goal" | "snooze_until"
        ));
        self.with(|c| {
            c.execute(
                &format!("UPDATE settings SET {field} = ?1 WHERE id = 1"),
                params![value],
            )
        })?;
        Ok(())
    }

    pub fn add_xp(&self, xp: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE settings SET total_xp = total_xp + ?1 WHERE id = 1",
                params![xp],
            )
        })?;
        Ok(())
    }

    pub fn set_streak(&self, streak: i64, last_study_date: Option<&str>) -> Result<()> {
        self.with(|c| match last_study_date {
            Some(d) => c.execute(
                "UPDATE settings SET current_streak = ?1, last_study_date = ?2 WHERE id = 1",
                params![streak, d],
            ),
            None => c.execute(
                "UPDATE settings SET current_streak = ?1 WHERE id = 1",
                params![streak],
            ),
        })?;
        Ok(())
    }

    // ---- lessons ----

    pub fn create_lesson(&self, title: &str, description: Option<&str>) -> Result<Lesson> {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = srs::fmt(Utc::now());
        self.with(|c| {
            c.execute(
                "INSERT INTO lessons (id, title, description, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, title, description, created_at],
            )
        })?;
        Ok(Lesson {
            id,
            title: title.to_string(),
            description: description.map(str::to_string),
            created_at,
            card_count: 0,
        })
    }

    pub fn list_lessons(&self) -> Result<Vec<Lesson>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT l.id, l.title, l.description, l.created_at,
                        (SELECT COUNT(*) FROM cards WHERE lesson_id = l.id) AS card_count
                 FROM lessons l ORDER BY l.created_at DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(Lesson {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    description: row.get(2)?,
                    created_at: row.get(3)?,
                    card_count: row.get(4)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn get_lesson(&self, id: &str) -> Result<Option<Lesson>> {
        self.with(|c| {
            c.query_row(
                "SELECT l.id, l.title, l.description, l.created_at,
                        (SELECT COUNT(*) FROM cards WHERE lesson_id = l.id) AS card_count
                 FROM lessons l WHERE l.id = ?1",
                params![id],
                |row| {
                    Ok(Lesson {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        description: row.get(2)?,
                        created_at: row.get(3)?,
                        card_count: row.get(4)?,
                    })
                },
            )
            .optional()
        })
    }

    pub fn update_lesson(
        &self,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
    ) -> Result<bool> {
        let n = self.with(|c| {
            c.execute(
                "UPDATE lessons SET title = COALESCE(?2, title), description = COALESCE(?3, description) WHERE id = ?1",
                params![id, title, description],
            )
        })?;
        Ok(n > 0)
    }

    pub fn delete_lesson(&self, id: &str) -> Result<bool> {
        let n = self.with(|c| c.execute("DELETE FROM lessons WHERE id = ?1", params![id]))?;
        Ok(n > 0)
    }

    // ---- cards ----

    #[allow(clippy::too_many_arguments)]
    pub fn insert_card(
        &self,
        lesson_id: &str,
        word: &str,
        image_url: Option<&str>,
        ipa: Option<&str>,
        part_of_speech: Option<&str>,
        examples: Option<&serde_json::Value>,
        explain: &str,
        meanings: Option<&serde_json::Value>,
    ) -> Result<Card> {
        let id = uuid::Uuid::new_v4().to_string();
        let examples_s = examples.map(|v| v.to_string());
        let meanings_s = meanings.map(|v| v.to_string());
        self.with(|c| {
            c.execute(
                "INSERT INTO cards (id, lesson_id, word, image_url, ipa, part_of_speech, examples, explain, meanings)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![id, lesson_id, word, image_url, ipa, part_of_speech, examples_s, explain, meanings_s],
            )
        })?;
        Ok(Card {
            id,
            lesson_id: lesson_id.to_string(),
            word: word.to_string(),
            image_url: image_url.map(str::to_string),
            ipa: ipa.map(str::to_string),
            part_of_speech: part_of_speech.map(str::to_string),
            examples: examples.cloned(),
            explain: explain.to_string(),
            meanings: meanings.cloned(),
        })
    }

    pub fn update_card_fields(
        &self,
        card_id: &str,
        fields: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<bool> {
        // Whitelisted camelCase → column mapping; unknown keys are ignored.
        let map = [
            ("word", "word"),
            ("imageUrl", "image_url"),
            ("ipa", "ipa"),
            ("partOfSpeech", "part_of_speech"),
            ("examples", "examples"),
            ("explain", "explain"),
            ("meanings", "meanings"),
        ];
        let mut sets = Vec::new();
        let mut vals: Vec<Option<String>> = Vec::new();
        for (key, col) in map {
            if let Some(v) = fields.get(key) {
                sets.push(format!("{col} = ?{}", sets.len() + 2));
                vals.push(match v {
                    serde_json::Value::Null => None,
                    serde_json::Value::String(s) => Some(s.clone()),
                    other => Some(other.to_string()),
                });
            }
        }
        if sets.is_empty() {
            return Ok(false);
        }
        let sql = format!("UPDATE cards SET {} WHERE id = ?1", sets.join(", "));
        let n = self.with(|c| {
            let mut p: Vec<&dyn rusqlite::ToSql> = vec![&card_id];
            for v in &vals {
                p.push(v);
            }
            c.execute(&sql, p.as_slice())
        })?;
        Ok(n > 0)
    }

    pub fn delete_card(&self, card_id: &str) -> Result<bool> {
        let n = self.with(|c| c.execute("DELETE FROM cards WHERE id = ?1", params![card_id]))?;
        Ok(n > 0)
    }

    pub fn get_card(&self, card_id: &str) -> Result<Option<Card>> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {CARD_COLS} FROM cards WHERE id = ?1"),
                params![card_id],
                card_from_row,
            )
            .optional()
        })
    }

    pub fn cards_of_lesson(&self, lesson_id: &str) -> Result<Vec<Card>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {CARD_COLS} FROM cards WHERE lesson_id = ?1 ORDER BY rowid ASC"
            ))?;
            let rows = stmt.query_map(params![lesson_id], card_from_row)?;
            rows.collect()
        })
    }

    /// Cards that have never been graded (no progress row) — the "new" pool.
    pub fn cards_without_progress(&self) -> Result<Vec<Card>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {CARD_COLS} FROM cards
                 WHERE id NOT IN (SELECT card_id FROM card_progress)"
            ))?;
            let rows = stmt.query_map([], card_from_row)?;
            rows.collect()
        })
    }

    // ---- progress ----

    pub fn get_progress(&self, card_id: &str) -> Result<Option<ProgressRow>> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {PROGRESS_COLS} FROM card_progress WHERE card_id = ?1"),
                params![card_id],
                progress_from_row,
            )
            .optional()
        })
    }

    pub fn upsert_progress(&self, card_id: &str, p: &ProgressData) -> Result<()> {
        let now = srs::fmt(Utc::now());
        self.with(|c| {
            c.execute(
                "INSERT INTO card_progress
                   (card_id, level, next_review, is_urgent, last_reviewed, first_due_at, notification_sent_at, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(card_id) DO UPDATE SET
                   level = excluded.level,
                   next_review = excluded.next_review,
                   is_urgent = excluded.is_urgent,
                   last_reviewed = excluded.last_reviewed,
                   first_due_at = excluded.first_due_at,
                   notification_sent_at = excluded.notification_sent_at",
                params![
                    card_id,
                    p.level,
                    srs::fmt(p.next_review),
                    p.is_urgent as i64,
                    srs::fmt(p.last_reviewed),
                    p.first_due_at.map(srs::fmt),
                    p.notification_sent_at.map(srs::fmt),
                    now,
                ],
            )
        })?;
        Ok(())
    }

    pub fn remove_progress(&self, card_id: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "DELETE FROM card_progress WHERE card_id = ?1",
                params![card_id],
            )
        })?;
        Ok(())
    }

    pub fn learned_count(&self) -> Result<i64> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM card_progress WHERE level > 0",
                [],
                |r| r.get(0),
            )
        })
    }

    pub fn due_count(&self, now: DateTime<Utc>) -> Result<i64> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM card_progress WHERE next_review <= ?1",
                params![srs::fmt(now)],
                |r| r.get(0),
            )
        })
    }

    /// Random review cards for the 6-minute session: `level in [min_level, max_level]`.
    pub fn random_review_cards(
        &self,
        min_level: i64,
        max_level: i64,
        limit: i64,
    ) -> Result<Vec<(Card, ProgressRow)>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {CARD_COLS}, {prog} FROM card_progress p
                 JOIN cards ON cards.id = p.card_id
                 WHERE p.level >= ?1 AND p.level <= ?2
                 ORDER BY RANDOM() LIMIT ?3",
                prog = "p.card_id, p.level, p.next_review, p.is_urgent, p.last_reviewed, p.first_due_at, p.notification_sent_at, p.created_at",
            ))?;
            let rows = stmt.query_map(params![min_level, max_level, limit], |row| {
                Ok((card_from_row(row)?, progress_from_row(row)?))
            })?;
            rows.collect()
        })
    }

    /// All progress rows joined with their card, most recently reviewed first.
    pub fn progress_with_cards(&self) -> Result<Vec<(Card, ProgressRow)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT cards.id, cards.lesson_id, cards.word, cards.image_url, cards.ipa,
                        cards.part_of_speech, cards.examples, cards.explain, cards.meanings,
                        p.card_id, p.level, p.next_review, p.is_urgent, p.last_reviewed,
                        p.first_due_at, p.notification_sent_at, p.created_at
                 FROM card_progress p JOIN cards ON cards.id = p.card_id
                 ORDER BY p.last_reviewed DESC",
            )?;
            let rows =
                stmt.query_map([], |row| Ok((card_from_row(row)?, progress_from_row(row)?)))?;
            rows.collect()
        })
    }

    pub fn progress_for_cards(&self, card_ids: &[String]) -> Result<Vec<ProgressRow>> {
        if card_ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|c| {
            let placeholders = (1..=card_ids.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            let mut stmt = c.prepare(&format!(
                "SELECT {PROGRESS_COLS} FROM card_progress WHERE card_id IN ({placeholders})"
            ))?;
            let p: Vec<&dyn rusqlite::ToSql> = card_ids
                .iter()
                .map(|id| id as &dyn rusqlite::ToSql)
                .collect();
            let rows = stmt.query_map(p.as_slice(), progress_from_row)?;
            rows.collect()
        })
    }

    pub fn level_histogram(&self) -> Result<Vec<(i64, i64)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT level, COUNT(*) FROM card_progress GROUP BY level ORDER BY level",
            )?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect()
        })
    }

    pub fn count_created_between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<i64> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM card_progress WHERE created_at >= ?1 AND created_at <= ?2",
                params![srs::fmt(from), srs::fmt(to)],
                |r| r.get(0),
            )
        })
    }

    pub fn count_reviewed_between(&self, from: DateTime<Utc>, to: DateTime<Utc>) -> Result<i64> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM card_progress WHERE last_reviewed >= ?1 AND last_reviewed <= ?2",
                params![srs::fmt(from), srs::fmt(to)],
                |r| r.get(0),
            )
        })
    }

    // ---- review sessions (24h anti-repeat marks for practice modes) ----

    pub fn insert_review_session(
        &self,
        card_id: &str,
        is_correct: bool,
        reviewed_at: DateTime<Utc>,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO review_sessions (id, card_id, is_correct, reviewed_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    card_id,
                    is_correct as i64,
                    srs::fmt(reviewed_at)
                ],
            )
        })?;
        Ok(())
    }

    /// Card ids seen in any practice mode since `since`.
    pub fn recently_reviewed_ids(
        &self,
        since: DateTime<Utc>,
    ) -> Result<std::collections::HashSet<String>> {
        self.with(|c| {
            let mut stmt =
                c.prepare("SELECT DISTINCT card_id FROM review_sessions WHERE reviewed_at > ?1")?;
            let rows = stmt.query_map(params![srs::fmt(since)], |r| r.get::<_, String>(0))?;
            rows.collect()
        })
    }

    /// Mark `is_correct` on this card's marks since `since`; returns rows touched.
    pub fn confirm_review_sessions(
        &self,
        card_id: &str,
        since: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<usize> {
        Ok(self.with(|c| {
            c.execute(
                "UPDATE review_sessions SET is_correct = 1, reviewed_at = ?3
                 WHERE card_id = ?1 AND reviewed_at > ?2",
                params![card_id, srs::fmt(since), srs::fmt(now)],
            )
        })?)
    }

    /// Delete recent marks — for one card, for a lesson's cards, or globally.
    pub fn delete_recent_review_sessions(
        &self,
        since: DateTime<Utc>,
        card_id: Option<&str>,
        lesson_id: Option<&str>,
    ) -> Result<usize> {
        Ok(self.with(|c| match (card_id, lesson_id) {
            (Some(id), _) => c.execute(
                "DELETE FROM review_sessions WHERE card_id = ?1 AND reviewed_at > ?2",
                params![id, srs::fmt(since)],
            ),
            (None, Some(lesson)) => c.execute(
                "DELETE FROM review_sessions WHERE reviewed_at > ?1
                 AND card_id IN (SELECT id FROM cards WHERE lesson_id = ?2)",
                params![srs::fmt(since), lesson],
            ),
            (None, None) => c.execute(
                "DELETE FROM review_sessions WHERE reviewed_at > ?1",
                params![srs::fmt(since)],
            ),
        })?)
    }

    // ---- study logs ----

    pub fn insert_study_log(
        &self,
        duration_seconds: i64,
        new_words_learned: i64,
        cards_reviewed: i64,
        game_score: Option<i64>,
        xp_earned: i64,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.with(|c| {
            c.execute(
                "INSERT INTO study_logs (id, created_at, duration_seconds, new_words_learned, cards_reviewed, game_score, xp_earned)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    srs::fmt(Utc::now()),
                    duration_seconds,
                    new_words_learned,
                    cards_reviewed,
                    game_score,
                    xp_earned
                ],
            )
        })?;
        Ok(id)
    }
}
