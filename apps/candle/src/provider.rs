//! The Candle engine behind an [`LlmProvider`].
//!
//! Structurally identical to the MLX app's provider, and deliberately so — the
//! two differ in which engine loads the weights, not in how a turn is served.
//! Weights load on the first turn rather than at startup (the daemon
//! health-gates a new app on 30 seconds; a checkpoint does not load in that),
//! and one model stays resident at a time.


use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use app_space_sdk::llm::{ChatRequest, Chunk, ChunkSink, LlmProvider, ModelCard};
use local_model_core::{api::EngineHost, settings, store};

use crate::engine::{runtime::LocalModelRuntime, CandleEngine};

pub struct CandleProvider {
    loaded: Mutex<Option<Loaded>>,
}

struct Loaded {
    model_id: String,
    engine: Arc<CandleEngine>,
    /// Last request that touched this model — what the idle sweeper measures.
    last_used: std::time::Instant,
}

impl CandleProvider {
    pub fn new() -> Self {
        Self {
            loaded: Mutex::new(None),
        }
    }

    /// Idle weight unload — same contract as the MLX app.
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
                        tracing::info!("idle — unloading {}", l.model_id);
                        l.engine.unload();
                    }
                }
            }
        });
    }

    /// Write the model list where the daemon reads it while this app is stopped.
    /// An empty list is not published — see the MLX app for why.
    pub fn publish(&self) -> Result<()> {
        let models = self.models();
        if models.is_empty() {
            return Ok(());
        }
        app_space_sdk::llm::publish_models(&app_dir(), &models)
    }

    fn engine_for(&self, model_id: &str) -> Result<Arc<CandleEngine>> {
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
        let e = Arc::new(CandleEngine::new(&dir, model_id));
        let mut g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        // Free the outgoing model's buffers before dropping it — see the MLX
        // provider for why a bare drop leaks residency.
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

impl Default for CandleProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl LlmProvider for CandleProvider {
    fn models(&self) -> Vec<ModelCard> {
        let cfg = settings::load(&store::models_root());
        store::list_installed()
            .into_iter()
            .filter(|m| crate::engine::supports(&m.dir))
            .map(|m| {
                // Candle's own ceilings, not the shared defaults: it decodes at
                // 7–12 tok/s where MLX does 60–100, so the 128 k window the MLX
                // app advertises would be minutes of prefill here.
                let ctx = m
                    .context_length
                    .unwrap_or(crate::engine::DEFAULT_CANDLE_MAX_PROMPT_TOKENS)
                    .min(
                        cfg.max_prompt_tokens
                            .unwrap_or(crate::engine::DEFAULT_CANDLE_MAX_PROMPT_TOKENS),
                    );
                let out = cfg
                    .max_new_tokens
                    .unwrap_or(crate::engine::DEFAULT_CANDLE_MAX_NEW_TOKENS);
                ModelCard::new(&m.id, ctx, out, crate::engine::has_vision(&m.dir))
            })
            .collect()
    }

    async fn chat(&self, req: ChatRequest, sink: ChunkSink) -> Result<()> {
        let engine = self.engine_for(&req.model)?;

        let wu = Arc::clone(&engine);
        tokio::task::spawn_blocking(move || wu.warm_up()).await??;

        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(32);
        let gen = {
            let engine = Arc::clone(&engine);
            let messages = req.messages.clone();
            let tools = req.tools.clone();
            tokio::spawn(async move { engine.generate_stream(messages, tools, tx).await })
        };

        // Buffered, not forwarded token by token: a local model emits its tool
        // calls and reasoning as *text* in its template's own dialect, and a
        // marker split across two tokens is only recognisable once both have
        // arrived. Streaming raw tokens leaks `<|tool_call|>` into the answer.
        let mut raw = String::new();
        while let Some(chunk) = rx.recv().await {
            raw.push_str(&chunk);
        }
        gen.await??;

        let dialect = crate::engine::stream_parser::dialect_for_model_id(&req.model);
        let (text, reasoning, tool_calls) = crate::engine::stream_parser::parse_complete(&raw, dialect);

        if !reasoning.is_empty() {
            sink.send(Chunk::Reasoning(reasoning)).await;
        }
        if !text.is_empty() {
            sink.send(Chunk::Text(text)).await;
        }
        for tc in tool_calls {
            let name = tc["function"]["name"].as_str().unwrap_or_default().to_string();
            if name.is_empty() {
                continue;
            }
            sink.send(Chunk::ToolCall {
                id: tc["id"].as_str().unwrap_or_default().to_string(),
                name,
                arguments: tc["function"]["arguments"]
                    .as_str()
                    .unwrap_or("{}")
                    .to_string(),
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
        let mut g = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(l) = g.as_mut() {
            l.last_used = std::time::Instant::now();
        }
        Ok(())
    }
}

impl EngineHost for CandleProvider {
    fn engine(&self) -> &'static str {
        "candle"
    }

    fn supports(&self, dir: &Path) -> bool {
        crate::engine::supports(dir)
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
                l.engine.unload();
            }
            *g = None;
        }
    }
}

fn app_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
