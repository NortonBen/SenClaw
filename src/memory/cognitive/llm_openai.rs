//! OpenAI-compatible `LlmClient` — concrete backend for cognify triplet
//! extraction.
//!
//! Works with any provider that speaks the OpenAI `/v1/chat/completions`
//! shape (OpenAI, OpenRouter, Ollama-OpenAI, vLLM, LM Studio, llama.cpp
//! `--server`, etc.). The cognify prompt asks for JSON, so we set
//! `response_format = {"type":"json_object"}` when the model supports it.
//!
//! ## Configuration
//!
//! Reuses [`MemoryConfig`] fields to avoid adding a second auth surface:
//!   * `openai_api_key`   → Authorization header
//!   * `openai_base_url`  → endpoint root (e.g. `https://api.openai.com`)
//! And one new env var for the chat model (since `openai_model` is taken by
//! the embedding model):
//!   * `SENCLAW_COG_CHAT_MODEL`  default `gpt-4o-mini`
//!
//! Disabled-by-default: callers go through [`create_cognitive_llm`], which
//! returns a [`DisabledLlm`] when no API key is configured rather than
//! constructing a half-broken HTTP client.

use std::time::Duration;

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::llm::LlmClient;

const DEFAULT_MODEL: &str = "gpt-4o-mini";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

// =====================================================================
// Public client
// =====================================================================

pub struct OpenAiCompatLlm {
    client: Client,
    base_url: String,
    api_key: String,
    model: String,
    /// Whether the endpoint honours `response_format = json_object`. Most
    /// real providers do; off-brand local servers sometimes 400. We default
    /// to true and fall back to plain text on 4xx — see [`complete`].
    request_json_object: bool,
}

impl OpenAiCompatLlm {
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, model: impl Into<String>) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("build reqwest client")?;
        Ok(Self {
            client,
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            api_key: api_key.into(),
            model: model.into(),
            request_json_object: true,
        })
    }

    /// Override default `response_format` handling — exposed so tests and
    /// stricter local servers can disable it without env var twiddling.
    pub fn with_json_object(mut self, on: bool) -> Self {
        self.request_json_object = on;
        self
    }

    fn endpoint(&self) -> String {
        // Allow both bare host (`https://api.openai.com`) and explicit
        // `/v1` suffix. Detect by presence of `/v1` to stay forgiving.
        if self.base_url.contains("/v1") {
            format!("{}/chat/completions", self.base_url)
        } else {
            format!("{}/v1/chat/completions", self.base_url)
        }
    }
}

// =====================================================================
// Wire schemas — internal only, kept private so the public surface stays
// small. Serializing structs (rather than json!{}) lets the unit tests
// snapshot the exact body shape.
// =====================================================================

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatChoiceMessage,
}

#[derive(Debug, Deserialize)]
struct ChatChoiceMessage {
    #[serde(default)]
    content: String,
}

// =====================================================================
// Internal helpers — extracted so they're directly testable.
// =====================================================================

pub(crate) fn build_body(
    model: &str,
    system: &str,
    user: &str,
    request_json_object: bool,
) -> serde_json::Value {
    let req = ChatRequest {
        model,
        messages: vec![
            ChatMessage { role: "system", content: system },
            ChatMessage { role: "user", content: user },
        ],
        temperature: 0.1, // low — we want deterministic JSON
        response_format: if request_json_object {
            Some(ResponseFormat { kind: "json_object" })
        } else {
            None
        },
    };
    serde_json::to_value(&req).unwrap_or(serde_json::Value::Null)
}

pub(crate) fn parse_response(raw: &str) -> Result<String> {
    let parsed: ChatResponse = serde_json::from_str(raw)
        .with_context(|| format!("chat-completion JSON parse failed: {raw}"))?;
    let choice = parsed
        .choices
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("chat completion returned no choices"))?;
    Ok(choice.message.content)
}

#[async_trait]
impl LlmClient for OpenAiCompatLlm {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let url = self.endpoint();
        let body = build_body(&self.model, system, user, self.request_json_object);

        let mut req = self.client.post(&url).json(&body);
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }

        let resp = req.send().await.context("send chat request")?;
        let status = resp.status();
        let text = resp.text().await.context("read chat response body")?;

        if !status.is_success() {
            // Retry once without `response_format` if a 400 looks like the
            // server doesn't support json_object. Cheap to do; saves the
            // user a config tweak for local servers.
            if status.as_u16() == 400 && self.request_json_object {
                let body = build_body(&self.model, system, user, false);
                let mut retry = self.client.post(&url).json(&body);
                if !self.api_key.is_empty() {
                    retry = retry.bearer_auth(&self.api_key);
                }
                let resp = retry.send().await.context("retry chat request")?;
                let status = resp.status();
                let text = resp.text().await.context("read retry response body")?;
                if !status.is_success() {
                    anyhow::bail!("chat completion HTTP {status}: {text}");
                }
                return parse_response(&text);
            }
            anyhow::bail!("chat completion HTTP {status}: {text}");
        }
        parse_response(&text)
    }
}

// =====================================================================
// Factory
// =====================================================================

/// Build a cognitive LLM client from the current `Config`, or return None
/// if no LLM is configured anywhere. Resolution order:
///
/// 1. **Settings → LLM Models → Cognitive Model** (explicit user pick).
/// 2. **Settings → LLM Models → Main Model** — most installs only set
///    the Main model. Borrowing it here means the cognify pipeline works
///    out of the box without the user having to configure a second LLM.
/// 3. **Settings → LLM Models → Quick Model** — last UI fallback.
/// 4. **Env / MemoryConfig** — legacy `SENCLAW_OPENAI_*` env vars.
/// 5. **None** → cognify will soft-skip triplet extraction (chunks still
///    embed); CogAdd warns the agent in its return message.
///
/// Returns an `Arc<dyn LlmClient>` so we can pick the right adapter at
/// resolution time. Earlier this returned a concrete `OpenAiCompatLlm`,
/// which silently misbehaved when the user picked an Anthropic-provider
/// LLM as the Cognitive Model — `/v1/messages` and `/v1/chat/completions`
/// take incompatible payloads, so requests 4xx'd and cognify soft-failed
/// with `llm_skipped = true` even though the LLM *was* configured. We
/// now dispatch by [`LlmConfig::adapt`] (`"openai"` vs `"anthropic"`).
pub fn create_cognitive_llm(
    config: &crate::config::Config,
) -> Option<std::sync::Arc<dyn super::llm::LlmClient>> {
    let stored = crate::gateway::group_manager::load_llm_configs(
        &config.paths.global_config_path,
    );

    // Try each LLM-config id in priority order. First one with both an
    // API key AND a base URL wins.
    let try_ids: [Option<&str>; 3] = [
        stored.active_cognitive_id.as_deref(),
        stored.active_id.as_deref(),
        stored.active_quick_id.as_deref(),
    ];
    for id in try_ids.iter().flatten() {
        if let Some(cfg) = stored.configs.iter().find(|c| c.id == *id) {
            let adapt_lc = cfg.adapt.trim().to_lowercase();
            tracing::debug!(
                llm_id = %cfg.id,
                model = %cfg.model_name,
                adapt = %adapt_lc,
                "[cognitive] LLM resolved from Settings"
            );

            // ───── Local in-process runtimes ─────
            // These don't speak HTTP, so empty `api_key`/`base_url` is
            // expected (the UI form may leave both blank for local
            // profiles). Dispatch BEFORE the http-fields validation.
            if adapt_lc == "local-mlx" {
                match super::llm_local_mlx::LocalMlxLlm::new(&cfg.model_name) {
                    Ok(c) => return Some(std::sync::Arc::new(c)),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "[cognitive] local-mlx adapter unavailable; trying next candidate"
                        );
                        continue;
                    }
                }
            }
            if adapt_lc == "local-candle-native" {
                match super::llm_local_candle::LocalCandleLlm::new(&cfg.model_name) {
                    Ok(c) => return Some(std::sync::Arc::new(c)),
                    Err(e) => {
                        tracing::warn!(
                            error = %e,
                            "[cognitive] local-candle-native adapter unavailable; trying next candidate"
                        );
                        continue;
                    }
                }
            }

            // ───── HTTP adapters ─────
            let key = cfg.api_key.trim();
            let base = cfg.base_url.trim();
            if key.is_empty() || base.is_empty() {
                continue;
            }
            let client: Option<std::sync::Arc<dyn super::llm::LlmClient>> =
                if adapt_lc == "anthropic" || adapt_lc == "claude" {
                    super::llm_anthropic::AnthropicLlm::new(base, key, cfg.model_name.clone())
                        .ok()
                        .map(|c| std::sync::Arc::new(c) as _)
                } else {
                    OpenAiCompatLlm::new(base, key, cfg.model_name.clone())
                        .ok()
                        .map(|c| std::sync::Arc::new(c) as _)
                };
            if let Some(c) = client {
                return Some(c);
            }
        }
    }

    // Env fallback (assumes OpenAI shape — no Anthropic env vars are
    // wired today).
    let key = config.memory.openai_api_key.trim();
    if key.is_empty() {
        return None;
    }
    let base_url = if config.memory.openai_base_url.trim().is_empty() {
        "https://api.openai.com".to_owned()
    } else {
        config.memory.openai_base_url.clone()
    };
    let model = std::env::var("SENCLAW_COG_CHAT_MODEL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_owned());

    OpenAiCompatLlm::new(base_url, key.to_owned(), model)
        .ok()
        .map(|c| std::sync::Arc::new(c) as std::sync::Arc<dyn super::llm::LlmClient>)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_handles_bare_host() {
        let llm = OpenAiCompatLlm::new("https://api.openai.com", "k", "m").unwrap();
        assert_eq!(llm.endpoint(), "https://api.openai.com/v1/chat/completions");
    }

    #[test]
    fn endpoint_preserves_existing_v1() {
        let llm = OpenAiCompatLlm::new("https://example.com/v1", "k", "m").unwrap();
        assert_eq!(llm.endpoint(), "https://example.com/v1/chat/completions");
    }

    #[test]
    fn endpoint_strips_trailing_slash() {
        let llm = OpenAiCompatLlm::new("https://example.com/", "k", "m").unwrap();
        assert_eq!(llm.endpoint(), "https://example.com/v1/chat/completions");
    }

    #[test]
    fn build_body_emits_system_user_temperature_json_object() {
        let body = build_body("gpt-x", "sys-prompt", "usr-prompt", true);
        assert_eq!(body["model"], "gpt-x");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "sys-prompt");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "usr-prompt");
        let temp = body["temperature"].as_f64().expect("temperature is number");
        assert!((temp - 0.1).abs() < 1e-4, "temperature ≈ 0.1, got {temp}");
        assert_eq!(body["response_format"]["type"], "json_object");
    }

    #[test]
    fn build_body_skips_response_format_when_disabled() {
        let body = build_body("m", "s", "u", false);
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn parse_response_extracts_content() {
        let raw = r#"{"choices":[{"message":{"role":"assistant","content":"hello"}}]}"#;
        assert_eq!(parse_response(raw).unwrap(), "hello");
    }

    #[test]
    fn parse_response_handles_missing_content() {
        // Some providers return null/missing content on tool-only responses.
        let raw = r#"{"choices":[{"message":{"role":"assistant"}}]}"#;
        assert_eq!(parse_response(raw).unwrap(), "");
    }

    #[test]
    fn parse_response_errors_on_no_choices() {
        let raw = r#"{"choices":[]}"#;
        assert!(parse_response(raw).is_err());
    }

    /// Build a Config pointing at a fresh empty `global_config.json` so the
    /// new "Settings UI selection" resolution path can't accidentally
    /// satisfy the test from a developer's real saved config.
    fn cfg_with_isolated_config() -> crate::config::Config {
        let mut cfg = crate::config::Config::from_env();
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("global_config.json");
        // Leak the TempDir so the file persists for the lifetime of the test.
        std::mem::forget(tmp);
        cfg.paths.global_config_path = path;
        cfg
    }

    #[test]
    fn create_returns_none_without_api_key() {
        let mut cfg = cfg_with_isolated_config();
        cfg.memory.openai_api_key = String::new();
        assert!(create_cognitive_llm(&cfg).is_none());
    }

    #[test]
    fn create_returns_some_with_api_key() {
        let mut cfg = cfg_with_isolated_config();
        cfg.memory.openai_api_key = "sk-test-123".into();
        cfg.memory.openai_base_url = "https://example.com".into();
        assert!(create_cognitive_llm(&cfg).is_some());
    }

    #[test]
    #[cfg(feature = "local-mlx")]
    fn create_picks_local_mlx_when_stored_adapt_is_local_mlx() {
        // Without the local-mlx feature this branch returns Err from the
        // adapter ctor and continues to the next candidate. With the
        // feature enabled, construction is lazy (engine isn't touched
        // until complete()), so a config alone is enough to resolve.
        use crate::gateway::group_manager::{save_llm_config, set_active_cognitive_llm_config};
        let cfg = cfg_with_isolated_config();
        let llm_cfg = crate::gateway::group_manager::LlmConfig {
            id: "test-mlx".into(),
            label: "MLX test".into(),
            provider: "local".into(),
            // Local MLX has no http endpoint — these fields are usually
            // empty in the UI. Resolution MUST NOT fail on that.
            base_url: String::new(),
            api_key: String::new(),
            model_name: "mlx-community/Qwen3-4B-4bit".into(),
            adapt: "local-mlx".into(),
            max_tokens: 4096,
            context_length: 32_000,
            vision: None,
        };
        save_llm_config(&cfg.paths.global_config_path, &llm_cfg).unwrap();
        set_active_cognitive_llm_config(&cfg.paths.global_config_path, Some("test-mlx")).unwrap();
        assert!(
            create_cognitive_llm(&cfg).is_some(),
            "local-mlx adapt with feature on must produce a client (regardless of empty base_url/api_key)"
        );
    }

    #[test]
    #[cfg(feature = "local-candle")]
    fn create_picks_local_candle_when_stored_adapt_is_local_candle_native() {
        // Same property as the local-mlx variant: in-process runtimes
        // must resolve even with empty base_url / api_key (the LLM
        // Settings form leaves those blank for local profiles).
        use crate::gateway::group_manager::{save_llm_config, set_active_cognitive_llm_config};
        let cfg = cfg_with_isolated_config();
        let llm_cfg = crate::gateway::group_manager::LlmConfig {
            id: "test-candle".into(),
            label: "Candle test".into(),
            provider: "local".into(),
            base_url: String::new(),
            api_key: String::new(),
            model_name: "Qwen/Qwen3-4B-Instruct".into(),
            adapt: "local-candle-native".into(),
            max_tokens: 4096,
            context_length: 32_000,
            vision: None,
        };
        save_llm_config(&cfg.paths.global_config_path, &llm_cfg).unwrap();
        set_active_cognitive_llm_config(&cfg.paths.global_config_path, Some("test-candle"))
            .unwrap();
        assert!(
            create_cognitive_llm(&cfg).is_some(),
            "local-candle-native with feature on must produce a client (regardless of empty base_url/api_key)"
        );
    }

    #[test]
    fn create_picks_anthropic_adapter_when_stored_adapt_is_anthropic() {
        // Reproduces the original bug report: user picked an Anthropic
        // LLM as Cognitive Model and saw the "not configured" warning
        // because OpenAiCompatLlm couldn't talk to /v1/messages.
        // We can't make a real HTTP request, so we just verify
        // `create_cognitive_llm` doesn't return None and that the
        // resolved client is wired (the integration is exercised by
        // `endpoint_handles_bare_host` for AnthropicLlm).
        use crate::gateway::group_manager::{save_llm_config, set_active_cognitive_llm_config};
        let cfg = cfg_with_isolated_config();
        let llm_cfg = crate::gateway::group_manager::LlmConfig {
            id: "test-anthropic".into(),
            label: "Anthropic test".into(),
            provider: "anthropic".into(),
            base_url: "https://api.anthropic.com".into(),
            api_key: "sk-ant-test-key".into(),
            model_name: "claude-3-5-sonnet-20241022".into(),
            adapt: "anthropic".into(),
            max_tokens: 4096,
            context_length: 200_000,
            vision: None,
        };
        save_llm_config(&cfg.paths.global_config_path, &llm_cfg).unwrap();
        set_active_cognitive_llm_config(
            &cfg.paths.global_config_path,
            Some("test-anthropic"),
        )
        .unwrap();

        let client = create_cognitive_llm(&cfg);
        assert!(
            client.is_some(),
            "Anthropic config in Cognitive Model slot must produce a client"
        );
    }
}
