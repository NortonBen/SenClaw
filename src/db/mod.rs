//! SQLite handle. Mirrors `src-old/db/db.ts`.
//!
//! Tables owned here:
//!   * `groups`            — GroupBinding registry (legacy, being phased out)
//!   * `channels`          — platform connections (Telegram bot, Feishu, QQ, etc.)
//!   * `agents`            — AI agent definitions (folder, name, tools, model, etc.)
//!   * `bindings`          — N:N join: agent ↔ channel + per-chat JID
//!   * `channel_messages`  — raw platform messages (incoming from Telegram/Feishu/etc.)
//!   * `group_messages`    — conversation history (user messages + bot responses)
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
use rusqlite::Connection;

use crate::config::Config;

mod helpers;
mod rows;
mod schema;

mod agents;
mod bindings;
mod channels;
mod embedding;
mod groups;
mod messages;
mod router_state;
mod scheduled_tasks;

pub mod cowork;

#[cfg(test)]
mod tests;

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
        let mut conn =
            Connection::open(path).with_context(|| format!("open sqlite {}", path.display()))?;
        Self::apply_pragmas_and_schema(&mut conn, config)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// In-memory DB. Used by integration tests and CLI dry-runs.
    pub fn open_in_memory(config: &Config) -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        Self::apply_pragmas_and_schema(&mut conn, config)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn apply_pragmas_and_schema(conn: &mut Connection, config: &Config) -> Result<()> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        schema::apply_schema(conn)?;
        schema::apply_memory_tables(conn, config)?;
        Ok(())
    }

    pub(crate) fn with_conn<R>(&self, f: impl FnOnce(&Connection) -> Result<R>) -> Result<R> {
        let guard = self.conn.lock().expect("db mutex poisoned");
        f(&guard)
    }

    pub(crate) fn with_conn_mut<R>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<R>,
    ) -> Result<R> {
        let mut guard = self.conn.lock().expect("db mutex poisoned");
        f(&mut guard)
    }
}
