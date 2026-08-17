//! The MLX engine behind an [`LlmProvider`].
//!
//! Two responsibilities beyond calling the engine:
//!
//! - **Load lazily.** Weights are read on the first turn, never at startup — see
//!   the module docs in `main.rs`.
//! - **Keep one model resident.** Two 4 GB checkpoints in memory at once is how
//!   a laptop starts swapping, so switching models evicts the previous one.
//!
//! MLX serialization is the engine's own concern: `MlxNativeEngine` holds a
//! process-wide lock around every load and generation, because concurrent MLX
//! work on separate threads corrupts Metal state. In the daemon that lock was
//! shared with Whisper and the TTS backends; here the app owns the entire MLX
//! surface in its own process, so nothing outside it can race.


use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use app_space_sdk::llm::{ChatRequest, Chunk, ChunkSink, LlmProvider, ModelCard};
use local_model_core::{api::EngineHost, settings, store};

use crate::engine::{runtime::LocalModelRuntime, MlxNativeEngine};

pub struct MlxProvider {
    loaded: Mutex<Option<Loaded>>,
}

struct Loaded {
    model_id: String,
    engine: Arc<MlxNativeEngine>,
    /// Last request that touched this model. What the idle sweeper measures —
    /// the daemon's old screen promised "unload after N idle seconds", and the
    /// settings file on real machines says 60.
    last_used: std::time::Instant,
}

impl MlxProvider {
    pub fn new() -> Self {
        Self {
            loaded: Mutex::new(None),
        }
    }

    /// Write the model list where the daemon can read it while this app is
    /// stopped.
    ///
    /// An empty list is not an error and not published: a fresh install has
    /// downloaded nothing, the daemon registers no provider, and the first
    /// completed download publishes for real. Publishing empty would instead
    /// clobber a good cache — which the SDK refuses anyway.
    /// Drop the resident model once it has sat unused past the settings'
    /// `idle_unload_secs` (0 = keep forever). The daemon's idle reaper already
    /// stops this *process* after its own timeout, but the two cover different
    /// gaps: proxy traffic of any kind — the settings page, a health probe —
    /// keeps the process alive, and without this sweeper the weights would sit
    /// at full residency the whole time.
    pub fn spawn_idle_sweeper(self: &Arc<Self>) {
        let me = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                let idle = settings::load(&store::models_root()).idle_unload_secs();
                if idle == 0 {
                    continue;
                }
                let mut g = me.loaded.lock().unwrap_or_else(|e| e.into_inner());
                let expired = g
                    .as_ref()
                    .is_some_and(|l| l.last_used.elapsed().as_secs() >= idle as u64);
                if expired {
                    if let Some(l) = g.take() {
                        tracing::info!(
                            "idle {}s — unloading {}",
                            l.last_used.elapsed().as_secs(),
                            l.model_id
                        );
                        // Skip-if-busy inside; a missed pass retries in 15 s.
                        l.engine.unload();
                    }
                }
            }
        });
    }

    pub fn publish(&self) -> Result<()> {
        let models = self.models();
        if models.is_empty() {
            return Ok(());
        }
        app_space_sdk::llm::publish_models(&app_dir(), &models)
    }

    fn engine_for(&self, model_id: &str) -> Result<Arc<MlxNativeEngine>> {
        {
            let mut g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(l) = g.as_mut() {
                if l.model_id == model_id {
                    l.last_used = std::time::Instant::now();
                    return Ok(Arc::clone(&l.engine));
                }
            }
        }

        let dir = store::model_dir(model_id);
        if !store::is_installed(&dir) {
            anyhow::bail!("model `{model_id}` is not installed");
        }
        let cfg = settings::load(&store::models_root());
        let e = Arc::new(MlxNativeEngine::new(
            &dir,
            model_id,
            cfg.kv_cache_bits.filter(|b| *b > 0),
        ));
        let mut g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        // Ask the outgoing engine to free its Metal buffers before dropping it.
        // Dropping alone returns them to MLX's *cache*, not the OS — switching
        // models a few times would otherwise stack multi-gigabyte residencies
        // that nothing ever reclaims. (`unload` skips if a generation is midway
        // on the old engine; that stragglers' memory returns when it finishes
        // and the process idles out.)
        if let Some(prev) = g.take() {
            prev.engine.unload();
        }
        *g = Some(Loaded {
            model_id: model_id.to_string(),
            engine: Arc::clone(&e),
            last_used: std::time::Instant::now(),
        });
        Ok(e)
    }
}

impl Default for MlxProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl LlmProvider for MlxProvider {
    fn models(&self) -> Vec<ModelCard> {
        let cfg = settings::load(&store::models_root());
        store::list_installed()
            .into_iter()
            .filter(|m| crate::engine::supports(&m.id, &m.dir))
            .map(|m| {
                ModelCard::new(
                    &m.id,
                    m.context_length.unwrap_or(cfg.max_prompt_tokens()),
                    cfg.max_new_tokens(),
                    crate::engine::has_vision(&m.dir),
                )
            })
            .collect()
    }

    async fn chat(&self, req: ChatRequest, sink: ChunkSink) -> Result<()> {
        let engine = self.engine_for(&req.model)?;

        // `warm_up` is where the gigabytes are read. On the first turn for a
        // model this is seconds to tens of seconds; it happens here rather than
        // at startup so the daemon's 30-second health gate never sees it.
        let wu = Arc::clone(&engine);
        tokio::task::spawn_blocking(move || wu.warm_up()).await??;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
        let gen = {
            let engine = Arc::clone(&engine);
            let messages = req.messages.clone();
            let tools = req.tools.clone();
            tokio::spawn(async move { engine.generate_stream(messages, tools, tx).await })
        };

        // Buffered, not forwarded token by token. A local model emits its tool
        // calls and its reasoning as *text*, in whatever dialect its chat
        // template uses, and a marker split across two tokens is only
        // recognisable once both have arrived. Streaming the raw tokens through
        // would leak `<|tool_call|>` and half-formed JSON into the visible
        // answer.
        let mut raw = String::new();
        while let Some(chunk) = rx.recv().await {
            raw.push_str(&chunk);
        }
        gen.await??;

        // Parse with the model's *own* config, loaded from its
        // `tokenizer_config.json` at warm-up. The dialect preset is a fallback
        // for the case where the engine could not surface one, which should not
        // happen after a successful warm-up.
        let (text, reasoning, tool_calls) = match engine.parser_config() {
            Ok(cfg) => crate::engine::stream_parser::parse_complete_with_config(&raw, &cfg),
            Err(e) => {
                tracing::warn!("parser_config unavailable ({e}); falling back to a dialect preset");
                let dialect = crate::engine::stream_parser::dialect_for_model_id(&req.model);
                crate::engine::stream_parser::parse_complete(&raw, dialect)
            }
        };

        if !reasoning.is_empty() {
            sink.send(Chunk::Reasoning(reasoning)).await;
        }
        if !text.is_empty() {
            sink.send(Chunk::Text(text)).await;
        }
        for tc in tool_calls {
            // The parser returns OpenAI-shaped calls; the SDK re-renders them as
            // indexed streaming deltas.
            let id = tc["id"].as_str().unwrap_or_default().to_string();
            let name = tc["function"]["name"].as_str().unwrap_or_default().to_string();
            let arguments = tc["function"]["arguments"]
                .as_str()
                .unwrap_or("{}")
                .to_string();
            if name.is_empty() {
                continue;
            }
            sink.send(Chunk::ToolCall {
                id,
                name,
                arguments,
            })
            .await;
        }

        if let Some((prompt_tokens, completion_tokens)) = engine.last_usage() {
            sink.send(Chunk::Usage {
                prompt_tokens: prompt_tokens as u64,
                completion_tokens: completion_tokens as u64,
            })
            .await;
        }

        // Same contract the daemon's own turn loop had: the idle clock restarts
        // at the end of a turn, and `release_cache_after_session` drops the
        // per-session KV (worth hundreds of MB after a long generation) while
        // keeping the weights warm.
        {
            let mut g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(l) = g.as_mut() {
                l.last_used = std::time::Instant::now();
            }
        }
        if settings::load(&store::models_root())
            .release_cache_after_session
            .unwrap_or(false)
        {
            engine.release_kv_cache();
        }
        Ok(())
    }
}

impl EngineHost for MlxProvider {
    fn engine(&self) -> &'static str {
        "mlx"
    }

    fn supports(&self, dir: &Path) -> bool {
        // The id is only a hint to the architecture detector; the config in
        // `dir` is the source of truth, and the directory name reconstructs the
        // id well enough for the hint.
        let id = dir
            .file_name()
            .map(|n| n.to_string_lossy().replacen("__", "/", 1))
            .unwrap_or_default();
        crate::engine::supports(&id, dir)
    }

    fn loaded(&self) -> Vec<String> {
        self.loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|l| vec![l.model_id.clone()])
            .unwrap_or_default()
    }

    fn load(&self, model_id: &str) {
        if let Ok(engine) = self.engine_for(model_id) {
            // Blocking Metal work off the async reactor, same as a chat turn.
            std::thread::spawn(move || {
                if let Err(e) = engine.warm_up() {
                    tracing::warn!("warm-up failed: {e:#}");
                }
            });
        }
    }

    fn unload(&self, model_id: Option<&str>) {
        let mut g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        let drop_it = match (&*g, model_id) {
            (Some(l), Some(id)) => l.model_id == id,
            (Some(_), None) => true,
            (None, _) => false,
        };
        if drop_it {
            if let Some(l) = g.as_ref() {
                // Ask the engine to free its Metal buffers first. Dropping the
                // Arc alone frees them only if nothing else holds a clone — and
                // an in-flight generation does.
                l.engine.unload();
            }
            *g = None;
        }
    }
}

/// This app's own directory, where the model cache is written.
fn app_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
