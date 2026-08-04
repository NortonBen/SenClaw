//! The **open API** a Space App uses to reach SenClaw services — instead of an
//! app talking to an LLM provider directly, it goes through the daemon's
//! Space-App bridge/REST. This is the single supported surface for apps.
//!
//! ```ignore
//! let sc = SpaceClient::from_env();               // reads SENCLAW_BASE_URL + app id
//! let (text, model) = sc.llm_request("You are…", "Hello", 512).await?;
//! let models = sc.list_models().await?;           // configured LLMs (incl. local-mlx)
//! sc.set_active_model("llm_123").await?;
//! ```

use anyhow::{Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

/// A client for the SenClaw daemon's Space-App open API.
#[derive(Clone, Debug)]
pub struct SpaceClient {
    /// Daemon base URL, e.g. `http://127.0.0.1:18788`.
    pub base_url: String,
    /// This app's id (used for the per-app bridge endpoint).
    pub app_id: String,
    http: reqwest::Client,
}

/// One configured LLM in the daemon.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    #[serde(rename = "modelName")]
    pub model_name: Option<String>,
    pub provider: Option<String>,
}

/// Provider-reported token usage for one `llm.request` call.
/// `input_tokens` is the total billed input (cache included); the cache
/// fields break it down when the provider reports them (Anthropic).
#[derive(Debug, Clone, Copy, Default)]
pub struct LlmUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

/// Result of [`SpaceClient::llm_request_usage`] — the richest reply shape.
#[derive(Debug, Clone)]
pub struct LlmReply {
    pub text: String,
    pub model: String,
    /// `"length"` (token cap hit), `"stop"`, or `""` when unreported.
    pub finish: String,
    /// `None` when the provider reported no usage (some local models).
    pub usage: Option<LlmUsage>,
}

impl SpaceClient {
    pub fn new(base_url: impl Into<String>, app_id: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            app_id: app_id.into(),
            http: reqwest::Client::new(),
        }
    }

    /// Build from the standard env the daemon injects into an app process:
    /// `SENCLAW_BASE_URL` (default `http://127.0.0.1:18788`) and
    /// `SENCLAW_SPACE_APP_ID` (default `"app"`).
    pub fn from_env() -> Self {
        let base =
            std::env::var("SENCLAW_BASE_URL").unwrap_or_else(|_| "http://127.0.0.1:18788".into());
        let app = std::env::var("SENCLAW_SPACE_APP_ID").unwrap_or_else(|_| "app".into());
        Self::new(base, app)
    }

    /// One-shot completion on the daemon's active LLM via the app bridge.
    /// Returns `(text, model)`. The app never sees provider keys.
    pub async fn llm_request(
        &self,
        system: &str,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<(String, String)> {
        self.llm_request_on(system, prompt, max_tokens, None).await
    }

    /// Same, but pinned to a specific LLM **profile** — a config id or its
    /// human label from Settings → Models.
    ///
    /// This is how an app runs on its own model *without* hijacking the
    /// daemon's global active model, which every other app and the agent share.
    /// A profile that does not exist is an error rather than a silent fallback:
    /// quietly answering on the wrong model is worse than saying so.
    pub async fn llm_request_on(
        &self,
        system: &str,
        prompt: &str,
        max_tokens: u32,
        profile: Option<&str>,
    ) -> Result<(String, String)> {
        self.llm_request_full(system, prompt, max_tokens, profile)
            .await
            .map(|(text, model, _finish)| (text, model))
    }

    /// Full form, returning `(text, model, finish_reason)`.
    ///
    /// `finish_reason` is `"length"` when the provider cut the reply at the
    /// token cap, `"stop"` on natural completion, `""` when unreported. Callers
    /// that expect structured output **must** check it: a reasoning model
    /// spends the same budget on its hidden trace first, so a cap that looks
    /// generous can still return JSON chopped mid-string — which is otherwise
    /// indistinguishable from the model simply answering badly.
    pub async fn llm_request_full(
        &self,
        system: &str,
        prompt: &str,
        max_tokens: u32,
        profile: Option<&str>,
    ) -> Result<(String, String, String)> {
        self.llm_request_usage(system, prompt, max_tokens, profile)
            .await
            .map(|r| (r.text, r.model, r.finish))
    }

    /// Richest form: text + model + finish + provider-reported token usage.
    ///
    /// `usage` is `None` when the provider reported none (some local models).
    /// `input_tokens` is the TOTAL billed input — cache tokens included; the
    /// cache fields break it down for providers that report them. Prefer this
    /// over local `chars/4` estimates for any per-task token bookkeeping.
    pub async fn llm_request_usage(
        &self,
        system: &str,
        prompt: &str,
        max_tokens: u32,
        profile: Option<&str>,
    ) -> Result<LlmReply> {
        let url = format!("{}/api/space/apps/{}/bridge", self.base_url, self.app_id);
        let mut payload = json!({ "system": system, "prompt": prompt, "maxTokens": max_tokens });
        if let Some(p) = profile.map(str::trim).filter(|p| !p.is_empty()) {
            payload["profile"] = json!(p);
        }
        let body = json!({ "action": "llm.request", "payload": payload });
        let v: Value = self
            .http
            .post(&url)
            .json(&body)
            .timeout(Duration::from_secs(125))
            .send()
            .await
            .map_err(|e| anyhow!("bridge llm.request failed ({url}): {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("invalid bridge response: {e}"))?;
        match v.get("status").and_then(|x| x.as_str()) {
            Some("ok") => {
                let s = |k: &str| {
                    v.get(k)
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string()
                };
                let usage = v.get("usage").filter(|u| u.is_object()).map(|u| {
                    let n = |k: &str| u.get(k).and_then(|x| x.as_u64()).unwrap_or(0);
                    LlmUsage {
                        input_tokens: n("inputTokens"),
                        output_tokens: n("outputTokens"),
                        cache_read_tokens: n("cacheReadTokens"),
                        cache_creation_tokens: n("cacheCreationTokens"),
                    }
                });
                Ok(LlmReply {
                    text: s("text"),
                    model: s("model"),
                    finish: s("finish"),
                    usage,
                })
            }
            Some("pending") => Err(anyhow!("bridge LLM not enabled in this daemon")),
            _ => Err(anyhow!(
                v.get("message")
                    .and_then(|x| x.as_str())
                    .unwrap_or("unknown LLM error")
                    .to_string()
            )),
        }
    }

    /// Report token usage for a provider call the app made DIRECTLY (own API
    /// key, not through `llm.request`) so the daemon's accounting stays
    /// complete. Fire-and-forget semantics are fine — pass `estimated = true`
    /// when the numbers are chars/4-style guesses rather than provider counts.
    pub async fn usage_report(
        &self,
        model: &str,
        provider: &str,
        input_tokens: u64,
        output_tokens: u64,
        latency_ms: u64,
        estimated: bool,
    ) -> Result<()> {
        self.bridge_action(
            "usage.report",
            json!({
                "model": model,
                "provider": provider,
                "inputTokens": input_tokens,
                "outputTokens": output_tokens,
                "latencyMs": latency_ms,
                "estimated": estimated,
            }),
        )
        .await
        .map(|_| ())
    }

    async fn bridge_action(&self, action: &str, payload: Value) -> Result<Value> {
        let url = format!("{}/api/space/apps/{}/bridge", self.base_url, self.app_id);
        let v: Value = self
            .http
            .post(&url)
            .json(&json!({ "action": action, "payload": payload }))
            .timeout(Duration::from_secs(125))
            .send()
            .await
            .map_err(|e| anyhow!("bridge {action} failed ({url}): {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("invalid bridge response: {e}"))?;
        match v.get("status").and_then(|x| x.as_str()) {
            Some("ok") => Ok(v),
            _ => Err(anyhow!(
                v.get("message")
                    .and_then(|x| x.as_str())
                    .unwrap_or("unknown bridge error")
                    .to_string()
            )),
        }
    }

    /// Save a memory into a knowledge space. `space = None` uses the app's
    /// own private space (named after the app id). Each space is an
    /// independent memory partition — recall/search scoped to one space
    /// never sees another space's items.
    pub async fn knowledge_save(
        &self,
        text: &str,
        space: Option<&str>,
        source: Option<&str>,
    ) -> Result<()> {
        self.bridge_action(
            "knowledge.save",
            json!({ "text": text, "space": space, "source": source }),
        )
        .await
        .map(|_| ())
    }

    /// Scoped search over one knowledge space. Returns raw hits as
    /// `(name, summary, score)`.
    pub async fn knowledge_search(
        &self,
        query: &str,
        space: Option<&str>,
        limit: u32,
    ) -> Result<Vec<(String, String, f64)>> {
        let v = self
            .bridge_action(
                "knowledge.search",
                json!({ "query": query, "space": space, "limit": limit }),
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

    /// Scoped recall with LLM synthesis: returns the synthesized answer
    /// (empty when the space holds nothing relevant).
    pub async fn knowledge_recall(&self, query: &str, space: Option<&str>) -> Result<String> {
        let v = self
            .bridge_action(
                "knowledge.recall",
                json!({ "query": query, "space": space }),
            )
            .await?;
        Ok(v.get("answer")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string())
    }

    /// List the daemon's configured LLMs (id + display name + provider).
    /// The active model's id is returned separately.
    pub async fn list_models(&self) -> Result<(Option<String>, Vec<ModelInfo>)> {
        let url = format!("{}/api/llm-config", self.base_url);
        let v: Value = self
            .http
            .get(&url)
            .timeout(Duration::from_secs(6))
            .send()
            .await
            .map_err(|e| anyhow!("cannot reach daemon: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("parse llm-config: {e}"))?;
        let active = v.get("activeId").and_then(|x| x.as_str()).map(String::from);
        let configs = v
            .get("configs")
            .and_then(|a| a.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|c| {
                        Some(ModelInfo {
                            id: c.get("id")?.as_str()?.to_string(),
                            model_name: c
                                .get("modelName")
                                .and_then(|x| x.as_str())
                                .map(String::from),
                            provider: c
                                .get("provider")
                                .or_else(|| c.get("adapt"))
                                .and_then(|x| x.as_str())
                                .map(String::from),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok((active, configs))
    }

    /// Set the daemon's active main model.
    pub async fn set_active_model(&self, id: &str) -> Result<()> {
        let url = format!("{}/api/llm-config/active", self.base_url);
        self.http
            .post(&url)
            .json(&json!({ "id": id }))
            .timeout(Duration::from_secs(6))
            .send()
            .await
            .map_err(|e| anyhow!("set active model failed: {e}"))?;
        Ok(())
    }
}
