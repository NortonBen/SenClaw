//! Everything a rule can reach outside itself: HTTP, node state, the SenClaw
//! bridge, logging, and the live event stream.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::db::Db;
use crate::engine::types::{now_ms, ChainId, RunId};
use crate::model::LogRow;

// ---------------------------------------------------------------- event bus

/// Pushed to `/api/events` (SSE) and consumed by the canvas debug console.
#[derive(Clone, Debug, serde::Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EngineEvent {
    RunStart {
        run_id: RunId,
        chain_id: ChainId,
        node: String,
    },
    Hop {
        run_id: RunId,
        chain_id: ChainId,
        seq: u64,
        node: String,
        rule: String,
        in_port: String,
        out_port: String,
        kind: String,
        data: Value,
        error: Option<String>,
        dur_ms: i64,
    },
    RunEnd {
        run_id: RunId,
        chain_id: ChainId,
        status: String,
        hops: u64,
        error: Option<String>,
    },
    Log {
        chain_id: ChainId,
        run_id: Option<RunId>,
        level: String,
        node: Option<String>,
        message: String,
        ts: i64,
    },
    ChainStatus {
        chain_id: ChainId,
        status: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<String>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(512);
        Self { tx }
    }
    pub fn publish(&self, ev: EngineEvent) {
        if let Ok(s) = serde_json::to_string(&ev) {
            let _ = self.tx.send(s);
        }
    }
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

// -------------------------------------------------------------- state store

/// Per-node persistent state. Replaces the Redis keys the Go filters used.
pub struct StateStore {
    db: Arc<Db>,
}

impl StateStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }
    pub fn get(&self, chain_id: ChainId, node: &str, scope: &str) -> Option<Value> {
        self.db.state_get(chain_id, node, scope)
    }
    pub fn set(&self, chain_id: ChainId, node: &str, scope: &str, v: &Value) {
        self.db.state_set(chain_id, node, scope, v);
    }
    pub fn clear(&self, chain_id: ChainId, node: Option<&str>) {
        let _ = self.db.state_clear(chain_id, node);
    }
}

// ------------------------------------------------------------- result store

/// In-memory slot for a run's *return value*, set by a `respond` node and read
/// by a synchronous caller (`start_run_wait` → the `rule_call` MCP tool).
///
/// Kept out of SQLite on purpose: a result lives only for the few seconds
/// between a `respond` firing and the caller collecting it, so persistence
/// would be pure overhead. If nobody collects (fire-and-forget run that happens
/// to hit a `respond`), the entry is reaped by the run reaper via `discard`.
#[derive(Default)]
pub struct ResultStore {
    inner: std::sync::Mutex<std::collections::HashMap<RunId, Value>>,
}

impl ResultStore {
    pub fn new() -> Self {
        Self::default()
    }
    /// Record the value a `respond` node produced. Last write wins — a flow with
    /// two `respond` nodes on one run keeps whichever fired last.
    pub fn set(&self, run_id: RunId, v: Value) {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(run_id, v);
    }
    /// Take the value out (the caller consumes it exactly once).
    pub fn take(&self, run_id: RunId) -> Option<Value> {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&run_id)
    }
    /// Drop a never-collected result so the map can't grow without bound.
    pub fn discard(&self, run_id: RunId) {
        self.inner
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&run_id);
    }
}

// -------------------------------------------------------------------- logs

#[derive(Clone)]
pub struct LogSink {
    db: Arc<Db>,
    bus: EventBus,
}

impl LogSink {
    pub fn new(db: Arc<Db>, bus: EventBus) -> Self {
        Self { db, bus }
    }
    pub fn write(
        &self,
        chain_id: ChainId,
        run_id: Option<RunId>,
        level: &str,
        node: Option<&str>,
        message: String,
    ) {
        let row = LogRow {
            id: 0,
            chain_id,
            run_id: run_id.map(|r| r as i64),
            level: level.to_string(),
            node: node.map(|s| s.to_string()),
            message: message.clone(),
            ts: now_ms(),
        };
        self.db.insert_log(&row);
        self.bus.publish(EngineEvent::Log {
            chain_id,
            run_id,
            level: level.to_string(),
            node: node.map(|s| s.to_string()),
            message,
            ts: row.ts,
        });
    }
}

// ------------------------------------------------------------------- bridge

/// Calls into the SenClaw daemon.
///
/// Hand-rolled rather than via `app-space-sdk` because we need `profile`,
/// `tools` and `model`, which the SDK helpers do not expose.
pub struct Bridge {
    http: reqwest::Client,
    base: String,
    app_id: String,
}

#[derive(Debug)]
pub struct LlmReply {
    pub text: String,
    pub model: String,
    pub finish: String,
}

impl Bridge {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            base: crate::config::senclaw_base_url(),
            app_id: crate::config::app_id(),
        }
    }

    async fn call(&self, action: &str, payload: Value) -> Result<Value, String> {
        let url = format!("{}/api/space/apps/{}/bridge", self.base, self.app_id);
        let resp = self
            .http
            .post(&url)
            .json(&json!({ "action": action, "payload": payload }))
            .timeout(Duration::from_secs(125))
            .send()
            .await
            .map_err(|e| format!("bridge {action}: {e}"))?;
        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .map_err(|e| format!("bridge {action}: phản hồi không phải JSON: {e}"))?;
        if !status.is_success() {
            return Err(format!("bridge {action}: HTTP {status} {body}"));
        }
        match body.get("status").and_then(|s| s.as_str()) {
            Some("ok") => Ok(body),
            Some("pending") => Err(format!(
                "bridge {action} chưa được bật: {}",
                body.get("message").and_then(|m| m.as_str()).unwrap_or("")
            )),
            _ => Err(format!(
                "bridge {action} lỗi: {}",
                body.get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or(&body.to_string())
            )),
        }
    }

    /// One system + one user turn. There is no `temperature` knob — the daemon
    /// hard-codes 0.2, so exposing one on the node would be silently inert.
    pub async fn llm_request(
        &self,
        system: &str,
        prompt: &str,
        max_tokens: u32,
        profile: Option<&str>,
    ) -> Result<LlmReply, String> {
        let mut payload = json!({
            "prompt": prompt,
            "system": system,
            "maxTokens": max_tokens,
        });
        if let Some(p) = profile {
            payload["profile"] = json!(p);
        }
        let body = self.call("llm.request", payload).await?;
        let text = body
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string();
        let finish = body
            .get("finish")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string();
        // A truncated answer is a failure, not a result: downstream nodes would
        // happily parse half a JSON document.
        if finish == "length" {
            return Err(
                "LLM cắt ngang vì chạm trần token (finish=length). Giảm dữ liệu vào hoặc tăng maxTokens."
                    .to_string(),
            );
        }
        Ok(LlmReply {
            text,
            model: body
                .get("model")
                .and_then(|t| t.as_str())
                .unwrap_or_default()
                .to_string(),
            finish,
        })
    }

    /// Full agent turn: persona, tool allowlist, workspace.
    pub async fn agent_run(
        &self,
        prompt: &str,
        system: Option<&str>,
        tools: Option<Vec<String>>,
        model: Option<&str>,
        timeout_seconds: u64,
    ) -> Result<String, String> {
        let mut payload = json!({
            "prompt": prompt,
            "timeoutSeconds": timeout_seconds.clamp(10, 1800),
        });
        if let Some(s) = system {
            payload["system"] = json!(s);
        }
        if let Some(t) = tools {
            payload["tools"] = json!(t);
        }
        if let Some(m) = model {
            payload["model"] = json!(m);
        }
        let body = self.call("agent.run", payload).await?;
        Ok(body
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
            .to_string())
    }

    pub async fn knowledge_save(
        &self,
        text: &str,
        space: Option<&str>,
        tags: Vec<String>,
        source: Option<&str>,
    ) -> Result<Value, String> {
        let mut payload = json!({ "text": text, "tags": tags });
        if let Some(s) = space {
            payload["space"] = json!(s);
        }
        if let Some(s) = source {
            payload["source"] = json!(s);
        }
        self.call("knowledge.save", payload).await
    }

    pub async fn knowledge_query(
        &self,
        action: &str, // "knowledge.search" | "knowledge.recall"
        query: &str,
        space: Option<&str>,
        limit: u32,
    ) -> Result<Value, String> {
        let mut payload = json!({ "query": query, "limit": limit.clamp(1, 30) });
        if let Some(s) = space {
            payload["space"] = json!(s);
        }
        self.call(action, payload).await
    }

    /// App→app MCP. `mcp.call` on the bridge is still a stub, so this posts
    /// straight at the other app's JSON-RPC endpoint.
    pub async fn app_mcp_call(
        &self,
        app_id: &str,
        tool: &str,
        args: Value,
    ) -> Result<Value, String> {
        let origin = self.app_origin(app_id).await?;
        let url = format!("{origin}/api/mcp/message");
        let body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": tool, "arguments": args }
        });
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| format!("gọi MCP {app_id}.{tool}: {e}"))?;
        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("gọi MCP {app_id}.{tool}: phản hồi không phải JSON: {e}"))?;
        if let Some(err) = v.get("error") {
            return Err(format!("MCP {app_id}.{tool} lỗi: {err}"));
        }
        let result = v.get("result").cloned().unwrap_or(Value::Null);
        if result
            .get("isError")
            .and_then(|b| b.as_bool())
            .unwrap_or(false)
        {
            return Err(format!(
                "MCP {app_id}.{tool} lỗi: {}",
                mcp_text(&result).unwrap_or_default()
            ));
        }
        // Unwrap the `content[0].text` envelope and re-parse if it is JSON.
        if let Some(text) = mcp_text(&result) {
            if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                return Ok(parsed);
            }
            return Ok(json!({ "text": text }));
        }
        Ok(result)
    }

    async fn app_origin(&self, app_id: &str) -> Result<String, String> {
        let url = format!("{}/api/space/apps", self.base);
        let resp = self
            .http
            .get(&url)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| format!("không đọc được danh sách Space App: {e}"))?;
        let v: Value = resp
            .json()
            .await
            .map_err(|e| format!("danh sách Space App không phải JSON: {e}"))?;
        let apps = v
            .get("apps")
            .and_then(|a| a.as_array())
            .cloned()
            .or_else(|| v.as_array().cloned())
            .unwrap_or_default();
        for app in apps {
            let manifest = app.get("manifest").unwrap_or(&app);
            let id = manifest
                .get("id")
                .and_then(|i| i.as_str())
                .or_else(|| app.get("id").and_then(|i| i.as_str()));
            if id != Some(app_id) {
                continue;
            }
            let runtime = manifest.get("runtime").cloned().unwrap_or(Value::Null);
            if let Some(u) = runtime.get("url").and_then(|u| u.as_str()) {
                return Ok(u.trim_end_matches('/').to_string());
            }
            if let Some(p) = runtime.get("port").and_then(|p| p.as_u64()) {
                return Ok(format!("http://127.0.0.1:{p}"));
            }
        }
        Err(format!("không tìm thấy Space App `{app_id}` đang chạy"))
    }
}

fn mcp_text(result: &Value) -> Option<String> {
    result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
}

// ------------------------------------------------------------------ bundle

pub struct Services {
    pub http: reqwest::Client,
    pub state: StateStore,
    pub results: ResultStore,
    pub bridge: Bridge,
    pub log: LogSink,
    pub bus: EventBus,
    pub db: Arc<Db>,
}

impl Services {
    pub fn new(db: Arc<Db>, bus: EventBus) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self {
            state: StateStore::new(db.clone()),
            results: ResultStore::new(),
            bridge: Bridge::new(http.clone()),
            log: LogSink::new(db.clone(), bus.clone()),
            http,
            bus,
            db,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_serializes_with_a_type_tag() {
        let ev = EngineEvent::RunStart {
            run_id: 5,
            chain_id: 1,
            node: "a".into(),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"type\":\"runStart\""), "{s}");
        assert!(s.contains("\"runId\":5"), "{s}");
    }

    #[test]
    fn mcp_text_unwraps_the_content_envelope() {
        let v = json!({ "content": [{ "type": "text", "text": "hi" }] });
        assert_eq!(mcp_text(&v).as_deref(), Some("hi"));
        assert_eq!(mcp_text(&json!({})), None);
    }

    #[test]
    fn log_sink_writes_to_db_and_bus() {
        let db = Arc::new(Db::open(":memory:").unwrap());
        db.create_chain(1, "c", "").unwrap();
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let sink = LogSink::new(db.clone(), bus);
        sink.write(1, Some(9), "error", Some("n1"), "hỏng".into());
        let logs = db.list_logs(1, 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].message, "hỏng");
        let ev = rx.try_recv().unwrap();
        assert!(ev.contains("\"type\":\"log\""), "{ev}");
    }
}
