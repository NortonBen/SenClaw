//! Daemon REST client (the `space.rest` capability).
//!
//! Used for core subsystems that expose plain HTTP and therefore need no agent:
//!   * `GET  /api/wiki/search`       (`src/gateway/ui_server/wiki.rs:234`)
//!   * `POST /api/cognitive/search`  (`src/gateway/ui_server/cognitive.rs:715`)
//!   * `GET  /api/cognitive/spaces`
//!
//! Note on knowledge scoping: the *bridge* action `knowledge.search` defaults
//! `space` to the calling app's id (`space.rs:1612`), which would silently
//! confine every search to the `search` space. The REST endpoint treats
//! `space: None` as **global** (`cognitive.rs:727` only sets `node_sets` when a
//! space is given), which is what a federated search actually wants — so the
//! knowledge source goes through REST, not the bridge.

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
pub struct WikiHit {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub snippet: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CogNode {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub summary: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CogHit {
    pub node: CogNode,
    #[serde(default)]
    pub score: f32,
    #[serde(default)]
    pub path_len: usize,
}

#[derive(Clone)]
pub struct CoreRest {
    base: String,
    http: reqwest::Client,
}

impl CoreRest {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn from_config() -> Self {
        Self::new(crate::config::senclaw_base_url())
    }

    async fn get_json(&self, path: &str, timeout: Duration) -> Result<Value> {
        let url = format!("{}{path}", self.base);
        let resp = self.http.get(&url).timeout(timeout).send().await?;
        if !resp.status().is_success() {
            bail!("GET {path}: HTTP {}", resp.status());
        }
        // An old daemon serves the SPA fallback for an unknown /api route, so a
        // non-JSON body means "endpoint missing", not "server broken".
        // See [[knowledge-multi-space]].
        resp.json()
            .await
            .map_err(|e| anyhow!("GET {path}: response was not JSON ({e}) — daemon too old?"))
    }

    async fn post_json(&self, path: &str, body: Value, timeout: Duration) -> Result<Value> {
        let url = format!("{}{path}", self.base);
        let resp = self
            .http
            .post(&url)
            .timeout(timeout)
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("POST {path}: HTTP {}", resp.status());
        }
        resp.json()
            .await
            .map_err(|e| anyhow!("POST {path}: response was not JSON ({e}) — daemon too old?"))
    }

    /// Wiki full-text search.
    ///
    /// The wiki's FTS **AND**-joins prefix terms (`src/wiki/search.rs:130`),
    /// unlike memory/cognitive which OR-join — so callers must pass the narrow
    /// query variant or this silently returns nothing.
    pub async fn wiki_search(
        &self,
        query: &str,
        tags: Option<&[String]>,
        limit: usize,
        timeout: Duration,
    ) -> Result<Vec<WikiHit>> {
        let mut path = format!("/api/wiki/search?q={}&limit={limit}", urlencode(query));
        if let Some(t) = tags.filter(|t| !t.is_empty()) {
            path.push_str(&format!("&tags={}", urlencode(&t.join(","))));
        }
        let v = self.get_json(&path, timeout).await?;
        Ok(serde_json::from_value(
            v.get("results").cloned().unwrap_or(Value::Array(vec![])),
        )?)
    }

    /// Cognitive-graph search. `space: None` = across every space.
    pub async fn cognitive_search(
        &self,
        query: &str,
        mode: &str,
        limit: usize,
        hops: u8,
        space: Option<&str>,
        timeout: Duration,
    ) -> Result<Vec<CogHit>> {
        let mut body = json!({
            "query": query,
            "mode": mode,
            "limit": limit,
            "hops": hops,
        });
        if let Some(s) = space.filter(|s| !s.trim().is_empty()) {
            body["space"] = json!(s);
        }
        let v = self
            .post_json("/api/cognitive/search", body, timeout)
            .await?;
        Ok(serde_json::from_value(
            v.get("hits").cloned().unwrap_or(Value::Array(vec![])),
        )?)
    }

    pub async fn cognitive_spaces(&self, timeout: Duration) -> Result<Vec<String>> {
        let v = self.get_json("/api/cognitive/spaces", timeout).await?;
        let arr = v
            .get("spaces")
            .and_then(Value::as_array)
            .or_else(|| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(arr
            .iter()
            .filter_map(|s| {
                s.as_str()
                    .map(String::from)
                    .or_else(|| s.get("name").and_then(Value::as_str).map(String::from))
                    .or_else(|| s.get("space").and_then(Value::as_str).map(String::from))
            })
            .collect())
    }

    /// Liveness probe for the wiki subsystem.
    pub async fn wiki_stats(&self, timeout: Duration) -> Result<Value> {
        self.get_json("/api/wiki/stats", timeout).await
    }

    /// Liveness probe for the cognitive subsystem.
    pub async fn cognitive_stats(&self, timeout: Duration) -> Result<Value> {
        self.get_json("/api/cognitive/stats", timeout).await
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_handles_spaces_and_unicode() {
        assert_eq!(urlencode("lãi suất"), "l%C3%A3i+su%E1%BA%A5t");
        assert_eq!(urlencode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(urlencode("a&b=c"), "a%26b%3Dc");
    }
}
