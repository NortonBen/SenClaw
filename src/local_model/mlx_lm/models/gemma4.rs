//! Gemma 3 / Gemma 4 model implementation for native MLX inference.
//!
//! Handles `model_type` values: `"gemma3"`, `"gemma3_text"`, `"gemma4"`, `"gemma4_text"`.
//!
//! Architecture differences from Gemma 2:
//! - QK-norm (RMSNorm on Q and K heads, same as Qwen3)
//! - Scale from `query_pre_attn_scalar` instead of `head_dim`
//! - No attention logit soft-capping (dropped in Gemma 3)
//! - Interleaved 5:1 local/global attention (`sliding_window_pattern = 6`)
//!   - Local layers: sliding window of `sliding_window` tokens
//!   - Global layers: full causal attention (every 6th layer, 1-indexed)
//! - RoPE applied to **all** layers (local and global)
//! - `sqrt(hidden_size)` embedding scaling
//! - Gemma 2 RMSNorm +1 weight convention preserved

use std::path::Path;

use mlx_rs::{
    Array, arange, array,
    builder::Builder,
    error::Exception,
    macros::{ModuleParameters, Quantizable},
    module::{Module, ModuleParameters, Param},
    nn, ops,
    ops::indexing::IndexOp,
    quantization::MaybeQuantized,
};
use serde::Deserialize;

use crate::local_model::higgs_error::ModelError;

use super::higgs_attn_utils::{AttentionMask, apply_rope, create_attention_mask};
use super::higgs_kv::{KeyValueCache, KvCacheView};

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

const fn default_rope_theta() -> f32 {
    10000.0
}
const fn default_sliding_window_pattern() -> i32 {
    6
}
const fn default_tie_word_embeddings() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuantizationConfig {
    pub group_size: i32,
    pub bits: i32,
}

/// Gemma 3/4 model configuration — common across text-only and the text
/// component of multimodal checkpoints.
#[derive(Debug, Clone, Deserialize)]
pub struct Gemma4Config {
    pub model_type: String,
    pub hidden_size: i32,
    pub num_hidden_layers: i32,
    pub intermediate_size: i32,
    pub num_attention_heads: i32,
    pub num_key_value_heads: i32,
    pub head_dim: i32,
    pub rms_norm_eps: f32,
    pub vocab_size: i32,
    pub max_position_embeddings: i32,
    #[serde(default = "default_rope_theta")]
    pub rope_theta: f32,
    #[serde(default = "default_tie_word_embeddings")]
    pub tie_word_embeddings: bool,
    #[serde(default)]
    pub attention_bias: bool,
    /// Attention scale denominator: `1 / sqrt(query_pre_attn_scalar)`.
    /// Falls back to `head_dim` when absent.
    #[serde(default)]
    pub query_pre_attn_scalar: Option<i32>,
    /// Sliding window size for local attention layers.
    #[serde(default)]
    pub sliding_window: Option<i32>,
    /// Period of the global attention layer pattern.  Every layer whose
    /// 1-indexed position is a multiple of this value is a **global** layer.
    /// Default 6 → layers 5,11,17,… (0-indexed) are global.
    #[serde(default = "default_sliding_window_pattern")]
    pub sliding_window_pattern: i32,
    /// Final logit soft-capping (Gemma 3/4 typically sets this to `None`).
    #[serde(default)]
    pub final_logit_softcapping: Option<f32>,
    #[serde(default)]
    pub quantization: Option<QuantizationConfig>,
}

impl Gemma4Config {
    pub fn attn_scale(&self) -> f32 {
        let s = self.query_pre_attn_scalar.unwrap_or(self.head_dim);
        (s as f32).sqrt().recip()
    }

    /// Returns `true` when layer `layer_idx` (0-indexed) is a local
    /// (sliding-window) attention layer.
    pub fn is_local_layer(&self, layer_idx: i32) -> bool {
        if self.sliding_window.is_none() || self.sliding_window_pattern <= 0 {
            return false;
        }
        (layer_idx + 1) % self.sliding_window_pattern != 0
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Boolean sliding-window mask: `mask[i, j] = true` iff key j is within the
/// window for query i.  Only enforces the lower bound; the causal mask handles
/// the upper bound.
#[allow(non_snake_case)]
fn create_sliding_window_mask(L: i32, S: i32, window: i32) -> Result<Array, Exception> {
    let offset = S - L;
    let q_pos = arange!(start = offset, stop = offset + L)?;
    let k_pos = arange!(stop = S)?;
    let lower = q_pos.subtract(array!(window - 1))?.reshape(&[L, 1])?;
    k_pos.reshape(&[1, S])?.ge(&lower)
}

fn is_single_decode(queries: &Array) -> bool {
    matches!(queries.shape(), [1, _, 1, _])
}

// ---------------------------------------------------------------------------
// Attention
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
struct Gemma4Attention {
    n_heads: i32,
    n_kv_heads: i32,
    n_rep: i32,
    scale: f32,
    /// `Some(w)` → local layer with window size `w`; `None` → global layer.
    sliding_window: Option<i32>,

    // dtype-matched scalar caches to avoid allocation and dtype promotion
    cached_scale: Option<Array>,
    cached_neg_inf: Option<Array>,

    #[quantizable]
    #[param]
    q_proj: MaybeQuantized<nn::Linear>,
    #[quantizable]
    #[param]
    k_proj: MaybeQuantized<nn::Linear>,
    #[quantizable]
    #[param]
    v_proj: MaybeQuantized<nn::Linear>,
    #[quantizable]
    #[param]
    o_proj: MaybeQuantized<nn::Linear>,
    #[param]
    q_norm: nn::RmsNorm,
    #[param]
    k_norm: nn::RmsNorm,
    #[param]
    rope: nn::Rope,
}

impl Gemma4Attention {
    fn new(args: &Gemma4Config, local: bool) -> Result<Self, Exception> {
        let hd = args.head_dim;
        let nh = args.num_attention_heads;
        let nkv = args.num_key_value_heads;
        let bias = args.attention_bias;

        let q_proj = nn::LinearBuilder::new(args.hidden_size, nh * hd).bias(bias).build()?;
        let k_proj = nn::LinearBuilder::new(args.hidden_size, nkv * hd).bias(bias).build()?;
        let v_proj = nn::LinearBuilder::new(args.hidden_size, nkv * hd).bias(bias).build()?;
        let o_proj = nn::LinearBuilder::new(nh * hd, args.hidden_size).bias(bias).build()?;
        let q_norm = nn::RmsNormBuilder::new(hd).eps(args.rms_norm_eps).build()?;
        let k_norm = nn::RmsNormBuilder::new(hd).eps(args.rms_norm_eps).build()?;
        let rope = nn::RopeBuilder::new(hd)
            .traditional(false)
            .base(args.rope_theta)
            .scale(1.0)
            .build()
            .map_err(|e| Exception::custom(format!("RoPE: {e}")))?;

        Ok(Self {
            n_heads: nh,
            n_kv_heads: nkv,
            n_rep: nh / nkv,
            scale: args.attn_scale(),
            sliding_window: if local { args.sliding_window } else { None },
            cached_scale: None,
            cached_neg_inf: None,
            q_proj: MaybeQuantized::Original(q_proj),
            k_proj: MaybeQuantized::Original(k_proj),
            v_proj: MaybeQuantized::Original(v_proj),
            o_proj: MaybeQuantized::Original(o_proj),
            q_norm,
            k_norm,
            rope,
        })
    }

    fn ensure_scale_cache(&mut self, dtype: mlx_rs::Dtype) -> Result<(), Exception> {
        if self.cached_scale.as_ref().is_none_or(|c| c.dtype() != dtype) {
            self.cached_scale = Some(array!(self.scale).as_dtype(dtype)?);
        }
        Ok(())
    }

    fn ensure_neg_inf_cache(&mut self, dtype: mlx_rs::Dtype) -> Result<(), Exception> {
        if self.cached_neg_inf.as_ref().is_none_or(|c| c.dtype() != dtype) {
            self.cached_neg_inf = Some(array!(f32::NEG_INFINITY).as_dtype(dtype)?);
        }
        Ok(())
    }
}

struct Gemma4AttnInput<'a, C> {
    x: &'a Array,
    mask: Option<&'a Array>,
    cache: Option<&'a mut C>,
}

impl<C> Module<Gemma4AttnInput<'_, C>> for Gemma4Attention
where
    C: KeyValueCache,
{
    type Output = Array;
    type Error = Exception;

    #[allow(non_snake_case)]
    fn forward(&mut self, input: Gemma4AttnInput<'_, C>) -> Result<Self::Output, Self::Error> {
        let Gemma4AttnInput { x, mask, mut cache } = input;
        let B = x.shape()[0];
        let L = x.shape()[1];

        // Project, reshape heads, apply QK-norm
        let mut queries = self.q_norm.forward(
            &self.q_proj.forward(x)?
                .reshape(&[B, L, self.n_heads, -1])?
                .transpose_axes(&[0, 2, 1, 3])?,
        )?;
        let mut keys = self.k_norm.forward(
            &self.k_proj.forward(x)?
                .reshape(&[B, L, self.n_kv_heads, -1])?
                .transpose_axes(&[0, 2, 1, 3])?,
        )?;
        let mut values = self.v_proj.forward(x)?
            .reshape(&[B, L, self.n_kv_heads, -1])?
            .transpose_axes(&[0, 2, 1, 3])?;

        if let Some(ref mut kv) = cache {
            queries = apply_rope(&queries, &self.rope, kv.offset())?;
            keys = apply_rope(&keys, &self.rope, kv.offset())?;

            match kv.update_and_view(keys, values)? {
                // ── TurboQuant single-token decode ────────────────────────
                KvCacheView::TurboQuant(view) if is_single_decode(&queries) => {
                    self.ensure_scale_cache(queries.dtype())?;
                    let sc = self.cached_scale.as_ref().unwrap();
                    let mut scores = view.decode_scores(&queries, self.n_heads)?.multiply(sc)?;

                    if let Some(w) = self.sliding_window {
                        let s_len = *scores.shape().last()
                            .ok_or_else(|| Exception::custom("scores: no dims"))?;
                        if s_len > w {
                            self.ensure_neg_inf_cache(scores.dtype())?;
                            let wm = create_sliding_window_mask(L, s_len, w)?;
                            let ni = self.cached_neg_inf.as_ref().unwrap();
                            scores = ops::r#where(&wm, &scores, ni)?;
                        }
                    }
                    let weights = ops::softmax_axis(&scores, -1, None)?;
                    let out = view.decode_values(&weights, self.n_heads)?
                        .transpose_axes(&[0, 2, 1, 3])?
                        .reshape(&[B, L, -1])?;
                    return self.o_proj.forward(&out);
                }
                // ── Dense path (prefill or TQ fallback) ──────────────────
                other @ (KvCacheView::Dense { .. } | KvCacheView::TurboQuant(_)) => {
                    let (ck, cv) = other.into_dense()?;
                    keys = ck;
                    values = cv;
                }
            }
        } else {
            queries = apply_rope(&queries, &self.rope, 0)?;
            keys = apply_rope(&keys, &self.rope, 0)?;
        }

        // ── Dense attention via 5D GQA broadcast ─────────────────────────
        let kv_s = keys.shape()[2];
        let hd = queries.shape()[3];
        let q5 = queries.reshape(&[B, self.n_kv_heads, self.n_rep, L, hd])?;
        let k5 = keys.reshape(&[B, self.n_kv_heads, 1, kv_s, hd])?;
        let v5 = values.reshape(&[B, self.n_kv_heads, 1, kv_s, hd])?;

        self.ensure_scale_cache(q5.dtype())?;
        let sc = self.cached_scale.as_ref().unwrap();
        let mut scores = q5.matmul(&k5.transpose_axes(&[0, 1, 2, 4, 3])?)?.multiply(sc)?;

        if let Some(m) = mask {
            self.ensure_neg_inf_cache(scores.dtype())?;
            let ni = self.cached_neg_inf.as_ref().unwrap();
            scores = ops::r#where(m, &scores, ni)?;
        }

        if let Some(w) = self.sliding_window {
            if kv_s > w {
                self.ensure_neg_inf_cache(scores.dtype())?;
                let wm = create_sliding_window_mask(L, kv_s, w)?;
                let ni = self.cached_neg_inf.as_ref().unwrap();
                scores = ops::r#where(&wm, &scores, ni)?;
            }
        }

        let weights = ops::softmax_axis(&scores, -1, None)?;
        let out = weights.matmul(&v5)?
            .reshape(&[B, self.n_heads, L, hd])?
            .transpose_axes(&[0, 2, 1, 3])?
            .reshape(&[B, L, -1])?;
        self.o_proj.forward(&out)
    }

    fn training_mode(&mut self, mode: bool) {
        self.q_proj.training_mode(mode);
        self.k_proj.training_mode(mode);
        self.v_proj.training_mode(mode);
        self.o_proj.training_mode(mode);
        self.q_norm.training_mode(mode);
        self.k_norm.training_mode(mode);
        <nn::Rope as Module<nn::RopeInput>>::training_mode(&mut self.rope, mode);
    }
}

// ---------------------------------------------------------------------------
// MLP (GeGLU)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
struct Gemma4Mlp {
    #[quantizable]
    #[param]
    gate_proj: MaybeQuantized<nn::Linear>,
    #[quantizable]
    #[param]
    down_proj: MaybeQuantized<nn::Linear>,
    #[quantizable]
    #[param]
    up_proj: MaybeQuantized<nn::Linear>,
}

impl Gemma4Mlp {
    fn new(dim: i32, hidden_dim: i32) -> Result<Self, Exception> {
        Ok(Self {
            gate_proj: MaybeQuantized::Original(
                nn::LinearBuilder::new(dim, hidden_dim).bias(false).build()?,
            ),
            down_proj: MaybeQuantized::Original(
                nn::LinearBuilder::new(hidden_dim, dim).bias(false).build()?,
            ),
            up_proj: MaybeQuantized::Original(
                nn::LinearBuilder::new(dim, hidden_dim).bias(false).build()?,
            ),
        })
    }
}

impl Module<&Array> for Gemma4Mlp {
    type Output = Array;
    type Error = Exception;

    fn forward(&mut self, x: &Array) -> Result<Self::Output, Self::Error> {
        let gated = nn::gelu_approximate(self.gate_proj.forward(x)?)?
            .multiply(self.up_proj.forward(x)?)?;
        self.down_proj.forward(&gated)
    }

    fn training_mode(&mut self, mode: bool) {
        self.gate_proj.training_mode(mode);
        self.down_proj.training_mode(mode);
        self.up_proj.training_mode(mode);
    }
}

// ---------------------------------------------------------------------------
// Transformer block (4 norms, same layout as Gemma 2)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
struct Gemma4Block {
    #[quantizable]
    #[param]
    self_attn: Gemma4Attention,
    #[quantizable]
    #[param]
    mlp: Gemma4Mlp,
    #[param]
    input_layernorm: nn::RmsNorm,
    #[param]
    post_attention_layernorm: nn::RmsNorm,
    #[param]
    pre_feedforward_layernorm: nn::RmsNorm,
    #[param]
    post_feedforward_layernorm: nn::RmsNorm,
    /// Per-layer learnable scalar that gates residual updates (Gemma 4).
    #[param]
    layer_scalar: Param<Array>,
}

impl Gemma4Block {
    fn new(args: &Gemma4Config, layer_idx: i32) -> Result<Self, Exception> {
        let local = args.is_local_layer(layer_idx);
        Ok(Self {
            self_attn: Gemma4Attention::new(args, local)?,
            mlp: Gemma4Mlp::new(args.hidden_size, args.intermediate_size)?,
            input_layernorm: nn::RmsNormBuilder::new(args.hidden_size)
                .eps(args.rms_norm_eps)
                .build()?,
            post_attention_layernorm: nn::RmsNormBuilder::new(args.hidden_size)
                .eps(args.rms_norm_eps)
                .build()?,
            pre_feedforward_layernorm: nn::RmsNormBuilder::new(args.hidden_size)
                .eps(args.rms_norm_eps)
                .build()?,
            post_feedforward_layernorm: nn::RmsNormBuilder::new(args.hidden_size)
                .eps(args.rms_norm_eps)
                .build()?,
            // Initialized to 1.0 (identity); overwritten by checkpoint weights.
            layer_scalar: Param::new(array!(1.0f32)),
        })
    }
}

impl<C> Module<Gemma4AttnInput<'_, C>> for Gemma4Block
where
    C: KeyValueCache,
{
    type Output = Array;
    type Error = Exception;

    fn forward(&mut self, input: Gemma4AttnInput<'_, C>) -> Result<Self::Output, Self::Error> {
        let Gemma4AttnInput { x, mask, cache } = input;

        let attn_out = self.self_attn.forward(Gemma4AttnInput {
            x: &self.input_layernorm.forward(x)?,
            mask,
            cache,
        })?;
        let h = x.add(
            self.post_attention_layernorm
                .forward(&attn_out)?
                .multiply(&self.layer_scalar.value)?,
        )?;

        let mlp_out = self.mlp.forward(&self.pre_feedforward_layernorm.forward(&h)?)?;
        h.add(
            self.post_feedforward_layernorm
                .forward(&mlp_out)?
                .multiply(&self.layer_scalar.value)?,
        )
    }

    fn training_mode(&mut self, mode: bool) {
        <Gemma4Attention as Module<Gemma4AttnInput<'_, C>>>::training_mode(
            &mut self.self_attn,
            mode,
        );
        self.mlp.training_mode(mode);
        self.input_layernorm.training_mode(mode);
        self.post_attention_layernorm.training_mode(mode);
        self.pre_feedforward_layernorm.training_mode(mode);
        self.post_feedforward_layernorm.training_mode(mode);
    }
}

// ---------------------------------------------------------------------------
// Inner model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
struct Gemma4Model {
    #[quantizable]
    #[param]
    embed_tokens: MaybeQuantized<nn::Embedding>,
    #[quantizable]
    #[param]
    layers: Vec<Gemma4Block>,
    #[param]
    norm: nn::RmsNorm,

    hidden_size: i32,
    cached_embed_scale: Option<Array>,
}

struct Gemma4ModelInput<'a, C> {
    inputs: &'a Array,
    mask: Option<&'a Array>,
    cache: &'a mut Vec<Option<C>>,
}

impl Gemma4Model {
    fn new(args: &Gemma4Config) -> Result<Self, Exception> {
        if !args.vocab_size.is_positive() {
            return Err(Exception::custom("vocab_size must be positive"));
        }
        if !args.num_hidden_layers.is_positive() {
            return Err(Exception::custom("num_hidden_layers must be positive"));
        }
        let layers = (0..args.num_hidden_layers)
            .map(|i| Gemma4Block::new(args, i))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            embed_tokens: MaybeQuantized::Original(
                nn::Embedding::new(args.vocab_size, args.hidden_size)?,
            ),
            layers,
            norm: nn::RmsNormBuilder::new(args.hidden_size)
                .eps(args.rms_norm_eps)
                .build()?,
            hidden_size: args.hidden_size,
            cached_embed_scale: None,
        })
    }
}

impl<C> Module<Gemma4ModelInput<'_, C>> for Gemma4Model
where
    C: KeyValueCache,
{
    type Output = Array;
    type Error = Exception;

    fn forward(&mut self, input: Gemma4ModelInput<'_, C>) -> Result<Self::Output, Self::Error> {
        let Gemma4ModelInput { inputs, mask, cache } = input;

        // Gemma 3/4 scales embeddings by sqrt(hidden_size)
        let mut h = self.embed_tokens.forward(inputs)?;
        if self.cached_embed_scale.is_none() {
            let s = (self.hidden_size as f32).sqrt();
            self.cached_embed_scale = Some(array!(s).as_dtype(h.dtype())?);
        }
        h = h.multiply(self.cached_embed_scale.as_ref().unwrap())?;

        let computed_mask = match mask {
            Some(m) => Some(m.clone()),
            None => match create_attention_mask(&h, cache, Some(true))? {
                Some(AttentionMask::Array(a)) => Some(a),
                Some(AttentionMask::Causal) => {
                    return Err(Exception::custom("only Array mask supported"));
                }
                None => None,
            },
        };

        if cache.is_empty() {
            *cache = (0..self.layers.len()).map(|_| None).collect();
        } else if cache.len() != self.layers.len() {
            return Err(Exception::custom(format!(
                "cache length {} != num_layers {}",
                cache.len(),
                self.layers.len()
            )));
        }

        for (layer, c) in self.layers.iter_mut().zip(cache.iter_mut()) {
            h = layer.forward(Gemma4AttnInput {
                x: &h,
                mask: computed_mask.as_ref(),
                cache: c.as_mut(),
            })?;
        }
        self.norm.forward(&h)
    }

    fn training_mode(&mut self, mode: bool) {
        self.embed_tokens.training_mode(mode);
        for layer in &mut self.layers {
            <Gemma4Block as Module<Gemma4AttnInput<'_, C>>>::training_mode(layer, mode);
        }
        self.norm.training_mode(mode);
    }
}

// ---------------------------------------------------------------------------
// Causal LM
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct Gemma4CausalLM {
    pub args: Gemma4Config,

    #[quantizable]
    #[param]
    model: Gemma4Model,

    #[quantizable]
    #[param]
    lm_head: Option<MaybeQuantized<nn::Linear>>,

    cached_final_inv_cap: Option<Array>,
    cached_final_cap: Option<Array>,
}

impl Gemma4CausalLM {
    pub fn new(args: Gemma4Config) -> Result<Self, Exception> {
        let model = Gemma4Model::new(&args)?;
        let lm_head = if args.tie_word_embeddings {
            None
        } else {
            Some(MaybeQuantized::Original(
                nn::LinearBuilder::new(args.hidden_size, args.vocab_size)
                    .bias(false)
                    .build()?,
            ))
        };
        Ok(Self { args, model, lm_head, cached_final_inv_cap: None, cached_final_cap: None })
    }

    /// Forward pass. `kv_cache` must have length 0 (auto-init) or `num_hidden_layers`.
    pub fn forward<C: KeyValueCache>(
        &mut self,
        inputs: &Array,
        mask: Option<&Array>,
        kv_cache: &mut Vec<Option<C>>,
    ) -> Result<Array, Exception> {
        let h = self.model.forward(Gemma4ModelInput { inputs, mask, cache: kv_cache })?;

        // Take the last token for LM head (same pattern as Gemma 2)
        let seq_len = inputs.shape().get(1).copied().unwrap_or(1);
        let lm_input = if seq_len > 1 {
            h.index((.., -1i32.., ..)).index((.., -1i32.., ..))
        } else {
            h.index((.., -1i32.., ..))
        };

        let mut logits = match self.lm_head.as_mut() {
            Some(head) => head.forward(&lm_input)?,
            None => match &mut self.model.embed_tokens {
                MaybeQuantized::Original(e) => e.as_linear(&lm_input)?,
                MaybeQuantized::Quantized(qe) => qe.as_linear(&lm_input)?,
            },
        };

        if let Some(cap) = self.args.final_logit_softcapping {
            let refresh = self
                .cached_final_inv_cap
                .as_ref()
                .is_none_or(|c| c.dtype() != logits.dtype());
            if refresh {
                self.cached_final_inv_cap = Some(array!(1.0 / cap).as_dtype(logits.dtype())?);
                self.cached_final_cap = Some(array!(cap).as_dtype(logits.dtype())?);
            }
            let inv = self.cached_final_inv_cap.as_ref().unwrap();
            let c = self.cached_final_cap.as_ref().unwrap();
            logits = ops::tanh(&logits.multiply(inv)?)?.multiply(c)?;
        }

        Ok(logits)
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Returns `(config, weight_prefix)`.
/// For multimodal (VLM) checkpoints the language-model weights are stored under
/// `language_model.*`; `weight_prefix` is set to `"language_model."` in that case.
pub fn load_gemma4_model_args<P: AsRef<Path>>(
    model_dir: P,
) -> Result<(Gemma4Config, Option<String>), ModelError> {
    let path = model_dir.as_ref().join("config.json");
    let raw: serde_json::Value = serde_json::from_reader(std::fs::File::open(&path)?)?;

    // Multimodal checkpoints (e.g. gemma4 VLM) nest text fields under `text_config`.
    // Weights are stored with a `language_model.` prefix in that case.
    let is_vlm = raw.get("hidden_size").is_none() && raw.get("text_config").is_some();

    let value = if is_vlm {
        if let Some(text_cfg) = raw.get("text_config").and_then(|v| v.as_object()) {
            // Start from root (contains model_type, architectures, etc.) then
            // let text_config fields OVERRIDE — its quantization/hidden_size/etc.
            // are authoritative for the language-model component.
            let mut merged = raw.as_object().cloned().unwrap_or_default();
            for (k, v) in text_cfg {
                merged.insert(k.clone(), v.clone());
            }
            serde_json::Value::Object(merged)
        } else {
            raw
        }
    } else {
        raw
    };

    let cfg = serde_json::from_value(value)?;
    let prefix = if is_vlm { Some("language_model.".to_owned()) } else { None };
    Ok((cfg, prefix))
}

/// Load a Gemma 3/4 model from a directory.
///
/// Applies quantization structure when `config.json` contains a `quantization`
/// block, then loads safetensors weights and applies the RMSNorm +1 convention.
pub fn load_gemma4_model<P: AsRef<Path>>(model_dir: P) -> Result<Gemma4CausalLM, ModelError> {
    let model_path = model_dir.as_ref();
    let (args, weight_prefix) = load_gemma4_model_args(model_path)?;

    tracing::info!(
        model_type = %args.model_type,
        hidden = args.hidden_size,
        layers = args.num_hidden_layers,
        heads = args.num_attention_heads,
        kv_heads = args.num_key_value_heads,
        head_dim = args.head_dim,
        vocab = args.vocab_size,
        vlm = weight_prefix.is_some(),
        "Loading Gemma 3/4 model"
    );

    let quantization = args.quantization.clone();
    let raw = Gemma4CausalLM::new(args)
        .map_err(|e| ModelError::ShapeMismatch(format!("model init failed: {e}")))?;

    let mut model = if let Some(ref qc) = quantization {
        tracing::info!(group_size = qc.group_size, bits = qc.bits, "Quantizing model structure");
        mlx_rs::nn::quantize(raw, qc.group_size, qc.bits)
            .map_err(|e| ModelError::ShapeMismatch(format!("quantize failed: {e}")))?
    } else {
        raw
    };

    if let Some(ref prefix) = weight_prefix {
        super::higgs_weights::load_quantized_safetensors_weights_with_prefix(
            &mut model,
            model_path,
            quantization.is_some(),
            prefix,
        )?;
    } else {
        super::higgs_weights::load_quantized_safetensors_weights(
            &mut model,
            model_path,
            quantization.is_some(),
        )?;
    }

    // Fix bits field on any QuantizedLinear/QuantizedEmbedding whose loaded weight
    // shape implies a different bitwidth than the uniform quantize pass assumed.
    // OptiQ checkpoints assign different bits per layer (e.g. v_proj=8, q_proj=4).
    fix_optiq_bits(&mut model);

    apply_rmsnorm_plus_one(&mut model)
        .map_err(|e| ModelError::ShapeMismatch(format!("RMSNorm +1 failed: {e}")))?;

    tracing::info!("Gemma 3/4 model loaded");
    Ok(model)
}

/// Fix the `bits` field on any `QuantizedLinear` / `QuantizedEmbedding` whose
/// loaded weight shape implies a different bitwidth than the uniform quantize
/// pass assumed.  OptiQ checkpoints assign different bits per layer (e.g.
/// `v_proj=8bit`, `q_proj=4bit`), so we infer the correct value from:
///   bits = 32 × weight_cols / in_features
/// where `in_features = scales_cols × group_size`.
fn fix_optiq_bits(model: &mut Gemma4CausalLM) {
    // Embedding
    fix_embedding_bits(&mut model.model.embed_tokens);

    // Layers
    for layer in &mut model.model.layers {
        fix_linear_bits(&mut layer.self_attn.q_proj);
        fix_linear_bits(&mut layer.self_attn.k_proj);
        fix_linear_bits(&mut layer.self_attn.v_proj);
        fix_linear_bits(&mut layer.self_attn.o_proj);
        fix_linear_bits(&mut layer.mlp.gate_proj);
        fix_linear_bits(&mut layer.mlp.down_proj);
        fix_linear_bits(&mut layer.mlp.up_proj);
    }

    // lm_head (when not tied)
    if let Some(ref mut head) = model.lm_head {
        fix_linear_bits(head);
    }
}

fn fix_linear_bits(m: &mut MaybeQuantized<nn::Linear>) {
    if let MaybeQuantized::Quantized(ref mut ql) = *m {
        let w_cols = ql.inner.weight.value.shape().get(1).copied().unwrap_or(0) as i64;
        let s_cols = ql.scales.value.shape().get(1).copied().unwrap_or(0) as i64;
        let g = ql.group_size as i64;
        if w_cols == 0 || s_cols == 0 || g == 0 { return; }
        let in_features = s_cols * g;
        let inferred = (32 * w_cols / in_features) as i32;
        if (inferred == 4 || inferred == 8) && inferred != ql.bits {
            tracing::debug!(from = ql.bits, to = inferred, "fix_optiq_bits: correcting QuantizedLinear bits");
            ql.bits = inferred;
        }
    }
}

fn fix_embedding_bits(m: &mut MaybeQuantized<nn::Embedding>) {
    if let MaybeQuantized::Quantized(ref mut qe) = *m {
        let w_cols = qe.inner.weight.value.shape().get(1).copied().unwrap_or(0) as i64;
        let s_cols = qe.scales.value.shape().get(1).copied().unwrap_or(0) as i64;
        let g = qe.group_size as i64;
        if w_cols == 0 || s_cols == 0 || g == 0 { return; }
        let in_features = s_cols * g;
        let inferred = (32 * w_cols / in_features) as i32;
        if (inferred == 4 || inferred == 8) && inferred != qe.bits {
            tracing::debug!(from = qe.bits, to = inferred, "fix_optiq_bits: correcting QuantizedEmbedding bits");
            qe.bits = inferred;
        }
    }
}

/// Add 1.0 to every RMSNorm weight parameter.
///
/// Gemma stores norm weights as `w − 1`.  Adding 1.0 restores the correct
/// `(w + 1) · rms_norm(x)` semantics without changing the computation graph.
fn apply_rmsnorm_plus_one(model: &mut Gemma4CausalLM) -> Result<(), Exception> {
    use std::rc::Rc;
    let one = array!(1.0_f32);
    let mut params = model.parameters_mut().flatten();

    let norm_keys: Vec<Rc<str>> = params
        .keys()
        .filter(|k| k.ends_with(".weight") && k.contains("norm"))
        .cloned()
        .collect();

    for key in &norm_keys {
        if let Some(p) = params.get_mut(&**key) {
            let shifted = p.add(&one)?;
            **p = shifted;
        }
    }

    let eval_targets: Vec<&Array> = norm_keys
        .iter()
        .filter_map(|k| params.get(&**k).map(|p| &**p))
        .collect();
    mlx_rs::transforms::eval(eval_targets)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;
    use super::super::higgs_kv::SteppingKeyValueCache;

    fn mini_args() -> Gemma4Config {
        Gemma4Config {
            model_type: "gemma4".to_owned(),
            hidden_size: 128,
            num_hidden_layers: 6,
            intermediate_size: 256,
            num_attention_heads: 4,
            num_key_value_heads: 2,
            head_dim: 32,
            rms_norm_eps: 1e-6,
            vocab_size: 256,
            max_position_embeddings: 512,
            rope_theta: 10000.0,
            tie_word_embeddings: true,
            attention_bias: false,
            query_pre_attn_scalar: Some(32),
            sliding_window: Some(64),
            sliding_window_pattern: 6,
            final_logit_softcapping: None,
            quantization: None,
        }
    }

    #[test]
    fn local_global_pattern() {
        let args = mini_args();
        // 6-layer model: layers 0-4 local, layer 5 global
        for i in 0..5 {
            assert!(args.is_local_layer(i), "layer {i} should be local");
        }
        assert!(!args.is_local_layer(5), "layer 5 should be global");
    }

    #[test]
    fn attn_scale_from_scalar() {
        let args = mini_args();
        let expected = (32.0_f32).sqrt().recip();
        assert!((args.attn_scale() - expected).abs() < 1e-6);
    }

    #[test]
    fn attn_scale_fallback_to_head_dim() {
        let mut args = mini_args();
        args.query_pre_attn_scalar = None;
        let expected = (32.0_f32).sqrt().recip();
        assert!((args.attn_scale() - expected).abs() < 1e-6);
    }

    #[test]
    fn model_construction_tied() {
        let args = mini_args();
        let m = Gemma4CausalLM::new(args).unwrap();
        assert!(m.lm_head.is_none());
    }

    #[test]
    fn model_construction_untied() {
        let mut args = mini_args();
        args.tie_word_embeddings = false;
        let m = Gemma4CausalLM::new(args).unwrap();
        assert!(m.lm_head.is_some());
    }

    #[test]
    fn forward_returns_right_shape() {
        let args = mini_args();
        let mut model = Gemma4CausalLM::new(args).unwrap();
        let tokens = Array::from_slice(&[1u32, 2, 3, 4], &[1, 4]);
        let mut cache: Vec<Option<SteppingKeyValueCache>> = vec![];
        let logits = model.forward(&tokens, None, &mut cache).unwrap();
        // shape: [1, 1, vocab_size]  (last token only)
        assert_eq!(logits.shape()[2], 256);
    }

    #[test]
    fn sliding_window_mask_shape() {
        let m = create_sliding_window_mask(4, 8, 3).unwrap();
        assert_eq!(m.shape(), &[4, 8]);
    }

    #[test]
    fn config_deserialization() {
        let json = r#"{
            "model_type": "gemma4",
            "hidden_size": 2560,
            "num_hidden_layers": 34,
            "intermediate_size": 10240,
            "num_attention_heads": 8,
            "num_key_value_heads": 4,
            "head_dim": 256,
            "rms_norm_eps": 1e-6,
            "vocab_size": 262144,
            "max_position_embeddings": 131072,
            "query_pre_attn_scalar": 256,
            "sliding_window": 1024,
            "sliding_window_pattern": 6
        }"#;
        let cfg: Gemma4Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.model_type, "gemma4");
        assert_eq!(cfg.num_attention_heads, 8);
        assert_eq!(cfg.sliding_window, Some(1024));
        assert_eq!(cfg.sliding_window_pattern, 6);
        assert!(cfg.final_logit_softcapping.is_none());
        assert!(cfg.tie_word_embeddings); // default true
    }
}
