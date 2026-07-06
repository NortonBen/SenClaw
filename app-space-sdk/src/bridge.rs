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

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
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
        let base = std::env::var("SENCLAW_BASE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:18788".into());
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
        let url = format!("{}/api/space/apps/{}/bridge", self.base_url, self.app_id);
        let body = json!({
            "action": "llm.request",
            "payload": { "system": system, "prompt": prompt, "maxTokens": max_tokens },
        });
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
            Some("ok") => Ok((
                v.get("text").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                v.get("model").and_then(|x| x.as_str()).unwrap_or("").to_string(),
            )),
            Some("pending") => Err(anyhow!("bridge LLM not enabled in this daemon")),
            _ => Err(anyhow!(v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown LLM error")
                .to_string())),
        }
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
                            model_name: c.get("modelName").and_then(|x| x.as_str()).map(String::from),
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
