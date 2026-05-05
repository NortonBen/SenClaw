use anyhow::Result;
use rusqlite::Connection;

use crate::config::Config;
use crate::memory::schema::{apply_memory_schema, build_model_key};

pub(crate) fn apply_schema(conn: &Connection) -> Result<()> {
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

        CREATE TABLE IF NOT EXISTS channels (
          id                INTEGER PRIMARY KEY AUTOINCREMENT,
          platform_type     TEXT NOT NULL,
          name              TEXT NOT NULL,
          credentials_json  TEXT NOT NULL DEFAULT '{}',
          connection_state  TEXT NOT NULL DEFAULT 'disconnected',
          created_at        TEXT NOT NULL,
          updated_at        TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS agents (
          id                INTEGER PRIMARY KEY AUTOINCREMENT,
          folder            TEXT UNIQUE NOT NULL,
          name              TEXT NOT NULL DEFAULT '',
          requires_trigger  INTEGER NOT NULL DEFAULT 1,
          allowed_tools     TEXT,
          allowed_paths     TEXT,
          allowed_work_dirs TEXT,
          core_prompt       TEXT NOT NULL DEFAULT '',
          model_id          TEXT,
          created_at        TEXT NOT NULL,
          updated_at        TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS bindings (
          id                  INTEGER PRIMARY KEY AUTOINCREMENT,
          jid                 TEXT UNIQUE,
          agent_id            INTEGER NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
          channel_id          INTEGER NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
          is_admin            INTEGER NOT NULL DEFAULT 0,
          bot_token_override  TEXT,
          max_messages        INTEGER,
          last_active         TEXT,
          created_at          TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS group_messages (
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
        CREATE INDEX IF NOT EXISTS idx_group_msg_ts
          ON group_messages(chat_jid, timestamp);

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

        CREATE TABLE IF NOT EXISTS cowork_workspaces (
          id          TEXT PRIMARY KEY,
          name        TEXT NOT NULL UNIQUE,
          description TEXT,
          status      TEXT NOT NULL DEFAULT 'active',
          root_dir    TEXT NOT NULL,
          working_dir TEXT,
          owner       TEXT NOT NULL,
          created_at  TEXT NOT NULL,
          updated_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS cowork_members (
          workspace_id        TEXT NOT NULL,
          member_id           TEXT NOT NULL,
          member_type         TEXT NOT NULL,
          role                TEXT NOT NULL,
          jid                 TEXT,
          subdir              TEXT,
          persona             TEXT,
          responsibilities    TEXT,
          triggers            TEXT,
          handoff_rules       TEXT,
          acceptance_criteria TEXT,
          output_format       TEXT,
          sla                 TEXT,
          limits              TEXT,
          joined_at           TEXT NOT NULL,
          updated_at          TEXT NOT NULL,
          PRIMARY KEY (workspace_id, member_id)
        );

        CREATE TABLE IF NOT EXISTS cowork_board_entries (
          id           TEXT PRIMARY KEY,
          workspace_id TEXT NOT NULL,
          section      TEXT NOT NULL,
          title        TEXT,
          content      TEXT NOT NULL,
          author       TEXT NOT NULL,
          pinned       INTEGER DEFAULT 0,
          tags         TEXT,
          created_at   TEXT NOT NULL,
          updated_at   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cowork_board_ws
          ON cowork_board_entries(workspace_id, section);

        CREATE TABLE IF NOT EXISTS cowork_tasks (
          id           TEXT PRIMARY KEY,
          workspace_id TEXT NOT NULL,
          title        TEXT NOT NULL,
          description  TEXT,
          status       TEXT NOT NULL DEFAULT 'todo',
          assignee     TEXT,
          reviewer     TEXT,
          priority     TEXT NOT NULL DEFAULT 'medium',
          depends_on   TEXT,
          attachments  TEXT,
          created_by   TEXT NOT NULL,
          created_at   TEXT NOT NULL,
          updated_at   TEXT NOT NULL,
          due_at       TEXT,
          completed_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_cowork_task_ws
          ON cowork_tasks(workspace_id, status);

        CREATE TABLE IF NOT EXISTS cowork_task_comments (
          id         INTEGER PRIMARY KEY AUTOINCREMENT,
          task_id    TEXT NOT NULL,
          author     TEXT NOT NULL,
          content    TEXT NOT NULL,
          created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cowork_task_comment
          ON cowork_task_comments(task_id);

        CREATE TABLE IF NOT EXISTS cowork_messages (
          id           TEXT PRIMARY KEY,
          workspace_id TEXT NOT NULL,
          from_member  TEXT NOT NULL,
          to_member    TEXT,
          message_type TEXT NOT NULL,
          content      TEXT NOT NULL,
          attachments  TEXT,
          task_id      TEXT,
          is_read      INTEGER DEFAULT 0,
          created_at   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_cowork_msg_ws
          ON cowork_messages(workspace_id, created_at);

        CREATE TABLE IF NOT EXISTS cowork_recording_sessions (
          id            TEXT PRIMARY KEY,
          workspace_id  TEXT NOT NULL,
          started_at    TEXT NOT NULL,
          ended_at      TEXT,
          event_count   INTEGER DEFAULT 0,
          total_tokens  INTEGER DEFAULT 0,
          agents        TEXT
        );
        "#,
    )?;

    // Run schema migrations
    run_migrations(conn)?;
    Ok(())
}

fn run_migrations(conn: &Connection) -> Result<()> {
    let group_cols = column_names(conn, "groups")?;
    if !group_cols.iter().any(|c| c == "allowed_work_dirs") {
        conn.execute("ALTER TABLE groups ADD COLUMN allowed_work_dirs TEXT", [])?;
    }
    if !group_cols.iter().any(|c| c == "group_type") {
        conn.execute(
            "ALTER TABLE groups ADD COLUMN group_type TEXT NOT NULL DEFAULT 'chat'",
            [],
        )?;
    }

    let task_cols = column_names(conn, "scheduled_tasks")?;
    if !task_cols.iter().any(|c| c == "script_path") {
        conn.execute(
            "ALTER TABLE scheduled_tasks ADD COLUMN script_path TEXT",
            [],
        )?;
    }

    let agent_cols = column_names(conn, "agents")?;
    if !agent_cols.iter().any(|c| c == "core_prompt") {
        conn.execute(
            "ALTER TABLE agents ADD COLUMN core_prompt TEXT NOT NULL DEFAULT ''",
            [],
        )?;
    }
    if !agent_cols.iter().any(|c| c == "model_id") {
        conn.execute("ALTER TABLE agents ADD COLUMN model_id TEXT", [])?;
    }

    let ws_cols = column_names(conn, "cowork_workspaces")?;
    if !ws_cols.iter().any(|c| c == "working_dir") {
        conn.execute(
            "ALTER TABLE cowork_workspaces ADD COLUMN working_dir TEXT",
            [],
        )?;
    }
    Ok(())
}

pub(crate) fn column_names(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(names)
}

/// Apply Space (personal productivity) tables.
pub(crate) fn apply_space_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS space_notes (
            id          TEXT PRIMARY KEY,
            title       TEXT NOT NULL DEFAULT '',
            body        TEXT NOT NULL DEFAULT '',
            body_html   TEXT,
            tags        TEXT NOT NULL DEFAULT '[]',
            folder_id   TEXT,
            pinned      INTEGER NOT NULL DEFAULT 0,
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL,
            deleted_at  INTEGER
        );

        CREATE TABLE IF NOT EXISTS space_note_folders (
            id          TEXT PRIMARY KEY,
            name        TEXT NOT NULL,
            parent_id   TEXT,
            created_at  INTEGER NOT NULL
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS space_notes_fts USING fts5(
            id UNINDEXED, title, body,
            content=space_notes, content_rowid=rowid
        );

        CREATE TABLE IF NOT EXISTS space_events (
            id          TEXT PRIMARY KEY,
            title       TEXT NOT NULL,
            description TEXT,
            start_at    INTEGER NOT NULL,
            end_at      INTEGER NOT NULL,
            all_day     INTEGER DEFAULT 0,
            location    TEXT,
            color       TEXT,
            recurrence  TEXT,
            reminder_min INTEGER,
            task_id     TEXT,
            source      TEXT DEFAULT 'manual',
            created_at  INTEGER NOT NULL,
            updated_at  INTEGER NOT NULL,
            deleted_at  INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_space_events_start ON space_events(start_at);

        CREATE TABLE IF NOT EXISTS space_email_accounts (
            id          TEXT PRIMARY KEY,
            label       TEXT NOT NULL,
            email       TEXT NOT NULL,
            imap_host   TEXT NOT NULL,
            imap_port   INTEGER NOT NULL DEFAULT 993,
            smtp_host   TEXT NOT NULL,
            smtp_port   INTEGER NOT NULL DEFAULT 587,
            username    TEXT NOT NULL,
            password    TEXT NOT NULL,
            use_tls     INTEGER DEFAULT 1,
            created_at  INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS space_email_cache (
            id          TEXT PRIMARY KEY,
            account_id  TEXT NOT NULL,
            folder      TEXT NOT NULL DEFAULT 'INBOX',
            subject     TEXT,
            from_addr   TEXT,
            to_addrs    TEXT,
            date        INTEGER,
            body_text   TEXT,
            body_html   TEXT,
            flags       TEXT DEFAULT '[]',
            synced_at   INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_space_email_cache_account ON space_email_cache(account_id, folder, date);

        CREATE TABLE IF NOT EXISTS space_apps (
            id          TEXT PRIMARY KEY,
            manifest    TEXT NOT NULL,
            enabled     INTEGER DEFAULT 1,
            installed_at INTEGER NOT NULL,
            last_seen_at INTEGER
        );
        "#,
    )?;
    Ok(())
}

/// Apply memory schema if embedding is enabled.
pub(crate) fn apply_memory_tables(conn: &mut Connection, config: &Config) -> Result<()> {
    let provider = config.memory.embedding_provider;
    let enable_vec = provider != crate::config::EmbeddingProvider::None;
    let dimensions = Config::resolve_dimensions(provider, config.memory.embedding_dimensions);
    let model_name = match provider {
        crate::config::EmbeddingProvider::Openrouter => config.memory.openrouter_model.clone(),
        crate::config::EmbeddingProvider::Ollama => config.memory.ollama_model.clone(),
        crate::config::EmbeddingProvider::Local => {
            let m = config.memory.local_model.clone();
            if m.is_empty() {
                "default".to_owned()
            } else {
                m
            }
        }
        crate::config::EmbeddingProvider::Openai => {
            let m = config.memory.openai_model.clone();
            if m.is_empty() {
                "text-embedding-3-small".to_owned()
            } else {
                m
            }
        }
        crate::config::EmbeddingProvider::None => String::new(),
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
