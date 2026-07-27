//! Flow engine — graph walker + run lifecycle.
//! Ported from internal/engine/runner.go (+ playwrightexec dispatch entry).
//!
//! The walker is NOT a linear loop: it indexes actions by id and follows the
//! branch edges encoded in `action.config` (`_next_on_success` /
//! `_next_on_error` / `_next_alt`, and their `*_step_id` variants). Control-flow
//! actions (log, notification, loop_repeat, loop_if, run_next_flow, set_params,
//! record_*, account_meta) are handled inline; everything else is delegated to
//! the `BrowserDriver`.

pub mod browser;
pub mod driver;
pub mod ext_page;
pub mod page;
pub mod run_state;

use crate::db::Db;
use crate::domain::{FlowAction, TikTokAccount};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use rand::Rng;
use run_state::RunState;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Sink for run log lines. The server appends each line to the run record.
pub type LogFn = Arc<dyn Fn(&str) + Send + Sync>;

/// Cooperative cancellation — the walker checks it between steps.
pub type Cancel = Arc<AtomicBool>;

#[async_trait]
pub trait BrowserDriver: Send + Sync {
    async fn before_run(&self, account: &TikTokAccount, log: &LogFn) -> Result<()>;
    async fn execute(&self, rs: &RunState, account: &TikTokAccount, action: &FlowAction, log: &LogFn) -> Result<()>;
    async fn after_run(&self, account: &TikTokAccount);
}

/// No-browser driver. Simulates actions with a short sleep and reproduces the
/// few checks the Go `StubDriver` performed so non-Playwright runs behave.
pub struct StubDriver;

#[async_trait]
impl BrowserDriver for StubDriver {
    async fn before_run(&self, _account: &TikTokAccount, _log: &LogFn) -> Result<()> {
        Ok(())
    }

    async fn after_run(&self, _account: &TikTokAccount) {}

    async fn execute(&self, _rs: &RunState, account: &TikTokAccount, action: &FlowAction, _log: &LogFn) -> Result<()> {
        let ms = 250 + rand::thread_rng().gen_range(0..650);
        tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        match action.type_.as_str() {
            "comment_video" | "reply_comment" => {
                if action.config_get("text").unwrap_or("").is_empty() {
                    return Err(anyhow!("comment/reply text is empty for account {}", account.username));
                }
            }
            "if_condition" => {
                match action.config_get("expect").unwrap_or("").trim().to_lowercase().as_str() {
                    "always_false" => return Err(anyhow!("if_condition: expect=always_false (stub)")),
                    _ => return Ok(()),
                }
            }
            "random_yes_no" => {
                let pct = stub_parse_yes_percent(&action.config);
                if rand::thread_rng().gen_range(0..100) < pct {
                    return Ok(());
                }
                return Err(anyhow!("random_yes_no: no (stub)"));
            }
            "open_url" => {
                if action.config_get("url").unwrap_or("").trim().is_empty() {
                    return Err(anyhow!("open_url: url is empty (stub)"));
                }
            }
            "run_next_flow" => {
                if action.config_get("next_flow_id").unwrap_or("").trim().is_empty()
                    && action.config_get("flow_id").unwrap_or("").trim().is_empty()
                {
                    return Err(anyhow!("run_next_flow: thiếu next_flow_id hoặc flow_id (stub)"));
                }
            }
            "ai_playwright_agent" => {
                return Err(anyhow!(
                    "ai_playwright_agent: cần TIKTOK_USE_PLAYWRIGHT=1 và LLM đã cấu hình trong Settings"
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

fn stub_parse_yes_percent(cfg: &BTreeMap<String, String>) -> i32 {
    for k in ["yes_percent", "probability", "percent", "p"] {
        if let Some(v) = cfg.get(k) {
            let v = v.trim();
            if v.is_empty() {
                continue;
            }
            if let Ok(n) = v.parse::<i32>() {
                return n.clamp(0, 100);
            }
        }
    }
    50
}

pub struct Runner {
    driver: Arc<dyn BrowserDriver>,
    db: Db,
    max_nest: usize,
}

impl Runner {
    pub fn new(driver: Arc<dyn BrowserDriver>, db: Db) -> Self {
        Self { driver, db, max_nest: 16 }
    }

    pub async fn run_flow_with_params(
        &self,
        account: &TikTokAccount,
        flow: &crate::domain::Flow,
        run_params: Option<&crate::domain::StrMap>,
        log: LogFn,
        cancel: Cancel,
    ) -> Result<()> {
        let rs = RunState::new();
        rs.set_params(run_params);
        self.run_flow_once(account, flow, &rs, &log, &cancel, false, 0).await
    }

    fn run_flow_once<'a>(
        &'a self,
        account: &'a TikTokAccount,
        flow: &'a crate::domain::Flow,
        rs: &'a RunState,
        log: &'a LogFn,
        cancel: &'a Cancel,
        nested: bool,
        nest_depth: usize,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            if !nested {
                if let Err(e) = self.driver.before_run(account, log).await {
                    log(&format!("BeforeRun failed: {e}"));
                    return Err(e);
                }
                log(&format!("Start run account={} flow={}", account.username, flow.name));
            } else {
                log(&format!("Nested flow: {} ({})", flow.name, flow.id));
            }

            // AfterRun runs once, at the outermost level, regardless of outcome.
            let result = self.walk(account, flow, rs, log, cancel, nested, nest_depth).await;

            if !nested {
                self.driver.after_run(account).await;
                if result.is_ok() {
                    log("Run completed");
                }
            } else if result.is_ok() {
                // nested completion messages already emitted inside walk()
            }
            result
        })
    }

    async fn walk(
        &self,
        account: &TikTokAccount,
        flow: &crate::domain::Flow,
        rs: &RunState,
        log: &LogFn,
        cancel: &Cancel,
        nested: bool,
        nest_depth: usize,
    ) -> Result<()> {
        if flow.actions.is_empty() {
            log(if nested { "Nested flow empty — done" } else { "Run completed" });
            return Ok(());
        }

        let groups = group_actions_by_stage(&flow.actions);
        log_stage_plan(&groups, log);

        let actions_by_id: BTreeMap<String, FlowAction> =
            flow.actions.iter().map(|a| (a.id.clone(), a.clone())).collect();

        let mut stage_first_step: BTreeMap<i32, String> = BTreeMap::new();
        for (i, a) in flow.actions.iter().enumerate() {
            let st = get_action_stage(a, (i + 1) as i32);
            stage_first_step.entry(st).or_insert_with(|| a.id.clone());
        }

        let first_action = flow
            .actions
            .iter()
            .find(|a| a.type_ == "start")
            .cloned()
            .unwrap_or_else(|| flow.actions[0].clone());

        let total = flow.actions.len();
        let mut done = 0usize;
        let mut current = first_action.id.clone();
        let max_visits = (flow.actions.len() * 20).max(40);
        let mut visit_count: BTreeMap<String, usize> = BTreeMap::new();
        let mut loop_count: BTreeMap<String, i64> = BTreeMap::new();

        while !current.is_empty() {
            let raw_action = match actions_by_id.get(&current) {
                Some(a) => a.clone(),
                None => {
                    log(&format!("Step {current:?} không tồn tại, kết thúc run"));
                    break;
                }
            };
            let a = rs.resolve_action(raw_action);
            *visit_count.entry(current.clone()).or_insert(0) += 1;
            if visit_count[&current] > max_visits {
                log(&format!("Flow loop detected(step_id={current}, visits={})", visit_count[&current]));
                log("Run completed (stopped to avoid loop)");
                return Ok(());
            }
            if done >= max_visits {
                return Err(anyhow!("flow branching loop detected (step_id={current})"));
            }
            if cancel.load(Ordering::Relaxed) {
                return Err(anyhow!("run cancelled"));
            }

            done += 1;
            let stage = get_action_stage(&a, done as i32);
            log(&format!(
                "[Step][{done}/{total}][Stage {stage}] Execute: {} ({})",
                a.name, a.type_
            ));

            // ----- inline control-flow -----
            match a.type_.as_str() {
                "log" => {
                    let mut msg = a.config_get("message").unwrap_or("").to_string();
                    if msg.is_empty() {
                        msg = a.config_get("text").unwrap_or("").to_string();
                    }
                    if msg.is_empty() {
                        msg = "log action".into();
                    }
                    let msg = rs.render_for_log(&msg);
                    log(&format!("[Stage {stage}] LOG: {msg}"));
                    current = branch(&a, true, &actions_by_id, &stage_first_step, stage, log);
                    continue;
                }
                "notification" => {
                    let mut title = a.config_get("title").unwrap_or("").to_string();
                    if title.is_empty() {
                        title = "Flow Notification".into();
                    }
                    let mut body = a.config_get("message").unwrap_or("").to_string();
                    if body.is_empty() {
                        body = a.config_get("body").unwrap_or("").to_string();
                    }
                    if body.is_empty() {
                        body = "notification action".into();
                    }
                    let title = rs.render_for_log(&title);
                    let body = rs.render_for_log(&body);
                    log(&format!("[Stage {stage}] NOTIFY: {title} || {body}"));
                    current = branch(&a, true, &actions_by_id, &stage_first_step, stage, log);
                    continue;
                }
                "loop_repeat" => {
                    let repeat = a
                        .config_get("repeat_times")
                        .and_then(|v| v.trim().parse::<i64>().ok())
                        .filter(|n| *n > 0)
                        .unwrap_or(3);
                    let cnt = loop_count.entry(a.id.clone()).or_insert(0);
                    *cnt += 1;
                    let cur = *cnt;
                    if cur <= repeat {
                        let next = branch(&a, true, &actions_by_id, &stage_first_step, stage, log);
                        log(&format!("[Stage {stage}] LOOP {cur}/{repeat} -> step {next}"));
                        current = next;
                    } else {
                        let next = branch(&a, false, &actions_by_id, &stage_first_step, stage, log);
                        log(&format!("[Stage {stage}] LOOP done {}/{repeat} -> step {next}", cur - 1));
                        current = next;
                    }
                    continue;
                }
                "loop_if" => {
                    let cnt = loop_count.entry(a.id.clone()).or_insert(0);
                    *cnt += 1;
                    let cur = *cnt;
                    let (mut should_exit, mut reason) = evaluate_loop_if_exit(rs, &a.config);
                    let max_loops = parse_positive_int(a.config_get("max_loops").unwrap_or(""), 0);
                    if !should_exit && max_loops > 0 && cur >= max_loops as i64 {
                        should_exit = true;
                        if !reason.is_empty() {
                            reason.push_str("; ");
                        }
                        reason.push_str(&format!("reach max_loops={max_loops}"));
                    }
                    if should_exit {
                        let next = branch(&a, false, &actions_by_id, &stage_first_step, stage, log);
                        log(&format!("[Stage {stage}] LOOP_IF exit ({reason}) -> step {next}"));
                        current = next;
                    } else {
                        let next = branch(&a, true, &actions_by_id, &stage_first_step, stage, log);
                        log(&format!("[Stage {stage}] LOOP_IF continue ({reason}) -> step {next}"));
                        current = next;
                    }
                    continue;
                }
                "run_next_flow" => {
                    let cfg = rs.resolve_config(&a.config);
                    let mut next_flow_id = cfg.get("next_flow_id").map(|s| s.trim().to_string()).unwrap_or_default();
                    if next_flow_id.is_empty() {
                        next_flow_id = cfg.get("flow_id").map(|s| s.trim().to_string()).unwrap_or_default();
                    }
                    if next_flow_id.is_empty() {
                        return Err(anyhow!("run_next_flow: thiếu next_flow_id hoặc flow_id"));
                    }
                    if next_flow_id == flow.id {
                        return Err(anyhow!("run_next_flow: không thể gọi chính flow hiện tại ({})", flow.id));
                    }
                    if nest_depth >= self.max_nest {
                        return Err(anyhow!("run_next_flow: vượt quá độ sâu lồng tối đa ({})", self.max_nest));
                    }
                    let child = match self.db.get_flow(&next_flow_id) {
                        Ok(f) => f,
                        Err(e) => {
                            log(&format!("[Stage {stage}] run_next_flow: không tải flow {next_flow_id:?}: {e}"));
                            let next = branch(&a, false, &actions_by_id, &stage_first_step, stage, log);
                            if next.is_empty() {
                                return Err(anyhow!("run_next_flow: {e}"));
                            }
                            current = next;
                            continue;
                        }
                    };
                    let nested_log: LogFn = {
                        let l = log.clone();
                        Arc::new(move |m: &str| l(&format!("[next-flow] {m}")))
                    };
                    let r = self
                        .run_flow_once(account, &child, rs, &nested_log, cancel, true, nest_depth + 1)
                        .await;
                    match r {
                        Ok(()) => {
                            let next = branch(&a, true, &actions_by_id, &stage_first_step, stage, log);
                            if !next.is_empty() {
                                log(&format!("[Stage {stage}] run_next_flow ok -> step {next}"));
                            }
                            current = next;
                        }
                        Err(e) => {
                            log(&format!("[Stage {stage}] run_next_flow failed: {e}"));
                            let next = branch(&a, false, &actions_by_id, &stage_first_step, stage, log);
                            if next.is_empty() {
                                return Err(e);
                            }
                            current = next;
                        }
                    }
                    continue;
                }
                "set_params" => {
                    let patch = parse_set_params_config(&a.config);
                    if patch.is_empty() {
                        log(&format!("[Stage {stage}] set_params: không có cập nhật (dùng config updates: key=value mỗi dòng)"));
                    } else {
                        rs.merge_params(&patch);
                        let keys: Vec<&str> = patch.keys().map(|s| s.as_str()).collect();
                        log(&format!("[Stage {stage}] set_params updated: {}", keys.join(", ")));
                    }
                    current = branch(&a, true, &actions_by_id, &stage_first_step, stage, log);
                    continue;
                }
                "record_post_interaction" => {
                    let cfg = &a.config;
                    let mut post_key = cfg.get("post_key").map(|s| s.trim().to_string()).unwrap_or_default();
                    if post_key.is_empty() {
                        post_key = cfg.get("video_id").map(|s| s.trim().to_string()).unwrap_or_default();
                    }
                    if post_key.is_empty() {
                        return Err(anyhow!("record_post_interaction: thiếu post_key (hoặc video_id)"));
                    }
                    let mut it = cfg.get("interaction").map(|s| s.trim().to_string()).unwrap_or_default();
                    if it.is_empty() {
                        it = cfg.get("interaction_type").map(|s| s.trim().to_string()).unwrap_or_default();
                    }
                    if it.is_empty() {
                        it = "interaction".into();
                    }
                    let r = self.db.record_post_interaction(
                        &account.id, &post_key, &it,
                        cfg.get("post_url").map(|s| s.as_str()).unwrap_or(""),
                        cfg.get("author_username").map(|s| s.as_str()).unwrap_or(""),
                        cfg.get("extra_json").map(|s| s.as_str()).unwrap_or(""),
                    );
                    if let Err(e) = r {
                        log(&format!("[Stage {stage}] record_post_interaction lỗi: {e}"));
                        let next = branch(&a, false, &actions_by_id, &stage_first_step, stage, log);
                        if next.is_empty() {
                            return Err(e);
                        }
                        current = next;
                        continue;
                    }
                    log(&format!("[Stage {stage}] record_post_interaction: account={} post_key={post_key} type={it}", account.id));
                    current = branch(&a, true, &actions_by_id, &stage_first_step, stage, log);
                    continue;
                }
                "record_friend_event" => {
                    let cfg = &a.config;
                    let ev = normalize_friend_event_type(cfg.get("event").map(|s| s.as_str()).unwrap_or(""));
                    if ev.is_empty() {
                        return Err(anyhow!("record_friend_event: thiếu event (follow|unfollow|friend_add|friend_remove|add|remove)"));
                    }
                    let mut tu = cfg.get("target_username").map(|s| s.trim().to_string()).unwrap_or_default();
                    if tu.is_empty() {
                        tu = cfg.get("peer_username").map(|s| s.trim().to_string()).unwrap_or_default();
                    }
                    let mut tid = cfg.get("target_user_id").map(|s| s.trim().to_string()).unwrap_or_default();
                    if tid.is_empty() {
                        tid = cfg.get("peer_user_id").map(|s| s.trim().to_string()).unwrap_or_default();
                    }
                    let r = self.db.record_friend_event(&account.id, &tu, &tid, &ev, cfg.get("notes").map(|s| s.as_str()).unwrap_or(""));
                    if let Err(e) = r {
                        log(&format!("[Stage {stage}] record_friend_event lỗi: {e}"));
                        let next = branch(&a, false, &actions_by_id, &stage_first_step, stage, log);
                        if next.is_empty() {
                            return Err(e);
                        }
                        current = next;
                        continue;
                    }
                    log(&format!("[Stage {stage}] record_friend_event: account={} event={ev} target={tu}", account.id));
                    current = branch(&a, true, &actions_by_id, &stage_first_step, stage, log);
                    continue;
                }
                "account_meta" => {
                    let cfg = &a.config;
                    let mut op = cfg.get("operation").map(|s| s.trim().to_lowercase()).unwrap_or_default();
                    if op.is_empty() {
                        op = cfg.get("op").map(|s| s.trim().to_lowercase()).unwrap_or_default();
                    }
                    let mut mk = cfg.get("meta_key").map(|s| s.trim().to_string()).unwrap_or_default();
                    if mk.is_empty() {
                        mk = cfg.get("key").map(|s| s.trim().to_string()).unwrap_or_default();
                    }
                    if mk.is_empty() {
                        return Err(anyhow!("account_meta: thiếu meta_key (hoặc key)"));
                    }
                    let r = match op.as_str() {
                        "delete" | "del" | "remove" | "xoa" => {
                            let r = self.db.delete_account_kv_meta(&account.id, &mk);
                            if r.is_ok() {
                                log(&format!("[Stage {stage}] account_meta: xóa key {mk:?} cho account {}", account.id));
                            }
                            r
                        }
                        _ => {
                            let mut mv = cfg.get("meta_value").map(|s| s.as_str()).unwrap_or("").to_string();
                            if mv.is_empty() {
                                mv = cfg.get("value").map(|s| s.as_str()).unwrap_or("").to_string();
                            }
                            let mv = rs.render_for_log(&mv);
                            let r = self.db.upsert_account_kv_meta(&account.id, &mk, &mv);
                            if r.is_ok() {
                                log(&format!("[Stage {stage}] account_meta: upsert key {mk:?} cho account {}", account.id));
                            }
                            r
                        }
                    };
                    if let Err(e) = r {
                        log(&format!("[Stage {stage}] account_meta lỗi: {e}"));
                        let next = branch(&a, false, &actions_by_id, &stage_first_step, stage, log);
                        if next.is_empty() {
                            return Err(e);
                        }
                        current = next;
                        continue;
                    }
                    current = branch(&a, true, &actions_by_id, &stage_first_step, stage, log);
                    continue;
                }
                _ => {}
            }

            // ----- browser-delegated action -----
            match self.driver.execute(rs, account, &a, log).await {
                Err(e) => {
                    log(&format!("[Stage {stage}] Action failed: {e}"));
                    let next = branch(&a, false, &actions_by_id, &stage_first_step, stage, log);
                    if next.is_empty() {
                        return Err(e);
                    }
                    current = next;
                }
                Ok(()) => {
                    current = branch(&a, true, &actions_by_id, &stage_first_step, stage, log);
                }
            }
        }

        if nested {
            log("Nested flow done");
        }
        Ok(())
    }
}

/// Resolve the next step id after this action, given whether it succeeded.
/// Also logs the branch line (matching the Go log format).
fn branch(
    action: &FlowAction,
    success: bool,
    actions_by_id: &BTreeMap<String, FlowAction>,
    stage_first_step: &BTreeMap<i32, String>,
    stage: i32,
    log: &LogFn,
) -> String {
    let next = pick_next_action_id(action, success, actions_by_id, stage_first_step);
    if !next.is_empty() {
        let kind = if success { "success" } else { "error" };
        log(&format!("[Stage {stage}] branch({kind}) to step {next}"));
    }
    next
}

fn pick_next_action_id(
    action: &FlowAction,
    success: bool,
    actions_by_id: &BTreeMap<String, FlowAction>,
    stage_first_step: &BTreeMap<i32, String>,
) -> String {
    let cfg = &action.config;
    let mut step_keys: Vec<&str> = vec!["_next_alt_step_id"];
    let mut stage_keys: Vec<&str> = vec!["_next_alt"];
    if success {
        step_keys.insert(0, "_next_on_success_step_id");
        stage_keys.insert(0, "_next_on_success");
    } else {
        step_keys.insert(0, "_next_on_error_step_id");
        stage_keys.insert(0, "_next_on_error");
    }
    for k in step_keys {
        if let Some(id) = cfg.get(k) {
            let id = id.trim();
            if !id.is_empty() && actions_by_id.contains_key(id) {
                return id.to_string();
            }
        }
    }
    for k in stage_keys {
        if let Some(raw) = cfg.get(k) {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            if let Ok(n) = raw.parse::<i32>() {
                if n > 0 {
                    if let Some(id) = stage_first_step.get(&n) {
                        if !id.is_empty() {
                            return id.clone();
                        }
                    }
                }
            }
        }
    }
    String::new()
}

fn get_action_stage(a: &FlowAction, fallback: i32) -> i32 {
    match a.config.get("_stage") {
        Some(raw) if !raw.is_empty() => raw.parse::<i32>().ok().filter(|n| *n > 0).unwrap_or(fallback),
        _ => fallback,
    }
}

struct StageGroup {
    stage: i32,
    actions: Vec<FlowAction>,
}

fn group_actions_by_stage(actions: &[FlowAction]) -> Vec<StageGroup> {
    let max_per_stage = 5usize;
    let mut stage_map: BTreeMap<i32, Vec<FlowAction>> = BTreeMap::new();
    for (i, a) in actions.iter().enumerate() {
        let mut stage = (i + 1) as i32;
        if let Some(v) = a.config.get("_stage") {
            if let Ok(n) = v.parse::<i32>() {
                if n > 0 {
                    stage = n;
                }
            }
        }
        stage_map.entry(stage).or_default().push(a.clone());
    }
    let mut out: Vec<StageGroup> = Vec::new();
    for (k, acts) in stage_map {
        if acts.len() <= max_per_stage {
            out.push(StageGroup { stage: k, actions: acts });
        } else {
            for (idx, ch) in acts.chunks(max_per_stage).enumerate() {
                out.push(StageGroup { stage: k + idx as i32, actions: ch.to_vec() });
            }
        }
    }
    out.sort_by_key(|g| g.stage);
    out
}

fn log_stage_plan(groups: &[StageGroup], log: &LogFn) {
    if groups.is_empty() {
        return;
    }
    for g in groups {
        let items: Vec<String> = g.actions.iter().map(|a| format!("{}({})", a.name, a.type_)).collect();
        log(&format!("[FLOW] Stage {} plan: {}", g.stage, items.join(", ")));
    }
    let first = &groups[0];
    let mut has_bootstrap = false;
    let mut has_risky = false;
    for a in &first.actions {
        match a.type_.as_str() {
            "open_home" | "open_url" | "search" | "login" => has_bootstrap = true,
            "watch_video" | "like_video" | "comment_video" | "reply_comment" | "share_video" | "follow_user"
            | "check_login" => has_risky = true,
            _ => {}
        }
    }
    if has_risky && !has_bootstrap {
        log("[FLOW][WARN] Stage đầu có action tương tác nhưng không có open_home/open_url/search/login; run có thể bắt đầu ở about:blank và fail selector.");
    }
}

fn parse_positive_int(raw: &str, def: i32) -> i32 {
    let v = raw.trim();
    if v.is_empty() {
        return def;
    }
    v.parse::<i32>().ok().filter(|n| *n > 0).unwrap_or(def)
}

fn evaluate_loop_if_exit(rs: &RunState, cfg: &BTreeMap<String, String>) -> (bool, String) {
    let mut param_key = cfg.get("param_key").map(|s| s.trim().to_string()).unwrap_or_default();
    if param_key.is_empty() {
        param_key = cfg.get("key").map(|s| s.trim().to_string()).unwrap_or_default();
    }
    let cur_val = if param_key.is_empty() { String::new() } else { rs.get_param(&param_key).unwrap_or_default() };
    let mut op = cfg.get("operator").map(|s| s.trim().to_lowercase()).unwrap_or_default();
    if op.is_empty() {
        op = "equals".into();
    }
    let expect = cfg.get("value").cloned().unwrap_or_default();
    let note = format!("param[{param_key}]={cur_val:?} op={op} value={expect:?}");
    match op.as_str() {
        "equals" | "eq" => (cur_val == expect, note),
        "not_equals" | "ne" => (cur_val != expect, note),
        "contains" => (cur_val.contains(&expect), note),
        "truthy" => {
            let l = cur_val.trim().to_lowercase();
            (matches!(l.as_str(), "1" | "true" | "yes" | "ok"), note)
        }
        "falsy" => {
            let l = cur_val.trim().to_lowercase();
            (matches!(l.as_str(), "" | "0" | "false" | "no"), note)
        }
        "empty" => (cur_val.trim().is_empty(), note),
        "not_empty" => (!cur_val.trim().is_empty(), note),
        "regex" => match regex::Regex::new(&expect) {
            Ok(re) => (re.is_match(&cur_val), note),
            Err(_) => (false, note + " (invalid regex)"),
        },
        "gt" | "gte" | "lt" | "lte" => {
            match (cur_val.trim().parse::<f64>(), expect.trim().parse::<f64>()) {
                (Ok(l), Ok(r)) => {
                    let res = match op.as_str() {
                        "gt" => l > r,
                        "gte" => l >= r,
                        "lt" => l < r,
                        _ => l <= r,
                    };
                    (res, note)
                }
                _ => (false, note + " (number parse fail)"),
            }
        }
        _ => (cur_val == expect, note + " (fallback equals)"),
    }
}

fn parse_set_params_config(cfg: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(k) = cfg.get("key") {
        let k = k.trim();
        if !k.is_empty() {
            out.insert(k.to_string(), cfg.get("value").cloned().unwrap_or_default());
        }
    }
    if let Some(updates) = cfg.get("updates") {
        for line in updates.split('\n') {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(sep) = line.find('=') {
                if sep == 0 {
                    continue;
                }
                let k = line[..sep].trim();
                if k.is_empty() {
                    continue;
                }
                out.insert(k.to_string(), line[sep + 1..].trim().to_string());
            }
        }
    }
    out
}

fn normalize_friend_event_type(raw: &str) -> String {
    match raw.trim().to_lowercase().as_str() {
        "follow" | "friend_add" | "add" | "themban" => "follow".into(),
        "unfollow" | "friend_remove" | "remove" | "xoaban" | "xoa_ban" => "unfollow".into(),
        other => other.to_string(),
    }
}
