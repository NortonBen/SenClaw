use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// SQLite store for the AI Office app: `agents` (the virtual staff roster),
/// `tasks` (one row per assignment from the boss), `steps` (per-agent slices of
/// a task) and `events` (the chat/handoff activity feed rendered by the UI).
pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS agents (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  key         TEXT NOT NULL UNIQUE,
  name        TEXT NOT NULL,
  role        TEXT NOT NULL DEFAULT '',
  duty        TEXT NOT NULL DEFAULT '',
  kind        TEXT NOT NULL DEFAULT 'worker',
  enabled     INTEGER NOT NULL DEFAULT 1,
  auto_assign INTEGER NOT NULL DEFAULT 1,
  skills      TEXT NOT NULL DEFAULT '[]',
  status      TEXT NOT NULL DEFAULT 'idle',
  status_note TEXT NOT NULL DEFAULT '',
  sort        INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS tasks (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  title       TEXT NOT NULL,
  mode        TEXT NOT NULL DEFAULT 'demo',
  status      TEXT NOT NULL DEFAULT 'pending',
  report      TEXT NOT NULL DEFAULT '',
  llm_calls   INTEGER NOT NULL DEFAULT 0,
  llm_model   TEXT NOT NULL DEFAULT '',
  tokens_in   INTEGER NOT NULL DEFAULT 0,
  tokens_out  INTEGER NOT NULL DEFAULT 0,
  created_at  INTEGER NOT NULL,
  finished_at INTEGER
);
CREATE TABLE IF NOT EXISTS steps (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id     INTEGER NOT NULL,
  agent_key   TEXT NOT NULL,
  title       TEXT NOT NULL,
  status      TEXT NOT NULL DEFAULT 'pending',
  result      TEXT NOT NULL DEFAULT '',
  ord         INTEGER NOT NULL DEFAULT 0,
  started_at  INTEGER,
  finished_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_steps_task ON steps(task_id);
CREATE TABLE IF NOT EXISTS events (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  task_id    INTEGER,
  kind       TEXT NOT NULL,
  actor      TEXT NOT NULL DEFAULT '',
  target     TEXT NOT NULL DEFAULT '',
  text       TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_task ON events(task_id);
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
"#;

/// Additive migrations applied to pre-existing DBs (errors ignored).
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE tasks ADD COLUMN tokens_in INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE tasks ADD COLUMN tokens_out INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE agents ADD COLUMN kind TEXT NOT NULL DEFAULT 'worker'",
    // Stamp kinds onto the default roster of pre-existing DBs.
    "UPDATE agents SET kind='manager' WHERE key='truong-phong' AND kind='worker'",
    "UPDATE agents SET kind='qa' WHERE key='kiem-dinh' AND kind='worker'",
    "ALTER TABLE agents ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
    "ALTER TABLE agents ADD COLUMN auto_assign INTEGER NOT NULL DEFAULT 1",
    "ALTER TABLE agents ADD COLUMN skills TEXT NOT NULL DEFAULT '[]'",
];

/// The default staff of the one-person company, mirroring the reel:
/// manager, research, content, analysis, QA. `(key, name, role, duty, kind)`.
pub const DEFAULT_AGENTS: &[(&str, &str, &str, &str, &str)] = &[
    (
        "truong-phong",
        "TRƯỞNG PHÒNG",
        "Điều phối & tổng hợp",
        "Nhận nhiệm vụ từ Sếp, phân công cho anh em, giám sát bàn giao và nộp báo cáo tổng hợp.",
        "manager",
    ),
    (
        "nghien-cuu",
        "NGHIÊN CỨU",
        "Thu thập & phân tích thông tin",
        "Phân tích đề bài, thu thập dữ kiện nền và chuẩn bị đầu vào cho cả phòng.",
        "worker",
    ),
    (
        "noi-dung",
        "NỘI DUNG",
        "Viết & biên tập",
        "Triển khai phần việc chính dựa trên đầu vào của Nghiên cứu.",
        "worker",
    ),
    (
        "phan-tich",
        "PHÂN TÍCH",
        "Số liệu, logic, đánh giá",
        "Rà soát tính logic, bổ sung số liệu và hoàn thiện kết quả.",
        "worker",
    ),
    (
        "kiem-dinh",
        "KIỂM ĐỊNH",
        "Giám sát chất lượng & rủi ro",
        "Soát lỗi, chỉ ra rủi ro và xác nhận chất lượng trước khi bàn giao Trưởng phòng.",
        "qa",
    ),
];

/// The office floor only has this many desks (plus the boss's).
pub const MAX_AGENTS: usize = 7;

#[derive(Serialize, Clone)]
pub struct Agent {
    pub key: String,
    pub name: String,
    pub role: String,
    pub duty: String,
    pub kind: String,
    /// Disabled staff keep their desk but are excluded from every pipeline.
    pub enabled: bool,
    /// "Tự nhận nhiệm vụ": when true the worker is always included in the
    /// plan; when false the manager only assigns them if their specialty is
    /// genuinely needed.
    pub auto_assign: bool,
    /// Names of skills / sub-agents (personas) this staff member holds —
    /// injected into their working context on LIVE runs.
    pub skills: Vec<String>,
    pub status: String,
    pub status_note: String,
    pub sort: i64,
}

fn skills_from_json(raw: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

#[derive(Serialize, Clone)]
pub struct Task {
    pub id: i64,
    pub title: String,
    pub mode: String,
    pub status: String,
    pub report: String,
    pub llm_calls: i64,
    pub llm_model: String,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Serialize, Clone)]
pub struct Step {
    pub id: i64,
    pub task_id: i64,
    pub agent_key: String,
    pub title: String,
    pub status: String,
    pub result: String,
    pub ord: i64,
}

#[derive(Serialize, Clone)]
pub struct Event {
    pub id: i64,
    pub task_id: Option<i64>,
    pub kind: String,
    pub actor: String,
    pub target: String,
    pub text: String,
    pub created_at: i64,
}

/// ASCII slug for agent keys: strips Vietnamese diacritics, lowercases,
/// collapses everything else to '-'.
pub fn slugify(s: &str) -> String {
    const FROM: &str = "àáạảãâầấậẩẫăằắặẳẵèéẹẻẽêềếệểễìíịỉĩòóọỏõôồốộổỗơờớợởỡùúụủũưừứựửữỳýỵỷỹđ";
    const TO: &str = "aaaaaaaaaaaaaaaaaeeeeeeeeeeeiiiiiooooooooooooooooouuuuuuuuuuuyyyyyd";
    let to: Vec<char> = TO.chars().collect();
    let from: Vec<char> = FROM.chars().collect();
    let mut out = String::new();
    for ch in s.to_lowercase().chars() {
        let ch = from.iter().position(|&f| f == ch).map(|i| to[i]).unwrap_or(ch);
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "nhan-su".to_string() } else { out }
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `~/...` → absolute path under $HOME.
pub fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home).join(rest)
    } else {
        PathBuf::from(p)
    }
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
        db.seed_agents()?;
        Ok(db)
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        f(&conn)
    }

    fn seed_agents(&self) -> Result<()> {
        self.with(|c| {
            let count: i64 = c.query_row("SELECT COUNT(*) FROM agents", [], |r| r.get(0))?;
            if count == 0 {
                for (i, (key, name, role, duty, kind)) in DEFAULT_AGENTS.iter().enumerate() {
                    c.execute(
                        "INSERT INTO agents(key,name,role,duty,kind,sort) VALUES(?1,?2,?3,?4,?5,?6)",
                        params![key, name, role, duty, kind, i as i64],
                    )?;
                }
            }
            Ok(())
        })
    }

    // ---- agents ----

    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT key,name,role,duty,kind,enabled,auto_assign,skills,status,status_note,sort
                 FROM agents ORDER BY sort",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(Agent {
                        key: r.get(0)?,
                        name: r.get(1)?,
                        role: r.get(2)?,
                        duty: r.get(3)?,
                        kind: r.get(4)?,
                        enabled: r.get::<_, i64>(5)? != 0,
                        auto_assign: r.get::<_, i64>(6)? != 0,
                        skills: skills_from_json(&r.get::<_, String>(7)?),
                        status: r.get(8)?,
                        status_note: r.get(9)?,
                        sort: r.get(10)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Create a staff member. Generates an ASCII slug key from the name and
    /// enforces the desk limit plus one-manager/one-QA invariants upstream.
    pub fn add_agent(&self, name: &str, role: &str, duty: &str, kind: &str) -> Result<Agent> {
        let agents = self.list_agents()?;
        if agents.len() >= MAX_AGENTS {
            anyhow::bail!("văn phòng chỉ có {} bàn — xoá bớt nhân sự trước", MAX_AGENTS);
        }
        let base = slugify(name);
        let mut key = base.clone();
        let mut n = 2;
        while agents.iter().any(|a| a.key == key) {
            key = format!("{}-{}", base, n);
            n += 1;
        }
        let sort = agents.iter().map(|a| a.sort).max().unwrap_or(-1) + 1;
        self.with(|c| {
            c.execute(
                "INSERT INTO agents(key,name,role,duty,kind,sort) VALUES(?1,?2,?3,?4,?5,?6)",
                params![key, name, role, duty, kind, sort],
            )?;
            Ok(())
        })?;
        Ok(Agent {
            key,
            name: name.to_string(),
            role: role.to_string(),
            duty: duty.to_string(),
            kind: kind.to_string(),
            enabled: true,
            auto_assign: true,
            skills: Vec::new(),
            status: "idle".into(),
            status_note: String::new(),
            sort,
        })
    }

    pub fn delete_agent(&self, key: &str) -> Result<bool> {
        self.with(|c| {
            let n = c.execute("DELETE FROM agents WHERE key=?1", params![key])?;
            Ok(n > 0)
        })
    }

    pub fn get_agent(&self, key: &str) -> Result<Option<Agent>> {
        Ok(self.list_agents()?.into_iter().find(|a| a.key == key))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_agent(
        &self,
        key: &str,
        name: Option<&str>,
        role: Option<&str>,
        duty: Option<&str>,
        enabled: Option<bool>,
        auto_assign: Option<bool>,
        skills: Option<&[String]>,
    ) -> Result<bool> {
        let skills_json = match skills {
            Some(s) => Some(serde_json::to_string(s)?),
            None => None,
        };
        self.with(|c| {
            let n = c.execute(
                "UPDATE agents SET
                    name = COALESCE(?2, name),
                    role = COALESCE(?3, role),
                    duty = COALESCE(?4, duty),
                    enabled = COALESCE(?5, enabled),
                    auto_assign = COALESCE(?6, auto_assign),
                    skills = COALESCE(?7, skills)
                 WHERE key = ?1",
                params![
                    key,
                    name,
                    role,
                    duty,
                    enabled.map(|b| b as i64),
                    auto_assign.map(|b| b as i64),
                    skills_json
                ],
            )?;
            Ok(n > 0)
        })
    }

    pub fn set_agent_status(&self, key: &str, status: &str, note: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE agents SET status=?2, status_note=?3 WHERE key=?1",
                params![key, status, note],
            )?;
            Ok(())
        })
    }

    pub fn reset_agent_statuses(&self) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE agents SET status='idle', status_note=''", [])?;
            Ok(())
        })
    }

    // ---- tasks ----

    pub fn create_task(&self, title: &str, mode: &str) -> Result<Task> {
        let ts = now();
        self.with(|c| {
            c.execute(
                "INSERT INTO tasks(title,mode,status,created_at) VALUES(?1,?2,'pending',?3)",
                params![title, mode, ts],
            )?;
            let id = c.last_insert_rowid();
            Ok(Task {
                id,
                title: title.to_string(),
                mode: mode.to_string(),
                status: "pending".into(),
                report: String::new(),
                llm_calls: 0,
                llm_model: String::new(),
                tokens_in: 0,
                tokens_out: 0,
                created_at: ts,
                finished_at: None,
            })
        })
    }

    fn row_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
        Ok(Task {
            id: r.get(0)?,
            title: r.get(1)?,
            mode: r.get(2)?,
            status: r.get(3)?,
            report: r.get(4)?,
            llm_calls: r.get(5)?,
            llm_model: r.get(6)?,
            tokens_in: r.get(7)?,
            tokens_out: r.get(8)?,
            created_at: r.get(9)?,
            finished_at: r.get(10)?,
        })
    }

    const TASK_COLS: &'static str =
        "id,title,mode,status,report,llm_calls,llm_model,tokens_in,tokens_out,created_at,finished_at";

    pub fn get_task(&self, id: i64) -> Result<Option<Task>> {
        self.with(|c| {
            let t = c
                .query_row(
                    &format!("SELECT {} FROM tasks WHERE id=?1", Self::TASK_COLS),
                    params![id],
                    Self::row_task,
                )
                .optional()?;
            Ok(t)
        })
    }

    pub fn latest_task(&self) -> Result<Option<Task>> {
        self.with(|c| {
            let t = c
                .query_row(
                    &format!("SELECT {} FROM tasks ORDER BY id DESC LIMIT 1", Self::TASK_COLS),
                    [],
                    Self::row_task,
                )
                .optional()?;
            Ok(t)
        })
    }

    pub fn list_tasks(&self, limit: i64) -> Result<Vec<Task>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM tasks ORDER BY id DESC LIMIT ?1",
                Self::TASK_COLS
            ))?;
            let rows = stmt
                .query_map(params![limit], Self::row_task)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn set_task_status(&self, id: i64, status: &str) -> Result<()> {
        self.with(|c| {
            let finished = matches!(status, "done" | "error");
            if finished {
                c.execute(
                    "UPDATE tasks SET status=?2, finished_at=?3 WHERE id=?1",
                    params![id, status, now()],
                )?;
            } else {
                c.execute("UPDATE tasks SET status=?2 WHERE id=?1", params![id, status])?;
            }
            Ok(())
        })
    }

    pub fn set_task_report(&self, id: i64, report: &str) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE tasks SET report=?2 WHERE id=?1", params![id, report])?;
            Ok(())
        })
    }

    pub fn bump_llm(&self, id: i64, model: &str, tokens_in: i64, tokens_out: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE tasks SET llm_calls=llm_calls+1, llm_model=?2,
                        tokens_in=tokens_in+?3, tokens_out=tokens_out+?4 WHERE id=?1",
                params![id, model, tokens_in, tokens_out],
            )?;
            Ok(())
        })
    }

    /// A task is actively being worked (not merely queued as `pending`).
    pub fn has_running_task(&self) -> Result<bool> {
        self.with(|c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM tasks WHERE status IN ('planning','running','review')",
                [],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
    }

    /// Oldest queued task id waiting to run (FIFO).
    pub fn next_pending(&self) -> Result<Option<i64>> {
        self.with(|c| {
            let id = c
                .query_row(
                    "SELECT id FROM tasks WHERE status='pending' ORDER BY id LIMIT 1",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?;
            Ok(id)
        })
    }

    /// Queued (not-yet-started) tasks, oldest first.
    pub fn pending_tasks(&self) -> Result<Vec<Task>> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM tasks WHERE status='pending' ORDER BY id",
                Self::TASK_COLS
            ))?;
            let rows = stmt
                .query_map([], Self::row_task)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Active tasks left mid-flight by a previous process are failed on boot;
    /// queued (`pending`) tasks are kept so they resume.
    pub fn fail_stale_running(&self) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE tasks SET status='error', finished_at=?1 WHERE status IN ('planning','running','review')",
                params![now()],
            )?;
            Ok(())
        })
    }

    // ---- feature toggles (stored in settings) ----

    /// Feature flag, default ON when unset. Keys: memory, wiki, workspace,
    /// tools, autocontinue.
    pub fn feature(&self, key: &str) -> bool {
        match self.get_setting(&format!("feat_{key}")) {
            Ok(Some(v)) => v != "0",
            _ => true,
        }
    }

    pub fn features_json(&self) -> serde_json::Value {
        serde_json::json!({
            "memory": self.feature("memory"),
            "wiki": self.feature("wiki"),
            "workspace": self.feature("workspace"),
            "tools": self.feature("tools"),
            "autocontinue": self.feature("autocontinue"),
        })
    }

    // ---- steps ----

    pub fn add_step(&self, task_id: i64, agent_key: &str, title: &str, ord: i64) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO steps(task_id,agent_key,title,ord) VALUES(?1,?2,?3,?4)",
                params![task_id, agent_key, title, ord],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn list_steps(&self, task_id: i64) -> Result<Vec<Step>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id,task_id,agent_key,title,status,result,ord FROM steps WHERE task_id=?1 ORDER BY ord",
            )?;
            let rows = stmt
                .query_map(params![task_id], |r| {
                    Ok(Step {
                        id: r.get(0)?,
                        task_id: r.get(1)?,
                        agent_key: r.get(2)?,
                        title: r.get(3)?,
                        status: r.get(4)?,
                        result: r.get(5)?,
                        ord: r.get(6)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn set_step_status(&self, id: i64, status: &str) -> Result<()> {
        self.with(|c| {
            match status {
                "working" => c.execute(
                    "UPDATE steps SET status=?2, started_at=?3 WHERE id=?1",
                    params![id, status, now()],
                )?,
                "done" | "error" => c.execute(
                    "UPDATE steps SET status=?2, finished_at=?3 WHERE id=?1",
                    params![id, status, now()],
                )?,
                _ => c.execute("UPDATE steps SET status=?2 WHERE id=?1", params![id, status])?,
            };
            Ok(())
        })
    }

    pub fn set_step_result(&self, id: i64, result: &str) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE steps SET result=?2 WHERE id=?1", params![id, result])?;
            Ok(())
        })
    }

    // ---- events ----

    pub fn add_event(
        &self,
        task_id: Option<i64>,
        kind: &str,
        actor: &str,
        target: &str,
        text: &str,
    ) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO events(task_id,kind,actor,target,text,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![task_id, kind, actor, target, text, now()],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn list_events(&self, task_id: Option<i64>, after: i64, limit: i64) -> Result<Vec<Event>> {
        self.with(|c| {
            let (sql, p): (String, Vec<Box<dyn rusqlite::ToSql>>) = match task_id {
                Some(tid) => (
                    "SELECT id,task_id,kind,actor,target,text,created_at FROM events
                     WHERE task_id=?1 AND id>?2 ORDER BY id LIMIT ?3"
                        .into(),
                    vec![Box::new(tid), Box::new(after), Box::new(limit)],
                ),
                None => (
                    "SELECT id,task_id,kind,actor,target,text,created_at FROM events
                     WHERE id>?1 ORDER BY id LIMIT ?2"
                        .into(),
                    vec![Box::new(after), Box::new(limit)],
                ),
            };
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(p.iter().map(|b| b.as_ref())), |r| {
                    Ok(Event {
                        id: r.get(0)?,
                        task_id: r.get(1)?,
                        kind: r.get(2)?,
                        actor: r.get(3)?,
                        target: r.get(4)?,
                        text: r.get(5)?,
                        created_at: r.get(6)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn recent_events(&self, limit: i64) -> Result<Vec<Event>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id,task_id,kind,actor,target,text,created_at FROM events
                 ORDER BY id DESC LIMIT ?1",
            )?;
            let mut rows = stmt
                .query_map(params![limit], |r| {
                    Ok(Event {
                        id: r.get(0)?,
                        task_id: r.get(1)?,
                        kind: r.get(2)?,
                        actor: r.get(3)?,
                        target: r.get(4)?,
                        text: r.get(5)?,
                        created_at: r.get(6)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows.reverse();
            Ok(rows)
        })
    }

    // ---- settings ----

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.with(|c| {
            let v = c
                .query_row("SELECT value FROM settings WHERE key=?1", params![key], |r| {
                    r.get::<_, String>(0)
                })
                .optional()?;
            Ok(v)
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

    /// The office's shared document folder: Sếp drops input files here and
    /// staff write their deliverables back. Defaults under the app data dir.
    pub fn workspace_dir(&self) -> PathBuf {
        match self.get_setting("workspace_dir") {
            Ok(Some(v)) if !v.trim().is_empty() => expand_home(v.trim()),
            _ => default_data_dir("ai-office").join("workspace"),
        }
    }

    // ---- stats (Kế toán) ----

    pub fn stats(&self) -> Result<serde_json::Value> {
        self.with(|c| {
            let total: i64 = c.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
            let done: i64 =
                c.query_row("SELECT COUNT(*) FROM tasks WHERE status='done'", [], |r| r.get(0))?;
            let live: i64 =
                c.query_row("SELECT COUNT(*) FROM tasks WHERE mode='live'", [], |r| r.get(0))?;
            let llm_calls: i64 =
                c.query_row("SELECT COALESCE(SUM(llm_calls),0) FROM tasks", [], |r| r.get(0))?;
            let (tokens_in, tokens_out): (i64, i64) = c.query_row(
                "SELECT COALESCE(SUM(tokens_in),0), COALESCE(SUM(tokens_out),0) FROM tasks",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let model: String = c
                .query_row(
                    "SELECT llm_model FROM tasks WHERE llm_model!='' ORDER BY id DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .optional()?
                .unwrap_or_default();
            Ok(serde_json::json!({
                "tasksTotal": total,
                "tasksDone": done,
                "tasksLive": live,
                "llmCalls": llm_calls,
                "tokensIn": tokens_in,
                "tokensOut": tokens_out,
                "lastModel": model,
            }))
        })
    }
}
