//! Daemon bridge client — `POST /api/space/apps/{app_id}/bridge`.
//!
//! All LLM in this app goes through the SenClaw bridge's `llm.request`
//! (provider/keys are the daemon's, per the user's choice). Two constraints
//! carried from prior App Space work:
//!   * there is NO `temperature` on this surface — a creativity knob is inert
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

    /// One-shot LLM call. No `temperature` exists on this surface. A `finish` of
    /// `"length"` means the model was cut off — treated as failure.
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

    /// Write text into the cognitive graph (used opportunistically by AI-memory
    /// features so extracted facts are visible to the rest of SenClaw).
    pub async fn knowledge_save(
        &self,
        text: &str,
        source: Option<&str>,
        timeout: Duration,
    ) -> Result<Value> {
        let mut payload = json!({ "text": text });
        if let Some(s) = source {
            payload["source"] = json!(s);
        }
        self.action("knowledge.save", payload, timeout).await
    }
}
