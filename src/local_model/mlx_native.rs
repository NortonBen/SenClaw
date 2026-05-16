//! Native MLX inference engine.
//!
//! Apple Silicon only. Uses **`mlx-rs`** plus Qwen3 weights/templates vendored in-tree (see [`super::mlx_lm`]).
//!
//! NOTE: only Qwen3 MLX checkpoints are supported by the vendored loader today.
//! Models that are not Qwen3 return an error from [`MlxNativeEngine::start`] when detected.
//!
//! KV cache: **`SteppingKeyValueCache` (FP16)** — `slice_update` + grow-by-256, sliding window.
//! Optional **`TurboQuantKeyValueCache`** when `kv_cache_bits` is set (`turboquant-rs` storage).
//! RoPE uses **`ModelInput::rope_offset`** (caller `usize`), not cache-internal position.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;

use super::runtime::{
    ChatMessage, LocalModelRuntime, RuntimeEndpoint, RuntimeHealth, RuntimeStatus,
};

/// Heavyweight cached state: Qwen3 weights + tokenizer + rendered chat
/// template. Populated by `load()` and dropped by `unload()`, so RAM can be
/// freed on demand without restarting the daemon.
struct Loaded {
    model: super::mlx_lm::models::qwen3::Model,
    tokenizer: super::mlx_lm_utils::tokenizer::Tokenizer,
    chat_template: String,
    n_layers: usize,
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
    use super::mlx_lm_utils::tokenizer::{load_model_chat_template_from_file, Tokenizer};

    detect_architecture(model_id, model_dir)?;
    let tokenizer_file = model_dir.join("tokenizer.json");
    let tokenizer_config = model_dir.join("tokenizer_config.json");
    let tokenizer = Tokenizer::from_file(&tokenizer_file)
        .map_err(|e| anyhow::anyhow!("tokenizer load failed: {e:?}"))?;
    let chat_template = load_model_chat_template_from_file(&tokenizer_config)?
        .ok_or_else(|| anyhow::anyhow!("chat template missing in tokenizer_config.json"))?;
    let model = load_qwen3_any(model_dir)
        .map_err(|e| anyhow::anyhow!("load_qwen3 failed: {e:?}"))?;
    let n_layers = model.args.num_hidden_layers as usize;
    let head_dim = model.args.head_dim;
    let n_kv_heads = model.args.num_key_value_heads;
    tracing::info!(
        "[local-mlx-native] cached state ready for {model_id} ({n_layers} layers, head_dim={head_dim}, kv_heads={n_kv_heads})"
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
    let model = &mut state.model;
    let tokenizer = &mut state.tokenizer;
    let template = state.chat_template.clone();
    let n_layers = state.n_layers;

    let settings_dir = model_dir.parent().unwrap_or(model_dir);
    let gen_opt =
        crate::gateway::ui_server::local_models::load_settings_blocking(settings_dir);

    use super::mlx_prompt::{
        strip_thinking_blocks, trim_conversation_history, MLX_MAX_HISTORY_TURNS,
    };

    // mlx-lm-utils Role has only User/Assistant. Fold System content into
    // the first user turn so the chat template still renders correctly.
    let mut convs: Vec<Conversation<TokRole, String>> = Vec::with_capacity(messages.len());
    let mut pending_system = String::new();
    for m in messages {
        match m.role {
            super::runtime::Role::System => {
                if !pending_system.is_empty() {
                    pending_system.push_str("\n\n");
                }
                pending_system.push_str(&m.content);
            }
            super::runtime::Role::User => {
                let content = if pending_system.is_empty() {
                    m.content.clone()
                } else {
                    let merged = format!("{}\n\n{}", pending_system, m.content);
                    pending_system.clear();
                    merged
                };
                convs.push(Conversation {
                    role: TokRole::User,
                    content,
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
    if !pending_system.is_empty() {
        // System-only — emit as a single user turn so the model has something to answer.
        convs.push(Conversation {
            role: TokRole::User,
            content: pending_system,
        });
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
    let args = ApplyChatTemplateArgs {
        conversations: vec![convs.into()],
        documents: None,
        model_id,
        chat_template_id: None,
        add_generation_prompt: Some(true),
        continue_final_message: None,
        enable_thinking,
    };
    let encodings = tokenizer
        .apply_chat_template_and_encode(template, args)
        .map_err(|e| anyhow::anyhow!("chat template apply failed: {e:?}"))?;
    let mut prompt: Vec<u32> = encodings
        .iter()
        .flat_map(|e| e.get_ids())
        .copied()
        .collect();
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
    if let Some(bits) = tq_bits {
        tracing::info!(
            "[local-mlx-native] TurboQuant KV: TQ{bits} (activate_at={tq_activate_at}, max_kv_window {max_kv_tokens})"
        );
    }

    let mut cache: Vec<Option<KvCache>> = (0..n_layers)
        .map(|_| {
            Some(if let Some(bits) = tq_bits {
                KvCache::turboquant_with_max(bits, head_dim, n_kv_heads, max_kv_tokens, tq_activate_at)
            } else {
                KvCache::fp16_with_max(max_kv_tokens)
            })
        })
        .collect();

    let mut rope_offset = 0usize;

    // Qwen3 stop tokens.
    const QWEN3_IM_END: u32 = 151645;
    const QWEN3_ENDOFTEXT: u32 = 151643;
    let is_stop = |t: u32| t == QWEN3_IM_END || t == QWEN3_ENDOFTEXT;

    use super::mlx_lm::models::qwen3::{sample, ModelInput};
    use mlx_rs::module::Module;

    let max_tokens: usize = max_new_tokens;
    let mut buffer: Vec<Array> = Vec::new();
    let mut hit_stop = false;
    let mut generated_count = 0usize;
    let temperature: f32 = 0.0;

    let rss_start = rss_mib();
    let (mlx_a0, mlx_c0, _) = mlx_mem_mib();
    tracing::info!(
        "[local-mlx-native][mem] generate start — rss={:.0} MiB | mlx active={:.0} cache={:.0} MiB (prompt={} tokens, max_new={})",
        rss_start,
        mlx_a0,
        mlx_c0,
        prompt.len(),
        max_new_tokens
    );

    // Prefill — feed full prompt, take logits at last position.
    let prefill_input = ModelInput {
        inputs: &prompt_tokens,
        mask: None,
        cache: &mut cache,
        rope_offset,
    };
    let logits = model
        .forward(prefill_input)
        .map_err(|e| anyhow::anyhow!("prefill forward failed: {e:?}"))?;
    rope_offset += prompt.len();
    eval_all_caches(&mut cache).map_err(|e| anyhow::anyhow!("prefill cache eval failed: {e:?}"))?;
    let last_logits = logits.index((.., -1, ..));
    eval(&[last_logits.clone()]).map_err(|e| anyhow::anyhow!("prefill logits eval failed: {e:?}"))?;
    let mut next_token = sample(&last_logits, temperature)
        .map_err(|e| anyhow::anyhow!("prefill sample failed: {e:?}"))?;
    eval(&[next_token.clone()]).map_err(|e| anyhow::anyhow!("prefill token eval failed: {e:?}"))?;
    buffer.push(next_token.clone());
    generated_count += 1;

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

    for _step in 1..max_tokens {
        let inputs = next_token
            .reshape(&[1, 1])
            .map_err(|e| anyhow::anyhow!("reshape failed: {e:?}"))?;
        let decode_input = ModelInput {
            inputs: &inputs,
            mask: None,
            cache: &mut cache,
            rope_offset,
        };
        let logits = model
            .forward(decode_input)
            .map_err(|e| anyhow::anyhow!("decode forward failed: {e:?}"))?;
        rope_offset += 1;
        eval_all_caches(&mut cache).map_err(|e| anyhow::anyhow!("decode cache eval failed: {e:?}"))?;
        let last_logits = logits.index((.., -1, ..));
        eval(&[last_logits.clone()]).map_err(|e| anyhow::anyhow!("decode logits eval failed: {e:?}"))?;
        let y = sample(&last_logits, temperature)
            .map_err(|e| anyhow::anyhow!("decode sample failed: {e:?}"))?;
        next_token = y;
        buffer.push(next_token.clone());
        generated_count += 1;

        if buffer.len() % 20 == 0 {
            eval(&buffer).map_err(|e| anyhow::anyhow!("eval failed: {e:?}"))?;
            let (ma, mc, _) = mlx_mem_mib();
            tracing::info!(
                "[local-mlx-native][mem] decode step {} — rss={:.0}(Δ{:.0}) | mlx active={:.0} cache={:.0} MiB",
                generated_count,
                rss_mib(),
                rss_mib() - rss_start,
                ma,
                mc
            );
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
            if let Some(&stop) = slice.iter().find(|&&t| is_stop(t)) {
                let cut: Vec<u32> = slice.iter().copied().take_while(|&t| t != stop).collect();
                let text = tokenizer
                    .decode(&cut, true)
                    .map_err(|e| anyhow::anyhow!("decode failed: {e:?}"))?;
                if !text.is_empty() {
                    let _ = tx.blocking_send(text);
                }
                hit_stop = true;
                break;
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

fn detect_architecture(model_id: &str, model_dir: &Path) -> anyhow::Result<Arch> {
    let lower = model_id.to_lowercase();
    if lower.contains("qwen3") {
        return Ok(Arch::Qwen3);
    }
    let cfg_path = model_dir.join("config.json");
    if let Ok(raw) = std::fs::read_to_string(&cfg_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(mt) = v.get("model_type").and_then(|x| x.as_str()) {
                if mt.contains("qwen3") {
                    return Ok(Arch::Qwen3);
                }
            }
        }
    }
    anyhow::bail!(
        "no native Qwen3 loader for `{}` — only Qwen3 MLX checkpoints are supported in this build.",
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
