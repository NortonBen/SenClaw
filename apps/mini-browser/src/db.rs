//! SQLite store for the mini browser: visit `history`, `bookmarks`, the chat
//! transcript, and the record of every agent run.
//!
//! Chat used to live entirely in React state, so reloading the panel threw the
//! conversation away — and with it any record of what the agent had been asked
//! to do. Agent runs were not recorded at all: you saw the steps scroll past
//! once and then they were gone. Both are now rows, which is also what lets the
//! Act panel and the chat refer to the same run instead of being two unrelated
//! views of the same browser.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS history (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  url        TEXT NOT NULL,
  title      TEXT NOT NULL DEFAULT '',
  visited_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_history_time ON history(visited_at DESC);
CREATE TABLE IF NOT EXISTS bookmarks (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  url        TEXT NOT NULL UNIQUE,
  title      TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);

-- The conversation. `run_id` ties an assistant message to the agent run it
-- performed, which is the link between the Chat panel and the Act panel.
CREATE TABLE IF NOT EXISTS chat_messages (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  role       TEXT NOT NULL,
  content    TEXT NOT NULL,
  run_id     INTEGER,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chat_time ON chat_messages(id DESC);

-- One row per user request the agent worked on.
CREATE TABLE IF NOT EXISTS act_runs (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  goal        TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'running',
  plans_used  INTEGER NOT NULL DEFAULT 0,
  outcome     TEXT NOT NULL DEFAULT '',
  verified    INTEGER,
  source      TEXT NOT NULL DEFAULT 'act',
  started_at  INTEGER NOT NULL,
  finished_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_runs_time ON act_runs(id DESC);

-- Every step of every plan, so a finished run can be read back in full.
CREATE TABLE IF NOT EXISTS act_steps (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id     INTEGER NOT NULL,
  plan_no    INTEGER NOT NULL,
  step_no    INTEGER NOT NULL,
  kind       TEXT NOT NULL,
  detail     TEXT NOT NULL DEFAULT '',
  ok         INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_steps_run ON act_steps(run_id, id);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- What the agent has learned from runs that actually worked.
--
-- Keyed by host because that is what the lessons are actually about: "on this
-- site, Enter in the search box does not submit — click the button". A note
-- keyed to nothing would be advice about the whole web, which is either obvious
-- or wrong. `host = '*'` is the escape hatch for the rare general lesson.
--
-- `wins`/`losses` are what stop this from silently rotting. A note is credited
-- when a run that was shown it succeeded and debited when one failed, so advice
-- that stops being true on a redesigned site falls out of retrieval by itself
-- instead of misleading every future plan.
CREATE TABLE IF NOT EXISTS knowledge (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  host       TEXT NOT NULL,
  note       TEXT NOT NULL,
  kind       TEXT NOT NULL DEFAULT 'recipe',
  uses       INTEGER NOT NULL DEFAULT 0,
  wins       INTEGER NOT NULL DEFAULT 0,
  losses     INTEGER NOT NULL DEFAULT 0,
  run_id     INTEGER,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_knowledge_uniq ON knowledge(host, note);
CREATE INDEX IF NOT EXISTS idx_knowledge_host ON knowledge(host);
"#;

#[derive(Serialize)]
pub struct Row {
    pub id: i64,
    pub url: String,
    pub title: String,
    pub at: i64,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Db {
            conn: Mutex::new(conn),
        })
    }

    pub fn add_history(&self, url: &str, title: &str, at: i64) -> Result<()> {
        if url.is_empty() || url == "about:blank" {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        // Skip if it's the same as the most recent entry.
        let last: Option<String> = conn
            .query_row(
                "SELECT url FROM history ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();
        if last.as_deref() == Some(url) {
            conn.execute("UPDATE history SET title=?1, visited_at=?2 WHERE url=?1 AND id=(SELECT MAX(id) FROM history)", params![title, at]).ok();
            return Ok(());
        }
        conn.execute(
            "INSERT INTO history (url, title, visited_at) VALUES (?1, ?2, ?3)",
            params![url, title, at],
        )?;
        Ok(())
    }

    pub fn recent_history(&self, limit: i64) -> Result<Vec<Row>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, url, title, visited_at FROM history ORDER BY id DESC LIMIT ?1")?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(Row {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    title: r.get(2)?,
                    at: r.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn add_bookmark(&self, url: &str, title: &str, at: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO bookmarks (url, title, created_at) VALUES (?1, ?2, ?3)",
            params![url, title, at],
        )?;
        Ok(())
    }

    pub fn remove_bookmark(&self, url: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM bookmarks WHERE url=?1", params![url])?;
        Ok(())
    }

    pub fn list_bookmarks(&self) -> Result<Vec<Row>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id, url, title, created_at FROM bookmarks ORDER BY id DESC")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Row {
                    id: r.get(0)?,
                    url: r.get(1)?,
                    title: r.get(2)?,
                    at: r.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }
}

#[derive(Serialize, Clone)]
pub struct ChatRow {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub run_id: Option<i64>,
    pub at: i64,
}

#[derive(Serialize, Clone)]
pub struct RunRow {
    pub id: i64,
    pub goal: String,
    pub status: String,
    pub plans_used: i64,
    pub outcome: String,
    pub verified: Option<bool>,
    pub source: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Serialize, Clone)]
pub struct StepRow {
    pub id: i64,
    pub plan_no: i64,
    pub step_no: i64,
    pub kind: String,
    pub detail: String,
    pub ok: bool,
    pub at: i64,
}

impl Db {
    // ---------------------------------------------------------------- chat ---

    pub fn add_chat(&self, role: &str, content: &str, run_id: Option<i64>, at: i64) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO chat_messages (role, content, run_id, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![role, content, run_id, at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// The most recent `limit` messages, oldest first — the order they are read
    /// in, and the order the model needs them in.
    pub fn chat_history(&self, limit: i64) -> Result<Vec<ChatRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, role, content, run_id, created_at FROM chat_messages \
             ORDER BY id DESC LIMIT ?1",
        )?;
        let mut rows: Vec<ChatRow> = stmt
            .query_map([limit], |r| {
                Ok(ChatRow {
                    id: r.get(0)?,
                    role: r.get(1)?,
                    content: r.get(2)?,
                    run_id: r.get(3)?,
                    at: r.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        rows.reverse();
        Ok(rows)
    }

    pub fn clear_chat(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM chat_messages", [])?;
        Ok(())
    }

    // ----------------------------------------------------------------- act ---

    pub fn start_run(&self, goal: &str, source: &str, at: i64) -> Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO act_runs (goal, source, started_at) VALUES (?1, ?2, ?3)",
            params![goal, source, at],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn finish_run(
        &self,
        run_id: i64,
        status: &str,
        plans_used: i64,
        outcome: &str,
        verified: Option<bool>,
        at: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE act_runs SET status=?2, plans_used=?3, outcome=?4, verified=?5, finished_at=?6 \
             WHERE id=?1",
            params![run_id, status, plans_used, outcome, verified, at],
        )?;
        Ok(())
    }

    pub fn add_step(
        &self,
        run_id: i64,
        plan_no: i64,
        step_no: i64,
        kind: &str,
        detail: &str,
        ok: bool,
        at: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO act_steps (run_id, plan_no, step_no, kind, detail, ok, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![run_id, plan_no, step_no, kind, detail, ok, at],
        )?;
        Ok(())
    }

    pub fn recent_runs(&self, limit: i64) -> Result<Vec<RunRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, goal, status, plans_used, outcome, verified, source, started_at, finished_at \
             FROM act_runs ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(RunRow {
                    id: r.get(0)?,
                    goal: r.get(1)?,
                    status: r.get(2)?,
                    plans_used: r.get(3)?,
                    outcome: r.get(4)?,
                    verified: r.get::<_, Option<i64>>(5)?.map(|v| v != 0),
                    source: r.get(6)?,
                    started_at: r.get(7)?,
                    finished_at: r.get(8)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn run_steps(&self, run_id: i64) -> Result<Vec<StepRow>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, plan_no, step_no, kind, detail, ok, created_at FROM act_steps \
             WHERE run_id=?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([run_id], |r| {
                Ok(StepRow {
                    id: r.get(0)?,
                    plan_no: r.get(1)?,
                    step_no: r.get(2)?,
                    kind: r.get(3)?,
                    detail: r.get(4)?,
                    ok: r.get::<_, i64>(5)? != 0,
                    at: r.get(6)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    // ------------------------------------------------------------ settings ---

    pub fn setting(&self, key: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT value FROM settings WHERE key=?1",
            params![key],
            |r| r.get(0),
        )
        .ok()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// How many plans one user request may consume before the agent gives up.
    ///
    /// A bound is not optional: without one, a goal the page simply cannot
    /// satisfy turns into an unbounded spend of model calls and clicks on the
    /// user's real logged-in browser.
    pub fn max_plans(&self) -> usize {
        self.setting("max_plans")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_PLANS)
            .clamp(1, HARD_MAX_PLANS)
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct Lesson {
    pub id: i64,
    pub host: String,
    pub note: String,
    pub kind: String,
    pub uses: i64,
    pub wins: i64,
    pub losses: i64,
    pub run_id: Option<i64>,
    pub at: i64,
}

impl Db {
    // ------------------------------------------------------------ knowledge ---

    /// Record a lesson. Re-learning the same thing refreshes it rather than
    /// filling the table with near-duplicates.
    pub fn add_lesson(
        &self,
        host: &str,
        note: &str,
        kind: &str,
        run_id: Option<i64>,
        at: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO knowledge (host, note, kind, run_id, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?5) \
             ON CONFLICT(host, note) DO UPDATE SET updated_at=?5, kind=excluded.kind",
            params![host, note, kind, run_id, at],
        )?;
        Ok(())
    }

    /// The lessons worth showing a planner about to work on `host`.
    ///
    /// Ordered by how well they have actually done, not by how recent they are:
    /// a note credited by three successful runs should outrank one written five
    /// minutes ago that has never been tested. Notes that have lost more often
    /// than they have won are withheld entirely — that is the whole staleness
    /// mechanism, and it needs no separate expiry.
    pub fn lessons_for(&self, host: &str, limit: i64) -> Result<Vec<Lesson>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, host, note, kind, uses, wins, losses, run_id, updated_at \
             FROM knowledge \
             WHERE (host = ?1 OR host = '*') AND (losses <= wins OR losses < 2) \
             ORDER BY (wins - losses) DESC, updated_at DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![host, limit], |r| {
                Ok(Lesson {
                    id: r.get(0)?,
                    host: r.get(1)?,
                    note: r.get(2)?,
                    kind: r.get(3)?,
                    uses: r.get(4)?,
                    wins: r.get(5)?,
                    losses: r.get(6)?,
                    run_id: r.get(7)?,
                    at: r.get(8)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn all_lessons(&self, limit: i64) -> Result<Vec<Lesson>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, host, note, kind, uses, wins, losses, run_id, updated_at \
             FROM knowledge ORDER BY (wins - losses) DESC, updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map([limit], |r| {
                Ok(Lesson {
                    id: r.get(0)?,
                    host: r.get(1)?,
                    note: r.get(2)?,
                    kind: r.get(3)?,
                    uses: r.get(4)?,
                    wins: r.get(5)?,
                    losses: r.get(6)?,
                    run_id: r.get(7)?,
                    at: r.get(8)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Credit or debit the notes a run was shown, once it is known how it went.
    pub fn score_lessons(&self, ids: &[i64], won: bool, at: i64) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        for id in ids {
            conn.execute(
                if won {
                    "UPDATE knowledge SET uses=uses+1, wins=wins+1, updated_at=?2 WHERE id=?1"
                } else {
                    "UPDATE knowledge SET uses=uses+1, losses=losses+1, updated_at=?2 WHERE id=?1"
                },
                params![id, at],
            )?;
        }
        Ok(())
    }

    pub fn forget_lesson(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM knowledge WHERE id=?1", params![id])?;
        Ok(())
    }

    pub fn learning_enabled(&self) -> bool {
        self.setting("learning").map(|v| v != "0").unwrap_or(true)
    }
}

pub const DEFAULT_MAX_PLANS: usize = 10;
/// Even a deliberate setting cannot exceed this.
pub const HARD_MAX_PLANS: usize = 10;

/// Per-app data dir, e.g. `~/.senclaw/space-apps/mini-browser/`.
pub fn default_data_dir(app: &str) -> PathBuf {
    let base = std::env::var("SENCLAW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".senclaw")
        });
    base.join("space-apps").join(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        Db {
            conn: Mutex::new(conn),
        }
    }

    /// Chat used to live in React state and die on reload. It has to come back
    /// in the order it was written, or the model reads the conversation
    /// backwards.
    #[test]
    fn chat_survives_and_reads_oldest_first() {
        let d = db();
        d.add_chat("user", "giá vàng hôm nay?", None, 1).unwrap();
        d.add_chat("assistant", "137,7 triệu", None, 2).unwrap();
        d.add_chat("user", "mở 4 trang", None, 3).unwrap();

        let h = d.chat_history(100).unwrap();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].content, "giá vàng hôm nay?");
        assert_eq!(h[2].content, "mở 4 trang");
    }

    /// Even when the tail is clipped, what comes back must be the *newest*
    /// messages, still oldest-first.
    #[test]
    fn a_clipped_history_keeps_the_newest_in_order() {
        let d = db();
        for i in 0..10 {
            d.add_chat("user", &format!("m{i}"), None, i).unwrap();
        }
        let h = d.chat_history(3).unwrap();
        let texts: Vec<&str> = h.iter().map(|r| r.content.as_str()).collect();
        assert_eq!(texts, vec!["m7", "m8", "m9"]);
    }

    /// The link between the two panels: an assistant message carries the id of
    /// the run that produced it.
    #[test]
    fn a_run_links_back_to_the_message_that_started_it() {
        let d = db();
        let run = d.start_run("mở 4 trang", "chat", 10).unwrap();
        d.add_step(run, 1, 1, "step", "opened article 1", true, 11)
            .unwrap();
        d.add_step(run, 1, 2, "action", "click e5 — ok", true, 12)
            .unwrap();
        d.finish_run(run, "done", 1, "all four opened", Some(true), 20)
            .unwrap();
        let msg = d.add_chat("assistant", "Xong.", Some(run), 21).unwrap();
        assert!(msg > 0);

        let runs = d.recent_runs(10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].verified, Some(true));
        assert_eq!(runs[0].plans_used, 1);
        assert_eq!(runs[0].source, "chat");

        let steps = d.run_steps(run).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].kind, "step");

        let linked = d.chat_history(10).unwrap();
        assert_eq!(linked[0].run_id, Some(run));
    }

    #[test]
    fn an_unfinished_run_is_recorded_as_unfinished() {
        let d = db();
        let run = d.start_run("g", "act", 1).unwrap();
        d.finish_run(run, "unfinished", 10, "ran out of plans", Some(false), 2)
            .unwrap();
        let r = &d.recent_runs(1).unwrap()[0];
        assert_eq!(r.status, "unfinished");
        assert_eq!(r.verified, Some(false));
    }

    /// The replan budget is what stops a hopeless goal spending forever. It is
    /// configurable, defaulted, and cannot be raised past the hard cap.
    #[test]
    fn max_plans_is_bounded_in_both_directions() {
        let d = db();
        assert_eq!(d.max_plans(), DEFAULT_MAX_PLANS);

        d.set_setting("max_plans", "3").unwrap();
        assert_eq!(d.max_plans(), 3);

        d.set_setting("max_plans", "9999").unwrap();
        assert_eq!(
            d.max_plans(),
            HARD_MAX_PLANS,
            "no setting may exceed the hard cap"
        );

        d.set_setting("max_plans", "0").unwrap();
        assert_eq!(d.max_plans(), 1, "zero plans would do nothing at all");

        d.set_setting("max_plans", "nonsense").unwrap();
        assert_eq!(
            d.max_plans(),
            DEFAULT_MAX_PLANS,
            "a bad value falls back to the default"
        );
    }

    /// Lessons are filed per host, and only the relevant ones come back.
    #[test]
    fn lessons_are_recalled_by_host() {
        let d = db();
        d.add_lesson("www.google.com", "Enter does not submit the search box — click the button", "gotcha", Some(1), 10).unwrap();
        d.add_lesson("vnexpress.net", "articles are under /kinh-doanh", "recipe", Some(2), 11).unwrap();
        d.add_lesson("*", "check the page actually changed before saying done", "gotcha", None, 12).unwrap();

        let g: Vec<String> = d.lessons_for("www.google.com", 10).unwrap()
            .into_iter().map(|l| l.note).collect();
        assert_eq!(g.len(), 2, "the google note plus the general one: {g:?}");
        assert!(g.iter().any(|n| n.contains("Enter does not submit")));
        assert!(g.iter().any(|n| n.contains("page actually changed")));
        assert!(!g.iter().any(|n| n.contains("kinh-doanh")), "another site's note must not leak in");
    }

    /// Re-learning the same thing must not fill the table with duplicates.
    #[test]
    fn learning_the_same_lesson_twice_updates_it() {
        let d = db();
        d.add_lesson("x.com", "click the Search button", "recipe", Some(1), 10).unwrap();
        d.add_lesson("x.com", "click the Search button", "gotcha", Some(2), 20).unwrap();
        let all = d.lessons_for("x.com", 10).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].kind, "gotcha", "the newer classification wins");
        assert_eq!(all[0].at, 20);
    }

    /// The staleness mechanism: a note that keeps being present when runs fail
    /// stops being offered, without anyone having to expire it by hand.
    #[test]
    fn advice_that_keeps_losing_is_withheld() {
        let d = db();
        d.add_lesson("x.com", "use the old checkout flow", "recipe", Some(1), 10).unwrap();
        let id = d.lessons_for("x.com", 10).unwrap()[0].id;

        // Two failed runs that were shown it.
        d.score_lessons(&[id], false, 11).unwrap();
        assert_eq!(d.lessons_for("x.com", 10).unwrap().len(), 1, "one bad run is not proof");
        d.score_lessons(&[id], false, 12).unwrap();
        assert!(d.lessons_for("x.com", 10).unwrap().is_empty(), "advice that keeps losing must stop being given");

        // It can earn its way back if it starts working again.
        d.score_lessons(&[id], true, 13).unwrap();
        d.score_lessons(&[id], true, 14).unwrap();
        assert_eq!(d.lessons_for("x.com", 10).unwrap().len(), 1);
    }

    #[test]
    fn better_proven_advice_is_offered_first() {
        let d = db();
        d.add_lesson("x.com", "proven route", "recipe", None, 10).unwrap();
        d.add_lesson("x.com", "untested guess", "recipe", None, 99).unwrap();
        let proven = d.lessons_for("x.com", 10).unwrap().iter().find(|l| l.note == "proven route").unwrap().id;
        d.score_lessons(&[proven], true, 11).unwrap();
        d.score_lessons(&[proven], true, 12).unwrap();

        let order: Vec<String> = d.lessons_for("x.com", 10).unwrap().into_iter().map(|l| l.note).collect();
        assert_eq!(order[0], "proven route", "wins should outrank recency: {order:?}");
    }

    #[test]
    fn learning_can_be_switched_off() {
        let d = db();
        assert!(d.learning_enabled(), "on by default");
        d.set_setting("learning", "0").unwrap();
        assert!(!d.learning_enabled());
    }

    #[test]
    fn a_forgotten_lesson_stays_forgotten() {
        let d = db();
        d.add_lesson("x.com", "something wrong", "recipe", None, 1).unwrap();
        let id = d.lessons_for("x.com", 5).unwrap()[0].id;
        d.forget_lesson(id).unwrap();
        assert!(d.lessons_for("x.com", 5).unwrap().is_empty());
    }

    #[test]
    fn clearing_chat_leaves_run_history_alone() {
        let d = db();
        let run = d.start_run("g", "act", 1).unwrap();
        d.add_chat("user", "hi", Some(run), 1).unwrap();
        d.clear_chat().unwrap();
        assert!(d.chat_history(10).unwrap().is_empty());
        assert_eq!(
            d.recent_runs(10).unwrap().len(),
            1,
            "runs are a separate record"
        );
    }
}
