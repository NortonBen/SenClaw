//! Local MLX adapter for the cognify LLM client.
//!
//! When the user picks a `local-mlx` profile as their Cognitive (or Main)
//! Model — typical example: `mlx-community/Qwen3-4B-4bit` — there's no
//! HTTP server to talk to. The model runs **in-process** via the same
//! cached `MlxNativeEngine` registry that powers the main agent's
//! `query_local_mlx`. This adapter funnels cognify completions through
//! that registry so the cognitive layer benefits from:
//!
//!   * Cached weights (one warm_up per process across both main + cognify)
//!   * No network hop, no separate keys/URLs to configure
//!   * Consistent prompt formatting (Qwen chat template) with the rest
//!     of the agent stack
//!
//! Feature-gated under `local-mlx` — without the feature, construction
//! returns an error pointing at the rebuild instruction.

use anyhow::Result;
use async_trait::async_trait;

use super::llm::LlmClient;

/// LLM client backed by the in-process MLX runtime.
///
/// We hold the canonical model id + on-disk dir on construction; the
/// actual engine handle is fetched (and warmed) lazily on the first
/// `complete()` call so building the adapter is non-blocking.
pub struct LocalMlxLlm {
    canonical_id: String,
    #[cfg(feature = "local-mlx")]
    model_dir: std::path::PathBuf,
    #[cfg(not(feature = "local-mlx"))]
    _phantom: std::marker::PhantomData<()>,
}

impl LocalMlxLlm {
    /// Resolve a user-facing model name to the canonical id + local
    /// weights directory and build an adapter. The actual MLX engine
    /// isn't touched until `complete()` runs.
    pub fn new(model_name: &str) -> Result<Self> {
        #[cfg(feature = "local-mlx")]
        {
            use crate::config::Config;
            use crate::gateway::ui_server::local_models::canonical_local_model_id;
            let cfg = Config::from_env();
            let canonical = canonical_local_model_id(model_name);
            let safe = canonical.replace('/', "__");
            let model_dir = cfg.paths.local_models_dir.join(safe);
            Ok(Self { canonical_id: canonical, model_dir })
        }

        #[cfg(not(feature = "local-mlx"))]
        {
            let _ = model_name;
            Err(anyhow::anyhow!(
                "local-mlx adapter requires the `local-mlx` cargo feature; \
                 rebuild with `cargo build --features local-mlx` (Apple Silicon only)."
            ))
        }
    }
}

#[async_trait]
impl LlmClient for LocalMlxLlm {
    async fn complete(&self, system: &str, user: &str) -> Result<String> {
        #[cfg(feature = "local-mlx")]
        {
            use crate::gateway::ui_server::local_models::{
                get_or_create_mlx_engine, MlxInferenceGuard,
            };
            use crate::local_model::LocalModelRuntime;

            let engine = get_or_create_mlx_engine(&self.canonical_id, &self.model_dir);
            let _guard = MlxInferenceGuard::new(&self.canonical_id);

            // Lazy warm-up. spawn_blocking because weight load is CPU-bound
            // and shouldn't stall the tokio runtime.
            let engine_for_warm = engine.clone();
            tokio::task::spawn_blocking(move || engine_for_warm.warm_up())
                .await
                .map_err(|e| anyhow::anyhow!("mlx warm_up join: {e}"))??;

            // Build the OpenAI-style message array `generate_stream` expects.
            // System message as a separate role; user message after it.
            // Qwen3 chat template wraps these the same way query_local_mlx does.
            let messages = vec![
                serde_json::json!({ "role": "system", "content": system }),
                serde_json::json!({ "role": "user", "content": user }),
            ];

            let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
            let engine_for_gen = engine.clone();
            let gen_handle = tokio::spawn(async move {
                engine_for_gen.generate_stream(messages, vec![], tx).await
            });

            // Drain the stream into a single string. No tools here, so we
            // don't need the qwen-tool-call splitter — just concatenate
            // raw chunks and strip any <think> blocks at the end.
            // Cap the streamed output by total chars. Without this a
            // Qwen3 turn with thinking enabled can decode 2000+ tokens
            // (mostly `<think>` blocks), blocking the engine for over a
            // minute. The cognify JSON we actually want is < 1 KB; the
            // remaining budget is just for the reasoning preamble.
            let cap = output_char_cap();
            let mut text = String::with_capacity(cap.min(8 * 1024));
            while let Some(chunk) = rx.recv().await {
                text.push_str(&chunk);
                if text.len() >= cap {
                    tracing::warn!(
                        bytes = text.len(),
                        cap,
                        "[local-mlx-cognitive] output cap hit; closing stream to stop runaway decode"
                    );
                    // Drop receiver explicitly — closing the channel makes
                    // the generator task observe a send-error and exit
                    // gracefully on its next yield.
                    drop(rx);
                    break;
                }
            }
            // Best-effort join; the generator may have already aborted from
            // the channel close above.
            let _ = gen_handle.await;

            // Qwen reasoning models emit `<think>…</think>` blocks. The
            // cognify pipeline expects JSON only, so we keep just the
            // visible portion (post-thinking).
            let (_reasoning, visible) =
                crate::local_model::thinking_parse::split_thinking_blocks(&text);
            Ok(visible)
        }

        #[cfg(not(feature = "local-mlx"))]
        {
            let _ = (system, user);
            anyhow::bail!(
                "local-mlx adapter requires the `local-mlx` cargo feature; \
                 rebuild with `cargo build --features local-mlx`."
            )
        }
    }
}

/// Resolve the output-byte cap once per call. Cheap — just reads
/// `SENCLAW_COGNITIVE_MAX_OUTPUT_CHARS` (matches the CognitiveConfig env
/// var) or falls back to 8 KB. Keeping this as a free function rather
/// than threading the config through every adapter keeps the trait
/// surface minimal — the cap is uniform across local adapters.
pub(crate) fn output_char_cap() -> usize {
    std::env::var("SENCLAW_COGNITIVE_MAX_OUTPUT_CHARS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(8 * 1024)
        .max(256)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_char_cap_has_safe_floor() {
        // Even if a user sets a comically low value, the cap stays ≥256 so
        // we don't truncate every response into garbage.
        std::env::set_var("SENCLAW_COGNITIVE_MAX_OUTPUT_CHARS", "10");
        assert_eq!(output_char_cap(), 256);
        std::env::remove_var("SENCLAW_COGNITIVE_MAX_OUTPUT_CHARS");
    }

    #[cfg(not(feature = "local-mlx"))]
    #[test]
    fn new_errors_without_feature() {
        let msg = match LocalMlxLlm::new("mlx-community/Qwen3-4B-4bit") {
            Ok(_) => panic!("expected error when feature is off"),
            Err(e) => e.to_string(),
        };
        assert!(msg.contains("local-mlx"));
    }

    #[cfg(feature = "local-mlx")]
    #[test]
    fn new_resolves_canonical_id() {
        let llm = LocalMlxLlm::new("mlx-community/Qwen3-4B-4bit").unwrap();
        // Canonical id must be non-empty — the actual mapping lives in
        // local_models; this test just guards against silent breakage.
        assert!(!llm.canonical_id.is_empty());
    }
}
