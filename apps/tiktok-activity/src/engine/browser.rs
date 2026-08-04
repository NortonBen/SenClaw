//! CDP-side flow-action executors — port of internal/engine/playwrightexec/*
//! (navigate / engage / social / auth / if / check_scroll_end / random_yes_no /
//! playwright_atomics + legacy like/follow/next_video) and the atomic kinds
//! (click / click_button_text / fill / press / wait_ms / wait_load / goto /
//! scroll / assert / click_unless_contains), plus the AI executors routed
//! through the SenClaw bridge.

use super::page::PageOps;
use super::run_state::{step_default_output, RunState};
use super::LogFn;
use crate::bridge::Bridge;
use crate::domain::{FlowAction, FlowAtomic, StrMap, TikTokAccount};
use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use rand::Rng;
use serde::Deserialize;
use serde_json::Value;
use std::sync::RwLock;
use std::time::Duration;

// ============================ legacy atomic rules ============================

#[derive(Debug, Clone, Deserialize)]
struct LegacyDoc {
    #[serde(default)]
    version: i64,
    #[serde(default)]
    rules: std::collections::HashMap<String, LegacyRule>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyRule {
    #[serde(default)]
    atomics: Vec<FlowAtomic>,
    #[serde(default)]
    default_method: String,
    #[serde(default)]
    configurable_wait_ms_key: String,
    #[serde(default)]
    methods: std::collections::HashMap<String, LegacyMethod>,
}

#[derive(Debug, Clone, Deserialize)]
struct LegacyMethod {
    #[serde(default)]
    atomics: Vec<FlowAtomic>,
}

static LEGACY: Lazy<RwLock<Option<LegacyDoc>>> = Lazy::new(|| RwLock::new(None));

/// Parse + install legacy rules JSON. Empty clears them (like/follow/next_video
/// then error until re-imported), matching ApplyLegacyAtomicRulesJSON.
pub fn apply_legacy_rules(raw: &str) -> Result<()> {
    let raw = raw.trim();
    if raw.is_empty() {
        *LEGACY.write().unwrap() = None;
        return Ok(());
    }
    let doc: LegacyDoc =
        serde_json::from_str(raw).map_err(|e| anyhow!("legacy_atomic_rules JSON: {e}"))?;
    if doc.version < 1 {
        return Err(anyhow!("legacy_atomic_rules: cần version >= 1"));
    }
    if doc.rules.is_empty() {
        return Err(anyhow!("legacy_atomic_rules: rules không được rỗng"));
    }
    *LEGACY.write().unwrap() = Some(doc);
    Ok(())
}

pub fn legacy_loaded() -> bool {
    LEGACY
        .read()
        .unwrap()
        .as_ref()
        .map(|d| !d.rules.is_empty())
        .unwrap_or(false)
}

fn steps_for_legacy(type_: &str, action: &FlowAction) -> Result<Vec<FlowAtomic>> {
    let guard = LEGACY.read().unwrap();
    let doc = guard.as_ref().ok_or_else(|| {
        anyhow!("legacy atomic rules chưa import — PUT /api/engine/legacy-atomic-rules")
    })?;
    let entry = doc
        .rules
        .get(type_)
        .ok_or_else(|| anyhow!("legacy_atomic_rules: không có rule {type_:?}"))?;
    if !entry.methods.is_empty() {
        let mut method = action
            .config
            .get("method")
            .map(|s| s.trim().to_lowercase())
            .unwrap_or_default();
        if method.is_empty() {
            method = entry.default_method.trim().to_lowercase();
        }
        if method.is_empty() {
            method = "wheel".into();
        }
        let me = entry
            .methods
            .get(&method)
            .or_else(|| entry.methods.get("wheel"))
            .ok_or_else(|| anyhow!("legacy_atomic_rules: method {method:?} không hỗ trợ và không có fallback wheel"))?;
        let mut steps = me.atomics.clone();
        if steps.is_empty() {
            return Err(anyhow!(
                "legacy_atomic_rules: method {method:?} không có atomics"
            ));
        }
        let cfg_key = entry.configurable_wait_ms_key.trim();
        if !cfg_key.is_empty() {
            if let Some(v) = action
                .config
                .get(cfg_key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
            {
                for st in steps.iter_mut().rev() {
                    if st.kind.trim().eq_ignore_ascii_case("wait_ms") {
                        st.params
                            .get_or_insert_with(StrMap::new)
                            .insert("ms".into(), v.to_string());
                        break;
                    }
                }
            }
        }
        return Ok(steps);
    }
    if entry.atomics.is_empty() {
        return Err(anyhow!("legacy_atomic_rules: rule {type_:?} rỗng"));
    }
    Ok(entry.atomics.clone())
}

// ============================ atomic kinds ============================

fn p_int(p: &StrMap, key: &str, def: i64) -> i64 {
    p.get(key)
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(def)
}

fn split_selector_list(p: &StrMap) -> Vec<String> {
    let mut out = vec![];
    if let Some(s) = p.get("selector") {
        let s = s.trim();
        if !s.is_empty() {
            out.push(s.to_string());
        }
    }
    if let Some(m) = p.get("selectors") {
        for line in m.split('\n') {
            for part in line.split("||") {
                let part = part.trim();
                if !part.is_empty() {
                    out.push(part.to_string());
                }
            }
        }
    }
    out
}

fn resolve_fill_text(account: &TikTokAccount, action: &FlowAction, p: &StrMap) -> Result<String> {
    let vs = p
        .get("value_source")
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    match vs.as_str() {
        "literal" => {
            let t = p.get("text").map(|s| s.trim()).unwrap_or("");
            if t.is_empty() {
                return Err(anyhow!("literal: thiếu text"));
            }
            return Ok(t.to_string());
        }
        "account_username" | "username" => {
            if account.username.trim().is_empty() {
                return Err(anyhow!("account không có username"));
            }
            return Ok(account.username.clone());
        }
        "account_password" | "password" => {
            if account.password.trim().is_empty() {
                return Err(anyhow!("account không có password"));
            }
            return Ok(account.password.clone());
        }
        "action_param" | "step_param" => {
            return action_param(action, p.get("param_key").map(|s| s.as_str()).unwrap_or(""))
        }
        "" | "auto" => {}
        other => return Err(anyhow!("value_source không hợp lệ: {other:?}")),
    }
    if let Some(t) = p.get("text").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        return Ok(t.to_string());
    }
    let tf = p
        .get("text_from")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    match tf.to_lowercase().as_str() {
        "account_username" | "username" => Ok(account.username.clone()),
        "account_password" | "password" => Ok(account.password.clone()),
        "action_param" | "step_param" => {
            action_param(action, p.get("param_key").map(|s| s.as_str()).unwrap_or(""))
        }
        _ => {
            if let Some(k) = tf
                .strip_prefix("param:")
                .or_else(|| tf.strip_prefix("PARAM:"))
            {
                action_param(action, k.trim())
            } else {
                Err(anyhow!("cần value_source/text/text_from"))
            }
        }
    }
}

fn action_param(action: &FlowAction, key: &str) -> Result<String> {
    let key = key.trim();
    if key.is_empty() {
        return Err(anyhow!("thiếu param_key"));
    }
    action
        .params
        .as_ref()
        .and_then(|m| m.get(key))
        .cloned()
        .ok_or_else(|| anyhow!("step không có params[{key:?}]"))
}

fn resolve_goto_url(action: &FlowAction, p: &StrMap) -> Result<String> {
    if let Some(u) = p.get("url").map(|s| s.trim()).filter(|s| !s.is_empty()) {
        return Ok(u.to_string());
    }
    let us = p
        .get("url_source")
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_default();
    if us == "action_param" || us == "step_param" {
        return action_param(
            action,
            p.get("url_param_key").map(|s| s.as_str()).unwrap_or(""),
        );
    }
    let uf = p
        .get("url_from")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if let Some(k) = uf
        .strip_prefix("param:")
        .or_else(|| uf.strip_prefix("PARAM:"))
    {
        return action_param(action, k.trim());
    }
    Err(anyhow!(
        "goto: cần url hoặc url_source=action_param + url_param_key"
    ))
}

async fn run_atomic(
    page: &dyn PageOps,
    account: &TikTokAccount,
    action: &FlowAction,
    kind: &str,
    p: &StrMap,
) -> Result<()> {
    match kind {
        "click" => {
            let sels = split_selector_list(p);
            if sels.is_empty() {
                return Err(anyhow!("click: cần selector hoặc selectors"));
            }
            page.click_selectors(&sels, p_int(p, "timeout_ms", 20000) as u64)
                .await
        }
        "click_unless_contains" => {
            let sels = split_selector_list(p);
            if sels.is_empty() {
                return Err(anyhow!("click_unless_contains: cần selector"));
            }
            let raw = p
                .get("unless_substrings")
                .or_else(|| p.get("skip_if_contains"))
                .map(|s| s.trim())
                .unwrap_or("");
            if raw.is_empty() {
                return Err(anyhow!("click_unless_contains: need unless_substrings"));
            }
            let needles: Vec<String> = raw
                .split('\n')
                .map(|l| l.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            let txt = page
                .inner_text(&sels[0])
                .await
                .unwrap_or_default()
                .to_lowercase();
            if needles.iter().any(|n| !n.is_empty() && txt.contains(n)) {
                return Ok(());
            }
            page.click_selectors(&sels, p_int(p, "timeout_ms", 20000) as u64)
                .await
        }
        "click_button_text" => {
            let text = p
                .get("text")
                .or_else(|| p.get("name"))
                .or_else(|| p.get("button_text"))
                .map(|s| s.trim())
                .unwrap_or("");
            if text.is_empty() {
                return Err(anyhow!("click_button_text: cần text/name/button_text"));
            }
            let base = p
                .get("base_selector")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    r#"button, [role="button"], input[type="button"], input[type="submit"], a"#
                        .to_string()
                });
            page.click_by_text(&base, text, p_int(p, "timeout_ms", 20000) as u64)
                .await
        }
        "fill" => {
            let sel = p
                .get("selector")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .or_else(|| split_selector_list(p).into_iter().next())
                .ok_or_else(|| anyhow!("fill: cần selector"))?;
            let text = resolve_fill_text(account, action, p)?;
            page.fill(&sel, &text, p_int(p, "timeout_ms", 20000) as u64)
                .await
        }
        "press" => {
            let key = p.get("key").map(|s| s.trim()).unwrap_or("");
            if key.is_empty() {
                return Err(anyhow!("press: cần key"));
            }
            let sels = split_selector_list(p);
            if !sels.is_empty() {
                page.click_selectors(&sels, p_int(p, "timeout_ms", 15000) as u64)
                    .await
                    .ok();
            }
            page.press_key(key).await
        }
        "wait_ms" => {
            let ms = p_int(p, "ms", 0);
            if ms <= 0 {
                return Err(anyhow!("wait_ms: cần ms > 0"));
            }
            page.wait_ms(ms as u64).await;
            Ok(())
        }
        "wait_load" => {
            // No load-state API; a short settle wait approximates it.
            page.wait_ms(800).await;
            Ok(())
        }
        "goto" => {
            let url = resolve_goto_url(action, p)?;
            page.goto(
                &url,
                p.get("wait_until").map(|s| s.as_str()).unwrap_or(""),
                p_int(p, "timeout_ms", 45000) as u64,
            )
            .await
        }
        "scroll" => {
            let dx = p_int(p, "delta_x", 0) as f64;
            let dy = p_int(p, "delta_y", 0) as f64;
            if dx == 0.0 && dy == 0.0 {
                return Err(anyhow!("scroll: cần delta_x hoặc delta_y"));
            }
            let sels = split_selector_list(p);
            if !sels.is_empty() {
                // scroll centered on the element via JS scrollBy for robustness
                let sel_j = serde_json::to_string(&sels[0]).unwrap();
                let js = format!(
                    r#"(() => {{ const el = document.querySelector({sel_j}); if(!el) return false; el.scrollBy({{left:{dx}, top:{dy}, behavior:'instant'}}); return true; }})()"#
                );
                if page.eval_bool(&js).await {
                    return Ok(());
                }
            }
            let js = format!(
                r#"(() => {{ window.scrollBy({{left:{dx}, top:{dy}, behavior:'instant'}}); return true; }})()"#
            );
            page.eval_bool(&js).await;
            Ok(())
        }
        "assert" => eval_assert(page, p).await,
        other => Err(anyhow!("kind không hỗ trợ {other:?}")),
    }
}

async fn eval_assert(page: &dyn PageOps, p: &StrMap) -> Result<()> {
    let ex = p
        .get("expect")
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "visible".into());
    let timeout = p_int(p, "timeout_ms", 10000) as u64;
    match ex.as_str() {
        "visible" => {
            let sels = split_selector_list(p);
            if sels.is_empty() {
                return Err(anyhow!("assert visible: cần selector"));
            }
            let deadline = std::time::Instant::now() + Duration::from_millis(timeout);
            loop {
                if page.is_visible_any(&sels).await {
                    return Ok(());
                }
                if std::time::Instant::now() >= deadline {
                    return Err(anyhow!("assert visible: hết thời gian"));
                }
                page.wait_ms(200).await;
            }
        }
        "hidden" => {
            let sels = split_selector_list(p);
            let deadline = std::time::Instant::now() + Duration::from_millis(timeout);
            loop {
                if !page.is_visible_any(&sels).await {
                    return Ok(());
                }
                if std::time::Instant::now() >= deadline {
                    return Err(anyhow!("assert hidden: vẫn còn hiển thị"));
                }
                page.wait_ms(200).await;
            }
        }
        "url_contains" => {
            let sub = p.get("value").map(|s| s.trim()).unwrap_or("");
            if sub.is_empty() {
                return Err(anyhow!("assert url_contains: cần value"));
            }
            poll_url(page, timeout, |u| u.contains(sub)).await
        }
        "url_regex" => {
            let pat = p
                .get("pattern")
                .or_else(|| p.get("value"))
                .map(|s| s.trim())
                .unwrap_or("");
            let re = regex::Regex::new(pat).map_err(|e| anyhow!("assert url_regex: {e}"))?;
            poll_url(page, timeout, |u| re.is_match(u)).await
        }
        "text_contains" => {
            let needle = p
                .get("value")
                .or_else(|| p.get("text"))
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if needle.is_empty() {
                return Err(anyhow!("assert text_contains: cần value/text"));
            }
            let sel = p
                .get("selector")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "body".into());
            let deadline = std::time::Instant::now() + Duration::from_millis(timeout);
            loop {
                if page
                    .inner_text(&sel)
                    .await
                    .unwrap_or_default()
                    .contains(&needle)
                {
                    return Ok(());
                }
                if std::time::Instant::now() >= deadline {
                    return Err(anyhow!("assert text_contains: hết thời gian"));
                }
                page.wait_ms(200).await;
            }
        }
        other => Err(anyhow!("assert: expect không hỗ trợ {other:?}")),
    }
}

async fn poll_url(page: &dyn PageOps, timeout_ms: u64, pred: impl Fn(&str) -> bool) -> Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        if pred(&page.url().await) {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(anyhow!("assert url: hết thời gian"));
        }
        page.wait_ms(200).await;
    }
}

async fn run_atomics(
    page: &dyn PageOps,
    account: &TikTokAccount,
    action: &FlowAction,
    steps: &[FlowAtomic],
) -> Result<()> {
    for (i, step) in steps.iter().enumerate() {
        let kind = step.kind.trim();
        if kind.is_empty() {
            return Err(anyhow!("atomic[{i}]: thiếu kind"));
        }
        let empty = StrMap::new();
        let p = step.params.as_ref().unwrap_or(&empty);
        run_atomic(page, account, action, kind, p)
            .await
            .map_err(|e| anyhow!("atomic[{i}] {kind}: {e}"))?;
    }
    Ok(())
}

// ============================ TikTok executors + dispatch ============================

const COMMENT_BOX_SELECTORS: &[&str] = &[
    r#"[data-e2e="comment-input"] div[contenteditable="true"]"#,
    r#"div[contenteditable="true"][data-placeholder*="comment" i]"#,
    r#"[contenteditable="true"][role="textbox"]"#,
    r#"div[contenteditable="true"]"#,
];

fn sv(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

async fn is_logged_in(page: &dyn PageOps) -> bool {
    let candidates = sv(&[
        r#"a[href*="/@"] img"#,
        r#"[data-e2e="nav-profile"]"#,
        r#"[data-e2e="profile-icon"]"#,
        r#"a[href*="/upload"]"#,
    ]);
    page.is_visible_any(&candidates).await
}

/// Dispatch one browser action. Returns Ok on success; the graph walker handles
/// branching on Err. `rs` carries params/extras; `bridge` powers AI actions.
pub async fn execute_action(
    page: &dyn PageOps,
    rs: &RunState,
    account: &TikTokAccount,
    action: &FlowAction,
    bridge: &Bridge,
    log: &LogFn,
) -> Result<()> {
    rs.reset_step_extras();
    let result = dispatch(page, rs, account, action, bridge, log).await;
    // Persist step output (with any extras) so later {{prev.*}} resolves — even
    // on error (some steps set extras before failing), mirroring the Go Dispatcher.
    let mut out = step_default_output(&action.type_, &action.name, &page.url().await);
    for (k, v) in rs.take_step_extras() {
        out.insert(k, v);
    }
    rs.save_step_output(&action.id, out);
    result
}

async fn dispatch(
    page: &dyn PageOps,
    rs: &RunState,
    account: &TikTokAccount,
    action: &FlowAction,
    bridge: &Bridge,
    log: &LogFn,
) -> Result<()> {
    let cfg = &action.config;
    match action.type_.as_str() {
        "open_home" => page
            .goto("https://www.tiktok.com/", "domcontentloaded", 60000)
            .await
            .map_err(|e| anyhow!("open_home: {e}")),
        "open_url" => {
            let raw = cfg.get("url").map(|s| s.trim()).unwrap_or("");
            if raw.is_empty() {
                return Err(anyhow!("open_url: thiếu config url"));
            }
            let u =
                url::Url::parse(raw).map_err(|_| anyhow!("open_url: url không hợp lệ {raw:?}"))?;
            if u.scheme() != "http" && u.scheme() != "https" {
                return Err(anyhow!("open_url: chỉ hỗ trợ http/https"));
            }
            let to = cfg
                .get("timeout_ms")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(60000);
            page.goto(
                raw,
                cfg.get("wait_until").map(|s| s.as_str()).unwrap_or(""),
                to,
            )
            .await
            .map_err(|e| anyhow!("open_url: {e}"))
        }
        "search" => {
            let q = cfg
                .get("query")
                .or_else(|| cfg.get("keyword"))
                .map(|s| s.trim())
                .unwrap_or("");
            if q.is_empty() {
                return Err(anyhow!("search: thiếu config query hoặc keyword"));
            }
            let u = format!("https://www.tiktok.com/search?q={}", urlencoding(q));
            page.goto(&u, "domcontentloaded", 60000)
                .await
                .map_err(|e| anyhow!("search: {e}"))
        }
        "wait_page_ready" => {
            let to = cfg
                .get("timeout_ms")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(30000);
            page.wait_ms(to.min(1500)).await;
            Ok(())
        }
        "watch_video" => {
            let ms = cfg
                .get("duration_ms")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|n| *n > 0)
                .unwrap_or_else(|| 3000 + rand::thread_rng().gen_range(0..5000));
            page.wait_ms(ms).await;
            Ok(())
        }
        "random_delay" => {
            let min = cfg
                .get("min_ms")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|n| *n > 0)
                .unwrap_or(800);
            let max = cfg
                .get("max_ms")
                .and_then(|v| v.trim().parse::<u64>().ok())
                .filter(|n| *n > min)
                .unwrap_or(2500);
            page.human_pause(min, max).await;
            Ok(())
        }
        "random_yes_no" => {
            let pct = parse_yes_percent(cfg);
            let roll = rand::thread_rng().gen_range(0..100);
            rs.add_step_extra("yes_percent", &pct.to_string());
            rs.add_step_extra("roll", &roll.to_string());
            if roll < pct {
                rs.add_step_extra("result", "yes");
                Ok(())
            } else {
                rs.add_step_extra("result", "no");
                Err(anyhow!("random_yes_no: no (roll={roll} yes_percent={pct})"))
            }
        }
        "if_condition" => {
            let ex = cfg
                .get("expect")
                .map(|s| s.trim().to_lowercase())
                .unwrap_or_default();
            match ex.as_str() {
                "always_true" => {
                    rs.add_step_extra("result", "true");
                    rs.add_step_extra("expect", "always_true");
                    Ok(())
                }
                "always_false" => {
                    rs.add_step_extra("result", "false");
                    rs.add_step_extra("expect", "always_false");
                    Err(anyhow!("if_condition: expect=always_false"))
                }
                _ => {
                    eval_assert(page, cfg)
                        .await
                        .map_err(|e| anyhow!("if_condition: {e}"))?;
                    rs.add_step_extra("result", "true");
                    rs.add_step_extra("expect", if ex.is_empty() { "visible" } else { &ex });
                    Ok(())
                }
            }
        }
        "check_login" => {
            if is_logged_in(page).await {
                Ok(())
            } else {
                Err(anyhow!("check_login: chưa đăng nhập"))
            }
        }
        "login" => auth_login(page, account, action, log).await,
        "comment_video" => social_comment(page, account, action).await,
        "share_video" => social_share(page, action).await,
        "reply_comment" => social_reply(page, account, action).await,
        "check_scroll_end" => check_scroll_end(page, rs, cfg).await,
        "like_video" | "follow_user" | "next_video_post" => {
            let steps = steps_for_legacy(&action.type_, action)?;
            log(&format!(
                "[PWX] legacy {} steps={}",
                action.type_,
                steps.len()
            ));
            run_atomics(page, account, action, &steps).await
        }
        "playwright_atomics" => {
            let steps = steps_from_action(action)?;
            if steps.is_empty() {
                return Err(anyhow!("playwright_atomics: danh sách atomic rỗng"));
            }
            run_atomics(page, account, action, &steps).await
        }
        // AI executors — route text generation through the bridge.
        "ai_gent_comment" => ai_gent_comment(page, rs, action, bridge, log).await,
        "get_info_post" => get_info_post(page, rs).await,
        "get_comments_in_page" => get_comments_in_page(page, rs, cfg).await,
        "reply_comment_ai" => reply_comment_ai(page, account, action, bridge, log).await,
        "ai_playwright_agent" => ai_playwright_agent(page, action, bridge, log).await,
        // start/log/notification/loop_* are handled by the walker; treat as no-op if reached.
        "start" | "log" | "notification" | "loop_repeat" | "loop_if" => Ok(()),
        other => Err(anyhow!("unknown action type: {other}")),
    }
}

fn steps_from_action(action: &FlowAction) -> Result<Vec<FlowAtomic>> {
    if !action.atomics.is_empty() {
        return Ok(action.atomics.clone());
    }
    let raw = action
        .config
        .get("atomics_json")
        .map(|s| s.trim())
        .unwrap_or("");
    if raw.is_empty() {
        return Err(anyhow!(
            "playwright_atomics: thiếu atomics hoặc config atomics_json"
        ));
    }
    serde_json::from_str(raw).map_err(|e| anyhow!("atomics_json: {e}"))
}

fn parse_yes_percent(cfg: &StrMap) -> i32 {
    for k in ["yes_percent", "probability", "percent", "p"] {
        if let Some(v) = cfg.get(k) {
            if let Ok(n) = v.trim().parse::<i32>() {
                return n.clamp(0, 100);
            }
        }
    }
    50
}

fn urlencoding(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

async fn open_comment_panel(page: &dyn PageOps) -> Result<()> {
    page.click_selectors(
        &sv(&[
            r#"[data-e2e="comment-icon"]"#,
            r#"[data-e2e="browse-comment"]"#,
            r#"button[aria-label*="Comment" i]"#,
        ]),
        8000,
    )
    .await
}

async fn social_comment(
    page: &dyn PageOps,
    account: &TikTokAccount,
    action: &FlowAction,
) -> Result<()> {
    let text = action.config.get("text").map(|s| s.trim()).unwrap_or("");
    if text.is_empty() {
        return Err(anyhow!(
            "comment text is empty for account {}",
            account.username
        ));
    }
    open_comment_panel(page)
        .await
        .map_err(|e| anyhow!("comment_video mở panel: {e}"))?;
    page.human_pause(300, 700).await;
    let boxes = sv(COMMENT_BOX_SELECTORS);
    page.click_selectors_optional(&boxes, 5000).await;
    page.human_pause(200, 400).await;
    page.type_text(text)
        .await
        .map_err(|e| anyhow!("comment_video gõ nội dung: {e}"))?;
    page.human_pause(200, 350).await;
    page.press_key("Enter").await.ok();
    Ok(())
}

async fn social_share(page: &dyn PageOps, action: &FlowAction) -> Result<()> {
    let mode = action
        .config
        .get("share_mode")
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "copy_link".into());
    page.click_selectors(
        &sv(&[
            r#"[data-e2e="share-icon"]"#,
            r#"[data-e2e="browse-share"]"#,
            r#"button[aria-label*="Share" i]"#,
        ]),
        8000,
    )
    .await
    .map_err(|e| anyhow!("share_video mở menu: {e}"))?;
    page.human_pause(400, 800).await;
    let base = r#"button, div, span, [role="menuitem"]"#;
    match mode.as_str() {
        "copy_link" | "copy" | "link" => page
            .click_by_text(base, "Copy link", 6000)
            .await
            .map_err(|e| anyhow!("share_video copy link: {e}")),
        "repost" | "re-post" => page
            .click_by_text(base, "Repost", 6000)
            .await
            .map_err(|e| anyhow!("share_video repost: {e}")),
        "messages" | "message" | "dm" => page
            .click_by_text(base, "Send to friends", 6000)
            .await
            .or(page.click_by_text(base, "Message", 6000).await)
            .map_err(|e| anyhow!("share_video messages: {e}")),
        other => Err(anyhow!("share_video: share_mode không hỗ trợ: {other:?}")),
    }
}

async fn social_reply(
    page: &dyn PageOps,
    account: &TikTokAccount,
    action: &FlowAction,
) -> Result<()> {
    let text = action.config.get("text").map(|s| s.trim()).unwrap_or("");
    if text.is_empty() {
        return Err(anyhow!(
            "reply_comment: thiếu config text (account {})",
            account.username
        ));
    }
    open_comment_panel(page)
        .await
        .map_err(|e| anyhow!("reply_comment mở panel: {e}"))?;
    page.human_pause(400, 900).await;
    // Click the Reply button on the target comment row (by index or default first).
    let idx = action
        .config
        .get("comment_index")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let sel = format!(
        r#"[data-e2e="comment-level-1"]:nth-of-type({}) button"#,
        idx + 1
    );
    if page.click_by_text(&sel, "Reply", 6000).await.is_err() {
        page.click_by_text(r#"[data-e2e="comment-level-1"] button"#, "Reply", 6000)
            .await
            .map_err(|e| anyhow!("reply_comment bấm Reply: {e}"))?;
    }
    page.human_pause(250, 500).await;
    page.type_text(text)
        .await
        .map_err(|e| anyhow!("reply_comment gõ: {e}"))?;
    page.human_pause(150, 300).await;
    page.press_key("Enter").await.ok();
    Ok(())
}

async fn auth_login(
    page: &dyn PageOps,
    account: &TikTokAccount,
    action: &FlowAction,
    log: &LogFn,
) -> Result<()> {
    if is_logged_in(page).await {
        return Ok(());
    }
    let user = action
        .config
        .get("username")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(account.username.trim());
    let pass = action
        .config
        .get("password")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(account.password.trim());
    if user.is_empty() || pass.is_empty() {
        return Err(anyhow!("login: thiếu username/password"));
    }
    page.goto(
        "https://www.tiktok.com/login/phone-or-email/email",
        "domcontentloaded",
        45000,
    )
    .await
    .ok();
    page.human_pause(500, 1000).await;
    let user_sels = sv(&[
        r#"form input[name="username"]"#,
        r#"form input[placeholder*="Email or username" i]"#,
        r#"input[name="username"]"#,
        r#"input[placeholder*="Email or username" i]"#,
    ]);
    let pass_sels = sv(&[
        r#"form input[type="password"]"#,
        r#"form input[placeholder*="Password" i]"#,
        r#"input[type="password"]"#,
        r#"input[placeholder*="Password" i]"#,
    ]);
    if page.find_first_visible(&user_sels, 20000).await.is_none()
        || page.find_first_visible(&pass_sels, 5000).await.is_none()
    {
        return Err(anyhow!("login: không tìm thấy input (có thể QR/captcha/2FA). hãy đăng nhập thủ công 1 lần trong profile"));
    }
    let user_sel = page.find_first_visible(&user_sels, 5000).await.unwrap();
    page.fill(&user_sel, user, 10000)
        .await
        .map_err(|e| anyhow!("login: không nhập được username: {e}"))?;
    page.wait_ms(800).await;
    let pass_sel = page.find_first_visible(&pass_sels, 5000).await.unwrap();
    page.fill(&pass_sel, pass, 10000)
        .await
        .map_err(|e| anyhow!("login: không nhập được password: {e}"))?;
    log(&"[PWX] login credentials filled".to_string());
    page.human_pause(600, 1000).await;
    page.click_selectors_optional(
        &sv(&[
            r#"form button[data-e2e="login-button"]"#,
            r#"form button[type="submit"]"#,
        ]),
        8000,
    )
    .await;
    page.press_key("Enter").await.ok();

    // Verify within ~30s.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        page.human_pause(600, 900).await;
        if is_logged_in(page).await {
            return Ok(());
        }
    }
    Err(anyhow!("login: chưa thấy đăng nhập thành công (có thể captcha/2FA/QR). hãy đăng nhập thủ công và reuse profile"))
}

async fn check_scroll_end(page: &dyn PageOps, rs: &RunState, cfg: &StrMap) -> Result<()> {
    let selector = cfg.get("selector").map(|s| s.trim()).unwrap_or("");
    if selector.is_empty() {
        return Err(anyhow!("check_scroll_end: missing selector"));
    }
    let tol = cfg
        .get("tolerance_px")
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|n| *n >= 0.0)
        .unwrap_or(1.0);
    let sel_j = serde_json::to_string(selector).unwrap();
    let js = format!(
        r#"(() => {{ const el = document.querySelector({sel_j}); if(!el) return {{ok:false}}; const top=Number(el.scrollTop||0), client=Number(el.clientHeight||0), scroll=Number(el.scrollHeight||0); return {{ok:true, at_end:(top+client)>=(scroll-{tol}), top, client, scroll}}; }})()"#
    );
    let v = page.eval(&js).await?;
    if !v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
        return Err(anyhow!("check_scroll_end: element_not_found"));
    }
    let at_end = v.get("at_end").and_then(Value::as_bool).unwrap_or(false);
    let out_key = cfg
        .get("output_param_key")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("is_scroll_end");
    let v_true = cfg
        .get("value_true")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("true");
    let v_false = cfg
        .get("value_false")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("false");
    let out_val = if at_end { v_true } else { v_false };
    let mut patch = StrMap::new();
    patch.insert(out_key.to_string(), out_val.to_string());
    rs.merge_params(&patch);
    rs.add_step_extra("result", &at_end.to_string());
    rs.add_step_extra("output_param_key", out_key);
    rs.add_step_extra("output_param_value", out_val);
    Ok(())
}

// ---- AI executors (bridge-backed) ----

async fn scrape_post_caption(page: &dyn PageOps) -> String {
    let js = r#"(() => { const el = document.querySelector('[data-e2e="browse-video-desc"], [data-e2e="video-desc"], h1'); return el ? (el.innerText||el.textContent||'').slice(0,600) : ''; })()"#;
    match page.eval(js).await {
        Ok(Value::String(s)) => s,
        _ => String::new(),
    }
}

async fn ai_gent_comment(
    page: &dyn PageOps,
    rs: &RunState,
    action: &FlowAction,
    bridge: &Bridge,
    log: &LogFn,
) -> Result<()> {
    let cfg = &action.config;
    let out_key = cfg
        .get("output_param_key")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("comment_text");
    // If a candidate list is provided, pick one at random (no LLM needed).
    if let Some(list) = cfg
        .get("candidates")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        let items: Vec<&str> = list
            .split('\n')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if !items.is_empty() {
            let pick = items[rand::thread_rng().gen_range(0..items.len())].to_string();
            store_comment(rs, out_key, &pick);
            log(&format!("[PWX] ai_gent_comment pick từ list -> {out_key}"));
            return Ok(());
        }
    }
    let caption = scrape_post_caption(page).await;
    let style = cfg
        .get("style")
        .or_else(|| cfg.get("instruction"))
        .map(|s| s.trim())
        .unwrap_or("thân thiện, tự nhiên, ngắn");
    let user = format!(
        "Viết MỘT bình luận TikTok bằng tiếng Việt ({style}). Không hashtag trừ khi tự nhiên. Chỉ trả về nội dung bình luận.\nCaption video: {}",
        if caption.is_empty() { "(không đọc được)" } else { &caption }
    );
    let reply = bridge
        .llm(
            "Bạn viết bình luận mạng xã hội ngắn, tự nhiên, an toàn.",
            &user,
            120,
            Duration::from_secs(60),
        )
        .await?;
    let comment = reply.text.trim().trim_matches('"').to_string();
    if comment.is_empty() {
        return Err(anyhow!("ai_gent_comment: LLM trả rỗng"));
    }
    store_comment(rs, out_key, &comment);
    log(&format!("[PWX] ai_gent_comment sinh comment -> {out_key}"));
    Ok(())
}

fn store_comment(rs: &RunState, out_key: &str, comment: &str) {
    let mut patch = StrMap::new();
    patch.insert(out_key.to_string(), comment.to_string());
    // also expose as `text` so a following comment step can read {{param.text}}
    patch.insert("text".to_string(), comment.to_string());
    rs.merge_params(&patch);
    rs.add_step_extra("comment", comment);
    rs.add_step_extra("output_param_key", out_key);
}

async fn get_info_post(page: &dyn PageOps, rs: &RunState) -> Result<()> {
    let js = r#"(() => {
        const q = (s) => { const el = document.querySelector(s); return el ? (el.innerText||el.textContent||'').trim() : ''; };
        return {
            caption: q('[data-e2e="browse-video-desc"], [data-e2e="video-desc"]'),
            author: q('[data-e2e="browse-username"], [data-e2e="video-author-uniqueid"]'),
            likes: q('[data-e2e="browse-like-count"], [data-e2e="like-count"]'),
            comments: q('[data-e2e="browse-comment-count"], [data-e2e="comment-count"]'),
            url: location.href
        };
    })()"#;
    let v = page.eval(js).await?;
    for k in ["caption", "author", "likes", "comments", "url"] {
        if let Some(s) = v.get(k).and_then(Value::as_str) {
            rs.add_step_extra(k, s);
        }
    }
    let mut patch = StrMap::new();
    if let Some(s) = v.get("caption").and_then(Value::as_str) {
        patch.insert("post_caption".into(), s.to_string());
    }
    rs.merge_params(&patch);
    Ok(())
}

async fn get_comments_in_page(page: &dyn PageOps, rs: &RunState, cfg: &StrMap) -> Result<()> {
    let limit = cfg
        .get("limit")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(20)
        .min(100);
    let js = format!(
        r#"(() => {{ const nodes = document.querySelectorAll('[data-e2e="comment-level-1"] [data-e2e="comment-text"], [data-e2e="comment-text"]'); const out=[]; for (const n of nodes) {{ const t=(n.innerText||n.textContent||'').trim(); if(t) out.push(t); if(out.length>={limit}) break; }} return out; }})()"#
    );
    let v = page.eval(&js).await?;
    let comments: Vec<String> = v
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    rs.add_step_extra("comment_count", &comments.len().to_string());
    rs.add_step_extra(
        "comments_json",
        &serde_json::to_string(&comments).unwrap_or_default(),
    );
    Ok(())
}

async fn reply_comment_ai(
    page: &dyn PageOps,
    account: &TikTokAccount,
    action: &FlowAction,
    bridge: &Bridge,
    log: &LogFn,
) -> Result<()> {
    open_comment_panel(page).await.ok();
    page.human_pause(400, 900).await;
    // Read the first comment text to reply to.
    let target = page
        .inner_text(r#"[data-e2e="comment-level-1"] [data-e2e="comment-text"]"#)
        .await
        .unwrap_or_default();
    let style = action
        .config
        .get("style")
        .map(|s| s.trim())
        .unwrap_or("thân thiện, ngắn");
    let user = format!(
        "Viết MỘT câu trả lời TikTok bằng tiếng Việt ({style}) cho bình luận sau. Chỉ trả về nội dung.\nBình luận: {}",
        if target.is_empty() { "(không đọc được)" } else { &target }
    );
    let reply = bridge
        .llm(
            "Bạn trả lời bình luận mạng xã hội ngắn, lịch sự.",
            &user,
            120,
            Duration::from_secs(60),
        )
        .await?;
    let text = reply.text.trim().trim_matches('"').to_string();
    if text.is_empty() {
        return Err(anyhow!("reply_comment_ai: LLM trả rỗng"));
    }
    log(&"[PWX] reply_comment_ai sinh câu trả lời".to_string());
    let mut act = action.clone();
    act.config.insert("text".into(), text);
    social_reply(page, account, &act).await
}

/// Reduced LLM tool-loop: the model inspects a compact DOM outline and calls
/// goto / click_text / fill / done. A faithful-enough port of the AI Playwright
/// Agent for the common "reach a goal" case.
async fn ai_playwright_agent(
    page: &dyn PageOps,
    action: &FlowAction,
    bridge: &Bridge,
    log: &LogFn,
) -> Result<()> {
    let goal = action
        .config
        .get("goal")
        .or_else(|| action.config.get("instruction"))
        .map(|s| s.trim())
        .unwrap_or("");
    if goal.is_empty() {
        return Err(anyhow!("ai_playwright_agent: thiếu goal/instruction"));
    }
    let max_steps = action
        .config
        .get("max_steps")
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(8)
        .min(20);
    let system = "Bạn điều khiển trình duyệt để hoàn thành mục tiêu trên TikTok. Mỗi lượt trả về DUY NHẤT một JSON: {\"tool\":\"goto|click_text|fill|done\",\"url\":\"\",\"text\":\"\",\"selector\":\"\",\"reason\":\"\"}. Dùng click_text để bấm nút chứa 'text'. done khi đạt mục tiêu.";
    for step in 0..max_steps {
        let outline = dom_outline(page).await;
        let user = format!(
            "Mục tiêu: {goal}\nURL hiện tại: {}\nDOM (rút gọn):\n{}\n\nHành động tiếp theo (JSON):",
            page.url().await,
            outline
        );
        let reply = bridge
            .llm(system, &user, 300, Duration::from_secs(60))
            .await?;
        let obj = crate::ai::extract_json_object(&reply.text)
            .ok_or_else(|| anyhow!("ai_playwright_agent: LLM không trả JSON"))?;
        let tool = obj.get("tool").and_then(Value::as_str).unwrap_or("");
        log(&format!(
            "[PWX] ai_agent step {}/{} tool={tool}",
            step + 1,
            max_steps
        ));
        match tool {
            "done" => return Ok(()),
            "goto" => {
                let u = obj.get("url").and_then(Value::as_str).unwrap_or("");
                if !u.is_empty() {
                    page.goto(u, "domcontentloaded", 45000).await.ok();
                }
            }
            "click_text" => {
                let t = obj.get("text").and_then(Value::as_str).unwrap_or("");
                if !t.is_empty() {
                    page.click_by_text(r#"button, a, [role="button"], div, span"#, t, 6000)
                        .await
                        .ok();
                }
            }
            "fill" => {
                let sel = obj.get("selector").and_then(Value::as_str).unwrap_or("");
                let t = obj.get("text").and_then(Value::as_str).unwrap_or("");
                if !sel.is_empty() {
                    page.fill(sel, t, 8000).await.ok();
                }
            }
            other => log(&format!("[PWX] ai_agent tool không hỗ trợ: {other}")),
        }
        page.human_pause(600, 1200).await;
    }
    Err(anyhow!(
        "ai_playwright_agent: hết {max_steps} bước mà chưa done"
    ))
}

async fn dom_outline(page: &dyn PageOps) -> String {
    let js = r#"(() => {
        const out = [];
        const els = document.querySelectorAll('button, a, input, [role="button"], [data-e2e]');
        let i = 0;
        for (const el of els) {
            if (i >= 40) break;
            const r = el.getBoundingClientRect();
            if (r.width<=0 || r.height<=0) continue;
            const t = (el.innerText||el.getAttribute('aria-label')||el.getAttribute('placeholder')||'').trim().slice(0,40);
            const e2e = el.getAttribute('data-e2e')||'';
            out.push(`${el.tagName.toLowerCase()}${e2e?('['+e2e+']'):''} "${t}"`);
            i++;
        }
        return out.join('\n');
    })()"#;
    match page.eval(js).await {
        Ok(Value::String(s)) => s,
        _ => String::new(),
    }
}
