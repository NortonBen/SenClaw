//! Generic Space-App MCP client.
//!
//! The bridge's `mcp.call` action is a stub — it always returns
//! `{"status":"pending"}` (`src/gateway/ui_server/space.rs:1704`). But every
//! Rust Space App mounts its MCP as a plain, unauthenticated JSON-RPC endpoint
//! (`apps/social/src/api.rs:43` → `mcp_message`), so we can simply be the
//! client:
//!
//! ```text
//! POST {origin}/api/mcp/message
//! {"jsonrpc":"2.0","id":1,"method":"tools/call",
//!  "params":{"name":"social_search","arguments":{…}}}
//! → {"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"…"}]}}
//! ```
//!
//! This is the deterministic app→app path the bridge was supposed to provide,
//! and it works for every Space App (social, youtube, deepwiki, crm, …).

use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

static RPC_ID: AtomicU64 = AtomicU64::new(1);

/// A peer Space App, as reported by the daemon's registry.
#[derive(Debug, Clone)]
pub struct PeerApp {
    pub id: String,
    pub name: String,
    /// Origin of the app's own HTTP server, e.g. `http://127.0.0.1:4520`.
    pub origin: String,
    /// Path the app's MCP is mounted at, e.g. `/api/mcp/sse`.
    pub mcp_path: String,
    pub mcp_name: Option<String>,
    pub enabled: bool,
}

impl PeerApp {
    /// The JSON-RPC message endpoint.
    ///
    /// Apps advertise the SSE path in their manifest (`/api/mcp/sse`); the POST
    /// sibling that actually carries `tools/call` is `/api/mcp/message`
    /// (`apps/social/src/api.rs:44`). The SSE path also accepts POST, but the
    /// message path is the unambiguous one.
    pub fn rpc_url(&self) -> String {
        let path = if self.mcp_path.ends_with("/sse") {
            self.mcp_path.replace("/sse", "/message")
        } else {
            self.mcp_path.clone()
        };
        format!("{}{}", self.origin.trim_end_matches('/'), path)
    }
}

/// Discovery is an HTTP round-trip to the daemon, and it is on the path of
/// every source's `health()` probe — which the UI calls on every page load.
/// A short TTL keeps `/api/sources` from fanning out N identical requests
/// while still noticing an app that was just installed or stopped.
const DISCOVERY_TTL: Duration = Duration::from_secs(20);

#[derive(Clone)]
pub struct AppMcp {
    daemon_base: String,
    http: reqwest::Client,
    cache: Arc<RwLock<Option<(Instant, HashMap<String, PeerApp>)>>>,
}

#[derive(Deserialize)]
struct RpcResponse {
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
}

impl AppMcp {
    pub fn new(daemon_base: impl Into<String>) -> Self {
        Self {
            daemon_base: daemon_base.into(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            cache: Arc::new(RwLock::new(None)),
        }
    }

    pub fn from_config() -> Self {
        Self::new(crate::config::senclaw_base_url())
    }

    /// Enumerate installed Space Apps via `GET /api/space/apps`.
    ///
    /// The daemon stamps `runtime.url` into the stored manifest once it has
    /// launched an app (`space_mcp.rs:293-309`), so that is the authoritative
    /// origin; `runtime.port` is the fallback for an app started outside the
    /// daemon.
    pub async fn discover(&self) -> Result<HashMap<String, PeerApp>> {
        if let Some((at, apps)) = self.cache.read().await.as_ref() {
            if at.elapsed() < DISCOVERY_TTL {
                return Ok(apps.clone());
            }
        }
        let apps = self.discover_uncached().await?;
        *self.cache.write().await = Some((Instant::now(), apps.clone()));
        Ok(apps)
    }

    /// Force the next `discover()` to hit the daemon — call after installing,
    /// enabling or restarting an app.
    pub async fn invalidate(&self) {
        *self.cache.write().await = None;
    }

    async fn discover_uncached(&self) -> Result<HashMap<String, PeerApp>> {
        let url = format!("{}/api/space/apps", self.daemon_base.trim_end_matches('/'));
        let body: Value = self.http.get(&url).send().await?.json().await?;
        let list = body
            .get("apps")
            .and_then(Value::as_array)
            .or_else(|| body.as_array())
            .cloned()
            .unwrap_or_default();

        let mut out = HashMap::new();
        for app in list {
            let manifest = app.get("manifest").unwrap_or(&app);
            let id = manifest
                .get("id")
                .or_else(|| app.get("id"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            if id.is_empty() {
                continue;
            }
            let mcp = match manifest.get("mcp") {
                Some(m) if m.is_object() => m,
                _ => continue, // no MCP surface — not a searchable peer
            };
            let runtime = manifest.get("runtime").cloned().unwrap_or(Value::Null);
            let origin = runtime
                .get("url")
                .and_then(Value::as_str)
                .map(|s| s.trim_end_matches('/').to_string())
                .or_else(|| {
                    runtime
                        .get("port")
                        .and_then(Value::as_u64)
                        .filter(|p| *p > 0)
                        .map(|p| format!("http://127.0.0.1:{p}"))
                });
            let Some(origin) = origin else { continue };

            out.insert(
                id.clone(),
                PeerApp {
                    name: manifest
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or(&id)
                        .to_string(),
                    id,
                    origin,
                    mcp_path: mcp
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or("/mcp")
                        .to_string(),
                    mcp_name: mcp.get("name").and_then(Value::as_str).map(String::from),
                    enabled: app.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                },
            );
        }
        Ok(out)
    }

    /// Call one MCP tool and return its unwrapped payload.
    ///
    /// Rust Space Apps wrap every result as `{"content":[{"type":"text",
    /// "text":"<pretty JSON>"}]}` (`apps/social/src/mcp.rs:44`), so we unwrap
    /// the text and re-parse it as JSON when possible — callers get structured
    /// data, not a string containing JSON.
    pub async fn call(
        &self,
        rpc_url: &str,
        tool: &str,
        args: Value,
        timeout: Duration,
    ) -> Result<Value> {
        let id = RPC_ID.fetch_add(1, Ordering::Relaxed);
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": args },
        });

        let resp = self
            .http
            .post(rpc_url)
            .timeout(timeout)
            .json(&req)
            .send()
            .await
            .map_err(|e| anyhow!("{tool}: request failed: {e}"))?;
        if !resp.status().is_success() {
            bail!("{tool}: HTTP {}", resp.status());
        }
        let parsed: RpcResponse = resp
            .json()
            .await
            .map_err(|e| anyhow!("{tool}: bad JSON-RPC response: {e}"))?;
        if let Some(err) = parsed.error {
            bail!("{tool}: {err}");
        }
        let result = parsed
            .result
            .ok_or_else(|| anyhow!("{tool}: response had neither result nor error"))?;

        // MCP-level tool error.
        if result.get("isError").and_then(Value::as_bool) == Some(true) {
            bail!("{tool}: {}", unwrap_content_text(&result));
        }
        let text = unwrap_content_text(&result);
        if text.is_empty() {
            return Ok(result);
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    /// `tools/list` — used by the UI to let a user pick a tool when registering
    /// a generic MCP source.
    pub async fn list_tools(&self, rpc_url: &str, timeout: Duration) -> Result<Vec<Value>> {
        let id = RPC_ID.fetch_add(1, Ordering::Relaxed);
        let req = json!({ "jsonrpc": "2.0", "id": id, "method": "tools/list" });
        let resp: RpcResponse = self
            .http
            .post(rpc_url)
            .timeout(timeout)
            .json(&req)
            .send()
            .await?
            .json()
            .await?;
        if let Some(err) = resp.error {
            bail!("tools/list: {err}");
        }
        Ok(resp
            .result
            .and_then(|r| r.get("tools").and_then(Value::as_array).cloned())
            .unwrap_or_default())
    }
}

/// Concatenate the `text` parts of an MCP content array.
fn unwrap_content_text(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(path: &str) -> PeerApp {
        PeerApp {
            id: "social".into(),
            name: "Social".into(),
            origin: "http://127.0.0.1:4520/".into(),
            mcp_path: path.into(),
            mcp_name: Some("social-mcp".into()),
            enabled: true,
        }
    }

    #[test]
    fn sse_path_maps_to_the_message_endpoint() {
        assert_eq!(
            peer("/api/mcp/sse").rpc_url(),
            "http://127.0.0.1:4520/api/mcp/message"
        );
    }

    #[test]
    fn non_sse_path_is_used_verbatim() {
        assert_eq!(peer("/mcp").rpc_url(), "http://127.0.0.1:4520/mcp");
    }

    #[test]
    fn content_text_is_unwrapped_and_reparsed() {
        let result = json!({ "content": [{ "type": "text", "text": "{\"a\":1}" }] });
        assert_eq!(unwrap_content_text(&result), "{\"a\":1}");
    }

    #[test]
    fn plain_text_content_survives_as_a_string() {
        let result = json!({ "content": [{ "type": "text", "text": "not json" }] });
        assert_eq!(unwrap_content_text(&result), "not json");
    }
}
