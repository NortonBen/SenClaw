use std::sync::Arc;

use candle_core::{DType, Device, Module, Result, Tensor};
use candle_nn::{embedding, linear_no_bias, rms_norm, Embedding, Linear, RmsNorm, VarBuilder};
use serde::Deserialize;

use super::{cache::KvCache, rope::RotaryEmbedding};

fn default_rope_theta() -> f64 {
    1_000_000.0
}

#[derive(Debug, Deserialize, Clone)]
pub struct Qwen3Config {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize,
    pub head_dim: usize,
    pub rms_norm_eps: f64,
    #[serde(default = "default_rope_theta")]
    pub rope_theta: f64,
    pub max_position_embeddings: usize,
    #[serde(default)]
    pub tie_word_embeddings: bool,
}

// ---------------------------------------------------------------------------
// MLP
// ---------------------------------------------------------------------------

struct Mlp {
    gate_proj: Linear,
    up_proj: Linear,
    down_proj: Linear,
}

impl Mlp {
    fn new(cfg: &Qwen3Config, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            gate_proj: linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("gate_proj"))?,
            up_proj: linear_no_bias(cfg.hidden_size, cfg.intermediate_size, vb.pp("up_proj"))?,
            down_proj: linear_no_bias(cfg.intermediate_size, cfg.hidden_size, vb.pp("down_proj"))?,
        })
    }

    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        // SiLU gated linear unit: down_proj(silu(gate) * up)
        let gate = self.gate_proj.forward(x)?.silu()?;
        let up = self.up_proj.forward(x)?;
        self.down_proj.forward(&gate.mul(&up)?)
    }
}

// ---------------------------------------------------------------------------
// Attention (GQA + QK-norm, always enabled in Qwen3)
// ---------------------------------------------------------------------------

struct Attention {
    q_proj: Linear,
    k_proj: Linear,
    v_proj: Linear,
    o_proj: Linear,
    /// Per-head RMSNorm on queries (Qwen3 always uses QK-norm).
    q_norm: RmsNorm,
    /// Per-head RMSNorm on keys.
    k_norm: RmsNorm,
    rope: Arc<RotaryEmbedding>,
    n_heads: usize,
    n_kv_heads: usize,
    head_dim: usize,
}

impl Attention {
    fn new(cfg: &Qwen3Config, rope: Arc<RotaryEmbedding>, vb: VarBuilder) -> Result<Self> {
        let h = cfg.hidden_size;
        let q_dim = cfg.num_attention_heads * cfg.head_dim;
        let kv_dim = cfg.num_key_value_heads * cfg.head_dim;
        Ok(Self {
            q_proj: linear_no_bias(h, q_dim, vb.pp("q_proj"))?,
            k_proj: linear_no_bias(h, kv_dim, vb.pp("k_proj"))?,
            v_proj: linear_no_bias(h, kv_dim, vb.pp("v_proj"))?,
            o_proj: linear_no_bias(q_dim, h, vb.pp("o_proj"))?,
            q_norm: rms_norm(cfg.head_dim, cfg.rms_norm_eps, vb.pp("q_norm"))?,
            k_norm: rms_norm(cfg.head_dim, cfg.rms_norm_eps, vb.pp("k_norm"))?,
            rope,
            n_heads: cfg.num_attention_heads,
            n_kv_heads: cfg.num_key_value_heads,
            head_dim: cfg.head_dim,
        })
    }

    fn forward(
        &self,
        x: &Tensor,
        offset: usize,
        cache: &mut KvCache,
        mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let (bsz, seq_len, _) = x.dims3()?;

        let q = self.q_proj.forward(x)?;
        let k = self.k_proj.forward(x)?;
        let v = self.v_proj.forward(x)?;

        // Reshape: [bsz, seq_len, n_heads, head_dim] for per-head norm
        let q = q.reshape((bsz, seq_len, self.n_heads, self.head_dim))?;
        let k = k.reshape((bsz, seq_len, self.n_kv_heads, self.head_dim))?;
        let v = v.reshape((bsz, seq_len, self.n_kv_heads, self.head_dim))?;

        // QK-norm (Qwen3: applied before RoPE, on head_dim = last dim)
        let q = self.q_norm.forward(&q)?;
        let k = self.k_norm.forward(&k)?;

        // Transpose to [bsz, n_heads, seq_len, head_dim] for matmul
        let q = q.transpose(1, 2)?;
        let k = k.transpose(1, 2)?;
        let v = v.transpose(1, 2)?;

        // Apply RoPE
        let (q, k) = self.rope.apply(&q, &k, offset)?;

        // Append to KV cache
        let (k, v) = cache.append(k, v)?;

        // Repeat KV heads for GQA
        let k = repeat_kv(k, self.n_heads / self.n_kv_heads)?;
        let v = repeat_kv(v, self.n_heads / self.n_kv_heads)?;

        // Scaled dot-product attention
        let scale = (self.head_dim as f64).sqrt().recip();
        let scores = q.matmul(&k.transpose(2, 3)?)?.affine(scale, 0.0)?;

        let scores = match mask {
            Some(m) => {
                // m: [seq, seq] → broadcast to [bsz, n_heads, seq, seq]
                scores.broadcast_add(&m.unsqueeze(0)?.unsqueeze(0)?)?
            }
            None => scores,
        };

        let attn = candle_nn::ops::softmax_last_dim(&scores)?;
        let out = attn.matmul(&v)?;

        // Merge heads: [bsz, n_heads, seq, head_dim] → [bsz, seq, n_heads * head_dim]
        let out = out
            .transpose(1, 2)?
            .reshape((bsz, seq_len, self.n_heads * self.head_dim))?;
        self.o_proj.forward(&out)
    }
}

fn repeat_kv(x: Tensor, n_rep: usize) -> Result<Tensor> {
    if n_rep == 1 {
        return Ok(x);
    }
    let (bsz, n_kv, seq, head_dim) = x.dims4()?;
    x.unsqueeze(2)?
        .expand((bsz, n_kv, n_rep, seq, head_dim))?
        .reshape((bsz, n_kv * n_rep, seq, head_dim))
}

// ---------------------------------------------------------------------------
// Decoder layer
// ---------------------------------------------------------------------------

struct DecoderLayer {
    self_attn: Attention,
    mlp: Mlp,
    input_layernorm: RmsNorm,
    post_attention_layernorm: RmsNorm,
}

impl DecoderLayer {
    fn new(cfg: &Qwen3Config, rope: Arc<RotaryEmbedding>, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            self_attn: Attention::new(cfg, rope, vb.pp("self_attn"))?,
            mlp: Mlp::new(cfg, vb.pp("mlp"))?,
            input_layernorm: rms_norm(cfg.hidden_size, cfg.rms_norm_eps, vb.pp("input_layernorm"))?,
            post_attention_layernorm: rms_norm(
                cfg.hidden_size,
                cfg.rms_norm_eps,
                vb.pp("post_attention_layernorm"),
            )?,
        })
    }

    fn forward(
        &self,
        x: &Tensor,
        offset: usize,
        cache: &mut KvCache,
        mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let h = self
            .self_attn
            .forward(&self.input_layernorm.forward(x)?, offset, cache, mask)?;
        let x = x.add(&h)?;
        let h = self
            .mlp
            .forward(&self.post_attention_layernorm.forward(&x)?)?;
        x.add(&h)
    }
}

// ---------------------------------------------------------------------------
// Full model
// ---------------------------------------------------------------------------

pub struct Qwen3Model {
    embed_tokens: Embedding,
    layers: Vec<DecoderLayer>,
    norm: RmsNorm,
    lm_head: Linear,
    dtype: DType,
}

impl Qwen3Model {
    /// Load model weights from a [`VarBuilder`] produced by
    /// [`candle_nn::VarBuilder::from_mmaped_safetensors`].
    pub fn from_vb(cfg: &Qwen3Config, vb: VarBuilder) -> Result<Self> {
        let dtype = vb.dtype();
        let rope = Arc::new(RotaryEmbedding::new(
            cfg.head_dim,
            cfg.max_position_embeddings,
            cfg.rope_theta,
            dtype,
            vb.device(),
        )?);

        let model_vb = vb.pp("model");
        let embed_tokens = embedding(cfg.vocab_size, cfg.hidden_size, model_vb.pp("embed_tokens"))?;

        let layers = (0..cfg.num_hidden_layers)
            .map(|i| DecoderLayer::new(cfg, rope.clone(), model_vb.pp(format!("layers.{i}"))))
            .collect::<Result<Vec<_>>>()?;

        let norm = rms_norm(cfg.hidden_size, cfg.rms_norm_eps, model_vb.pp("norm"))?;

        // When tie_word_embeddings=true the safetensors file has no lm_head.weight;
        // we reuse the embedding matrix (shape [vocab, hidden] = correct for Linear).
        let lm_head = if cfg.tie_word_embeddings {
            Linear::new(embed_tokens.embeddings().clone(), None)
        } else {
            linear_no_bias(cfg.hidden_size, cfg.vocab_size, vb.pp("lm_head"))?
        };

        Ok(Self {
            embed_tokens,
            layers,
            norm,
            lm_head,
            dtype,
        })
    }

    /// Forward pass. `tokens`: `[1, seq_len]`.
    ///
    /// Returns logits `[1, 1, vocab_size]` — **always just the last token position**.
    ///
    /// Only the last hidden state is passed through `norm` + `lm_head`, saving
    /// `seq_len ×` compute vs computing all positions (only the last logit is
    /// ever used for next-token prediction).
    ///
    /// Pass `need_logits = false` to skip `norm` + `lm_head` entirely (e.g. for
    /// intermediate prefill chunks whose logits will be discarded).  The return
    /// value in that case is a dummy zero scalar — callers must not use it.
    ///
    /// The causal mask accounts for any tokens already in the KV cache (`past_len`)
    /// so chunked prefill produces a correct `[seq_len, past_len + seq_len]` mask.
    pub fn forward(
        &self,
        tokens: &Tensor,
        offset: usize,
        caches: &mut [KvCache],
        need_logits: bool,
    ) -> Result<Tensor> {
        let (_bsz, seq_len) = tokens.dims2()?;

        // past_len = number of KV tokens already cached from earlier chunks/steps.
        let past_len = caches.first().map_or(0, |c| c.seq_len());
        let mask = if seq_len > 1 {
            Some(causal_mask(seq_len, past_len, self.dtype, tokens.device())?)
        } else {
            None
        };

        let mut x = self.embed_tokens.forward(tokens)?;

        for (layer, cache) in self.layers.iter().zip(caches.iter_mut()) {
            x = layer.forward(&x, offset, cache, mask.as_ref())?;
        }

        if !need_logits {
            // Caller only wants KV cache updates — skip norm + lm_head.
            return Tensor::zeros((), self.dtype, tokens.device());
        }

        // Only compute norm + lm_head for the LAST token position.
        // For seq_len = 1 (decode) this is a no-op narrow; for long prefill chunks
        // this avoids computing vocab-projection for every position, saving
        // seq_len× work on the largest matmul ([hidden] × [hidden, vocab]).
        let last = x.narrow(1, seq_len - 1, 1)?; // [bsz, 1, hidden]
        self.lm_head.forward(&self.norm.forward(&last)?)
    }

    pub fn n_layers(&self) -> usize {
        self.layers.len()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Additive causal mask for a query chunk of length `seq_len` attending to
/// `past_len + seq_len` total key/value positions.
///
/// Shape: `[seq_len, past_len + seq_len]`
///
/// Layout:
/// - Columns `0 .. past_len` → all zeros (attend freely to all cached tokens).
/// - Columns `past_len .. past_len + seq_len` → standard lower-triangular causal
///   mask (each query can see itself and earlier queries in this chunk, but not
///   later ones).
///
/// This handles both:
/// - Single-shot full-prompt prefill (`past_len = 0`, square matrix).
/// - Chunked prefill (`past_len > 0`) where earlier chunks are already in cache.
/// - Single-token decode (`seq_len = 1`, caller skips the mask entirely).
fn causal_mask(seq_len: usize, past_len: usize, dtype: DType, device: &Device) -> Result<Tensor> {
    let total = past_len + seq_len;
    let inf = f32::NEG_INFINITY;
    let mut data = vec![0f32; seq_len * total];
    for i in 0..seq_len {
        // Positions j > past_len + i are future tokens within this chunk — mask them.
        for j in (past_len + i + 1)..total {
            data[i * total + j] = inf;
        }
    }
    Tensor::from_vec(data, (seq_len, total), device)?.to_dtype(dtype)
}
