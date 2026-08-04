//! DAG execution engine — port of `internal/agent/dag` (task.go, state.go,
//! engine.go). Polls dag_parents/dag_tasks every 500ms, runs ready tasks via
//! the agent pool (max 5 concurrent) and mirrors the Go lifecycle:
//! registered → active → done | error | timeout. Cancellation is hand-rolled
//! with a oneshot sender stored per running task (no tokio_util dep).

use crate::agents::{self, Pool};
use crate::db::{self, Db};
use crate::state::Core;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{oneshot, Semaphore};

// ---- task model (task.go) ----

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Registered,
    Active,
    Done,
    Error,
    Timeout,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Registered => "registered",
            Status::Active => "active",
            Status::Done => "done",
            Status::Error => "error",
            Status::Timeout => "timeout",
        }
    }

    pub fn parse(s: &str) -> Status {
        match s {
            "registered" => Status::Registered,
            "active" => Status::Active,
            "done" => Status::Done,
            "timeout" => Status::Timeout,
            _ => Status::Error,
        }
    }
}

/// DAG execution record (mirrors the dag_tasks table).
#[derive(Clone, Debug)]
pub struct Task {
    pub id: String,
    pub parent_id: String,
    pub label: String,
    pub agent_type: String,
    pub prompt: String,
    pub depends_on: Vec<String>,
    pub input_from: Vec<String>,
    pub status: Status,
    pub result: String,
    pub timeout_seconds: i64,
}

/// In-memory dag_parents row + its tasks.
#[derive(Clone, Debug)]
pub struct Parent {
    pub id: String,
    pub project_id: String,
    pub status: String,
    pub goal: String,
    pub orientation: String,
    pub tasks: Vec<Task>,
}

/// Decode the JSON depends_on/input_from column (tolerant, like the Go callers).
pub fn parse_depends_on(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw == "null" || raw == "[]" {
        return Vec::new();
    }
    serde_json::from_str::<Vec<String>>(raw).unwrap_or_default()
}

pub fn encode_depends_on(deps: &[String]) -> String {
    if deps.is_empty() {
        return "[]".to_string();
    }
    serde_json::to_string(deps).unwrap_or_else(|_| "[]".to_string())
}

/// Kahn topological sort; error on cycle.
pub fn topological_sort(tasks: &[Task]) -> Result<Vec<Task>, String> {
    let by_label: HashMap<&str, &Task> = tasks.iter().map(|t| (t.label.as_str(), t)).collect();
    let mut in_degree: HashMap<&str, usize> = tasks.iter().map(|t| (t.label.as_str(), 0)).collect();
    for t in tasks {
        *in_degree.entry(t.label.as_str()).or_insert(0) += t.depends_on.len();
    }
    let mut queue: Vec<&str> = tasks
        .iter()
        .filter(|t| in_degree.get(t.label.as_str()) == Some(&0))
        .map(|t| t.label.as_str())
        .collect();
    let mut sorted: Vec<Task> = Vec::with_capacity(tasks.len());
    while let Some(label) = queue.first().copied() {
        queue.remove(0);
        if let Some(t) = by_label.get(label) {
            sorted.push((*t).clone());
        }
        for other in tasks {
            for dep in &other.depends_on {
                if dep == label {
                    let d = in_degree.entry(other.label.as_str()).or_insert(0);
                    if *d > 0 {
                        *d -= 1;
                        if *d == 0 {
                            queue.push(other.label.as_str());
                        }
                    }
                }
            }
        }
    }
    if sorted.len() != tasks.len() {
        return Err("dag: cycle detected in tasks".to_string());
    }
    Ok(sorted)
}

/// Registered tasks whose deps are ALL done. Failed-dep tasks are not ready —
/// see `blocked_tasks`.
pub fn ready_tasks(tasks: &[Task]) -> Vec<Task> {
    let status_by_label: HashMap<&str, Status> =
        tasks.iter().map(|t| (t.label.as_str(), t.status)).collect();
    tasks
        .iter()
        .filter(|t| {
            t.status == Status::Registered
                && t.depends_on
                    .iter()
                    .all(|d| status_by_label.get(d.as_str()) == Some(&Status::Done))
        })
        .cloned()
        .collect()
}

/// Registered tasks with at least one errored/timed-out dependency, paired
/// with the first failing dep label.
pub fn blocked_tasks(tasks: &[Task]) -> Vec<(Task, String)> {
    let status_by_label: HashMap<&str, Status> =
        tasks.iter().map(|t| (t.label.as_str(), t.status)).collect();
    let mut blocked = Vec::new();
    for t in tasks {
        if t.status != Status::Registered {
            continue;
        }
        for dep in &t.depends_on {
            match status_by_label.get(dep.as_str()) {
                Some(Status::Error) | Some(Status::Timeout) => {
                    blocked.push((t.clone(), dep.clone()));
                    break;
                }
                _ => {}
            }
        }
    }
    blocked
}

/// All tasks in a terminal state.
pub fn is_done(tasks: &[Task]) -> bool {
    !tasks
        .iter()
        .any(|t| t.status == Status::Registered || t.status == Status::Active)
}

// ---- persistence (state.go) ----

pub fn load_tasks(db: &Db, parent_id: &str) -> Result<Vec<Task>, String> {
    let rows = db
        .query(
            "SELECT * FROM dag_tasks WHERE parent_id = ?1 ORDER BY rowid",
            &[&parent_id],
        )
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| {
            let mut timeout = db::i64_of(&r, "timeout_seconds");
            if timeout <= 0 {
                timeout = 900;
            }
            Task {
                id: db::str_of(&r, "id"),
                parent_id: parent_id.to_string(),
                label: db::str_of(&r, "label"),
                agent_type: db::str_of(&r, "agent_type"),
                prompt: db::str_of(&r, "prompt"),
                depends_on: parse_depends_on(&db::str_of(&r, "depends_on")),
                input_from: parse_depends_on(&db::str_of(&r, "input_from")),
                status: Status::parse(&db::str_of(&r, "status")),
                result: db::str_of(&r, "result"),
                timeout_seconds: timeout,
            }
        })
        .collect())
}

pub fn load_parent(db: &Db, parent_id: &str) -> Result<Parent, String> {
    let row = db
        .get("dag_parents", parent_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("dag parent {parent_id:?} not found"))?;
    Ok(Parent {
        id: db::str_of(&row, "id"),
        project_id: db::str_of(&row, "project_id"),
        status: db::str_of(&row, "status"),
        goal: db::str_of(&row, "goal"),
        orientation: db::str_of(&row, "orientation"),
        tasks: load_tasks(db, parent_id)?,
    })
}

fn load_active_parents(db: &Db) -> Result<Vec<Parent>, String> {
    let rows = db
        .query(
            "SELECT * FROM dag_parents WHERE status IN ('queued','active') ORDER BY created_at",
            &[],
        )
        .map_err(|e| e.to_string())?;
    let mut parents = Vec::with_capacity(rows.len());
    for r in rows {
        let pid = db::str_of(&r, "id");
        let tasks = match load_tasks(db, &pid) {
            Ok(t) => t,
            Err(_) => continue,
        };
        parents.push(Parent {
            id: pid,
            project_id: db::str_of(&r, "project_id"),
            status: db::str_of(&r, "status"),
            goal: db::str_of(&r, "goal"),
            orientation: db::str_of(&r, "orientation"),
            tasks,
        });
    }
    Ok(parents)
}

/// Persist a task status change; `result` is stored as JSON text, empty
/// started_at/completed_at leave those columns untouched.
pub fn update_task(
    db: &Db,
    task_id: &str,
    status: Status,
    result: Option<Value>,
    started_at: &str,
    completed_at: &str,
) -> Result<(), String> {
    let mut fields = Map::new();
    fields.insert("status".into(), json!(status.as_str()));
    if let Some(r) = result {
        fields.insert("result".into(), Value::String(r.to_string()));
    }
    if !started_at.is_empty() {
        fields.insert("started_at".into(), json!(started_at));
    }
    if !completed_at.is_empty() {
        fields.insert("completed_at".into(), json!(completed_at));
    }
    db.update("dag_tasks", task_id, &fields)
        .map_err(|e| e.to_string())
}

pub fn update_parent_status(db: &Db, parent_id: &str, status: &str) -> Result<(), String> {
    let mut fields = Map::new();
    fields.insert("status".into(), json!(status));
    db.update("dag_parents", parent_id, &fields)
        .map_err(|e| e.to_string())
}

// ---- engine (engine.go) ----

enum ActiveEntry {
    /// Claimed by the tick loop, cancel channel not yet installed.
    Claimed,
    Running(oneshot::Sender<()>),
}

pub struct Engine {
    core: Arc<Core>,
    pool: Arc<Pool>,
    active: Mutex<HashMap<String, ActiveEntry>>,
    sem: Arc<Semaphore>,
}

const POLL_MS: u64 = 500;
const MAX_CONCURRENT: usize = 5;

impl Engine {
    pub fn new(core: Arc<Core>, pool: Arc<Pool>) -> Arc<Engine> {
        Arc::new(Engine {
            core,
            pool,
            active: Mutex::new(HashMap::new()),
            sem: Arc::new(Semaphore::new(MAX_CONCURRENT)),
        })
    }

    /// Launch the background polling loop.
    pub fn start(self: Arc<Self>) {
        eprintln!("[dag] engine started (poll={POLL_MS}ms maxConc={MAX_CONCURRENT})");
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(POLL_MS));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                Engine::tick_once(&self);
            }
        });
    }

    /// Cancel a running task; no-op when the task isn't active.
    pub fn stop_task(&self, task_id: &str) {
        let mut map = self.active.lock().unwrap();
        match map.remove(task_id) {
            Some(ActiveEntry::Running(tx)) => {
                // Keep the claim so the tick loop can't relaunch before cleanup.
                map.insert(task_id.to_string(), ActiveEntry::Claimed);
                let _ = tx.send(());
            }
            Some(ActiveEntry::Claimed) => {
                map.insert(task_id.to_string(), ActiveEntry::Claimed);
            }
            None => {}
        }
    }

    fn tick_once(eng: &Arc<Engine>) {
        let db = &eng.core.db;
        let parents = match load_active_parents(db) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[dag] load active parents: {e}");
                return;
            }
        };
        for mut p in parents {
            if p.status == "queued" {
                let _ = update_parent_status(db, &p.id, "active");
                p.status = "active".to_string();
            }
            // Mark tasks whose dependencies failed as error (blocked) before
            // launching ready ones.
            for (t, dep) in blocked_tasks(&p.tasks) {
                let now = db::now();
                let msg = format!("blocked: {dep} failed");
                let _ = update_task(
                    db,
                    &t.id,
                    Status::Error,
                    Some(json!({"error": msg})),
                    &now,
                    &now,
                );
                eng.emit_agent_state(&p.id, &t, "error", Some(&msg), None);
                eprintln!("[dag] task {} blocked by failed dep {dep:?}", short(&t.id));
            }

            for t in ready_tasks(&p.tasks) {
                {
                    let mut map = eng.active.lock().unwrap();
                    if map.contains_key(&t.id) {
                        continue; // already running or claimed
                    }
                    map.insert(t.id.clone(), ActiveEntry::Claimed);
                }
                match eng.sem.clone().try_acquire_owned() {
                    Ok(permit) => {
                        let engine = eng.clone();
                        let parent = p.clone();
                        tokio::spawn(async move {
                            engine.run_task(permit, parent, t).await;
                        });
                    }
                    Err(_) => {
                        eng.active.lock().unwrap().remove(&t.id);
                    }
                }
            }

            if is_done(&p.tasks) && p.status == "active" {
                let _ = update_parent_status(db, &p.id, "done");
                eng.core.dash.emit(
                    "pipeline:updated",
                    json!({"pipeline_id": p.id, "status": "done"}),
                );
            }
        }
    }

    async fn run_task(
        self: Arc<Self>,
        _permit: tokio::sync::OwnedSemaphorePermit,
        p: Parent,
        t: Task,
    ) {
        let (cancel_tx, mut cancel_rx) = oneshot::channel::<()>();
        self.active
            .lock()
            .unwrap()
            .insert(t.id.clone(), ActiveEntry::Running(cancel_tx));
        let db = self.core.db.clone();

        // Skip disabled built-ins as done/skipped.
        if db.builtin_agent_disabled(&t.agent_type) {
            let now = db::now();
            let _ = update_task(
                &db,
                &t.id,
                Status::Done,
                Some(json!({"skipped": true, "reason": "builtin_agent_disabled"})),
                &now,
                &now,
            );
            self.emit_agent_state(
                &p.id,
                &t,
                "done",
                None,
                Some("skipped: agent disabled in settings"),
            );
            self.emit_pipeline_progress(&p.id);
            self.active.lock().unwrap().remove(&t.id);
            return;
        }

        eprintln!(
            "[dag] start task {} label={} agent={}",
            short(&t.id),
            t.label,
            t.agent_type
        );
        let now = db::now();
        let _ = update_task(&db, &t.id, Status::Active, None, &now, "");
        self.emit_agent_state(&p.id, &t, "active", None, None);

        // Upstream results for prompt injection (token-conscious):
        // input_from when non-empty (explicit override), else depends_on labels.
        let want: &[String] = if !t.input_from.is_empty() {
            &t.input_from
        } else {
            &t.depends_on
        };
        let mut upstream: HashMap<String, String> = HashMap::new();
        for sib in &p.tasks {
            if sib.status != Status::Done || sib.result.is_empty() {
                continue;
            }
            if want.iter().any(|w| w == &sib.label) {
                upstream.insert(sib.label.clone(), sib.result.clone());
            }
        }

        let agent_task = agents::Task {
            id: t.id.clone(),
            label: t.label.clone(),
            agent_type: t.agent_type.clone(),
            prompt: t.prompt.clone(),
            timeout_seconds: t.timeout_seconds,
            upstream_results: upstream,
        };

        enum Outcome {
            Done(agents::TaskResult),
            Failed(String),
            Cancelled,
            TimedOut,
        }

        let outcome = {
            let exec = self.pool.execute(&agent_task, &p.id, &p.project_id);
            tokio::pin!(exec);
            let dur = if t.timeout_seconds > 0 {
                Duration::from_secs(t.timeout_seconds as u64)
            } else {
                Duration::from_secs(3600)
            };
            let timeout = tokio::time::sleep(dur);
            tokio::pin!(timeout);
            tokio::select! {
                r = &mut exec => match r {
                    Ok(v) => Outcome::Done(v),
                    Err(e) => Outcome::Failed(e),
                },
                _ = &mut cancel_rx => Outcome::Cancelled,
                _ = &mut timeout, if t.timeout_seconds > 0 => Outcome::TimedOut,
            }
        };

        let done_at = db::now();
        match outcome {
            Outcome::Done(result) => {
                eprintln!("[dag] task {} done: {}", short(&t.id), result.summary);
                let _ = update_task(
                    &db,
                    &t.id,
                    Status::Done,
                    Some(Value::Object(result.data.clone())),
                    "",
                    &done_at,
                );
                self.emit_agent_state(&p.id, &t, "done", None, Some(&result.summary));
                self.emit_pipeline_progress(&p.id);
            }
            Outcome::Failed(e) => {
                eprintln!("[dag] task {} error: {e}", short(&t.id));
                let _ = update_task(
                    &db,
                    &t.id,
                    Status::Error,
                    Some(json!({"error": e})),
                    "",
                    &done_at,
                );
                self.emit_agent_state(&p.id, &t, "error", Some(&e), None);
            }
            Outcome::Cancelled => {
                let msg = "task cancelled".to_string();
                eprintln!("[dag] task {} cancelled", short(&t.id));
                let _ = update_task(
                    &db,
                    &t.id,
                    Status::Error,
                    Some(json!({"error": msg})),
                    "",
                    &done_at,
                );
                self.emit_agent_state(&p.id, &t, "error", Some(&msg), None);
            }
            Outcome::TimedOut => {
                let msg = format!("task timeout after {}s", t.timeout_seconds);
                eprintln!("[dag] task {} timeout", short(&t.id));
                let _ = update_task(
                    &db,
                    &t.id,
                    Status::Timeout,
                    Some(json!({"error": msg})),
                    "",
                    &done_at,
                );
                self.emit_agent_state(&p.id, &t, "timeout", Some(&msg), None);
            }
        }
        self.active.lock().unwrap().remove(&t.id);
    }

    fn emit_agent_state(
        &self,
        pipeline_id: &str,
        t: &Task,
        status: &str,
        error: Option<&str>,
        summary: Option<&str>,
    ) {
        let mut d = Map::new();
        d.insert("task_id".into(), json!(t.id));
        d.insert("label".into(), json!(t.label));
        d.insert("agent_type".into(), json!(t.agent_type));
        d.insert("status".into(), json!(status));
        if let Some(e) = error {
            d.insert("error".into(), json!(e));
        }
        if let Some(s) = summary {
            d.insert("summary".into(), json!(s));
        }
        d.insert("pipeline_id".into(), json!(pipeline_id));
        self.core.dash.emit("agent:state", Value::Object(d));
    }

    fn emit_pipeline_progress(&self, parent_id: &str) {
        let parents = match load_active_parents(&self.core.db) {
            Ok(p) => p,
            Err(_) => return,
        };
        for p in parents {
            if p.id != parent_id {
                continue;
            }
            let total = p.tasks.len();
            let done = p
                .tasks
                .iter()
                .filter(|t| matches!(t.status, Status::Done | Status::Error | Status::Timeout))
                .count();
            self.core.dash.emit(
                "pipeline:updated",
                json!({
                    "pipeline_id": p.id,
                    "status": p.status,
                    "completed_tasks": done,
                    "total_tasks": total,
                }),
            );
            return;
        }
    }
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(label: &str, deps: &[&str], status: Status) -> Task {
        Task {
            id: format!("id-{label}"),
            parent_id: "p".into(),
            label: label.into(),
            agent_type: label.into(),
            prompt: String::new(),
            depends_on: deps.iter().map(|s| s.to_string()).collect(),
            input_from: Vec::new(),
            status,
            result: String::new(),
            timeout_seconds: 900,
        }
    }

    #[test]
    fn topological_sort_respects_deps() {
        let tasks = vec![
            t("c", &["b"], Status::Registered),
            t("a", &[], Status::Registered),
            t("b", &["a"], Status::Registered),
        ];
        let sorted = topological_sort(&tasks).unwrap();
        let pos: HashMap<&str, usize> = sorted
            .iter()
            .enumerate()
            .map(|(i, x)| (x.label.as_str(), i))
            .collect();
        assert!(pos["a"] < pos["b"]);
        assert!(pos["b"] < pos["c"]);
        assert_eq!(sorted.len(), 3);
    }

    #[test]
    fn topological_sort_detects_cycle() {
        let tasks = vec![
            t("a", &["b"], Status::Registered),
            t("b", &["a"], Status::Registered),
        ];
        assert!(topological_sort(&tasks).is_err());
    }

    #[test]
    fn ready_blocked_done_helpers() {
        let tasks = vec![
            t("a", &[], Status::Done),
            t("b", &["a"], Status::Registered),
            t("c", &["missing_or_failed"], Status::Registered),
            t("missing_or_failed", &[], Status::Error),
        ];
        let ready = ready_tasks(&tasks);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].label, "b");

        let blocked = blocked_tasks(&tasks);
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].0.label, "c");
        assert_eq!(blocked[0].1, "missing_or_failed");

        assert!(!is_done(&tasks));
        let terminal = vec![t("a", &[], Status::Done), t("b", &[], Status::Timeout)];
        assert!(is_done(&terminal));
    }

    #[test]
    fn depends_on_roundtrip() {
        assert!(parse_depends_on("").is_empty());
        assert!(parse_depends_on("null").is_empty());
        assert!(parse_depends_on("[]").is_empty());
        let deps = vec!["a".to_string(), "b".to_string()];
        let enc = encode_depends_on(&deps);
        assert_eq!(parse_depends_on(&enc), deps);
        assert_eq!(encode_depends_on(&[]), "[]");
    }
}
