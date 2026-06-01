//! Local Candle-native adapter for the cognify LLM client.
//!
//! Mirrors [`super::llm_local_mlx::LocalMlxLlm`] but targets the cross-
//! platform Candle backend (`adapt = "local-candle-native"`). Used when
//! the user is on x86_64 Linux/Windows or doesn't have MLX, and selects
//! a local model (e.g. `Qwen/Qwen3-4B-Instruct` running on Candle CPU
//! or Candle/Metal).
//!
//! Same in-process semantics: shares the cached `CandleEngine` registry
//! with the main agent's `query_local_candle_native`, so weights load
//! once across the process and cognify/main agent both benefit from
//! warm cache. Feature-gated under `local-candle`.

use anyhow::Result;
use async_trait::async_trait;

use super::llm::LlmClient;

pub struct LocalCandleLlm {
    canonical_id: String,
    #[cfg(feature = "local-candle")]
    model_dir: std::path::PathBuf,
    #[cfg(not(feature = "local-candle"))]
    _phantom: std::marker::PhantomData<()>,
}

impl LocalCandleLlm {
    pub fn new(model_name: &str) -> Result<Self> {
        #[cfg(feature = "local-candle")]
        {
            use crate::config::Config;
            use crate::gateway::ui_server::local_models::canonical_local_model_id;
            let cfg = Config::from_env();
            let canonical = canonical_local_model_id(model_name);
            let safe = canonical.replace('/', "__");
            let model_dir = cfg.paths.local_models_dir.join(safe);
            Ok(Self {
                canonical_id: canonical,
                model_dir,
            })
        }
        #[cfg(not(feature = "local-candle"))]
        {
            let _ = model_name;
            Err(anyhow::anyhow!(
                "local-candle-native adapter requires the `local-candle` cargo feature; \
                 rebuild with `cargo build --features local-candle` \
                 (or `local-candle-metal` for Apple Silicon Metal acceleration)."
            ))
        }
    }
}

#[async_trait]
impl LlmClient for LocalCandleLlm {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        #[cfg(feature = "local-candle")]
        {
            use crate::gateway::ui_server::local_models::{
                get_or_create_loaded_engine, CandleInferenceGuard,
            };
            use crate::local_model::LocalModelRuntime;

            let engine = get_or_create_loaded_engine(&self.canonical_id, &self.model_dir);
            let _guard = CandleInferenceGuard::new(&self.canonical_id);

            let messages = vec![
                serde_json::json!({ "role": "system", "content": system }),
                serde_json::json!({ "role": "user", "content": user }),
            ];

            let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
            let engine_for_gen = engine.clone();
            let gen_handle =
                tokio::spawn(
                    async move { engine_for_gen.generate_stream(messages, vec![], tx).await },
                );

            // Same output cap as the MLX adapter — see
            // `llm_local_mlx::output_char_cap` for rationale.
            let cap = super::llm_local_mlx::output_char_cap();
            let mut text = String::with_capacity(cap.min(8 * 1024));
            while let Some(chunk) = rx.recv().await {
                text.push_str(&chunk);
                if text.len() >= cap {
                    tracing::warn!(
                        bytes = text.len(),
                        cap,
                        "[local-candle-cognitive] output cap hit; closing stream"
                    );
                    drop(rx);
                    break;
                }
            }
            let _ = gen_handle.await;

            // Strip <think>…</think> reasoning blocks — Qwen3 family emits
            // them; cognify wants JSON only.
            let (_reasoning, visible) =
                crate::local_model::thinking_parse::split_thinking_blocks(&text);
            Ok(visible)
        }

        #[cfg(not(feature = "local-candle"))]
        {
            let _ = (system, user);
            anyhow::bail!("local-candle-native adapter requires the `local-candle` cargo feature.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(feature = "local-candle"))]
    #[test]
    fn new_errors_without_feature() {
        let msg = match LocalCandleLlm::new("Qwen/Qwen3-4B-Instruct") {
            Ok(_) => panic!("expected error when feature is off"),
            Err(e) => e.to_string(),
        };
        assert!(msg.contains("local-candle"));
    }

    #[cfg(feature = "local-candle")]
    #[test]
    fn new_resolves_canonical_id() {
        let llm = LocalCandleLlm::new("Qwen/Qwen3-4B-Instruct").unwrap();
        assert!(!llm.canonical_id.is_empty());
    }
}
