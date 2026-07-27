//! Daemon bridge client — `POST /api/space/apps/{app_id}/bridge`.
//!
//! Used for the two things only the daemon can do for us:
//!   * `llm.request` — one-shot LLM (planning, claim extraction, synthesis).
//!   * `agent.run`   — a full tool-enabled agent, the ONLY way to reach an MCP
//!     tool that has no HTTP surface. In P0 that is exactly one source: file
//!     memory (`memory_search` is MCP-only — there is no `/api/memory/*`).
//!
//! We hand-roll the POST rather than use `app_space_sdk::SpaceClient` because
//! its `bridge_action` is private and its helpers cover only `llm.request`
//! without the payload fields we need (`profile`, `tools`, `model`) — the same
//! reason `apps/crm/src/senclaw.rs:16` gives.
//!
//! Two constraints carried from prior work:
//!   * there is NO `temperature` — a creativity knob is silently inert
//!     ([[space-app-llm-bridge-no-temperature]]);
//!   * output is capped in practice, and `finish == "length"` must be treated
//!     as an error, not a result ([[space-app-llm-bridge-output-ceiling]]).

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct Bridge {
    url: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct LlmReply {
    pub text: String,
    pub model: String,
    pub finish: String,
}

impl Bridge {
    pub fn new(base: &str, app_id: &str) -> Self {
        Self {
            url: format!(
                "{}/api/space/apps/{app_id}/bridge",
                base.trim_end_matches('/')
            ),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(600))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn from_config() -> Self {
        Self::new(&crate::config::senclaw_base_url(), &crate::config::app_id())
    }

    async fn action(&self, action: &str, payload: Value, timeout: Duration) -> Result<Value> {
        let resp = self
            .http
            .post(&self.url)
            .timeout(timeout)
            .json(&json!({ "action": action, "payload": payload }))
            .send()
            .await
            .map_err(|e| anyhow!("bridge {action}: {e}"))?;
        if !resp.status().is_success() {
            bail!("bridge {action}: HTTP {}", resp.status());
        }
        let v: Value = resp
            .json()
            .await
            .map_err(|e| anyhow!("bridge {action}: bad JSON: {e}"))?;
        match v.get("status").and_then(Value::as_str) {
            Some("ok") => Ok(v),
            // "pending" means the daemon knows the action but has it disabled
            // (or, for `mcp.call`, never implemented it).
            Some("pending") => bail!(
                "bridge {action} unavailable: {}",
                v.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("not enabled in this daemon")
            ),
            _ => bail!(
                "bridge {action} failed: {}",
                v.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or(&v.to_string())
            ),
        }
    }

    /// One-shot LLM call. No `temperature` exists on this surface.
    ///
    /// A `finish` of `"length"` means the model was cut off mid-answer; callers
    /// must treat that as a failure rather than shipping a truncated result.
    pub async fn llm(
        &self,
        system: &str,
        prompt: &str,
        max_tokens: u32,
        timeout: Duration,
    ) -> Result<LlmReply> {
        let v = self
            .action(
                "llm.request",
                json!({ "system": system, "prompt": prompt, "maxTokens": max_tokens }),
                timeout,
            )
            .await?;
        let finish = v
            .get("finish")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if finish == "length" {
            bail!("llm.request hit the output ceiling (finish=length) — shrink the input");
        }
        Ok(LlmReply {
            text: v
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            model: v
                .get("model")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            finish,
        })
    }

    /// Run a full tool-enabled agent with an explicit tool allowlist.
    ///
    /// The fallback transport — slow, non-deterministic, token-hungry. Every
    /// call is recorded in `run_sources` so its cost stays visible. Use only
    /// where no HTTP surface exists.
    pub async fn agent_run(
        &self,
        system: &str,
        prompt: &str,
        tools: &[String],
        timeout: Duration,
    ) -> Result<String> {
        let secs = timeout.as_secs().clamp(10, 1800);
        let mut payload = json!({
            "system": system,
            "prompt": prompt,
            "timeoutSeconds": secs,
        });
        if !tools.is_empty() {
            payload["tools"] = json!(tools);
        }
        let v = self
            .action("agent.run", payload, timeout + Duration::from_secs(30))
            .await?;
        Ok(v.get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string())
    }

    /// Write into the cognitive graph — used by the corpus source (P1) to make
    /// uploaded documents visible to the rest of SenClaw, not just to this app.
    pub async fn knowledge_save(
        &self,
        text: &str,
        space: Option<&str>,
        source: Option<&str>,
        timeout: Duration,
    ) -> Result<Value> {
        let mut payload = json!({ "text": text });
        if let Some(s) = space {
            payload["space"] = json!(s);
        }
        if let Some(s) = source {
            payload["source"] = json!(s);
        }
        self.action("knowledge.save", payload, timeout).await
    }
}
