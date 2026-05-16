//! Native MLX inference engine.
//!
//! Apple Silicon only. Uses **`mlx-rs`** plus weights/templates vendored in-tree (see [`super::mlx_lm`]).
//!
//! Supported architectures (autodetected from `config.json::model_type`):
//! - **Qwen3** — `qwen3*` (transformer + GQA, with optional MLX quantization & TurboQuant KV).
//! - **Llama** — `llama` (Llama-3.x, Llama-3.2, Nesso; GQA, no Q/K norm, custom EOS from config).
//! - **Gemma-2** — `gemma2` (alternating sliding-window/global attention, Gemma RMSNorm +1, attn/final soft-capping).
//! - **Gemma-3** — `gemma3` / `gemma3_text` (hybrid sliding+full attention, Gemma RMSNorm, 4 norms/block, dual-RoPE).
//! - **Mamba-2** — `mamba2` (SSD recurrence, fixed-size SSM cache; ignores `kv_cache_bits`).
//! - **Bonsai-Q1** — MLX 1-bit affine checkpoints (`bonsai*` in `model_type`; FP16 KV only).
//!
//! KV cache: **`SteppingKeyValueCache` (FP16)** — `slice_update` + grow-by-256, sliding window.
//! Optional **`TurboQuantKeyValueCache`** when `kv_cache_bits` is set (`turboquant-rs` storage).
//! RoPE uses **`ModelInput::rope_offset`** (caller `usize`), not cache-internal position.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::runtime::{
    ChatMessage, LocalModelRuntime, RuntimeEndpoint, RuntimeHealth, RuntimeStatus,
};

/// Architecture-tagged model handle. The transformer (Qwen3) and SSM (Mamba-2)
/// paths share a single engine but have wildly different forward signatures,
/// cache shapes, and stop-token sets — so we keep one variant per supported arch.
enum ModelKind {
    Qwen3(super::mlx_lm::models::qwen3::Model),
    Llama(super::mlx_lm::models::llama::Model),
    Gemma2(super::mlx_lm::models::gemma2::Gemma2CausalLM),
    Gemma3(super::mlx_lm::models::gemma3::Model),
    Mamba2(super::mlx_lm::models::mamba2::Model),
    FalconMamba(super::mlx_lm::models::falcon_mamba::Model),
    BonsaiQ1(super::mlx_lm::models::bonsai_q1::LoadedBonsaiQ1),
}

impl ModelKind {
    fn arch_name(&self) -> &'static str {
        match self {
            Self::Qwen3(_) => "qwen3",
            Self::Llama(_) => "llama",
            Self::Gemma2(_) => "gemma2",
            Self::Gemma3(_) => "gemma3",
            Self::Mamba2(_) => "mamba2",
            Self::FalconMamba(_) => "falcon_mamba",
            Self::BonsaiQ1(_) => "bonsai_q1",
        }
    }
}

/// Heavyweight cached state: model weights + tokenizer + (optional) rendered
/// chat template. Populated by `load()` and dropped by `unload()`, so RAM can
/// be freed on demand without restarting the daemon.
struct Loaded {
    model: ModelKind,
    tokenizer: super::mlx_lm_utils::tokenizer::Tokenizer,
    /// `None` for base models that ship no `chat_template` (e.g. Mamba-2 base).
    /// Generation falls back to plain concatenation in that case.
    chat_template: Option<String>,
    n_layers: usize,
    /// Attention-only dims; `0` for SSM-only models (Mamba-2 leaves them unused).
    head_dim: i32,
    n_kv_heads: i32,
}

/// In-process MLX inference engine. Caches the loaded model so subsequent
/// chats reuse weights instead of re-reading safetensors every call.
pub struct MlxNativeEngine {
    model_dir: PathBuf,
    model_id: String,
    /// `settings.json` `kv_cache_bits` (`3` / `4` → TurboQuant storage; `None` → FP16 stepping).
    kv_cache_bits: Option<u8>,
    /// Arc-wrapped so we can clone the handle into the blocking worker that
    /// runs generation. MLX state is non-Send through naked references, but
    /// `Arc<Mutex<...>>` is.
    loaded: Arc<Mutex<Option<Loaded>>>,
    status: Mutex<RuntimeStatus>,
}

impl MlxNativeEngine {
    pub fn new(model_dir: &Path, model_id: &str, kv_cache_bits: Option<u8>) -> Self {
        Self {
            model_dir: model_dir.to_path_buf(),
            model_id: model_id.to_owned(),
            kv_cache_bits,
            loaded: Arc::new(Mutex::new(None)),
            status: Mutex::new(RuntimeStatus::NotInstalled),
        }
    }

    /// True when weights are currently in memory.
    pub fn is_loaded(&self) -> bool {
        self.loaded.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// Drop the cached model + tokenizer, freeing the bulk of RAM used by
    /// this engine. Safe to call when nothing is loaded (no-op).
    pub fn unload(&self) {
        if let Ok(mut g) = self.loaded.lock() {
            *g = None;
        }
        self.set_status(RuntimeStatus::Stopped);
    }

    /// Force a load now (used by the UI "Load" button). Equivalent to the
    /// lazy load that happens on first generate_stream.
    pub fn warm_up(&self) -> anyhow::Result<()> {
        ensure_loaded_blocking(self)
    }

    pub fn model_dir(&self) -> &Path {
        &self.model_dir
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    /// Settings fingerprint for registry reload (see `local_models/settings.json` `kv_cache_bits`).
    pub fn kv_cache_bits(&self) -> Option<u8> {
        self.kv_cache_bits
    }

    fn set_status(&self, s: RuntimeStatus) {
        if let Ok(mut g) = self.status.lock() {
            *g = s;
        }
    }
}

#[async_trait]
impl LocalModelRuntime for MlxNativeEngine {
    async fn ensure_installed(&self) -> anyhow::Result<()> {
        if !self.model_dir.exists() {
            anyhow::bail!(
                "model directory not found: {} — download weights first \
                 (e.g. `huggingface-cli download {}`)",
                self.model_dir.display(),
                self.model_id
            );
        }
        for required in ["tokenizer.json", "tokenizer_config.json"] {
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
            adapt: "local-mlx-native".to_owned(),
            api_key: None,
        })
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.set_status(RuntimeStatus::Stopped);
        Ok(())
    }

    async fn health(&self) -> anyhow::Result<RuntimeHealth> {
        let status = self.status.lock().map(|g| *g).unwrap_or(RuntimeStatus::Error);
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
        messages: Vec<serde_json::Value>,
        _tools: Vec<serde_json::Value>,
        tx: mpsc::Sender<String>,
    ) -> anyhow::Result<()> {
        // Convert OpenAI-format Value messages → ChatMessage.
        let chat_msgs: Vec<ChatMessage> = messages
            .iter()
            .filter_map(|v| {
                let role_str = v.get("role")?.as_str()?;
                let content = v.get("content")?.as_str().unwrap_or("").to_owned();
                let role = match role_str {
                    "system" => super::runtime::Role::System,
                    "assistant" => super::runtime::Role::Assistant,
                    _ => super::runtime::Role::User,
                };
                Some(ChatMessage { role, content })
            })
            .collect();

        self.stream_to_channel(&chat_msgs, tx).await
    }
}

impl MlxNativeEngine {
    /// Public native-stream entry point used by `query_llm.rs`.
    pub async fn stream_to_channel(
        &self,
        messages: &[ChatMessage],
        tx: mpsc::Sender<String>,
    ) -> anyhow::Result<()> {
        let loaded = Arc::clone(&self.loaded);
        let model_dir = self.model_dir.clone();
        let model_id = self.model_id.clone();
        let messages = messages.to_vec();
        let kv_bits = self.kv_cache_bits;

        let join = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            generate_with_cache(&loaded, &model_dir, &model_id, &messages, kv_bits, tx)
        });

        join.await??;
        Ok(())
    }
}

/// Synchronous, blocking-thread version of `MlxNativeEngine::warm_up`.
/// Populates `loaded` (if currently None) with Model + Tokenizer + template.
fn ensure_loaded_blocking(engine: &MlxNativeEngine) -> anyhow::Result<()> {
    let mut guard = engine
        .loaded
        .lock()
        .map_err(|_| anyhow::anyhow!("loaded mutex poisoned"))?;
    if guard.is_some() {
        return Ok(());
    }
    let state = load_state(&engine.model_dir, &engine.model_id)?;
    *guard = Some(state);
    engine.set_status(RuntimeStatus::Ready);
    Ok(())
}

fn load_state(model_dir: &Path, model_id: &str) -> anyhow::Result<Loaded> {
    use super::mlx_lm_utils::tokenizer::Tokenizer;

    let arch = detect_architecture(model_id, model_dir)?;
    let tokenizer_file = model_dir.join("tokenizer.json");
    let tokenizer_config = model_dir.join("tokenizer_config.json");
    let tokenizer = Tokenizer::from_file(&tokenizer_file)
        .map_err(|e| anyhow::anyhow!("tokenizer load failed: {e:?}"))?;
    let chat_template = load_mlx_chat_template(model_dir, model_id, &tokenizer_config)?;

    let (model, n_layers, head_dim, n_kv_heads) = match arch {
        Arch::Qwen3 => {
            if chat_template.is_none() {
                anyhow::bail!("Qwen3 chat template missing in tokenizer_config.json");
            }
            let m = load_qwen3_any(model_dir)
                .map_err(|e| anyhow::anyhow!("load_qwen3 failed: {e:?}"))?;
            let n = m.args.num_hidden_layers as usize;
            let hd = m.args.head_dim;
            let kvh = m.args.num_key_value_heads;
            (ModelKind::Qwen3(m), n, hd, kvh)
        }
        Arch::Llama => {
            let m = load_llama_any(model_dir)
                .map_err(|e| anyhow::anyhow!("load_llama failed: {e:?}"))?;
            let n = m.args.num_hidden_layers as usize;
            let hd = m.args.head_dim;
            let kvh = m.args.num_key_value_heads;
            (ModelKind::Llama(m), n, hd, kvh)
        }
        Arch::Gemma2 => {
            if chat_template.is_none() {
                anyhow::bail!("Gemma 2 chat template missing in tokenizer_config.json");
            }
            let m = super::mlx_lm::models::gemma2::load_gemma2_model(model_dir)
                .map_err(|e| anyhow::anyhow!("load_gemma2 failed: {e:?}"))?;
            let n = m.args.num_hidden_layers as usize;
            let hd = m.args.head_dim;
            let kvh = m.args.num_key_value_heads;
            (ModelKind::Gemma2(m), n, hd, kvh)
        }
        Arch::Gemma3 => {
            let m = load_gemma3_any(model_dir)
                .map_err(|e| anyhow::anyhow!("load_gemma3 failed: {e:?}"))?;
            let n = m.args.num_hidden_layers as usize;
            let hd = m.args.head_dim;
            let kvh = m.args.num_key_value_heads;
            (ModelKind::Gemma3(m), n, hd, kvh)
        }
        Arch::Mamba2 => {
            let m = load_mamba2_any(model_dir)
                .map_err(|e| anyhow::anyhow!("load_mamba2 failed: {e:?}"))?;
            let n = m.args.num_hidden_layers as usize;
            (ModelKind::Mamba2(m), n, 0, 0)
        }
        Arch::FalconMamba => {
            let m = load_falcon_mamba_any(model_dir)
                .map_err(|e| anyhow::anyhow!("load_falcon_mamba failed: {e:?}"))?;
            let n = m.args.num_hidden_layers as usize;
            (ModelKind::FalconMamba(m), n, 0, 0)
        }
        Arch::BonsaiQ1 => {
            if chat_template.is_none() {
                anyhow::bail!("Bonsai-Q1 chat template missing in tokenizer_config.json");
            }
            let b = super::mlx_lm::models::bonsai_q1::load_bonsai_q1_bundle(model_dir)
                .map_err(|e| anyhow::anyhow!("load_bonsai_q1 failed: {e:?}"))?;
            let n = b.gpu.config.layers;
            let hd = i32::try_from(b.gpu.config.head_dim)
                .map_err(|_| anyhow::anyhow!("bonsai-q1 head_dim does not fit i32"))?;
            let kvh = i32::try_from(b.gpu.config.kv_heads)
                .map_err(|_| anyhow::anyhow!("bonsai-q1 kv_heads does not fit i32"))?;
            (ModelKind::BonsaiQ1(b), n, hd, kvh)
        }
    };
    tracing::info!(
        "[local-mlx-native] cached state ready for {model_id} (arch={}, {n_layers} layers, head_dim={head_dim}, kv_heads={n_kv_heads})",
        model.arch_name(),
    );
    Ok(Loaded {
        model,
        tokenizer,
        chat_template,
        n_layers,
        head_dim,
        n_kv_heads,
    })
}

/// Current process RSS (MiB) via macOS Mach `task_info`. Returns **0** on failure or non-macOS.
#[allow(deprecated)] // `libc::mach_task_self` deprecated in favor of `mach2`; keep single deps for MLX path.
fn rss_mib() -> f64 {
    #[cfg(target_os = "macos")]
    unsafe {
        #[repr(C)]
        struct MachTaskBasicInfo {
            virtual_size: u64,
            resident_size: u64,
            resident_size_max: u64,
            user_time: libc::time_value_t,
            system_time: libc::time_value_t,
            policy: i32,
            suspend_count: i32,
        }
        const MACH_TASK_BASIC_INFO: libc::c_int = 20;
        const MACH_TASK_BASIC_INFO_COUNT: u32 = 12;

        let mut info: MachTaskBasicInfo = std::mem::zeroed();
        let mut count: u32 = MACH_TASK_BASIC_INFO_COUNT;
        let kr = libc::task_info(
            libc::mach_task_self(),
            MACH_TASK_BASIC_INFO as _,
            &mut info as *mut _ as _,
            &mut count,
        );
        if kr == 0 {
            info.resident_size as f64 / (1024.0 * 1024.0)
        } else {
            0.0
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        0.0
    }
}

/// MLX Metal allocator stats (**active**, **cache**, **peak**) in MiB (via `mlx-sys`, same MLX build as `mlx-rs`).
///
/// **active**: bytes referenced by live arrays. **cache**: freed buffers held in MLX’s pool.
/// **peak**: high-water since start (or since last [`mlx_sys::mlx_reset_peak_memory`]).
#[inline]
fn mlx_mem_mib() -> (f64, f64, f64) {
    unsafe {
        let mut active: usize = 0;
        let mut cache: usize = 0;
        let mut peak: usize = 0;
        mlx_sys::mlx_get_active_memory(&mut active);
        mlx_sys::mlx_get_cache_memory(&mut cache);
        mlx_sys::mlx_get_peak_memory(&mut peak);
        const M: f64 = 1024.0 * 1024.0;
        (active as f64 / M, cache as f64 / M, peak as f64 / M)
    }
}

fn mlx_log_generate_done(label: &'static str, rss_start: f64, generated_count: usize) {
    let (ma, mc, mp) = mlx_mem_mib();
    tracing::info!(
        "[local-mlx-native][mem] {} — rss={:.0}(Δ{:.0}) | mlx active={:.0} cache={:.0} peak={:.0} MiB | new_tokens={}",
        label,
        rss_mib(),
        rss_mib() - rss_start,
        ma,
        mc,
        mp,
        generated_count,
    );
}

/// Drop MLX’s pooled device buffers (`mlx_clear_cache`) and the lazy compile cache before the next turn.
fn mlx_release_after_turn() {
    unsafe {
        mlx_sys::mlx_clear_cache();
    }
    let (a, c, _) = mlx_mem_mib();
    tracing::info!(
        "[local-mlx-native][mem] MLX buffer pool cleared — active={:.0} cache={:.0} MiB",
        a,
        c,
    );
    mlx_rs::transforms::compile::clear_cache();
}

/// Resolve `bos_token` / `eos_token` strings for HF-style chat templates (`{{ bos_token }}`, etc.).
/// Gemma‑3 prefixes the template with `{{ bos_token }}`; Minijinja must receive the literal BOS piece
/// the tokenizer uses (typically id 2).
fn mlx_chat_template_special_tokens(
    tokenizer: &super::mlx_lm_utils::tokenizer::Tokenizer,
    model: &ModelKind,
    template: &str,
) -> (Option<String>, Option<String>) {
    let need_bos = template.contains("bos_token");
    let need_eos = template.contains("eos_token");
    if !need_bos && !need_eos {
        return (None, None);
    }
    match model {
        ModelKind::Gemma3(m) => {
            let bos = if need_bos {
                m.args.bos_token_id.and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
            } else {
                None
            };
            if need_bos {
                match &bos {
                    Some(s) if !s.is_empty() => {
                        tracing::debug!(
                            "[local-mlx-native] chat_template bos_token injected (decoded len={})",
                            s.len()
                        )
                    }
                    _ => tracing::warn!(
                        "[local-mlx-native] chat_template mentions bos_token — could not decode from bos_token_id={:?}",
                        m.args.bos_token_id
                    ),
                }
            }

            let eos = if need_eos {
                m.args
                    .eos_token_ids
                    .first()
                    .copied()
                    .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
            } else {
                None
            };
            (bos, eos)
        }
        ModelKind::Gemma2(m) => {
            let bos = if need_bos {
                m.args
                    .bos_token_id
                    .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
            } else {
                None
            };
            if need_bos {
                match &bos {
                    Some(s) if !s.is_empty() => {
                        tracing::debug!(
                            "[local-mlx-native] chat_template bos_token injected for Gemma-2 (decoded len={})",
                            s.len()
                        )
                    }
                    _ => tracing::warn!(
                        "[local-mlx-native] chat_template mentions bos_token — could not decode from bos_token_id={:?}",
                        m.args.bos_token_id
                    ),
                }
            }

            let eos = if need_eos {
                m.args
                    .eos_token_ids
                    .first()
                    .copied()
                    .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
            } else {
                None
            };
            (bos, eos)
        }
        ModelKind::BonsaiQ1(b) => {
            let bos = if need_bos {
                b.bos_token_id
                    .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
            } else {
                None
            };
            if need_bos {
                match &bos {
                    Some(s) if !s.is_empty() => tracing::debug!(
                        "[local-mlx-native] chat_template bos_token injected for Bonsai-Q1 (decoded len={})",
                        s.len()
                    ),
                    _ => tracing::warn!(
                        "[local-mlx-native] chat_template mentions bos_token — could not decode from bos_token_id={:?}",
                        b.bos_token_id,
                    ),
                }
            }

            let eos = if need_eos {
                b.eos_token_ids
                    .first()
                    .copied()
                    .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
            } else {
                None
            };
            (bos, eos)
        }
        ModelKind::Mamba2(m) => {
            let bos = if need_bos {
                tokenizer
                    .token_to_id("<s>")
                    .or(if m.args.bos_token_id > 0 {
                        Some(m.args.bos_token_id as u32)
                    } else {
                        None
                    })
                    .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
            } else {
                None
            };
            let eos = if need_eos {
                tokenizer
                    .token_to_id("</s>")
                    .or_else(|| m.args.stop_token_ids().first().copied())
                    .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
            } else {
                None
            };
            (bos, eos)
        }
        ModelKind::FalconMamba(_) => {
            let bos = if need_bos {
                tokenizer
                    .token_to_id("<s>")
                    .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
            } else {
                None
            };
            let eos = if need_eos {
                tokenizer
                    .token_to_id("</s>")
                    .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
            } else {
                None
            };
            (bos, eos)
        }
        _ => {
            if need_bos || need_eos {
                tracing::warn!(
                    "[local-mlx-native] chat template references bos_token/eos_token but special-token injection is only wired for Gemma-2/3 / Mamba (arch={}); leaving empty placeholders",
                    model.arch_name(),
                );
            }
            (None, None)
        }
    }
}

/// Load Jinja chat template: `tokenizer_config.json`, then `chat_template.jinja`, then
/// architecture-specific fallbacks (Mistral `[INST]` for Mamba-Codestral / `[INST]` tokenizers).
fn load_mlx_chat_template(
    model_dir: &Path,
    model_id: &str,
    tokenizer_config_path: &Path,
) -> anyhow::Result<Option<String>> {
    use super::mlx_lm_utils::tokenizer::load_model_chat_template_from_file;

    if let Some(t) = load_model_chat_template_from_file(tokenizer_config_path).ok().flatten() {
        return Ok(Some(t));
    }
    let jinja_path = model_dir.join("chat_template.jinja");
    if jinja_path.exists() {
        let t = std::fs::read_to_string(&jinja_path)
            .map_err(|e| anyhow::anyhow!("read chat_template.jinja: {e}"))?;
        tracing::info!(
            "[local-mlx-native] loaded chat_template.jinja for {model_id}"
        );
        return Ok(Some(t));
    }

    let arch = detect_architecture(model_id, model_dir)?;
    match arch {
        Arch::Mamba2 | Arch::FalconMamba => {
            let tok_path = model_dir.join("tokenizer.json");
            if tokenizer_has_mistral_inst_tokens(&tok_path) {
                tracing::info!(
                    "[local-mlx-native] no chat template for {model_id}; \
                     using Mistral V1 [INST] fallback (Mamba-Codestral / instruct)"
                );
                return Ok(Some(
                    super::mlx_lm::models::mamba2::MISTRAL_V1_INSTRUCT_CHAT_TEMPLATE.to_string(),
                ));
            }
            tracing::warn!(
                "[local-mlx-native] no chat template for {model_id}; \
                 using plain User/Assistant transcript"
            );
            Ok(None)
        }
        _ => Ok(None),
    }
}

/// True when the tokenizer defines Mistral-style `[INST]` / `[/INST]` special tokens.
fn tokenizer_has_mistral_inst_tokens(tokenizer_json: &Path) -> bool {
    let Ok(raw) = std::fs::read_to_string(tokenizer_json) else {
        return false;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    let Some(decoder) = v.get("added_tokens").and_then(|a| a.as_array()) else {
        return false;
    };
    let mut has_inst = false;
    let mut has_end_inst = false;
    for entry in decoder {
        let Some(content) = entry.get("content").and_then(|c| c.as_str()) else {
            continue;
        };
        if content == "[INST]" {
            has_inst = true;
        }
        if content == "[/INST]" {
            has_end_inst = true;
        }
    }
    has_inst && has_end_inst
}

/// Collect stop token ids for Mamba / Falcon-Mamba generation.
fn mamba_stop_token_ids(
    config_stops: &[u32],
    tokenizer: &super::mlx_lm_utils::tokenizer::Tokenizer,
) -> Vec<u32> {
    let mut stops: Vec<u32> = Vec::new();
    for &id in config_stops {
        if !stops.contains(&id) {
            stops.push(id);
        }
    }
    for special in ["</s>", "<|endoftext|>", "[/INST]"] {
        if let Some(id) = tokenizer.token_to_id(special) {
            if !stops.contains(&id) {
                stops.push(id);
            }
        }
    }
    if stops.is_empty() {
        stops.push(2);
    }
    stops
}

/// Decoded token ids counted back from the current step (HF repetition penalty window).
const MLX_REPEN_DECODE_WINDOW: usize = 128;

fn mlx_recent_decode_push(buf: &mut Vec<u32>, tok: u32, window: usize) {
    if window == 0 {
        return;
    }
    buf.push(tok);
    if buf.len() > window {
        let drop = buf.len() - window;
        buf.drain(..drop);
    }
}

/// Scale logits for tokens that already appeared recently (GPT-2 / HF convention).
fn hf_repetition_penalty_row(logits: &mut [f32], recent: &[u32], penalty: f32) {
    if penalty <= 1.0 {
        return;
    }
    let n = logits.len();
    let mut seen = HashSet::new();
    for &tid in recent {
        let i = tid as usize;
        if i >= n || !seen.insert(tid) {
            continue;
        }
        let v = logits[i];
        logits[i] = if v > 0.0 { v / penalty } else { v * penalty };
    }
}

fn sample_decode_token_id(
    last_logits: &mlx_rs::Array,
    temperature: f32,
    repetition_penalty: f32,
    recent_decode_ids: &[u32],
) -> anyhow::Result<mlx_rs::Array> {
    use mlx_rs::ops::flatten;
    use mlx_rs::Array;

    let mlx_sample = super::mlx_lm::models::qwen3::sample;

    if repetition_penalty <= 1.0 || recent_decode_ids.is_empty() {
        return mlx_sample(last_logits, temperature).map_err(|e| anyhow::anyhow!("mlx sample: {e:?}"));
    }

    let shape: Vec<i32> = last_logits.shape().to_vec();
    let flat = flatten(last_logits, None, None).map_err(|e| anyhow::anyhow!("flatten logits: {e:?}"))?;
    mlx_rs::transforms::eval(std::slice::from_ref(&flat)).map_err(|e| anyhow::anyhow!("eval logits: {e:?}"))?;
    let mut row = flat
        .try_as_slice::<f32>()
        .map_err(|e| anyhow::anyhow!("logits not contiguous f32: {e:?}"))?
        .to_vec();
    hf_repetition_penalty_row(&mut row, recent_decode_ids, repetition_penalty);
    let adj = Array::from_slice(row.as_slice(), &[row.len() as i32])
        .reshape(shape.as_slice())
        .map_err(|e| anyhow::anyhow!("reshape logits: {e:?}"))?;
    mlx_rs::transforms::eval(std::slice::from_ref(&adj)).map_err(|e| anyhow::anyhow!("eval logits: {e:?}"))?;
    mlx_sample(&adj, temperature).map_err(|e| anyhow::anyhow!("mlx sample: {e:?}"))
}

/// Synchronous generation entry. Runs on a blocking worker, holding the
/// engine's `loaded` mutex for the duration of one inference call.
fn generate_with_cache(
    loaded: &Arc<Mutex<Option<Loaded>>>,
    model_dir: &Path,
    model_id: &str,
    messages: &[ChatMessage],
    _kv_cache_bits: Option<u8>,
    tx: mpsc::Sender<String>,
) -> anyhow::Result<()> {
    use super::mlx_lm::cache::{eval_all_caches, DEFAULT_TQ_ACTIVATE_AT, KvCache};
    use super::mlx_lm_utils::tokenizer::{ApplyChatTemplateArgs, Conversation, Role as TokRole};
    use mlx_rs::ops::indexing::{IndexOp, NewAxis};
    use mlx_rs::transforms::eval;
    use mlx_rs::Array;

    let mut guard = loaded
        .lock()
        .map_err(|_| anyhow::anyhow!("loaded mutex poisoned"))?;
    if guard.is_none() {
        *guard = Some(load_state(model_dir, model_id)?);
    }
    // Safe: we just ensured Some.
    let state = guard.as_mut().unwrap();
    let tokenizer = &mut state.tokenizer;
    let template_opt = state.chat_template.clone();
    let n_layers = state.n_layers;

    let settings_dir = model_dir.parent().unwrap_or(model_dir);
    let gen_opt =
        crate::gateway::ui_server::local_models::load_settings_blocking(settings_dir);

    use super::mlx_prompt::{
        strip_thinking_blocks, trim_conversation_history, MLX_MAX_HISTORY_TURNS,
    };

    // Pass `system` as its own message when present: Gemma-3's `chat_template`
    // requires `messages[0].role == 'system'` to set `first_user_prefix`; merging
    // system into the user blob skips that branch and mis-formats the prompt.
    let mut convs: Vec<Conversation<TokRole, String>> = Vec::with_capacity(messages.len());
    for m in messages {
        match m.role {
            super::runtime::Role::System => {
                convs.push(Conversation {
                    role: TokRole::System,
                    content: m.content.clone(),
                });
            }
            super::runtime::Role::User => {
                convs.push(Conversation {
                    role: TokRole::User,
                    content: m.content.clone(),
                });
            }
            super::runtime::Role::Assistant => {
                let content = strip_thinking_blocks(&m.content);
                if content.is_empty() {
                    continue;
                }
                convs.push(Conversation {
                    role: TokRole::Assistant,
                    content,
                });
            }
        }
    }

    let history_before = convs.len();
    trim_conversation_history(&mut convs, MLX_MAX_HISTORY_TURNS);
    if convs.len() < history_before {
        tracing::info!(
            "[local-mlx-native] history {} → {} messages after turn cap ({MLX_MAX_HISTORY_TURNS} user turns)",
            history_before,
            convs.len()
        );
    }

    let enable_thinking = gen_opt.enable_thinking;
    let mut prompt: Vec<u32> = if let Some(template) = template_opt {
        let (bos_token, eos_token) =
            mlx_chat_template_special_tokens(tokenizer, &state.model, template.as_str());
        let args = ApplyChatTemplateArgs {
            conversations: vec![convs.clone().into()],
            documents: None,
            model_id,
            chat_template_id: None,
            add_generation_prompt: Some(true),
            continue_final_message: None,
            enable_thinking,
            bos_token,
            eos_token,
        };
        let encodings = tokenizer
            .apply_chat_template_and_encode(template, args)
            .map_err(|e| anyhow::anyhow!("chat template apply failed: {e:?}"))?;
        encodings
            .iter()
            .flat_map(|e| e.get_ids())
            .copied()
            .collect()
    } else {
        // No chat template (Mamba-2 base etc.) — fall back to a plain transcript.
        let mut text = String::new();
        for c in &convs {
            let role = match c.role {
                TokRole::User => "User",
                TokRole::Assistant => "Assistant",
                TokRole::System => "System",
            };
            text.push_str(&format!("{role}: {}\n", c.content));
        }
        text.push_str("Assistant:");
        let enc = tokenizer
            .encode(text, false)
            .map_err(|e| anyhow::anyhow!("plain encode failed: {e:?}"))?;
        enc.get_ids().to_vec()
    };
    let max_prompt_tokens = gen_opt
        .max_prompt_tokens
        .unwrap_or(crate::gateway::ui_server::local_models::DEFAULT_MLX_MAX_PROMPT_TOKENS)
        .clamp(512, 262_144) as usize;
    let max_new_tokens = gen_opt
        .max_new_tokens
        .unwrap_or(crate::gateway::ui_server::local_models::DEFAULT_MLX_MAX_NEW_TOKENS)
        .clamp(1, 8192) as usize;
    let max_kv_tokens = gen_opt
        .max_kv_tokens
        .unwrap_or(crate::gateway::ui_server::local_models::DEFAULT_KV_WINDOW_TOKENS)
        .clamp(128, 262_144) as i32;

    if prompt.len() > max_prompt_tokens {
        let drop = prompt.len() - max_prompt_tokens;
        tracing::warn!(
            "[local-mlx-native] truncating prompt {} → {} tokens (RAM/KV cap; edit ~/.senclaw/local_models/settings.json max_prompt_tokens)",
            prompt.len(),
            max_prompt_tokens
        );
        prompt.drain(..drop);
    }
    // KV window also bounds prompt length — keeps prefill RAM/graph bounded per turn.
    let max_kv_usize = max_kv_tokens as usize;
    if prompt.len() > max_kv_usize {
        let drop = prompt.len() - max_kv_usize;
        tracing::warn!(
            "[local-mlx-native] truncating prompt {} → {} tokens (max_kv_tokens; older context dropped)",
            prompt.len(),
            max_kv_usize
        );
        prompt.drain(..drop);
    }

    tracing::info!(
        "[local-mlx-native] prompt {} tokens, max_kv_window {}, head={:?} tail={:?}",
        prompt.len(),
        max_kv_tokens,
        &prompt[..prompt.len().min(8)],
        &prompt[prompt.len().saturating_sub(8)..]
    );
    let prompt_tokens = Array::from(&prompt[..]).index(NewAxis);

    let tq_bits = _kv_cache_bits.or(gen_opt.kv_cache_bits).filter(|b| *b > 0);
    let tq_activate_at = gen_opt
        .tq_activate_at
        .map(|v| v as i32)
        .unwrap_or(DEFAULT_TQ_ACTIVATE_AT);
    let head_dim = state.head_dim;
    let n_kv_heads = state.n_kv_heads;

    // Per-arch cache allocation + stop-token set.
    let (mut cache, is_stop): (Vec<Option<KvCache>>, Box<dyn Fn(u32) -> bool>) = match &state.model {
        ModelKind::Qwen3(_) => {
            if let Some(bits) = tq_bits {
                tracing::info!(
                    "[local-mlx-native] TurboQuant KV: TQ{bits} (activate_at={tq_activate_at}, max_kv_window {max_kv_tokens})"
                );
            }
            let c = (0..n_layers)
                .map(|_| {
                    Some(if let Some(bits) = tq_bits {
                        KvCache::turboquant_with_max(
                            bits, head_dim, n_kv_heads, max_kv_tokens, tq_activate_at,
                        )
                    } else {
                        KvCache::fp16_with_max(max_kv_tokens)
                    })
                })
                .collect();
            // Qwen3 stop tokens.
            const QWEN3_IM_END: u32 = 151645;
            const QWEN3_ENDOFTEXT: u32 = 151643;
            (
                c,
                Box::new(|t: u32| t == QWEN3_IM_END || t == QWEN3_ENDOFTEXT),
            )
        }
        ModelKind::Llama(m) => {
            if let Some(bits) = tq_bits {
                tracing::info!(
                    "[local-mlx-native] TurboQuant KV: TQ{bits} (activate_at={tq_activate_at}, max_kv_window {max_kv_tokens})"
                );
            }
            let c = (0..n_layers)
                .map(|_| {
                    Some(if let Some(bits) = tq_bits {
                        KvCache::turboquant_with_max(
                            bits, head_dim, n_kv_heads, max_kv_tokens, tq_activate_at,
                        )
                    } else {
                        KvCache::fp16_with_max(max_kv_tokens)
                    })
                })
                .collect();
            // Llama stop tokens: config-driven EOS plus the well-known Llama-3
            // chat terminators (<|eot_id|>=128009, <|end_of_text|>=128001) so
            // base/Instruct/agentic variants all behave correctly without
            // tokenizer round-trips. Custom EOS values (e.g. Nesso 128256)
            // come from `args.eos_token_id`.
            let cfg_eos = m.args.eos_token_id;
            (
                c,
                Box::new(move |t: u32| t == cfg_eos || t == 128_001 || t == 128_009),
            )
        }
        ModelKind::Gemma2(m) => {
            if tq_bits.is_some() {
                tracing::warn!(
                    "[local-mlx-native] kv_cache_bits ignored for Gemma-2 (TurboQuant KV path not wired for this architecture)"
                );
            }
            let c = (0..n_layers)
                .map(|_| Some(KvCache::fp16_with_max(max_kv_tokens)))
                .collect();
            let eos_ids: Vec<u32> = m.args.eos_token_ids.clone();
            let predicate: Box<dyn Fn(u32) -> bool> = if eos_ids.is_empty() {
                Box::new(|_| false)
            } else {
                Box::new(move |t: u32| eos_ids.contains(&t))
            };
            (c, predicate)
        }
        ModelKind::Gemma3(m) => {
            if tq_bits.is_some() {
                tracing::warn!(
                    "[local-mlx-native] kv_cache_bits ignored for Gemma-3 (TurboQuant path \
                     not yet wired); hybrid layers use unified FP16 KV cap, sliding via mask only"
                );
            }
            let c = m.make_caches(max_kv_tokens);
            let eos_ids: Vec<u32> = m.args.eos_token_ids.clone();
            let predicate: Box<dyn Fn(u32) -> bool> = if eos_ids.is_empty() {
                Box::new(|_| false)
            } else {
                Box::new(move |t: u32| eos_ids.contains(&t))
            };
            (c, predicate)
        }
        ModelKind::Mamba2(m) => {
            if tq_bits.is_some() {
                tracing::warn!(
                    "[local-mlx-native] kv_cache_bits ignored for Mamba-2 (fixed-size SSM state)"
                );
            }
            let c = m.make_cache();
            let stops = mamba_stop_token_ids(&m.args.stop_token_ids(), tokenizer);
            (c, Box::new(move |t: u32| stops.contains(&t)))
        }
        ModelKind::FalconMamba(m) => {
            if tq_bits.is_some() {
                tracing::warn!(
                    "[local-mlx-native] kv_cache_bits ignored for Falcon-Mamba (fixed-size SSM state)"
                );
            }
            let c = m.make_cache();
            let stops = mamba_stop_token_ids(&[], tokenizer);
            (c, Box::new(move |t: u32| stops.contains(&t)))
        }
        ModelKind::BonsaiQ1(b) => {
            if tq_bits.is_some() {
                tracing::warn!(
                    "[local-mlx-native] kv_cache_bits ignored for Bonsai-Q1 (TurboQuant KV path not wired for this architecture)"
                );
            }
            let c = (0..n_layers)
                .map(|_| Some(KvCache::fp16_with_max(max_kv_tokens)))
                .collect();
            let eos_ids = b.eos_token_ids.clone();
            let predicate: Box<dyn Fn(u32) -> bool> = if eos_ids.is_empty() {
                Box::new(|_| false)
            } else {
                Box::new(move |t: u32| eos_ids.contains(&t))
            };
            (c, predicate)
        }
    };

    let mut rope_offset = 0usize;

    use super::mlx_lm::models::qwen3::ModelInput;
    use mlx_rs::module::Module;

    let decode_temperature = gen_opt
        .temperature
        .map(|t| t.clamp(0.0_f32, 4.0_f32))
        .unwrap_or_else(|| match &state.model {
            ModelKind::Gemma2(_) | ModelKind::Gemma3(_) | ModelKind::BonsaiQ1(_) => 0.65_f32,
            _ => 0.0_f32,
        });
    let decode_repetition_penalty = gen_opt
        .repetition_penalty
        .map(|p| p.clamp(1.0_f32, 2.0_f32))
        .unwrap_or_else(|| match &state.model {
            ModelKind::Gemma2(_) | ModelKind::Gemma3(_) | ModelKind::BonsaiQ1(_) => 1.15_f32,
            _ => 1.0_f32,
        });
    let mut recent_decode_ids: Vec<u32> = Vec::new();

    let max_tokens: usize = max_new_tokens;
    let mut buffer: Vec<Array> = Vec::new();
    let mut hit_stop = false;
    let mut generated_count = 0usize;

    let rss_start = rss_mib();
    let (mlx_a0, mlx_c0, _) = mlx_mem_mib();
    tracing::info!(
        "[local-mlx-native][mem] generate start — rss={:.0} MiB | mlx active={:.0} cache={:.0} MiB | prompt={} max_new={}, temperature={}, repetition_penalty={}",
        rss_start,
        mlx_a0,
        mlx_c0,
        prompt.len(),
        max_new_tokens,
        decode_temperature,
        decode_repetition_penalty
    );

    let scan = super::mlx_lm::models::mamba2::SequentialScan;

    // Prefill — feed full prompt, take logits at last position.
    let logits = match &mut state.model {
        ModelKind::Qwen3(m) => {
            let prefill_input = ModelInput {
                inputs: &prompt_tokens,
                mask: None,
                cache: &mut cache,
                rope_offset,
            };
            m.forward(prefill_input)
                .map_err(|e| anyhow::anyhow!("prefill forward failed: {e:?}"))?
        }
        ModelKind::Llama(m) => {
            let prefill_input = ModelInput {
                inputs: &prompt_tokens,
                mask: None,
                cache: &mut cache,
                rope_offset,
            };
            m.forward(prefill_input)
                .map_err(|e| anyhow::anyhow!("prefill forward failed: {e:?}"))?
        }
        ModelKind::Gemma2(m) => {
            let prefill_input = ModelInput {
                inputs: &prompt_tokens,
                mask: None,
                cache: &mut cache,
                rope_offset,
            };
            m.forward(prefill_input)
                .map_err(|e| anyhow::anyhow!("prefill forward failed: {e:?}"))?
        }
        ModelKind::Gemma3(m) => {
            let prefill_input = ModelInput {
                inputs: &prompt_tokens,
                mask: None,
                cache: &mut cache,
                rope_offset,
            };
            m.forward(prefill_input)
                .map_err(|e| anyhow::anyhow!("prefill forward failed: {e:?}"))?
        }
        ModelKind::Mamba2(m) => m
            .forward(&prompt_tokens, &mut cache, &scan)
            .map_err(|e| anyhow::anyhow!("prefill forward failed: {e:?}"))?,
        ModelKind::FalconMamba(m) => m
            .forward(&prompt_tokens, &mut cache)
            .map_err(|e| anyhow::anyhow!("prefill forward failed: {e:?}"))?,
        ModelKind::BonsaiQ1(b) => b
            .gpu
            .forward_all_logits_native(&prompt_tokens, &mut cache, rope_offset)
            .map_err(|e| anyhow::anyhow!("prefill forward failed: {e:?}"))?,
    };
    rope_offset += prompt.len();
    eval_all_caches(&mut cache).map_err(|e| anyhow::anyhow!("prefill cache eval failed: {e:?}"))?;
    let last_logits = logits.index((.., -1, ..));
    eval(&[last_logits.clone()]).map_err(|e| anyhow::anyhow!("prefill logits eval failed: {e:?}"))?;
    let mut next_token = sample_decode_token_id(
        &last_logits,
        decode_temperature,
        decode_repetition_penalty,
        &recent_decode_ids,
    )?;
    eval(&[next_token.clone()]).map_err(|e| anyhow::anyhow!("prefill token eval failed: {e:?}"))?;
    let first_id = next_token.item::<u32>();
    mlx_recent_decode_push(
        &mut recent_decode_ids,
        first_id,
        MLX_REPEN_DECODE_WINDOW,
    );
    if is_stop(first_id) {
        hit_stop = true;
    } else {
        buffer.push(next_token.clone());
        generated_count += 1;
    }

    {
        let (ma, mc, mp) = mlx_mem_mib();
        tracing::info!(
            "[local-mlx-native][mem] after prefill — rss={:.0}(Δ{:.0}) | mlx active={:.0} cache={:.0} peak={:.0} MiB",
            rss_mib(),
            rss_mib() - rss_start,
            ma,
            mc,
            mp
        );
    }

    // Decode operates on single tokens — the prefill's working-set pool
    // (often >10 GB after a 2k-token prompt) is dead weight from here on.
    // Drop MLX's cached buffers back to the OS so RSS doesn't carry them
    // through every decode step.
    unsafe {
        mlx_sys::mlx_clear_cache();
    }

    for _step in 1..max_tokens {
        if hit_stop {
            break;
        }
        let inputs = next_token
            .reshape(&[1, 1])
            .map_err(|e| anyhow::anyhow!("reshape failed: {e:?}"))?;
        let logits = match &mut state.model {
            ModelKind::Qwen3(m) => {
                let decode_input = ModelInput {
                    inputs: &inputs,
                    mask: None,
                    cache: &mut cache,
                    rope_offset,
                };
                m.forward(decode_input)
                    .map_err(|e| anyhow::anyhow!("decode forward failed: {e:?}"))?
            }
            ModelKind::Llama(m) => {
                let decode_input = ModelInput {
                    inputs: &inputs,
                    mask: None,
                    cache: &mut cache,
                    rope_offset,
                };
                m.forward(decode_input)
                    .map_err(|e| anyhow::anyhow!("decode forward failed: {e:?}"))?
            }
            ModelKind::Gemma2(m) => {
                let decode_input = ModelInput {
                    inputs: &inputs,
                    mask: None,
                    cache: &mut cache,
                    rope_offset,
                };
                m.forward(decode_input)
                    .map_err(|e| anyhow::anyhow!("decode forward failed: {e:?}"))?
            }
            ModelKind::Gemma3(m) => {
                let decode_input = ModelInput {
                    inputs: &inputs,
                    mask: None,
                    cache: &mut cache,
                    rope_offset,
                };
                m.forward(decode_input)
                    .map_err(|e| anyhow::anyhow!("decode forward failed: {e:?}"))?
            }
            ModelKind::Mamba2(m) => m
                .forward(&inputs, &mut cache, &scan)
                .map_err(|e| anyhow::anyhow!("decode forward failed: {e:?}"))?,
            ModelKind::FalconMamba(m) => m
                .forward(&inputs, &mut cache)
                .map_err(|e| anyhow::anyhow!("decode forward failed: {e:?}"))?,
            ModelKind::BonsaiQ1(b) => b
                .gpu
                .forward_all_logits_native(&inputs, &mut cache, rope_offset)
                .map_err(|e| anyhow::anyhow!("decode forward failed: {e:?}"))?,
        };
        rope_offset += 1;
        eval_all_caches(&mut cache).map_err(|e| anyhow::anyhow!("decode cache eval failed: {e:?}"))?;
        let last_logits = logits.index((.., -1, ..));
        eval(&[last_logits.clone()]).map_err(|e| anyhow::anyhow!("decode logits eval failed: {e:?}"))?;
        let y = sample_decode_token_id(
            &last_logits,
            decode_temperature,
            decode_repetition_penalty,
            &recent_decode_ids,
        )?;
        eval(&[y.clone()]).map_err(|e| anyhow::anyhow!("decode token eval failed: {e:?}"))?;
        let token_id = y.item::<u32>();
        mlx_recent_decode_push(
            &mut recent_decode_ids,
            token_id,
            MLX_REPEN_DECODE_WINDOW,
        );
        next_token = y;

        if is_stop(token_id) {
            hit_stop = true;
            break;
        }
        buffer.push(next_token.clone());
        generated_count += 1;

        let should_flush = buffer.len() >= 20;
        if should_flush {
            eval(&buffer).map_err(|e| anyhow::anyhow!("eval failed: {e:?}"))?;
            if generated_count <= 20 {
                let (ma, mc, _) = mlx_mem_mib();
                tracing::info!(
                    "[local-mlx-native][mem] decode step {} — rss={:.0}(Δ{:.0}) | mlx active={:.0} cache={:.0} MiB",
                    generated_count,
                    rss_mib(),
                    rss_mib() - rss_start,
                    ma,
                    mc
                );
            }
            let slice: Vec<u32> = buffer.drain(..).map(|t| t.item::<u32>()).collect();
            if generated_count <= 20 {
                let decoded_preview = tokenizer.decode(&slice, false).unwrap_or_default();
                tracing::info!(
                    "[local-mlx-native] first {} token ids: {:?} → {:?}",
                    slice.len(),
                    slice,
                    decoded_preview
                );
            }
            let text = tokenizer
                .decode(&slice, true)
                .map_err(|e| anyhow::anyhow!("decode failed: {e:?}"))?;
            if !text.is_empty() && tx.blocking_send(text).is_err() {
                mlx_release_after_turn();
                return Ok(());
            }
        }
    }
    if hit_stop {
        mlx_log_generate_done("stopped (eos/im_end)", rss_start, generated_count);
        mlx_release_after_turn();
        return Ok(());
    }
    if !buffer.is_empty() {
        eval(&buffer).map_err(|e| anyhow::anyhow!("final eval failed: {e:?}"))?;
        let slice: Vec<u32> = buffer.drain(..).map(|t| t.item::<u32>()).collect();
        let text = tokenizer
            .decode(&slice, true)
            .map_err(|e| anyhow::anyhow!("final decode failed: {e:?}"))?;
        if !text.is_empty() {
            let _ = tx.blocking_send(text);
        }
    }

    mlx_log_generate_done("completed", rss_start, generated_count);
    mlx_release_after_turn();

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum Arch {
    Qwen3,
    Llama,
    Gemma2,
    Gemma3,
    Mamba2,
    FalconMamba,
    BonsaiQ1,
}

/// Strip the same multimodal prefix as [`load_gemma3_any`] / gemma3 model loader.
fn gemma3_loader_strip_prefix(key: &str) -> &str {
    key.strip_prefix("language_model.").unwrap_or(key)
}

/// True when weight files list a standalone `lm_head` (`lm_head.weight`, quant scales, etc.).
/// Used to override `tie_word_embeddings` for QAT checkpoints that ship `lm_head` but omit
/// `tie_word_embeddings: false` in `config.json`.
fn gemma3_safetensors_has_lm_head(model_dir: &Path) -> anyhow::Result<bool> {
    let index = model_dir.join("model.safetensors.index.json");
    if index.exists() {
        let json = std::fs::read_to_string(&index)
            .map_err(|e| anyhow::anyhow!("read gemma3 index: {e}"))?;
        let map: super::mlx_lm::models::gemma3::WeightMap = serde_json::from_str(&json)
            .map_err(|e| anyhow::anyhow!("parse gemma3 index: {e}"))?;
        for raw in map.weight_map.keys() {
            let k = gemma3_loader_strip_prefix(raw);
            if k == "lm_head" || k.starts_with("lm_head.") {
                return Ok(true);
            }
        }
        return Ok(false);
    }
    let single = model_dir.join("model.safetensors");
    if single.exists() {
        let loaded = mlx_rs::Array::load_safetensors(&single)
            .map_err(|e| anyhow::anyhow!("load_safetensors keys: {e:?}"))?;
        for raw in loaded.keys() {
            let k = gemma3_loader_strip_prefix(raw.as_str());
            if k == "lm_head" || k.starts_with("lm_head.") {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Load a Gemma-3 [`Model`]. Handles three layouts uniformly:
///
/// 1. Plain bf16 / f32 checkpoint — direct `load_safetensors`.
/// 2. 4-bit / 8-bit uniform quantization (`config.quantization.bits + group_size`
///    at the top level) — runs `nn::quantize` first so the param tree exposes
///    `.inner.{weight,bias}` + `.scales` + `.biases` slots, then maps
///    safetensors keys onto them (including the `language_model.` prefix strip
///    for multimodal Gemma-3 4B/12B checkpoints).
/// 3. `QuantizedEmbedding` workaround — mlx-rs 0.25.3 leaves the
///    `inner/scales/biases` fields without `#[param]` annotations, so
///    `parameters_mut()` misses them. We capture them by raw key and apply
///    directly into the struct (same pattern as `load_qwen3_any`).
fn load_gemma3_any(model_dir: &Path) -> anyhow::Result<super::mlx_lm::models::gemma3::Model> {
    use super::mlx_lm::models::gemma3::{get_gemma3_model_args, Model};
    use mlx_rs::module::{ModuleParameters, ModuleParametersExt};
    use std::collections::HashSet;

    let mut args = get_gemma3_model_args(model_dir)
        .map_err(|e| anyhow::anyhow!("read gemma3 args failed: {e:?}"))?;

    // Some checkpoints (e.g. `mlx-community/gemma-3-1b-it-qat-4bit`) ship a
    // separate quantised `lm_head` even though their `config.json` omits
    // `tie_word_embeddings` (so the HF default of `true` would otherwise
    // make us drop those weights and reuse `embed_tokens.as_linear`, which
    // is the WRONG matrix for the output projection after independent
    // QAT — empirically yields gibberish logits).
    //
    // Detection: scan the safetensors index (or single-file weights map)
    // for any `lm_head.weight` key, stripping the multimodal prefix. If
    // present, force `tie_word_embeddings = false` so the model allocates
    // an `lm_head` slot to receive those weights.
    let has_lm_head = gemma3_safetensors_has_lm_head(model_dir).unwrap_or(false);
    if has_lm_head && args.tie_word_embeddings {
        tracing::info!(
            "[local-mlx-native] gemma3: `lm_head.weight` present in safetensors — \
             overriding `tie_word_embeddings = false` (config default was true)"
        );
        args.tie_word_embeddings = false;
    }

    let model = Model::new(args).map_err(|e| anyhow::anyhow!("Model::new failed: {e:?}"))?;

    // Inspect for quantization. Gemma-3 4-bit checkpoints from mlx-community
    // (`gemma-3-*-it-4bit`) ship a top-level `quantization: {bits, group_size}`.
    // Some wrappers also expose `quantization_config` with the same shape.
    let cfg_path = model_dir.join("config.json");
    let raw = std::fs::read_to_string(&cfg_path)
        .map_err(|e| anyhow::anyhow!("read config.json failed: {e}"))?;
    let cfg: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("parse config.json failed: {e}"))?;
    let quant = cfg
        .get("quantization")
        .or_else(|| cfg.get("quantization_config"))
        .and_then(|q| {
            let g = q.get("group_size")?.as_i64()? as i32;
            let b = q.get("bits")?.as_i64()? as i32;
            Some((g, b))
        });

    let mut model = if let Some((group_size, bits)) = quant {
        tracing::info!(
            "[local-mlx-native] quantizing Gemma-3 layers: group_size={group_size}, bits={bits}"
        );
        let m = mlx_rs::nn::quantize(model, Some(group_size), Some(bits))
            .map_err(|e| anyhow::anyhow!("nn::quantize failed: {e:?}"))?;
        m.eval()
            .map_err(|e| anyhow::anyhow!("post-quantize eval failed: {e:?}"))?;
        m
    } else {
        model
    };

    let mut shard_files: Vec<std::path::PathBuf> = Vec::new();
    let weights_index = model_dir.join("model.safetensors.index.json");
    if weights_index.exists() {
        let json = std::fs::read_to_string(&weights_index)
            .map_err(|e| anyhow::anyhow!("read index failed: {e}"))?;
        let map: super::mlx_lm::models::gemma3::WeightMap = serde_json::from_str(&json)
            .map_err(|e| anyhow::anyhow!("parse index failed: {e}"))?;
        let files: HashSet<&String> = map.weight_map.values().collect();
        for f in files {
            shard_files.push(model_dir.join(f));
        }
    } else {
        let single = model_dir.join("model.safetensors");
        if !single.exists() {
            anyhow::bail!(
                "no model.safetensors.index.json or model.safetensors in {}",
                model_dir.display()
            );
        }
        shard_files.push(single);
    }

    let is_quant = quant.is_some();
    let mut total_loaded = 0usize;
    let mut total_missed = 0usize;
    let mut unmatched_samples: Vec<String> = Vec::new();

    let mut embed_weight: Option<mlx_rs::Array> = None;
    let mut embed_scales: Option<mlx_rs::Array> = None;
    let mut embed_biases: Option<mlx_rs::Array> = None;

    let strip = |key: &str| -> String {
        key.strip_prefix("language_model.").unwrap_or(key).to_string()
    };

    let mut unfilled_slots: std::collections::HashSet<String> = {
        let snap = model.parameters_mut().flatten();
        snap.keys().map(|k| k.to_string()).collect()
    };
    for shard in &shard_files {
        let loaded = mlx_rs::Array::load_safetensors(shard)
            .map_err(|e| anyhow::anyhow!("read shard {}: {e:?}", shard.display()))?;
        let mut params = model.parameters_mut().flatten();
        for (raw_key, value) in loaded {
            let key = strip(raw_key.as_str());
            // QuantizedEmbedding workaround (same as qwen3/llama).
            match key.as_str() {
                "model.embed_tokens.weight" => {
                    embed_weight = Some(value);
                    total_loaded += 1;
                    continue;
                }
                "model.embed_tokens.scales" => {
                    embed_scales = Some(value);
                    total_loaded += 1;
                    continue;
                }
                "model.embed_tokens.biases" => {
                    embed_biases = Some(value);
                    total_loaded += 1;
                    continue;
                }
                _ => {}
            }
            if let Some(slot) = params.get_mut(key.as_str()) {
                **slot = value;
                total_loaded += 1;
                unfilled_slots.remove(&key);
                continue;
            }
            if is_quant {
                // QuantizedLinear: HF stores `proj.weight` but the param tree
                // exposes it under `proj.inner.weight` after `nn::quantize`.
                if let Some(stripped) = key.strip_suffix(".weight") {
                    let remapped = format!("{stripped}.inner.weight");
                    if let Some(slot) = params.get_mut(remapped.as_str()) {
                        **slot = value;
                        total_loaded += 1;
                        unfilled_slots.remove(&remapped);
                        continue;
                    }
                }
                if let Some(stripped) = key.strip_suffix(".bias") {
                    let remapped = format!("{stripped}.inner.bias");
                    if let Some(slot) = params.get_mut(remapped.as_str()) {
                        **slot = value;
                        total_loaded += 1;
                        unfilled_slots.remove(&remapped);
                        continue;
                    }
                }
            }
            total_missed += 1;
            if unmatched_samples.len() < 5 {
                unmatched_samples.push(key);
            }
        }
    }

    if embed_weight.is_some() || embed_scales.is_some() || embed_biases.is_some() {
        match &mut model.model.embed_tokens {
            mlx_rs::quantization::MaybeQuantized::Quantized(q) => {
                if let Some(w) = embed_weight {
                    q.inner.weight.value = w;
                }
                if let Some(s) = embed_scales {
                    q.scales.value = s;
                }
                if let Some(b) = embed_biases {
                    q.biases.value = b;
                }
            }
            mlx_rs::quantization::MaybeQuantized::Original(e) => {
                if let Some(w) = embed_weight {
                    e.weight.value = w;
                }
            }
        }
    }

    tracing::info!(
        "[local-mlx-native] gemma3 safetensor load: {total_loaded} matched, {total_missed} unmatched (quantized={is_quant})"
    );
    if !unmatched_samples.is_empty() {
        tracing::warn!(
            "[local-mlx-native] gemma3 unmatched key samples: {}",
            unmatched_samples.join(", ")
        );
    }
    if !unfilled_slots.is_empty() {
        let mut samples: Vec<&String> = unfilled_slots.iter().collect();
        samples.sort();
        let preview: Vec<&str> = samples.iter().take(8).map(|s| s.as_str()).collect();
        tracing::warn!(
            "[local-mlx-native] gemma3 {} parameter slot(s) NOT populated — first few: {}",
            unfilled_slots.len(),
            preview.join(", ")
        );
    }
    if total_loaded == 0 {
        anyhow::bail!("no safetensor keys matched the Gemma-3 parameter tree");
    }

    model
        .eval()
        .map_err(|e| anyhow::anyhow!("eval after load failed: {e:?}"))?;
    Ok(model)
}

/// Load a Mamba-2 [`Model`]. Weight-population uses `load_safetensors` (no
/// post-load quantization in this build — `mlx-community/mamba2-*` ships bf16).
fn load_mamba2_any(model_dir: &Path) -> anyhow::Result<super::mlx_lm::models::mamba2::Model> {
    use super::mlx_lm::models::mamba2::load_mamba2_model;
    load_mamba2_model(model_dir)
        .map_err(|e| anyhow::anyhow!("load_mamba2_model failed: {e:?}"))
}

/// Load a Falcon-Mamba (Mamba-1) [`Model`]. The loader inside the model module
/// handles both plain and quantised (`mlx-community/falcon-mamba-7b-{4,8}bit`)
/// checkpoints, including the `QuantizedEmbedding` workaround.
fn load_falcon_mamba_any(
    model_dir: &Path,
) -> anyhow::Result<super::mlx_lm::models::falcon_mamba::Model> {
    use super::mlx_lm::models::falcon_mamba::load_falcon_mamba_model;
    load_falcon_mamba_model(model_dir)
        .map_err(|e| anyhow::anyhow!("load_falcon_mamba_model failed: {e:?}"))
}

/// Load a Llama [`Model`]. Mirrors `load_qwen3_any`'s logging/quantization
/// support: honours `config.json::quantization` for mlx-community 4-bit
/// variants and surfaces unmatched safetensors keys instead of silently
/// dropping them.
fn load_llama_any(model_dir: &Path) -> anyhow::Result<super::mlx_lm::models::llama::Model> {
    use super::mlx_lm::models::llama::{get_llama_model_args, Model, WeightMap};
    use mlx_rs::module::{ModuleParameters, ModuleParametersExt};
    use std::collections::HashSet;

    let args = get_llama_model_args(model_dir)
        .map_err(|e| anyhow::anyhow!("read llama args failed: {e:?}"))?;
    let model = Model::new(args).map_err(|e| anyhow::anyhow!("Model::new failed: {e:?}"))?;

    let cfg_path = model_dir.join("config.json");
    let raw = std::fs::read_to_string(&cfg_path)
        .map_err(|e| anyhow::anyhow!("read config.json failed: {e}"))?;
    let cfg: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("parse config.json failed: {e}"))?;
    let quant = cfg.get("quantization").and_then(|q| {
        let g = q.get("group_size")?.as_i64()? as i32;
        let b = q.get("bits")?.as_i64()? as i32;
        Some((g, b))
    });

    let mut model = if let Some((group_size, bits)) = quant {
        tracing::info!(
            "[local-mlx-native] quantizing Llama layers: group_size={group_size}, bits={bits}"
        );
        let m = mlx_rs::nn::quantize(model, Some(group_size), Some(bits))
            .map_err(|e| anyhow::anyhow!("nn::quantize failed: {e:?}"))?;
        m.eval()
            .map_err(|e| anyhow::anyhow!("post-quantize eval failed: {e:?}"))?;
        m
    } else {
        model
    };

    let mut shard_files: Vec<std::path::PathBuf> = Vec::new();
    let weights_index = model_dir.join("model.safetensors.index.json");
    if weights_index.exists() {
        let json = std::fs::read_to_string(&weights_index)
            .map_err(|e| anyhow::anyhow!("read index failed: {e}"))?;
        let map: WeightMap = serde_json::from_str(&json)
            .map_err(|e| anyhow::anyhow!("parse index failed: {e}"))?;
        let files: HashSet<&String> = map.weight_map.values().collect();
        for f in files {
            shard_files.push(model_dir.join(f));
        }
    } else {
        let single = model_dir.join("model.safetensors");
        if !single.exists() {
            anyhow::bail!(
                "no model.safetensors.index.json or model.safetensors in {}",
                model_dir.display()
            );
        }
        shard_files.push(single);
    }

    let is_quant = quant.is_some();
    let mut total_loaded = 0usize;
    let mut total_missed = 0usize;
    let mut unmatched_samples: Vec<String> = Vec::new();

    // Same QuantizedEmbedding workaround as qwen3 (mlx-rs 0.25.3 leaves the
    // inner/scales/biases fields without `#[param]`, so they don't appear in
    // parameters_mut(); capture directly).
    let mut embed_weight: Option<mlx_rs::Array> = None;
    let mut embed_scales: Option<mlx_rs::Array> = None;
    let mut embed_biases: Option<mlx_rs::Array> = None;

    let mut unfilled_slots: std::collections::HashSet<String> = {
        let snap = model.parameters_mut().flatten();
        snap.keys().map(|k| k.to_string()).collect()
    };
    for shard in &shard_files {
        let loaded = mlx_rs::Array::load_safetensors(shard)
            .map_err(|e| anyhow::anyhow!("read shard {}: {e:?}", shard.display()))?;
        let mut params = model.parameters_mut().flatten();
        for (raw_key, value) in loaded {
            let key = raw_key.as_str();
            match key {
                "model.embed_tokens.weight" => {
                    embed_weight = Some(value);
                    total_loaded += 1;
                    continue;
                }
                "model.embed_tokens.scales" => {
                    embed_scales = Some(value);
                    total_loaded += 1;
                    continue;
                }
                "model.embed_tokens.biases" => {
                    embed_biases = Some(value);
                    total_loaded += 1;
                    continue;
                }
                _ => {}
            }
            if let Some(slot) = params.get_mut(key) {
                **slot = value;
                total_loaded += 1;
                unfilled_slots.remove(key);
                continue;
            }
            if is_quant {
                if let Some(stripped) = key.strip_suffix(".weight") {
                    let remapped = format!("{stripped}.inner.weight");
                    if let Some(slot) = params.get_mut(remapped.as_str()) {
                        **slot = value;
                        total_loaded += 1;
                        unfilled_slots.remove(&remapped);
                        continue;
                    }
                }
                if let Some(stripped) = key.strip_suffix(".bias") {
                    let remapped = format!("{stripped}.inner.bias");
                    if let Some(slot) = params.get_mut(remapped.as_str()) {
                        **slot = value;
                        total_loaded += 1;
                        unfilled_slots.remove(&remapped);
                        continue;
                    }
                }
            }
            total_missed += 1;
            if unmatched_samples.len() < 5 {
                unmatched_samples.push(key.to_string());
            }
        }
    }

    if embed_weight.is_some() || embed_scales.is_some() || embed_biases.is_some() {
        match &mut model.model.embed_tokens {
            mlx_rs::quantization::MaybeQuantized::Quantized(q) => {
                if let Some(w) = embed_weight {
                    q.inner.weight.value = w;
                }
                if let Some(s) = embed_scales {
                    q.scales.value = s;
                }
                if let Some(b) = embed_biases {
                    q.biases.value = b;
                }
            }
            mlx_rs::quantization::MaybeQuantized::Original(e) => {
                if let Some(w) = embed_weight {
                    e.weight.value = w;
                }
            }
        }
    }

    tracing::info!(
        "[local-mlx-native] llama safetensor load: {total_loaded} matched, {total_missed} unmatched (quantized={is_quant})"
    );
    if !unmatched_samples.is_empty() {
        tracing::warn!(
            "[local-mlx-native] llama unmatched key samples: {}",
            unmatched_samples.join(", ")
        );
    }
    if !unfilled_slots.is_empty() {
        let mut samples: Vec<&String> = unfilled_slots.iter().collect();
        samples.sort();
        let preview: Vec<&str> = samples.iter().take(8).map(|s| s.as_str()).collect();
        tracing::warn!(
            "[local-mlx-native] llama {} parameter slot(s) NOT populated — first few: {}",
            unfilled_slots.len(),
            preview.join(", ")
        );
    }
    if total_loaded == 0 {
        anyhow::bail!("no safetensor keys matched the Llama parameter tree");
    }

    model
        .eval()
        .map_err(|e| anyhow::anyhow!("eval after load failed: {e:?}"))?;
    Ok(model)
}

/// Load a Qwen3 `Model`, applying `nn::quantize` when `config.json` declares a
/// `quantization` block (mlx-community 4-bit / 8-bit variants).
fn load_qwen3_any(model_dir: &Path) -> anyhow::Result<super::mlx_lm::models::qwen3::Model> {
    use super::mlx_lm::models::qwen3::{get_qwen3_model_args, Model, WeightMap};
    use mlx_rs::module::{ModuleParameters, ModuleParametersExt};
    use std::collections::HashSet;

    let args = get_qwen3_model_args(model_dir)
        .map_err(|e| anyhow::anyhow!("read model args failed: {e:?}"))?;
    let model = Model::new(args).map_err(|e| anyhow::anyhow!("Model::new failed: {e:?}"))?;

    // Inspect config.json for an optional `quantization: { group_size, bits }`.
    let cfg_path = model_dir.join("config.json");
    let raw = std::fs::read_to_string(&cfg_path)
        .map_err(|e| anyhow::anyhow!("read config.json failed: {e}"))?;
    let cfg: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|e| anyhow::anyhow!("parse config.json failed: {e}"))?;
    let quant = cfg.get("quantization").and_then(|q| {
        let g = q.get("group_size")?.as_i64()? as i32;
        let b = q.get("bits")?.as_i64()? as i32;
        Some((g, b))
    });

    let mut model = if let Some((group_size, bits)) = quant {
        tracing::info!(
            "[local-mlx-native] quantizing Qwen3 layers: group_size={group_size}, bits={bits}"
        );
        let m = mlx_rs::nn::quantize(model, Some(group_size), Some(bits))
            .map_err(|e| anyhow::anyhow!("nn::quantize failed: {e:?}"))?;
        m.eval()
            .map_err(|e| anyhow::anyhow!("post-quantize eval failed: {e:?}"))?;
        m
    } else {
        model
    };

    // Collect every safetensor shard we need to read.
    let mut shard_files: Vec<std::path::PathBuf> = Vec::new();
    let weights_index = model_dir.join("model.safetensors.index.json");
    if weights_index.exists() {
        let json = std::fs::read_to_string(&weights_index)
            .map_err(|e| anyhow::anyhow!("read index failed: {e}"))?;
        let map: WeightMap = serde_json::from_str(&json)
            .map_err(|e| anyhow::anyhow!("parse index failed: {e}"))?;
        let files: HashSet<&String> = map.weight_map.values().collect();
        for f in files {
            shard_files.push(model_dir.join(f));
        }
    } else {
        let single = model_dir.join("model.safetensors");
        if !single.exists() {
            anyhow::bail!(
                "no model.safetensors.index.json or model.safetensors in {}",
                model_dir.display()
            );
        }
        shard_files.push(single);
    }

    let n_layers = model.args.num_hidden_layers as usize;
    let _ = n_layers;

    let is_quant = quant.is_some();
    let mut total_loaded = 0usize;
    let mut total_missed = 0usize;
    let mut unmatched_samples: Vec<String> = Vec::new();

    // mlx-rs 0.25.3 `QuantizedEmbedding` is missing `#[param]` annotations on
    // its `inner` / `scales` / `biases` fields, so `parameters_mut().flatten()`
    // returns nothing for `model.embed_tokens.*`. Capture those tensors here
    // and apply them directly below.
    let mut embed_weight: Option<mlx_rs::Array> = None;
    let mut embed_scales: Option<mlx_rs::Array> = None;
    let mut embed_biases: Option<mlx_rs::Array> = None;

    let mut unfilled_slots: std::collections::HashSet<String> = {
        let snap = model.parameters_mut().flatten();
        snap.keys().map(|k| k.to_string()).collect()
    };
    for shard in &shard_files {
        let loaded = mlx_rs::Array::load_safetensors(shard)
            .map_err(|e| anyhow::anyhow!("read shard {}: {e:?}", shard.display()))?;
        let mut params = model.parameters_mut().flatten();
        for (raw_key, value) in loaded {
            let key = raw_key.as_str();

            match key {
                "model.embed_tokens.weight" => {
                    embed_weight = Some(value);
                    total_loaded += 1;
                    continue;
                }
                "model.embed_tokens.scales" => {
                    embed_scales = Some(value);
                    total_loaded += 1;
                    continue;
                }
                "model.embed_tokens.biases" => {
                    embed_biases = Some(value);
                    total_loaded += 1;
                    continue;
                }
                _ => {}
            }

            if let Some(slot) = params.get_mut(key) {
                **slot = value;
                total_loaded += 1;
                unfilled_slots.remove(key);
                continue;
            }
            if is_quant {
                if let Some(stripped) = key.strip_suffix(".weight") {
                    let remapped = format!("{stripped}.inner.weight");
                    if let Some(slot) = params.get_mut(remapped.as_str()) {
                        **slot = value;
                        total_loaded += 1;
                        unfilled_slots.remove(&remapped);
                        continue;
                    }
                }
                if let Some(stripped) = key.strip_suffix(".bias") {
                    let remapped = format!("{stripped}.inner.bias");
                    if let Some(slot) = params.get_mut(remapped.as_str()) {
                        **slot = value;
                        total_loaded += 1;
                        unfilled_slots.remove(&remapped);
                        continue;
                    }
                }
            }
            total_missed += 1;
            if unmatched_samples.len() < 5 {
                unmatched_samples.push(key.to_string());
            }
        }
    }

    // Apply embed_tokens tensors directly into the QuantizedEmbedding struct.
    if embed_weight.is_some() || embed_scales.is_some() || embed_biases.is_some() {
        match &mut model.model.embed_tokens {
            mlx_rs::quantization::MaybeQuantized::Quantized(q) => {
                if let Some(w) = embed_weight {
                    q.inner.weight.value = w;
                }
                if let Some(s) = embed_scales {
                    q.scales.value = s;
                }
                if let Some(b) = embed_biases {
                    q.biases.value = b;
                }
                tracing::info!("[local-mlx-native] embed_tokens (QuantizedEmbedding) populated via direct mutation");
            }
            mlx_rs::quantization::MaybeQuantized::Original(e) => {
                if let Some(w) = embed_weight {
                    e.weight.value = w;
                }
                tracing::info!("[local-mlx-native] embed_tokens (Embedding) populated via direct mutation");
            }
        }
    }
    tracing::info!(
        "[local-mlx-native] safetensor load: {total_loaded} matched, {total_missed} unmatched (quantized={is_quant})"
    );
    if !unmatched_samples.is_empty() {
        tracing::warn!(
            "[local-mlx-native] sample unmatched safetensor keys: {}",
            unmatched_samples.join(", ")
        );
    }
    if !unfilled_slots.is_empty() {
        let mut samples: Vec<&String> = unfilled_slots.iter().collect();
        samples.sort();
        let preview: Vec<&str> = samples.iter().take(8).map(|s| s.as_str()).collect();
        tracing::warn!(
            "[local-mlx-native] {} model parameter slot(s) NOT populated — first few: {}",
            unfilled_slots.len(),
            preview.join(", ")
        );
    }
    if total_loaded == 0 {
        anyhow::bail!(
            "no safetensor keys matched the Qwen3 model parameter tree — \
             this build of mlx-lm may use a different layout than this model expects"
        );
    }

    model
        .eval()
        .map_err(|e| anyhow::anyhow!("eval after load failed: {e:?}"))?;

    Ok(model)
}

/// Best-effort extractor for `"model_type": "..."` that tolerates JSON which
/// `serde_json` would reject — most importantly the `Infinity` value HF writes
/// into `time_step_limit` for Mamba-2 / Mamba-Codestral checkpoints.
///
/// Tries strict `serde_json::Value` first; on failure falls back to a manual
/// scan for the `"model_type"` key. Returns `None` only when the key is truly
/// absent from the file.
fn extract_model_type(raw: &str) -> Option<String> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
        if let Some(s) = v.get("model_type").and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    let key = "\"model_type\"";
    let idx = raw.find(key)?;
    let rest = &raw[idx + key.len()..];
    let colon = rest.find(':')?;
    let after = &rest[colon + 1..];
    let quote = after.find('"')?;
    let after_quote = &after[quote + 1..];
    let end = after_quote.find('"')?;
    Some(after_quote[..end].to_string())
}

fn detect_architecture(model_id: &str, model_dir: &Path) -> anyhow::Result<Arch> {
    let lower = model_id.to_lowercase();
    // config.json::model_type is the source of truth; the id-substring check
    // below is just a hint when config.json hasn't been read yet.
    let cfg_path = model_dir.join("config.json");
    if let Ok(raw) = std::fs::read_to_string(&cfg_path) {
        if let Some(mt_raw) = extract_model_type(&raw) {
            {
                let mt = mt_raw.to_lowercase();
                if mt == "mamba2" || mt.starts_with("mamba2") {
                    return Ok(Arch::Mamba2);
                }
                // Falcon-Mamba is a Mamba-1 backbone with `use_bcdt_rms = true`
                // applied to (delta, B, C). Routed through the dedicated Rust
                // module rather than the Mamba-2 path.
                if mt == "falcon_mamba" || mt.starts_with("falcon_mamba") {
                    return Ok(Arch::FalconMamba);
                }
                // Generic Mamba-1 (`state-spaces/mamba-*`) — same forward path
                // but `use_bcdt_rms = false`. Reuse the FalconMamba loader.
                if mt == "mamba" {
                    return Ok(Arch::FalconMamba);
                }
                if mt.contains("qwen3_moe") || mt.contains("qwen3_5_moe") {
                    anyhow::bail!(
                        "Qwen3-MoE (`model_type={mt}`) is not supported by native MLX in this build — use a dense Qwen3 checkpoint or another supported architecture."
                    );
                }
                if mt.contains("qwen3_next") {
                    anyhow::bail!(
                        "Qwen3-Next (`model_type={mt}`) is not supported by native MLX in this build."
                    );
                }
                if mt.contains("qwen3") {
                    return Ok(Arch::Qwen3);
                }
                if mt == "gemma2" || mt.starts_with("gemma2") {
                    return Ok(Arch::Gemma2);
                }
                // Gemma-3 ships both `gemma3` (multimodal wrapper) and
                // `gemma3_text` (text-only) model_types; both load through
                // the same Rust path.
                if mt == "gemma3" || mt.starts_with("gemma3") {
                    return Ok(Arch::Gemma3);
                }
                // Llama covers `llama`, `llama-3`, `llama3`, Nesso, etc.
                if mt == "llama" || mt.starts_with("llama") {
                    return Ok(Arch::Llama);
                }
                if mt.contains("bonsai") {
                    return Ok(Arch::BonsaiQ1);
                }
            }
        }
    }
    if lower.contains("mamba2") {
        return Ok(Arch::Mamba2);
    }
    if lower.contains("falcon-mamba") || lower.contains("falcon_mamba") {
        return Ok(Arch::FalconMamba);
    }
    if lower.contains("qwen3_moe") || lower.contains("qwen3_5_moe") {
        anyhow::bail!(
            "no native MLX loader for `{model_id}` — Qwen3-MoE is not supported in this build."
        );
    }
    if lower.contains("qwen3_next") {
        anyhow::bail!(
            "no native MLX loader for `{model_id}` — Qwen3-Next is not supported in this build."
        );
    }
    if lower.contains("qwen3") {
        return Ok(Arch::Qwen3);
    }
    if lower.contains("gemma2") || lower.contains("gemma-2") {
        return Ok(Arch::Gemma2);
    }
    if lower.contains("gemma-3") || lower.contains("gemma3") {
        return Ok(Arch::Gemma3);
    }
    if lower.contains("llama") || lower.contains("nesso") {
        return Ok(Arch::Llama);
    }
    if lower.contains("bonsai") {
        return Ok(Arch::BonsaiQ1);
    }
    anyhow::bail!(
        "no native loader for `{}` — supported model_type values: qwen3, llama, gemma2, gemma3, mamba2, mamba, falcon_mamba, bonsai / bonsai_q1.",
        model_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ensure_installed_errors_when_missing() {
        let eng = MlxNativeEngine::new(Path::new("/nonexistent/path"), "mlx-community/Qwen3-4B-bf16", None);
        let err = eng.ensure_installed().await.unwrap_err().to_string();
        assert!(err.contains("model directory not found"), "got: {err}");
    }
}
