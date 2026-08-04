//! SQLite store — ported from internal/store/*.go.
//!
//! Single Mutex-guarded rusqlite connection (the Go store pinned MaxOpenConns=1
//! for the same single-writer reason). Times are stored as RFC3339 strings and
//! compared with SQLite's `datetime(...)`, exactly as the Go code did.

use crate::domain::*;
use anyhow::{anyhow, Result};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

/// RFC3339 timestamp with nanosecond precision, UTC — matches Go's
/// `time.Now().Format(time.RFC3339Nano)` for the string comparisons the schema
/// relies on.
pub fn now_str() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

/// Monotonic-ish unique id like the Go `fmt.Sprintf("%s_%d", prefix, UnixNano)`.
/// A per-process counter breaks ties when two ids are minted in the same ns.
pub fn gen_id(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{prefix}_{}", nanos as u64 + seq)
}

fn json_str_map(m: &Option<StrMap>) -> String {
    serde_json::to_string(&m.clone().unwrap_or_default()).unwrap_or_else(|_| "{}".into())
}

fn parse_str_map(s: &str) -> Option<StrMap> {
    serde_json::from_str::<StrMap>(s).ok()
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        if let Some(dir) = Path::new(path).parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).ok();
            }
        }
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self.conn.lock().map_err(|_| anyhow!("db mutex poisoned"))?;
        f(&guard)
    }

    fn column_exists(&self, conn: &Connection, table: &str, col: &str) -> bool {
        let mut stmt = match conn.prepare(&format!("PRAGMA table_info({table})")) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let names = stmt.query_map([], |r| r.get::<_, String>(1));
        match names {
            Ok(rows) => rows.flatten().any(|n| n.eq_ignore_ascii_case(col)),
            Err(_) => false,
        }
    }

    fn migrate(&self) -> Result<()> {
        self.with(|c| {
            for q in [
                r#"CREATE TABLE IF NOT EXISTS accounts (
                    id TEXT PRIMARY KEY, username TEXT NOT NULL, password TEXT NOT NULL,
                    proxy TEXT NOT NULL DEFAULT '', profile_path TEXT NOT NULL,
                    user_agent TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL)"#,
                r#"CREATE TABLE IF NOT EXISTS flows (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL,
                    params_json TEXT NOT NULL DEFAULT '{}', actions_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL)"#,
                r#"CREATE TABLE IF NOT EXISTS flow_runs (
                    id TEXT PRIMARY KEY, account_id TEXT NOT NULL, flow_id TEXT NOT NULL,
                    status TEXT NOT NULL, logs_json TEXT NOT NULL,
                    started_at TEXT NOT NULL, ended_at TEXT)"#,
                r#"CREATE TABLE IF NOT EXISTS proxies (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, url TEXT NOT NULL,
                    notes TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL)"#,
                r#"CREATE TABLE IF NOT EXISTS browser_profiles (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, user_data_dir TEXT NOT NULL,
                    user_agent TEXT NOT NULL DEFAULT '', viewport_w INTEGER NOT NULL DEFAULT 0,
                    viewport_h INTEGER NOT NULL DEFAULT 0, locale TEXT NOT NULL DEFAULT '',
                    timezone_id TEXT NOT NULL DEFAULT '', account_id TEXT NOT NULL DEFAULT '',
                    notes TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, updated_at TEXT NOT NULL)"#,
                r#"CREATE TABLE IF NOT EXISTS saved_flow_actions (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, step_json TEXT NOT NULL,
                    updated_at TEXT NOT NULL)"#,
                r#"CREATE TABLE IF NOT EXISTS schedules (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1,
                    flow_id TEXT NOT NULL, params_json TEXT NOT NULL DEFAULT '{}',
                    all_accounts INTEGER NOT NULL DEFAULT 1, account_ids_json TEXT NOT NULL,
                    type TEXT NOT NULL, daily_at TEXT NOT NULL DEFAULT '',
                    once_at TEXT NOT NULL DEFAULT '', timezone_id TEXT NOT NULL DEFAULT '',
                    last_run_at TEXT NOT NULL DEFAULT '', next_run_at TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL, updated_at TEXT NOT NULL)"#,
                r#"CREATE TABLE IF NOT EXISTS notification_rules (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1,
                    event TEXT NOT NULL, flow_id TEXT NOT NULL DEFAULT '',
                    account_id TEXT NOT NULL DEFAULT '', message_template TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL, updated_at TEXT NOT NULL)"#,
                r#"CREATE TABLE IF NOT EXISTS notifications (
                    id TEXT PRIMARY KEY, rule_id TEXT NOT NULL DEFAULT '', event TEXT NOT NULL,
                    title TEXT NOT NULL, body TEXT NOT NULL, run_id TEXT NOT NULL DEFAULT '',
                    account_id TEXT NOT NULL DEFAULT '', flow_id TEXT NOT NULL DEFAULT '',
                    read_at TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL)"#,
                r#"CREATE TABLE IF NOT EXISTS agent_skills (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
                    body_md TEXT NOT NULL DEFAULT '', enabled INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL, updated_at TEXT NOT NULL)"#,
                r#"CREATE TABLE IF NOT EXISTS account_post_interactions (
                    id TEXT PRIMARY KEY, account_id TEXT NOT NULL, post_key TEXT NOT NULL,
                    interaction_type TEXT NOT NULL, post_url TEXT NOT NULL DEFAULT '',
                    author_username TEXT NOT NULL DEFAULT '', extra_json TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL)"#,
                r#"CREATE INDEX IF NOT EXISTS idx_api_account ON account_post_interactions(account_id)"#,
                r#"CREATE TABLE IF NOT EXISTS account_friend_events (
                    id TEXT PRIMARY KEY, account_id TEXT NOT NULL, target_username TEXT NOT NULL DEFAULT '',
                    target_user_id TEXT NOT NULL DEFAULT '', event_type TEXT NOT NULL,
                    notes TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL)"#,
                r#"CREATE INDEX IF NOT EXISTS idx_afe_account ON account_friend_events(account_id)"#,
                r#"CREATE TABLE IF NOT EXISTS account_kv_meta (
                    account_id TEXT NOT NULL, meta_key TEXT NOT NULL, meta_value TEXT NOT NULL,
                    updated_at TEXT NOT NULL, PRIMARY KEY (account_id, meta_key))"#,
                r#"CREATE TABLE IF NOT EXISTS app_settings (
                    key TEXT PRIMARY KEY, value_json TEXT NOT NULL, updated_at TEXT NOT NULL)"#,
                r#"CREATE TABLE IF NOT EXISTS engine_kv (
                    key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL)"#,
            ] {
                c.execute(q, [])?;
            }
            Ok(())
        })?;
        // Additive migrations for pre-existing DBs.
        self.with(|c| {
            if !self.column_exists(c, "accounts", "proxy_id") {
                c.execute(
                    "ALTER TABLE accounts ADD COLUMN proxy_id TEXT NOT NULL DEFAULT ''",
                    [],
                )?;
            }
            if !self.column_exists(c, "accounts", "browser_profile_id") {
                c.execute(
                    "ALTER TABLE accounts ADD COLUMN browser_profile_id TEXT NOT NULL DEFAULT ''",
                    [],
                )?;
            }
            if !self.column_exists(c, "flows", "params_json") {
                c.execute(
                    "ALTER TABLE flows ADD COLUMN params_json TEXT NOT NULL DEFAULT '{}'",
                    [],
                )?;
            }
            if !self.column_exists(c, "flow_runs", "schedule_id") {
                c.execute(
                    "ALTER TABLE flow_runs ADD COLUMN schedule_id TEXT NOT NULL DEFAULT ''",
                    [],
                )?;
            }
            if !self.column_exists(c, "schedules", "params_json") {
                c.execute(
                    "ALTER TABLE schedules ADD COLUMN params_json TEXT NOT NULL DEFAULT '{}'",
                    [],
                )?;
            }
            Ok(())
        })
    }

    // ---------------- Accounts ----------------

    pub fn upsert_account(&self, mut acc: TikTokAccount) -> TikTokAccount {
        if acc.id.trim().is_empty() {
            acc.id = gen_id("acc");
        }
        if acc.created_at.trim().is_empty() {
            acc.created_at = now_str();
        }
        let _ = self.with(|c| {
            c.execute(
                r#"INSERT INTO accounts (id, username, password, proxy, profile_path, user_agent, proxy_id, browser_profile_id, created_at)
                   VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                   ON CONFLICT(id) DO UPDATE SET username=excluded.username, password=excluded.password,
                     proxy=excluded.proxy, profile_path=excluded.profile_path, user_agent=excluded.user_agent,
                     proxy_id=excluded.proxy_id, browser_profile_id=excluded.browser_profile_id"#,
                params![acc.id, acc.username, acc.password, acc.proxy, acc.profile_path,
                        acc.user_agent, acc.proxy_id, acc.browser_profile_id, acc.created_at],
            )?;
            Ok(())
        });
        acc
    }

    fn map_account(r: &rusqlite::Row) -> rusqlite::Result<TikTokAccount> {
        Ok(TikTokAccount {
            id: r.get(0)?,
            username: r.get(1)?,
            password: r.get(2)?,
            proxy: r.get(3)?,
            profile_path: r.get(4)?,
            user_agent: r.get(5)?,
            proxy_id: r.get(6)?,
            browser_profile_id: r.get(7)?,
            created_at: r.get(8)?,
            ..Default::default()
        })
    }

    pub fn list_accounts(&self) -> Vec<TikTokAccount> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, username, password, proxy, profile_path, user_agent, proxy_id, browser_profile_id, created_at FROM accounts ORDER BY datetime(created_at)",
            )?;
            let rows = stmt.query_map([], Self::map_account)?;
            Ok(rows.flatten().collect())
        })
        .unwrap_or_default()
    }

    pub fn get_account(&self, id: &str) -> Option<TikTokAccount> {
        self.with(|c| {
            let acc = c
                .query_row(
                    "SELECT id, username, password, proxy, profile_path, user_agent, proxy_id, browser_profile_id, created_at FROM accounts WHERE id = ?1",
                    params![id],
                    Self::map_account,
                )
                .optional()?;
            Ok(acc)
        })
        .unwrap_or(None)
    }

    pub fn list_accounts_page(
        &self,
        offset: i64,
        limit: i64,
        q: &str,
    ) -> Result<(Vec<TikTokAccount>, i64)> {
        let q = q.trim().to_lowercase();
        self.with(|c| {
            let (cond, args) = if q.is_empty() {
                (String::new(), vec![])
            } else {
                (
                    " WHERE (instr(LOWER(id),?1)>0 OR instr(LOWER(username),?1)>0 OR instr(LOWER(COALESCE(proxy_id,'')),?1)>0 OR instr(LOWER(COALESCE(browser_profile_id,'')),?1)>0 OR instr(LOWER(COALESCE(proxy,'')),?1)>0 OR instr(LOWER(COALESCE(profile_path,'')),?1)>0)".to_string(),
                    vec![q.clone()],
                )
            };
            let total: i64 = c.query_row(&format!("SELECT COUNT(*) FROM accounts{cond}"), params_from_iter(args.iter()), |r| r.get(0))?;
            let (limit, offset) = clamp_page(limit, offset);
            let sql = format!(
                "SELECT id, username, password, proxy, profile_path, user_agent, proxy_id, browser_profile_id, created_at FROM accounts{cond} ORDER BY datetime(created_at) LIMIT {limit} OFFSET {offset}"
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(args.iter()), Self::map_account)?;
            Ok((rows.flatten().collect(), total))
        })
    }

    // ---------------- Flows ----------------

    pub fn upsert_flow(&self, mut flow: Flow) -> Flow {
        if flow.id.trim().is_empty() {
            flow.id = gen_id("flow");
        }
        flow.updated_at = now_str();
        let actions = serde_json::to_string(&flow.actions).unwrap_or_else(|_| "[]".into());
        let params_json = json_str_map(&flow.params);
        let _ = self.with(|c| {
            c.execute(
                r#"INSERT INTO flows (id, name, params_json, actions_json, updated_at) VALUES (?1,?2,?3,?4,?5)
                   ON CONFLICT(id) DO UPDATE SET name=excluded.name, params_json=excluded.params_json,
                     actions_json=excluded.actions_json, updated_at=excluded.updated_at"#,
                params![flow.id, flow.name, params_json, actions, flow.updated_at],
            )?;
            Ok(())
        });
        flow
    }

    fn map_flow(r: &rusqlite::Row) -> rusqlite::Result<Flow> {
        let params_json: String = r.get(2)?;
        let actions_json: String = r.get(3)?;
        Ok(Flow {
            id: r.get(0)?,
            name: r.get(1)?,
            params: parse_str_map(&params_json),
            actions: serde_json::from_str(&actions_json).unwrap_or_default(),
            updated_at: r.get(4)?,
        })
    }

    pub fn get_flow(&self, id: &str) -> Result<Flow> {
        self.with(|c| {
            c.query_row(
                "SELECT id, name, params_json, actions_json, updated_at FROM flows WHERE id = ?1",
                params![id],
                Self::map_flow,
            )
            .optional()?
            .ok_or_else(|| anyhow!("flow not found"))
        })
    }

    pub fn list_flows(&self) -> Vec<Flow> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id, name, params_json, actions_json, updated_at FROM flows ORDER BY datetime(updated_at) DESC")?;
            let rows = stmt.query_map([], Self::map_flow)?;
            Ok(rows.flatten().collect())
        })
        .unwrap_or_default()
    }

    // ---------------- Runs ----------------

    pub fn save_run(&self, run: &FlowRun) {
        let logs = serde_json::to_string(&run.logs).unwrap_or_else(|_| "[]".into());
        let ended = if run.ended_at.is_empty() {
            None
        } else {
            Some(run.ended_at.clone())
        };
        let _ = self.with(|c| {
            c.execute(
                r#"INSERT INTO flow_runs (id, account_id, flow_id, schedule_id, status, logs_json, started_at, ended_at)
                   VALUES (?1,?2,?3,?4,?5,?6,?7,?8)"#,
                params![run.id, run.account_id, run.flow_id, run.schedule_id, run.status, logs, run.started_at, ended],
            )?;
            Ok(())
        });
    }

    pub fn update_run(&self, run: &FlowRun) {
        let logs = serde_json::to_string(&run.logs).unwrap_or_else(|_| "[]".into());
        let ended = if run.ended_at.is_empty() {
            None
        } else {
            Some(run.ended_at.clone())
        };
        let _ = self.with(|c| {
            c.execute(
                r#"UPDATE flow_runs SET account_id=?1, flow_id=?2, schedule_id=?3, status=?4, logs_json=?5, started_at=?6, ended_at=?7 WHERE id=?8"#,
                params![run.account_id, run.flow_id, run.schedule_id, run.status, logs, run.started_at, ended, run.id],
            )?;
            Ok(())
        });
    }

    pub fn list_runs_page(&self, offset: i64, limit: i64, q: &str) -> Result<(Vec<FlowRun>, i64)> {
        let q = q.trim().to_lowercase();
        self.with(|c| {
            let (cond, args) = if q.is_empty() {
                (String::new(), vec![])
            } else {
                (" WHERE instr(LOWER(id),?1)>0 OR instr(LOWER(status),?1)>0 OR instr(LOWER(account_id),?1)>0 OR instr(LOWER(flow_id),?1)>0".to_string(), vec![q.clone()])
            };
            let total: i64 = c.query_row(&format!("SELECT COUNT(*) FROM flow_runs{cond}"), params_from_iter(args.iter()), |r| r.get(0))?;
            let (limit, offset) = clamp_page(limit, offset);
            let sql = format!("SELECT id, account_id, flow_id, schedule_id, status, started_at, ended_at FROM flow_runs{cond} ORDER BY datetime(started_at) DESC LIMIT {limit} OFFSET {offset}");
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(args.iter()), |r| {
                Ok(FlowRun {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    flow_id: r.get(2)?,
                    schedule_id: r.get(3)?,
                    status: r.get(4)?,
                    logs: vec![],
                    started_at: r.get(5)?,
                    ended_at: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
                })
            })?;
            Ok((rows.flatten().collect(), total))
        })
    }

    pub fn get_run(&self, id: &str) -> Result<FlowRun> {
        self.with(|c| {
            c.query_row(
                "SELECT id, account_id, flow_id, schedule_id, status, logs_json, started_at, ended_at FROM flow_runs WHERE id = ?1",
                params![id],
                |r| {
                    let logs_json: String = r.get(5)?;
                    Ok(FlowRun {
                        id: r.get(0)?,
                        account_id: r.get(1)?,
                        flow_id: r.get(2)?,
                        schedule_id: r.get(3)?,
                        status: r.get(4)?,
                        logs: serde_json::from_str(&logs_json).unwrap_or_default(),
                        started_at: r.get(6)?,
                        ended_at: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    })
                },
            )
            .optional()?
            .ok_or_else(|| anyhow!("run not found"))
        })
    }

    pub fn dashboard_run_stats(&self) -> Result<DashboardRunStats> {
        let now = chrono::Utc::now();
        let day_keys: Vec<String> = (0..7)
            .rev()
            .map(|i| {
                (now - chrono::Duration::days(i))
                    .format("%Y-%m-%d")
                    .to_string()
            })
            .collect();
        let min_d = day_keys.first().cloned().unwrap_or_default();
        let max_d = day_keys.last().cloned().unwrap_or_default();

        self.with(|c| {
            let mut status_totals: BTreeMap<String, i64> = BTreeMap::new();
            let mut by_day: BTreeMap<String, [i64; 4]> = BTreeMap::new(); // [done, failed, running, queued]

            let mut stmt = c.prepare(
                "SELECT substr(started_at,1,10) AS day, status, COUNT(*) FROM flow_runs WHERE substr(started_at,1,10) >= ?1 AND substr(started_at,1,10) <= ?2 GROUP BY day, status",
            )?;
            let rows = stmt.query_map(params![min_d, max_d], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
            })?;
            for (day, status, n) in rows.flatten() {
                *status_totals.entry(status.clone()).or_default() += n;
                if day < min_d || day > max_d {
                    continue;
                }
                let e = by_day.entry(day).or_insert([0; 4]);
                match status.as_str() {
                    RUN_DONE => e[0] += n,
                    RUN_FAILED => e[1] += n,
                    RUN_RUNNING => e[2] += n,
                    RUN_QUEUED => e[3] += n,
                    _ => {}
                }
            }
            let last7 = day_keys
                .iter()
                .map(|dk| {
                    let a = by_day.get(dk).copied().unwrap_or([0; 4]);
                    DailyRunCount {
                        date: dk.clone(),
                        done: a[0],
                        failed: a[1],
                        running: a[2],
                        queued: a[3],
                        total: a[0] + a[1] + a[2] + a[3],
                    }
                })
                .collect();

            let mut stmt2 = c.prepare(
                "SELECT flow_id, COUNT(*) AS n FROM flow_runs WHERE substr(started_at,1,10) >= ?1 AND substr(started_at,1,10) <= ?2 GROUP BY flow_id ORDER BY n DESC LIMIT 8",
            )?;
            let top = stmt2
                .query_map(params![min_d, max_d], |r| {
                    Ok(FlowRunRank { flow_id: r.get(0)?, count: r.get(1)? })
                })?
                .flatten()
                .collect();

            Ok(DashboardRunStats {
                last7_days: last7,
                status_totals7d: status_totals,
                top_flows7d: top,
            })
        })
    }

    // ---------------- Proxies ----------------

    pub fn upsert_proxy(&self, mut p: ManagedProxy) -> ManagedProxy {
        if p.id.trim().is_empty() {
            p.id = gen_id("proxy");
        }
        if p.created_at.trim().is_empty() {
            p.created_at = now_str();
        }
        let _ = self.with(|c| {
            c.execute(
                r#"INSERT INTO proxies (id, name, url, notes, created_at) VALUES (?1,?2,?3,?4,?5)
                   ON CONFLICT(id) DO UPDATE SET name=excluded.name, url=excluded.url, notes=excluded.notes"#,
                params![p.id, p.name, p.url, p.notes, p.created_at],
            )?;
            Ok(())
        });
        p
    }

    fn map_proxy(r: &rusqlite::Row) -> rusqlite::Result<ManagedProxy> {
        Ok(ManagedProxy {
            id: r.get(0)?,
            name: r.get(1)?,
            url: r.get(2)?,
            notes: r.get(3)?,
            created_at: r.get(4)?,
        })
    }

    pub fn list_proxies(&self) -> Vec<ManagedProxy> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id, name, url, notes, created_at FROM proxies ORDER BY datetime(created_at)")?;
            let out: Vec<ManagedProxy> = stmt.query_map([], Self::map_proxy)?.flatten().collect();
            Ok(out)
        })
        .unwrap_or_default()
    }

    pub fn list_proxies_page(
        &self,
        offset: i64,
        limit: i64,
        q: &str,
    ) -> Result<(Vec<ManagedProxy>, i64)> {
        let q = q.trim().to_lowercase();
        self.with(|c| {
            let (cond, args) = if q.is_empty() {
                (String::new(), vec![])
            } else {
                (" WHERE (instr(LOWER(id),?1)>0 OR instr(LOWER(name),?1)>0 OR instr(LOWER(url),?1)>0 OR instr(LOWER(COALESCE(notes,'')),?1)>0)".to_string(), vec![q.clone()])
            };
            let total: i64 = c.query_row(&format!("SELECT COUNT(*) FROM proxies{cond}"), params_from_iter(args.iter()), |r| r.get(0))?;
            let (limit, offset) = clamp_page(limit, offset);
            let sql = format!("SELECT id, name, url, notes, created_at FROM proxies{cond} ORDER BY datetime(created_at) LIMIT {limit} OFFSET {offset}");
            let mut stmt = c.prepare(&sql)?;
            let out: Vec<ManagedProxy> = stmt.query_map(params_from_iter(args.iter()), Self::map_proxy)?.flatten().collect();
            Ok((out, total))
        })
    }

    pub fn get_proxy(&self, id: &str) -> Result<ManagedProxy> {
        self.with(|c| {
            c.query_row(
                "SELECT id, name, url, notes, created_at FROM proxies WHERE id = ?1",
                params![id],
                Self::map_proxy,
            )
            .optional()?
            .ok_or_else(|| anyhow!("proxy not found"))
        })
    }

    pub fn delete_proxy(&self, id: &str) -> Result<()> {
        self.with(|c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM accounts WHERE proxy_id = ?1",
                params![id],
                |r| r.get(0),
            )?;
            if n > 0 {
                return Err(anyhow!("proxy đang được {n} account sử dụng"));
            }
            c.execute("DELETE FROM proxies WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    // ---------------- Browser profiles ----------------

    pub fn upsert_browser_profile(&self, mut bp: BrowserProfile) -> BrowserProfile {
        if bp.id.trim().is_empty() {
            bp.id = gen_id("bprof");
        }
        if bp.created_at.trim().is_empty() {
            bp.created_at = now_str();
        }
        bp.updated_at = now_str();
        let _ = self.with(|c| {
            c.execute(
                r#"INSERT INTO browser_profiles (id, name, user_data_dir, user_agent, viewport_w, viewport_h, locale, timezone_id, account_id, notes, created_at, updated_at)
                   VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                   ON CONFLICT(id) DO UPDATE SET name=excluded.name, user_data_dir=excluded.user_data_dir,
                     user_agent=excluded.user_agent, viewport_w=excluded.viewport_w, viewport_h=excluded.viewport_h,
                     locale=excluded.locale, timezone_id=excluded.timezone_id, account_id=excluded.account_id,
                     notes=excluded.notes, updated_at=excluded.updated_at"#,
                params![bp.id, bp.name, bp.user_data_dir, bp.user_agent, bp.viewport_width, bp.viewport_height,
                        bp.locale, bp.timezone_id, bp.account_id, bp.notes, bp.created_at, bp.updated_at],
            )?;
            Ok(())
        });
        bp
    }

    fn map_bp(r: &rusqlite::Row) -> rusqlite::Result<BrowserProfile> {
        Ok(BrowserProfile {
            id: r.get(0)?,
            name: r.get(1)?,
            user_data_dir: r.get(2)?,
            user_agent: r.get(3)?,
            viewport_width: r.get(4)?,
            viewport_height: r.get(5)?,
            locale: r.get(6)?,
            timezone_id: r.get(7)?,
            account_id: r.get(8)?,
            notes: r.get(9)?,
            created_at: r.get(10)?,
            updated_at: r.get(11)?,
        })
    }

    const BP_COLS: &'static str = "id, name, user_data_dir, user_agent, viewport_w, viewport_h, locale, timezone_id, account_id, notes, created_at, updated_at";

    pub fn list_browser_profiles(&self) -> Vec<BrowserProfile> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM browser_profiles ORDER BY datetime(updated_at) DESC",
                Self::BP_COLS
            ))?;
            let out: Vec<BrowserProfile> = stmt.query_map([], Self::map_bp)?.flatten().collect();
            Ok(out)
        })
        .unwrap_or_default()
    }

    pub fn list_browser_profiles_page(
        &self,
        offset: i64,
        limit: i64,
        q: &str,
    ) -> Result<(Vec<BrowserProfile>, i64)> {
        let q = q.trim().to_lowercase();
        self.with(|c| {
            let (cond, args) = if q.is_empty() {
                (String::new(), vec![])
            } else {
                (" WHERE (instr(LOWER(id),?1)>0 OR instr(LOWER(name),?1)>0 OR instr(LOWER(user_data_dir),?1)>0 OR instr(LOWER(COALESCE(user_agent,'')),?1)>0 OR instr(LOWER(COALESCE(account_id,'')),?1)>0 OR instr(LOWER(COALESCE(notes,'')),?1)>0 OR instr(LOWER(COALESCE(locale,'')),?1)>0 OR instr(LOWER(COALESCE(timezone_id,'')),?1)>0)".to_string(), vec![q.clone()])
            };
            let total: i64 = c.query_row(&format!("SELECT COUNT(*) FROM browser_profiles{cond}"), params_from_iter(args.iter()), |r| r.get(0))?;
            let (limit, offset) = clamp_page(limit, offset);
            let sql = format!("SELECT {} FROM browser_profiles{cond} ORDER BY datetime(updated_at) DESC LIMIT {limit} OFFSET {offset}", Self::BP_COLS);
            let mut stmt = c.prepare(&sql)?;
            let out: Vec<BrowserProfile> = stmt.query_map(params_from_iter(args.iter()), Self::map_bp)?.flatten().collect();
            Ok((out, total))
        })
    }

    pub fn get_browser_profile(&self, id: &str) -> Result<BrowserProfile> {
        self.with(|c| {
            c.query_row(
                &format!(
                    "SELECT {} FROM browser_profiles WHERE id = ?1",
                    Self::BP_COLS
                ),
                params![id],
                Self::map_bp,
            )
            .optional()?
            .ok_or_else(|| anyhow!("browser profile not found"))
        })
    }

    pub fn delete_browser_profile(&self, id: &str) -> Result<()> {
        self.with(|c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM accounts WHERE browser_profile_id = ?1",
                params![id],
                |r| r.get(0),
            )?;
            if n > 0 {
                return Err(anyhow!("profile đang được {n} account sử dụng"));
            }
            c.execute("DELETE FROM browser_profiles WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    // ---------------- Resolve account for run ----------------

    /// Merge managed proxy + browser profile into the account row used for a run.
    /// Ported from store/resolve_account.go.
    pub fn resolve_account_for_run(&self, acc: &TikTokAccount) -> TikTokAccount {
        let mut out = acc.clone();
        if !acc.proxy_id.is_empty() {
            if let Ok(p) = self.get_proxy(&acc.proxy_id) {
                if !p.url.trim().is_empty() {
                    out.proxy = p.url;
                }
            }
        }
        if !acc.browser_profile_id.is_empty() {
            if let Ok(bp) = self.get_browser_profile(&acc.browser_profile_id) {
                if !bp.user_data_dir.trim().is_empty() {
                    out.profile_path = bp.user_data_dir;
                }
                if !bp.user_agent.trim().is_empty() {
                    out.user_agent = bp.user_agent;
                }
                if bp.viewport_width > 0 && bp.viewport_height > 0 {
                    out.viewport_width = bp.viewport_width;
                    out.viewport_height = bp.viewport_height;
                }
                if !bp.locale.trim().is_empty() {
                    out.locale = bp.locale;
                }
                if !bp.timezone_id.trim().is_empty() {
                    out.timezone_id = bp.timezone_id;
                }
            }
        }
        out
    }

    // ---------------- Saved flow actions ----------------

    pub fn list_saved_flow_actions(&self) -> Vec<SavedFlowAction> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id, name, step_json, updated_at FROM saved_flow_actions ORDER BY datetime(updated_at) DESC")?;
            let rows = stmt.query_map([], |r| {
                let step_json: String = r.get(2)?;
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, step_json, r.get::<_, String>(3)?))
            })?;
            let mut out = vec![];
            for (id, name, step_json, updated) in rows.flatten() {
                if let Ok(step) = serde_json::from_str::<FlowAction>(&step_json) {
                    out.push(SavedFlowAction { id, name, step, updated_at: updated });
                }
            }
            Ok(out)
        })
        .unwrap_or_default()
    }

    pub fn get_saved_flow_action(&self, id: &str) -> Result<SavedFlowAction> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT id, name, step_json, updated_at FROM saved_flow_actions WHERE id = ?1",
                    params![id.trim()],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()?;
            match row {
                Some((id, name, step_json, updated)) => {
                    let step = serde_json::from_str::<FlowAction>(&step_json)?;
                    Ok(SavedFlowAction {
                        id,
                        name,
                        step,
                        updated_at: updated,
                    })
                }
                None => Err(anyhow!("saved action not found")),
            }
        })
    }

    pub fn upsert_saved_flow_action(&self, mut in_: SavedFlowAction) -> Result<SavedFlowAction> {
        if in_.name.trim().is_empty() {
            in_.name = in_.step.name.trim().to_string();
        }
        if in_.name.trim().is_empty() {
            return Err(anyhow!("name is required"));
        }
        in_.step.type_ = "playwright_atomics".to_string();
        if in_.id.trim().is_empty() {
            in_.id = gen_id("sfa");
        }
        if in_.step.id.trim().is_empty() {
            in_.step.id = format!("step_saved_{}", in_.id);
        }
        in_.step.name = in_.name.trim().to_string();
        if in_.step.timeout <= 0 {
            in_.step.timeout = 60;
        }
        in_.updated_at = now_str();
        let step_json = serde_json::to_string(&in_.step)?;
        self.with(|c| {
            c.execute(
                r#"INSERT INTO saved_flow_actions (id, name, step_json, updated_at) VALUES (?1,?2,?3,?4)
                   ON CONFLICT(id) DO UPDATE SET name=excluded.name, step_json=excluded.step_json, updated_at=excluded.updated_at"#,
                params![in_.id, in_.name.trim(), step_json, in_.updated_at],
            )?;
            Ok(())
        })?;
        Ok(in_)
    }

    pub fn delete_saved_flow_action(&self, id: &str) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "DELETE FROM saved_flow_actions WHERE id = ?1",
                params![id.trim()],
            )?;
            if n == 0 {
                return Err(anyhow!("saved action not found"));
            }
            Ok(())
        })
    }

    // ---------------- Schedules ----------------

    pub fn upsert_schedule(&self, mut sc: Schedule) -> Schedule {
        if sc.id.trim().is_empty() {
            sc.id = gen_id("sch");
        }
        if sc.created_at.trim().is_empty() {
            sc.created_at = now_str();
        }
        sc.updated_at = now_str();
        let ids_json = serde_json::to_string(&sc.account_ids).unwrap_or_else(|_| "[]".into());
        let params_json = json_str_map(&sc.params);
        let _ = self.with(|c| {
            c.execute(
                r#"INSERT INTO schedules (id, name, enabled, flow_id, params_json, all_accounts, account_ids_json, type, daily_at, once_at, timezone_id, last_run_at, next_run_at, created_at, updated_at)
                   VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
                   ON CONFLICT(id) DO UPDATE SET name=excluded.name, enabled=excluded.enabled, flow_id=excluded.flow_id,
                     params_json=excluded.params_json, all_accounts=excluded.all_accounts, account_ids_json=excluded.account_ids_json,
                     type=excluded.type, daily_at=excluded.daily_at, once_at=excluded.once_at, timezone_id=excluded.timezone_id,
                     last_run_at=excluded.last_run_at, next_run_at=excluded.next_run_at, updated_at=excluded.updated_at"#,
                params![sc.id, sc.name, sc.enabled as i64, sc.flow_id, params_json, sc.all_accounts as i64, ids_json,
                        sc.type_, sc.daily_at, sc.once_at, sc.timezone_id, sc.last_run_at, sc.next_run_at, sc.created_at, sc.updated_at],
            )?;
            Ok(())
        });
        sc
    }

    fn map_schedule(r: &rusqlite::Row) -> rusqlite::Result<Schedule> {
        let params_json: String = r.get(4)?;
        let ids_json: String = r.get(6)?;
        Ok(Schedule {
            id: r.get(0)?,
            name: r.get(1)?,
            enabled: r.get::<_, i64>(2)? == 1,
            flow_id: r.get(3)?,
            params: parse_str_map(&params_json),
            all_accounts: r.get::<_, i64>(5)? == 1,
            account_ids: serde_json::from_str(&ids_json).unwrap_or_default(),
            type_: r.get(7)?,
            daily_at: r.get(8)?,
            once_at: r.get(9)?,
            timezone_id: r.get(10)?,
            last_run_at: r.get(11)?,
            next_run_at: r.get(12)?,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
        })
    }

    const SCH_COLS: &'static str = "id, name, enabled, flow_id, params_json, all_accounts, account_ids_json, type, daily_at, once_at, timezone_id, last_run_at, next_run_at, created_at, updated_at";

    pub fn list_schedules(&self) -> Vec<Schedule> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM schedules ORDER BY datetime(updated_at) DESC",
                Self::SCH_COLS
            ))?;
            let out: Vec<Schedule> = stmt.query_map([], Self::map_schedule)?.flatten().collect();
            Ok(out)
        })
        .unwrap_or_default()
    }

    pub fn get_schedule(&self, id: &str) -> Result<Schedule> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT {} FROM schedules WHERE id = ?1", Self::SCH_COLS),
                params![id],
                Self::map_schedule,
            )
            .optional()?
            .ok_or_else(|| anyhow!("schedule not found"))
        })
    }

    pub fn delete_schedule(&self, id: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM schedules WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    pub fn list_due_schedules(&self, now: &str) -> Vec<Schedule> {
        self.with(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {} FROM schedules WHERE enabled = 1 AND next_run_at != '' AND datetime(next_run_at) <= datetime(?1) ORDER BY datetime(next_run_at) ASC",
                Self::SCH_COLS
            ))?;
            let out: Vec<Schedule> = stmt.query_map(params![now], Self::map_schedule)?.flatten().collect();
            Ok(out)
        })
        .unwrap_or_default()
    }

    pub fn mark_schedule_run(&self, id: &str, last_run_at: &str, next_run_at: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE schedules SET last_run_at = ?1, next_run_at = ?2, updated_at = ?3 WHERE id = ?4",
                params![last_run_at, next_run_at, now_str(), id],
            )?;
            Ok(())
        })
    }

    // ---------------- Notifications ----------------

    pub fn upsert_notification_rule(&self, mut r: NotificationRule) -> NotificationRule {
        if r.id.trim().is_empty() {
            r.id = gen_id("nr");
        }
        if r.created_at.trim().is_empty() {
            r.created_at = now_str();
        }
        r.updated_at = now_str();
        let _ = self.with(|c| {
            c.execute(
                r#"INSERT INTO notification_rules (id, name, enabled, event, flow_id, account_id, message_template, created_at, updated_at)
                   VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                   ON CONFLICT(id) DO UPDATE SET name=excluded.name, enabled=excluded.enabled, event=excluded.event,
                     flow_id=excluded.flow_id, account_id=excluded.account_id, message_template=excluded.message_template,
                     updated_at=excluded.updated_at"#,
                params![r.id, r.name, r.enabled as i64, r.event, r.flow_id, r.account_id, r.message_template, r.created_at, r.updated_at],
            )?;
            Ok(())
        });
        r
    }

    fn map_rule(r: &rusqlite::Row) -> rusqlite::Result<NotificationRule> {
        Ok(NotificationRule {
            id: r.get(0)?,
            name: r.get(1)?,
            enabled: r.get::<_, i64>(2)? == 1,
            event: r.get(3)?,
            flow_id: r.get(4)?,
            account_id: r.get(5)?,
            message_template: r.get(6)?,
            created_at: r.get(7)?,
            updated_at: r.get(8)?,
        })
    }

    pub fn list_notification_rules(&self) -> Vec<NotificationRule> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id, name, enabled, event, flow_id, account_id, message_template, created_at, updated_at FROM notification_rules ORDER BY datetime(updated_at) DESC")?;
            let out: Vec<NotificationRule> = stmt.query_map([], Self::map_rule)?.flatten().collect();
            Ok(out)
        })
        .unwrap_or_default()
    }

    pub fn delete_notification_rule(&self, id: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM notification_rules WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    pub fn create_notification(&self, mut n: Notification) {
        if n.id.trim().is_empty() {
            n.id = gen_id("ntf");
        }
        if n.created_at.trim().is_empty() {
            n.created_at = now_str();
        }
        let _ = self.with(|c| {
            c.execute(
                r#"INSERT INTO notifications (id, rule_id, event, title, body, run_id, account_id, flow_id, read_at, created_at)
                   VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"#,
                params![n.id, n.rule_id, n.event, n.title, n.body, n.run_id, n.account_id, n.flow_id, n.read_at, n.created_at],
            )?;
            Ok(())
        });
    }

    pub fn list_notifications(&self, unread_only: bool, limit: i64) -> Vec<Notification> {
        let limit = if limit <= 0 { 50 } else { limit };
        let where_ = if unread_only {
            "WHERE read_at = ''"
        } else {
            ""
        };
        self.with(|c| {
            let sql = format!("SELECT id, rule_id, event, title, body, run_id, account_id, flow_id, read_at, created_at FROM notifications {where_} ORDER BY datetime(created_at) DESC LIMIT ?1");
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(params![limit], |r| {
                Ok(Notification {
                    id: r.get(0)?,
                    rule_id: r.get(1)?,
                    event: r.get(2)?,
                    title: r.get(3)?,
                    body: r.get(4)?,
                    run_id: r.get(5)?,
                    account_id: r.get(6)?,
                    flow_id: r.get(7)?,
                    read_at: r.get(8)?,
                    created_at: r.get(9)?,
                })
            })?;
            Ok(rows.flatten().collect())
        })
        .unwrap_or_default()
    }

    pub fn mark_notification_read(&self, id: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE notifications SET read_at = ?1 WHERE id = ?2 AND read_at = ''",
                params![now_str(), id],
            )?;
            Ok(())
        })
    }

    pub fn mark_all_notifications_read(&self) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE notifications SET read_at = ?1 WHERE read_at = ''",
                params![now_str()],
            )?;
            Ok(())
        })
    }

    pub fn count_unread_notifications(&self) -> i64 {
        self.with(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM notifications WHERE read_at = ''",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap_or(0)
    }

    // ---------------- Agent skills ----------------

    fn map_skill(r: &rusqlite::Row) -> rusqlite::Result<AgentSkill> {
        Ok(AgentSkill {
            id: r.get(0)?,
            name: r.get(1)?,
            description: r.get(2)?,
            body: r.get(3)?,
            enabled: r.get::<_, i64>(4)? != 0,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
        })
    }

    pub fn list_agent_skills(&self) -> Result<Vec<AgentSkill>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id, name, description, body_md, enabled, created_at, updated_at FROM agent_skills ORDER BY datetime(updated_at) DESC")?;
            let out: Vec<AgentSkill> = stmt.query_map([], Self::map_skill)?.flatten().collect();
            Ok(out)
        })
    }

    pub fn get_agent_skill(&self, id: &str) -> Result<AgentSkill> {
        let id = id.trim();
        if id.is_empty() {
            return Err(anyhow!("skill id empty"));
        }
        self.with(|c| {
            c.query_row("SELECT id, name, description, body_md, enabled, created_at, updated_at FROM agent_skills WHERE id = ?1", params![id], Self::map_skill)
                .optional()?
                .ok_or_else(|| anyhow!("skill not found"))
        })
    }

    pub fn create_agent_skill(&self, in_: &mut AgentSkill) -> Result<()> {
        in_.name = in_.name.trim().to_string();
        if in_.name.is_empty() {
            return Err(anyhow!("name required"));
        }
        in_.id = in_.id.trim().to_string();
        if in_.id.is_empty() {
            return Err(anyhow!("id required"));
        }
        if in_.created_at.trim().is_empty() {
            in_.created_at = now_str();
        }
        in_.updated_at = now_str();
        self.with(|c| {
            let existing: i64 = c.query_row("SELECT COUNT(*) FROM agent_skills WHERE id = ?1", params![in_.id], |r| r.get(0))?;
            if existing > 0 {
                return Err(anyhow!("skill id already exists"));
            }
            c.execute(
                "INSERT INTO agent_skills (id, name, description, body_md, enabled, created_at, updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![in_.id, in_.name, in_.description, in_.body, in_.enabled as i64, in_.created_at, in_.updated_at],
            )?;
            Ok(())
        })
    }

    pub fn update_agent_skill(&self, in_: &mut AgentSkill) -> Result<()> {
        in_.id = in_.id.trim().to_string();
        if in_.id.is_empty() {
            return Err(anyhow!("skill id empty"));
        }
        in_.name = in_.name.trim().to_string();
        if in_.name.is_empty() {
            return Err(anyhow!("name required"));
        }
        in_.updated_at = now_str();
        self.with(|c| {
            let n = c.execute(
                "UPDATE agent_skills SET name=?1, description=?2, body_md=?3, enabled=?4, updated_at=?5 WHERE id=?6",
                params![in_.name, in_.description, in_.body, in_.enabled as i64, in_.updated_at, in_.id],
            )?;
            if n == 0 {
                return Err(anyhow!("skill not found"));
            }
            Ok(())
        })
    }

    pub fn delete_agent_skill(&self, id: &str) -> Result<()> {
        let id = id.trim();
        if id.is_empty() {
            return Err(anyhow!("skill id empty"));
        }
        self.with(|c| {
            let n = c.execute("DELETE FROM agent_skills WHERE id = ?1", params![id])?;
            if n == 0 {
                return Err(anyhow!("skill not found"));
            }
            Ok(())
        })
    }

    // ---------------- Account activity ----------------

    pub fn record_post_interaction(
        &self,
        account_id: &str,
        post_key: &str,
        interaction_type: &str,
        post_url: &str,
        author_username: &str,
        extra_json: &str,
    ) -> Result<()> {
        let account_id = account_id.trim();
        let post_key = post_key.trim();
        if account_id.is_empty() || post_key.is_empty() {
            return Err(anyhow!(
                "RecordPostInteraction: thiếu account_id hoặc post_key"
            ));
        }
        let it = if interaction_type.trim().is_empty() {
            "interaction"
        } else {
            interaction_type.trim()
        };
        self.with(|c| {
            c.execute(
                "INSERT INTO account_post_interactions (id, account_id, post_key, interaction_type, post_url, author_username, extra_json, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![gen_id("api"), account_id, post_key, it, post_url, author_username, extra_json, now_str()],
            )?;
            Ok(())
        })
    }

    pub fn record_friend_event(
        &self,
        account_id: &str,
        target_username: &str,
        target_user_id: &str,
        event_type: &str,
        notes: &str,
    ) -> Result<()> {
        let account_id = account_id.trim();
        let event_type = event_type.trim().to_lowercase();
        if account_id.is_empty() || event_type.is_empty() {
            return Err(anyhow!(
                "RecordFriendEvent: thiếu account_id hoặc event_type"
            ));
        }
        self.with(|c| {
            c.execute(
                "INSERT INTO account_friend_events (id, account_id, target_username, target_user_id, event_type, notes, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![gen_id("afe"), account_id, target_username, target_user_id, event_type, notes, now_str()],
            )?;
            Ok(())
        })
    }

    pub fn upsert_account_kv_meta(&self, account_id: &str, key: &str, value: &str) -> Result<()> {
        let account_id = account_id.trim();
        let key = key.trim();
        if account_id.is_empty() || key.is_empty() {
            return Err(anyhow!(
                "UpsertAccountKVMeta: thiếu account_id hoặc meta_key"
            ));
        }
        self.with(|c| {
            c.execute(
                "INSERT INTO account_kv_meta (account_id, meta_key, meta_value, updated_at) VALUES (?1,?2,?3,?4) ON CONFLICT(account_id, meta_key) DO UPDATE SET meta_value=excluded.meta_value, updated_at=excluded.updated_at",
                params![account_id, key, value, now_str()],
            )?;
            Ok(())
        })
    }

    pub fn delete_account_kv_meta(&self, account_id: &str, key: &str) -> Result<()> {
        let account_id = account_id.trim();
        let key = key.trim();
        if account_id.is_empty() || key.is_empty() {
            return Err(anyhow!(
                "DeleteAccountKVMeta: thiếu account_id hoặc meta_key"
            ));
        }
        self.with(|c| {
            c.execute(
                "DELETE FROM account_kv_meta WHERE account_id = ?1 AND meta_key = ?2",
                params![account_id, key],
            )?;
            Ok(())
        })
    }

    pub fn list_post_interactions_by_account(
        &self,
        account_id: &str,
        limit: i64,
    ) -> Result<Vec<AccountPostInteractionRow>> {
        let account_id = account_id.trim();
        if account_id.is_empty() {
            return Err(anyhow!("thiếu account_id"));
        }
        let limit = if limit <= 0 || limit > 2000 {
            200
        } else {
            limit
        };
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id, account_id, post_key, interaction_type, post_url, author_username, extra_json, created_at FROM account_post_interactions WHERE account_id = ?1 ORDER BY datetime(created_at) DESC LIMIT ?2")?;
            let rows = stmt.query_map(params![account_id, limit], |r| {
                Ok(AccountPostInteractionRow {
                    id: r.get(0)?, account_id: r.get(1)?, post_key: r.get(2)?, interaction_type: r.get(3)?,
                    post_url: r.get(4)?, author_username: r.get(5)?, extra_json: r.get(6)?, created_at: r.get(7)?,
                })
            })?;
            Ok(rows.flatten().collect())
        })
    }

    pub fn list_friend_events_by_account(
        &self,
        account_id: &str,
        limit: i64,
    ) -> Result<Vec<AccountFriendEventRow>> {
        let account_id = account_id.trim();
        if account_id.is_empty() {
            return Err(anyhow!("thiếu account_id"));
        }
        let limit = if limit <= 0 || limit > 2000 {
            200
        } else {
            limit
        };
        self.with(|c| {
            let mut stmt = c.prepare("SELECT id, account_id, target_username, target_user_id, event_type, notes, created_at FROM account_friend_events WHERE account_id = ?1 ORDER BY datetime(created_at) DESC LIMIT ?2")?;
            let rows = stmt.query_map(params![account_id, limit], |r| {
                Ok(AccountFriendEventRow {
                    id: r.get(0)?, account_id: r.get(1)?, target_username: r.get(2)?, target_user_id: r.get(3)?,
                    event_type: r.get(4)?, notes: r.get(5)?, created_at: r.get(6)?,
                })
            })?;
            Ok(rows.flatten().collect())
        })
    }

    pub fn list_account_kv_meta(&self, account_id: &str) -> Result<Vec<AccountKVMetaRow>> {
        let account_id = account_id.trim();
        if account_id.is_empty() {
            return Err(anyhow!("thiếu account_id"));
        }
        self.with(|c| {
            let mut stmt = c.prepare("SELECT account_id, meta_key, meta_value, updated_at FROM account_kv_meta WHERE account_id = ?1 ORDER BY meta_key")?;
            let rows = stmt.query_map(params![account_id], |r| {
                Ok(AccountKVMetaRow { account_id: r.get(0)?, key: r.get(1)?, value: r.get(2)?, updated_at: r.get(3)? })
            })?;
            Ok(rows.flatten().collect())
        })
    }

    // ---------------- App settings ----------------

    pub fn get_app_settings(&self) -> Result<AppSettings> {
        self.with(|c| {
            let raw: Option<String> = c
                .query_row(
                    "SELECT value_json FROM app_settings WHERE key = 'app'",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            match raw {
                Some(s) => Ok(serde_json::from_str(&s).unwrap_or_default()),
                None => Ok(AppSettings::default()),
            }
        })
    }

    pub fn upsert_app_settings(&self, v: &AppSettings) -> Result<()> {
        let raw = serde_json::to_string(v)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO app_settings (key, value_json, updated_at) VALUES ('app', ?1, ?2) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
                params![raw, now_str()],
            )?;
            Ok(())
        })
    }

    // ---------------- Engine KV (legacy atomic rules) ----------------

    pub fn get_legacy_atomic_rules_json(&self) -> Result<String> {
        self.with(|c| {
            let v: Option<String> = c
                .query_row(
                    "SELECT value FROM engine_kv WHERE key = 'legacy_atomic_rules'",
                    [],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(v.unwrap_or_default())
        })
    }

    pub fn set_legacy_atomic_rules_json(&self, json: &str) -> Result<()> {
        let json = json.trim();
        self.with(|c| {
            if json.is_empty() {
                c.execute("DELETE FROM engine_kv WHERE key = 'legacy_atomic_rules'", [])?;
            } else {
                c.execute(
                    "INSERT INTO engine_kv (key, value, updated_at) VALUES ('legacy_atomic_rules', ?1, ?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
                    params![json, now_str()],
                )?;
            }
            Ok(())
        })
    }
}

fn clamp_page(mut limit: i64, mut offset: i64) -> (i64, i64) {
    if limit <= 0 {
        limit = 20;
    }
    if limit > 500 {
        limit = 500;
    }
    if offset < 0 {
        offset = 0;
    }
    (limit, offset)
}
