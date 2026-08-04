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
CREATE TABLE IF NOT EXISTS teams (
  key         TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  sort        INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS agents (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  key         TEXT NOT NULL UNIQUE,
  name        TEXT NOT NULL,
  role        TEXT NOT NULL DEFAULT '',
  duty        TEXT NOT NULL DEFAULT '',
  kind        TEXT NOT NULL DEFAULT 'worker',
  team        TEXT NOT NULL DEFAULT '',
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
  team        TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'pending',
  report      TEXT NOT NULL DEFAULT '',
  llm_calls   INTEGER NOT NULL DEFAULT 0,
  llm_model   TEXT NOT NULL DEFAULT '',
  tokens_in   INTEGER NOT NULL DEFAULT 0,
  tokens_out  INTEGER NOT NULL DEFAULT 0,
  goal_id     INTEGER,
  approval    TEXT NOT NULL DEFAULT '',
  approved_at INTEGER,
  boss_note   TEXT NOT NULL DEFAULT '',
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
CREATE TABLE IF NOT EXISTS goals (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  title       TEXT NOT NULL,
  quarter     TEXT NOT NULL DEFAULT '',
  key_results TEXT NOT NULL DEFAULT '[]',
  archived    INTEGER NOT NULL DEFAULT 0,
  created_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS meetings (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  kind       TEXT NOT NULL,
  day        TEXT NOT NULL,
  content    TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE(kind, day)
);
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
    "ALTER TABLE agents ADD COLUMN team TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE tasks ADD COLUMN team TEXT NOT NULL DEFAULT ''",
    // Bảng việc (kanban) + vòng duyệt của Sếp.
    "ALTER TABLE tasks ADD COLUMN goal_id INTEGER",
    "ALTER TABLE tasks ADD COLUMN approval TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE tasks ADD COLUMN approved_at INTEGER",
    "ALTER TABLE tasks ADD COLUMN boss_note TEXT NOT NULL DEFAULT ''",
];

/// A member of a team's default roster: `(name, role, duty, kind)`.
type RosterMember = (&'static str, &'static str, &'static str, &'static str);

/// The AI Office ships with several teams, each an independent crew (one
/// manager, specialist workers, one QA) that the CEO (user) commands.
/// `(key, name, description, roster)`.
pub const DEFAULT_TEAMS: &[(&str, &str, &str, &[RosterMember])] = &[
    (
        "nctt",
        "NGHIÊN CỨU THỊ TRƯỜNG",
        "Đội nghiên cứu thị trường, đối thủ, hành vi khách hàng và cơ hội kinh doanh.",
        &[
            (
                "TRƯỞNG NHÓM",
                "Điều phối & tổng hợp",
                "Nhận nhiệm vụ từ Sếp, phân công cho đội và nộp báo cáo tổng hợp.",
                "manager",
            ),
            (
                "NGHIÊN CỨU",
                "Thu thập & phân tích thông tin",
                "Phân tích đề bài, thu thập dữ kiện thị trường làm đầu vào cho đội.",
                "worker",
            ),
            (
                "PHÂN TÍCH",
                "Số liệu, logic, đánh giá",
                "Rà soát logic, bổ sung số liệu và hoàn thiện kết quả nghiên cứu.",
                "worker",
            ),
            (
                "KIỂM ĐỊNH",
                "Giám sát chất lượng & rủi ro",
                "Soát lỗi, chỉ ra rủi ro trước khi bàn giao Trưởng nhóm.",
                "qa",
            ),
        ],
    ),
    (
        "ptud",
        "PHÁT TRIỂN ỨNG DỤNG",
        "Đội phát triển sản phẩm/ứng dụng: thiết kế, lập trình, kiểm thử.",
        &[
            (
                "TRƯỞNG NHÓM",
                "Điều phối & tổng hợp",
                "Nhận yêu cầu từ Sếp, chia việc cho đội và tổng hợp kết quả bàn giao.",
                "manager",
            ),
            (
                "THIẾT KẾ",
                "Thiết kế & trải nghiệm",
                "Phác thảo giao diện, luồng người dùng và trải nghiệm sản phẩm.",
                "worker",
            ),
            (
                "LẬP TRÌNH",
                "Phát triển tính năng",
                "Triển khai tính năng, mô tả kỹ thuật và giải pháp khả thi.",
                "worker",
            ),
            (
                "KIỂM THỬ",
                "Kiểm thử & chất lượng",
                "Soát lỗi, rủi ro kỹ thuật và xác nhận chất lượng trước khi bàn giao.",
                "qa",
            ),
        ],
    ),
    (
        "dltk",
        "DỮ LIỆU & THỐNG KÊ",
        "Đội tìm kiếm, tổng hợp và thống kê dữ liệu để ra quyết định.",
        &[
            (
                "TRƯỞNG NHÓM",
                "Điều phối & tổng hợp",
                "Nhận nhiệm vụ dữ liệu từ Sếp, phân công và tổng hợp báo cáo.",
                "manager",
            ),
            (
                "THU THẬP DL",
                "Tìm kiếm & thu thập dữ liệu",
                "Tìm nguồn, thu thập và làm sạch dữ liệu cho đội.",
                "worker",
            ),
            (
                "THỐNG KÊ",
                "Thống kê & trực quan hoá",
                "Phân tích thống kê, rút ra xu hướng và trực quan hoá số liệu.",
                "worker",
            ),
            (
                "KIỂM ĐỊNH DL",
                "Giám sát chất lượng dữ liệu",
                "Xác minh độ chính xác, chỉ ra sai lệch trước khi bàn giao.",
                "qa",
            ),
        ],
    ),
];

/// Each team's floor only has this many desks (plus the boss's).
pub const MAX_AGENTS: usize = 7;

#[derive(Serialize, Clone)]
pub struct Team {
    pub key: String,
    pub name: String,
    pub description: String,
    pub sort: i64,
}

#[derive(Serialize, Clone)]
pub struct Agent {
    pub key: String,
    pub name: String,
    pub role: String,
    pub duty: String,
    pub kind: String,
    /// Which team this staff member belongs to (team key).
    pub team: String,
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
    pub team: String,
    pub status: String,
    pub report: String,
    pub llm_calls: i64,
    pub llm_model: String,
    pub tokens_in: i64,
    pub tokens_out: i64,
    /// Mục tiêu quý mà việc này phục vụ (NULL = "lạc hướng" trên bảng việc).
    pub goal_id: Option<i64>,
    /// Vòng duyệt của Sếp: '' (chưa vào vòng duyệt / việc cũ), 'waiting'
    /// (AI xong, chờ Sếp nghiệm thu), 'approved' (Sếp duyệt — Hoàn tất),
    /// 'returned' (Sếp trả lại kèm boss_note — đã xếp chạy lại).
    pub approval: String,
    pub approved_at: Option<i64>,
    /// Ghi chú của Sếp khi trả lại — được đưa vào context lần chạy lại.
    pub boss_note: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

/// Một kết quả then chốt (key result) của mục tiêu quý.
#[derive(Serialize, serde::Deserialize, Clone)]
pub struct KeyResult {
    pub text: String,
    pub done: bool,
}

/// Mục tiêu quý (OKR-style): tiến độ = tỷ lệ key result đã tick.
#[derive(Serialize, Clone)]
pub struct Goal {
    pub id: i64,
    pub title: String,
    pub quarter: String,
    pub key_results: Vec<KeyResult>,
    pub archived: bool,
    pub created_at: i64,
    /// 0–100, suy ra từ key_results.
    pub progress: i64,
}

fn goal_progress(krs: &[KeyResult]) -> i64 {
    if krs.is_empty() {
        return 0;
    }
    let done = krs.iter().filter(|k| k.done).count();
    (done as i64 * 100) / krs.len() as i64
}

/// Biên bản họp điều hành: kind 'morning' (họp sáng) / 'evening' (họp tối),
/// mỗi ngày mỗi loại một bản (họp lại = ghi đè).
#[derive(Serialize, Clone)]
pub struct Meeting {
    pub id: i64,
    pub kind: String,
    pub day: String,
    pub content: String,
    pub created_at: i64,
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
        let ch = from
            .iter()
            .position(|&f| f == ch)
            .map(|i| to[i])
            .unwrap_or(ch);
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "nhan-su".to_string()
    } else {
        out
    }
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
        let db = Self {
            conn: Mutex::new(conn),
        };
        db.seed()?;
        Ok(db)
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().expect("db mutex poisoned");
        f(&conn)
    }

    /// Ensure the default teams exist, adopt any team-less legacy agents into
    /// the first team, and seed a fresh roster for any team that has none.
    fn seed(&self) -> Result<()> {
        self.with(|c| {
            // 1. teams
            let team_count: i64 = c.query_row("SELECT COUNT(*) FROM teams", [], |r| r.get(0))?;
            if team_count == 0 {
                for (i, (key, name, desc, _)) in DEFAULT_TEAMS.iter().enumerate() {
                    c.execute(
                        "INSERT INTO teams(key,name,description,sort) VALUES(?1,?2,?3,?4)",
                        params![key, name, desc, i as i64],
                    )?;
                }
            }
            let first_team: String = c.query_row("SELECT key FROM teams ORDER BY sort LIMIT 1", [], |r| r.get(0))?;
            // 2. legacy agents with no team → first team
            c.execute("UPDATE agents SET team=?1 WHERE team=''", params![first_team])?;
            // 3. seed roster for any team that is empty
            for (tkey, _, _, roster) in DEFAULT_TEAMS.iter() {
                let n: i64 = c.query_row(
                    "SELECT COUNT(*) FROM agents WHERE team=?1",
                    params![tkey],
                    |r| r.get(0),
                )?;
                if n == 0 {
                    for (i, (name, role, duty, kind)) in roster.iter().enumerate() {
                        let key = format!("{}-{}", tkey, slugify(name));
                        c.execute(
                            "INSERT OR IGNORE INTO agents(key,name,role,duty,kind,team,sort) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                            params![key, name, role, duty, kind, tkey, i as i64],
                        )?;
                    }
                }
            }
            Ok(())
        })
    }

    // ---- teams ----

    pub fn list_teams(&self) -> Result<Vec<Team>> {
        self.with(|c| {
            let mut stmt =
                c.prepare("SELECT key,name,description,sort FROM teams ORDER BY sort")?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(Team {
                        key: r.get(0)?,
                        name: r.get(1)?,
                        description: r.get(2)?,
                        sort: r.get(3)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn add_team(&self, name: &str, description: &str) -> Result<Team> {
        let teams = self.list_teams()?;
        let base = slugify(name);
        let mut key = base.clone();
        let mut n = 2;
        while teams.iter().any(|t| t.key == key) {
            key = format!("{}-{}", base, n);
            n += 1;
        }
        let sort = teams.iter().map(|t| t.sort).max().unwrap_or(-1) + 1;
        self.with(|c| {
            c.execute(
                "INSERT INTO teams(key,name,description,sort) VALUES(?1,?2,?3,?4)",
                params![key, name, description, sort],
            )?;
            Ok(())
        })?;
        // New team starts with a manager so it can accept work immediately.
        let mkey = format!("{}-truong-nhom", key);
        self.with(|c| {
            c.execute(
                "INSERT OR IGNORE INTO agents(key,name,role,duty,kind,team,sort) VALUES(?1,?2,?3,?4,'manager',?5,0)",
                params![mkey, "TRƯỞNG NHÓM", "Điều phối & tổng hợp", "Nhận nhiệm vụ từ Sếp, phân công cho đội và tổng hợp báo cáo.", key],
            )?;
            Ok(())
        })?;
        Ok(Team {
            key,
            name: name.to_string(),
            description: description.to_string(),
            sort,
        })
    }

    pub fn update_team(
        &self,
        key: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<bool> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE teams SET name=COALESCE(?2,name), description=COALESCE(?3,description) WHERE key=?1",
                params![key, name, description],
            )?;
            Ok(n > 0)
        })
    }

    pub fn delete_team(&self, key: &str) -> Result<bool> {
        self.with(|c| {
            c.execute("DELETE FROM agents WHERE team=?1", params![key])?;
            let n = c.execute("DELETE FROM teams WHERE key=?1", params![key])?;
            Ok(n > 0)
        })
    }

    // ---- agents ----

    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT key,name,role,duty,kind,team,enabled,auto_assign,skills,status,status_note,sort
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
                        team: r.get(5)?,
                        enabled: r.get::<_, i64>(6)? != 0,
                        auto_assign: r.get::<_, i64>(7)? != 0,
                        skills: skills_from_json(&r.get::<_, String>(8)?),
                        status: r.get(9)?,
                        status_note: r.get(10)?,
                        sort: r.get(11)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Agents belonging to one team.
    pub fn list_agents_in(&self, team: &str) -> Result<Vec<Agent>> {
        Ok(self
            .list_agents()?
            .into_iter()
            .filter(|a| a.team == team)
            .collect())
    }

    /// Create a staff member. Generates an ASCII slug key from the name and
    /// enforces the desk limit plus one-manager/one-QA invariants upstream.
    pub fn add_agent(
        &self,
        name: &str,
        role: &str,
        duty: &str,
        kind: &str,
        team: &str,
    ) -> Result<Agent> {
        let all = self.list_agents()?;
        let team_count = all.iter().filter(|a| a.team == team).count();
        if team_count >= MAX_AGENTS {
            anyhow::bail!("đội chỉ có {} bàn — xoá bớt nhân sự trước", MAX_AGENTS);
        }
        let base = format!("{}-{}", team, slugify(name));
        let mut key = base.clone();
        let mut n = 2;
        while all.iter().any(|a| a.key == key) {
            key = format!("{}-{}", base, n);
            n += 1;
        }
        let sort = all
            .iter()
            .filter(|a| a.team == team)
            .map(|a| a.sort)
            .max()
            .unwrap_or(-1)
            + 1;
        self.with(|c| {
            c.execute(
                "INSERT INTO agents(key,name,role,duty,kind,team,sort) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![key, name, role, duty, kind, team, sort],
            )?;
            Ok(())
        })?;
        Ok(Agent {
            key,
            name: name.to_string(),
            role: role.to_string(),
            duty: duty.to_string(),
            kind: kind.to_string(),
            team: team.to_string(),
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

    /// Reset statuses. With a team, only that team's agents (so a parallel
    /// team's live statuses aren't wiped); empty team = all agents.
    pub fn reset_agent_statuses(&self, team: &str) -> Result<()> {
        self.with(|c| {
            if team.is_empty() {
                c.execute("UPDATE agents SET status='idle', status_note=''", [])?;
            } else {
                c.execute(
                    "UPDATE agents SET status='idle', status_note='' WHERE team=?1",
                    params![team],
                )?;
            }
            Ok(())
        })
    }

    // ---- tasks ----

    /// `start=false` đặt việc vào HỘP VIỆC (status 'inbox') — scheduler chỉ
    /// nhặt 'pending', nên việc nằm yên tới khi Sếp bấm ▶ Chạy.
    pub fn create_task(
        &self,
        title: &str,
        mode: &str,
        team: &str,
        goal_id: Option<i64>,
        start: bool,
    ) -> Result<Task> {
        let ts = now();
        let status = if start { "pending" } else { "inbox" };
        self.with(|c| {
            c.execute(
                "INSERT INTO tasks(title,mode,team,status,goal_id,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![title, mode, team, status, goal_id, ts],
            )?;
            let id = c.last_insert_rowid();
            Ok(Task {
                id,
                title: title.to_string(),
                mode: mode.to_string(),
                team: team.to_string(),
                status: status.into(),
                report: String::new(),
                llm_calls: 0,
                llm_model: String::new(),
                tokens_in: 0,
                tokens_out: 0,
                goal_id,
                approval: String::new(),
                approved_at: None,
                boss_note: String::new(),
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
            team: r.get(3)?,
            status: r.get(4)?,
            report: r.get(5)?,
            llm_calls: r.get(6)?,
            llm_model: r.get(7)?,
            tokens_in: r.get(8)?,
            tokens_out: r.get(9)?,
            goal_id: r.get(10)?,
            approval: r.get(11)?,
            approved_at: r.get(12)?,
            boss_note: r.get(13)?,
            created_at: r.get(14)?,
            finished_at: r.get(15)?,
        })
    }

    const TASK_COLS: &'static str =
        "id,title,mode,team,status,report,llm_calls,llm_model,tokens_in,tokens_out,goal_id,approval,approved_at,boss_note,created_at,finished_at";

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
                    &format!(
                        "SELECT {} FROM tasks ORDER BY id DESC LIMIT 1",
                        Self::TASK_COLS
                    ),
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
                c.execute(
                    "UPDATE tasks SET status=?2 WHERE id=?1",
                    params![id, status],
                )?;
            }
            Ok(())
        })
    }

    pub fn set_task_report(&self, id: i64, report: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE tasks SET report=?2 WHERE id=?1",
                params![id, report],
            )?;
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

    /// Whether a team is actively working a task (not merely queued). Empty
    /// team = any team.
    pub fn has_running_task(&self, team: &str) -> Result<bool> {
        self.with(|c| {
            let n: i64 = if team.is_empty() {
                c.query_row(
                    "SELECT COUNT(*) FROM tasks WHERE status IN ('planning','running','review')",
                    [],
                    |r| r.get(0),
                )?
            } else {
                c.query_row(
                    "SELECT COUNT(*) FROM tasks WHERE team=?1 AND status IN ('planning','running','review')",
                    params![team],
                    |r| r.get(0),
                )?
            };
            Ok(n > 0)
        })
    }

    /// Oldest queued task id for a team (FIFO within the team).
    pub fn next_pending(&self, team: &str) -> Result<Option<i64>> {
        self.with(|c| {
            let id = c
                .query_row(
                    "SELECT id FROM tasks WHERE team=?1 AND status='pending' ORDER BY id LIMIT 1",
                    params![team],
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

    // ---- bảng việc: vòng duyệt của Sếp ----

    /// Đặt trạng thái duyệt. 'approved' đóng dấu thời điểm nghiệm thu.
    pub fn set_task_approval(&self, id: i64, approval: &str) -> Result<()> {
        self.with(|c| {
            if approval == "approved" {
                c.execute(
                    "UPDATE tasks SET approval=?2, approved_at=?3 WHERE id=?1",
                    params![id, approval, now()],
                )?;
            } else {
                c.execute(
                    "UPDATE tasks SET approval=?2 WHERE id=?1",
                    params![id, approval],
                )?;
            }
            Ok(())
        })
    }

    /// Sếp nghiệm thu một việc đang chờ duyệt → cột HOÀN TẤT.
    pub fn approve_task(&self, id: i64) -> Result<bool> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE tasks SET approval='approved', approved_at=?2
                 WHERE id=?1 AND status='done' AND approval='waiting'",
                params![id, now()],
            )?;
            Ok(n > 0)
        })
    }

    /// Sếp trả lại việc chờ duyệt kèm ghi chú: việc quay về hàng đợi, chạy
    /// lại với ghi chú trong context; các bước cũ xoá để Trưởng nhóm phân
    /// công lại từ đầu (báo cáo cũ giữ nguyên làm tham chiếu).
    pub fn return_task(&self, id: i64, note: &str) -> Result<bool> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE tasks SET approval='returned', boss_note=?2, status='pending', finished_at=NULL
                 WHERE id=?1 AND status='done' AND approval='waiting'",
                params![id, note],
            )?;
            if n > 0 {
                c.execute("DELETE FROM steps WHERE task_id=?1", params![id])?;
            }
            Ok(n > 0)
        })
    }

    /// Đưa việc trong HỘP VIỆC (hoặc việc lỗi) vào hàng đợi chạy.
    pub fn start_task(&self, id: i64) -> Result<bool> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE tasks SET status='pending', approval='', finished_at=NULL
                 WHERE id=?1 AND status IN ('inbox','error')",
                params![id],
            )?;
            if n > 0 {
                c.execute("DELETE FROM steps WHERE task_id=?1", params![id])?;
            }
            Ok(n > 0)
        })
    }

    /// Xoá một việc khỏi bảng (kèm steps + nhật ký). Không xoá việc đang
    /// chạy — trả false để API báo lỗi.
    pub fn delete_task(&self, id: i64) -> Result<bool> {
        self.with(|c| {
            let running: i64 = c.query_row(
                "SELECT COUNT(*) FROM tasks WHERE id=?1 AND status IN ('planning','running','review')",
                params![id],
                |r| r.get(0),
            )?;
            if running > 0 {
                return Ok(false);
            }
            c.execute("DELETE FROM steps WHERE task_id=?1", params![id])?;
            c.execute("DELETE FROM events WHERE task_id=?1", params![id])?;
            let n = c.execute("DELETE FROM tasks WHERE id=?1", params![id])?;
            Ok(n > 0)
        })
    }

    /// Gắn / gỡ mục tiêu cho một việc (goal_id=None → "lạc hướng").
    pub fn set_task_goal(&self, id: i64, goal_id: Option<i64>) -> Result<bool> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE tasks SET goal_id=?2 WHERE id=?1",
                params![id, goal_id],
            )?;
            Ok(n > 0)
        })
    }

    pub fn set_task_title(&self, id: i64, title: &str) -> Result<bool> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE tasks SET title=?2 WHERE id=?1",
                params![id, title],
            )?;
            Ok(n > 0)
        })
    }

    /// Việc "đang mở" trên bảng: chưa được Sếp nghiệm thu và không lỗi.
    /// Trả `(tổng, số việc có gắn mục tiêu)` cho thẻ ĐỘ BÁM HƯỚNG.
    pub fn open_task_stats(&self) -> Result<(i64, i64)> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*), COALESCE(SUM(goal_id IS NOT NULL), 0) FROM tasks
                 WHERE status IN ('inbox','pending','planning','running','review')
                    OR (status='done' AND approval='waiting')",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(Into::into)
        })
    }

    /// Số việc đang nằm trên bàn Sếp chờ nghiệm thu.
    pub fn waiting_count(&self) -> Result<i64> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM tasks WHERE status='done' AND approval='waiting'",
                [],
                |r| r.get(0),
            )
            .map_err(Into::into)
        })
    }

    /// Tổng token (vào + ra) của các việc tạo từ mốc thời gian `since`.
    pub fn tokens_since(&self, since: i64) -> Result<i64> {
        self.with(|c| {
            c.query_row(
                "SELECT COALESCE(SUM(tokens_in+tokens_out),0) FROM tasks WHERE created_at>=?1",
                params![since],
                |r| r.get(0),
            )
            .map_err(Into::into)
        })
    }

    // ---- mục tiêu quý (goals) ----

    fn row_goal(r: &rusqlite::Row<'_>) -> rusqlite::Result<Goal> {
        let krs: Vec<KeyResult> =
            serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default();
        let progress = goal_progress(&krs);
        Ok(Goal {
            id: r.get(0)?,
            title: r.get(1)?,
            quarter: r.get(2)?,
            key_results: krs,
            archived: r.get::<_, i64>(4)? != 0,
            created_at: r.get(5)?,
            progress,
        })
    }

    pub fn list_goals(&self, include_archived: bool) -> Result<Vec<Goal>> {
        self.with(|c| {
            let sql = if include_archived {
                "SELECT id,title,quarter,key_results,archived,created_at FROM goals ORDER BY archived, id"
            } else {
                "SELECT id,title,quarter,key_results,archived,created_at FROM goals WHERE archived=0 ORDER BY id"
            };
            let mut stmt = c.prepare(sql)?;
            let rows = stmt
                .query_map([], Self::row_goal)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn get_goal(&self, id: i64) -> Result<Option<Goal>> {
        self.with(|c| {
            let g = c
                .query_row(
                    "SELECT id,title,quarter,key_results,archived,created_at FROM goals WHERE id=?1",
                    params![id],
                    Self::row_goal,
                )
                .optional()?;
            Ok(g)
        })
    }

    pub fn add_goal(&self, title: &str, quarter: &str, key_results: &[KeyResult]) -> Result<Goal> {
        let ts = now();
        let krs_json = serde_json::to_string(key_results)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO goals(title,quarter,key_results,archived,created_at) VALUES(?1,?2,?3,0,?4)",
                params![title, quarter, krs_json, ts],
            )?;
            Ok(Goal {
                id: c.last_insert_rowid(),
                title: title.to_string(),
                quarter: quarter.to_string(),
                key_results: key_results.to_vec(),
                archived: false,
                created_at: ts,
                progress: goal_progress(key_results),
            })
        })
    }

    pub fn update_goal(
        &self,
        id: i64,
        title: Option<&str>,
        quarter: Option<&str>,
        key_results: Option<&[KeyResult]>,
        archived: Option<bool>,
    ) -> Result<bool> {
        let krs_json = match key_results {
            Some(k) => Some(serde_json::to_string(k)?),
            None => None,
        };
        self.with(|c| {
            let n = c.execute(
                "UPDATE goals SET
                    title = COALESCE(?2, title),
                    quarter = COALESCE(?3, quarter),
                    key_results = COALESCE(?4, key_results),
                    archived = COALESCE(?5, archived)
                 WHERE id = ?1",
                params![id, title, quarter, krs_json, archived.map(|b| b as i64)],
            )?;
            Ok(n > 0)
        })
    }

    /// Xoá mục tiêu; việc đang gắn vào nó trở thành "lạc hướng" (goal NULL).
    pub fn delete_goal(&self, id: i64) -> Result<bool> {
        self.with(|c| {
            c.execute("UPDATE tasks SET goal_id=NULL WHERE goal_id=?1", params![id])?;
            let n = c.execute("DELETE FROM goals WHERE id=?1", params![id])?;
            Ok(n > 0)
        })
    }

    /// `goal_id → (tổng số việc, số việc đang mở)` cho phần Mục tiêu.
    pub fn goal_task_counts(&self) -> Result<std::collections::HashMap<i64, (i64, i64)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT goal_id, COUNT(*),
                        COALESCE(SUM(status IN ('inbox','pending','planning','running','review')
                                     OR (status='done' AND approval='waiting')), 0)
                 FROM tasks WHERE goal_id IS NOT NULL GROUP BY goal_id",
            )?;
            let mut out = std::collections::HashMap::new();
            let rows = stmt.query_map([], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))
            })?;
            for row in rows {
                let (gid, total, open) = row?;
                out.insert(gid, (total, open));
            }
            Ok(out)
        })
    }

    // ---- họp điều hành (meetings) ----

    /// Ghi biên bản họp của một ngày; họp lại trong ngày = ghi đè bản cũ.
    pub fn upsert_meeting(&self, kind: &str, day: &str, content: &str) -> Result<Meeting> {
        let ts = now();
        self.with(|c| {
            c.execute(
                "INSERT INTO meetings(kind,day,content,created_at) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(kind,day) DO UPDATE SET content=excluded.content, created_at=excluded.created_at",
                params![kind, day, content, ts],
            )?;
            let id: i64 = c.query_row(
                "SELECT id FROM meetings WHERE kind=?1 AND day=?2",
                params![kind, day],
                |r| r.get(0),
            )?;
            Ok(Meeting {
                id,
                kind: kind.to_string(),
                day: day.to_string(),
                content: content.to_string(),
                created_at: ts,
            })
        })
    }

    pub fn list_meetings(&self, limit: i64) -> Result<Vec<Meeting>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id,kind,day,content,created_at FROM meetings
                 ORDER BY day DESC, created_at DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![limit], |r| {
                    Ok(Meeting {
                        id: r.get(0)?,
                        kind: r.get(1)?,
                        day: r.get(2)?,
                        content: r.get(3)?,
                        created_at: r.get(4)?,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    /// Các ngày đã họp sáng (YYYY-MM-DD, mới nhất trước) — tính NHỊP ĐIỀU HÀNH.
    pub fn morning_days(&self) -> Result<Vec<String>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT DISTINCT day FROM meetings WHERE kind='morning' ORDER BY day DESC LIMIT 400",
            )?;
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
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
                _ => c.execute(
                    "UPDATE steps SET status=?2 WHERE id=?1",
                    params![id, status],
                )?,
            };
            Ok(())
        })
    }

    pub fn set_step_result(&self, id: i64, result: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE steps SET result=?2 WHERE id=?1",
                params![id, result],
            )?;
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
                .query_map(
                    rusqlite::params_from_iter(p.iter().map(|b| b.as_ref())),
                    |r| {
                        Ok(Event {
                            id: r.get(0)?,
                            task_id: r.get(1)?,
                            kind: r.get(2)?,
                            actor: r.get(3)?,
                            target: r.get(4)?,
                            text: r.get(5)?,
                            created_at: r.get(6)?,
                        })
                    },
                )?
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
                .query_row(
                    "SELECT value FROM settings WHERE key=?1",
                    params![key],
                    |r| r.get::<_, String>(0),
                )
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

    #[cfg(test)]
    pub fn open_temp() -> (Self, PathBuf) {
        // Tests chạy song song — đếm atomic để mỗi test một file DB riêng.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ai-office-test-{}-{}-{}.db",
            std::process::id(),
            now(),
            n
        ));
        let _ = std::fs::remove_file(&path);
        (Self::open(&path).expect("open temp db"), path)
    }

    // ---- stats (Kế toán) ----

    pub fn stats(&self) -> Result<serde_json::Value> {
        self.with(|c| {
            let total: i64 = c.query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))?;
            let done: i64 =
                c.query_row("SELECT COUNT(*) FROM tasks WHERE status='done'", [], |r| {
                    r.get(0)
                })?;
            let live: i64 =
                c.query_row("SELECT COUNT(*) FROM tasks WHERE mode='live'", [], |r| {
                    r.get(0)
                })?;
            let llm_calls: i64 =
                c.query_row("SELECT COALESCE(SUM(llm_calls),0) FROM tasks", [], |r| {
                    r.get(0)
                })?;
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

#[cfg(test)]
mod tests {
    use super::{Db, KeyResult};

    fn temp() -> (Db, std::path::PathBuf) {
        Db::open_temp()
    }

    #[test]
    fn approval_cycle_waiting_approved() {
        let (db, path) = temp();
        let t = db.create_task("việc A", "live", "nctt", None, true).unwrap();
        assert_eq!(t.status, "pending");
        // AI xong → chờ Sếp duyệt; chưa done thì không duyệt được.
        assert!(!db.approve_task(t.id).unwrap());
        db.set_task_status(t.id, "done").unwrap();
        db.set_task_approval(t.id, "waiting").unwrap();
        assert_eq!(db.waiting_count().unwrap(), 1);
        assert!(db.approve_task(t.id).unwrap());
        let t = db.get_task(t.id).unwrap().unwrap();
        assert_eq!(t.approval, "approved");
        assert!(t.approved_at.is_some());
        assert_eq!(db.waiting_count().unwrap(), 0);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn return_requeues_with_note_and_clears_steps() {
        let (db, path) = temp();
        let t = db.create_task("việc B", "live", "nctt", None, true).unwrap();
        db.add_step(t.id, "nctt-nghien-cuu", "tìm hiểu", 0).unwrap();
        db.set_task_status(t.id, "done").unwrap();
        db.set_task_approval(t.id, "waiting").unwrap();
        assert!(db.return_task(t.id, "thiếu số liệu đối thủ").unwrap());
        let t2 = db.get_task(t.id).unwrap().unwrap();
        assert_eq!(t2.status, "pending"); // quay lại hàng đợi
        assert_eq!(t2.approval, "returned");
        assert_eq!(t2.boss_note, "thiếu số liệu đối thủ");
        assert!(db.list_steps(t.id).unwrap().is_empty());
        // Trả lại lần nữa khi không còn chờ duyệt → từ chối.
        assert!(!db.return_task(t.id, "x").unwrap());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn inbox_task_stays_until_started() {
        let (db, path) = temp();
        let t = db
            .create_task("việc C", "live", "nctt", None, false)
            .unwrap();
        assert_eq!(t.status, "inbox");
        // Scheduler không nhặt việc trong Hộp việc.
        assert!(db.next_pending("nctt").unwrap().is_none());
        assert!(db.start_task(t.id).unwrap());
        assert_eq!(db.next_pending("nctt").unwrap(), Some(t.id));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn goals_progress_and_task_alignment() {
        let (db, path) = temp();
        let g = db
            .add_goal(
                "Đạt 30 triệu doanh thu",
                "Q3/2026",
                &[
                    KeyResult { text: "50 học viên".into(), done: true },
                    KeyResult { text: "chuỗi email".into(), done: false },
                ],
            )
            .unwrap();
        assert_eq!(g.progress, 50);
        let t1 = db
            .create_task("việc bám hướng", "live", "nctt", Some(g.id), false)
            .unwrap();
        db.create_task("việc lạc hướng", "live", "nctt", None, false)
            .unwrap();
        let (open, aligned) = db.open_task_stats().unwrap();
        assert_eq!((open, aligned), (2, 1));
        let counts = db.goal_task_counts().unwrap();
        assert_eq!(counts.get(&g.id), Some(&(1, 1)));
        // Xoá mục tiêu → việc thành lạc hướng chứ không mất.
        assert!(db.delete_goal(g.id).unwrap());
        assert_eq!(db.get_task(t1.id).unwrap().unwrap().goal_id, None);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn meetings_upsert_per_day() {
        let (db, path) = temp();
        let m1 = db.upsert_meeting("morning", "2026-08-04", "bản 1").unwrap();
        let m2 = db.upsert_meeting("morning", "2026-08-04", "bản 2").unwrap();
        assert_eq!(m1.id, m2.id); // họp lại trong ngày = ghi đè
        db.upsert_meeting("evening", "2026-08-04", "tối").unwrap();
        db.upsert_meeting("morning", "2026-08-03", "hôm qua").unwrap();
        assert_eq!(db.list_meetings(10).unwrap().len(), 3);
        assert_eq!(
            db.morning_days().unwrap(),
            vec!["2026-08-04".to_string(), "2026-08-03".to_string()]
        );
        let _ = std::fs::remove_file(path);
    }
}
