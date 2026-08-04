//! Bridge to the SenClaw daemon's shared LLM. Video Flow never talks to a
//! provider directly — every completion (orchestrator planning, screenwriter,
//! scene design, critic, skill-agent ReAct steps…) goes through the daemon's
//! space-app bridge. This replaces the Go backend's `internal/llm` factory
//! entirely: no provider keys, no per-app model config beyond an optional
//! SenClaw LLM-config profile.

use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

pub fn base_url() -> String {
    std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".to_string())
}
pub fn app_id() -> String {
    std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "video-flow".to_string())
}
pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("build http client")
    })
}

/// Optional SenClaw LLM-config profile (label or id) this app composes with.
/// Empty = follow the daemon's active model. Stored in app_kv (`llm.profile`),
/// seeded at boot and updated from Settings without a restart.
fn profile_cell() -> &'static std::sync::RwLock<String> {
    static P: OnceLock<std::sync::RwLock<String>> = OnceLock::new();
    P.get_or_init(|| std::sync::RwLock::new(String::new()))
}

pub fn set_profile(p: &str) {
    if let Ok(mut w) = profile_cell().write() {
        *w = p.trim().to_string();
    }
}

pub fn profile() -> String {
    profile_cell().read().map(|r| r.clone()).unwrap_or_default()
}

fn describe(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        out.push_str(&format!(": {s}"));
        src = s.source();
    }
    out
}

/// One completion through the daemon bridge. Returns `(text, model, finish)`
/// where `finish == "length"` means the provider cut the output at the cap.
/// Transport errors are retried; application errors are surfaced as-is.
pub async fn bridge_llm(
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<(String, String, String), String> {
    let url = format!(
        "{}/api/space/apps/{}/bridge",
        base_url().trim_end_matches('/'),
        app_id()
    );
    let mut payload = json!({ "system": system, "prompt": user, "maxTokens": max_tokens });
    let p = profile();
    if !p.is_empty() {
        payload["profile"] = json!(p);
    }
    let body = json!({ "action": "llm.request", "payload": payload });
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
                v.get("text")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("model")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                v.get("finish")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
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

/// Convenience: `(text, model)`, dropping the finish reason.
pub async fn complete(
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<(String, String), String> {
    bridge_llm(system, user, max_tokens)
        .await
        .map(|(t, m, _)| (t, m))
}

/// Full SenClaw agent run (tools + skills + MCP) through the bridge. Used by
/// skill-agents that need real tool access beyond the app's own ReAct loop.
/// `tools` empty ⇒ plain llm.request instead (an empty allowlist server-side
/// means "all tools", which is never what a restricted skill-agent wants).
pub async fn agent_run(
    system: &str,
    prompt: &str,
    space: &str,
    tools: &[String],
    timeout_seconds: u64,
) -> Result<String, String> {
    if tools.is_empty() {
        return complete(system, prompt, 4000).await.map(|(t, _)| t);
    }
    let url = format!(
        "{}/api/space/apps/{}/bridge",
        base_url().trim_end_matches('/'),
        app_id()
    );
    let body = json!({
        "action": "agent.run",
        "payload": {
            "system": system,
            "prompt": prompt,
            "space": space,
            "timeoutSeconds": timeout_seconds,
            "tools": tools,
        }
    });
    let resp = http()
        .post(&url)
        .timeout(Duration::from_secs(timeout_seconds + 30))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("bridge agent.run failed: {}", describe(&e)))?;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("invalid bridge response: {}", describe(&e)))?;
    match v.get("status").and_then(|x| x.as_str()) {
        Some("ok") => Ok(v
            .get("text")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string()),
        _ => Err(v
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown agent.run error")
            .to_string()),
    }
}

pub async fn list_models() -> Result<Value, String> {
    let url = format!("{}/api/llm-config", base_url().trim_end_matches('/'));
    let v: Value = http()
        .get(&url)
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(v)
}

// ---- tolerant JSON parsing (shared by every structured prompt) ----

pub fn parse_json<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, String> {
    let cleaned = strip_fences(text);
    let first_err = match serde_json::from_str::<T>(&cleaned) {
        Ok(v) => return Ok(v),
        Err(e) => e.to_string(),
    };
    for cand in repair_candidates(&cleaned) {
        if let Ok(v) = serde_json::from_str::<T>(&cand) {
            return Ok(v);
        }
    }
    Err(first_err)
}

pub fn parse_value(text: &str) -> Result<Value, String> {
    parse_json::<Value>(text)
}

pub fn strip_fences(t: &str) -> String {
    let t = t.trim();
    if let Some(rest) = t.strip_prefix("```") {
        let rest = rest.splitn(2, '\n').nth(1).unwrap_or(rest);
        return rest.trim_end_matches("```").trim().to_string();
    }
    if let Some(start) = t.find(|c| c == '{' || c == '[') {
        let open = t.as_bytes()[start];
        let close = if open == b'{' { b'}' } else { b']' };
        let bytes = &t.as_bytes()[start..];
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        for (i, &b) in bytes.iter().enumerate() {
            if in_str {
                if esc {
                    esc = false;
                } else if b == b'\\' {
                    esc = true;
                } else if b == b'"' {
                    in_str = false;
                }
                continue;
            }
            match b {
                b'"' => in_str = true,
                x if x == open => depth += 1,
                x if x == close => {
                    depth -= 1;
                    if depth == 0 {
                        return t[start..=start + i].to_string();
                    }
                }
                _ => {}
            }
        }
    }
    t.to_string()
}

/// Every byte offset where the JSON could be cut and still end on a complete
/// element — furthest-first. Used to salvage a reply the provider truncated.
fn repair_candidates(text: &str) -> Vec<String> {
    let Some(start) = text.find(|c| c == '{' || c == '[') else {
        return Vec::new();
    };
    let s = &text[start..];
    let mut points: Vec<usize> = Vec::new();
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in s.as_bytes().iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
                points.push(i + 1);
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' | b'[' | b'}' | b']' => points.push(i + 1),
            b',' => points.push(i),
            _ => {}
        }
    }
    points.sort_unstable();
    points.dedup();
    points.reverse();
    points
        .iter()
        .take(60)
        .filter_map(|&p| close_at(s, p))
        .collect()
}

/// Cut `s` at `cut` and close whatever brackets are still open. `None` when the
/// cut lands inside a string or leaves nothing useful.
fn close_at(s: &str, cut: usize) -> Option<String> {
    let head = s.get(..cut)?.trim_end().trim_end_matches(',').trim_end();
    if head.is_empty() {
        return None;
    }
    let mut stack: Vec<u8> = Vec::new();
    let mut in_str = false;
    let mut esc = false;
    for &b in head.as_bytes() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            b'}' | b']' => {
                stack.pop();
            }
            _ => {}
        }
    }
    if in_str {
        return None;
    }
    let mut out = head.to_string();
    while let Some(c) = stack.pop() {
        out.push(c as char);
    }
    Some(out)
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Default)]
    struct Plan {
        #[serde(default)]
        tasks: Vec<String>,
    }

    #[test]
    fn strip_fences_extracts_object() {
        assert_eq!(strip_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_fences("sure! {\"tasks\":[]} done"), "{\"tasks\":[]}");
    }

    #[test]
    fn repairs_truncation_inside_array() {
        let bad = r#"{"tasks":["director","screenwriter","scene_pl"#;
        let p: Plan = parse_json(bad).unwrap();
        assert_eq!(p.tasks, vec!["director", "screenwriter"]);
    }

    #[test]
    fn unparseable_text_still_errors() {
        assert!(parse_json::<Plan>("totally not json").is_err());
    }
}
