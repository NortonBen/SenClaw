//! The daemon bridge: the pieces the sales engine *reuses* rather than rebuilds.
//!
//! - long-term memory — the daemon's cognitive store, scoped per customer
//!   (`crm:sale:<customer_id>`). The CRM owns no vector store of its own.
//! - the shared wiki — product knowledge, so a draft is grounded in what the
//!   business actually published rather than whatever the model recalls.
//!
//! What used to live here and deliberately no longer does: the standalone AI
//! Sale app reached the CRM over `SENCLAW_CRM_URL` (`crm_base`,
//! `crm_upsert_customer`, `crm_get_customer`) because it ran in its own process
//! and could only name a customer by an id fetched over HTTP. Merged in, those
//! are plain `Db` calls — no HTTP hop, and no best-effort fallback to id 0 when
//! the other side was down (which is what let the same person be captured twice).
//!
//! `crate::llm` also talks to the daemon, but through `app_space_sdk`, whose
//! bridge helper covers only `llm.request`. The actions below need payload
//! fields the SDK doesn't expose (`mode: "hybrid"`, `limit`), so they post to
//! the bridge endpoint directly.

use serde_json::{json, Value};
use std::sync::OnceLock;
use std::time::Duration;

pub fn base_url() -> String {
    std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".to_string())
}

pub fn app_id() -> String {
    std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "crm".to_string())
}

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

/// POST one action to the app bridge. Returns the whole envelope on
/// `status: "ok"`, else the daemon's own message as the error.
pub async fn bridge(action: &str, payload: Value) -> Result<Value, String> {
    let url = bridge_url();
    let resp = http()
        .post(&url)
        .json(&json!({ "action": action, "payload": payload }))
        .send()
        .await
        .map_err(|e| format!("bridge {action} failed ({url}): {e}"))?;
    let v: Value = resp.json().await.map_err(|e| format!("invalid bridge response: {e}"))?;
    match v.get("status").and_then(|x| x.as_str()) {
        Some("ok") => Ok(v),
        _ => Err(v
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("unknown bridge error")
            .to_string()),
    }
}

// ---- long-term memory (daemon cognitive store, scoped per customer) ----

/// The knowledge space holding what we remember about one customer. One space
/// per customer is what keeps recall from bleeding across people.
pub fn lead_space(customer_id: i64) -> String {
    format!("crm:sale:{customer_id}")
}

pub async fn knowledge_save(space: &str, text: &str, source: &str) -> Result<(), String> {
    bridge("knowledge.save", json!({ "text": text, "space": space, "source": source }))
        .await
        .map(|_| ())
}

/// Scoped recall with synthesis; empty when the space holds nothing relevant.
/// `mode: "hybrid"` is load-bearing — the default mode misses on the short
/// conversational queries this engine asks with.
pub async fn knowledge_recall(space: &str, query: &str, limit: i64) -> Result<String, String> {
    let v = bridge(
        "knowledge.recall",
        json!({ "query": query, "space": space, "limit": limit, "mode": "hybrid" }),
    )
    .await?;
    Ok(v.get("answer").and_then(|x| x.as_str()).unwrap_or("").trim().to_string())
}

// ---- wiki (product knowledge for grounding) ----

/// Top wiki snippets for a query, joined. Shape-tolerant: the daemon has
/// returned `results`/`hits` carrying `snippet`/`body`/`content` over time.
pub async fn wiki_search(query: &str) -> Result<String, String> {
    let url =
        format!("{}/api/wiki/search?q={}", base_url().trim_end_matches('/'), urlencode(query));
    let resp = http().get(&url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("wiki search: daemon trả về {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let arr = v
        .get("results")
        .or_else(|| v.get("hits"))
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();
    let joined: Vec<String> = arr
        .iter()
        .take(3)
        .filter_map(|h| {
            h.get("snippet")
                .or_else(|| h.get("body"))
                .or_else(|| h.get("content"))
                .and_then(|x| x.as_str())
        })
        .map(|s| s.trim().to_string())
        .collect();
    Ok(joined.join("\n"))
}

pub fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
