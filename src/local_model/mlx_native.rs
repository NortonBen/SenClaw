//! Native MLX inference engine.
//!
//! Apple Silicon only. Uses **`mlx-rs`** plus Qwen3 weights/templates vendored in-tree (see [`super::mlx_lm`]).
//!
//! NOTE: only Qwen3 MLX checkpoints are supported by the vendored loader today.
//! Models that are not Qwen3 return an error from [`MlxNativeEngine::start`] when detected.
//!
//! KV cache:
//! - **Default:** [`super::mlx_lm::cache::ConcatKeyValueCache`] (FP16 on device).
//! - **With `--features local-mlx-turboquant` + `kv_cache_bits` in settings / engine:** CPU-side
//!   [`turboquant::QuantizedKVCache`] (see `mlx_lm::turboquant_kv`) to shrink KV RAM at the cost of
//!   approximate attention (sequential CPU scoring per head). **`3`** = **TQ3** (default when remapping `2`); **`4`** = TQ4.
//!   Prompt + decode is capped at [`crate::gateway::ui_server::local_models::TURBOQUANT_MAX_CONTEXT_TOKENS`] (**32k**).
//! MLX packed KV (`MlxQuantizedConcatKeyValueCache`) stays disabled — concat quantized tensors broke quality.

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
}

/// In-process MLX inference engine. Caches the loaded model so subsequent
/// chats reuse weights instead of re-reading safetensors every call.
pub struct MlxNativeEngine {
    model_dir: PathBuf,
    model_id: String,
    /// When `Some(2|3|4)` and built with `local-mlx-turboquant`, enables TurboQuant KV (`3`=TQ3, `4`=TQ4; `2`→TQ3); else FP16 concat.
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
        messages: &[ChatMessage],
        tx: mpsc::Sender<String>,
    ) -> anyhow::Result<()> {
        // Clone Arc handles for the blocking worker. The engine itself isn't
        // moved — only references to its cached state.
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
    tracing::info!(
        "[local-mlx-native] cached state ready for {model_id} ({n_layers} layers)"
    );
    Ok(Loaded {
        model,
        tokenizer,
        chat_template,
        n_layers,
    })
}

/// Synchronous generation entry. Runs on a blocking worker, holding the
/// engine's `loaded` mutex for the duration of one inference call.
fn generate_with_cache(
    loaded: &Arc<Mutex<Option<Loaded>>>,
    model_dir: &Path,
    model_id: &str,
    messages: &[ChatMessage],
    engine_kv_bits: Option<u8>,
    tx: mpsc::Sender<String>,
) -> anyhow::Result<()> {
    use super::mlx_lm::cache::ConcatKeyValueCache;
    use super::mlx_lm::kv_layer::Qwen3LayerKv;
    #[cfg(feature = "local-mlx-turboquant")]
    use super::mlx_lm::turboquant_kv::TurboQuantKeyValueCache;
    #[cfg(feature = "local-mlx-turboquant")]
    use std::sync::{Arc, Mutex};
    #[cfg(feature = "local-mlx-turboquant")]
    use turboquant::{packed::TurboQuantConfig, QuantizedKVCache};
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
            super::runtime::Role::Assistant => convs.push(Conversation {
                role: TokRole::Assistant,
                content: m.content.clone(),
            }),
        }
    }
    if !pending_system.is_empty() {
        // System-only — emit as a single user turn so the model has something to answer.
        convs.push(Conversation {
            role: TokRole::User,
            content: pending_system,
        });
    }

    let args = ApplyChatTemplateArgs {
        conversations: vec![convs.into()],
        documents: None,
        model_id,
        chat_template_id: None,
        add_generation_prompt: Some(true),
        continue_final_message: None,
    };
    let encodings = tokenizer
        .apply_chat_template_and_encode(template, args)
        .map_err(|e| anyhow::anyhow!("chat template apply failed: {e:?}"))?;
    let mut prompt: Vec<u32> = encodings
        .iter()
        .flat_map(|e| e.get_ids())
        .copied()
        .collect();

    let settings_dir = model_dir.parent().unwrap_or(model_dir);
    let gen_opt =
        crate::gateway::ui_server::local_models::load_settings_blocking(settings_dir);
    let kv_bits_merged = gen_opt.kv_cache_bits.or(engine_kv_bits);

    let raw_max_new = gen_opt
        .max_new_tokens
        .unwrap_or(crate::gateway::ui_server::local_models::DEFAULT_MLX_MAX_NEW_TOKENS)
        .clamp(1, 8192) as usize;

    let max_new_tokens = {
        #[cfg(feature = "local-mlx-turboquant")]
        {
            if kv_bits_merged.is_some() {
                use crate::gateway::ui_server::local_models::TURBOQUANT_MAX_NEW_TOKENS_CAP;
                let cap = TURBOQUANT_MAX_NEW_TOKENS_CAP as usize;
                let v = raw_max_new.min(cap);
                if raw_max_new > cap {
                    tracing::info!(
                        "[local-mlx-native] TurboQuant: capping max_new_tokens {} → {} (faster reply, lower RAM; max {} in settings for this path)",
                        raw_max_new,
                        v,
                        TURBOQUANT_MAX_NEW_TOKENS_CAP
                    );
                }
                v
            } else {
                raw_max_new
            }
        }
        #[cfg(not(feature = "local-mlx-turboquant"))]
        {
            raw_max_new
        }
    };

    let max_prompt_tokens = {
        let base = gen_opt
            .max_prompt_tokens
            .unwrap_or(crate::gateway::ui_server::local_models::DEFAULT_MLX_MAX_PROMPT_TOKENS)
            .clamp(512, 262_144) as usize;
        #[cfg(feature = "local-mlx-turboquant")]
        {
            let mut max_pt = base;
            if kv_bits_merged.is_some() {
                use crate::gateway::ui_server::local_models::TURBOQUANT_MAX_CONTEXT_TOKENS;
                let cap = TURBOQUANT_MAX_CONTEXT_TOKENS as usize;
                let max_prompt_for_budget = cap.saturating_sub(max_new_tokens).max(512);
                if max_pt > max_prompt_for_budget {
                    tracing::info!(
                        "[local-mlx-native] TurboQuant {}k context: clamping max_prompt_tokens {} → {} (max_new_tokens={}; prompt+decode ≤ {} tokens)",
                        TURBOQUANT_MAX_CONTEXT_TOKENS / 1024,
                        max_pt,
                        max_prompt_for_budget,
                        max_new_tokens,
                        cap
                    );
                    max_pt = max_prompt_for_budget;
                }
            }
            max_pt
        }
        #[cfg(not(feature = "local-mlx-turboquant"))]
        {
            base
        }
    };

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
    #[cfg(feature = "local-mlx-turboquant")]
    let tq_mem = kv_bits_merged.is_some();
    #[cfg(not(feature = "local-mlx-turboquant"))]
    let tq_mem = false;
    log_local_mlx_memory_estimates(&model.args, prompt.len(), max_new_tokens, None, tq_mem);
    let prompt_tokens = Array::from(&prompt[..]).index(NewAxis);

    // Pre-populate cache with `Some(...)` per layer. Otherwise Qwen3Model
    // auto-fills the vec with `None` slots, and Attention's `if let Some(cache)`
    // takes the no-cache branch for every step — KV state never accumulates,
    // and decode after the first token degenerates into a fixed-point loop.
    #[cfg(feature = "local-mlx-turboquant")]
    let mut cache: Vec<Option<Qwen3LayerKv>> = if let Some(bits) = kv_bits_merged {
        let raw = bits.clamp(2, 4);
        // turboquant-rs `QuantizedKVCache` / `quantize_with_qjl` only accepts **total** bit
        // budget **3** (TQ3) or **4** (TQ4). `2` is invalid → map to [`DEFAULT_TURBOQUANT_KV_TOTAL_BITS`] (TQ3).
        let tq_total_bits = if raw == 2 {
            tracing::warn!(
                "[local-mlx-native] kv_cache_bits=2 — turboquant-rs requires TQ3 (3) or TQ4 (4); using TQ3 ({})",
                crate::gateway::ui_server::local_models::DEFAULT_TURBOQUANT_KV_TOTAL_BITS
            );
            crate::gateway::ui_server::local_models::DEFAULT_TURBOQUANT_KV_TOTAL_BITS
        } else {
            raw
        };
        tracing::info!(
            "[local-mlx-native] KV cache: TurboQuant TQ{tq_total_bits} (turboquant-rs; requested={}; {}k context window)",
            raw,
            crate::gateway::ui_server::local_models::TURBOQUANT_MAX_CONTEXT_TOKENS / 1024
        );
        if prompt.len() > 256 {
            tracing::warn!(
                "[local-mlx-native] TurboQuant attention is **CPU**, roughly O(prompt_len² × layers × heads) per forward — \
                 prompt_len={} can stall **minutes**. Lower `max_prompt_tokens` in ~/.senclaw/local_models/settings.json for snappier replies, \
                 or omit `kv_cache_bits` for fast FP16 KV on GPU.",
                prompt.len()
            );
        }
        let ma = &model.args;
        let config = TurboQuantConfig::new(tq_total_bits, ma.head_dim as usize)
            .map_err(|e| anyhow::anyhow!("TurboQuantConfig: {e:?}"))?
            .with_seed(42u64);
        let n_virt = (ma.num_hidden_layers as usize) * (ma.num_key_value_heads as usize);
        let inner = Arc::new(Mutex::new(QuantizedKVCache::new(
            config,
            n_virt,
            0xC0FFEE_u64,
        )));
        (0..n_layers)
            .map(|li| {
                Some(Qwen3LayerKv::TurboQuant(TurboQuantKeyValueCache::new(
                    Arc::clone(&inner),
                    li,
                    ma.num_key_value_heads,
                )))
            })
            .collect()
    } else {
        (0..n_layers)
            .map(|_| Some(Qwen3LayerKv::Concat(ConcatKeyValueCache::new())))
            .collect()
    };

    #[cfg(not(feature = "local-mlx-turboquant"))]
    let mut cache: Vec<Option<Qwen3LayerKv>> = {
        if kv_bits_merged.is_some() {
            tracing::warn!(
                "[local-mlx-native] kv_cache_bits={:?} ignored — rebuild with `--features local-mlx-turboquant`",
                kv_bits_merged
            );
        }
        (0..n_layers)
            .map(|_| Some(ConcatKeyValueCache::new()))
            .collect()
    };

    // Qwen3 stop tokens.
    const QWEN3_IM_END: u32 = 151645;
    const QWEN3_ENDOFTEXT: u32 = 151643;
    let is_stop = |t: u32| t == QWEN3_IM_END || t == QWEN3_ENDOFTEXT;

    // ── Custom decode loop ────────────────────────────────────────────
    // Bypass `mlx_lm::models::qwen3::Generate` because its Decode→Decode
    // transition stores `y` at shape [1,1] (from argmax on [1,1,V]) and then
    // re-applies `y.index((.., NewAxis))` which produces [1,1,1] — the
    // embedding lookup gets a 3D tensor and every subsequent forward pass
    // is mis-shaped, causing greedy decode to lock after ~2 tokens.
    //
    // The fix is to keep the running token reshaped explicitly to [1, 1].
    use super::mlx_lm::models::qwen3::{sample, ModelInput};
    use mlx_rs::module::Module;

    let max_tokens: usize = max_new_tokens;
    let mut buffer: Vec<Array> = Vec::new();
    let mut hit_stop = false;
    let mut generated_count = 0usize;
    let temperature: f32 = 0.0;

    // Prefill — feed full prompt, take logits at last position.
    let prefill_input = ModelInput {
        inputs: &prompt_tokens,
        mask: None,
        cache: &mut cache,
    };
    let logits = model
        .forward(prefill_input)
        .map_err(|e| anyhow::anyhow!("prefill forward failed: {e:?}"))?;
    let last_logits = logits.index((.., -1, ..));
    let mut next_token = sample(&last_logits, temperature)
        .map_err(|e| anyhow::anyhow!("prefill sample failed: {e:?}"))?;
    // next_token shape: [1]. Force eval so we can `.item::<u32>()` cleanly later.
    eval(&[next_token.clone()]).map_err(|e| anyhow::anyhow!("prefill eval failed: {e:?}"))?;
    buffer.push(next_token.clone());
    generated_count += 1;

    for _step in 1..max_tokens {
        // Reshape next_token from [1] → [1, 1] explicitly.
        let inputs = next_token
            .reshape(&[1, 1])
            .map_err(|e| anyhow::anyhow!("reshape failed: {e:?}"))?;
        let decode_input = ModelInput {
            inputs: &inputs,
            mask: None,
            cache: &mut cache,
        };
        let logits = model
            .forward(decode_input)
            .map_err(|e| anyhow::anyhow!("decode forward failed: {e:?}"))?;
        // logits shape [1, 1, V] — slice last position to get [1, V] for sampling.
        let last_logits = logits.index((.., -1, ..));
        let y = sample(&last_logits, temperature)
            .map_err(|e| anyhow::anyhow!("decode sample failed: {e:?}"))?;
        next_token = y;
        buffer.push(next_token.clone());
        generated_count += 1;

        if buffer.len() % 20 == 0 {
            eval(&buffer).map_err(|e| anyhow::anyhow!("eval failed: {e:?}"))?;
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
                log_local_mlx_memory_estimates(
                    &model.args,
                    prompt.len(),
                    max_new_tokens,
                    Some(generated_count),
                    tq_mem,
                );
                return Ok(());
            }
        }
    }
    if hit_stop {
        log_local_mlx_memory_estimates(
            &model.args,
            prompt.len(),
            max_new_tokens,
            Some(generated_count),
            tq_mem,
        );
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

    log_local_mlx_memory_estimates(
        &model.args,
        prompt.len(),
        max_new_tokens,
        Some(generated_count),
        tq_mem,
    );
    Ok(())
}

/// Heuristic unified-memory breakdown for Apple Silicon (GiB). Not RSS: MLX keeps pools,
/// prefill can allocate large **S×S** scratch per attention layer; FP16 **KV** grows ~linearly with seq.
fn log_local_mlx_memory_estimates(
    ma: &super::mlx_lm::models::qwen3::ModelArgs,
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

#[derive(Debug, Clone, Copy)]
enum Arch {
    Qwen3,
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

    // Loading is lazy; eval to materialize.
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
