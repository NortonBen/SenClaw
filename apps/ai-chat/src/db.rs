//! SQLite store for the AI Chat app.
//!
//! Holds only app-local state — the LLM, memory and knowledge live in the
//! SenClaw daemon and are reached through the bridge (see `llm.rs` /
//! `senclaw.rs`). Tables:
//! - `bots`      — one row per chatbot (system prompt + model + per-bot
//!                 MCP/skill allowlist policy + knowledge scope).
//! - `channels`  — messaging channels bound to a bot (telegram/websocket/
//!                 zalo/facebook/tiktok) with their credentials + sync cursor.
//! - `sessions`  — one conversation per (bot, channel, external id), with a
//!                 human-handoff state machine (`bot`→`pending`→`with_operator`).
//! - `messages`  — the transcript of each session.
//! - `settings`  — k/v (feature toggles, metrics counters, default language).

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS bots (
  key             TEXT PRIMARY KEY,
  name            TEXT NOT NULL,
  system_prompt   TEXT NOT NULL DEFAULT '',
  greeting        TEXT NOT NULL DEFAULT '',
  model           TEXT NOT NULL DEFAULT '',
  knowledge_scope TEXT NOT NULL DEFAULT 'bot',
  allowed_mcp     TEXT NOT NULL DEFAULT '[]',
  allowed_skills  TEXT NOT NULL DEFAULT '[]',
  use_tools       INTEGER NOT NULL DEFAULT 1,
  use_knowledge   INTEGER NOT NULL DEFAULT 1,
  auto_ingest     INTEGER NOT NULL DEFAULT 0,
  auto_issue      INTEGER NOT NULL DEFAULT 1,
  enabled         INTEGER NOT NULL DEFAULT 1,
  created_at      INTEGER NOT NULL,
  sort            INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS channels (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  bot_key      TEXT NOT NULL,
  kind         TEXT NOT NULL,
  name         TEXT NOT NULL DEFAULT '',
  config       TEXT NOT NULL DEFAULT '{}',
  enabled      INTEGER NOT NULL DEFAULT 1,
  cursor       TEXT NOT NULL DEFAULT '',
  last_sync_at INTEGER,
  last_status  TEXT NOT NULL DEFAULT '',
  last_error   TEXT NOT NULL DEFAULT '',
  created_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_channels_bot ON channels(bot_key);
CREATE TABLE IF NOT EXISTS sessions (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  bot_key       TEXT NOT NULL,
  channel_kind  TEXT NOT NULL,
  channel_id    INTEGER NOT NULL DEFAULT 0,
  external_id   TEXT NOT NULL DEFAULT '',
  jid           TEXT NOT NULL DEFAULT '',
  customer_name TEXT NOT NULL DEFAULT '',
  handoff_state TEXT NOT NULL DEFAULT 'bot',
  context       TEXT NOT NULL DEFAULT '{}',
  last_activity INTEGER NOT NULL,
  created_at    INTEGER NOT NULL,
  UNIQUE(bot_key, channel_kind, external_id)
);
CREATE INDEX IF NOT EXISTS idx_sessions_bot ON sessions(bot_key);
CREATE TABLE IF NOT EXISTS messages (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id INTEGER NOT NULL,
  role       TEXT NOT NULL,
  content    TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS issues (
  id              INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id      INTEGER,
  bot_key         TEXT NOT NULL DEFAULT '',
  external_id     TEXT NOT NULL DEFAULT '',
  title           TEXT NOT NULL DEFAULT '',
  description     TEXT NOT NULL DEFAULT '',
  status          TEXT NOT NULL DEFAULT 'open',
  priority        TEXT NOT NULL DEFAULT 'medium',
  category        TEXT NOT NULL DEFAULT '',
  sentiment       TEXT NOT NULL DEFAULT '',
  ai_summary      TEXT NOT NULL DEFAULT '',
  tags            TEXT NOT NULL DEFAULT '[]',
  resolution_note TEXT NOT NULL DEFAULT '',
  assignee        TEXT NOT NULL DEFAULT '',
  created_at      INTEGER NOT NULL,
  updated_at      INTEGER NOT NULL,
  resolved_at     INTEGER
);
CREATE INDEX IF NOT EXISTS idx_issues_status ON issues(status);
CREATE INDEX IF NOT EXISTS idx_issues_bot ON issues(bot_key);
CREATE INDEX IF NOT EXISTS idx_issues_session ON issues(session_id);
CREATE TABLE IF NOT EXISTS issue_events (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  issue_id   INTEGER NOT NULL,
  kind       TEXT NOT NULL,
  field      TEXT NOT NULL DEFAULT '',
  old_val    TEXT NOT NULL DEFAULT '',
  new_val    TEXT NOT NULL DEFAULT '',
  note       TEXT NOT NULL DEFAULT '',
  actor      TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_issue_events_issue ON issue_events(issue_id);
"#;

/// Additive migrations applied to pre-existing DBs (errors ignored).
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE bots ADD COLUMN use_knowledge INTEGER NOT NULL DEFAULT 1",
    "ALTER TABLE bots ADD COLUMN auto_ingest INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE bots ADD COLUMN auto_issue INTEGER NOT NULL DEFAULT 1",
];

pub const HANDOFF_BOT: &str = "bot";
pub const HANDOFF_PENDING: &str = "pending";
#[allow(dead_code)]
pub const HANDOFF_OPERATOR: &str = "with_operator";

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn default_data_dir(app: &str) -> PathBuf {
    let base = std::env::var("SENCLAW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".senclaw")
        });
    base.join("space-apps").join(app)
}

/// Turn a display name into a stable, url-safe key.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in s.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        format!("bot-{}", now_ms() % 100000)
    } else {
        trimmed
    }
}

fn json_arr(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

// ---- row structs ----

#[derive(Serialize, Clone)]
pub struct Bot {
    pub key: String,
    pub name: String,
    pub system_prompt: String,
    pub greeting: String,
    pub model: String,
    pub knowledge_scope: String,
    pub allowed_mcp: Vec<String>,
    pub allowed_skills: Vec<String>,
    pub use_tools: bool,
    pub use_knowledge: bool,
    pub auto_ingest: bool,
    pub auto_issue: bool,
    pub enabled: bool,
    pub created_at: i64,
}

#[derive(Serialize, Clone)]
pub struct Channel {
    pub id: i64,
    pub bot_key: String,
    pub kind: String,
    pub name: String,
    /// Parsed credential/config blob. Secret fields are redacted by the API
    /// layer before this leaves the process.
    pub config: serde_json::Value,
    pub enabled: bool,
    pub cursor: String,
    pub last_sync_at: Option<i64>,
    pub last_status: String,
    pub last_error: String,
    pub created_at: i64,
}

#[derive(Serialize, Clone)]
pub struct Session {
    pub id: i64,
    pub bot_key: String,
    pub channel_kind: String,
    pub channel_id: i64,
    pub external_id: String,
    pub jid: String,
    pub customer_name: String,
    pub handoff_state: String,
    pub context: serde_json::Value,
    pub last_activity: i64,
    pub created_at: i64,
}

#[derive(Serialize, Clone)]
pub struct Message {
    pub id: i64,
    pub session_id: i64,
    pub role: String,
    pub content: String,
    pub created_at: i64,
}

#[derive(Serialize, Clone)]
pub struct Issue {
    pub id: i64,
    pub session_id: Option<i64>,
    pub bot_key: String,
    pub external_id: String,
    pub title: String,
    pub description: String,
    pub status: String,
    pub priority: String,
    pub category: String,
    pub sentiment: String,
    pub ai_summary: String,
    pub tags: Vec<String>,
    pub resolution_note: String,
    pub assignee: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub resolved_at: Option<i64>,
}

/// One field the caller may patch on an issue. `None` = leave unchanged.
#[derive(Default)]
pub struct IssuePatch {
    pub status: Option<String>,
    pub priority: Option<String>,
    pub category: Option<String>,
    pub assignee: Option<String>,
    pub resolution_note: Option<String>,
    pub title: Option<String>,
}

pub const ISSUE_STATUSES: [&str; 4] = ["open", "in_progress", "resolved", "closed"];
pub const ISSUE_PRIORITIES: [&str; 4] = ["low", "medium", "high", "urgent"];

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        for m in MIGRATIONS {
            let _ = conn.execute(m, []);
        }
        let db = Self { conn: Mutex::new(conn) };
        db.seed()?;
        Ok(db)
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        f(&conn)
    }

    /// Ship a friendly default support bot (with a web/WebSocket channel) so the
    /// app is usable the moment it starts, with zero configuration.
    fn seed(&self) -> Result<()> {
        self.with(|c| {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM bots", [], |r| r.get(0))?;
            if n == 0 {
                let now = now_ms();
                c.execute(
                    "INSERT INTO bots(key,name,system_prompt,greeting,knowledge_scope,use_tools,use_knowledge,created_at,sort)
                     VALUES('support','Trợ lý CSKH',?1,?2,'bot',1,1,?3,0)",
                    params![DEFAULT_SUPPORT_PROMPT, DEFAULT_GREETING, now],
                )?;
                c.execute(
                    "INSERT INTO channels(bot_key,kind,name,enabled,created_at)
                     VALUES('support','websocket','Web chat',1,?1)",
                    params![now],
                )?;
            }
            Ok(())
        })
    }

    // ---- bots ----

    fn row_to_bot(r: &rusqlite::Row) -> rusqlite::Result<Bot> {
        Ok(Bot {
            key: r.get("key")?,
            name: r.get("name")?,
            system_prompt: r.get("system_prompt")?,
            greeting: r.get("greeting")?,
            model: r.get("model")?,
            knowledge_scope: r.get("knowledge_scope")?,
            allowed_mcp: json_arr(&r.get::<_, String>("allowed_mcp")?),
            allowed_skills: json_arr(&r.get::<_, String>("allowed_skills")?),
            use_tools: r.get::<_, i64>("use_tools")? != 0,
            use_knowledge: r.get::<_, i64>("use_knowledge")? != 0,
            auto_ingest: r.get::<_, i64>("auto_ingest")? != 0,
            auto_issue: r.get::<_, i64>("auto_issue")? != 0,
            enabled: r.get::<_, i64>("enabled")? != 0,
            created_at: r.get("created_at")?,
        })
    }

    pub fn list_bots(&self) -> Result<Vec<Bot>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM bots ORDER BY sort, created_at")?;
            let rows = stmt
                .query_map([], Self::row_to_bot)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn get_bot(&self, key: &str) -> Result<Option<Bot>> {
        self.with(|c| {
            Ok(c.query_row("SELECT * FROM bots WHERE key=?1", params![key], Self::row_to_bot)
                .optional()?)
        })
    }

    pub fn create_bot(&self, name: &str, system_prompt: &str, greeting: &str) -> Result<Bot> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("tên bot trống");
        }
        let mut key = slugify(name);
        self.with(|c| {
            // De-dupe the key.
            let mut i = 1;
            loop {
                let exists: i64 =
                    c.query_row("SELECT COUNT(*) FROM bots WHERE key=?1", params![key], |r| r.get(0))?;
                if exists == 0 {
                    break;
                }
                i += 1;
                key = format!("{}-{}", slugify(name), i);
            }
            let now = now_ms();
            c.execute(
                "INSERT INTO bots(key,name,system_prompt,greeting,created_at) VALUES(?1,?2,?3,?4,?5)",
                params![key, name, system_prompt.trim(), greeting.trim(), now],
            )?;
            Ok(())
        })?;
        Ok(self.get_bot(&key)?.expect("just inserted"))
    }

    /// Patch bot fields; any `None` is left unchanged.
    #[allow(clippy::too_many_arguments)]
    pub fn update_bot(
        &self,
        key: &str,
        name: Option<&str>,
        system_prompt: Option<&str>,
        greeting: Option<&str>,
        model: Option<&str>,
        knowledge_scope: Option<&str>,
        allowed_mcp: Option<&[String]>,
        allowed_skills: Option<&[String]>,
        use_tools: Option<bool>,
        use_knowledge: Option<bool>,
        auto_ingest: Option<bool>,
        auto_issue: Option<bool>,
        enabled: Option<bool>,
    ) -> Result<bool> {
        self.with(|c| {
            macro_rules! set_str {
                ($col:literal, $v:expr) => {
                    if let Some(v) = $v {
                        c.execute(concat!("UPDATE bots SET ", $col, "=?1 WHERE key=?2"), params![v, key])?;
                    }
                };
            }
            set_str!("name", name.map(str::trim));
            set_str!("system_prompt", system_prompt);
            set_str!("greeting", greeting);
            set_str!("model", model);
            if let Some(v) = knowledge_scope.filter(|v| ["bot", "session", "user"].contains(v)) {
                c.execute("UPDATE bots SET knowledge_scope=?1 WHERE key=?2", params![v, key])?;
            }
            if let Some(v) = allowed_mcp {
                let j = serde_json::to_string(v).unwrap_or_else(|_| "[]".into());
                c.execute("UPDATE bots SET allowed_mcp=?1 WHERE key=?2", params![j, key])?;
            }
            if let Some(v) = allowed_skills {
                let j = serde_json::to_string(v).unwrap_or_else(|_| "[]".into());
                c.execute("UPDATE bots SET allowed_skills=?1 WHERE key=?2", params![j, key])?;
            }
            for (col, val) in [
                ("use_tools", use_tools),
                ("use_knowledge", use_knowledge),
                ("auto_ingest", auto_ingest),
                ("auto_issue", auto_issue),
                ("enabled", enabled),
            ] {
                if let Some(v) = val {
                    c.execute(
                        &format!("UPDATE bots SET {col}=?1 WHERE key=?2"),
                        params![v as i64, key],
                    )?;
                }
            }
            let found: i64 =
                c.query_row("SELECT COUNT(*) FROM bots WHERE key=?1", params![key], |r| r.get(0))?;
            Ok(found > 0)
        })
    }

    pub fn delete_bot(&self, key: &str) -> Result<bool> {
        self.with(|c| {
            let n = c.execute("DELETE FROM bots WHERE key=?1", params![key])?;
            c.execute("DELETE FROM channels WHERE bot_key=?1", params![key])?;
            // Sessions/messages are kept (transcript history) but orphaned.
            Ok(n > 0)
        })
    }

    // ---- channels ----

    fn row_to_channel(r: &rusqlite::Row) -> rusqlite::Result<Channel> {
        let raw: String = r.get("config")?;
        Ok(Channel {
            id: r.get("id")?,
            bot_key: r.get("bot_key")?,
            kind: r.get("kind")?,
            name: r.get("name")?,
            config: serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({})),
            enabled: r.get::<_, i64>("enabled")? != 0,
            cursor: r.get("cursor")?,
            last_sync_at: r.get("last_sync_at")?,
            last_status: r.get("last_status")?,
            last_error: r.get("last_error")?,
            created_at: r.get("created_at")?,
        })
    }

    pub fn list_channels(&self, bot_key: Option<&str>) -> Result<Vec<Channel>> {
        self.with(|c| {
            let mut out = Vec::new();
            if let Some(bk) = bot_key {
                let mut stmt = c.prepare("SELECT * FROM channels WHERE bot_key=?1 ORDER BY id")?;
                let rows = stmt.query_map(params![bk], Self::row_to_channel)?;
                for row in rows {
                    out.push(row?);
                }
            } else {
                let mut stmt = c.prepare("SELECT * FROM channels ORDER BY id")?;
                let rows = stmt.query_map([], Self::row_to_channel)?;
                for row in rows {
                    out.push(row?);
                }
            }
            Ok(out)
        })
    }

    pub fn list_enabled_channels(&self) -> Result<Vec<Channel>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT c.* FROM channels c JOIN bots b ON b.key=c.bot_key
                 WHERE c.enabled=1 AND b.enabled=1 ORDER BY c.id",
            )?;
            let rows = stmt
                .query_map([], Self::row_to_channel)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn get_channel(&self, id: i64) -> Result<Option<Channel>> {
        self.with(|c| {
            Ok(c.query_row("SELECT * FROM channels WHERE id=?1", params![id], Self::row_to_channel)
                .optional()?)
        })
    }

    pub fn create_channel(
        &self,
        bot_key: &str,
        kind: &str,
        name: &str,
        config: &serde_json::Value,
    ) -> Result<Channel> {
        let id = self.with(|c| {
            let now = now_ms();
            let cfg = config.to_string();
            c.execute(
                "INSERT INTO channels(bot_key,kind,name,config,created_at) VALUES(?1,?2,?3,?4,?5)",
                params![bot_key, kind, name.trim(), cfg, now],
            )?;
            Ok(c.last_insert_rowid())
        })?;
        Ok(self.get_channel(id)?.expect("just inserted"))
    }

    pub fn update_channel(
        &self,
        id: i64,
        name: Option<&str>,
        config: Option<&serde_json::Value>,
        enabled: Option<bool>,
    ) -> Result<bool> {
        self.with(|c| {
            if let Some(v) = name {
                c.execute("UPDATE channels SET name=?1 WHERE id=?2", params![v.trim(), id])?;
            }
            if let Some(v) = config {
                c.execute("UPDATE channels SET config=?1 WHERE id=?2", params![v.to_string(), id])?;
            }
            if let Some(v) = enabled {
                c.execute("UPDATE channels SET enabled=?1 WHERE id=?2", params![v as i64, id])?;
            }
            let found: i64 =
                c.query_row("SELECT COUNT(*) FROM channels WHERE id=?1", params![id], |r| r.get(0))?;
            Ok(found > 0)
        })
    }

    pub fn delete_channel(&self, id: i64) -> Result<bool> {
        self.with(|c| Ok(c.execute("DELETE FROM channels WHERE id=?1", params![id])? > 0))
    }

    /// Record the outcome of a poll/sync cycle for a channel.
    pub fn set_channel_sync(&self, id: i64, status: &str, error: &str, cursor: Option<&str>) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE channels SET last_sync_at=?1, last_status=?2, last_error=?3 WHERE id=?4",
                params![now_ms(), status, error, id],
            )?;
            if let Some(cur) = cursor {
                c.execute("UPDATE channels SET cursor=?1 WHERE id=?2", params![cur, id])?;
            }
            Ok(())
        })
    }

    // ---- sessions ----

    fn row_to_session(r: &rusqlite::Row) -> rusqlite::Result<Session> {
        let raw: String = r.get("context")?;
        Ok(Session {
            id: r.get("id")?,
            bot_key: r.get("bot_key")?,
            channel_kind: r.get("channel_kind")?,
            channel_id: r.get("channel_id")?,
            external_id: r.get("external_id")?,
            jid: r.get("jid")?,
            customer_name: r.get("customer_name")?,
            handoff_state: r.get("handoff_state")?,
            context: serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!({})),
            last_activity: r.get("last_activity")?,
            created_at: r.get("created_at")?,
        })
    }

    /// Find or open the conversation for `(bot, channel, external id)`.
    pub fn get_or_create_session(
        &self,
        bot_key: &str,
        channel_kind: &str,
        channel_id: i64,
        external_id: &str,
        jid: &str,
        customer_name: &str,
    ) -> Result<Session> {
        self.with(|c| {
            let now = now_ms();
            c.execute(
                "INSERT OR IGNORE INTO sessions(bot_key,channel_kind,channel_id,external_id,jid,customer_name,last_activity,created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?7)",
                params![bot_key, channel_kind, channel_id, external_id, jid, customer_name, now],
            )?;
            if !customer_name.trim().is_empty() {
                c.execute(
                    "UPDATE sessions SET customer_name=?1 WHERE bot_key=?2 AND channel_kind=?3 AND external_id=?4 AND customer_name=''",
                    params![customer_name, bot_key, channel_kind, external_id],
                )?;
            }
            let s = c.query_row(
                "SELECT * FROM sessions WHERE bot_key=?1 AND channel_kind=?2 AND external_id=?3",
                params![bot_key, channel_kind, external_id],
                Self::row_to_session,
            )?;
            Ok(s)
        })
    }

    pub fn get_session(&self, id: i64) -> Result<Option<Session>> {
        self.with(|c| {
            Ok(c.query_row("SELECT * FROM sessions WHERE id=?1", params![id], Self::row_to_session)
                .optional()?)
        })
    }

    pub fn list_sessions(&self, bot_key: Option<&str>, limit: i64) -> Result<Vec<Session>> {
        self.with(|c| {
            let mut out = Vec::new();
            // Only real conversations (≥1 message) — hides empty probe sessions.
            let has_msg = "EXISTS (SELECT 1 FROM messages m WHERE m.session_id=sessions.id)";
            if let Some(bk) = bot_key {
                let mut stmt = c.prepare(&format!(
                    "SELECT * FROM sessions WHERE bot_key=?1 AND {has_msg} ORDER BY last_activity DESC LIMIT ?2",
                ))?;
                let rows = stmt.query_map(params![bk, limit], Self::row_to_session)?;
                for row in rows {
                    out.push(row?);
                }
            } else {
                let mut stmt = c.prepare(&format!(
                    "SELECT * FROM sessions WHERE {has_msg} ORDER BY last_activity DESC LIMIT ?1",
                ))?;
                let rows = stmt.query_map(params![limit], Self::row_to_session)?;
                for row in rows {
                    out.push(row?);
                }
            }
            Ok(out)
        })
    }

    #[allow(dead_code)]
    pub fn touch_session(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE sessions SET last_activity=?1 WHERE id=?2", params![now_ms(), id])?;
            Ok(())
        })
    }

    pub fn delete_session(&self, id: i64) -> Result<bool> {
        self.with(|c| {
            c.execute("DELETE FROM messages WHERE session_id=?1", params![id])?;
            Ok(c.execute("DELETE FROM sessions WHERE id=?1", params![id])? > 0)
        })
    }

    /// Conversations for one bot, newest-first, each with its channel + a
    /// last-message preview + count (feeds the Chat conversation list).
    /// `channel_kind = None` → every platform.
    pub fn list_conversations(
        &self,
        bot_key: &str,
        channel_kind: Option<&str>,
        limit: i64,
    ) -> Result<Vec<serde_json::Value>> {
        self.with(|c| {
            let kind_filter = if channel_kind.is_some() { "AND s.channel_kind=?2" } else { "" };
            let sql = format!(
                "SELECT s.id, s.external_id, s.customer_name, s.last_activity, s.channel_kind,
                        (SELECT COUNT(*) FROM messages m WHERE m.session_id=s.id) AS cnt,
                        (SELECT content FROM messages m WHERE m.session_id=s.id ORDER BY m.id DESC LIMIT 1) AS preview
                 FROM sessions s WHERE s.bot_key=?1 {kind_filter}
                   AND EXISTS (SELECT 1 FROM messages m WHERE m.session_id=s.id)
                 ORDER BY s.last_activity DESC LIMIT {limit}"
            );
            let row = |r: &rusqlite::Row| -> rusqlite::Result<serde_json::Value> {
                Ok(serde_json::json!({
                    "id": r.get::<_, i64>(0)?,
                    "externalId": r.get::<_, String>(1)?,
                    "customerName": r.get::<_, String>(2)?,
                    "lastActivity": r.get::<_, i64>(3)?,
                    "channelKind": r.get::<_, String>(4)?,
                    "messageCount": r.get::<_, i64>(5)?,
                    "preview": r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                }))
            };
            let mut stmt = c.prepare(&sql)?;
            let rows = match channel_kind {
                Some(k) => stmt.query_map(params![bot_key, k], row)?.collect::<rusqlite::Result<Vec<_>>>()?,
                None => stmt.query_map(params![bot_key], row)?.collect::<rusqlite::Result<Vec<_>>>()?,
            };
            Ok(rows)
        })
    }

    pub fn set_handoff(&self, id: i64, state: &str) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE sessions SET handoff_state=?1 WHERE id=?2", params![state, id])?;
            Ok(())
        })
    }

    pub fn set_session_context(&self, id: i64, ctx: &serde_json::Value) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE sessions SET context=?1 WHERE id=?2", params![ctx.to_string(), id])?;
            Ok(())
        })
    }

    pub fn set_customer_name(&self, id: i64, name: &str) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE sessions SET customer_name=?1 WHERE id=?2", params![name.trim(), id])?;
            Ok(())
        })
    }

    // ---- messages ----

    pub fn add_message(&self, session_id: i64, role: &str, content: &str) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO messages(session_id,role,content,created_at) VALUES(?1,?2,?3,?4)",
                params![session_id, role, content, now_ms()],
            )?;
            c.execute("UPDATE sessions SET last_activity=?1 WHERE id=?2", params![now_ms(), session_id])?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Recent transcript for a session, oldest-first (for LLM context).
    pub fn history(&self, session_id: i64, limit: i64) -> Result<Vec<Message>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM (SELECT * FROM messages WHERE session_id=?1 ORDER BY id DESC LIMIT ?2)
                 ORDER BY id ASC",
            )?;
            let rows = stmt
                .query_map(params![session_id, limit], |r| {
                    Ok(Message {
                        id: r.get("id")?,
                        session_id: r.get("session_id")?,
                        role: r.get("role")?,
                        content: r.get("content")?,
                        created_at: r.get("created_at")?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn list_messages(&self, session_id: i64, limit: i64) -> Result<Vec<Message>> {
        self.history(session_id, limit)
    }

    // ---- settings + metrics ----

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| {
            Ok(c.query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| r.get(0))
                .optional()?)
        })
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO settings(key,value) VALUES(?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    /// Increment a metric counter (llm_calls / tokens_in / tokens_out).
    pub fn bump_metric(&self, key: &str, by: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO settings(key,value) VALUES(?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value=CAST(CAST(value AS INTEGER)+?3 AS TEXT)",
                params![format!("metric_{key}"), by.to_string(), by],
            )?;
            Ok(())
        })
    }

    fn metric(&self, c: &Connection, key: &str) -> i64 {
        c.query_row(
            "SELECT value FROM settings WHERE key=?1",
            params![format!("metric_{key}")],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
    }

    /// Feature toggles (default on except auto_ingest).
    pub fn features_json(&self) -> serde_json::Value {
        let get = |k: &str, default: bool| {
            self.get_setting(&format!("feat_{k}"))
                .ok()
                .flatten()
                .map(|v| v == "1")
                .unwrap_or(default)
        };
        serde_json::json!({
            "knowledge": get("knowledge", true),
            "wiki": get("wiki", true),
            "tools": get("tools", true),
        })
    }

    pub fn stats(&self) -> Result<serde_json::Value> {
        self.with(|c| {
            let bots: i64 = c.query_row("SELECT COUNT(*) FROM bots", [], |r| r.get(0))?;
            let channels: i64 = c.query_row("SELECT COUNT(*) FROM channels WHERE enabled=1", [], |r| r.get(0))?;
            let sessions: i64 = c.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
            let messages: i64 = c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?;
            let handoffs: i64 = c.query_row(
                "SELECT COUNT(*) FROM sessions WHERE handoff_state<>'bot'",
                [],
                |r| r.get(0),
            )?;
            // NB: read last_model via `c` directly — calling self.get_setting()
            // here would re-lock the (non-reentrant) mutex and deadlock.
            let last_model: String = c
                .query_row("SELECT value FROM settings WHERE key='last_model'", [], |r| r.get(0))
                .optional()
                .ok()
                .flatten()
                .unwrap_or_default();
            Ok(serde_json::json!({
                "bots": bots,
                "activeChannels": channels,
                "sessions": sessions,
                "messages": messages,
                "openHandoffs": handoffs,
                "llmCalls": self.metric(c, "llm_calls"),
                "tokensIn": self.metric(c, "tokens_in"),
                "tokensOut": self.metric(c, "tokens_out"),
                "lastModel": last_model,
            }))
        })
    }

    // ---- issues (support tickets) ----

    fn row_to_issue(r: &rusqlite::Row) -> rusqlite::Result<Issue> {
        Ok(Issue {
            id: r.get("id")?,
            session_id: r.get("session_id")?,
            bot_key: r.get("bot_key")?,
            external_id: r.get("external_id")?,
            title: r.get("title")?,
            description: r.get("description")?,
            status: r.get("status")?,
            priority: r.get("priority")?,
            category: r.get("category")?,
            sentiment: r.get("sentiment")?,
            ai_summary: r.get("ai_summary")?,
            tags: json_arr(&r.get::<_, String>("tags")?),
            resolution_note: r.get("resolution_note")?,
            assignee: r.get("assignee")?,
            created_at: r.get("created_at")?,
            updated_at: r.get("updated_at")?,
            resolved_at: r.get("resolved_at")?,
        })
    }

    /// Raise a support ticket (from a bot's [ISSUE] sentinel, an operator, or MCP).
    #[allow(clippy::too_many_arguments)]
    pub fn create_issue(
        &self,
        session_id: Option<i64>,
        bot_key: &str,
        external_id: &str,
        title: &str,
        description: &str,
        priority: &str,
        category: &str,
        sentiment: &str,
        ai_summary: &str,
        tags: &[String],
    ) -> Result<Issue> {
        let priority = if ISSUE_PRIORITIES.contains(&priority) { priority } else { "medium" };
        let id = self.with(|c| {
            let now = now_ms();
            c.execute(
                "INSERT INTO issues(session_id,bot_key,external_id,title,description,priority,category,sentiment,ai_summary,tags,created_at,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
                params![
                    session_id, bot_key, external_id, title.trim(), description.trim(),
                    priority, category.trim(), sentiment.trim(), ai_summary.trim(),
                    serde_json::to_string(tags).unwrap_or_else(|_| "[]".into()), now,
                ],
            )?;
            let id = c.last_insert_rowid();
            c.execute(
                "INSERT INTO issue_events(issue_id,kind,note,actor,created_at) VALUES(?1,'created',?2,?3,?4)",
                params![id, title.trim(), "system", now],
            )?;
            Ok(id)
        })?;
        Ok(self.get_issue(id)?.expect("just inserted"))
    }

    pub fn get_issue(&self, id: i64) -> Result<Option<Issue>> {
        self.with(|c| {
            Ok(c.query_row("SELECT * FROM issues WHERE id=?1", params![id], Self::row_to_issue)
                .optional()?)
        })
    }

    /// List issues, newest-first, filtered by optional status/priority/bot/search.
    pub fn list_issues(
        &self,
        status: Option<&str>,
        priority: Option<&str>,
        bot: Option<&str>,
        search: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Issue>> {
        self.with(|c| {
            let mut sql = String::from("SELECT * FROM issues WHERE 1=1");
            let mut args: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            if let Some(s) = status.filter(|s| !s.is_empty()) {
                sql.push_str(" AND status=?");
                args.push(Box::new(s.to_string()));
            }
            if let Some(p) = priority.filter(|p| !p.is_empty()) {
                sql.push_str(" AND priority=?");
                args.push(Box::new(p.to_string()));
            }
            if let Some(b) = bot.filter(|b| !b.is_empty()) {
                sql.push_str(" AND bot_key=?");
                args.push(Box::new(b.to_string()));
            }
            if let Some(q) = search.filter(|q| !q.is_empty()) {
                sql.push_str(" AND (title LIKE ? OR description LIKE ? OR category LIKE ?)");
                let like = format!("%{q}%");
                args.push(Box::new(like.clone()));
                args.push(Box::new(like.clone()));
                args.push(Box::new(like));
            }
            sql.push_str(" ORDER BY created_at DESC LIMIT ?");
            args.push(Box::new(limit));
            let mut stmt = c.prepare(&sql)?;
            let params_ref: Vec<&dyn rusqlite::types::ToSql> = args.iter().map(|b| b.as_ref()).collect();
            let rows = stmt
                .query_map(params_ref.as_slice(), Self::row_to_issue)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Patch an issue's fields, logging each change to `issue_events`. Setting
    /// status to resolved/closed stamps `resolved_at`.
    pub fn update_issue(&self, id: i64, patch: &IssuePatch, actor: &str) -> Result<bool> {
        let Some(before) = self.get_issue(id)? else {
            return Ok(false);
        };
        self.with(|c| {
            let now = now_ms();
            let log = |c: &Connection, field: &str, old: &str, new: &str| -> Result<()> {
                if old != new {
                    c.execute(
                        "INSERT INTO issue_events(issue_id,kind,field,old_val,new_val,actor,created_at)
                         VALUES(?1,'updated',?2,?3,?4,?5,?6)",
                        params![id, field, old, new, actor, now],
                    )?;
                }
                Ok(())
            };
            if let Some(v) = &patch.status {
                if ISSUE_STATUSES.contains(&v.as_str()) {
                    log(c, "status", &before.status, v)?;
                    c.execute("UPDATE issues SET status=?1 WHERE id=?2", params![v, id])?;
                    if (v == "resolved" || v == "closed") && before.resolved_at.is_none() {
                        c.execute("UPDATE issues SET resolved_at=?1 WHERE id=?2", params![now, id])?;
                    }
                }
            }
            if let Some(v) = &patch.priority {
                if ISSUE_PRIORITIES.contains(&v.as_str()) {
                    log(c, "priority", &before.priority, v)?;
                    c.execute("UPDATE issues SET priority=?1 WHERE id=?2", params![v, id])?;
                }
            }
            for (field, cur, val) in [
                ("category", &before.category, &patch.category),
                ("assignee", &before.assignee, &patch.assignee),
                ("resolution_note", &before.resolution_note, &patch.resolution_note),
                ("title", &before.title, &patch.title),
            ] {
                if let Some(v) = val {
                    log(c, field, cur, v)?;
                    c.execute(&format!("UPDATE issues SET {field}=?1 WHERE id=?2"), params![v, id])?;
                }
            }
            c.execute("UPDATE issues SET updated_at=?1 WHERE id=?2", params![now, id])?;
            Ok(true)
        })
    }

    pub fn list_issue_events(&self, issue_id: i64) -> Result<Vec<serde_json::Value>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT kind,field,old_val,new_val,note,actor,created_at FROM issue_events
                 WHERE issue_id=?1 ORDER BY id ASC",
            )?;
            let rows = stmt
                .query_map(params![issue_id], |r| {
                    Ok(serde_json::json!({
                        "kind": r.get::<_, String>(0)?,
                        "field": r.get::<_, String>(1)?,
                        "oldVal": r.get::<_, String>(2)?,
                        "newVal": r.get::<_, String>(3)?,
                        "note": r.get::<_, String>(4)?,
                        "actor": r.get::<_, String>(5)?,
                        "createdAt": r.get::<_, i64>(6)?,
                    }))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Group-by counts over a column, as `{value: count}`.
    fn group_count(&self, c: &Connection, col: &str) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        if let Ok(mut stmt) = c.prepare(&format!("SELECT {col}, COUNT(*) FROM issues GROUP BY {col}")) {
            let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)));
            if let Ok(rows) = rows {
                for row in rows.flatten() {
                    let key = if row.0.trim().is_empty() { "(none)".to_string() } else { row.0 };
                    out.insert(key, serde_json::json!(row.1));
                }
            }
        }
        serde_json::Value::Object(out)
    }

    /// Support-analysis aggregates for the Analytics dashboard.
    pub fn analytics(&self) -> Result<serde_json::Value> {
        self.with(|c| {
            let issues_total: i64 = c.query_row("SELECT COUNT(*) FROM issues", [], |r| r.get(0))?;
            let issues_open: i64 =
                c.query_row("SELECT COUNT(*) FROM issues WHERE status IN ('open','in_progress')", [], |r| r.get(0))?;
            // Sessions by channel (group_count is issues-only, so build inline).
            let mut by_channel = serde_json::Map::new();
            if let Ok(mut stmt) = c.prepare("SELECT channel_kind, COUNT(*) FROM sessions GROUP BY channel_kind") {
                if let Ok(rows) = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))) {
                    for row in rows.flatten() {
                        by_channel.insert(row.0, serde_json::json!(row.1));
                    }
                }
            }
            let handoffs: i64 =
                c.query_row("SELECT COUNT(*) FROM sessions WHERE handoff_state<>'bot'", [], |r| r.get(0))?;
            let sessions: i64 = c.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
            Ok(serde_json::json!({
                "issues": {
                    "total": issues_total,
                    "open": issues_open,
                    "byStatus": self.group_count(c, "status"),
                    "byPriority": self.group_count(c, "priority"),
                    "byCategory": self.group_count(c, "category"),
                    "bySentiment": self.group_count(c, "sentiment"),
                },
                "sessions": {
                    "total": sessions,
                    "openHandoffs": handoffs,
                    "byChannel": serde_json::Value::Object(by_channel),
                },
                "llmCalls": self.metric(c, "llm_calls"),
                "tokensIn": self.metric(c, "tokens_in"),
                "tokensOut": self.metric(c, "tokens_out"),
            }))
        })
    }
}

const DEFAULT_SUPPORT_PROMPT: &str = "Bạn là trợ lý chăm sóc khách hàng (CSKH) thân thiện, trả lời ngắn gọn, chính xác và lịch sự bằng ngôn ngữ của khách. \
Chỉ dùng thông tin có thật; khi không chắc hoặc cần thao tác ngoài khả năng, hãy đề nghị chuyển cho nhân viên hỗ trợ (human handoff) thay vì bịa. \
Ưu tiên dùng kiến thức (knowledge) của bot khi được cung cấp trong ngữ cảnh.";

const DEFAULT_GREETING: &str = "Xin chào 👋 Mình là trợ lý CSKH. Mình có thể giúp gì cho bạn?";
