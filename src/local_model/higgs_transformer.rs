//! Unified transformer model implementation.
//!
//! Supports Qwen2, Llama, and Mistral architectures. Architecture-specific
//! behavior (e.g., Q/K/V bias) is parameterized through `ModelArgs`.

use std::path::Path;

use mlx_rs::{
    Array,
    builder::Builder,
    error::Exception,
    macros::ModuleParameters,
    module::Module,
    nn, ops,
    ops::indexing::IndexOp,
};
use serde::Deserialize;

use super::{
    higgs_cache::{KeyValueCache, SteppingKeyValueCache},
    higgs_error::ModelError,
    higgs_utils::{
        AttentionMask, apply_rope, cached_scaled_dot_product_attention, create_attention_mask,
        create_batched_decode_mask, scaled_dot_product_attention,
    },
};

const fn default_rope_theta() -> f32 {
    10000.0
}

/// Deserialize an `Option<i32>` that may appear as the string `"None"` in
/// some `HuggingFace` configs (e.g., `nanoLLaVA`'s `sliding_window`).
fn deserialize_optional_i32<'de, D>(deserializer: D) -> Result<Option<i32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(n) => n
            .as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("invalid number for i32")),
        serde_json::Value::String(ref s) if s == "None" || s == "null" => Ok(None),
        serde_json::Value::String(_)
        | serde_json::Value::Bool(_)
        | serde_json::Value::Array(_)
        | serde_json::Value::Object(_) => Err(serde::de::Error::custom(format!(
            "expected i32 or null, got {value}"
        ))),
    }
}

/// Quantization parameters from config.json.
#[derive(Debug, Clone, Deserialize)]
pub struct QuantizationConfig {
    pub group_size: i32,
    pub bits: i32,
}

/// Unified model configuration, deserialized from config.json.
///
/// Architecture-specific fields use serde defaults so that configs from
/// Qwen2, Llama, and Mistral all deserialize into the same struct.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelArgs {
    pub model_type: String,
    pub hidden_size: i32,
    pub num_hidden_layers: i32,
    pub intermediate_size: i32,
    pub num_attention_heads: i32,
    pub rms_norm_eps: f32,
    pub vocab_size: i32,
    pub num_key_value_heads: i32,
    pub max_position_embeddings: i32,
    #[serde(default = "default_rope_theta")]
    pub rope_theta: f32,
    #[serde(default)]
    pub tie_word_embeddings: bool,

    // Architecture-specific optional fields
    #[serde(default)]
    pub attention_bias: Option<bool>,
    #[serde(default)]
    pub use_sliding_window: bool,
    #[serde(default, deserialize_with = "deserialize_optional_i32")]
    pub sliding_window: Option<i32>,
    #[serde(default)]
    pub rope_scaling: Option<serde_json::Value>,

    // Quantization (present in pre-quantized MLX models)
    #[serde(default)]
    pub quantization: Option<QuantizationConfig>,
}

impl ModelArgs {
    /// Whether Q/K/V projections should have bias.
    ///
    /// Uses the config's `attention_bias` field when present, otherwise falls
    /// back to architecture defaults (only qwen2 uses bias by default).
    pub fn qkv_bias(&self) -> bool {
        self.attention_bias
            .unwrap_or(matches!(self.model_type.as_str(), "qwen2"))
    }

    /// Head dimension, computed from `hidden_size / num_attention_heads`.
    ///
    /// Panics in debug builds if not evenly divisible.
    pub fn head_dim(&self) -> i32 {
        debug_assert!(
            self.num_attention_heads != 0 && self.hidden_size % self.num_attention_heads == 0,
            "hidden_size ({}) must be divisible by num_attention_heads ({})",
            self.hidden_size,
            self.num_attention_heads
        );
        self.hidden_size / self.num_attention_heads
    }

    /// Validated head dimension that returns an error if not evenly divisible.
    pub fn checked_head_dim(&self) -> Result<i32, ModelError> {
        if self.num_attention_heads == 0 {
            return Err(ModelError::ShapeMismatch(
                "num_attention_heads must be positive".to_owned(),
            ));
        }
        if self.hidden_size % self.num_attention_heads != 0 {
            return Err(ModelError::ShapeMismatch(format!(
                "hidden_size ({}) must be divisible by num_attention_heads ({})",
                self.hidden_size, self.num_attention_heads
            )));
        }
        Ok(self.hidden_size / self.num_attention_heads)
    }
}

/// Multi-head attention module.
#[derive(Debug, Clone, ModuleParameters)]
pub struct Attention {
    pub n_heads: i32,
    pub n_kv_heads: i32,
    pub scale: f32,

    pub q_proj: nn::Linear,
    pub k_proj: nn::Linear,
    pub v_proj: nn::Linear,
    pub o_proj: nn::Linear,
    pub q_norm: Option<nn::RmsNorm>,
    pub k_norm: Option<nn::RmsNorm>,
    pub rope: nn::Rope,
}

impl Attention {
    pub fn new(args: &ModelArgs) -> Result<Self, Exception> {
        let dim = args.hidden_size;
        let n_heads = args.num_attention_heads;
        let n_kv_heads = args.num_key_value_heads;
        let head_dim = args
            .checked_head_dim()
            .map_err(|e| Exception::custom(e.to_string()))?;
        let head_dim_f32 = f32::from(
            i16::try_from(head_dim).map_err(|_| Exception::custom("head_dim out of i16 range"))?,
        );
        let scale = head_dim_f32.sqrt().recip();

        let qkv_bias = args.qkv_bias();
        let q_proj = nn::LinearBuilder::new(dim, n_heads * head_dim)
            .bias(qkv_bias)
            .build()?;
        let k_proj = nn::LinearBuilder::new(dim, n_kv_heads * head_dim)
            .bias(qkv_bias)
            .build()?;
        let v_proj = nn::LinearBuilder::new(dim, n_kv_heads * head_dim)
            .bias(qkv_bias)
            .build()?;
        let o_proj = nn::LinearBuilder::new(n_heads * head_dim, dim)
            .bias(false)
            .build()?;

        let qk_norm = matches!(args.model_type.as_str(), "qwen3");
        let q_norm = qk_norm
            .then(|| {
                nn::RmsNormBuilder::new(head_dim)
                    .eps(args.rms_norm_eps)
                    .build()
            })
            .transpose()?;
        let k_norm = qk_norm
            .then(|| {
                nn::RmsNormBuilder::new(head_dim)
                    .eps(args.rms_norm_eps)
                    .build()
            })
            .transpose()?;

        let rope = nn::RopeBuilder::new(head_dim)
            .traditional(false)
            .base(args.rope_theta)
            .scale(1.0)
            .build()
            .map_err(|e| Exception::custom(format!("Failed to build RoPE: {e}")))?;

        Ok(Self {
            n_heads,
            n_kv_heads,
            scale,
            q_proj,
            k_proj,
            v_proj,
            o_proj,
            q_norm,
            k_norm,
            rope,
        })
    }
}

/// Input to the attention module.
pub struct AttentionInput<'a, C> {
    pub x: &'a Array,
    pub mask: Option<&'a Array>,
    pub cache: Option<&'a mut C>,
}

impl<C> Module<AttentionInput<'_, C>> for Attention
where
    C: KeyValueCache,
{
    type Output = Array;
    type Error = Exception;

    #[allow(non_snake_case)]
    fn forward(&mut self, input: AttentionInput<'_, C>) -> Result<Self::Output, Self::Error> {
        let AttentionInput { x, mask, mut cache } = input;

        let shape = x.shape();
        let B = *shape
            .first()
            .ok_or_else(|| Exception::custom("Input must have at least 2 dimensions"))?;
        let L = *shape
            .get(1)
            .ok_or_else(|| Exception::custom("Input must have at least 2 dimensions"))?;

        let q_raw = self.q_proj.forward(x)?;
        let k_raw = self.k_proj.forward(x)?;
        let v_raw = self.v_proj.forward(x)?;

        let mut queries = q_raw.reshape(&[B, L, self.n_heads, -1])?;
        let mut keys = k_raw.reshape(&[B, L, self.n_kv_heads, -1])?;

        if let Some(ref mut qn) = self.q_norm {
            queries = qn.forward(&queries)?;
        }
        if let Some(ref mut kn) = self.k_norm {
            keys = kn.forward(&keys)?;
        }

        queries = queries.transpose_axes(&[0, 2, 1, 3])?;
        keys = keys.transpose_axes(&[0, 2, 1, 3])?;
        let values = v_raw
            .reshape(&[B, L, self.n_kv_heads, -1])?
            .transpose_axes(&[0, 2, 1, 3])?;

        if let Some(ref mut kv_cache) = cache {
            queries = apply_rope(&queries, &self.rope, kv_cache.offset())?;
            keys = apply_rope(&keys, &self.rope, kv_cache.offset())?;

            let output = cached_scaled_dot_product_attention(
                queries, kv_cache, keys, values, self.scale, mask,
            )?
            .transpose_axes(&[0, 2, 1, 3])?
            .reshape(&[B, L, -1])?;

            return self.o_proj.forward(&output);
        }
        queries = apply_rope(&queries, &self.rope, 0)?;
        keys = apply_rope(&keys, &self.rope, 0)?;

        let output = scaled_dot_product_attention(queries, keys, values, self.scale, mask)?
            .transpose_axes(&[0, 2, 1, 3])?
            .reshape(&[B, L, -1])?;

        self.o_proj.forward(&output)
    }

    fn training_mode(&mut self, mode: bool) {
        self.q_proj.training_mode(mode);
        self.k_proj.training_mode(mode);
        self.v_proj.training_mode(mode);
        self.o_proj.training_mode(mode);
        if let Some(ref mut qn) = self.q_norm {
            qn.training_mode(mode);
        }
        if let Some(ref mut kn) = self.k_norm {
            kn.training_mode(mode);
        }
        <nn::Rope as Module<nn::RopeInput>>::training_mode(&mut self.rope, mode);
    }
}

/// SiLU-gated MLP.
#[derive(Debug, Clone, ModuleParameters)]
pub struct Mlp {
    pub gate_proj: nn::Linear,
    pub down_proj: nn::Linear,
    pub up_proj: nn::Linear,
}

impl Mlp {
    pub fn new(dim: i32, hidden_dim: i32) -> Result<Self, Exception> {
        let gate_proj = nn::LinearBuilder::new(dim, hidden_dim)
            .bias(false)
            .build()?;
        let down_proj = nn::LinearBuilder::new(hidden_dim, dim)
            .bias(false)
            .build()?;
        let up_proj = nn::LinearBuilder::new(dim, hidden_dim)
            .bias(false)
            .build()?;

        Ok(Self {
            gate_proj,
            down_proj,
            up_proj,
        })
    }
}

impl Module<&Array> for Mlp {
    type Output = Array;
    type Error = Exception;

    fn forward(&mut self, input: &Array) -> Result<Self::Output, Self::Error> {
        let gated =
            nn::silu(self.gate_proj.forward(input)?)?.multiply(self.up_proj.forward(input)?)?;
        self.down_proj.forward(&gated)
    }

    fn training_mode(&mut self, mode: bool) {
        self.gate_proj.training_mode(mode);
        self.down_proj.training_mode(mode);
        self.up_proj.training_mode(mode);
    }
}

/// A single transformer block (attention + MLP with residual connections).
#[derive(Debug, Clone, ModuleParameters)]
pub struct TransformerBlock {
    pub num_attention_heads: i32,
    pub hidden_size: i32,

    pub self_attn: Attention,
    pub mlp: Mlp,
    pub input_layernorm: nn::RmsNorm,
    pub post_attention_layernorm: nn::RmsNorm,
}

impl TransformerBlock {
    pub fn new(args: &ModelArgs) -> Result<Self, Exception> {
        Ok(Self {
            num_attention_heads: args.num_attention_heads,
            hidden_size: args.hidden_size,
            self_attn: Attention::new(args)?,
            mlp: Mlp::new(args.hidden_size, args.intermediate_size)?,
            input_layernorm: nn::RmsNormBuilder::new(args.hidden_size)
                .eps(args.rms_norm_eps)
                .build()?,
            post_attention_layernorm: nn::RmsNormBuilder::new(args.hidden_size)
                .eps(args.rms_norm_eps)
                .build()?,
        })
    }
}

impl<C> Module<AttentionInput<'_, C>> for TransformerBlock
where
    C: KeyValueCache,
{
    type Output = Array;
    type Error = Exception;

    fn forward(&mut self, input: AttentionInput<'_, C>) -> Result<Self::Output, Self::Error> {
        let AttentionInput { x, mask, cache } = input;

        let normed = self.input_layernorm.forward(x)?;
        let residual = self.self_attn.forward(AttentionInput {
            x: &normed,
            mask,
            cache,
        })?;
        let h = x.add(residual)?;

        let normed_post = self.post_attention_layernorm.forward(&h)?;
        let mlp_out = self.mlp.forward(&normed_post)?;
        h.add(mlp_out)
    }

    fn training_mode(&mut self, mode: bool) {
        <Attention as Module<AttentionInput<'_, C>>>::training_mode(&mut self.self_attn, mode);
        self.mlp.training_mode(mode);
        self.input_layernorm.training_mode(mode);
        self.post_attention_layernorm.training_mode(mode);
    }
}

/// Transformer model (embedding + layers + norm, without LM head).
#[derive(Debug, Clone, ModuleParameters)]
struct TransformerModel {
    pub vocab_size: i32,
    pub num_hidden_layers: i32,

    pub embed_tokens: nn::Embedding,
    pub layers: Vec<TransformerBlock>,
    pub norm: nn::RmsNorm,
}

impl TransformerModel {
    fn new(args: &ModelArgs) -> Result<Self, Exception> {
        if !args.vocab_size.is_positive() {
            return Err(Exception::custom("vocab_size must be positive"));
        }
        if !args.num_hidden_layers.is_positive() {
            return Err(Exception::custom("num_hidden_layers must be positive"));
        }
        if !args.num_key_value_heads.is_positive() {
            return Err(Exception::custom("num_key_value_heads must be positive"));
        }

        Ok(Self {
            vocab_size: args.vocab_size,
            num_hidden_layers: args.num_hidden_layers,
            embed_tokens: nn::Embedding::new(
                args.vocab_size,
                args.hidden_size,
            )?,
            layers: (0..args.num_hidden_layers)
                .map(|_| TransformerBlock::new(args))
                .collect::<Result<Vec<_>, _>>()?,
            norm: nn::RmsNormBuilder::new(args.hidden_size)
                .eps(args.rms_norm_eps)
                .build()?,
        })
    }
}

/// Input to the transformer model.
struct ModelInput<'a, C> {
    pub inputs: &'a Array,
    pub mask: Option<&'a Array>,
    pub cache: &'a mut Vec<Option<C>>,
}

impl<C> Module<ModelInput<'_, C>> for TransformerModel
where
    C: KeyValueCache,
{
    type Output = Array;
    type Error = Exception;

    fn forward(&mut self, input: ModelInput<'_, C>) -> Result<Self::Output, Self::Error> {
        let ModelInput {
            inputs,
            mask,
            cache,
        } = input;

        let mut h = self.embed_tokens.forward(inputs)?;

        let computed_mask = match mask {
            Some(m) => Some(m.clone()),
            None => match create_attention_mask(&h, cache, Some(true))? {
                Some(AttentionMask::Array(a)) => Some(a),
                Some(AttentionMask::Causal) => {
                    return Err(Exception::custom("Only Array mask is supported"));
                }
                None => None,
            },
        };

        if cache.is_empty() {
            *cache = (0..self.layers.len()).map(|_| None).collect();
        } else if cache.len() != self.layers.len() {
            return Err(Exception::custom(format!(
                "kv_cache length ({}) must match num layers ({})",
                cache.len(),
                self.layers.len()
            )));
        }

        for (layer, layer_cache) in self.layers.iter_mut().zip(cache.iter_mut()) {
            h = layer.forward(AttentionInput {
                x: &h,
                mask: computed_mask.as_ref(),
                cache: layer_cache.as_mut(),
            })?;
        }

        self.norm.forward(&h)
    }

    fn training_mode(&mut self, mode: bool) {
        self.embed_tokens.training_mode(mode);
        for layer in &mut self.layers {
            <TransformerBlock as Module<AttentionInput<'_, C>>>::training_mode(layer, mode);
        }
        self.norm.training_mode(mode);
    }
}

/// Full causal language model with LM head.
#[derive(Debug, Clone)]
pub struct Model {
    pub args: ModelArgs,

    model: TransformerModel,

    lm_head: Option<nn::Linear>,
}

impl Model {
    pub fn new(args: ModelArgs) -> Result<Self, Exception> {
        let model = TransformerModel::new(&args)?;
        let lm_head = if args.tie_word_embeddings {
            None
        } else {
            Some(
                nn::LinearBuilder::new(args.hidden_size, args.vocab_size)
                    .bias(false)
                    .build()?,
            )
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

    /// Run a forward pass returning hidden states before the LM head.
    pub fn forward_hidden<C: KeyValueCache>(
        &mut self,
        inputs: &Array,
        mask: Option<&Array>,
        kv_cache: &mut Vec<Option<C>>,
    ) -> Result<Array, Exception> {
        self.model.forward(ModelInput {
            inputs,
            mask,
            cache: kv_cache,
        })
    }

    /// Run a forward pass producing logits.
    pub fn forward<C: KeyValueCache>(
        &mut self,
        inputs: &Array,
        mask: Option<&Array>,
        kv_cache: &mut Vec<Option<C>>,
    ) -> Result<Array, Exception> {
        let hidden = self.forward_hidden(inputs, mask, kv_cache)?;
        let last = hidden.index((.., -1.., ..));
        self.apply_lm_head(&last)
    }

    /// Get the hidden size.
    pub const fn hidden_size(&self) -> i32 {
        self.args.hidden_size
    }

    /// Number of transformer layers.
    pub const fn num_layers(&self) -> i32 {
        self.args.num_hidden_layers
    }

    /// Look up token embeddings without running the transformer.
    pub fn embed_tokens(&mut self, input_ids: &Array) -> Result<Array, Exception> {
        self.model.embed_tokens.forward(input_ids)
    }

    /// Forward pass starting from pre-computed embeddings (skips embedding lookup).
    /// Used by VLMs that merge text + image embeddings before running the transformer.
    pub fn forward_from_embeddings<C: KeyValueCache>(
        &mut self,
        embeddings: &Array,
        mask: Option<&Array>,
        kv_cache: &mut Vec<Option<C>>,
    ) -> Result<Array, Exception> {
        let computed_mask = match mask {
            Some(m) => Some(m.clone()),
            None => match create_attention_mask(embeddings, kv_cache, Some(true))? {
                Some(AttentionMask::Array(a)) => Some(a),
                Some(AttentionMask::Causal) => {
                    return Err(Exception::custom("Only Array mask is supported"));
                }
                None => None,
            },
        };

        if kv_cache.is_empty() {
            *kv_cache = (0..self.model.layers.len()).map(|_| None).collect();
        } else if kv_cache.len() != self.model.layers.len() {
            return Err(Exception::custom(format!(
                "kv_cache length ({}) must match num layers ({})",
                kv_cache.len(),
                self.model.layers.len()
            )));
        }

        let mut h = embeddings.clone();
        for (layer, layer_cache) in self.model.layers.iter_mut().zip(kv_cache.iter_mut()) {
            h = layer.forward(AttentionInput {
                x: &h,
                mask: computed_mask.as_ref(),
                cache: layer_cache.as_mut(),
            })?;
        }

        let out = self.model.norm.forward(&h)?;
        self.apply_lm_head(&out)
    }

    /// Batched decode: one forward pass for N requests each with 1 token.
    ///
    /// Heavy ops (projections, MLP, LM head) run batched. Per-request ops
    /// (`RoPE`, KV cache update) loop over individual requests since each has
    /// a different position offset and cache state.
    #[allow(clippy::too_many_lines, clippy::indexing_slicing)]
    pub fn forward_batched(
        &mut self,
        inputs: &Array,
        kv_caches: &mut [&mut Vec<Option<SteppingKeyValueCache>>],
    ) -> Result<Array, Exception> {
        let n = *inputs
            .shape()
            .first()
            .ok_or_else(|| Exception::custom("inputs must have batch dimension"))?;
        let num_layers = self.model.layers.len();
        let n_usize = usize::try_from(n).map_err(|_| Exception::custom("batch size overflow"))?;
        if kv_caches.len() != n_usize {
            return Err(Exception::custom("kv_caches length must match batch size"));
        }
        for (i, cache) in kv_caches.iter().enumerate() {
            if cache.len() != num_layers {
                return Err(Exception::custom(format!(
                    "kv_cache[{i}] length ({}) must match num layers ({num_layers})",
                    cache.len()
                )));
            }
        }
        let head_dim = self.args.head_dim();

        // Per-request offsets (from layer 0's cache, all layers have the same offset)
        let offsets: Vec<i32> = kv_caches
            .iter()
            .map(|req| {
                req.first()
                    .and_then(Option::as_ref)
                    .map_or(0, KeyValueCache::offset)
            })
            .collect();
        let max_kv_len = offsets.iter().map(|&o| o + 1).max().unwrap_or(1);
        let kv_lengths: Vec<i32> = offsets.iter().map(|&o| o + 1).collect();

        let mut h = self.model.embed_tokens.forward(inputs)?;

        for (layer_idx, layer) in self.model.layers.iter_mut().enumerate() {
            let n_heads = layer.self_attn.n_heads;
            let n_kv_heads = layer.self_attn.n_kv_heads;
            let scale = layer.self_attn.scale;

            // Extract RoPE params as scalars (avoids borrow conflict with mutable layer)
            let rope_dims = layer.self_attn.rope.dimensions;
            let rope_traditional = layer.self_attn.rope.traditional;
            let rope_base = layer.self_attn.rope.base;
            let rope_scale = layer.self_attn.rope.scale;

            // --- Batched: layernorm + Q/K/V projections ---
            let normed = layer.input_layernorm.forward(&h)?;
            let q_raw = layer.self_attn.q_proj.forward(&normed)?;
            let k_raw = layer.self_attn.k_proj.forward(&normed)?;
            let v_raw = layer.self_attn.v_proj.forward(&normed)?;

            // [N, 1, proj_dim] -> [N, heads, 1, head_dim]
            let mut queries = q_raw
                .reshape(&[n, 1, n_heads, -1])?
                .transpose_axes(&[0, 2, 1, 3])?;
            let mut keys = k_raw
                .reshape(&[n, 1, n_kv_heads, -1])?
                .transpose_axes(&[0, 2, 1, 3])?;
            let values = v_raw
                .reshape(&[n, 1, n_kv_heads, -1])?
                .transpose_axes(&[0, 2, 1, 3])?;

            // --- Batched: QK norm (Qwen3) ---
            if let Some(ref mut qn) = layer.self_attn.q_norm {
                queries = qn.forward(&queries)?;
            }
            if let Some(ref mut kn) = layer.self_attn.k_norm {
                keys = kn.forward(&keys)?;
            }

            // --- Per-request: RoPE + KV cache update + pad ---
            // Flatten to 2D for reliable per-request slicing
            let q_flat = queries.reshape(&[n, n_heads * head_dim])?;
            let k_flat = keys.reshape(&[n, n_kv_heads * head_dim])?;
            let v_flat = values.reshape(&[n, n_kv_heads * head_dim])?;

            let mut all_queries = Vec::with_capacity(n_usize);
            let mut all_keys = Vec::with_capacity(n_usize);
            let mut all_values = Vec::with_capacity(n_usize);

            for (req_idx, &offset) in offsets.iter().enumerate() {
                let i = i32::try_from(req_idx)
                    .map_err(|_| Exception::custom("request index overflow"))?;

                let q_i = q_flat
                    .index((i..i + 1, ..))
                    .reshape(&[1, n_heads, 1, head_dim])?;
                let k_i = k_flat
                    .index((i..i + 1, ..))
                    .reshape(&[1, n_kv_heads, 1, head_dim])?;
                let v_i = v_flat
                    .index((i..i + 1, ..))
                    .reshape(&[1, n_kv_heads, 1, head_dim])?;

                // RoPE with this request's offset
                let q_rope = mlx_rs::fast::rope(
                    &q_i,
                    rope_dims,
                    rope_traditional,
                    rope_base,
                    rope_scale,
                    offset,
                    None,
                )?;
                let k_rope = mlx_rs::fast::rope(
                    &k_i,
                    rope_dims,
                    rope_traditional,
                    rope_base,
                    rope_scale,
                    offset,
                    None,
                )?;

                // Update this request's KV cache
                let cache = kv_caches[req_idx][layer_idx]
                    .as_mut()
                    .ok_or_else(|| Exception::custom("Cache not initialized"))?;
                let (full_k, full_v) = cache.update_and_fetch(k_rope, v_i)?;

                // Right-pad shorter caches to max_kv_len
                let seq_len = full_k.shape()[2];
                if seq_len < max_kv_len {
                    let pad_len = max_kv_len - seq_len;
                    let pad_k =
                        ops::zeros_dtype(&[1, n_kv_heads, pad_len, head_dim], full_k.dtype())?;
                    let pad_v =
                        ops::zeros_dtype(&[1, n_kv_heads, pad_len, head_dim], full_v.dtype())?;
                    all_keys.push(ops::concatenate_axis(&[&full_k, &pad_k], 2)?);
                    all_values.push(ops::concatenate_axis(&[&full_v, &pad_v], 2)?);
                } else {
                    all_keys.push(full_k);
                    all_values.push(full_v);
                }
                all_queries.push(q_rope);
            }

            // --- Batched: stack + SDPA + output proj + MLP ---
            let stacked_q = ops::concatenate_axis(&all_queries.iter().collect::<Vec<_>>(), 0)?;
            let stacked_k = ops::concatenate_axis(&all_keys.iter().collect::<Vec<_>>(), 0)?;
            let stacked_v = ops::concatenate_axis(&all_values.iter().collect::<Vec<_>>(), 0)?;

            let mask = create_batched_decode_mask(&kv_lengths, max_kv_len)?;

            let attn_out =
                scaled_dot_product_attention(stacked_q, stacked_k, stacked_v, scale, Some(&mask))?;

            let attn_flat = attn_out
                .transpose_axes(&[0, 2, 1, 3])?
                .reshape(&[n, 1, -1])?;
            let residual = layer.self_attn.o_proj.forward(&attn_flat)?;
            h = h.add(residual)?;

            let normed_post = layer.post_attention_layernorm.forward(&h)?;
            let mlp_out = layer.mlp.forward(&normed_post)?;
            h = h.add(mlp_out)?;
        }

        let out = self.model.norm.forward(&h)?;
        self.apply_lm_head(&out)
    }

    /// Apply the LM head to hidden states (last position only during prefill).
    #[allow(non_snake_case)]
    fn apply_lm_head(&mut self, hidden: &Array) -> Result<Array, Exception> {
        let t = hidden.shape().get(1).copied().unwrap_or(1);
        let lm_input = if t > 1 {
            hidden.index((.., -1.., ..))
        } else {
            hidden.clone()
        };
        match self.lm_head.as_mut() {
            Some(head) => head.forward(&lm_input),
            None => self.model.embed_tokens.as_linear(&lm_input),
        }
    }
}

// --- Loading ---

/// Load model args from config.json.
pub fn load_model_args<P: AsRef<Path>>(model_dir: P) -> Result<ModelArgs, ModelError> {
    let config_path = model_dir.as_ref().join("config.json");
    let file = std::fs::File::open(config_path)?;
    Ok(serde_json::from_reader(file)?)
}

/// Load model args from the `text_config` section of config.json (used by VLMs).
pub fn load_text_config_args<P: AsRef<Path>>(model_dir: P) -> Result<ModelArgs, ModelError> {
    let config_path = model_dir.as_ref().join("config.json");
    let file = std::fs::File::open(config_path)?;
    let config: serde_json::Value = serde_json::from_reader(file)?;

    let text_config = config
        .get("text_config")
        .ok_or_else(|| ModelError::UnsupportedModel("missing text_config in config.json".into()))?;

    // Merge top-level quantization config into text_config
    let mut text_obj = text_config.clone();
    if let Some(quant) = config.get("quantization") {
        if let Some(obj) = text_obj.as_object_mut() {
            obj.insert("quantization".to_owned(), quant.clone());
        }
    }
    // Also merge tie_word_embeddings from top level if not in text_config
    if text_obj.get("tie_word_embeddings").is_none() {
        if let Some(tie) = config.get("tie_word_embeddings") {
            if let Some(obj) = text_obj.as_object_mut() {
                obj.insert("tie_word_embeddings".to_owned(), tie.clone());
            }
        }
    }

    Ok(serde_json::from_value(text_obj)?)
}
