//! Run lifecycle + scheduler — ported from cmd/server/schedules.go.
//!
//! `start_flow_run` persists a queued run and spawns `execute_run` on a tokio
//! task (the Go code used a goroutine). The log callback appends to the run and
//! calls `update_run` on every line — chatty by design, since the logs ARE the
//! run record. Final status fires notification rules.

use crate::db::{gen_id, now_str, Db};
use crate::domain::*;
use crate::engine::{Cancel, Runner};
use anyhow::{anyhow, Result};
use chrono::{Datelike, TimeZone};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct RunManager {
    db: Db,
    runner: Arc<Runner>,
    /// run_id -> cancel flag, so a run can be stopped mid-flight.
    cancels: Arc<Mutex<HashMap<String, Cancel>>>,
}

impl RunManager {
    pub fn new(db: Db, runner: Arc<Runner>) -> Self {
        Self { db, runner, cancels: Arc::new(Mutex::new(HashMap::new())) }
    }

    pub fn find_account(&self, id: &str) -> Result<TikTokAccount> {
        self.db.get_account(id).ok_or_else(|| anyhow!("account not found"))
    }

    pub fn start_flow_run(
        &self,
        account: TikTokAccount,
        flow_id: &str,
        schedule_id: &str,
        run_params: Option<StrMap>,
    ) -> Result<FlowRun> {
        let flow = self.db.get_flow(flow_id)?;
        let run = FlowRun {
            id: gen_id("run"),
            account_id: account.id.clone(),
            flow_id: flow_id.to_string(),
            schedule_id: schedule_id.to_string(),
            status: RUN_QUEUED.to_string(),
            logs: vec![],
            started_at: now_str(),
            ended_at: String::new(),
        };
        self.db.save_run(&run);

        // Merge flow defaults with per-run params (run params win).
        let mut merged: StrMap = StrMap::new();
        if let Some(p) = &flow.params {
            for (k, v) in p {
                if !k.trim().is_empty() {
                    merged.insert(k.clone(), v.clone());
                }
            }
        }
        if let Some(p) = run_params {
            for (k, v) in p {
                if !k.trim().is_empty() {
                    merged.insert(k.clone(), v.clone());
                }
            }
        }

        let this = self.clone();
        let run_clone = run.clone();
        tokio::spawn(async move {
            this.execute_run(account, flow, run_clone, merged).await;
        });
        Ok(run)
    }

    pub fn start_browser_preview(&self, account: TikTokAccount) -> Result<FlowRun> {
        let flow = browser_preview_flow();
        let run = FlowRun {
            id: gen_id("run"),
            account_id: account.id.clone(),
            flow_id: flow.id.clone(),
            schedule_id: String::new(),
            status: RUN_QUEUED.to_string(),
            logs: vec![],
            started_at: now_str(),
            ended_at: String::new(),
        };
        self.db.save_run(&run);
        let this = self.clone();
        let run_clone = run.clone();
        tokio::spawn(async move {
            this.execute_run(account, flow, run_clone, StrMap::new()).await;
        });
        Ok(run)
    }

    pub fn cancel_run(&self, run_id: &str) -> bool {
        if let Some(c) = self.cancels.lock().unwrap().get(run_id) {
            c.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    async fn execute_run(&self, account: TikTokAccount, flow: Flow, mut run: FlowRun, params: StrMap) {
        run.status = RUN_RUNNING.to_string();
        self.db.update_run(&run);

        let resolved = self.db.resolve_account_for_run(&account);

        // Shared, mutable run record for the log callback.
        let shared = Arc::new(Mutex::new(run.clone()));
        let cancel: Cancel = Arc::new(AtomicBool::new(false));
        self.cancels.lock().unwrap().insert(run.id.clone(), cancel.clone());

        let db = self.db.clone();
        let shared_log = shared.clone();
        let run_id = run.id.clone();
        let account_id = run.account_id.clone();
        let flow_id = run.flow_id.clone();
        let log: crate::engine::LogFn = Arc::new(move |msg: &str| {
            let mut g = shared_log.lock().unwrap();
            g.logs.push(msg.to_string());
            db.update_run(&g);
            drop(g);
            // Inline NOTIFY lines produce an in-app notification (flow_action).
            if let Some(idx) = msg.find("NOTIFY:") {
                let raw = &msg[idx + "NOTIFY:".len()..];
                let (title, mut body) = if let Some(sep) = raw.find("||") {
                    (raw[..sep].trim().to_string(), raw[sep + 2..].trim().to_string())
                } else {
                    ("Flow Notification".to_string(), raw.trim().to_string())
                };
                if body.is_empty() {
                    body = "notification action".into();
                }
                db.create_notification(Notification {
                    id: gen_id("ntf"),
                    rule_id: String::new(),
                    event: EVENT_FLOW_ACTION.to_string(),
                    title,
                    body,
                    run_id: run_id.clone(),
                    account_id: account_id.clone(),
                    flow_id: flow_id.clone(),
                    read_at: String::new(),
                    created_at: now_str(),
                });
            }
        });

        // 10-minute wall clock, matching the Go context timeout.
        let params_opt = if params.is_empty() { None } else { Some(params) };
        let fut = self.runner.run_flow_with_params(&resolved, &flow, params_opt.as_ref(), log, cancel);
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(600), fut).await;

        self.cancels.lock().unwrap().remove(&run.id);

        let mut final_run = shared.lock().unwrap().clone();
        match outcome {
            Ok(Ok(())) => final_run.status = RUN_DONE.to_string(),
            Ok(Err(_)) => final_run.status = RUN_FAILED.to_string(),
            Err(_) => {
                final_run.logs.push("run timed out after 10m".to_string());
                final_run.status = RUN_FAILED.to_string();
            }
        }
        final_run.ended_at = now_str();
        self.db.update_run(&final_run);
        self.emit_notifications_for_run(&final_run);
    }

    fn emit_notifications_for_run(&self, run: &FlowRun) {
        let event = match run.status.as_str() {
            RUN_FAILED => EVENT_RUN_FAILED,
            RUN_DONE => EVENT_RUN_DONE,
            _ => return,
        };
        for r in self.db.list_notification_rules() {
            if !r.enabled || r.event != event {
                continue;
            }
            if !r.flow_id.is_empty() && r.flow_id != run.flow_id {
                continue;
            }
            if !r.account_id.is_empty() && r.account_id != run.account_id {
                continue;
            }
            let title = if r.name.is_empty() { event.to_string() } else { r.name.clone() };
            let body = if r.message_template.trim().is_empty() {
                format!("Event={event} run={} account={} flow={}", run.id, run.account_id, run.flow_id)
            } else {
                r.message_template.clone()
            };
            self.db.create_notification(Notification {
                id: gen_id("ntf"),
                rule_id: r.id.clone(),
                event: event.to_string(),
                title,
                body,
                run_id: run.id.clone(),
                account_id: run.account_id.clone(),
                flow_id: run.flow_id.clone(),
                read_at: String::new(),
                created_at: now_str(),
            });
        }
    }

    // ---- scheduler ----

    pub fn spawn_scheduler(self: &Arc<Self>) {
        let this = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(std::time::Duration::from_secs(15));
            loop {
                ticker.tick().await;
                let now = now_str();
                for sc in this.db.list_due_schedules(&now) {
                    if let Err(e) = this.trigger_schedule(sc) {
                        tracing::warn!("schedule error: {e}");
                    }
                }
            }
        });
    }

    pub fn trigger_schedule(&self, mut sc: Schedule) -> Result<()> {
        let targets = self.schedule_accounts(&sc)?;
        let now = chrono::Utc::now();
        let (next_run, keep_enabled) = next_run_after(&sc, now)?;
        sc.last_run_at = now.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        sc.next_run_at = next_run;
        if !keep_enabled {
            sc.enabled = false;
        }
        self.db.upsert_schedule(sc.clone());
        for acc in targets {
            if let Err(e) = self.start_flow_run(acc.clone(), &sc.flow_id, &sc.id, sc.params.clone()) {
                tracing::warn!("schedule {} account {} start failed: {e}", sc.id, acc.id);
            }
        }
        Ok(())
    }

    fn schedule_accounts(&self, sc: &Schedule) -> Result<Vec<TikTokAccount>> {
        let all = self.db.list_accounts();
        if sc.all_accounts {
            return Ok(all);
        }
        if sc.account_ids.is_empty() {
            return Err(anyhow!("schedule has no account targets"));
        }
        let allowed: std::collections::HashSet<&String> = sc.account_ids.iter().collect();
        let out: Vec<TikTokAccount> = all.into_iter().filter(|a| allowed.contains(&a.id)).collect();
        if out.is_empty() {
            return Err(anyhow!("no valid account targets"));
        }
        Ok(out)
    }
}

const INLINE_BROWSER_PREVIEW_FLOW_ID: &str = "_inline_browser_preview";

fn browser_preview_flow() -> Flow {
    let mut start_cfg = StrMap::new();
    start_cfg.insert("_stage".into(), "1".into());
    start_cfg.insert("_next_on_success".into(), "2".into());
    let mut home_cfg = StrMap::new();
    home_cfg.insert("_stage".into(), "2".into());
    Flow {
        id: INLINE_BROWSER_PREVIEW_FLOW_ID.into(),
        name: "Xem trình duyệt (thử / AI)".into(),
        params: Some(StrMap::new()),
        actions: vec![
            FlowAction {
                id: "step_start".into(),
                type_: "start".into(),
                name: "Start".into(),
                config: start_cfg,
                timeout: 0,
                params: None,
                atomics: vec![],
            },
            FlowAction {
                id: "step_open_home".into(),
                type_: "open_home".into(),
                name: "Open Home".into(),
                config: home_cfg,
                timeout: 120,
                params: None,
                atomics: vec![],
            },
        ],
        updated_at: String::new(),
    }
}

// ---- schedule timing (ported from schedules.go) ----

pub fn validate_schedule_input(sc: &Schedule, db: &Db) -> Result<()> {
    if sc.name.trim().is_empty() {
        return Err(anyhow!("name is required"));
    }
    if sc.flow_id.trim().is_empty() {
        return Err(anyhow!("flowId is required"));
    }
    if db.get_flow(&sc.flow_id).is_err() {
        return Err(anyhow!("flow not found"));
    }
    match sc.type_.as_str() {
        SCHEDULE_RUN_NOW => {}
        SCHEDULE_DAILY_AT => {
            parse_daily_at(&sc.daily_at)?;
        }
        SCHEDULE_ONCE_AT => {
            if sc.once_at.trim().is_empty() {
                return Err(anyhow!("onceAt is required"));
            }
            chrono::DateTime::parse_from_rfc3339(&sc.once_at).map_err(|_| anyhow!("onceAt must be RFC3339"))?;
        }
        _ => return Err(anyhow!("invalid schedule type")),
    }
    if !sc.all_accounts && sc.account_ids.is_empty() {
        return Err(anyhow!("accountIds is required when allAccounts=false"));
    }
    Ok(())
}

/// Returns (next_run_at RFC3339 or "", keep_enabled).
pub fn next_run_after(sc: &Schedule, now: chrono::DateTime<chrono::Utc>) -> Result<(String, bool)> {
    let fmt = |t: chrono::DateTime<chrono::Utc>| t.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    match sc.type_.as_str() {
        SCHEDULE_RUN_NOW => Ok((fmt(now), false)),
        SCHEDULE_ONCE_AT => {
            let t = chrono::DateTime::parse_from_rfc3339(&sc.once_at).map_err(|_| anyhow!("onceAt must be RFC3339"))?;
            let t = t.with_timezone(&chrono::Utc);
            if t < now {
                return Err(anyhow!("onceAt must be in the future"));
            }
            Ok((fmt(t), false))
        }
        SCHEDULE_DAILY_AT => {
            let (h, m) = parse_daily_at(&sc.daily_at)?;
            let tz: chrono_tz::Tz = if sc.timezone_id.trim().is_empty() {
                chrono_tz::UTC
            } else {
                sc.timezone_id.parse().map_err(|_| anyhow!("invalid timezoneId"))?
            };
            let local_now = now.with_timezone(&tz);
            let mut next = tz
                .with_ymd_and_hms(local_now.year(), local_now.month(), local_now.day(), h, m, 0)
                .single()
                .ok_or_else(|| anyhow!("invalid local time"))?;
            if next <= local_now {
                next += chrono::Duration::days(1);
            }
            Ok((fmt(next.with_timezone(&chrono::Utc)), true))
        }
        _ => Err(anyhow!("invalid schedule type")),
    }
}

fn parse_daily_at(v: &str) -> Result<(u32, u32)> {
    let parts: Vec<&str> = v.trim().split(':').collect();
    if parts.len() != 2 {
        return Err(anyhow!("dailyAt must be HH:MM"));
    }
    let h: u32 = parts[0].parse().map_err(|_| anyhow!("dailyAt must be HH:MM"))?;
    let m: u32 = parts[1].parse().map_err(|_| anyhow!("dailyAt must be HH:MM"))?;
    if h > 23 || m > 59 {
        return Err(anyhow!("dailyAt must be HH:MM"));
    }
    Ok((h, m))
}
