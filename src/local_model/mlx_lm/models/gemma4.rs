//! Gemma-4 (text-only) inference — `model_type = gemma4` / `gemma4_text`.
//!
//! Supports `mlx-community/gemma-4-{e2b,e4b}-it-*` checkpoints. The vision and
//! audio towers of `Gemma4ForConditionalGeneration` are skipped: safetensors
//! keys are stripped of the `language_model.` prefix (see `load_gemma4_any` in
//! `mlx_native.rs`) and the `vision_tower.*` / `audio_tower.*` / `embed_vision.*`
//! / `embed_audio.*` weights are simply left unmatched.
//!
//! ## What Gemma-4 does differently from Gemma-3 (why this is a separate module)
//!
//! Ported faithfully from Apple's `mlx_lm/models/gemma4_text.py` (© 2025 Apple).
//!
//! - **Standard RMSNorm** (`weight * normalize(x)`), NOT Gemma's historical
//!   `(1 + weight)` form. The reference uses `nn.RMSNorm` directly with no `+1`
//!   and no sanitize fix-up, so the checkpoints store norm weights centred at
//!   ~1.0. We therefore use [`mlx_rs::nn::RmsNorm`] (and a scale-free variant
//!   for the value norm), unlike `gemma3::GemmaRmsNorm`.
//! - **Per-Layer Embeddings (PLE)** — the "effective params" mechanism behind
//!   the `E2B` / `E4B` naming. A second embedding table (`embed_tokens_per_layer`)
//!   plus a model-level projection feed a per-layer side input that is gated
//!   into each block's residual stream. See [`Gemma4TextModel::per_layer_inputs`].
//! - **Cross-layer KV sharing** — the last `num_kv_shared_layers` layers carry
//!   **no** `k_proj` / `v_proj` / `k_norm`; they reuse the (already-RoPE'd,
//!   already-cached) keys/values produced by the most recent earlier layer of
//!   the same attention type. See [`Gemma4TextModel::forward`].
//! - **Per-attention-type head dim** — `full_attention` layers use
//!   `global_head_dim` (512); `sliding_attention` layers use `head_dim` (256).
//! - **Proportional / partial RoPE** — `full_attention` layers rotate only
//!   `partial_rotary_factor * head_dim` dims (theta = 1e6); the rest pass
//!   through unrotated (encoded as `inf` frequencies). `sliding_attention`
//!   layers use plain RoPE (theta = 1e4). See [`ProportionalRope`].
//! - **Double-wide MLP** on KV-shared layers (`intermediate_size * 2`).
//! - **GeGLU** MLP (`gelu_approx(gate) * up`) and **`layer_scalar`** post-block
//!   scaling.
//! - **Attention scale is `1.0`** (queries are pre-normalised by `q_norm`),
//!   not `1/sqrt(head_dim)`.
//! - **Final logit softcap** = 30.0.
//!
//! ## Caching (matches the `gemma3` approach in this crate)
//!
//! Non-shared layers each own an FP16 KV cache (`fp16_with_max`); KV-shared
//! layers hold `None` and reuse intermediates. Sliding-window attention is
//! enforced **only via the prefill mask** — the KV buffer is never shrunk to
//! `sliding_window` (that would break SDPA shapes on a single-pass prefill).
//! As in `gemma3`, this means single-token decode attends to the full cache
//! rather than a strict window; accepted as a known approximation.

use std::{collections::HashMap, path::Path};

use mlx_rs::{
    array,
    builder::Builder,
    error::Exception,
    macros::{ModuleParameters, Quantizable},
    module::{Module, Param},
    nn,
    ops::{
        arange, concatenate_axis, full,
        indexing::IndexOp,
        ones, power,
    },
    quantization::MaybeQuantized,
    Array,
};
use serde::Deserialize;
use serde_json::Value;

use super::super::{
    cache::{KeyValueCache, KvCache, KvFetchResult},
    error::Error,
    utils::{create_causal_mask, scaled_dot_product_attention},
};

// -----------------------------------------------------------------------------
// Config
// -----------------------------------------------------------------------------

/// One entry of `config.json::text_config::rope_parameters`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RopeParam {
    #[serde(default)]
    pub rope_type: Option<String>,
    #[serde(default)]
    pub rope_theta: Option<f32>,
    #[serde(default)]
    pub partial_rotary_factor: Option<f32>,
}

/// Text config schema for `gemma4` / `gemma4_text`. Multimodal checkpoints nest
/// these under `text_config`; [`get_gemma4_model_args`] handles both.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelArgs {
    pub model_type: String,
    pub hidden_size: i32,
    pub num_hidden_layers: i32,
    pub intermediate_size: i32,
    pub num_attention_heads: i32,
    pub head_dim: i32,
    #[serde(default = "default_global_head_dim")]
    pub global_head_dim: i32,
    pub num_key_value_heads: i32,
    #[serde(default)]
    pub num_kv_shared_layers: i32,
    #[serde(default)]
    pub hidden_size_per_layer_input: i32,
    pub vocab_size: i32,
    #[serde(default)]
    pub vocab_size_per_layer_input: i32,
    pub rms_norm_eps: f32,
    pub max_position_embeddings: i32,
    #[serde(default = "default_sliding_window")]
    pub sliding_window: i32,
    #[serde(default = "default_sliding_window_pattern")]
    pub sliding_window_pattern: i32,
    #[serde(default)]
    pub final_logit_softcapping: Option<f32>,
    #[serde(default)]
    pub use_double_wide_mlp: bool,
    #[serde(default = "default_tie_word_embeddings")]
    pub tie_word_embeddings: bool,
    /// Per-layer attention type. Defaults to `sliding_window_pattern-1` sliding
    /// layers followed by one full layer, repeated.
    #[serde(default)]
    pub layer_types: Option<Vec<String>>,
    #[serde(default)]
    pub rope_parameters: Option<HashMap<String, RopeParam>>,

    /// EOS token id(s). Gemma-4 instruct lists `[1, 106, 50]`. Folded in by
    /// [`get_gemma4_model_args`] from the outer config scope.
    #[serde(skip)]
    pub eos_token_ids: Vec<u32>,
    /// BOS token (default 2). The Gemma chat template starts with `{{ bos_token }}`.
    #[serde(skip)]
    pub bos_token_id: Option<u32>,
}

fn default_global_head_dim() -> i32 {
    0
}
fn default_sliding_window() -> i32 {
    512
}
fn default_sliding_window_pattern() -> i32 {
    5
}
fn default_tie_word_embeddings() -> bool {
    true
}

impl ModelArgs {
    /// `first_kv_shared_layer_idx` — layers at or above this index reuse KV.
    pub fn first_kv_shared(&self) -> i32 {
        self.num_hidden_layers - self.num_kv_shared_layers
    }

    /// Resolved per-layer attention types (from config or the default pattern).
    pub fn resolved_layer_types(&self) -> Vec<String> {
        if let Some(lt) = &self.layer_types {
            if lt.len() == self.num_hidden_layers as usize {
                return lt.clone();
            }
        }
        let p = self.sliding_window_pattern.max(1) as usize;
        let n = self.num_hidden_layers as usize;
        (0..n)
            .map(|i| {
                if (i + 1) % p == 0 {
                    "full_attention".to_string()
                } else {
                    "sliding_attention".to_string()
                }
            })
            .collect()
    }

    fn rope_for(&self, layer_type: &str) -> RopeParam {
        if let Some(rp) = &self.rope_parameters {
            if let Some(p) = rp.get(layer_type) {
                return p.clone();
            }
        }
        // Reference defaults.
        if layer_type == "full_attention" {
            RopeParam {
                rope_type: Some("proportional".into()),
                rope_theta: Some(1_000_000.0),
                partial_rotary_factor: Some(0.25),
            }
        } else {
            RopeParam {
                rope_type: Some("default".into()),
                rope_theta: Some(10_000.0),
                partial_rotary_factor: Some(1.0),
            }
        }
    }

    /// Head dim used by a given attention type.
    fn head_dim_for(&self, layer_type: &str) -> i32 {
        if layer_type == "full_attention" && self.global_head_dim > 0 {
            self.global_head_dim
        } else {
            self.head_dim
        }
    }
}

// -----------------------------------------------------------------------------
// RoPE — default (sliding) or proportional/partial (full)
// -----------------------------------------------------------------------------

/// Partial / proportional RoPE: rotate the first `rotated_dims` of each head and
/// leave the rest untouched. Mirrors `mlx_lm.models.rope_utils.ProportionalRoPE`
/// — the non-rotated tail is encoded as `inf` frequencies (rotation angle
/// `pos/inf = 0`, i.e. identity).
#[derive(Debug, Clone, ModuleParameters)]
pub struct ProportionalRope {
    dims: i32,
    /// Precomputed frequencies (not a trainable parameter).
    freqs: Array,
}

impl ProportionalRope {
    pub fn new(dims: i32, rotated_dims: i32, base: f32, factor: f32) -> Result<Self, Exception> {
        // freqs_rotated[i] = factor * base^(2i / dims)  for i in 0..rotated_dims/2
        let exps = arange::<_, f32>(0, rotated_dims, 2)?.divide(&array!(dims as f32))?;
        let rotated = power(&array!(base), &exps)?.multiply(&array!(factor))?;
        let pad_len = ((dims - rotated_dims) / 2).max(0);
        let freqs = if pad_len > 0 {
            let pad = full::<f32>(&[pad_len], &array!(f32::INFINITY))?;
            concatenate_axis(&[rotated, pad], 0)?
        } else {
            rotated
        };
        Ok(Self { dims, freqs })
    }

    /// `x`: `[B, n_heads, L, head_dim]`.
    fn forward(&self, x: &Array, offset: i32) -> Result<Array, Exception> {
        let shape = x.shape().to_vec();
        let x3 = x.reshape(&[-1, x.dim(-2), x.dim(-1)])?;
        let out = mlx_rs::fast::rope(
            &x3,
            self.dims,
            false,
            None::<f32>,
            1.0,
            offset,
            Some(&self.freqs),
        )?;
        out.reshape(&shape)
    }
}

#[derive(Debug, Clone)]
pub enum Gemma4Rope {
    Default(nn::Rope),
    Proportional(ProportionalRope),
}

impl Gemma4Rope {
    fn new(head_dim: i32, param: &RopeParam) -> Result<Self, Exception> {
        let theta = param.rope_theta.unwrap_or(10_000.0);
        let kind = param.rope_type.as_deref().unwrap_or("default");
        if kind == "proportional" {
            let factor = param.partial_rotary_factor.unwrap_or(1.0);
            let rotated = ((head_dim as f32) * factor).round() as i32;
            Ok(Self::Proportional(ProportionalRope::new(
                head_dim, rotated, theta, 1.0,
            )?))
        } else {
            let rope = nn::RopeBuilder::new(head_dim)
                .traditional(false)
                .base(theta)
                .scale(1.0)
                .build()
                .expect("Infallible");
            Ok(Self::Default(rope))
        }
    }

    fn forward(&mut self, x: &Array, offset: i32) -> Result<Array, Exception> {
        match self {
            Self::Default(rope) => {
                let input = nn::RopeInputBuilder::new(x).offset(offset).build()?;
                rope.forward(input)
            }
            Self::Proportional(p) => p.forward(x, offset),
        }
    }
}

// `Gemma4Rope` carries no trainable parameters (RoPE is config-only), but the
// `#[param]` field on `Attention` needs it to be a `ModuleParameters`. Delegate
// to the active variant (both expose empty parameter sets).
impl mlx_rs::module::ModuleParameters for Gemma4Rope {
    fn num_parameters(&self) -> usize {
        0
    }
    fn freeze_parameters(&mut self, recursive: bool) {
        match self {
            Self::Default(r) => r.freeze_parameters(recursive),
            Self::Proportional(r) => r.freeze_parameters(recursive),
        }
    }
    fn unfreeze_parameters(&mut self, recursive: bool) {
        match self {
            Self::Default(r) => r.unfreeze_parameters(recursive),
            Self::Proportional(r) => r.unfreeze_parameters(recursive),
        }
    }
    fn parameters(&self) -> mlx_rs::module::ModuleParamRef<'_> {
        match self {
            Self::Default(r) => r.parameters(),
            Self::Proportional(r) => r.parameters(),
        }
    }
    fn parameters_mut(&mut self) -> mlx_rs::module::ModuleParamMut<'_> {
        match self {
            Self::Default(r) => r.parameters_mut(),
            Self::Proportional(r) => r.parameters_mut(),
        }
    }
    fn trainable_parameters(&self) -> mlx_rs::module::ModuleParamRef<'_> {
        match self {
            Self::Default(r) => r.trainable_parameters(),
            Self::Proportional(r) => r.trainable_parameters(),
        }
    }
    fn all_frozen(&self) -> Option<bool> {
        match self {
            Self::Default(r) => r.all_frozen(),
            Self::Proportional(r) => r.all_frozen(),
        }
    }
    fn any_frozen(&self) -> Option<bool> {
        match self {
            Self::Default(r) => r.any_frozen(),
            Self::Proportional(r) => r.any_frozen(),
        }
    }
}

// -----------------------------------------------------------------------------
// Scale-free RMSNorm (value norm) — `rms_norm(x, ones, eps)`
// -----------------------------------------------------------------------------

fn rms_norm_no_scale(x: &Array, eps: f32) -> Result<Array, Exception> {
    let dim = x.dim(-1);
    let ones = ones::<f32>(&[dim])?;
    mlx_rs::fast::rms_norm(x, &ones, eps)
}

// -----------------------------------------------------------------------------
// Attention
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct Attention {
    pub n_heads: i32,
    pub n_kv_heads: i32,
    pub head_dim: i32,
    pub scale: f32,
    pub is_full: bool,
    /// `false` for KV-shared layers (no k/v projections of their own).
    pub has_kv: bool,

    #[quantizable]
    #[param]
    pub q_proj: MaybeQuantized<nn::Linear>,
    #[quantizable]
    #[param]
    pub k_proj: Option<MaybeQuantized<nn::Linear>>,
    #[quantizable]
    #[param]
    pub v_proj: Option<MaybeQuantized<nn::Linear>>,
    #[quantizable]
    #[param]
    pub o_proj: MaybeQuantized<nn::Linear>,
    #[param]
    pub q_norm: nn::RmsNorm,
    #[param]
    pub k_norm: Option<nn::RmsNorm>,
    #[param]
    pub rope: Gemma4Rope,
}

impl Attention {
    pub fn new(args: &ModelArgs, layer_idx: usize, layer_type: &str) -> Result<Self, Exception> {
        let dim = args.hidden_size;
        let n_heads = args.num_attention_heads;
        let n_kv_heads = args.num_key_value_heads;
        let head_dim = args.head_dim_for(layer_type);
        let is_full = layer_type == "full_attention";
        let has_kv = (layer_idx as i32) < args.first_kv_shared();

        let mk_lin = |i: i32, o: i32| nn::LinearBuilder::new(i, o).bias(false).build();
        let q_proj = mk_lin(dim, n_heads * head_dim)?;
        let o_proj = mk_lin(n_heads * head_dim, dim)?;
        let (k_proj, v_proj, k_norm) = if has_kv {
            (
                Some(MaybeQuantized::Original(mk_lin(dim, n_kv_heads * head_dim)?)),
                Some(MaybeQuantized::Original(mk_lin(dim, n_kv_heads * head_dim)?)),
                Some(
                    nn::RmsNormBuilder::new(head_dim)
                        .eps(args.rms_norm_eps)
                        .build()?,
                ),
            )
        } else {
            (None, None, None)
        };

        let q_norm = nn::RmsNormBuilder::new(head_dim)
            .eps(args.rms_norm_eps)
            .build()?;
        let rope = Gemma4Rope::new(head_dim, &args.rope_for(layer_type))?;

        Ok(Self {
            n_heads,
            n_kv_heads,
            head_dim,
            scale: 1.0,
            is_full,
            has_kv,
            q_proj: MaybeQuantized::Original(q_proj),
            k_proj,
            v_proj,
            o_proj: MaybeQuantized::Original(o_proj),
            q_norm,
            k_norm,
            rope,
        })
    }

    /// Returns `(output, keys, values)` where `keys`/`values` are the full
    /// (RoPE'd, cached) tensors — captured so KV-shared layers can reuse them.
    #[allow(non_snake_case)]
    fn forward(
        &mut self,
        x: &Array,
        mask: Option<&Array>,
        cache: Option<&mut KvCache>,
        shared_kv: Option<(&Array, &Array)>,
        rope_offset: i32,
    ) -> Result<(Array, Array, Array), Exception> {
        let shape = x.shape();
        let B = shape[0];
        let L = shape[1];

        let queries = self.q_proj.forward(x)?;
        let queries = self.q_norm.forward(&queries.reshape(&[B, L, self.n_heads, -1])?)?;
        let mut queries = queries.transpose_axes(&[0, 2, 1, 3])?;
        queries = self.rope.forward(&queries, rope_offset)?;

        // Resolve keys/values: either reused from an earlier layer, or freshly
        // projected, normed and RoPE'd here.
        let (keys, values) = if let Some((k, v)) = shared_kv {
            (k.clone(), v.clone())
        } else {
            let k_proj = self
                .k_proj
                .as_mut()
                .ok_or_else(|| Exception::custom("non-shared layer missing k_proj"))?;
            let raw_k = k_proj.forward(x)?;
            let v_proj = self
                .v_proj
                .as_mut()
                .ok_or_else(|| Exception::custom("non-shared layer missing v_proj"))?;
            let raw_v = v_proj.forward(x)?;

            let k_norm = self
                .k_norm
                .as_mut()
                .ok_or_else(|| Exception::custom("non-shared layer missing k_norm"))?;
            let keys = k_norm.forward(&raw_k.reshape(&[B, L, self.n_kv_heads, -1])?)?;
            let mut keys = keys.transpose_axes(&[0, 2, 1, 3])?;
            keys = self.rope.forward(&keys, rope_offset)?;

            let values = rms_norm_no_scale(&raw_v.reshape(&[B, L, self.n_kv_heads, -1])?, 1e-6)?;
            let values = values.transpose_axes(&[0, 2, 1, 3])?;
            (keys, values)
        };

        // Non-shared layers append to and fetch from their own cache; shared
        // layers reuse the (already full) tensors directly.
        let (k_full, v_full) = if let Some(cache) = cache {
            match cache.update_and_fetch(keys, values)? {
                KvFetchResult::Fp16(k, v) => (k, v),
                KvFetchResult::TurboQuant => {
                    return Err(Exception::custom(
                        "Gemma-4 KV cache must be FP16 (TurboQuant not wired)",
                    ));
                }
            }
        } else {
            (keys, values)
        };

        let output = scaled_dot_product_attention(
            queries,
            k_full.clone(),
            v_full.clone(),
            None::<&mut KvCache>,
            self.scale,
            mask,
        )?
        .transpose_axes(&[0, 2, 1, 3])?
        .reshape(&[B, L, -1])?;

        Ok((self.o_proj.forward(&output)?, k_full, v_full))
    }
}

// -----------------------------------------------------------------------------
// MLP — GeGLU
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct Mlp {
    #[quantizable]
    #[param]
    pub gate_proj: MaybeQuantized<nn::Linear>,
    #[quantizable]
    #[param]
    pub up_proj: MaybeQuantized<nn::Linear>,
    #[quantizable]
    #[param]
    pub down_proj: MaybeQuantized<nn::Linear>,
}

impl Mlp {
    pub fn new(dim: i32, hidden_dim: i32) -> Result<Self, Exception> {
        let mk = |i: i32, o: i32| nn::LinearBuilder::new(i, o).bias(false).build();
        Ok(Self {
            gate_proj: MaybeQuantized::Original(mk(dim, hidden_dim)?),
            up_proj: MaybeQuantized::Original(mk(dim, hidden_dim)?),
            down_proj: MaybeQuantized::Original(mk(hidden_dim, dim)?),
        })
    }

    fn forward(&mut self, x: &Array) -> Result<Array, Exception> {
        let gated = nn::gelu_approximate(self.gate_proj.forward(x)?)?
            .multiply(self.up_proj.forward(x)?)?;
        self.down_proj.forward(&gated)
    }
}

// -----------------------------------------------------------------------------
// Decoder layer — 4 norms + attn + mlp + per-layer-input gate + layer scalar
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct DecoderLayer {
    #[quantizable]
    #[param]
    pub self_attn: Attention,
    #[quantizable]
    #[param]
    pub mlp: Mlp,
    #[param]
    pub input_layernorm: nn::RmsNorm,
    #[param]
    pub post_attention_layernorm: nn::RmsNorm,
    #[param]
    pub pre_feedforward_layernorm: nn::RmsNorm,
    #[param]
    pub post_feedforward_layernorm: nn::RmsNorm,

    // Per-layer input gating (PLE). Present when `hidden_size_per_layer_input > 0`.
    #[quantizable]
    #[param]
    pub per_layer_input_gate: Option<MaybeQuantized<nn::Linear>>,
    #[quantizable]
    #[param]
    pub per_layer_projection: Option<MaybeQuantized<nn::Linear>>,
    #[param]
    pub post_per_layer_input_norm: Option<nn::RmsNorm>,

    #[param]
    pub layer_scalar: Param<Array>,
}

impl DecoderLayer {
    pub fn new(args: &ModelArgs, layer_idx: usize) -> Result<Self, Exception> {
        let layer_types = args.resolved_layer_types();
        let layer_type = &layer_types[layer_idx];
        let self_attn = Attention::new(args, layer_idx, layer_type)?;

        let is_kv_shared = (layer_idx as i32) >= args.first_kv_shared() && args.first_kv_shared() > 0;
        let use_double_wide = args.use_double_wide_mlp && is_kv_shared;
        let inter = args.intermediate_size * if use_double_wide { 2 } else { 1 };
        let mlp = Mlp::new(args.hidden_size, inter)?;

        let mk_norm = || {
            nn::RmsNormBuilder::new(args.hidden_size)
                .eps(args.rms_norm_eps)
                .build()
        };

        let hpl = args.hidden_size_per_layer_input;
        let (gate, proj, norm) = if hpl > 0 {
            (
                Some(MaybeQuantized::Original(
                    nn::LinearBuilder::new(args.hidden_size, hpl).bias(false).build()?,
                )),
                Some(MaybeQuantized::Original(
                    nn::LinearBuilder::new(hpl, args.hidden_size).bias(false).build()?,
                )),
                Some(mk_norm()?),
            )
        } else {
            (None, None, None)
        };

        Ok(Self {
            self_attn,
            mlp,
            input_layernorm: mk_norm()?,
            post_attention_layernorm: mk_norm()?,
            pre_feedforward_layernorm: mk_norm()?,
            post_feedforward_layernorm: mk_norm()?,
            per_layer_input_gate: gate,
            per_layer_projection: proj,
            post_per_layer_input_norm: norm,
            layer_scalar: Param::new(ones::<f32>(&[1])?),
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn forward(
        &mut self,
        x: &Array,
        mask: Option<&Array>,
        cache: Option<&mut KvCache>,
        per_layer_input: Option<&Array>,
        shared_kv: Option<(&Array, &Array)>,
        rope_offset: i32,
    ) -> Result<(Array, Array, Array), Exception> {
        // h = x + post_attn(attn(input_norm(x)))
        let normed = self.input_layernorm.forward(x)?;
        let (attn_out, k_full, v_full) =
            self.self_attn.forward(&normed, mask, cache, shared_kv, rope_offset)?;
        let h = x.add(&self.post_attention_layernorm.forward(&attn_out)?)?;

        // h = h + post_ffn(mlp(pre_ffn(h)))
        let ffn_in = self.pre_feedforward_layernorm.forward(&h)?;
        let ffn_out = self.mlp.forward(&ffn_in)?;
        let mut h = h.add(&self.post_feedforward_layernorm.forward(&ffn_out)?)?;

        // Per-layer input gating.
        if let (Some(gate_lin), Some(proj_lin), Some(norm), Some(ple)) = (
            self.per_layer_input_gate.as_mut(),
            self.per_layer_projection.as_mut(),
            self.post_per_layer_input_norm.as_mut(),
            per_layer_input,
        ) {
            let residual = h.clone();
            let gate = nn::gelu_approximate(gate_lin.forward(&h)?)?;
            let gate = gate.multiply(ple)?;
            let gate = proj_lin.forward(&gate)?;
            let gate = norm.forward(&gate)?;
            h = residual.add(&gate)?;
        }

        // Learned per-layer scalar.
        h = h.multiply(self.layer_scalar.as_ref())?;

        Ok((h, k_full, v_full))
    }
}

// -----------------------------------------------------------------------------
// Backbone
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct Gemma4TextModel {
    pub args: ModelArgs,
    pub layer_types: Vec<String>,
    /// For each layer, the index of the layer whose KV it reuses (== own index
    /// for non-shared layers).
    pub previous_kvs: Vec<usize>,

    #[quantizable]
    #[param]
    pub embed_tokens: MaybeQuantized<nn::Embedding>,
    #[quantizable]
    #[param]
    pub embed_tokens_per_layer: Option<MaybeQuantized<nn::Embedding>>,
    /// NOT quantized in the mlx-community checkpoint (stored bf16) — keep it a
    /// plain `Linear` so `nn::quantize` skips it, else `quantized_matmul` sees a
    /// bf16 weight where it expects uint32.
    #[param]
    pub per_layer_model_projection: Option<nn::Linear>,
    #[param]
    pub per_layer_projection_norm: Option<nn::RmsNorm>,
    #[quantizable]
    #[param]
    pub layers: Vec<DecoderLayer>,
    #[param]
    pub norm: nn::RmsNorm,
}

impl Gemma4TextModel {
    pub fn new(args: &ModelArgs) -> Result<Self, Exception> {
        let n = args.num_hidden_layers as usize;
        let layer_types = args.resolved_layer_types();

        let embed_tokens = nn::Embedding::new(args.vocab_size, args.hidden_size)?;
        let hpl = args.hidden_size_per_layer_input;
        let (eple, plmp, plpn) = if hpl > 0 {
            (
                Some(MaybeQuantized::Original(nn::Embedding::new(
                    args.vocab_size_per_layer_input,
                    args.num_hidden_layers * hpl,
                )?)),
                Some(
                    nn::LinearBuilder::new(args.hidden_size, args.num_hidden_layers * hpl)
                        .bias(false)
                        .build()?,
                ),
                Some(nn::RmsNormBuilder::new(hpl).eps(args.rms_norm_eps).build()?),
            )
        } else {
            (None, None, None)
        };

        let layers = (0..n)
            .map(|i| DecoderLayer::new(args, i))
            .collect::<Result<Vec<_>, _>>()?;
        let norm = nn::RmsNormBuilder::new(args.hidden_size)
            .eps(args.rms_norm_eps)
            .build()?;

        // Map each KV-shared layer to the most recent earlier layer of the same
        // attention type (mirrors the reference `previous_kvs` construction).
        let mut previous_kvs: Vec<usize> = (0..n).collect();
        let m = args.first_kv_shared();
        if args.num_kv_shared_layers > 0 && m > 0 {
            let m = m as usize;
            let mut last_of_type: HashMap<&str, usize> = HashMap::new();
            for (i, lt) in layer_types.iter().enumerate().take(m) {
                last_of_type.insert(lt.as_str(), i);
            }
            for (j, slot) in previous_kvs.iter_mut().enumerate().take(n).skip(m) {
                if let Some(src) = last_of_type.get(layer_types[j].as_str()) {
                    *slot = *src;
                }
            }
        }

        Ok(Self {
            args: args.clone(),
            layer_types,
            previous_kvs,
            embed_tokens: MaybeQuantized::Original(embed_tokens),
            embed_tokens_per_layer: eple,
            per_layer_model_projection: plmp,
            per_layer_projection_norm: plpn,
            layers,
            norm,
        })
    }

    fn embed(&mut self, inputs: &Array) -> Result<Array, Exception> {
        match &mut self.embed_tokens {
            MaybeQuantized::Original(e) => e.forward(inputs),
            MaybeQuantized::Quantized(q) => q.forward(inputs),
        }
    }

    /// Build the `[B, L, num_layers, hidden_per_layer]` per-layer side input.
    fn per_layer_inputs(
        &mut self,
        inputs: &Array,
        scaled_emb: &Array,
    ) -> Result<Option<Array>, Exception> {
        let hpl = self.args.hidden_size_per_layer_input;
        if hpl <= 0 {
            return Ok(None);
        }
        let n = self.args.num_hidden_layers;
        let shape = scaled_emb.shape();
        let (b, l) = (shape[0], shape[1]);

        // Lookup table contribution.
        let eple = self
            .embed_tokens_per_layer
            .as_mut()
            .ok_or_else(|| Exception::custom("PLE configured but embed_tokens_per_layer missing"))?;
        let ple = match eple {
            MaybeQuantized::Original(e) => e.forward(inputs)?,
            MaybeQuantized::Quantized(q) => q.forward(inputs)?,
        };
        let ple = ple
            .multiply(&array!((hpl as f32).sqrt()))?
            .reshape(&[b, l, n, hpl])?;

        // Model-projection contribution.
        let plmp = self
            .per_layer_model_projection
            .as_mut()
            .ok_or_else(|| Exception::custom("PLE configured but per_layer_model_projection missing"))?;
        let proj = plmp.forward(scaled_emb)?;
        let proj = proj
            .multiply(&array!((self.args.hidden_size as f32).powf(-0.5)))?
            .reshape(&[b, l, n, hpl])?;
        let norm = self
            .per_layer_projection_norm
            .as_mut()
            .ok_or_else(|| Exception::custom("PLE configured but per_layer_projection_norm missing"))?;
        let proj = norm.forward(&proj)?;

        // Combine: (proj + ple) * 2^-0.5.
        let combined = proj
            .add(&ple)?
            .multiply(&array!(2.0_f32.powf(-0.5)))?;
        Ok(Some(combined))
    }

    pub fn forward(
        &mut self,
        inputs: &Array,
        caches: &mut [Option<KvCache>],
        rope_offset: usize,
    ) -> Result<Array, Exception> {
        let n = self.layers.len();
        let rope_off = i32::try_from(rope_offset)
            .map_err(|_| Exception::custom("rope_offset exceeds i32::MAX"))?;

        let emb = self.embed(inputs)?;
        let scaled_emb = emb.multiply(&array!((self.args.hidden_size as f32).sqrt()))?;
        let per_layer = self.per_layer_inputs(inputs, &scaled_emb)?;

        // Per-layer masks. Full-attention layers need a causal mask only during
        // multi-token prefill — single-token decode (`seq == 1`) attends the
        // whole cache, which is correct for full attention, so `None`.
        //
        // Sliding-window layers need a windowed mask at EVERY step, including
        // single-token decode: `create_causal_mask(1, offset, window)` yields a
        // `[1, offset+1]` row that restricts the query to the last
        // `sliding_window` keys. Omitting it on decode (as the gemma3 path does)
        // lets sliding layers attend the full cache — out-of-distribution for
        // layers trained with a 512 window, which degrades long generations.
        // The KV buffer is still never shrunk (memory grows with the sequence,
        // same trade-off as the rest of this crate); only attention is windowed.
        let seq = scaled_emb.dim(1);
        let full_mask = if seq <= 1 {
            None
        } else {
            Some(create_causal_mask(seq, Some(rope_off), None, None)?)
        };
        let sliding_mask = Some(create_causal_mask(
            seq,
            Some(rope_off),
            Some(self.args.sliding_window),
            None,
        )?);

        let mut h = scaled_emb;
        // Captured (keys, values) per layer for KV reuse.
        let mut intermediates: Vec<Option<(Array, Array)>> = vec![None; n];

        for idx in 0..n {
            let is_full = self.layer_types[idx] == "full_attention";
            let mask = if is_full {
                full_mask.as_ref()
            } else {
                sliding_mask.as_ref()
            };

            // Per-layer side input slice `[B, L, hpl]`.
            let ple_slice = match &per_layer {
                Some(p) => Some(p.index((.., .., idx as i32, ..))),
                None => None,
            };

            let prev_idx = self.previous_kvs[idx];
            let is_shared = prev_idx != idx;

            let (h_new, k_full, v_full) = if is_shared {
                let (k, v) = intermediates[prev_idx]
                    .clone()
                    .ok_or_else(|| Exception::custom("shared layer has no source KV"))?;
                self.layers[idx].forward(
                    &h,
                    mask,
                    None,
                    ple_slice.as_ref(),
                    Some((&k, &v)),
                    rope_off,
                )?
            } else {
                // SAFETY: caches and layers are disjoint structures; split the
                // borrow so we can hold `&mut KvCache` and `&mut DecoderLayer`.
                let cache = caches.get_mut(idx).and_then(|c| c.as_mut());
                self.layers[idx].forward(&h, mask, cache, ple_slice.as_ref(), None, rope_off)?
            };

            h = h_new;
            intermediates[idx] = Some((k_full, v_full));
        }

        self.norm.forward(&h)
    }
}

// -----------------------------------------------------------------------------
// Top-level model
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct Model {
    pub args: ModelArgs,
    #[quantizable]
    #[param]
    pub model: Gemma4TextModel,
    #[quantizable]
    #[param]
    pub lm_head: Option<MaybeQuantized<nn::Linear>>,
}

impl Model {
    pub fn new(args: ModelArgs) -> Result<Self, Exception> {
        let model = Gemma4TextModel::new(&args)?;
        let lm_head = if !args.tie_word_embeddings {
            Some(MaybeQuantized::Original(
                nn::LinearBuilder::new(args.hidden_size, args.vocab_size)
                    .bias(false)
                    .build()?,
            ))
        } else {
            None
        };
        Ok(Self {
            args,
            model,
            lm_head,
        })
    }

    pub fn model_type(&self) -> &str {
        &self.args.model_type
    }

    /// FP16 KV cache for non-shared layers only; KV-shared layers hold `None`
    /// and reuse intermediates. Length == `num_hidden_layers`.
    pub fn make_caches(&self, max_kv_tokens: i32) -> Vec<Option<KvCache>> {
        let cap = max_kv_tokens.max(1);
        let first_shared = self.args.first_kv_shared();
        (0..self.args.num_hidden_layers)
            .map(|i| {
                if first_shared <= 0 || i < first_shared {
                    Some(KvCache::fp16_with_max(cap))
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn forward(
        &mut self,
        inputs: &Array,
        caches: &mut [Option<KvCache>],
        rope_offset: usize,
    ) -> Result<Array, Exception> {
        let out = self.model.forward(inputs, caches, rope_offset)?;
        let logits = match self.lm_head.as_mut() {
            Some(lm) => lm.forward(&out)?,
            None => match &mut self.model.embed_tokens {
                MaybeQuantized::Original(e) => e.as_linear(&out)?,
                MaybeQuantized::Quantized(q) => q.as_linear(&out)?,
            },
        };
        if let Some(cap) = self.args.final_logit_softcapping {
            let cap_a = array!(cap);
            return mlx_rs::ops::tanh(&logits.divide(&cap_a)?)?.multiply(&cap_a);
        }
        Ok(logits)
    }

    pub fn eval(&self) -> Result<(), Exception> {
        use mlx_rs::module::ModuleParameters;
        mlx_rs::transforms::eval(self.parameters().flatten().values().copied())?;
        Ok(())
    }
}

// -----------------------------------------------------------------------------
// Config loader
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct WeightMap {
    pub metadata: Option<HashMap<String, Value>>,
    pub weight_map: HashMap<String, String>,
}

/// Parse `config.json` for `gemma4` (multimodal wrapper, `text_config` nested)
/// or `gemma4_text` (top-level). Folds the chat-tuned `eos_token_id`
/// (`[1, 106, 50]`) and `bos_token_id` from the outer scope.
pub fn get_gemma4_model_args(model_dir: impl AsRef<Path>) -> Result<ModelArgs, Error> {
    let path = model_dir.as_ref().join("config.json");
    let raw = std::fs::read_to_string(&path)?;
    let root: Value = serde_json::from_str(&raw)?;

    let text_obj = match root.get("text_config") {
        Some(inner) => inner.clone(),
        None => root.clone(),
    };
    let mut args: ModelArgs = serde_json::from_value(text_obj)?;

    let eos_value = root
        .get("eos_token_id")
        .cloned()
        .or_else(|| root.get("text_config").and_then(|t| t.get("eos_token_id")).cloned());
    args.eos_token_ids = match eos_value {
        Some(Value::Number(n)) => n.as_u64().map(|x| vec![x as u32]).unwrap_or_default(),
        Some(Value::Array(arr)) => arr
            .into_iter()
            .filter_map(|v| v.as_u64().map(|x| x as u32))
            .collect(),
        _ => vec![],
    };
    args.bos_token_id = root
        .get("bos_token_id")
        .and_then(|v| v.as_u64())
        .map(|x| x as u32)
        .or(Some(2));

    Ok(args)
}

// -----------------------------------------------------------------------------
// Chat template integration
// -----------------------------------------------------------------------------

impl crate::local_model::chat_template_openai::ChatTemplateModel for Model {
    fn resolve_special_tokens(
        &self,
        template: &str,
        tokenizer: &crate::local_model::mlx_lm_utils::tokenizer::Tokenizer,
    ) -> crate::local_model::chat_template_openai::SpecialTokens {
        use crate::local_model::chat_template_openai::{template_mentions, SpecialTokens};
        let need_bos = template_mentions(template, "bos_token");
        let need_eos = template_mentions(template, "eos_token");
        if !need_bos && !need_eos {
            return SpecialTokens::empty();
        }
        let bos = if need_bos {
            self.args
                .bos_token_id
                .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
        } else {
            None
        };
        let eos = if need_eos {
            self.args
                .eos_token_ids
                .first()
                .copied()
                .and_then(|id| tokenizer.decode(std::slice::from_ref(&id), false).ok())
        } else {
            None
        };
        SpecialTokens { bos, eos }
    }

    fn stop_token_ids(
        &self,
        _tokenizer: &crate::local_model::mlx_lm_utils::tokenizer::Tokenizer,
    ) -> Vec<u32> {
        self.args.eos_token_ids.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `config.json` mirroring `mlx-community/gemma-4-e2b-it-4bit`
    /// (multimodal wrapper with nested `text_config` + top-level eos array).
    fn write_e2b_config(dir: &std::path::Path) {
        let cfg = serde_json::json!({
            "model_type": "gemma4",
            "architectures": ["Gemma4ForConditionalGeneration"],
            "eos_token_id": [1, 106, 50],
            "bos_token_id": 2,
            "text_config": {
                "model_type": "gemma4_text",
                "hidden_size": 1536,
                "num_hidden_layers": 35,
                "intermediate_size": 6144,
                "num_attention_heads": 8,
                "head_dim": 256,
                "global_head_dim": 512,
                "num_key_value_heads": 1,
                "num_kv_shared_layers": 20,
                "hidden_size_per_layer_input": 256,
                "vocab_size": 262144,
                "vocab_size_per_layer_input": 262144,
                "rms_norm_eps": 1e-6,
                "max_position_embeddings": 131072,
                "sliding_window": 512,
                "sliding_window_pattern": 5,
                "final_logit_softcapping": 30.0,
                "use_double_wide_mlp": true,
                "tie_word_embeddings": true,
                "rope_parameters": {
                    "full_attention": {
                        "partial_rotary_factor": 0.25,
                        "rope_theta": 1000000.0,
                        "rope_type": "proportional"
                    },
                    "sliding_attention": {
                        "rope_theta": 10000.0,
                        "rope_type": "default"
                    }
                }
            }
        });
        std::fs::write(dir.join("config.json"), serde_json::to_string(&cfg).unwrap()).unwrap();
    }

    #[test]
    fn parses_text_config_and_folds_eos_bos() {
        let tmp = std::env::temp_dir().join("gemma4_args_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_e2b_config(&tmp);

        let args = get_gemma4_model_args(&tmp).unwrap();
        assert_eq!(args.model_type, "gemma4_text");
        assert_eq!(args.num_hidden_layers, 35);
        assert_eq!(args.head_dim, 256);
        assert_eq!(args.global_head_dim, 512);
        assert_eq!(args.num_kv_shared_layers, 20);
        assert_eq!(args.hidden_size_per_layer_input, 256);
        assert_eq!(args.final_logit_softcapping, Some(30.0));
        assert!(args.use_double_wide_mlp);
        // Chat-tuned EOS array + default BOS folded from outer scope.
        assert_eq!(args.eos_token_ids, vec![1, 106, 50]);
        assert_eq!(args.bos_token_id, Some(2));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn layer_types_pattern_and_head_dims() {
        let tmp = std::env::temp_dir().join("gemma4_layertypes_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_e2b_config(&tmp);
        let args = get_gemma4_model_args(&tmp).unwrap();

        let lt = args.resolved_layer_types();
        assert_eq!(lt.len(), 35);
        // Pattern = 4× sliding then 1× full (sliding_window_pattern = 5):
        // full layers at indices 4, 9, 14, 19, 24, 29, 34.
        for (i, t) in lt.iter().enumerate() {
            let want_full = (i + 1) % 5 == 0;
            assert_eq!(
                t == "full_attention",
                want_full,
                "layer {i} attention type mismatch"
            );
        }
        // Full layers use global_head_dim; sliding use head_dim.
        assert_eq!(args.head_dim_for("full_attention"), 512);
        assert_eq!(args.head_dim_for("sliding_attention"), 256);
        // RoPE: full = proportional/1e6, sliding = default/1e4.
        assert_eq!(args.rope_for("full_attention").rope_type.as_deref(), Some("proportional"));
        assert_eq!(args.rope_for("full_attention").rope_theta, Some(1_000_000.0));
        assert_eq!(args.rope_for("sliding_attention").rope_theta, Some(10_000.0));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    /// KV-shared layers (idx ≥ 15) reuse the most recent earlier same-type
    /// layer: full → 14, sliding → 13. Mirrors the reference `previous_kvs`.
    #[test]
    fn previous_kvs_routing_matches_reference() {
        let tmp = std::env::temp_dir().join("gemma4_prevkv_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        write_e2b_config(&tmp);
        let args = get_gemma4_model_args(&tmp).unwrap();

        let n = args.num_hidden_layers as usize;
        let m = args.first_kv_shared() as usize; // 35 - 20 = 15
        assert_eq!(m, 15);
        let layer_types = args.resolved_layer_types();

        // Replicate the routing logic (pure, no MLX needed).
        let mut previous_kvs: Vec<usize> = (0..n).collect();
        let mut last_of_type: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for (i, lt) in layer_types.iter().enumerate().take(m) {
            last_of_type.insert(lt.as_str(), i);
        }
        for (j, slot) in previous_kvs.iter_mut().enumerate().take(n).skip(m) {
            *slot = last_of_type[layer_types[j].as_str()];
        }

        // Last full in 0..15 is idx 14; last sliding is idx 13.
        assert_eq!(previous_kvs[19], 14, "shared full layer reuses layer 14");
        assert_eq!(previous_kvs[34], 14);
        assert_eq!(previous_kvs[15], 13, "shared sliding layer reuses layer 13");
        // Non-shared layers point to themselves.
        for (i, &p) in previous_kvs.iter().enumerate().take(m) {
            assert_eq!(p, i);
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
