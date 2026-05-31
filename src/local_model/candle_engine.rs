/// Native Candle inference engine.
///
/// Cross-platform LLM inference using [`candle_core`] + [`candle_nn`].
/// Supports CPU and Apple Silicon Metal (feature `local-candle-metal`).
///
/// # Supported architectures
/// | `model_type`        | Arch class      | Notes                                  |
/// |---------------------|-----------------|----------------------------------------|
/// | `qwen3`, `qwen2`    | Qwen3           | Custom impl, GQA + QK-norm             |
/// | `gemma3`, `gemma4`  | Gemma3          | candle-transformers, sliding-win attn  |
/// | `mamba`             | Mamba1          | candle-transformers SSM                |
/// | `mamba2`            | Mamba2          | Custom impl, SSD recurrent             |
///
/// # Tool calling
/// Tools are injected via the Jinja2 chat template (passed as `Vec<Value>` in
/// OpenAI format). The model outputs `<tool_call>JSON</tool_call>` tags which
/// are parsed by `stream_parser::parse_complete` in `local_model/stream_parser.rs`.
///
/// # Vision
/// Text-only models do not process images.  Any `image_url` content parts are
/// replaced with a text placeholder before the chat template runs.
///
/// # KV / SSM cache
/// - **Qwen3**: external `Vec<KvCache>` allocated fresh each generation.
/// - **Gemma3**: internal KV cache inside the model; `clear_kv_cache()` is
///   called at the start of each generation.
/// - **Mamba1/2**: per-generation `State`/`Mamba2State` allocated fresh.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use serde_json::Value;
use tokio::sync::mpsc;

use super::{
    candle_models::{
        cache::KvCache,
        mamba2::{Mamba2Config, Mamba2Model, Mamba2State},
        qwen3::{Qwen3Config, Qwen3Model},
    },
    tokenizer_utils::tokenizer::{load_model_chat_template_from_file, Tokenizer},
    runtime::{LocalModelRuntime, RuntimeEndpoint, RuntimeHealth, RuntimeStatus},
};

// Bring candle-transformers model types into scope
use candle_transformers::models::gemma3 as ct_gemma3;
use candle_transformers::models::mamba as ct_mamba;

// ---------------------------------------------------------------------------
// Architecture enum
// ---------------------------------------------------------------------------

/// Detected model architecture (from `model_type` in `config.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelArch {
    Qwen3,
    Gemma3,
    Mamba1,
    Mamba2,
}

impl ModelArch {
    fn from_model_type(s: &str) -> anyhow::Result<Self> {
        match s {
            "qwen3" | "qwen2" => Ok(Self::Qwen3),
            "gemma3" | "gemma4" => Ok(Self::Gemma3),
            "mamba" => Ok(Self::Mamba1),
            "mamba2" => Ok(Self::Mamba2),
            other => anyhow::bail!(
                "unsupported model_type `{other}` — supported: \
                 qwen3/qwen2, gemma3/gemma4, mamba, mamba2"
            ),
        }
    }

    /// EOS/stop token IDs for this architecture.
    fn stop_tokens(self) -> HashSet<u32> {
        match self {
            // Qwen3: <|im_end|>=151645  <|endoftext|>=151643
            Self::Qwen3 => [151645u32, 151643].into(),
            // Gemma3: <eos>=1  <end_of_turn>=107
            Self::Gemma3 => [1u32, 107].into(),
            // Mamba: EOS=0 (NeoX tokenizer)
            Self::Mamba1 | Self::Mamba2 => [0u32].into(),
        }
    }

    pub fn supports_vision(self) -> bool {
        match self {
            Self::Qwen3 => false,  // Qwen3-VL is a separate arch
            Self::Gemma3 => false, // text-only; PaliGemma handles vision
            Self::Mamba1 | Self::Mamba2 => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Loaded model variants
// ---------------------------------------------------------------------------

/// Runtime-polymorphic loaded model.  Each variant owns the model weights and,
/// where applicable, the generation state or KV cache.
enum LocalModel {
    Qwen3(Qwen3Model),
    Gemma3(ct_gemma3::Model),
    Mamba1 {
        model: ct_mamba::Model,
        cfg: ct_mamba::Config,
    },
    Mamba2(Mamba2Model),
}

// ---------------------------------------------------------------------------
// Loaded state (held inside the engine mutex)
// ---------------------------------------------------------------------------

struct Loaded {
    model: LocalModel,
    tokenizer: Tokenizer,
    chat_template: String,
    model_context_length: Option<u32>,
    arch: ModelArch,
    device: Device,
    dtype: DType,
}

// ---------------------------------------------------------------------------
// CandleEngine — public type
// ---------------------------------------------------------------------------

/// Cross-platform Candle inference engine.
///
/// Implements [`LocalModelRuntime`]; weights are loaded lazily on the first
/// `generate_stream` call and cached in memory until `stop()` is called.
pub struct CandleEngine {
    model_dir: PathBuf,
    model_id: String,
    loaded: Arc<Mutex<Option<Loaded>>>,
    status: Mutex<RuntimeStatus>,
}

impl CandleEngine {
    pub fn new(model_dir: &Path, model_id: &str) -> Self {
        Self {
            model_dir: model_dir.to_path_buf(),
            model_id: model_id.to_owned(),
            loaded: Arc::new(Mutex::new(None)),
            status: Mutex::new(RuntimeStatus::NotInstalled),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    pub fn warm_up(&self) -> anyhow::Result<()> {
        let mut guard = self
            .loaded
            .lock()
            .map_err(|_| anyhow::anyhow!("mutex poisoned"))?;
        if guard.is_none() {
            *guard = Some(load_state(&self.model_dir, &self.model_id)?);
            self.set_status(RuntimeStatus::Ready);
        }
        Ok(())
    }

    pub fn unload(&self) {
        if let Ok(mut g) = self.loaded.lock() {
            *g = None;
        }
        self.set_status(RuntimeStatus::Stopped);
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }
    pub fn model_id(&self) -> &str {
        &self.model_id
    }
    pub fn model_context_length(&self) -> Option<u32> {
        let guard = self.loaded.lock().ok()?;
        guard.as_ref().and_then(|s| s.model_context_length)
    }

    fn set_status(&self, s: RuntimeStatus) {
        if let Ok(mut g) = self.status.lock() {
            *g = s;
        }
    }
}

// ---------------------------------------------------------------------------
// LocalModelRuntime impl
// ---------------------------------------------------------------------------

#[async_trait]
impl LocalModelRuntime for CandleEngine {
    async fn ensure_installed(&self) -> anyhow::Result<()> {
        if !self.model_dir.exists() {
            anyhow::bail!(
                "model directory not found: {} — download weights first \
                 (e.g. `huggingface-cli download {}`)",
                self.model_dir.display(),
                self.model_id
            );
        }
        for required in ["config.json", "tokenizer.json"] {
            let p = self.model_dir.join(required);
            if !p.exists() {
                anyhow::bail!("missing {} in {}", required, self.model_dir.display());
            }
        }
        Ok(())
    }

    async fn start(&self, _model: &str) -> anyhow::Result<RuntimeEndpoint> {
        self.ensure_installed().await?;
        self.set_status(RuntimeStatus::Ready);
        Ok(RuntimeEndpoint {
            base_url: None,
            model_name: self.model_id.clone(),
            adapt: "local-candle-native".to_owned(),
            api_key: None,
        })
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.unload();
        Ok(())
    }

    async fn health(&self) -> anyhow::Result<RuntimeHealth> {
        let status = self
            .status
            .lock()
            .map(|g| *g)
            .unwrap_or(RuntimeStatus::Error);
        Ok(RuntimeHealth {
            status,
            message: None,
        })
    }

    fn supports_native_stream(&self) -> bool {
        true
    }

    async fn generate_stream(
        &self,
        messages: Vec<Value>,
        tools: Vec<Value>,
        tx: mpsc::Sender<String>,
    ) -> anyhow::Result<()> {
        let loaded = Arc::clone(&self.loaded);
        let model_dir = self.model_dir.clone();
        let model_id = self.model_id.clone();

        let join = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            generate_with_cache(&loaded, &model_dir, &model_id, &messages, &tools, tx)
        });

        join.await??;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Weight loading
// ---------------------------------------------------------------------------

fn load_state(model_dir: &Path, model_id: &str) -> anyhow::Result<Loaded> {
    use super::models::read_model_context_length_from_dir;

    tracing::info!("[local-candle] loading {model_id} from {}", model_dir.display());

    let config_str = std::fs::read_to_string(model_dir.join("config.json"))
        .map_err(|e| anyhow::anyhow!("config.json read failed: {e}"))?;

    let arch = detect_arch(&config_str)?;
    let model_context_length = read_model_context_length_from_dir(model_dir);
    let device = select_device()?;

    // Accelerate BLAS (cblas_sgemm) requires F32; BF16 CPU matmul is not
    // dispatched to Accelerate and falls back to a slow scalar loop.
    // Metal: BF16 saves memory bandwidth but the GEMM kernel is inefficient
    // for M=1 decode (see select_device comment).
    let dtype = match &device {
        Device::Cpu => DType::F32,
        _ => DType::BF16,
    };

    // Debug builds run ~10–15× slower than release; warn explicitly so users
    // know to `cargo run --release --features local-candle` for real use.
    #[cfg(debug_assertions)]
    tracing::warn!(
        "[local-candle] ⚠ DEBUG BUILD — inference is ~10–15× slower than release. \
         Run `cargo run --release --features local-candle` for production use."
    );

    tracing::info!("[local-candle] arch={arch:?} device={device:?}");

    let tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
        .map_err(|e| anyhow::anyhow!("tokenizer load failed: {e:?}"))?;

    let chat_template = load_chat_template(model_dir, model_id, arch)?;

    let weight_files = find_weight_files(model_dir)?;
    tracing::info!("[local-candle] weight shards: {}", weight_files.len());

    let t0 = std::time::Instant::now();
    // Safety: read-only memory-mapped weight files.
    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&weight_files, dtype, &device)
            .map_err(|e| anyhow::anyhow!("safetensors load failed: {e:?}"))?
    };

    let model = match arch {
        ModelArch::Qwen3 => {
            let cfg: Qwen3Config = serde_json::from_str(&config_str)
                .map_err(|e| anyhow::anyhow!("qwen3 config parse failed: {e}"))?;
            let m = Qwen3Model::from_vb(&cfg, vb)
                .map_err(|e| anyhow::anyhow!("qwen3 model init failed: {e:?}"))?;
            tracing::info!(
                "[local-candle] ready in {:.1}s ({} layers, ctx={:?})",
                t0.elapsed().as_secs_f32(),
                m.n_layers(),
                model_context_length,
            );
            LocalModel::Qwen3(m)
        }
        ModelArch::Gemma3 => {
            let cfg: ct_gemma3::Config = serde_json::from_str(&config_str)
                .map_err(|e| anyhow::anyhow!("gemma3 config parse failed: {e}"))?;
            let m = ct_gemma3::Model::new(false, &cfg, vb)
                .map_err(|e| anyhow::anyhow!("gemma3 model init failed: {e:?}"))?;
            tracing::info!(
                "[local-candle] ready in {:.1}s ({} layers, ctx={:?})",
                t0.elapsed().as_secs_f32(),
                cfg.num_hidden_layers,
                model_context_length,
            );
            LocalModel::Gemma3(m)
        }
        ModelArch::Mamba1 => {
            let cfg: ct_mamba::Config = serde_json::from_str(&config_str)
                .map_err(|e| anyhow::anyhow!("mamba config parse failed: {e}"))?;
            // state-spaces checkpoints use a `backbone` prefix
            let backbone_vb = vb.pp("backbone");
            let m = ct_mamba::Model::new(&cfg, backbone_vb)
                .map_err(|e| anyhow::anyhow!("mamba model init failed: {e:?}"))?;
            tracing::info!(
                "[local-candle] ready in {:.1}s ({} layers, ctx={:?})",
                t0.elapsed().as_secs_f32(),
                cfg.n_layer,
                model_context_length,
            );
            LocalModel::Mamba1 { model: m, cfg }
        }
        ModelArch::Mamba2 => {
            let cfg: Mamba2Config = serde_json::from_str(&config_str)
                .map_err(|e| anyhow::anyhow!("mamba2 config parse failed: {e}"))?;
            let backbone_vb = vb.pp("backbone");
            let m = Mamba2Model::from_vb(&cfg, backbone_vb)
                .map_err(|e| anyhow::anyhow!("mamba2 model init failed: {e:?}"))?;
            tracing::info!(
                "[local-candle] ready in {:.1}s ({} layers, ctx={:?})",
                t0.elapsed().as_secs_f32(),
                m.n_layers(),
                model_context_length,
            );
            LocalModel::Mamba2(m)
        }
    };

    Ok(Loaded {
        model,
        tokenizer,
        chat_template,
        model_context_length,
        arch,
        device,
        dtype,
    })
}

/// Load chat template from the model directory.  For Mamba models (completion-
/// only) we fall back to a simple turn-based format if no template is found.
fn load_chat_template(
    model_dir: &Path,
    model_id: &str,
    arch: ModelArch,
) -> anyhow::Result<String> {
    if let Some(t) =
        load_model_chat_template_from_file(&model_dir.join("tokenizer_config.json"))?
    {
        return Ok(t);
    }
    if let Ok(t) = std::fs::read_to_string(model_dir.join("chat_template.jinja")) {
        tracing::info!("[local-candle] loaded chat_template.jinja for {model_id}");
        return Ok(t);
    }
    // Mamba (SSM) models are completion models without an instruct template;
    // use a plain turn-based fallback.
    if matches!(arch, ModelArch::Mamba1 | ModelArch::Mamba2) {
        tracing::warn!(
            "[local-candle] no chat template for {model_id}; \
             using completion fallback"
        );
        return Ok(
            "{% for message in messages %}{{ message.role }}: {{ message.content }}\n\
             {% endfor %}Assistant: "
                .to_string(),
        );
    }
    anyhow::bail!(
        "chat template missing — expected `chat_template` key in \
         tokenizer_config.json or a `chat_template.jinja` file"
    )
}

fn detect_arch(config_str: &str) -> anyhow::Result<ModelArch> {
    let v: serde_json::Value =
        serde_json::from_str(config_str).map_err(|e| anyhow::anyhow!("config.json json: {e}"))?;
    let model_type = v
        .get("model_type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    ModelArch::from_model_type(model_type)
}

fn find_weight_files(model_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let single = model_dir.join("model.safetensors");
    if single.exists() {
        return Ok(vec![single]);
    }
    let index = model_dir.join("model.safetensors.index.json");
    if index.exists() {
        let s = std::fs::read_to_string(&index)?;
        let v: serde_json::Value = serde_json::from_str(&s)?;
        let weight_map = v
            .get("weight_map")
            .and_then(|m| m.as_object())
            .ok_or_else(|| anyhow::anyhow!("malformed safetensors index"))?;
        let mut shards: Vec<PathBuf> = weight_map
            .values()
            .filter_map(|v| v.as_str())
            .map(|name| model_dir.join(name))
            .collect();
        shards.sort();
        shards.dedup();
        if shards.is_empty() {
            anyhow::bail!("safetensors index has no weight files");
        }
        return Ok(shards);
    }
    anyhow::bail!(
        "no safetensors weights found in {} \
         (expected model.safetensors or model.safetensors.index.json)",
        model_dir.display()
    )
}

// `#[allow(unreachable_code)]` is needed because the `local-candle-accelerate`
// early-return makes the final `Ok(Device::Cpu)` unreachable when that feature
// is active, but the final expression is still required to type-check in builds
// where neither Metal nor Accelerate features are enabled.
#[allow(unreachable_code)]
fn select_device() -> anyhow::Result<Device> {
    // Why CPU + Accelerate beats Metal for single-token decode (the hot path):
    // - Candle's Metal backend uses a fixed BM=32×BN=32 GEMM tile for ALL batch
    //   sizes.  With M=1 (single-token decode) only 1 of 32 tile rows is active
    //   → ~3% GPU thread occupancy → ~7 tok/s on M4 Pro.
    // - Apple's Accelerate `cblas_sgemm` handles M=1 as SGEMV, uses AMX hardware,
    //   and achieves ~43% of CPU memory bandwidth → ~12 tok/s on M4 Pro.
    //
    // Prefill (M = chunk_size = 512) is faster on Metal (789 tok/s vs 341 tok/s),
    // but the decode phase dominates wall-clock time so CPU wins overall.

    #[cfg(feature = "local-candle-accelerate")]
    {
        tracing::info!(
            "[local-candle] CPU + Apple Accelerate BLAS \
             (~12 tok/s decode, 341 tok/s prefill — 1.8× faster decode than Metal)"
        );
        return Ok(Device::Cpu);
    }

    #[cfg(feature = "local-candle-metal")]
    {
        // Increase the Metal command-buffer batch size from the default 50 to 500.
        // Each decode step dispatches ~500+ Metal kernels (28 layers × ~18 ops).
        // At 50-per-buffer this creates ~10 command buffers per token; at 500 we
        // get 1–2, reducing command-buffer submission overhead.
        if std::env::var("CANDLE_METAL_COMPUTE_PER_BUFFER").is_err() {
            // Safety: single-threaded context; no Metal device exists yet.
            std::env::set_var("CANDLE_METAL_COMPUTE_PER_BUFFER", "500");
            tracing::debug!(
                "[local-candle] CANDLE_METAL_COMPUTE_PER_BUFFER=500 \
                 (default was 50 — reduces Metal CB fragmentation)"
            );
        }
        match Device::new_metal(0) {
            Ok(d) => {
                tracing::info!("[local-candle] Metal GPU selected (~7 tok/s decode)");
                return Ok(d);
            }
            Err(e) => tracing::warn!(
                "[local-candle] Metal unavailable ({e}), falling back to CPU"
            ),
        }
    }

    Ok(Device::Cpu)
}

// ---------------------------------------------------------------------------
// Vision stripping
// ---------------------------------------------------------------------------

fn strip_images_from_messages(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .map(|msg| {
            let content = match msg.get("content") {
                Some(Value::Array(parts)) => {
                    let replaced: Vec<Value> = parts
                        .iter()
                        .map(|part| {
                            if part.get("type").and_then(|t| t.as_str()) == Some("image_url") {
                                serde_json::json!({
                                    "type": "text",
                                    "text": "[Image attached — vision not supported by this model]"
                                })
                            } else {
                                part.clone()
                            }
                        })
                        .collect();
                    let all_text = replaced
                        .iter()
                        .all(|p| p.get("type").and_then(|t| t.as_str()) == Some("text"));
                    if all_text && replaced.len() == 1 {
                        replaced[0]
                            .get("text")
                            .cloned()
                            .unwrap_or(Value::Array(replaced))
                    } else {
                        Value::Array(replaced)
                    }
                }
                _ => msg.get("content").cloned().unwrap_or(Value::Null),
            };
            let mut out = msg.as_object().cloned().unwrap_or_default();
            out.insert("content".to_string(), content);
            Value::Object(out)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Generation entry-point
// ---------------------------------------------------------------------------

fn generate_with_cache(
    loaded: &Arc<Mutex<Option<Loaded>>>,
    model_dir: &Path,
    model_id: &str,
    messages: &[Value],
    _tools: &[Value],  // tools intentionally ignored — local models can't call them
    tx: mpsc::Sender<String>,
) -> anyhow::Result<()> {
    let mut guard = loaded
        .lock()
        .map_err(|_| anyhow::anyhow!("mutex poisoned"))?;
    if guard.is_none() {
        *guard = Some(load_state(model_dir, model_id)?);
    }
    let state = guard.as_mut().unwrap();

    let settings_dir = model_dir.parent().unwrap_or(model_dir);
    let gen_opt = crate::gateway::ui_server::local_models::load_settings_blocking(settings_dir);

    // Use Candle-specific defaults (much lower than MLX defaults — CPU has O(L²) cost).
    let max_new_tokens = gen_opt
        .max_new_tokens
        .unwrap_or(crate::gateway::ui_server::local_models::DEFAULT_CANDLE_MAX_NEW_TOKENS)
        .clamp(1, 8192) as usize;
    let max_prompt_tokens = gen_opt
        .max_prompt_tokens
        .unwrap_or(crate::gateway::ui_server::local_models::DEFAULT_CANDLE_MAX_PROMPT_TOKENS)
        .clamp(128, 262_144) as usize;
    // KV-cache sliding window: caps peak RAM regardless of generation length.
    // Applied only to transformer models (Qwen3); Mamba uses fixed SSM state.
    let max_kv_tokens = gen_opt
        .max_kv_tokens
        .unwrap_or(crate::gateway::ui_server::local_models::DEFAULT_KV_WINDOW_TOKENS)
        .clamp(128, 262_144) as usize;

    let processed_messages: Vec<Value> = if state.arch.supports_vision() {
        messages.to_vec()
    } else {
        let stripped = strip_images_from_messages(messages);
        if stripped != messages {
            tracing::info!("[local-candle] stripped image content (text-only model)");
        }
        stripped
    };

    // Local Candle models are text-only completion models — they cannot call tools
    // and injecting tool schemas into the prompt wastes hundreds of tokens per tool
    // (114 tools × ~130 tokens each ≈ 14 800 tokens).  Always pass an empty tools
    // list to keep the prompt small.
    let template = state.chat_template.clone();
    let encodings = state
        .tokenizer
        .apply_chat_template_json_and_encode(
            template,
            model_id,
            None,
            &processed_messages,
            &[],   // no tools — local models can't call them
            None,
            Some(true),
            gen_opt.enable_thinking,
        )
        .map_err(|e| anyhow::anyhow!("chat template failed: {e:?}"))?;

    let mut prompt: Vec<u32> = encodings
        .iter()
        .flat_map(|e| e.get_ids())
        .copied()
        .collect();

    if prompt.len() > max_prompt_tokens {
        let drop = prompt.len() - max_prompt_tokens;
        tracing::warn!(
            "[local-candle] truncating prompt {} → {} tokens",
            prompt.len(),
            max_prompt_tokens
        );
        prompt.drain(..drop);
    }

    tracing::info!(
        "[local-candle] prompt {} tokens head={:?} tail={:?}",
        prompt.len(),
        &prompt[..prompt.len().min(8)],
        &prompt[prompt.len().saturating_sub(8)..],
    );

    let stop_tokens = state.arch.stop_tokens();
    let t_start = std::time::Instant::now();

    match &mut state.model {
        LocalModel::Qwen3(model) => {
            generate_transformer(
                model,
                &prompt,
                &stop_tokens,
                max_new_tokens,
                max_kv_tokens,
                &state.device,
                &state.tokenizer,
                tx,
                t_start,
            )?;
        }
        LocalModel::Gemma3(model) => {
            model.clear_kv_cache();
            generate_gemma3(
                model,
                &prompt,
                &stop_tokens,
                max_new_tokens,
                max_kv_tokens,
                &state.device,
                &state.tokenizer,
                tx,
                t_start,
            )?;
        }
        LocalModel::Mamba1 { model, cfg } => {
            let mut mamba_state = ct_mamba::State::new(1, cfg, state.dtype, &state.device)
                .map_err(|e| anyhow::anyhow!("mamba state init: {e:?}"))?;
            generate_mamba1(
                model,
                &mut mamba_state,
                &prompt,
                &stop_tokens,
                max_new_tokens,
                &state.tokenizer,
                tx,
                t_start,
            )?;
        }
        LocalModel::Mamba2(model) => {
            let mut mamba_state = Mamba2State::new(&model.cfg.clone(), state.dtype, &state.device)
                .map_err(|e| anyhow::anyhow!("mamba2 state init: {e:?}"))?;
            generate_mamba2(
                model,
                &mut mamba_state,
                &prompt,
                &stop_tokens,
                max_new_tokens,
                &state.tokenizer,
                tx,
                t_start,
            )?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Per-arch generation loops
// ---------------------------------------------------------------------------

/// Transformer generate (Qwen3): chunked prefill, then token-by-token decode.
///
/// ## Chunked prefill
/// The prompt is processed in slices of `PREFILL_CHUNK` tokens rather than all
/// at once.  This bounds the attention score matrix at
/// `[n_heads, PREFILL_CHUNK, past + PREFILL_CHUNK]` instead of the O(L²) full
/// allocation that caused ~20 GB peak RAM on long prompts with Qwen3-8B.
///
/// ## KV-cache sliding window
/// `max_kv_tokens` bounds the KV-cache to a sliding window per layer, capping
/// peak RAM regardless of how long the decode phase runs.
fn generate_transformer(
    model: &Qwen3Model,
    prompt: &[u32],
    stop_tokens: &HashSet<u32>,
    max_new_tokens: usize,
    max_kv_tokens: usize,
    device: &Device,
    tokenizer: &Tokenizer,
    tx: mpsc::Sender<String>,
    t_start: std::time::Instant,
) -> anyhow::Result<()> {
    /// Maximum tokens processed in a single prefill forward pass.
    /// Bounds peak attention memory: [n_heads, CHUNK, past+CHUNK].
    /// 512 → ~150 MB peak per layer on Qwen3-8B (BF16), vs ~1.1 GB for 4096.
    const PREFILL_CHUNK: usize = 512;

    let n_layers = model.n_layers();
    let mut caches: Vec<KvCache> = (0..n_layers)
        .map(|_| KvCache::with_max(max_kv_tokens))
        .collect();
    tracing::debug!(
        "[local-candle] KV window = {} tokens/layer ({} layers) → max ~{:.0} MB BF16",
        max_kv_tokens,
        n_layers,
        // 2 (K+V) × layers × window × head_bytes  — rough upper bound
        2.0 * n_layers as f64 * max_kv_tokens as f64 * 128.0 * 2.0 / 1_048_576.0
    );

    let prompt_len = prompt.len();
    let mut last_logits: Option<Tensor> = None;

    // Chunked prefill: feed the prompt in slices of PREFILL_CHUNK tokens.
    // Each chunk sees the KV cache populated by all previous chunks.
    //
    // For non-last chunks we pass `need_logits = false` so the model skips
    // the expensive norm + lm_head projection entirely (those intermediate
    // logits are discarded anyway).
    let n_chunks = prompt_len.div_ceil(PREFILL_CHUNK);
    for (chunk_idx, chunk_start) in (0..prompt_len).step_by(PREFILL_CHUNK).enumerate() {
        let chunk_end = (chunk_start + PREFILL_CHUNK).min(prompt_len);
        let chunk = &prompt[chunk_start..chunk_end];
        let is_last = chunk_end == prompt_len;
        let chunk_tensor = Tensor::new(chunk, device)
            .and_then(|t| t.unsqueeze(0))
            .map_err(|e| anyhow::anyhow!("chunk tensor: {e:?}"))?;
        let logits = model
            .forward(&chunk_tensor, chunk_start, &mut caches, is_last)
            .map_err(|e| anyhow::anyhow!("prefill forward (chunk {}/{}): {e:?}", chunk_idx + 1, n_chunks))?;
        if is_last {
            last_logits = Some(logits);
        }
    }

    let logits = last_logits.ok_or_else(|| anyhow::anyhow!("empty prompt"))?;
    let mut next_token =
        greedy_last(&logits).map_err(|e| anyhow::anyhow!("prefill sample: {e:?}"))?;
    let mut offset = prompt_len;

    let prefill_ms = t_start.elapsed().as_millis();
    let prefill_tok_s = if prefill_ms > 0 {
        prompt_len as f64 / (prefill_ms as f64 / 1000.0)
    } else {
        f64::INFINITY
    };
    tracing::info!(
        "[local-candle][perf] prefill {} tokens in {}ms ({:.0} tok/s, {} chunk(s) of {})",
        prompt_len,
        prefill_ms,
        prefill_tok_s,
        n_chunks,
        PREFILL_CHUNK,
    );

    decode_loop(
        stop_tokens,
        max_new_tokens,
        tokenizer,
        tx,
        &mut next_token,
        &mut offset,
        |tok, off| {
            let tok_tensor = Tensor::new(&[tok], device)
                .and_then(|t| t.unsqueeze(0))
                .map_err(|e| anyhow::anyhow!("tok tensor: {e:?}"))?;
            let logits = model
                .forward(&tok_tensor, off, &mut caches, true)
                .map_err(|e| anyhow::anyhow!("decode forward: {e:?}"))?;
            greedy_last(&logits).map_err(|e| anyhow::anyhow!("decode sample: {e:?}"))
        },
        prompt_len,
    )
}

/// Gemma3 generate: uses candle-transformers internal rotating + full KV cache.
///
/// The internal cache already applies sliding-window for local attention layers.
/// `max_kv_tokens` here simply limits how many prompt tokens we feed in the
/// prefill, consistent with the Qwen3 RAM budget contract.
fn generate_gemma3(
    model: &mut ct_gemma3::Model,
    prompt: &[u32],
    stop_tokens: &HashSet<u32>,
    max_new_tokens: usize,
    max_kv_tokens: usize,
    device: &Device,
    tokenizer: &Tokenizer,
    tx: mpsc::Sender<String>,
    t_start: std::time::Instant,
) -> anyhow::Result<()> {
    // Clip prompt to max_kv_tokens (keep tail — most recent context)
    let prompt = if prompt.len() > max_kv_tokens {
        let drop = prompt.len() - max_kv_tokens;
        tracing::warn!(
            "[local-candle] gemma3: truncating prompt {} → {} tokens (max_kv_tokens)",
            prompt.len(),
            max_kv_tokens
        );
        &prompt[drop..]
    } else {
        prompt
    };
    let prompt_len = prompt.len();
    let prompt_tensor = Tensor::new(prompt, device)
        .and_then(|t| t.unsqueeze(0))
        .map_err(|e| anyhow::anyhow!("prompt tensor: {e:?}"))?;

    // Gemma3 returns [1, 1, vocab] — last position only
    let logits = model
        .forward(&prompt_tensor, 0)
        .map_err(|e| anyhow::anyhow!("prefill forward: {e:?}"))?;
    let mut next_token =
        greedy_last(&logits).map_err(|e| anyhow::anyhow!("prefill sample: {e:?}"))?;
    let mut offset = prompt_len;

    tracing::info!(
        "[local-candle][perf] prefill {} tokens in {}ms",
        prompt_len,
        t_start.elapsed().as_millis()
    );

    decode_loop(
        stop_tokens,
        max_new_tokens,
        tokenizer,
        tx,
        &mut next_token,
        &mut offset,
        |tok, off| {
            let tok_tensor = Tensor::new(&[tok], device)
                .and_then(|t| t.unsqueeze(0))
                .map_err(|e| anyhow::anyhow!("tok tensor: {e:?}"))?;
            let logits = model
                .forward(&tok_tensor, off)
                .map_err(|e| anyhow::anyhow!("decode forward: {e:?}"))?;
            greedy_last(&logits).map_err(|e| anyhow::anyhow!("decode sample: {e:?}"))
        },
        prompt_len,
    )
}

/// Mamba 1 generate: sequential token-by-token (prefill + decode).
fn generate_mamba1(
    model: &ct_mamba::Model,
    mamba_state: &mut ct_mamba::State,
    prompt: &[u32],
    stop_tokens: &HashSet<u32>,
    max_new_tokens: usize,
    tokenizer: &Tokenizer,
    tx: mpsc::Sender<String>,
    t_start: std::time::Instant,
) -> anyhow::Result<()> {
    let device = model.dtype(); // dtype, not device — need device from model
    // Mamba model: forward takes 1D token tensor [1], returns [1, vocab]
    let dev = mamba_state.hs[0].device().clone();

    // Prefill: feed each prompt token through the SSM state
    let mut last_logits: Option<Tensor> = None;
    let prompt_len = prompt.len();
    for &tok in prompt {
        let tok_t = Tensor::new(&[tok], &dev)
            .map_err(|e| anyhow::anyhow!("tok tensor: {e:?}"))?;
        last_logits = Some(
            model
                .forward(&tok_t, mamba_state)
                .map_err(|e| anyhow::anyhow!("mamba1 prefill: {e:?}"))?,
        );
    }

    let logits = last_logits.ok_or_else(|| anyhow::anyhow!("empty prompt"))?;
    let mut next_token =
        greedy_2d(&logits).map_err(|e| anyhow::anyhow!("prefill sample: {e:?}"))?;
    let mut offset = prompt_len;

    tracing::info!(
        "[local-candle][perf] mamba1 prefill {} tokens in {}ms",
        prompt_len,
        t_start.elapsed().as_millis()
    );

    // Suppress the device binding warning — we use device via mamba_state
    let _ = device;

    decode_loop(
        stop_tokens,
        max_new_tokens,
        tokenizer,
        tx,
        &mut next_token,
        &mut offset,
        |tok, _off| {
            let tok_t =
                Tensor::new(&[tok], &dev).map_err(|e| anyhow::anyhow!("tok tensor: {e:?}"))?;
            let logits = model
                .forward(&tok_t, mamba_state)
                .map_err(|e| anyhow::anyhow!("mamba1 decode: {e:?}"))?;
            greedy_2d(&logits).map_err(|e| anyhow::anyhow!("decode sample: {e:?}"))
        },
        prompt_len,
    )
}

/// Mamba 2 generate: sequential token-by-token (prefill + decode).
fn generate_mamba2(
    model: &Mamba2Model,
    mamba_state: &mut Mamba2State,
    prompt: &[u32],
    stop_tokens: &HashSet<u32>,
    max_new_tokens: usize,
    tokenizer: &Tokenizer,
    tx: mpsc::Sender<String>,
    t_start: std::time::Instant,
) -> anyhow::Result<()> {
    let prompt_len = prompt.len();
    let mut last_logits: Option<Tensor> = None;

    for &tok in prompt {
        last_logits = Some(
            model
                .forward_token(tok, mamba_state)
                .map_err(|e| anyhow::anyhow!("mamba2 prefill: {e:?}"))?,
        );
    }

    // last_logits: [1, vocab]
    let logits = last_logits.ok_or_else(|| anyhow::anyhow!("empty prompt"))?;
    let mut next_token =
        greedy_2d(&logits).map_err(|e| anyhow::anyhow!("prefill sample: {e:?}"))?;
    let mut offset = prompt_len;

    tracing::info!(
        "[local-candle][perf] mamba2 prefill {} tokens in {}ms",
        prompt_len,
        t_start.elapsed().as_millis()
    );

    decode_loop(
        stop_tokens,
        max_new_tokens,
        tokenizer,
        tx,
        &mut next_token,
        &mut offset,
        |tok, _off| {
            let logits = model
                .forward_token(tok, mamba_state)
                .map_err(|e| anyhow::anyhow!("mamba2 decode: {e:?}"))?;
            greedy_2d(&logits).map_err(|e| anyhow::anyhow!("decode sample: {e:?}"))
        },
        prompt_len,
    )
}

// ---------------------------------------------------------------------------
// Shared decode loop
// ---------------------------------------------------------------------------

/// Common token-streaming decode loop used by all arch variants.
///
/// `sample_fn` is called with `(current_token, offset)` and must return the
/// next predicted token id.  Returns after `max_new_tokens` or a stop token.
fn decode_loop<F>(
    stop_tokens: &HashSet<u32>,
    max_new_tokens: usize,
    tokenizer: &Tokenizer,
    tx: mpsc::Sender<String>,
    next_token: &mut u32,
    offset: &mut usize,
    mut sample_fn: F,
    prompt_len: usize,
) -> anyhow::Result<()>
where
    F: FnMut(u32, usize) -> anyhow::Result<u32>,
{
    // Pending tokens that haven't been flushed yet.  We accumulate a small
    // buffer because some tokenizers emit empty strings for partial multi-byte
    // sequences (e.g. the first byte of a UTF-8 kanji).  Flushing after every
    // token keeps latency low; the non-empty guard handles partial sequences.
    let mut token_buf: Vec<u32> = Vec::with_capacity(8);
    let t_decode = std::time::Instant::now();
    // Track time of first generated token (first `sample_fn` result used).
    let mut t_first_token: Option<std::time::Duration> = None;

    // Stop check is BEFORE push: the EOS token itself is never streamed, and
    // `break` falls through to the post-loop drain below. Do NOT add an early
    // `return Ok(())` inside this branch — MLX once had that exact bug and
    // dropped up to (flush_threshold − 1) tokens, including any pending
    // `</tool_call>` close marker, causing the parser to reject otherwise
    // valid tool calls as "truncated". The structural rule is: every exit
    // path flushes the token buffer.
    for step in 0..max_new_tokens {
        if stop_tokens.contains(next_token) {
            break;
        }

        token_buf.push(*next_token);

        // Try to decode and stream immediately.  If the tokenizer returns an
        // empty string (partial multi-byte char), hold the token and retry on
        // the next step — after at most 4 held tokens force a flush anyway.
        let should_flush = step + 1 == max_new_tokens || token_buf.len() >= 4;
        if let Ok(text) = tokenizer.decode(&token_buf, true) {
            if !text.is_empty() {
                token_buf.clear();
                tx.blocking_send(text)
                    .map_err(|_| anyhow::anyhow!("stream channel closed"))?;
            } else if should_flush {
                token_buf.clear(); // discard undecodable tokens
            }
        } else if should_flush {
            token_buf.clear();
        }

        let t_before_sample = std::time::Instant::now();
        *next_token = sample_fn(*next_token, *offset)?;
        if t_first_token.is_none() {
            // First call to sample_fn = one full decode forward pass.
            t_first_token = Some(t_before_sample.elapsed());
            tracing::info!(
                "[local-candle][perf] first decode step: {}ms ({:.1} tok/s single-step)",
                t_before_sample.elapsed().as_millis(),
                1000.0 / t_before_sample.elapsed().as_millis().max(1) as f64,
            );
        }
        *offset += 1;
    }

    if !token_buf.is_empty() {
        if let Ok(text) = tokenizer.decode(&token_buf, true) {
            if !text.is_empty() {
                let _ = tx.blocking_send(text);
            }
        }
    }

    let decode_steps = *offset - prompt_len;
    let decode_ms = t_decode.elapsed().as_millis();
    if decode_ms > 0 {
        tracing::info!(
            "[local-candle][perf] decoded {} tokens in {}ms ({:.1} tok/s)",
            decode_steps,
            decode_ms,
            decode_steps as f64 / (decode_ms as f64 / 1000.0),
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Greedy sampling helpers
// ---------------------------------------------------------------------------

/// Greedy decode from `[1, 1, vocab_size]` (Qwen3 `forward` always returns
/// only the last token's logits).
fn greedy_last(logits: &Tensor) -> candle_core::Result<u32> {
    logits
        .squeeze(0)?  // [1, vocab]
        .squeeze(0)?  // [vocab]
        .argmax(candle_core::D::Minus1)?
        .to_scalar::<u32>()
}

/// Greedy decode from `[1, vocab_size]` (SSM model logits).
fn greedy_2d(logits: &Tensor) -> candle_core::Result<u32> {
    logits
        .squeeze(0)?
        .argmax(candle_core::D::Minus1)?
        .to_scalar::<u32>()
}
