//! Direct client of the daemon's browser bridge.
//!
//! `senclaw-browser`'s MCP server is itself only a client of
//! `ws://127.0.0.1:{ws_port}/browser-mcp` — one fresh WebSocket per request,
//! send a `DaemonMessage`, read back an `ExtensionMessage::Response`
//! (`src/mcp/browser_server.rs:521-545`). The route is registered unauthenticated
//! at `src/gateway/websocket_gateway/gateway.rs:213`, and `DaemonMessage` is
//! `#[serde(tag = "type")]`, so we speak the same protocol and get web search
//! with NO agent and NO LLM in the retrieval loop.
//!
//! Wire shapes (verified against `src/browser/protocol.rs` + `types.rs`):
//! ```text
//! →  {"type":"Search","request_id":"…","agent_id":"…","query":"…",
//!     "engine":"google","num_results":10,"ephemeral":true}
//! ←  {"type":"Response","request_id":"…","status":"ok","data":{…}}
//! ←  {"type":"Response","request_id":"…","status":"error","message":"…"}
//! ```
//! `status`/`data`/`message` are flattened onto the frame because
//! `ExtensionMessage::Response` holds `#[serde(flatten)] result: ActionResult`
//! and `ActionResult` is `#[serde(tag = "status")]`.

use anyhow::{anyhow, bail, Result};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, Deserialize)]
pub struct SerpItem {
    #[serde(default)]
    pub position: u8,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub snippet: String,
}

/// `total_estimated` / `search_url` are part of the bridge's wire contract and
/// are kept so the struct mirrors `SearchResults` (`src/browser/types.rs:102`).
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct SerpResults {
    #[serde(default)]
    pub results: Vec<SerpItem>,
    #[serde(default)]
    pub total_estimated: u64,
    #[serde(default)]
    pub search_url: String,
}

#[derive(Clone)]
pub struct BrowserWs {
    url: String,
    agent_id: String,
}

impl BrowserWs {
    pub fn new(url: impl Into<String>, agent_id: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            agent_id: agent_id.into(),
        }
    }

    pub fn from_config() -> Self {
        Self::new(
            crate::config::browser_ws_url(),
            crate::config::browser_agent_id(),
        )
    }

    /// A clone bound to a distinct agent identity, and therefore a distinct tab.
    ///
    /// `NewTab` does NOT create a fresh tab — the extension reuses the *calling
    /// agent's* tab (`getOrCreateForAgent`, background.ts:374). So concurrent
    /// `fetch_text` calls sharing one agent id would trample each other. Lanes
    /// give each concurrent fetch its own tab.
    pub fn lane(&self, n: usize) -> Self {
        Self {
            url: self.url.clone(),
            agent_id: format!("{}#{n}", self.agent_id),
        }
    }

    fn request_id() -> String {
        format!(
            "{:x}-{:x}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
            std::process::id()
        )
    }

    /// Send one `DaemonMessage` and await its `Response`, returning `data`.
    ///
    /// `msg` must already carry `type`; `request_id` and `agent_id` are stamped
    /// here. Frames for other request ids (tab events, heartbeats, crawl
    /// progress) are skipped — the bridge multiplexes.
    pub async fn request(&self, mut msg: Value, timeout: Duration) -> Result<Value> {
        let rid = Self::request_id();
        msg["request_id"] = json!(rid);
        msg["agent_id"] = json!(self.agent_id);

        let fut = async {
            let (mut ws, _) = tokio_tungstenite::connect_async(&self.url)
                .await
                .map_err(|e| anyhow!("browser bridge connect failed: {e}"))?;
            ws.send(Message::Text(msg.to_string()))
                .await
                .map_err(|e| anyhow!("browser bridge send failed: {e}"))?;

            while let Some(frame) = ws.next().await {
                let text = match frame {
                    Ok(Message::Text(t)) => t,
                    Ok(Message::Close(_)) => bail!("browser bridge closed before responding"),
                    Ok(_) => continue,
                    Err(e) => bail!("browser bridge read failed: {e}"),
                };
                let v: Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                if v.get("type").and_then(Value::as_str) != Some("Response") {
                    continue; // tab event / heartbeat / crawl progress
                }
                if v.get("request_id").and_then(Value::as_str) != Some(rid.as_str()) {
                    continue; // another caller's response
                }
                let _ = ws.close(None).await;
                return match v.get("status").and_then(Value::as_str) {
                    Some("ok") => Ok(v.get("data").cloned().unwrap_or(Value::Null)),
                    Some("error") => Err(anyhow!(
                        "{}",
                        v.get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("browser action failed")
                    )),
                    other => Err(anyhow!("unexpected response status {other:?}")),
                };
            }
            bail!("browser bridge stream ended without a response")
        };

        tokio::time::timeout(timeout, fut)
            .await
            .map_err(|_| anyhow!("browser bridge timed out after {:?}", timeout))?
    }

    /// SERP search. `ephemeral` uses a throwaway tab so parallel searches never
    /// fight over one tab ([[browser-multiagent-concurrency]]).
    ///
    /// Only `google` is really google — the extension routes every other engine
    /// string to Bing (`senclaw-extension-chrome/src/agent/SearchEngine.ts:51`).
    pub async fn search(
        &self,
        query: &str,
        engine: &str,
        num_results: u8,
        language: Option<&str>,
        timeout: Duration,
    ) -> Result<SerpResults> {
        let mut msg = json!({
            "type": "Search",
            "query": query,
            "engine": engine,
            "num_results": num_results,
            "ephemeral": true,
        });
        if let Some(l) = language.filter(|l| !l.trim().is_empty()) {
            msg["language"] = json!(l);
        }
        let data = self.request(msg, timeout).await?;
        Ok(serde_json::from_value(data)?)
    }

    /// Open a URL in a throwaway tab and return its extracted text.
    ///
    /// `NewTab { url }` rather than `Navigate` so the run never hijacks a tab
    /// the user (or another agent) is looking at.
    pub async fn fetch_text(&self, url: &str, timeout: Duration) -> Result<String> {
        let tab = self
            .request(json!({ "type": "NewTab", "url": url }), timeout)
            .await?;
        let tab_id = tab
            .get("tab_id")
            .or_else(|| tab.get("tabId"))
            .and_then(Value::as_u64);

        let mut extract = json!({ "type": "ExtractText" });
        if let Some(id) = tab_id {
            extract["tab_id"] = json!(id);
        }
        let result = self.request(extract, timeout).await;

        if let Some(id) = tab_id {
            // Best-effort cleanup; a leaked tab must not fail the fetch.
            let _ = self
                .request(
                    json!({ "type": "CloseTab", "tab_id": id }),
                    Duration::from_secs(5),
                )
                .await;
        }

        let data = result?;
        Ok(match &data {
            Value::String(s) => s.clone(),
            _ => data
                .get("text")
                .or_else(|| data.get("content"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
    }

    /// Cheap liveness probe: `GetStatus` round-trips only if the extension is
    /// actually attached to the bridge.
    pub async fn is_connected(&self) -> Result<Value> {
        self.request(json!({ "type": "GetStatus" }), Duration::from_secs(5))
            .await
    }
}
