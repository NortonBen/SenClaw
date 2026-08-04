use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// SQLite store for the YouTube app.
///
/// - `kv`      — small key→JSON store; holds the captured auth snapshot (whether a
///               `SAPISID` cookie is present, the page's InnerTube context, when it
///               was last seen) pushed by the extension. We NEVER persist raw
///               cookies here — only presence flags + non-secret context.
/// - `drafts`  — draft-first pipeline for every WRITE action (comment/post). A draft
///               must be explicitly approved before it can be sent (human-in-the-loop).
/// - `activity`— an append-only audit log of what the app did.
pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS kv (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS drafts (
  id         TEXT PRIMARY KEY,
  kind       TEXT NOT NULL,              -- 'comment' | 'reply' | 'community_post'
  target     TEXT NOT NULL DEFAULT '',   -- video_id / comment_id / channel_id
  body       TEXT NOT NULL,
  status     TEXT NOT NULL DEFAULT 'draft', -- draft | approved | sent | failed
  result     TEXT,                       -- JSON of the send outcome
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_drafts_status ON drafts(status);
CREATE TABLE IF NOT EXISTS activity (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  action     TEXT NOT NULL,
  detail     TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);

-- Cache of comments pulled via InnerTube (analytics + a pull-feed can't run off a
-- live-only fetch). `tokens_json` holds volatile per-session action tokens (heart/
-- like/…) captured at sync time, to be used soon after by youtube_comment_action.
CREATE TABLE IF NOT EXISTS comments (
  id             TEXT PRIMARY KEY,        -- YouTube commentId
  video_id       TEXT NOT NULL,
  parent_id      TEXT,                    -- set for replies
  author         TEXT NOT NULL DEFAULT '',
  author_channel TEXT,
  text           TEXT NOT NULL DEFAULT '',
  like_count     INTEGER,
  reply_count    INTEGER,
  published      TEXT,                    -- relative text, e.g. "2 days ago"
  reply_params   TEXT,
  tokens_json    TEXT,
  fetched_at     INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_comments_video ON comments(video_id);

-- Per-comment analysis filled in P7 (sentiment/intent/topic/spam via the LLM).
CREATE TABLE IF NOT EXISTS comment_analysis (
  comment_id      TEXT PRIMARY KEY,
  sentiment       TEXT,                   -- pos | neu | neg
  sentiment_score REAL,
  intent          TEXT,                   -- question | complaint | praise | ...
  topics_json     TEXT,
  lang            TEXT,
  is_spam         INTEGER,
  toxicity        REAL,
  model           TEXT,
  analyzed_at     INTEGER
);
"#;

/// A draft row.
#[derive(Serialize, Clone)]
pub struct Draft {
    pub id: String,
    pub kind: String,
    pub target: String,
    pub body: String,
    pub status: String,
    pub result: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A comment to insert into the cache (owned).
pub struct CommentIn {
    pub id: String,
    pub video_id: String,
    pub parent_id: Option<String>,
    pub author: String,
    pub author_channel: Option<String>,
    pub text: String,
    pub like_count: Option<i64>,
    pub reply_count: Option<i64>,
    pub published: Option<String>,
    pub reply_params: Option<String>,
    pub tokens_json: Option<String>,
}

/// A cached comment row (with any analysis joined in).
#[derive(Serialize)]
pub struct CommentRow {
    pub id: String,
    pub video_id: String,
    pub parent_id: Option<String>,
    pub author: String,
    pub author_channel: Option<String>,
    pub text: String,
    pub like_count: Option<i64>,
    pub reply_count: Option<i64>,
    pub published: Option<String>,
    pub reply_params: Option<String>,
    pub fetched_at: i64,
    pub sentiment: Option<String>,
    pub intent: Option<String>,
}

/// One activity-log row.
#[derive(Serialize)]
pub struct Activity {
    pub id: i64,
    pub action: String,
    pub detail: String,
    pub created_at: i64,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }

    // ---- kv / auth snapshot ----

    pub fn set_kv(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        let s = value.to_string();
        self.with(|c| {
            c.execute(
                "INSERT INTO kv(key, value) VALUES(?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, s],
            )?;
            Ok(())
        })
    }

    pub fn del_kv(&self, key: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM kv WHERE key=?1", params![key])?;
            Ok(())
        })
    }

    pub fn get_kv(&self, key: &str) -> Result<Option<serde_json::Value>> {
        self.with(|c| {
            let s: Option<String> = c
                .query_row("SELECT value FROM kv WHERE key=?1", params![key], |r| {
                    r.get(0)
                })
                .optional()?;
            Ok(s.and_then(|s| serde_json::from_str(&s).ok()))
        })
    }

    /// The last auth snapshot the extension pushed (or a default "unknown" shape).
    pub fn auth_snapshot(&self) -> serde_json::Value {
        self.get_kv("auth")
            .ok()
            .flatten()
            .unwrap_or_else(|| serde_json::json!({ "hasSapisid": false, "loggedIn": false }))
    }

    // ---- drafts ----

    pub fn create_draft(&self, kind: &str, target: &str, body: &str, now: i64) -> Result<String> {
        let id = new_id();
        let id2 = id.clone();
        self.with(|c| {
            c.execute(
                "INSERT INTO drafts(id, kind, target, body, status, created_at, updated_at)
                 VALUES(?1, ?2, ?3, ?4, 'draft', ?5, ?5)",
                params![id2, kind, target, body, now],
            )?;
            Ok(())
        })?;
        Ok(id)
    }

    pub fn list_drafts(&self, status: Option<&str>) -> Result<Vec<Draft>> {
        self.with(|c| {
            let (sql, filter) = match status {
                Some(st) => (
                    "SELECT id, kind, target, body, status, result, created_at, updated_at
                     FROM drafts WHERE status=?1 ORDER BY updated_at DESC",
                    Some(st.to_string()),
                ),
                None => (
                    "SELECT id, kind, target, body, status, result, created_at, updated_at
                     FROM drafts ORDER BY updated_at DESC",
                    None,
                ),
            };
            let mut stmt = c.prepare(sql)?;
            let map = |r: &rusqlite::Row| {
                Ok(Draft {
                    id: r.get(0)?,
                    kind: r.get(1)?,
                    target: r.get(2)?,
                    body: r.get(3)?,
                    status: r.get(4)?,
                    result: r.get(5)?,
                    created_at: r.get(6)?,
                    updated_at: r.get(7)?,
                })
            };
            let rows: Vec<Draft> = match filter {
                Some(st) => stmt
                    .query_map(params![st], map)?
                    .filter_map(|r| r.ok())
                    .collect(),
                None => stmt.query_map([], map)?.filter_map(|r| r.ok()).collect(),
            };
            Ok(rows)
        })
    }

    pub fn get_draft(&self, id: &str) -> Result<Option<Draft>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT id, kind, target, body, status, result, created_at, updated_at
                     FROM drafts WHERE id=?1",
                    params![id],
                    |r| {
                        Ok(Draft {
                            id: r.get(0)?,
                            kind: r.get(1)?,
                            target: r.get(2)?,
                            body: r.get(3)?,
                            status: r.get(4)?,
                            result: r.get(5)?,
                            created_at: r.get(6)?,
                            updated_at: r.get(7)?,
                        })
                    },
                )
                .optional()?;
            Ok(row)
        })
    }

    pub fn set_draft_status(
        &self,
        id: &str,
        status: &str,
        result: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE drafts SET status=?2, result=COALESCE(?3, result), updated_at=?4 WHERE id=?1",
                params![id, status, result, now],
            )?;
            if n == 0 {
                return Err(anyhow!("draft {id} not found"));
            }
            Ok(())
        })
    }

    // ---- activity ----

    pub fn log(&self, action: &str, detail: &str, now: i64) {
        let _ = self.with(|c| {
            c.execute(
                "INSERT INTO activity(action, detail, created_at) VALUES(?1, ?2, ?3)",
                params![action, detail, now],
            )?;
            Ok(())
        });
    }

    // ---- comment cache ----

    /// Insert or update a cached comment. Returns true when it was NEW (first seen),
    /// so a sync can report how many fresh comments arrived.
    pub fn upsert_comment(&self, c: &CommentIn, now: i64) -> Result<bool> {
        self.with(|conn| {
            let existed: bool = conn
                .query_row("SELECT 1 FROM comments WHERE id=?1", params![c.id], |_| Ok(()))
                .optional()?
                .is_some();
            conn.execute(
                "INSERT INTO comments(id, video_id, parent_id, author, author_channel, text,
                                      like_count, reply_count, published, reply_params, tokens_json, fetched_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(id) DO UPDATE SET
                   text=excluded.text, like_count=excluded.like_count,
                   reply_count=excluded.reply_count, published=excluded.published,
                   reply_params=COALESCE(excluded.reply_params, comments.reply_params),
                   tokens_json=COALESCE(excluded.tokens_json, comments.tokens_json),
                   fetched_at=excluded.fetched_at",
                params![
                    c.id, c.video_id, c.parent_id, c.author, c.author_channel, c.text,
                    c.like_count, c.reply_count, c.published, c.reply_params, c.tokens_json, now
                ],
            )?;
            Ok(!existed)
        })
    }

    pub fn list_comments(&self, video_id: &str, limit: i64) -> Result<Vec<CommentRow>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT c.id, c.video_id, c.parent_id, c.author, c.author_channel, c.text,
                        c.like_count, c.reply_count, c.published, c.reply_params, c.fetched_at,
                        a.sentiment, a.intent
                 FROM comments c LEFT JOIN comment_analysis a ON a.comment_id = c.id
                 WHERE c.video_id=?1 ORDER BY c.fetched_at DESC, c.id LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![video_id, limit], |r| {
                    Ok(CommentRow {
                        id: r.get(0)?,
                        video_id: r.get(1)?,
                        parent_id: r.get(2)?,
                        author: r.get(3)?,
                        author_channel: r.get(4)?,
                        text: r.get(5)?,
                        like_count: r.get(6)?,
                        reply_count: r.get(7)?,
                        published: r.get(8)?,
                        reply_params: r.get(9)?,
                        fetched_at: r.get(10)?,
                        sentiment: r.get(11)?,
                        intent: r.get(12)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Comments not yet analysed (P7). Returns `(id, text)` pairs.
    pub fn unanalyzed_comments(&self, limit: i64) -> Result<Vec<(String, String)>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT c.id, c.text FROM comments c
                 LEFT JOIN comment_analysis a ON a.comment_id = c.id
                 WHERE a.comment_id IS NULL AND c.text <> '' LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![limit], |r| Ok((r.get(0)?, r.get(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Persist one comment's analysis (P7).
    #[allow(clippy::too_many_arguments)]
    pub fn save_analysis(
        &self,
        comment_id: &str,
        sentiment: &str,
        score: f64,
        intent: &str,
        topics_json: &str,
        lang: &str,
        is_spam: bool,
        toxicity: f64,
        model: &str,
        now: i64,
    ) -> Result<()> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO comment_analysis(comment_id, sentiment, sentiment_score, intent,
                     topics_json, lang, is_spam, toxicity, model, analyzed_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(comment_id) DO UPDATE SET
                   sentiment=excluded.sentiment, sentiment_score=excluded.sentiment_score,
                   intent=excluded.intent, topics_json=excluded.topics_json, lang=excluded.lang,
                   is_spam=excluded.is_spam, toxicity=excluded.toxicity, model=excluded.model,
                   analyzed_at=excluded.analyzed_at",
                params![
                    comment_id,
                    sentiment,
                    score,
                    intent,
                    topics_json,
                    lang,
                    is_spam as i64,
                    toxicity,
                    model,
                    now
                ],
            )?;
            Ok(())
        })
    }

    /// Aggregated stats for a video's cached+analysed comments (P7 dashboard).
    pub fn comment_stats(&self, video_id: &str) -> Result<serde_json::Value> {
        self.with(|conn| {
            let group = |sql: &str| -> Result<serde_json::Value> {
                let mut stmt = conn.prepare(sql)?;
                let rows: Vec<(String, i64)> = stmt
                    .query_map(params![video_id], |r| {
                        Ok((r.get::<_, Option<String>>(0)?.unwrap_or_else(|| "unknown".into()), r.get(1)?))
                    })?
                    .filter_map(|r| r.ok())
                    .collect();
                Ok(serde_json::Value::Object(rows.into_iter().map(|(k, v)| (k, serde_json::json!(v))).collect()))
            };

            let total: i64 = conn.query_row(
                "SELECT COUNT(*) FROM comments WHERE video_id=?1", params![video_id], |r| r.get(0))?;
            let analyzed: i64 = conn.query_row(
                "SELECT COUNT(*) FROM comments c JOIN comment_analysis a ON a.comment_id=c.id
                 WHERE c.video_id=?1", params![video_id], |r| r.get(0))?;
            let spam: i64 = conn.query_row(
                "SELECT COUNT(*) FROM comments c JOIN comment_analysis a ON a.comment_id=c.id
                 WHERE c.video_id=?1 AND a.is_spam=1", params![video_id], |r| r.get(0))?;
            let avg_sentiment: Option<f64> = conn.query_row(
                "SELECT AVG(a.sentiment_score) FROM comments c JOIN comment_analysis a ON a.comment_id=c.id
                 WHERE c.video_id=?1", params![video_id], |r| r.get(0))?;

            let sentiment = group(
                "SELECT a.sentiment, COUNT(*) FROM comments c JOIN comment_analysis a ON a.comment_id=c.id
                 WHERE c.video_id=?1 GROUP BY a.sentiment")?;
            let intent = group(
                "SELECT a.intent, COUNT(*) FROM comments c JOIN comment_analysis a ON a.comment_id=c.id
                 WHERE c.video_id=?1 GROUP BY a.intent")?;
            let lang = group(
                "SELECT a.lang, COUNT(*) FROM comments c JOIN comment_analysis a ON a.comment_id=c.id
                 WHERE c.video_id=?1 GROUP BY a.lang")?;
            let top_authors = group(
                "SELECT author, COUNT(*) FROM comments WHERE video_id=?1 AND author<>''
                 GROUP BY author ORDER BY COUNT(*) DESC LIMIT 10")?;

            Ok(serde_json::json!({
                "videoId": video_id,
                "total": total,
                "analyzed": analyzed,
                "spam": spam,
                "avgSentiment": avg_sentiment,
                "sentiment": sentiment,
                "intent": intent,
                "lang": lang,
                "topAuthors": top_authors,
            }))
        })
    }

    /// CRM pull-feed cursor page (P9). `since` is a rowid; returns rows with rowid > since.
    pub fn feed_since(&self, since: i64, limit: i64) -> Result<Vec<serde_json::Value>> {
        self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT rowid, id, author, text FROM comments
                 WHERE rowid > ?1 ORDER BY rowid LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![since, limit], |r| {
                    let seq: i64 = r.get(0)?;
                    let id: String = r.get(1)?;
                    let author: String = r.get(2)?;
                    let text: String = r.get(3)?;
                    Ok(serde_json::json!({
                        "id": seq,
                        "platform": "youtube",
                        "external_id": id,
                        "sender": author,
                        "text": text,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// The `createReplyParams` token cached for a comment (P9 reply / P8).
    pub fn reply_params_of(&self, comment_id: &str) -> Result<Option<String>> {
        self.with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT reply_params FROM comments WHERE id=?1",
                    params![comment_id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten())
        })
    }

    /// The cached action tokens (heart/like/…) for a comment (P8).
    pub fn tokens_of(&self, comment_id: &str) -> Result<Option<serde_json::Value>> {
        self.with(|conn| {
            let s: Option<String> = conn
                .query_row(
                    "SELECT tokens_json FROM comments WHERE id=?1",
                    params![comment_id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            Ok(s.and_then(|s| serde_json::from_str(&s).ok()))
        })
    }

    /// Cached comments whose text contains ANY of `keywords` (case-insensitive) —
    /// the data source for keyword alerts (P10). `video_id` None = across all videos.
    pub fn search_comments(
        &self,
        video_id: Option<&str>,
        keywords: &[String],
        limit: i64,
    ) -> Result<Vec<CommentRow>> {
        if keywords.is_empty() {
            return Ok(vec![]);
        }
        self.with(|conn| {
            let mut sql = String::from(
                "SELECT c.id, c.video_id, c.parent_id, c.author, c.author_channel, c.text,
                        c.like_count, c.reply_count, c.published, c.reply_params, c.fetched_at,
                        a.sentiment, a.intent
                 FROM comments c LEFT JOIN comment_analysis a ON a.comment_id=c.id WHERE ",
            );
            let mut binds: Vec<String> = Vec::new();
            if let Some(v) = video_id {
                sql.push_str("c.video_id = ? AND ");
                binds.push(v.to_string());
            }
            let ors: Vec<String> = keywords
                .iter()
                .map(|_| "LOWER(c.text) LIKE ?".to_string())
                .collect();
            sql.push_str(&format!("({})", ors.join(" OR ")));
            for k in keywords {
                binds.push(format!("%{}%", k.to_lowercase()));
            }
            sql.push_str(" ORDER BY c.fetched_at DESC LIMIT ?");
            let mut stmt = conn.prepare(&sql)?;
            let params_dyn: Vec<&dyn rusqlite::ToSql> = binds
                .iter()
                .map(|b| b as &dyn rusqlite::ToSql)
                .chain(std::iter::once(&limit as &dyn rusqlite::ToSql))
                .collect();
            let rows = stmt
                .query_map(params_dyn.as_slice(), |r| {
                    Ok(CommentRow {
                        id: r.get(0)?,
                        video_id: r.get(1)?,
                        parent_id: r.get(2)?,
                        author: r.get(3)?,
                        author_channel: r.get(4)?,
                        text: r.get(5)?,
                        like_count: r.get(6)?,
                        reply_count: r.get(7)?,
                        published: r.get(8)?,
                        reply_params: r.get(9)?,
                        fetched_at: r.get(10)?,
                        sentiment: r.get(11)?,
                        intent: r.get(12)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Counts for a video's cached comments (for the sync summary / UI).
    pub fn comment_counts(&self, video_id: &str) -> Result<serde_json::Value> {
        self.with(|conn| {
            let total: i64 = conn.query_row(
                "SELECT COUNT(*) FROM comments WHERE video_id=?1",
                params![video_id],
                |r| r.get(0),
            )?;
            let replies: i64 = conn.query_row(
                "SELECT COUNT(*) FROM comments WHERE video_id=?1 AND parent_id IS NOT NULL",
                params![video_id],
                |r| r.get(0),
            )?;
            let last: Option<i64> = conn.query_row(
                "SELECT MAX(fetched_at) FROM comments WHERE video_id=?1",
                params![video_id],
                |r| r.get(0),
            )?;
            Ok(serde_json::json!({
                "videoId": video_id,
                "total": total,
                "topLevel": total - replies,
                "replies": replies,
                "lastFetched": last,
            }))
        })
    }

    pub fn recent_activity(&self, limit: i64) -> Result<Vec<Activity>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, action, detail, created_at FROM activity ORDER BY id DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![limit], |r| {
                    Ok(Activity {
                        id: r.get(0)?,
                        action: r.get(1)?,
                        detail: r.get(2)?,
                        created_at: r.get(3)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }
}

/// A short, collision-resistant opaque id (used for drafts and the extension-bridge
/// callback correlation). Monotonic counter mixed with the wall clock so ids stay
/// unique within a process without pulling in a uuid dependency.
static COUNTER: AtomicU64 = AtomicU64::new(0);
pub fn new_id() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{t:x}{n:x}")
}

/// Per-app data dir, e.g. `~/.senclaw/space-apps/youtube/`.
pub fn default_data_dir(app: &str) -> PathBuf {
    let base = std::env::var("SENCLAW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".senclaw")
        });
    base.join("space-apps").join(app)
}
