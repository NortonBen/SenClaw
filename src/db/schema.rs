use anyhow::Result;
use rusqlite::Connection;

use crate::config::Config;
use crate::memory::schema::{apply_memory_schema, build_model_key};

pub(crate) fn apply_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS groups (
          jid                  TEXT PRIMARY KEY,
          -- folder is NOT unique: multiple chat sessions can share the same
          -- agent profile (folder). The old UNIQUE constraint silently broke
          -- every new chat after the first per profile (insert violated
          -- constraint → error swallowed → FE saw a phantom group → user
          -- never got a reply). Migrated away in apply_schema below.
          folder               TEXT NOT NULL,
          name                 TEXT NOT NULL DEFAULT '',
          channel              TEXT NOT NULL DEFAULT 'telegram',
          is_admin             INTEGER NOT NULL DEFAULT 0,
          requires_trigger     INTEGER NOT NULL DEFAULT 1,
          allowed_tools        TEXT,
          allowed_paths        TEXT,
          allowed_work_dirs    TEXT,
          bot_token            TEXT,
          max_messages         INTEGER,
          llm_config_id        TEXT,
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
          attachments  TEXT,
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
          attachments  TEXT,
          PRIMARY KEY (message_id, chat_jid)
        );
        CREATE INDEX IF NOT EXISTS idx_group_msg_ts
          ON group_messages(chat_jid, timestamp);

        -- Tool execution events. Kept in a SEPARATE table from group_messages
        -- so the discriminator (`role: 'tool'`) stays explicit at the storage
        -- layer too. Replayed on subscribe so the chat UI can re-render the
        -- claude-code-style tool cards after a page reload.
        -- Persisted log of every space-event reminder/renotify the
        -- EventNotifier has fired. Today reminders only broadcast as
        -- live WS frames — if the user wasn't connected, they're gone.
        -- The DB row lets reload still surface the notification, and
        -- tracks `delayed_ms` so the UI can flag late reminders (daemon
        -- was down past the trigger time).
        CREATE TABLE IF NOT EXISTS event_notifications (
          id          TEXT PRIMARY KEY,
          event_id    TEXT NOT NULL,
          title       TEXT NOT NULL,
          start_at    INTEGER NOT NULL,
          kind        TEXT NOT NULL,
          fired_at    INTEGER NOT NULL,
          delayed_ms  INTEGER NOT NULL DEFAULT 0,
          read_at     INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_event_notif_fired
          ON event_notifications(fired_at DESC);
        CREATE INDEX IF NOT EXISTS idx_event_notif_event
          ON event_notifications(event_id);

        -- Ephemeral chat events (agent state transitions, permission/question
        -- requests + their resolutions). Replayed on subscribe so the
        -- chat UI can rebuild interactive state after a page reload.
        CREATE TABLE IF NOT EXISTS chat_events (
          id          INTEGER PRIMARY KEY AUTOINCREMENT,
          chat_jid    TEXT NOT NULL,
          event_type  TEXT NOT NULL,
          request_id  TEXT,
          payload     TEXT NOT NULL DEFAULT '{}',
          timestamp   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_chat_events_chat_ts
          ON chat_events(chat_jid, timestamp);
        CREATE INDEX IF NOT EXISTS idx_chat_events_req
          ON chat_events(request_id);

        -- Per-agent TODO snapshot for the Agent Console. Replayed on
        -- admin reconnect so the list survives daemon restart, not just
        -- in-memory cache rebuilds.
        CREATE TABLE IF NOT EXISTS agent_todos (
          agent_jid   TEXT PRIMARY KEY,
          agent_name  TEXT NOT NULL DEFAULT '',
          todos_json  TEXT NOT NULL DEFAULT '[]',
          updated_at  TEXT NOT NULL
        );

        -- Plans produced by ExitPlanMode. Persisted at request time so
        -- the markdown is queryable as history even if the user never
        -- clicks approve. The approval column tracks the eventual response.
        CREATE TABLE IF NOT EXISTS plans (
          id           TEXT PRIMARY KEY,
          chat_jid     TEXT NOT NULL,
          agent_id     TEXT NOT NULL DEFAULT 'main',
          title        TEXT NOT NULL DEFAULT '',
          file_path    TEXT NOT NULL,
          content_md   TEXT NOT NULL,
          approval     TEXT NOT NULL DEFAULT 'pending',
          created_at   TEXT NOT NULL,
          approved_at  TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_plans_chat_ts
          ON plans(chat_jid, created_at DESC);

        -- Tool auto-accept/deny rules. Stored as a JSON blob so new
        -- matcher/action variants in the Rust enum don't need a schema
        -- migration. The server is the source of truth; the browser
        -- localStorage cache mirrors the canonical list.
        CREATE TABLE IF NOT EXISTS tool_rules (
          id          TEXT PRIMARY KEY,
          rule_json   TEXT NOT NULL,
          updated_at  TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS tool_executions (
          id            INTEGER PRIMARY KEY AUTOINCREMENT,
          chat_jid      TEXT NOT NULL,
          agent_id      TEXT NOT NULL DEFAULT 'main',
          tool_name     TEXT NOT NULL,
          title         TEXT NOT NULL DEFAULT '',
          summary       TEXT NOT NULL DEFAULT '',
          content_json  TEXT NOT NULL DEFAULT '{}',
          ok            INTEGER NOT NULL DEFAULT 1,
          timestamp     TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_tool_exec_chat_ts
          ON tool_executions(chat_jid, timestamp);

        -- One-way chat widgets (chart/image/clock/weather) pushed by
        -- `emit_widget`. Persisted so a page reload replays them in
        -- `history:load`. `widget_json` is the full WidgetSpec {kind,title,data}.
        -- Mirrors tool_executions (FIFO-trimmed per chat on insert).
        CREATE TABLE IF NOT EXISTS chat_widgets (
          id            TEXT PRIMARY KEY,
          chat_jid      TEXT NOT NULL,
          widget_json   TEXT NOT NULL DEFAULT '{}',
          created_at    TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_chat_widgets_chat_ts
          ON chat_widgets(chat_jid, created_at);

        CREATE TABLE IF NOT EXISTS dispatch_activity (
          id            INTEGER PRIMARY KEY AUTOINCREMENT,
          task_id       TEXT NOT NULL,
          parent_id     TEXT NOT NULL DEFAULT '',
          entry_type    TEXT NOT NULL DEFAULT 'tool',
          tool_name     TEXT,
          title         TEXT,
          summary       TEXT,
          content_json  TEXT,
          ok            INTEGER,
          text          TEXT,
          ts            TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_dispatch_activity_task
          ON dispatch_activity(task_id, ts);
        CREATE INDEX IF NOT EXISTS idx_dispatch_activity_parent
          ON dispatch_activity(parent_id, ts);

        CREATE TABLE IF NOT EXISTS scheduled_tasks (
          id             TEXT PRIMARY KEY,
          group_folder   TEXT NOT NULL,
          chat_jid       TEXT NOT NULL,
          prompt         TEXT NOT NULL,
          schedule_type  TEXT NOT NULL,
          schedule_value TEXT NOT NULL,
          context_mode   TEXT NOT NULL DEFAULT 'isolated',
          agent_mode     TEXT NOT NULL DEFAULT 'agent',
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

        -- Background tasks: autonomous work SenClaw runs by itself. Kept apart
        -- from scheduled_tasks above, which is the *user's* schedule and runs
        -- in a chat session. See docs/background-tasks-design.md.
        CREATE TABLE IF NOT EXISTS background_tasks (
          id                   TEXT PRIMARY KEY,
          owner_kind           TEXT NOT NULL,
          owner_id             TEXT NOT NULL,
          owner_key            TEXT NOT NULL,
          title                TEXT NOT NULL,
          description          TEXT,
          job_kind             TEXT NOT NULL DEFAULT 'prompt',
          native_job           TEXT,
          prompt_kind          TEXT NOT NULL DEFAULT 'static',
          prompt               TEXT,
          context_url          TEXT,
          persona              TEXT,
          agent_folder         TEXT,
          workspace_dir        TEXT,
          use_tools            TEXT,
          mcp                  TEXT,
          model_id             TEXT,
          max_turns            INTEGER,
          timeout_secs         INTEGER,
          continuity           TEXT NOT NULL DEFAULT 'fresh',
          memory_folder        TEXT,
          trigger_type         TEXT NOT NULL,
          trigger_value        TEXT,
          next_run             TEXT,
          last_run             TEXT,
          overlap_policy       TEXT NOT NULL DEFAULT 'skip',
          catch_up             INTEGER NOT NULL DEFAULT 0,
          max_failures         INTEGER NOT NULL DEFAULT 5,
          consecutive_failures INTEGER NOT NULL DEFAULT 0,
          visibility           TEXT NOT NULL DEFAULT 'normal',
          notify               INTEGER NOT NULL DEFAULT 0,
          status               TEXT NOT NULL DEFAULT 'active',
          created_at           TEXT NOT NULL,
          updated_at           TEXT NOT NULL,
          UNIQUE(owner_id, owner_key)
        );
        CREATE INDEX IF NOT EXISTS idx_bg_due
          ON background_tasks(next_run, status);
        CREATE INDEX IF NOT EXISTS idx_bg_owner
          ON background_tasks(owner_kind, owner_id);

        CREATE TABLE IF NOT EXISTS background_runs (
          id           TEXT PRIMARY KEY,
          task_id      TEXT NOT NULL,
          session_id   TEXT NOT NULL,
          trigger_kind TEXT NOT NULL,
          status       TEXT NOT NULL,
          started_at   TEXT NOT NULL,
          finished_at  TEXT,
          duration_ms  INTEGER,
          turn_count   INTEGER,
          tokens_in    INTEGER,
          tokens_out   INTEGER,
          prompt       TEXT,
          result       TEXT,
          error        TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_bg_runs_task
          ON background_runs(task_id, started_at DESC);
        CREATE INDEX IF NOT EXISTS idx_bg_runs_status
          ON background_runs(status, started_at DESC);

        CREATE TABLE IF NOT EXISTS background_activity (
          id     INTEGER PRIMARY KEY AUTOINCREMENT,
          run_id TEXT NOT NULL,
          ts     TEXT NOT NULL,
          kind   TEXT NOT NULL,
          detail TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_bg_activity_run
          ON background_activity(run_id, id);

        CREATE TABLE IF NOT EXISTS router_state (
          key   TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );

        -- Cowork teams (multi-agent dispatch): a named team with one
        -- "manager" profile (agent folder) plus a JSON list of "member"
        -- profile folders the manager can delegate to via the dispatch
        -- MCP tools. Maps 1:1 to a chat group (jid = "cowork:<id>")
        -- created on first open.
        CREATE TABLE IF NOT EXISTS cowork_teams (
          id              TEXT PRIMARY KEY,
          name            TEXT NOT NULL,
          manager_folder  TEXT NOT NULL,
          members_json    TEXT NOT NULL DEFAULT '[]',
          workspace_dir   TEXT,
          created_at      TEXT NOT NULL
        );

        -- Cowork tasks (manager-tracked work items): one row per task the
        -- team is working on. Status moves through a small kanban — backlog
        -- / todo / in_progress / review / done / blocked. `assignee` and
        -- `reviewer` reference a member folder. `depends_on` is a JSON
        -- array of task ids (free-form dependency graph).
        CREATE TABLE IF NOT EXISTS cowork_team_tasks (
          id               TEXT PRIMARY KEY,
          team_id          TEXT NOT NULL,
          title            TEXT NOT NULL,
          description      TEXT,
          status           TEXT NOT NULL DEFAULT 'todo',
          assignee         TEXT,
          reviewer         TEXT,
          priority         TEXT NOT NULL DEFAULT 'medium',
          depends_on       TEXT NOT NULL DEFAULT '[]',
          result_output    TEXT,
          created_at       TEXT NOT NULL,
          updated_at       TEXT NOT NULL,
          due_at           TEXT,
          completed_at     TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_cowork_team_tasks_team
          ON cowork_team_tasks(team_id);

        -- Custom cowork templates (user-authored squad blueprints). Built-in
        -- templates live in code (`BUILTIN_TEMPLATES`); these are the editable,
        -- user-managed ones. `members_json` is an array of TeamMember specs and
        -- `settings_json` holds CoworkTeamSettings (manager preamble override,
        -- manager tool whitelist, auto-task toggle).
        CREATE TABLE IF NOT EXISTS cowork_templates (
          id              TEXT PRIMARY KEY,
          name            TEXT NOT NULL,
          description     TEXT NOT NULL DEFAULT '',
          icon            TEXT NOT NULL DEFAULT '🧩',
          manager_folder  TEXT NOT NULL,
          manager_role    TEXT NOT NULL DEFAULT 'lead',
          members_json    TEXT NOT NULL DEFAULT '[]',
          settings_json   TEXT NOT NULL DEFAULT '{}',
          created_at      TEXT NOT NULL,
          updated_at      TEXT NOT NULL
        );

        -- Code executor "artifacts": saved, reusable code snippets (JS/TS/Bash)
        -- published from the Code REPL.
        CREATE TABLE IF NOT EXISTS code_artifacts (
          id           TEXT PRIMARY KEY,
          name         TEXT NOT NULL,
          language     TEXT NOT NULL,
          code         TEXT NOT NULL,
          description  TEXT NOT NULL DEFAULT '',
          tags_json    TEXT NOT NULL DEFAULT '[]',
          created_at   TEXT NOT NULL,
          updated_at   TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_code_artifacts_created
          ON code_artifacts(created_at DESC);

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
    if !group_cols.iter().any(|c| c == "llm_config_id") {
        conn.execute("ALTER TABLE groups ADD COLUMN llm_config_id TEXT", [])?;
    }

    // Drop the legacy UNIQUE constraint on `groups.folder`.
    // Old behaviour: only one chat per agent profile (folder), so a new chat
    // for the same profile silently failed on insert and the web UI showed
    // a "Group not found" reply.
    // SQLite can't ALTER a column to drop a constraint — we rebuild the
    // table only when the legacy UNIQUE index is still present.
    // Detect the legacy UNIQUE column by inspecting the table's CREATE
    // statement directly. (Auto-generated UNIQUE indexes are hidden from
    // sqlite_master's `sql` column, so searching for the index doesn't work.)
    let folder_unique: bool = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='groups'",
            [],
            |r| r.get::<_, String>(0),
        )
        .map(|sql| sql.to_uppercase().contains("FOLDER") && sql.to_uppercase().contains("UNIQUE"))
        .unwrap_or(false);
    if folder_unique {
        conn.execute_batch(
            r#"
            BEGIN;
            CREATE TABLE groups_new (
              jid                  TEXT PRIMARY KEY,
              folder               TEXT NOT NULL,
              name                 TEXT NOT NULL DEFAULT '',
              channel              TEXT NOT NULL DEFAULT 'telegram',
              group_type           TEXT NOT NULL DEFAULT 'chat',
              is_admin             INTEGER NOT NULL DEFAULT 0,
              requires_trigger     INTEGER NOT NULL DEFAULT 1,
              allowed_tools        TEXT,
              allowed_paths        TEXT,
              allowed_work_dirs    TEXT,
              bot_token            TEXT,
              max_messages         INTEGER,
              llm_config_id        TEXT,
              last_active          TEXT,
              added_at             TEXT NOT NULL
            );
            INSERT INTO groups_new (jid, folder, name, channel, group_type, is_admin,
              requires_trigger, allowed_tools, allowed_paths, allowed_work_dirs,
              bot_token, max_messages, llm_config_id, last_active, added_at)
            SELECT jid, folder, name, channel,
              COALESCE(group_type, 'chat'),
              is_admin, requires_trigger, allowed_tools, allowed_paths,
              allowed_work_dirs, bot_token, max_messages, llm_config_id,
              last_active, added_at
            FROM groups;
            DROP TABLE groups;
            ALTER TABLE groups_new RENAME TO groups;
            COMMIT;
            "#,
        )?;
    }

    let task_cols = column_names(conn, "scheduled_tasks")?;
    if !task_cols.iter().any(|c| c == "script_path") {
        conn.execute(
            "ALTER TABLE scheduled_tasks ADD COLUMN script_path TEXT",
            [],
        )?;
    }
    if !task_cols.iter().any(|c| c == "agent_mode") {
        conn.execute(
            "ALTER TABLE scheduled_tasks ADD COLUMN agent_mode TEXT NOT NULL DEFAULT 'agent'",
            [],
        )?;
    }
    let bg_cols = column_names(conn, "background_tasks")?;
    if !bg_cols.is_empty() && !bg_cols.iter().any(|c| c == "notify") {
        conn.execute(
            "ALTER TABLE background_tasks ADD COLUMN notify INTEGER NOT NULL DEFAULT 0",
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

    // space_events — add notification tracking columns
    let event_cols = column_names(conn, "space_events").unwrap_or_default();
    for (col, def) in &[
        ("status", "TEXT NOT NULL DEFAULT 'upcoming'"),
        ("reminder_sent_at", "INTEGER"),
        ("renotify_min", "INTEGER"),
        ("renotify_sent_at", "INTEGER"),
        // start_sent_at: tracks the "event is starting now" notification so
        // EVERY event pings at its start time, even without a reminder_min.
        ("start_sent_at", "INTEGER"),
    ] {
        if !event_cols.iter().any(|c| c == col) {
            let _ = conn.execute(
                &format!("ALTER TABLE space_events ADD COLUMN {col} {def}"),
                [],
            );
        }
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
            id UNINDEXED, title, body, tags,
            content=space_notes, content_rowid=rowid
        );

        CREATE TRIGGER IF NOT EXISTS space_notes_ai AFTER INSERT ON space_notes BEGIN
            INSERT INTO space_notes_fts(rowid, id, title, body, tags)
            VALUES (new.rowid, new.id, new.title, new.body, new.tags);
        END;
        CREATE TRIGGER IF NOT EXISTS space_notes_ad AFTER DELETE ON space_notes BEGIN
            INSERT INTO space_notes_fts(space_notes_fts, rowid, id, title, body, tags)
            VALUES ('delete', old.rowid, old.id, old.title, old.body, old.tags);
        END;
        CREATE TRIGGER IF NOT EXISTS space_notes_au AFTER UPDATE ON space_notes BEGIN
            INSERT INTO space_notes_fts(space_notes_fts, rowid, id, title, body, tags)
            VALUES ('delete', old.rowid, old.id, old.title, old.body, old.tags);
            INSERT INTO space_notes_fts(rowid, id, title, body, tags)
            VALUES (new.rowid, new.id, new.title, new.body, new.tags);
        END;

        CREATE TABLE IF NOT EXISTS space_events (
            id              TEXT PRIMARY KEY,
            title           TEXT NOT NULL,
            description     TEXT,
            start_at        INTEGER NOT NULL,
            end_at          INTEGER NOT NULL,
            all_day         INTEGER DEFAULT 0,
            location        TEXT,
            color           TEXT,
            recurrence      TEXT,
            reminder_min    INTEGER,
            task_id         TEXT,
            source          TEXT DEFAULT 'manual',
            status          TEXT NOT NULL DEFAULT 'upcoming',
            reminder_sent_at INTEGER,
            renotify_min    INTEGER,
            renotify_sent_at INTEGER,
            start_sent_at   INTEGER,
            created_at      INTEGER NOT NULL,
            updated_at      INTEGER NOT NULL,
            deleted_at      INTEGER
        );

        CREATE INDEX IF NOT EXISTS idx_space_events_start ON space_events(start_at);

        CREATE TABLE IF NOT EXISTS space_apps (
            id          TEXT PRIMARY KEY,
            manifest    TEXT NOT NULL,
            enabled     INTEGER DEFAULT 1,
            installed_at INTEGER NOT NULL,
            last_seen_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS space_app_config (
            app_id      TEXT NOT NULL,
            key         TEXT NOT NULL,
            value       TEXT NOT NULL,
            updated_at  INTEGER NOT NULL,
            PRIMARY KEY (app_id, key)
        );
        "#,
    )?;

    // Ensure FTS table has the tags column (older databases lack it).
    // Check column count: id, title, body = 3 old; id, title, body, tags = 4 new.
    let needs_rebuild: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('space_notes_fts') WHERE name='tags'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        == 0;

    if needs_rebuild {
        conn.execute_batch(
            r#"
            DROP TABLE IF EXISTS space_notes_fts;
            CREATE VIRTUAL TABLE space_notes_fts USING fts5(
                id UNINDEXED, title, body, tags,
                content=space_notes, content_rowid=rowid
            );
            CREATE TRIGGER IF NOT EXISTS space_notes_ai AFTER INSERT ON space_notes BEGIN
                INSERT INTO space_notes_fts(rowid, id, title, body, tags)
                VALUES (new.rowid, new.id, new.title, new.body, new.tags);
            END;
            CREATE TRIGGER IF NOT EXISTS space_notes_ad AFTER DELETE ON space_notes BEGIN
                INSERT INTO space_notes_fts(space_notes_fts, rowid, id, title, body, tags)
                VALUES ('delete', old.rowid, old.id, old.title, old.body, old.tags);
            END;
            CREATE TRIGGER IF NOT EXISTS space_notes_au AFTER UPDATE ON space_notes BEGIN
                INSERT INTO space_notes_fts(space_notes_fts, rowid, id, title, body, tags)
                VALUES ('delete', old.rowid, old.id, old.title, old.body, old.tags);
                INSERT INTO space_notes_fts(rowid, id, title, body, tags)
                VALUES (new.rowid, new.id, new.title, new.body, new.tags);
            END;
            INSERT INTO space_notes_fts(space_notes_fts) VALUES('rebuild');
            "#,
        )?;
    } else {
        // Triggers use IF NOT EXISTS, so just rebuild to fill any gaps.
        conn.execute_batch("INSERT INTO space_notes_fts(space_notes_fts) VALUES('rebuild');")?;
    }

    Ok(())
}

/// Marketplace tables — installed_skills, installed_plugins, plugin_runtime.
pub(crate) fn apply_marketplace_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS installed_skills (
            slug            TEXT PRIMARY KEY,
            display_name    TEXT,
            summary         TEXT,
            version         TEXT NOT NULL DEFAULT '',
            registry        TEXT NOT NULL DEFAULT 'https://lightmake.site',
            source          TEXT NOT NULL DEFAULT 'clawhub',
            enabled         INTEGER NOT NULL DEFAULT 1,
            installed_at    INTEGER NOT NULL DEFAULT 0,
            updated_at      INTEGER NOT NULL DEFAULT 0,
            manifest_json   TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_installed_skills_enabled
            ON installed_skills(enabled, source);

        CREATE TABLE IF NOT EXISTS installed_plugins (
            slug            TEXT PRIMARY KEY,
            display_name    TEXT,
            summary         TEXT,
            version         TEXT NOT NULL DEFAULT '',
            plugin_type     TEXT NOT NULL DEFAULT 'mcp_server',
            registry        TEXT NOT NULL DEFAULT 'https://lightmake.site',
            enabled         INTEGER NOT NULL DEFAULT 1,
            installed_at    INTEGER NOT NULL DEFAULT 0,
            updated_at      INTEGER NOT NULL DEFAULT 0,
            config_json     TEXT NOT NULL DEFAULT '{}',
            manifest_json   TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_installed_plugins_type
            ON installed_plugins(plugin_type, enabled);

        CREATE TABLE IF NOT EXISTS plugin_runtime (
            slug        TEXT PRIMARY KEY REFERENCES installed_plugins(slug) ON DELETE CASCADE,
            status      TEXT NOT NULL DEFAULT 'stopped',
            pid         INTEGER,
            port        INTEGER,
            started_at  INTEGER,
            error_msg   TEXT,
            last_ping   INTEGER
        );
        "#,
    )?;

    // Migration: Add attachments column if it doesn't exist
    conn.execute(
        "ALTER TABLE channel_messages ADD COLUMN attachments TEXT",
        [],
    )
    .ok(); // Ignore error if column already exists
    conn.execute("ALTER TABLE group_messages ADD COLUMN attachments TEXT", [])
        .ok(); // Ignore error if column already exists

    // Cowork team behaviour settings (manager preamble override, manager tool
    // whitelist, auto-task toggle). Added after the table shipped, so guard it.
    if let Ok(cols) = column_names(conn, "cowork_teams") {
        if !cols.iter().any(|c| c == "settings_json") {
            conn.execute(
                "ALTER TABLE cowork_teams ADD COLUMN settings_json TEXT NOT NULL DEFAULT '{}'",
                [],
            )
            .ok();
        }
    }

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
