//! SQLite handle. Mirrors `src-old/db/db.ts`.
//!
//! Tables owned here:
//!   * `groups`            — GroupBinding registry
//!   * `channel_messages`  — message history (FIFO, retention per group)
//!   * `scheduled_tasks`   — scheduler entries
//!   * `task_run_logs`     — task execution log
//!   * `router_state`      — KV cursor (e.g. lastAgentTimestamp)
//!
//! Memory tables live in [`crate::memory::schema`] and are applied during
//! [`Db::open`] alongside the schema here.
//!
//! The handle wraps a single [`rusqlite::Connection`] under a [`Mutex`].
//! That matches the TS one-process model and keeps this layer simple. If we
//! ever need real concurrency, swap to `tokio_rusqlite` or a connection pool.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::config::{Config, EmbeddingProvider};
use crate::memory::schema::{apply_memory_schema, build_model_key};
use crate::types::{
    ContextMode, GroupBinding, RunStatus, ScheduleType, ScheduledTask, StoredMessage, TaskRunLog,
    TaskRunLogInsert, TaskStatus,
};

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    /// Open (or create) the SQLite database, ensure pragmas, schema, and
    /// memory tables are in place. Idempotent.
    pub fn open(config: &Config) -> Result<Self> {
        if let Some(parent) = config.paths.db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
        Self::open_at(&config.paths.db_path, config)
    }

    /// Open a DB at an explicit path — used by tests + when callers want to
    /// override the configured location.
    pub fn open_at(path: &Path, config: &Config) -> Result<Self> {
        let mut conn = Connection::open(path)
            .with_context(|| format!("open sqlite {}", path.display()))?;
        Self::apply_pragmas_and_schema(&mut conn, config)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// In-memory DB. Used by integration tests and CLI dry-runs.
    pub fn open_in_memory(config: &Config) -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::apply_pragmas_and_schema(&mut conn, config)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn apply_pragmas_and_schema(conn: &mut Connection, config: &Config) -> Result<()> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        apply_schema(conn)?;

        let provider = config.memory.embedding_provider;
        let enable_vec = provider != EmbeddingProvider::None;
        let dimensions =
            Config::resolve_dimensions(provider, config.memory.embedding_dimensions);
        let model_name = match provider {
            EmbeddingProvider::Openrouter => config.memory.openrouter_model.clone(),
            EmbeddingProvider::Ollama => config.memory.ollama_model.clone(),
            EmbeddingProvider::Local => {
                let m = config.memory.local_model.clone();
                if m.is_empty() { "default".to_owned() } else { m }
            }
            EmbeddingProvider::Openai => {
                let m = config.memory.openai_model.clone();
                if m.is_empty() { "text-embedding-3-small".to_owned() } else { m }
            }
            EmbeddingProvider::None => String::new(),
        };
        let model_key = if enable_vec {
            build_model_key(provider.as_str(), &model_name, dimensions)
        } else {
            String::new()
        };
        if let Err(e) = apply_memory_schema(conn, enable_vec, dimensions, &model_key) {
            tracing::error!(
                error = %e,
                "[DB] applyMemorySchema failed, memory search will be unavailable"
            );
        }
        Ok(())
    }

    pub(crate) fn with_conn<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let guard = self.conn.lock().expect("db mutex poisoned");
        f(&guard)
    }

    pub(crate) fn with_conn_mut<R>(&self, f: impl FnOnce(&mut Connection) -> Result<R>) -> Result<R> {
        let mut guard = self.conn.lock().expect("db mutex poisoned");
        f(&mut guard)
    }

    // ============================================================
    // Groups
    // ============================================================

    pub fn upsert_group(&self, g: &GroupBinding) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                r#"
                INSERT INTO groups
                  (jid, folder, name, channel, is_admin, requires_trigger,
                   allowed_tools, allowed_paths, allowed_work_dirs,
                   bot_token, max_messages, last_active, added_at)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                ON CONFLICT(jid) DO UPDATE SET
                  folder            = excluded.folder,
                  name              = excluded.name,
                  channel           = excluded.channel,
                  is_admin          = excluded.is_admin,
                  requires_trigger  = excluded.requires_trigger,
                  allowed_tools     = excluded.allowed_tools,
                  allowed_paths     = excluded.allowed_paths,
                  allowed_work_dirs = excluded.allowed_work_dirs,
                  bot_token         = excluded.bot_token,
                  max_messages      = excluded.max_messages,
                  last_active       = excluded.last_active
                "#,
                params![
                    g.jid,
                    g.folder,
                    g.name,
                    g.channel,
                    g.is_admin as i64,
                    g.requires_trigger as i64,
                    json_or_null(&g.allowed_tools)?,
                    json_or_null(&g.allowed_paths)?,
                    json_or_null(&g.allowed_work_dirs)?,
                    g.bot_token,
                    g.max_messages,
                    g.last_active,
                    g.added_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_group(&self, jid: &str) -> Result<Option<GroupBinding>> {
        self.with_conn(|c| {
            let row = c
                .query_row("SELECT * FROM groups WHERE jid = ?1", params![jid], |r| {
                    Ok(row_to_group(r))
                })
                .optional()?;
            row.transpose()
        })
    }

    pub fn list_groups(&self) -> Result<Vec<GroupBinding>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM groups ORDER BY added_at")?;
            let rows = stmt
                .query_map([], |r| Ok(row_to_group(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect::<Result<Vec<_>>>()
        })
    }

    pub fn delete_group(&self, jid: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM groups WHERE jid = ?1", params![jid])?;
            Ok(())
        })
    }

    pub fn delete_group_by_folder(&self, folder: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM groups WHERE folder = ?1", params![folder])?;
            Ok(())
        })
    }

    /// Atomic JID rename via transaction. SQLite forbids `UPDATE PRIMARY KEY`,
    /// so we delete the old row + insert the new one inside one tx.
    pub fn rename_group_jid(&self, old_jid: &str, new_jid: &str) -> Result<Option<GroupBinding>> {
        self.with_conn_mut(|c| {
            let existing: Option<GroupBinding> = c
                .query_row("SELECT * FROM groups WHERE jid = ?1", params![old_jid], |r| {
                    Ok(row_to_group(r))
                })
                .optional()?
                .transpose()?;
            let Some(mut binding) = existing else { return Ok(None) };
            binding.jid = new_jid.to_owned();

            let tx = c.transaction()?;
            tx.execute("DELETE FROM groups WHERE jid = ?1", params![old_jid])?;
            tx.execute(
                r#"
                INSERT INTO groups
                  (jid, folder, name, channel, is_admin, requires_trigger,
                   allowed_tools, allowed_paths, allowed_work_dirs,
                   bot_token, max_messages, last_active, added_at)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                "#,
                params![
                    binding.jid,
                    binding.folder,
                    binding.name,
                    binding.channel,
                    binding.is_admin as i64,
                    binding.requires_trigger as i64,
                    json_or_null(&binding.allowed_tools)?,
                    json_or_null(&binding.allowed_paths)?,
                    json_or_null(&binding.allowed_work_dirs)?,
                    binding.bot_token,
                    binding.max_messages,
                    binding.last_active,
                    binding.added_at,
                ],
            )?;
            tx.commit()?;
            Ok(Some(binding))
        })
    }

    pub fn touch_group_active(&self, jid: &str, timestamp: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE groups SET last_active = ?1 WHERE jid = ?2",
                params![timestamp, jid],
            )?;
            Ok(())
        })
    }

    // ============================================================
    // Messages
    // ============================================================

    /// Insert a message and FIFO-trim the chat to its retention limit.
    /// Limit precedence: per-group `max_messages` override → config default.
    pub fn insert_message(&self, msg: &StoredMessage, default_limit: u32) -> Result<()> {
        self.with_conn(|c| {
            let limit: i64 = c
                .query_row(
                    "SELECT max_messages FROM groups WHERE jid = ?1",
                    params![msg.chat_jid],
                    |r| r.get::<_, Option<i64>>(0),
                )
                .optional()?
                .flatten()
                .unwrap_or(default_limit as i64);

            c.execute(
                r#"
                INSERT OR IGNORE INTO channel_messages
                  (message_id, chat_jid, sender_jid, sender_name, content,
                   timestamp, is_from_me, is_bot_reply, reply_to_id, media_type)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                "#,
                params![
                    msg.message_id,
                    msg.chat_jid,
                    msg.sender_jid,
                    msg.sender_name,
                    msg.content,
                    msg.timestamp,
                    msg.is_from_me as i64,
                    msg.is_bot_reply as i64,
                    msg.reply_to_id,
                    msg.media_type,
                ],
            )?;

            c.execute(
                r#"
                DELETE FROM channel_messages
                WHERE chat_jid = ?1
                  AND message_id NOT IN (
                    SELECT message_id FROM channel_messages
                    WHERE chat_jid = ?1
                    ORDER BY timestamp DESC
                    LIMIT ?2
                  )
                "#,
                params![msg.chat_jid, limit],
            )?;
            Ok(())
        })
    }

    pub fn get_messages(&self, chat_jid: &str, since: Option<&str>) -> Result<Vec<StoredMessage>> {
        self.with_conn(|c| {
            let rows: Vec<rusqlite::Result<Result<StoredMessage>>> = if let Some(since) = since {
                let mut stmt = c.prepare(
                    "SELECT * FROM channel_messages
                     WHERE chat_jid = ?1 AND timestamp > ?2
                     ORDER BY timestamp ASC",
                )?;
                let v: Vec<_> = stmt
                    .query_map(params![chat_jid, since], |r| Ok(row_to_message(r)))?
                    .collect();
                v
            } else {
                let mut stmt = c.prepare(
                    "SELECT * FROM channel_messages
                     WHERE chat_jid = ?1
                     ORDER BY timestamp ASC",
                )?;
                let v: Vec<_> = stmt
                    .query_map(params![chat_jid], |r| Ok(row_to_message(r)))?
                    .collect();
                v
            };
            rows.into_iter()
                .map(|r| r.map_err(anyhow::Error::from).and_then(|inner| inner))
                .collect()
        })
    }

    // ============================================================
    // Scheduled tasks
    // ============================================================

    pub fn insert_task(&self, task: &ScheduledTask) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                r#"
                INSERT INTO scheduled_tasks
                  (id, group_folder, chat_jid, prompt, schedule_type, schedule_value,
                   context_mode, script_path, next_run, last_run, last_result, status, created_at)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                "#,
                params![
                    task.id,
                    task.group_folder,
                    task.chat_jid,
                    task.prompt,
                    task.schedule_type.as_str(),
                    task.schedule_value,
                    task.context_mode.as_str(),
                    task.script_command,
                    task.next_run,
                    task.last_run,
                    task.last_result,
                    task.status.as_str(),
                    task.created_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_due_tasks(&self, now: &str) -> Result<Vec<ScheduledTask>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM scheduled_tasks
                 WHERE status = 'active' AND next_run IS NOT NULL AND next_run <= ?1
                 ORDER BY next_run ASC",
            )?;
            let rows = stmt
                .query_map(params![now], |r| Ok(row_to_task(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        })
    }

    pub fn get_tasks_by_group(&self, group_folder: &str) -> Result<Vec<ScheduledTask>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM scheduled_tasks WHERE group_folder = ?1 ORDER BY created_at DESC",
            )?;
            let rows = stmt
                .query_map(params![group_folder], |r| Ok(row_to_task(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        })
    }

    pub fn list_all_tasks(&self) -> Result<Vec<ScheduledTask>> {
        self.with_conn(|c| {
            let mut stmt =
                c.prepare("SELECT * FROM scheduled_tasks ORDER BY created_at DESC")?;
            let rows = stmt
                .query_map([], |r| Ok(row_to_task(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        })
    }

    /// Result is truncated to 500 chars (matches TS).
    pub fn update_task_run(
        &self,
        id: &str,
        next_run: Option<&str>,
        last_run: &str,
        last_result: Option<&str>,
        status: TaskStatus,
    ) -> Result<()> {
        let truncated: Option<String> = last_result.map(|s| {
            if s.chars().count() > 500 {
                s.chars().take(500).collect()
            } else {
                s.to_owned()
            }
        });
        self.with_conn(|c| {
            c.execute(
                "UPDATE scheduled_tasks
                 SET next_run = ?1, last_run = ?2, last_result = ?3, status = ?4
                 WHERE id = ?5",
                params![next_run, last_run, truncated, status.as_str(), id],
            )?;
            Ok(())
        })
    }

    pub fn advance_task_next_run(
        &self,
        id: &str,
        next_run: Option<&str>,
        status: TaskStatus,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE scheduled_tasks SET next_run = ?1, status = ?2 WHERE id = ?3",
                params![next_run, status.as_str(), id],
            )?;
            Ok(())
        })
    }

    pub fn update_task_status(&self, id: &str, status: TaskStatus) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE scheduled_tasks SET status = ?1 WHERE id = ?2",
                params![status.as_str(), id],
            )?;
            Ok(())
        })
    }

    pub fn delete_task(&self, id: &str) -> Result<bool> {
        self.with_conn(|c| {
            let n = c.execute("DELETE FROM scheduled_tasks WHERE id = ?1", params![id])?;
            Ok(n > 0)
        })
    }

    // ============================================================
    // Task run logs
    // ============================================================

    pub fn insert_task_run_log(&self, e: &TaskRunLogInsert) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO task_run_logs (task_id, run_at, duration_ms, status, result, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    e.task_id,
                    e.run_at,
                    e.duration_ms,
                    e.status.as_str(),
                    e.result,
                    e.error,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_task_run_logs(&self, task_id: &str, limit: u32) -> Result<Vec<TaskRunLog>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT id, task_id, run_at, duration_ms, status, result, error
                 FROM task_run_logs WHERE task_id = ?1 ORDER BY run_at DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![task_id, limit as i64], |r| {
                    Ok(TaskRunLog {
                        id: r.get(0)?,
                        task_id: r.get(1)?,
                        run_at: r.get(2)?,
                        duration_ms: r.get(3)?,
                        status: RunStatus::parse(&r.get::<_, String>(4)?),
                        result: r.get(5)?,
                        error: r.get(6)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    // ============================================================
    // Router state
    // ============================================================

    pub fn get_router_state(&self, key: &str) -> Result<Option<String>> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT value FROM router_state WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()?)
        })
    }

    pub fn set_router_state(&self, key: &str, value: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO router_state (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
            Ok(())
        })
    }

    pub fn get_last_agent_timestamp(&self, chat_jid: &str) -> Result<Option<String>> {
        self.get_router_state(&format!("lastAgent:{chat_jid}"))
    }

    pub fn set_last_agent_timestamp(&self, chat_jid: &str, timestamp: &str) -> Result<()> {
        self.set_router_state(&format!("lastAgent:{chat_jid}"), timestamp)
    }

    // ============================================================
    // Embedding cache
    // ============================================================

    pub fn get_cached_embedding(
        &self,
        provider: &str,
        model: &str,
        hash: &str,
    ) -> Result<Option<Vec<u8>>> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT embedding FROM embedding_cache WHERE provider = ?1 AND model = ?2 AND hash = ?3",
                params![provider, model, hash],
                |r| r.get::<_, Vec<u8>>(0),
            )
            .optional()?)
        })
    }

    pub fn insert_cached_embedding(
        &self,
        provider: &str,
        model: &str,
        hash: &str,
        embedding: &[u8],
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT OR IGNORE INTO embedding_cache (provider, model, hash, embedding) VALUES (?1, ?2, ?3, ?4)",
                params![provider, model, hash, embedding],
            )?;
            Ok(())
        })
    }
}

// ============================================================
// Schema + helpers
// ============================================================

fn apply_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS groups (
          jid                  TEXT PRIMARY KEY,
          folder               TEXT UNIQUE NOT NULL,
          name                 TEXT NOT NULL DEFAULT '',
          channel              TEXT NOT NULL DEFAULT 'telegram',
          is_admin             INTEGER NOT NULL DEFAULT 0,
          requires_trigger     INTEGER NOT NULL DEFAULT 1,
          allowed_tools        TEXT,
          allowed_paths        TEXT,
          allowed_work_dirs    TEXT,
          bot_token            TEXT,
          max_messages         INTEGER,
          last_active          TEXT,
          added_at             TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS channel_messages (
          message_id   TEXT NOT NULL,
          chat_jid     TEXT NOT NULL,
          sender_jid   TEXT NOT NULL DEFAULT '',
          sender_name  TEXT NOT NULL DEFAULT '',
          content      TEXT NOT NULL DEFAULT '',
          timestamp    TEXT NOT NULL,
          is_from_me   INTEGER NOT NULL DEFAULT 0,
          is_bot_reply INTEGER NOT NULL DEFAULT 0,
          reply_to_id  TEXT,
          media_type   TEXT,
          PRIMARY KEY (message_id, chat_jid)
        );
        CREATE INDEX IF NOT EXISTS idx_msg_timestamp
          ON channel_messages(chat_jid, timestamp);

        CREATE TABLE IF NOT EXISTS scheduled_tasks (
          id             TEXT PRIMARY KEY,
          group_folder   TEXT NOT NULL,
          chat_jid       TEXT NOT NULL,
          prompt         TEXT NOT NULL,
          schedule_type  TEXT NOT NULL,
          schedule_value TEXT NOT NULL,
          context_mode   TEXT NOT NULL DEFAULT 'isolated',
          script_path    TEXT,
          next_run       TEXT,
          last_run       TEXT,
          last_result    TEXT,
          status         TEXT NOT NULL DEFAULT 'active',
          created_at     TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_task_next_run
          ON scheduled_tasks(next_run, status);

        CREATE TABLE IF NOT EXISTS task_run_logs (
          id          INTEGER PRIMARY KEY AUTOINCREMENT,
          task_id     TEXT NOT NULL,
          run_at      TEXT NOT NULL,
          duration_ms INTEGER,
          status      TEXT NOT NULL,
          result      TEXT,
          error       TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_run_task_id
          ON task_run_logs(task_id, run_at);

        CREATE TABLE IF NOT EXISTS router_state (
          key   TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );
        "#,
    )?;

    // Migrations: SQLite has no `IF NOT EXISTS` on `ALTER TABLE`.
    let group_cols = column_names(conn, "groups")?;
    if !group_cols.iter().any(|c| c == "allowed_work_dirs") {
        conn.execute("ALTER TABLE groups ADD COLUMN allowed_work_dirs TEXT", [])?;
    }
    let task_cols = column_names(conn, "scheduled_tasks")?;
    if !task_cols.iter().any(|c| c == "script_path") {
        conn.execute("ALTER TABLE scheduled_tasks ADD COLUMN script_path TEXT", [])?;
    }
    Ok(())
}

fn column_names(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names)
}

fn json_or_null(v: &Option<Vec<String>>) -> Result<Option<String>> {
    Ok(match v {
        None => None,
        Some(list) => Some(serde_json::to_string(list)?),
    })
}

fn parse_json_array(raw: Option<String>) -> Option<Vec<String>> {
    raw.and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
}

fn row_to_group(row: &Row<'_>) -> Result<GroupBinding> {
    Ok(GroupBinding {
        jid: row.get("jid")?,
        folder: row.get("folder")?,
        name: row.get("name")?,
        channel: row.get::<_, Option<String>>("channel")?.unwrap_or_default(),
        is_admin: row.get::<_, i64>("is_admin")? != 0,
        requires_trigger: row.get::<_, i64>("requires_trigger")? != 0,
        allowed_tools: parse_json_array(row.get("allowed_tools")?),
        allowed_paths: parse_json_array(row.get("allowed_paths")?),
        allowed_work_dirs: parse_json_array(row.get("allowed_work_dirs")?),
        bot_token: row.get("bot_token")?,
        max_messages: row
            .get::<_, Option<i64>>("max_messages")?
            .map(|n| n as u32),
        last_active: row.get("last_active")?,
        added_at: row.get("added_at")?,
    })
}

fn row_to_message(row: &Row<'_>) -> Result<StoredMessage> {
    Ok(StoredMessage {
        message_id: row.get("message_id")?,
        chat_jid: row.get("chat_jid")?,
        sender_jid: row.get("sender_jid")?,
        sender_name: row.get("sender_name")?,
        content: row.get("content")?,
        timestamp: row.get("timestamp")?,
        is_from_me: row.get::<_, i64>("is_from_me")? != 0,
        is_bot_reply: row.get::<_, i64>("is_bot_reply")? != 0,
        reply_to_id: row.get("reply_to_id")?,
        media_type: row.get("media_type")?,
    })
}

fn row_to_task(row: &Row<'_>) -> Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: row.get("id")?,
        group_folder: row.get("group_folder")?,
        chat_jid: row.get("chat_jid")?,
        prompt: row.get("prompt")?,
        schedule_type: ScheduleType::parse(&row.get::<_, String>("schedule_type")?),
        schedule_value: row.get("schedule_value")?,
        context_mode: ContextMode::parse(&row.get::<_, String>("context_mode")?),
        script_command: row.get("script_path")?,
        next_run: row.get("next_run")?,
        last_run: row.get("last_run")?,
        last_result: row.get("last_result")?,
        status: TaskStatus::parse(&row.get::<_, String>("status")?),
        created_at: row.get("created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg() -> Config {
        Config::from_env()
    }

    fn sample_group() -> GroupBinding {
        GroupBinding {
            jid: "tg:group:1".into(),
            folder: "team-a".into(),
            name: "Team A".into(),
            channel: "telegram".into(),
            is_admin: true,
            requires_trigger: false,
            allowed_tools: Some(vec!["Read".into(), "Grep".into()]),
            allowed_paths: None,
            allowed_work_dirs: Some(vec!["/tmp/work".into()]),
            bot_token: Some("tok".into()),
            max_messages: Some(50),
            last_active: None,
            added_at: "2026-04-28T00:00:00Z".into(),
        }
    }

    #[test]
    fn open_in_memory_smoke() {
        Db::open_in_memory(&cfg()).unwrap();
    }

    #[test]
    fn group_upsert_get_list_delete() {
        let db = Db::open_in_memory(&cfg()).unwrap();
        let g = sample_group();
        db.upsert_group(&g).unwrap();
        let got = db.get_group(&g.jid).unwrap().unwrap();
        assert_eq!(got.folder, g.folder);
        assert_eq!(got.allowed_tools.as_deref(), Some(&["Read".into(), "Grep".into()][..]));
        assert_eq!(got.allowed_paths, None);
        assert_eq!(got.allowed_work_dirs.as_deref(), Some(&["/tmp/work".into()][..]));

        // upsert again with a name change
        let mut g2 = g.clone();
        g2.name = "Renamed".into();
        db.upsert_group(&g2).unwrap();
        assert_eq!(db.get_group(&g.jid).unwrap().unwrap().name, "Renamed");

        let all = db.list_groups().unwrap();
        assert_eq!(all.len(), 1);

        db.delete_group(&g.jid).unwrap();
        assert!(db.get_group(&g.jid).unwrap().is_none());
    }

    #[test]
    fn rename_group_jid_atomic() {
        let db = Db::open_in_memory(&cfg()).unwrap();
        db.upsert_group(&sample_group()).unwrap();
        let renamed = db
            .rename_group_jid("tg:group:1", "tg:group:99")
            .unwrap()
            .unwrap();
        assert_eq!(renamed.jid, "tg:group:99");
        assert!(db.get_group("tg:group:1").unwrap().is_none());
        assert!(db.get_group("tg:group:99").unwrap().is_some());
    }

    #[test]
    fn message_fifo_trims() {
        let db = Db::open_in_memory(&cfg()).unwrap();
        // Insert 5 messages with limit=3 → only the latest 3 by timestamp survive.
        for i in 0..5 {
            let msg = StoredMessage {
                message_id: format!("m{i}"),
                chat_jid: "tg:group:1".into(),
                sender_jid: "u".into(),
                sender_name: "u".into(),
                content: format!("hi {i}"),
                timestamp: format!("2026-04-28T00:00:0{i}Z"),
                is_from_me: false,
                is_bot_reply: false,
                reply_to_id: None,
                media_type: None,
            };
            db.insert_message(&msg, 3).unwrap();
        }
        let kept = db.get_messages("tg:group:1", None).unwrap();
        assert_eq!(kept.len(), 3);
        let ids: Vec<&str> = kept.iter().map(|m| m.message_id.as_str()).collect();
        assert_eq!(ids, ["m2", "m3", "m4"]);
    }

    #[test]
    fn message_since_filter() {
        let db = Db::open_in_memory(&cfg()).unwrap();
        for i in 0..3 {
            let msg = StoredMessage {
                message_id: format!("m{i}"),
                chat_jid: "tg:group:1".into(),
                sender_jid: "u".into(),
                sender_name: "u".into(),
                content: "x".into(),
                timestamp: format!("2026-04-28T00:00:0{i}Z"),
                is_from_me: false,
                is_bot_reply: false,
                reply_to_id: None,
                media_type: None,
            };
            db.insert_message(&msg, 100).unwrap();
        }
        let after = db
            .get_messages("tg:group:1", Some("2026-04-28T00:00:00Z"))
            .unwrap();
        assert_eq!(after.len(), 2);
    }

    #[test]
    fn task_lifecycle_and_logs() {
        let db = Db::open_in_memory(&cfg()).unwrap();
        let task = ScheduledTask {
            id: "t1".into(),
            group_folder: "team-a".into(),
            chat_jid: "tg:group:1".into(),
            prompt: "do thing".into(),
            schedule_type: ScheduleType::Cron,
            schedule_value: "*/5 * * * *".into(),
            context_mode: ContextMode::Isolated,
            script_command: None,
            next_run: Some("2026-04-28T00:05:00Z".into()),
            last_run: None,
            last_result: None,
            status: TaskStatus::Active,
            created_at: "2026-04-28T00:00:00Z".into(),
        };
        db.insert_task(&task).unwrap();
        assert_eq!(db.get_tasks_by_group("team-a").unwrap().len(), 1);

        // Due now → returned.
        let due = db.get_due_tasks("2026-04-28T00:10:00Z").unwrap();
        assert_eq!(due.len(), 1);

        let big = "x".repeat(800);
        db.update_task_run(
            "t1",
            Some("2026-04-28T00:10:00Z"),
            "2026-04-28T00:05:00Z",
            Some(&big),
            TaskStatus::Active,
        )
        .unwrap();
        let after = &db.get_tasks_by_group("team-a").unwrap()[0];
        assert_eq!(after.last_result.as_deref().unwrap().chars().count(), 500);

        db.insert_task_run_log(&TaskRunLogInsert {
            task_id: "t1".into(),
            run_at: "2026-04-28T00:05:00Z".into(),
            duration_ms: Some(120),
            status: RunStatus::Success,
            result: Some("ok".into()),
            error: None,
        })
        .unwrap();
        let logs = db.get_task_run_logs("t1", 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status, RunStatus::Success);
        assert_eq!(logs[0].duration_ms, Some(120));

        assert!(db.delete_task("t1").unwrap());
        assert!(!db.delete_task("t1").unwrap());
    }

    #[test]
    fn router_state_get_set() {
        let db = Db::open_in_memory(&cfg()).unwrap();
        assert!(db.get_router_state("k").unwrap().is_none());
        db.set_router_state("k", "v").unwrap();
        assert_eq!(db.get_router_state("k").unwrap().as_deref(), Some("v"));
        db.set_router_state("k", "v2").unwrap();
        assert_eq!(db.get_router_state("k").unwrap().as_deref(), Some("v2"));

        db.set_last_agent_timestamp("tg:group:1", "2026-04-28T00:00:00Z").unwrap();
        assert_eq!(
            db.get_last_agent_timestamp("tg:group:1").unwrap().as_deref(),
            Some("2026-04-28T00:00:00Z")
        );
    }

    #[test]
    fn migration_adds_missing_columns_on_existing_db() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        // Pre-create an "old" DB without the migrated columns.
        {
            let conn = Connection::open(tmp.path()).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE groups (
                  jid TEXT PRIMARY KEY, folder TEXT UNIQUE NOT NULL, name TEXT NOT NULL DEFAULT '',
                  channel TEXT NOT NULL DEFAULT 'telegram', is_admin INTEGER NOT NULL DEFAULT 0,
                  requires_trigger INTEGER NOT NULL DEFAULT 1, allowed_tools TEXT, allowed_paths TEXT,
                  bot_token TEXT, max_messages INTEGER, last_active TEXT, added_at TEXT NOT NULL
                );
                CREATE TABLE scheduled_tasks (
                  id TEXT PRIMARY KEY, group_folder TEXT NOT NULL, chat_jid TEXT NOT NULL,
                  prompt TEXT NOT NULL, schedule_type TEXT NOT NULL, schedule_value TEXT NOT NULL,
                  context_mode TEXT NOT NULL DEFAULT 'isolated', next_run TEXT, last_run TEXT,
                  last_result TEXT, status TEXT NOT NULL DEFAULT 'active', created_at TEXT NOT NULL
                );
                "#,
            )
            .unwrap();
        }
        let db = Db::open_at(tmp.path(), &cfg()).unwrap();
        // Should not error; column should exist now.
        db.upsert_group(&sample_group()).unwrap();
        let got = db.get_group("tg:group:1").unwrap().unwrap();
        assert_eq!(got.allowed_work_dirs.as_deref(), Some(&["/tmp/work".into()][..]));
    }
}
