//! Thin client over the SenClaw daemon bridge. The AI Chat app owns no LLM of
//! its own — every completion runs on the daemon's active model, reached via
//! `POST {base}/api/space/apps/{app}/bridge`.

use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

pub fn base_url() -> String {
    std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".to_string())
}

pub fn app_id() -> String {
    std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "ai-chat".to_string())
}

/// One shared connection pool for every bridge call.
pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(125))
            .build()
            .expect("build http client")
    })
}

pub fn bridge_url() -> String {
    format!("{}/api/space/apps/{}/bridge", base_url().trim_end_matches('/'), app_id())
}

/// Walk the error chain so "error sending request" also says why.
fn describe(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        out.push_str(&format!(": {}", s));
        src = s.source();
    }
    out
}

/// One-shot completion (no tools). Returns `(text, model, finish)`.
/// Transport errors are retried; application errors are returned as-is.
pub async fn bridge_llm(system: &str, user: &str, max_tokens: u32) -> Result<(String, String, String), String> {
    let url = bridge_url();
    let body = json!({
        "action": "llm.request",
        "payload": { "system": system, "prompt": user, "maxTokens": max_tokens },
    });
    let mut last_err = String::new();
    for attempt in 0..3u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(700 * attempt as u64)).await;
        }
        let resp = match http().post(&url).json(&body).send().await {
            Ok(r) => r,
            Err(e) => {
                last_err = format!("bridge llm.request failed ({url}): {}", describe(&e));
                continue;
            }
        };
        let v: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                last_err = format!("invalid bridge response: {}", describe(&e));
                continue;
            }
        };
        return match v.get("status").and_then(|x| x.as_str()) {
            Some("ok") => Ok((
                v.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("finish").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            )),
            Some("pending") => Err("bridge LLM chưa được bật trong daemon này".to_string()),
            _ => Err(v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown LLM error")
                .to_string()),
        };
    }
    Err(last_err)
}

/// Run a FULL tool-enabled agent on the daemon. `tools` is the bot's per-bot
/// allowlist — when non-empty, the daemon restricts the agent to EXACTLY those
/// tools (this is the enforced MCP/skill security policy; see the `agent.run`
/// bridge in core `space.rs`). Empty = the daemon's full default toolset.
/// `space` isolates the agent's working memory. Returns the agent's final text.
pub async fn agent_run(
    system: &str,
    prompt: &str,
    space: &str,
    tools: &[String],
    model: Option<&str>,
    timeout_secs: u64,
) -> Result<String, String> {
    let mut payload = json!({
        "system": system,
        "prompt": prompt,
        "space": space,
        "timeoutSeconds": timeout_secs,
    });
    if !tools.is_empty() {
        payload["tools"] = json!(tools);
    }
    if let Some(m) = model.filter(|m| !m.is_empty()) {
        payload["model"] = json!(m);
    }
    let resp = http()
        .post(bridge_url())
        .json(&json!({ "action": "agent.run", "payload": payload }))
        .timeout(Duration::from_secs(timeout_secs + 30))
        .send()
        .await
        .map_err(|e| format!("agent.run failed: {}", describe(&e)))?;
    let v: Value = resp.json().await.map_err(|e| format!("invalid agent.run response: {}", e))?;
    match v.get("status").and_then(|x| x.as_str()) {
        Some("ok") => Ok(v.get("text").and_then(|x| x.as_str()).unwrap_or("").trim().to_string()),
        _ => Err(v
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("agent.run error (daemon chưa hỗ trợ?)")
            .to_string()),
    }
}

/// Info for the Settings panel: whether a live LLM is reachable + which model.
pub async fn llm_info() -> Value {
    let url = format!("{}/api/llm-config", base_url().trim_end_matches('/'));
    match http().get(&url).timeout(Duration::from_secs(4)).send().await {
        Ok(resp) if resp.status().is_success() => {
            let v: Value = resp.json().await.unwrap_or_default();
            json!({ "available": true, "config": v })
        }
        _ => json!({ "available": false }),
    }
}
