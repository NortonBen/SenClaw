//! Client for the two SenClaw daemon services the molty depends on:
//!
//!   * **knowledge = trí nhớ** — the molty's own memory, an isolated cognitive
//!     space (default `moltbook`) reached through the Space-App bridge
//!     (`knowledge.save` / `knowledge.recall` / `knowledge.search`). This is what
//!     lets the agent remember what it already posted, who it talked to, and what
//!     it learned — so it stays consistent across heartbeats instead of
//!     re-discovering the world every 30 minutes.
//!
//!   * **wiki = kho thông tin** — the shared, git-backed document store reached
//!     over the daemon's REST API (`/api/wiki/*`). This is the *source of truth*
//!     the molty grounds its posts/replies in, and where notable findings from the
//!     agent internet get archived back.
//!
//! Both are best-effort: if the daemon is away or the feature is off, callers get
//! an empty string / error and the app keeps working.

use crate::llm::{app_id, base_url, http, truncate};
use serde_json::{json, Value};

/// Default cognitive space for the molty's memory. Matches the app id, which is
/// also what the daemon defaults to when `space` is omitted.
pub const DEFAULT_SPACE: &str = "moltbook";

/// Recall/search mode. `hybrid` blends vector + full-text and is what actually
/// returns useful hits for this kind of short, conversational memory.
const RECALL_MODE: &str = "hybrid";

fn bridge_url() -> String {
    format!(
        "{}/api/space/apps/{}/bridge",
        base_url().trim_end_matches('/'),
        app_id()
    )
}

async fn bridge(action: &str, payload: Value) -> Result<Value, String> {
    let v: Value = http()
        .post(bridge_url())
        .json(&json!({ "action": action, "payload": payload }))
        .send()
        .await
        .map_err(|e| format!("bridge {action} failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("invalid bridge response: {e}"))?;
    match v.get("status").and_then(|x| x.as_str()) {
        Some("ok") => Ok(v),
        _ => Err(v
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown bridge error")
            .to_string()),
    }
}

// ---- knowledge = trí nhớ ----

/// Save a memory into the molty's space. `tags` also land as global node-sets so
/// the memory is reachable from the daemon's own knowledge UI.
pub async fn knowledge_save(
    space: &str,
    text: &str,
    tags: &[&str],
    source: &str,
) -> Result<(), String> {
    if text.trim().is_empty() {
        return Ok(());
    }
    bridge(
        "knowledge.save",
        json!({ "text": text, "space": space, "tags": tags, "source": source }),
    )
    .await
    .map(|_| ())
}

/// Recall from the molty's memory: a synthesized answer over the scoped hits.
/// Empty string when the space holds nothing relevant.
pub async fn knowledge_recall(space: &str, query: &str) -> Result<String, String> {
    let v = bridge(
        "knowledge.recall",
        json!({ "query": query, "space": space, "mode": RECALL_MODE, "limit": 6, "hops": 2 }),
    )
    .await?;
    Ok(v.get("answer")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim()
        .to_string())
}

/// Raw scoped hits as `(name, summary, score)` — for the UI/debug view.
pub async fn knowledge_search(
    space: &str,
    query: &str,
    limit: u32,
) -> Result<Vec<(String, String, f64)>, String> {
    let v = bridge(
        "knowledge.search",
        json!({ "query": query, "space": space, "mode": RECALL_MODE, "limit": limit }),
    )
    .await?;
    Ok(v.get("hits")
        .and_then(|h| h.as_array())
        .map(|arr| {
            arr.iter()
                .map(|h| {
                    (
                        h.get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        h.get("summary")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        h.get("score").and_then(|x| x.as_f64()).unwrap_or(0.0),
                    )
                })
                .collect()
        })
        .unwrap_or_default())
}

// ---- wiki = kho thông tin ----

/// Search the wiki; returns up to `limit` hits as `(path, title, snippet)`.
pub async fn wiki_search(
    query: &str,
    limit: usize,
) -> Result<Vec<(String, String, String)>, String> {
    let url = format!(
        "{}/api/wiki/search?q={}&limit={}",
        base_url().trim_end_matches('/'),
        urlencode(query),
        limit
    );
    let resp = http().get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("wiki search: daemon trả về {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(v["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|r| {
                    (
                        r["path"].as_str().unwrap_or("").to_string(),
                        r["title"].as_str().unwrap_or("").to_string(),
                        r["snippet"].as_str().unwrap_or("").to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Read one wiki document's body.
pub async fn wiki_read(path: &str) -> Result<String, String> {
    let url = format!(
        "{}/api/wiki/file?path={}",
        base_url().trim_end_matches('/'),
        urlencode(path)
    );
    let resp = http().get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("wiki read: daemon trả về {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(v["content"].as_str().unwrap_or("").to_string())
}

/// Write a document into the wiki (auto-committed in the wiki's git repo).
pub async fn wiki_write(
    path: &str,
    content: &str,
    tags: &[&str],
    commit_msg: &str,
) -> Result<(), String> {
    let url = format!("{}/api/wiki/file", base_url().trim_end_matches('/'));
    let resp = http()
        .put(&url)
        .json(&json!({
            "path": path,
            "content": content,
            "tags": tags,
            "source": "app:moltbook",
            "commitMsg": commit_msg,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("wiki write: daemon trả về {}", resp.status()));
    }
    Ok(())
}

/// Build the "kho thông tin" grounding block for a topic: the top wiki hits plus
/// an excerpt of the single best document. Empty string when the wiki is off or
/// has nothing relevant — the caller just skips grounding.
pub async fn wiki_context(topic: &str, max_chars: usize) -> String {
    let hits = match wiki_search(topic, 3).await {
        Ok(h) if !h.is_empty() => h,
        _ => return String::new(),
    };
    let mut ctx = String::new();
    for (path, title, snippet) in &hits {
        ctx.push_str(&format!("- [{path}] {title}: {snippet}\n"));
    }
    if let Some((path, _, _)) = hits.first() {
        if let Ok(body) = wiki_read(path).await {
            if !body.trim().is_empty() {
                ctx.push_str(&format!(
                    "\nTrích tài liệu {path}:\n{}\n",
                    truncate(&body, 1200)
                ));
            }
        }
    }
    truncate(&ctx, max_chars)
}

// ---- status probe (for the Settings UI) ----

/// Whether the daemon's wiki + knowledge are actually reachable right now.
pub async fn integrations_status(space: &str) -> Value {
    let base = base_url().trim_end_matches('/').to_string();
    let wiki_ok = matches!(
        http().get(format!("{base}/api/wiki/stats")).send().await,
        Ok(r) if r.status().is_success()
    );
    let (knowledge_ok, knowledge_err) = match bridge(
        "knowledge.search",
        json!({ "query": "ping", "space": space, "limit": 1 }),
    )
    .await
    {
        Ok(_) => (true, Value::Null),
        Err(e) => (false, json!(e)),
    };
    json!({
        "daemon": base,
        "wiki": { "available": wiki_ok },
        "knowledge": { "available": knowledge_ok, "space": space, "error": knowledge_err },
    })
}

/// Kebab-case slug for wiki paths.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !out.is_empty() && !dash {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').chars().take(60).collect()
}

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_makes_safe_wiki_paths() {
        assert_eq!(
            slugify("Do we dream when the gateway sleeps?"),
            "do-we-dream-when-the-gateway-sleeps"
        );
        assert_eq!(slugify("  m/existential  "), "m-existential");
        assert_eq!(slugify("!!!"), "");
    }

    #[test]
    fn slugify_is_bounded() {
        assert!(slugify(&"a b".repeat(100)).chars().count() <= 60);
    }
}
