//! Native MLX inference engine.
//!
//! Apple Silicon only. Uses **`mlx-rs`** plus model weights/templates vendored in-tree (see [`super::mlx_lm`]).
//!
//! Supported architectures:
//! - **Qwen3** (`model_type = "qwen3"`) — standard GQA transformer.
//!
//! KV cache:
//! - **Default:** [`higgs_cache::SteppingKeyValueCache`] dense mode — pre-allocated 256-slot stepping
//!   buffer, `mlx_slice_update` writes, FP16 on GPU. Avoids per-token concat chain.
//! - **`kv_cache_bits = 3 | 4` in `settings.json`:** Higgs GPU TurboQuant via
//!   [`higgs_cache::SteppingKeyValueCache`] + Metal kernels. Prefill stays dense (fast SDPA);
//!   first decode bulk-quantizes to packed codes on GPU. **3** = TQ3 (key=2bit, value=3bit);
//!   **4** = TQ4 (key=3bit, value=4bit). No feature flag required — always available with `local-mlx`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;

use super::runtime::{
    LocalModelRuntime, RuntimeEndpoint, RuntimeHealth, RuntimeStatus,
};

/// Which model architecture is loaded.
enum LoadedModel {
    Qwen3(super::mlx_lm::models::qwen3::Model),
    Gemma4(super::mlx_lm::models::gemma4::Gemma4CausalLM),
    Qwen3_5(super::mlx_lm::models::qwen3_5::Model),
}

/// Detected architecture for dispatch in `load_state`.
enum Arch {
    Qwen3,
    Gemma4,
    Qwen3_5,
}

/// Heavyweight cached state. Populated by `load()` and dropped by `unload()`.
struct Loaded {
    model: LoadedModel,
    tokenizer: super::mlx_lm_utils::tokenizer::Tokenizer,
    chat_template: String,
    n_layers: usize,
    /// From `tokenizer_config.json` / `config.json` when weights were loaded (`model_max_length`, etc.).
    model_context_length: Option<u32>,
}

/// In-process MLX inference engine. Caches the loaded model so subsequent
/// chats reuse weights instead of re-reading safetensors every call.
pub struct MlxNativeEngine {
    model_dir: PathBuf,
    model_id: String,
    /// When `Some(3|4)`, enables Higgs GPU TurboQuant KV (`3`=TQ3, `4`=TQ4). Defaults to dense FP16 stepping cache.
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

    /// Context length read from tokenizer/model config at load time (`None` if missing or unload).
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
        messages: Vec<Value>,
        tools: Vec<Value>,
        tx: mpsc::Sender<String>,
    ) -> anyhow::Result<()> {
        // Clone Arc handles for the blocking worker. The engine itself isn't
        // moved — only references to its cached state.
        let loaded = Arc::clone(&self.loaded);
        let model_dir = self.model_dir.clone();
        let model_id = self.model_id.clone();
        let kv_bits = self.kv_cache_bits;

        let join = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            generate_with_cache(&loaded, &model_dir, &model_id, &messages, &tools, kv_bits, tx)
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
    use crate::local_model::read_model_context_length_from_dir;

    let model_context_length = read_model_context_length_from_dir(model_dir);

    detect_architecture(model_id, model_dir)?;
    let tokenizer_file = model_dir.join("tokenizer.json");
    let tokenizer_config = model_dir.join("tokenizer_config.json");
    let tokenizer = Tokenizer::from_file(&tokenizer_file)
        .map_err(|e| anyhow::anyhow!("tokenizer load failed: {e:?}"))?;

    // Try `tokenizer_config.json` first; fall back to standalone `chat_template.jinja`
    // (used by e.g. mlx-community OptiQ models which split the template into a separate file).
    let chat_template = load_model_chat_template_from_file(&tokenizer_config)?
        .or_else(|| {
            let jinja = model_dir.join("chat_template.jinja");
            std::fs::read_to_string(&jinja).ok().map(|s| {
                tracing::info!("[local-mlx-native] loaded chat template from chat_template.jinja");
                s
            })
        })
        .ok_or_else(|| {
            anyhow::anyhow!(
                "chat template missing — expected `chat_template` key in tokenizer_config.json \
                 or a `chat_template.jinja` file in the model directory"
            )
        })?;

    let arch = detect_architecture(model_id, model_dir)?;
    let (model, n_layers) = match arch {
        Arch::Qwen3 => {
            let m = load_qwen3_any(model_dir)
                .map_err(|e| anyhow::anyhow!("load_qwen3 failed: {e:?}"))?;
            let n = m.args.num_hidden_layers as usize;
            (LoadedModel::Qwen3(m), n)
        }
        Arch::Gemma4 => {
            let m = load_gemma4_any(model_dir)?;
            let n = m.args.num_hidden_layers as usize;
            (LoadedModel::Gemma4(m), n)
        }
        Arch::Qwen3_5 => {
            let m = load_qwen35_any(model_dir)?;
            let n = m.args.num_hidden_layers as usize;
            (LoadedModel::Qwen3_5(m), n)
        }
    };

    tracing::info!(
        "[local-mlx-native] cached state ready for {model_id} ({n_layers} layers, model_context_length={model_context_length:?})"
    );
    Ok(Loaded {
        model,
        tokenizer,
        chat_template,
        n_layers,
        model_context_length,
    })
}

/// Current process RSS (MiB) via macOS mach task_info. Returns 0 on failure or non-macOS.
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

/// MLX Metal allocator stats (active, cache, peak) in MiB.
///
/// - **active**: bytes currently referenced by live MLX arrays (what "costs" RAM right now)
/// - **cache**: buffers freed by MLX but held in its pool (immediately reusable, counts against
///   Activity Monitor "Memory" but not actually leaking)
/// - **peak**: high-water mark since process start (or last `mlx_reset_peak_memory`)
///
/// These map directly to what Activity Monitor shows as the process's GPU/Metal memory.
#[cfg(feature = "local-mlx")]
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

#[cfg(not(feature = "local-mlx"))]
#[allow(dead_code)]
fn mlx_mem_mib() -> (f64, f64, f64) {
    (0.0, 0.0, 0.0)
}

/// Common model dimensions needed for memory estimates.
struct MemArgs {
    hidden_size: i32,
    num_hidden_layers: i32,
    num_attention_heads: i32,
    num_key_value_heads: i32,
    head_dim: i32,
    vocab_size: i32,
}

impl From<&super::mlx_lm::models::qwen3::ModelArgs> for MemArgs {
    fn from(a: &super::mlx_lm::models::qwen3::ModelArgs) -> Self {
        Self {
            hidden_size: a.hidden_size,
            num_hidden_layers: a.num_hidden_layers,
            num_attention_heads: a.num_attention_heads,
            num_key_value_heads: a.num_key_value_heads,
            head_dim: a.head_dim,
            vocab_size: a.vocab_size,
        }
    }
}

impl From<&super::mlx_lm::models::gemma4::Gemma4Config> for MemArgs {
    fn from(a: &super::mlx_lm::models::gemma4::Gemma4Config) -> Self {
        Self {
            hidden_size: a.hidden_size,
            num_hidden_layers: a.num_hidden_layers,
            num_attention_heads: a.num_attention_heads,
            num_key_value_heads: a.num_key_value_heads,
            head_dim: a.head_dim,
            vocab_size: a.vocab_size,
        }
    }
}

impl From<&super::mlx_lm::models::qwen3_5::ModelArgs> for MemArgs {
    fn from(a: &super::mlx_lm::models::qwen3_5::ModelArgs) -> Self {
        Self {
            hidden_size: a.hidden_size,
            num_hidden_layers: a.num_hidden_layers,
            num_attention_heads: a.num_attention_heads,
            num_key_value_heads: a.num_key_value_heads,
            head_dim: a.head_dim,
            vocab_size: a.vocab_size,
        }
    }
}

/// Synchronous generation entry. Runs on a blocking worker, holding the
/// engine's `loaded` mutex for the duration of one inference call.
fn generate_with_cache(
    loaded: &Arc<Mutex<Option<Loaded>>>,
    model_dir: &Path,
    model_id: &str,
    messages: &[Value],
    tools: &[Value],
    engine_kv_bits: Option<u8>,
    tx: mpsc::Sender<String>,
) -> anyhow::Result<()> {
    use super::mlx_lm::kv_layer::Qwen3LayerKv;
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
    let template = state.chat_template.clone();
    let n_layers = state.n_layers;

    let encodings = state.tokenizer
        .apply_chat_template_json_and_encode(
            template,
            model_id,
            None,
            messages,
            tools,
            None,
            Some(true),
        )
        .map_err(|e| anyhow::anyhow!("chat template apply failed: {e:?}"))?;
    let mut prompt: Vec<u32> = encodings
        .iter()
        .flat_map(|e| e.get_ids())
        .copied()
        .collect();

    let settings_dir = model_dir.parent().unwrap_or(model_dir);
    let gen_opt =
        crate::gateway::ui_server::local_models::load_settings_blocking(settings_dir);
    // Auto-enable TurboQuant KV when the weights are 4-bit and the user hasn't
    // set an explicit override. Reading config.json here is cheap (OS page cache).
    let auto_tq = if gen_opt.kv_cache_bits.is_none() && engine_kv_bits.is_none() {
        detect_weight_bits(model_dir)
            .filter(|&b| b <= 4)
            .map(|_| 4u8)
    } else {
        None
    };
    let kv_bits_merged = gen_opt.kv_cache_bits.or(engine_kv_bits).or(auto_tq);
    if auto_tq.is_some() {
        tracing::info!("[local-mlx-native] 4-bit weights detected → auto-enabling TurboQuant TQ4 KV cache");
    }

    let raw_max_new = gen_opt
        .max_new_tokens
        .unwrap_or(crate::gateway::ui_server::local_models::DEFAULT_MLX_MAX_NEW_TOKENS)
        .clamp(1, 8192) as usize;

    let max_new_tokens = raw_max_new;

    let max_prompt_tokens = gen_opt
        .max_prompt_tokens
        .unwrap_or(crate::gateway::ui_server::local_models::DEFAULT_MLX_MAX_PROMPT_TOKENS)
        .clamp(512, 262_144) as usize;

    if prompt.len() > max_prompt_tokens {
        let drop = prompt.len() - max_prompt_tokens;
        tracing::warn!(
            "[local-mlx-native] truncating prompt {} → {} tokens (RAM/KV cap; edit ~/.senclaw/local_models/settings.json max_prompt_tokens)",
            prompt.len(),
            max_prompt_tokens
        );
        prompt.drain(..drop);
    }

    // Log the first/last few prompt tokens — lets us confirm the chat
    // template produced `<|im_start|>` / `<|im_end|>` (Qwen3 = 151644 / 151645)
    // rather than splitting them into many BPE pieces.
    tracing::info!(
        "[local-mlx-native] prompt {} tokens, head={:?} tail={:?}",
        prompt.len(),
        &prompt[..prompt.len().min(8)],
        &prompt[prompt.len().saturating_sub(8)..]
    );
    let tq_mem = kv_bits_merged.is_some();

    let mem_args: MemArgs = match &state.model {
        LoadedModel::Qwen3(m) => MemArgs::from(&m.args),
        LoadedModel::Gemma4(m) => MemArgs::from(&m.args),
        LoadedModel::Qwen3_5(m) => MemArgs::from(&m.args),
    };
    log_local_mlx_memory_estimates(&mem_args, prompt.len(), max_new_tokens, None, tq_mem);
    let prompt_tokens = Array::from(&prompt[..]).index(NewAxis);

    use super::mlx_lm::models::qwen3::sample;
    let max_tokens: usize = max_new_tokens;
    let temperature: f32 = 0.0;

    let rss_start = rss_mib();
    let (mlx_a0, mlx_c0, _) = mlx_mem_mib();
    tracing::info!(
        "[local-mlx-native][mem] generate start — rss={:.0} MiB | mlx active={:.0} cache={:.0} MiB (prompt={} tokens, max_new={})",
        rss_start, mlx_a0, mlx_c0, prompt.len(), max_new_tokens
    );

    // ── Model-specific generation paths ──────────────────────────────────
    // Each branch builds the right cache type and runs the decode loop.
    // The loop body is identical; only the forward() call signature differs.
    macro_rules! decode_loop {
        ($model_forward:expr, $cache:expr, $is_stop:expr) => {{
            let mut buffer: Vec<Array> = Vec::new();
            let mut hit_stop = false;
            let mut generated_count = 0usize;

            // Prefill
            let t_prefill_start = std::time::Instant::now();
            let mut next_token = {
                let logits = $model_forward(&prompt_tokens, $cache)
                    .map_err(|e| anyhow::anyhow!("prefill forward failed: {e:?}"))?;
                sample(&logits.index((.., -1i32, ..)), temperature)
                    .map_err(|e| anyhow::anyhow!("prefill sample failed: {e:?}"))?
            };
            eval(&[next_token.clone()]).map_err(|e| anyhow::anyhow!("prefill eval failed: {e:?}"))?;
            let prefill_secs = t_prefill_start.elapsed().as_secs_f64();
            let t_decode_start = std::time::Instant::now();
            {
                let (ma, mc, mp) = mlx_mem_mib();
                tracing::info!(
                    "[local-mlx-native][mem] after prefill {:.0} tok/s — rss={:.0}(Δ{:.0}) | mlx active={:.0} cache={:.0} peak={:.0} MiB",
                    prompt.len() as f64 / prefill_secs.max(0.001),
                    rss_mib(), rss_mib() - rss_start, ma, mc, mp
                );
            }
            // Release prefill attention workspace from Metal buffer pool before decode.
            // Prefill produces large intermediate arrays (seq_len×seq_len per layer) that
            // stay in the pool; clearing here prevents them from inflating RSS during decode.
            unsafe { mlx_sys::mlx_clear_cache(); }
            buffer.push(next_token.clone());
            generated_count += 1;

            for _step in 1..max_tokens {
                let inputs = next_token.reshape(&[1i32, 1i32])
                    .map_err(|e| anyhow::anyhow!("reshape failed: {e:?}"))?;
                let y = {
                    let logits = $model_forward(&inputs, $cache)
                        .map_err(|e| anyhow::anyhow!("decode forward failed: {e:?}"))?;
                    sample(&logits.index((.., -1i32, ..)), temperature)
                        .map_err(|e| anyhow::anyhow!("decode sample failed: {e:?}"))?
                };
                next_token = y;
                eval(std::iter::once(&next_token))
                    .map_err(|e| anyhow::anyhow!("step eval failed: {e:?}"))?;
                buffer.push(next_token.clone());
                generated_count += 1;

                if buffer.len() % 20 == 0 {
                    eval(&buffer).map_err(|e| anyhow::anyhow!("eval failed: {e:?}"))?;
                    {
                        let (ma, mc, _) = mlx_mem_mib();
                        tracing::info!(
                            "[local-mlx-native][mem] decode step {} — rss={:.0}(Δ{:.0}) | mlx active={:.0} cache={:.0} MiB",
                            generated_count, rss_mib(), rss_mib() - rss_start, ma, mc
                        );
                    }
                    let slice: Vec<u32> = buffer.drain(..).map(|t| t.item::<u32>()).collect();
                    if generated_count <= 20 {
                        let decoded_preview = state.tokenizer.decode(&slice, false).unwrap_or_default();
                        tracing::info!("[local-mlx-native] first {} token ids: {:?} → {:?}", slice.len(), slice, decoded_preview);
                    }
                    if let Some(&stop) = slice.iter().find(|&&t| $is_stop(t)) {
                        let cut: Vec<u32> = slice.iter().copied().take_while(|&t| t != stop).collect();
                        let text = state.tokenizer.decode(&cut, true)
                            .map_err(|e| anyhow::anyhow!("decode failed: {e:?}"))?;
                        if !text.is_empty() { let _ = tx.blocking_send(text); }
                        hit_stop = true;
                        break;
                    }
                    let text = state.tokenizer.decode(&slice, true)
                        .map_err(|e| anyhow::anyhow!("decode failed: {e:?}"))?;
                    if !text.is_empty() && tx.blocking_send(text).is_err() {
                        log_local_mlx_memory_estimates(&mem_args, prompt.len(), max_new_tokens, Some(generated_count), tq_mem);
                        return Ok(());
                    }
                }
            }

            // Flush remaining buffer
            let done_stop = hit_stop;
            if !buffer.is_empty() {
                eval(&buffer).map_err(|e| anyhow::anyhow!("final eval failed: {e:?}"))?;
                let slice: Vec<u32> = buffer.drain(..).map(|t| t.item::<u32>()).collect();
                let (slice_use, _) = if done_stop {
                    // Already sent text up to stop; remaining tokens after stop are discarded.
                    // (This path is unreachable: we break before hitting here when hit_stop=true,
                    // but keep it for clarity.)
                    (slice.as_slice(), false)
                } else {
                    (slice.as_slice(), true)
                };
                if !done_stop {
                    let text = state.tokenizer.decode(slice_use, true)
                        .map_err(|e| anyhow::anyhow!("final decode failed: {e:?}"))?;
                    if !text.is_empty() { let _ = tx.blocking_send(text); }
                }
            }

            {
                let (ma, mc, mp) = mlx_mem_mib();
                let decode_secs = t_decode_start.elapsed().as_secs_f64();
                let decode_tps = (generated_count.saturating_sub(1)) as f64 / decode_secs.max(0.001);
                let tag = if done_stop { "done (stop)" } else { "done" };
                tracing::info!(
                    "[local-mlx-native][mem] generate {} — {:.1} tok/s decode | rss={:.0}(Δ{:.0}) | mlx active={:.0} cache={:.0} peak={:.0} MiB | new_tokens={}",
                    tag, decode_tps, rss_mib(), rss_mib() - rss_start, ma, mc, mp, generated_count
                );
            }
            unsafe { mlx_sys::mlx_clear_cache(); }
            tracing::info!("[local-mlx-native][mem] cache cleared, mlx active={:.0} cache={:.0} MiB",
                {let (a,_,_)=mlx_mem_mib();a}, {let(_,c,_)=mlx_mem_mib();c});
            log_local_mlx_memory_estimates(&mem_args, prompt.len(), max_new_tokens, Some(generated_count), tq_mem);
        }};
    }

    let tq_bits = kv_bits_merged.map(|raw| {
        if raw == 4 {
            4u8
        } else {
            if raw != 3 {
                tracing::warn!("[local-mlx-native] kv_cache_bits={raw}: clamping to TQ3");
            }
            3u8
        }
    });
    if let Some(b) = tq_bits {
        tracing::info!(
            "[local-mlx-native] KV cache: TurboQuant TQ{b} (key={}bit value={}bit; threshold={}tok)",
            b - 1,
            b,
            std::env::var("HIGGS_TURBOQUANT_MIN_TOKENS")
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(2048),
        );
    }

    match &mut state.model {
        LoadedModel::Qwen3(model) => {
            use super::mlx_lm::models::qwen3::ModelInput;
            use mlx_rs::module::Module;

            const QWEN3_IM_END: u32 = 151645;
            const QWEN3_ENDOFTEXT: u32 = 151643;
            let is_stop = |t: u32| t == QWEN3_IM_END || t == QWEN3_ENDOFTEXT;

            let mut cache: Vec<Option<Qwen3LayerKv>> = match tq_bits {
                Some(b) => (0..n_layers)
                    .map(|_| {
                        Some(
                            Qwen3LayerKv::turbo(b, model.args.num_key_value_heads, model.args.head_dim)
                                .expect("TQ cache init"),
                        )
                    })
                    .collect(),
                None => (0..n_layers).map(|_| Some(Qwen3LayerKv::dense())).collect(),
            };

            decode_loop!(
                |inputs: &Array, c: &mut Vec<Option<Qwen3LayerKv>>| {
                    model.forward(ModelInput { inputs, mask: None, cache: c })
                },
                &mut cache,
                is_stop
            );
        }
        LoadedModel::Gemma4(model) => {
            use super::mlx_lm::models::higgs_kv::SteppingKeyValueCache;
            use super::mlx_lm::models::higgs_turboquant_mlx::{KvCacheConfig, KvCacheMode};

            // Gemma 4 stop tokens: eos=1, <turn|>=106, <tool_response|>=50
            // (eos_token_id=[1, 106, 50] per config.json; token 107 is NOT end-of-turn)
            const GEMMA_EOS: u32 = 1;
            const GEMMA_TURN_END: u32 = 106;
            const GEMMA_TOOL_RESP: u32 = 50;
            let is_stop =
                |t: u32| t == GEMMA_EOS || t == GEMMA_TURN_END || t == GEMMA_TOOL_RESP;

            let n_kv = model.args.num_key_value_heads;
            let hd = model.args.head_dim;
            let mut cache: Vec<Option<SteppingKeyValueCache>> = match tq_bits {
                Some(b) => (0..n_layers)
                    .map(|_| {
                        Some(
                            SteppingKeyValueCache::new_turbo(
                                KvCacheConfig {
                                    mode: KvCacheMode::Turboquant,
                                    bits: b,
                                    norm_correction: true,
                                    seed: 0,
                                    ..KvCacheConfig::default()
                                },
                                n_kv,
                                hd,
                            )
                            .expect("TQ cache init"),
                        )
                    })
                    .collect(),
                None => (0..n_layers).map(|_| Some(SteppingKeyValueCache::new())).collect(),
            };

            decode_loop!(
                |inputs: &Array, c: &mut Vec<Option<SteppingKeyValueCache>>| {
                    model.forward(inputs, None, c)
                },
                &mut cache,
                is_stop
            );
        }
        LoadedModel::Qwen3_5(model) => {
            use super::mlx_lm::models::qwen3_5::{build_layer_caches, Qwen3_5LayerCache};

            // Qwen3.5 EOS token
            const QWEN35_EOS: u32 = 248044;
            const QWEN35_IM_END: u32 = 248043;
            let is_stop = |t: u32| t == QWEN35_EOS || t == QWEN35_IM_END;

            let mut cache: Vec<Option<Qwen3_5LayerCache>> =
                build_layer_caches(&model.args, tq_bits)
                    .map_err(|e| anyhow::anyhow!("Qwen3.5 cache init failed: {e:?}"))?;

            decode_loop!(
                |inputs: &Array, c: &mut Vec<Option<Qwen3_5LayerCache>>| {
                    model.forward(inputs, c)
                },
                &mut cache,
                is_stop
            );
        }
    }

    Ok(())
}

/// Heuristic unified-memory breakdown for Apple Silicon (GiB). Not RSS: MLX keeps pools,
/// prefill can allocate large **S×S** scratch per attention layer; FP16 **KV** grows ~linearly with seq.
fn log_local_mlx_memory_estimates(
    ma: &MemArgs,
    prompt_tokens: usize,
    max_new_tokens: usize,
    generated_tokens: Option<usize>,
    turboquant_kv: bool,
) {
    fn gib_u128(bytes: u128) -> f64 {
        bytes as f64 / (1024.0_f64.powi(3))
    }

    let n_l = ma.num_hidden_layers as u128;
    let n_kv = ma.num_key_value_heads as u128;
    let n_h = ma.num_attention_heads as u128;
    let hd = ma.head_dim as u128;
    let s_pref = prompt_tokens as u128;
    let seq_peak = prompt_tokens.saturating_add(max_new_tokens) as u128;

    // Stored KV: per layer, FP16 K and V → 2 × (n_kv × seq × head_dim) elems × 2 bytes
    let kv_peak_bytes = n_l * 2u128 * n_kv * seq_peak * hd * 2u128;

    // Naive attention scores [..., heads, S, S] in f32 — kernels often avoid full materialization.
    let attn_layer_bytes = n_h * s_pref * s_pref * 4u128;
    let attn_all_layers_naive = attn_layer_bytes.saturating_mul(n_l);

    tracing::info!(
        "[local-mlx-native][mem] model hidden={} layers={} attn_heads={} kv_heads={} head_dim={} vocab={}",
        ma.hidden_size,
        ma.num_hidden_layers,
        ma.num_attention_heads,
        ma.num_key_value_heads,
        ma.head_dim,
        ma.vocab_size
    );
    tracing::info!(
        "[local-mlx-native][mem] run budget max_new_tokens={} | seq_peak≈{} (= prompt + max decode)",
        max_new_tokens,
        seq_peak
    );
    if turboquant_kv {
        // TQ3 ≈4× smaller than FP16 KV for the same seq (see turboquant-rs compression tests); heuristic only.
        const TQ3_KV_VS_FP16: u128 = 4;
        let kv_tq_heuristic = kv_peak_bytes / TQ3_KV_VS_FP16;
        tracing::info!(
            "[local-mlx-native][mem] KV TurboQuant TQ3 (heuristic ≈ FP16/{TQ3_KV_VS_FP16}): ≈{:.3} GiB peak — **not** FP16 concat; FP16 line below is SDPA-only reference",
            gib_u128(kv_tq_heuristic)
        );
    }
    tracing::info!(
        "[local-mlx-native][mem] KV FP16 reference (if this were concat SDPA path): ≈{:.3} GiB",
        gib_u128(kv_peak_bytes)
    );
    tracing::info!(
        "[local-mlx-native][mem] prefill attention S×S f32 (naive matmul upper bound): ≈{:.3} GiB/layer at prompt_tokens={}, ×{} layers ≈{:.2} GiB if fully materialized — often **far less** with flash/sdpa; long prompts spike Activity Monitor anyway",
        gib_u128(attn_layer_bytes),
        prompt_tokens,
        ma.num_hidden_layers,
        gib_u128(attn_all_layers_naive)
    );
    tracing::info!(
        "[local-mlx-native][mem] activations: prefill workspace ~S² per layer + transient tensors; lower max_prompt_tokens / TurboQuant cap on max_new to shrink CPU time and peak alloc"
    );

    if let Some(gen) = generated_tokens {
        let final_seq = (prompt_tokens + gen) as u128;
        let kv_final = n_l * 2u128 * n_kv * final_seq * hd * 2u128;
        if turboquant_kv {
            tracing::info!(
                "[local-mlx-native][mem] done: new_tokens={} final_seq≈{} | KV TQ3 heuristic ≈{:.3} GiB | FP16 ref ≈{:.3} GiB",
                gen,
                prompt_tokens + gen,
                gib_u128(kv_final / 4),
                gib_u128(kv_final)
            );
        } else {
            tracing::info!(
                "[local-mlx-native][mem] done: new_tokens={} final_seq≈{} | KV FP16 ≈{:.3} GiB",
                gen,
                prompt_tokens + gen,
                gib_u128(kv_final)
            );
        }
    }
}

/// Load a Qwen3 `Model`, applying `nn::quantize` when `config.json` declares a
/// `quantization` block (mlx-community 4-bit / 8-bit variants). Without this
/// step, plain `Linear` slots can't accept the packed-int safetensor weights
/// and inference crashes with an `rms_norm` shape mismatch.
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
        // Force-materialize the int4-quantized params immediately so the
        // original f32 random-init tensors from `Model::new` can be released
        // by the Metal buffer pool before we start overwriting them with the
        // real weights from safetensors. Without this `eval`, the lazy graph
        // keeps both the random-init f32 (~16 GB for Qwen3-4B) AND the
        // quantized derivation alive concurrently.
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

    let disk_bytes: u64 = shard_files
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    tracing::info!(
        "[local-mlx-native][mem] safetensors on disk ≈ {:.3} GiB ({} file(s)); loaded VRAM/UM tends to track this for 4-bit + overhead",
        disk_bytes as f64 / (1024.0_f64.powi(3)),
        shard_files.len()
    );

    // Custom safetensors load with .weight → .inner.weight remap for
    // QuantizedLinear / QuantizedEmbedding slots. Without this, the packed
    // int4 weight tensor never reaches the model and inference yields
    // uniform-noise logits even though scales/biases load fine.
    let n_layers = model.args.num_hidden_layers as usize;
    let _ = n_layers; // (kept for debugging; lets us assert shard coverage if needed)

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

            // Side-channel: stash embed_tokens.* for direct assignment after.
            match key {
                "model.embed_tokens.weight" => {
                    embed_weight = Some(value);
                    total_loaded += 1;
                    unfilled_slots.remove(key);
                    continue;
                }
                "model.embed_tokens.scales" => {
                    embed_scales = Some(value);
                    total_loaded += 1;
                    unfilled_slots.remove(key);
                    continue;
                }
                "model.embed_tokens.biases" => {
                    embed_biases = Some(value);
                    total_loaded += 1;
                    unfilled_slots.remove(key);
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

    // Loading is lazy; eval to materialize.
    model
        .eval()
        .map_err(|e| anyhow::anyhow!("eval after load failed: {e:?}"))?;

    Ok(model)
}

/// Read the top-level `quantization.bits` from `config.json`, returns None if absent.
fn detect_weight_bits(model_dir: &Path) -> Option<u8> {
    let raw = std::fs::read_to_string(model_dir.join("config.json")).ok()?;
    let cfg: serde_json::Value = serde_json::from_str(&raw).ok()?;
    cfg.get("quantization")
        .and_then(|q| q.get("bits"))
        .and_then(|b| b.as_u64())
        .map(|b| b as u8)
}

fn detect_architecture(model_id: &str, model_dir: &Path) -> anyhow::Result<Arch> {
    let cfg_path = model_dir.join("config.json");
    let model_type = if let Ok(raw) = std::fs::read_to_string(&cfg_path) {
        serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| v.get("model_type").and_then(|x| x.as_str()).map(str::to_owned))
    } else {
        None
    };

    match model_type.as_deref() {
        Some("qwen3") => return Ok(Arch::Qwen3),
        Some("qwen3_5" | "qwen3_5_text") => return Ok(Arch::Qwen3_5),
        Some("gemma3" | "gemma3_text" | "gemma4" | "gemma4_text") => return Ok(Arch::Gemma4),
        _ => {}
    }

    let lower = model_id.to_lowercase();
    if lower.contains("qwen3.5") || lower.contains("qwen3_5") {
        return Ok(Arch::Qwen3_5);
    }
    if lower.contains("qwen3") {
        return Ok(Arch::Qwen3);
    }
    if lower.contains("gemma") {
        return Ok(Arch::Gemma4);
    }

    anyhow::bail!(
        "no native loader for `{}` (model_type={}) — supported: qwen3, gemma3, gemma4.",
        model_id,
        model_type.as_deref().unwrap_or("unknown")
    )
}

fn load_gemma4_any(
    model_dir: &Path,
) -> anyhow::Result<super::mlx_lm::models::gemma4::Gemma4CausalLM> {
    super::mlx_lm::models::gemma4::load_gemma4_model(model_dir)
        .map_err(|e| anyhow::anyhow!("load_gemma4 failed: {e:?}"))
}

fn load_qwen35_any(
    model_dir: &Path,
) -> anyhow::Result<super::mlx_lm::models::qwen3_5::Model> {
    super::mlx_lm::models::qwen3_5::load_qwen35_model(model_dir)
        .map_err(|e| anyhow::anyhow!("load_qwen35 failed: {e:?}"))
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
