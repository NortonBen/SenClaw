//! Qwen3.5 hybrid linear+standard attention model (mlx-community OptiQ checkpoints).
//!
//! Architecture: 32 layers, 3×GatedDeltaNet + 1×SelfAttention repeating 8×.
//! Layer pattern: i%4 ∈ {0,1,2} → LinearAttention (GDN); i%4==3 → SelfAttention.
//!
//! Gated Delta Net (GDN):
//!   state S[B, H_v, V, K] updated by: S = S*g + outer((v - S@k)*beta, k); o = S@q
//!   Causal depthwise conv1d (kernel=4) pre-processes the q/k/v projection.
//!   Gated RMSNorm output: rms_norm(o) * silu(z).
//!
//! Self-attention: standard GQA (10q/4kv heads, head_dim=256),
//!   partial RoPE (rotary_factor=0.25 → 64 of 256 dims rotated).

use std::collections::HashMap;
use std::path::Path;

use mlx_rs::{
    builder::Builder,
    error::Exception,
    macros::{ModuleParameters, Quantizable},
    module::{Module, ModuleParametersExt},
    nn,
    ops::{concatenate_axis},
    ops::indexing::{IndexOp, NewAxis},
    quantization::MaybeQuantized,
    Array,
};
use serde::Deserialize;
use serde_json::Value;

use super::super::{
    cache::{KeyValueCache, KvFetchResult},
    error::Error,
    utils::{
        create_causal_mask,
        scaled_dot_product_attention,
        rope::{initialize_rope, FloatOrString, RopeVariant},
    },
};
use crate::local_model::mlx_lm::kv_layer::Qwen3LayerKv;

// ─── Config ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ModelArgs {
    pub model_type: String,
    pub hidden_size: i32,
    pub num_hidden_layers: i32,
    pub intermediate_size: i32,
    pub num_attention_heads: i32,
    pub num_key_value_heads: i32,
    pub head_dim: i32,
    pub linear_num_key_heads: i32,
    pub linear_key_head_dim: i32,
    pub linear_num_value_heads: i32,
    pub linear_value_head_dim: i32,
    pub linear_conv_kernel_dim: i32,
    pub rms_norm_eps: f32,
    pub vocab_size: i32,
    pub max_position_embeddings: i32,
    pub rope_theta: f32,
    pub partial_rotary_factor: f32,
    pub tie_word_embeddings: bool,
    pub rope_scaling: Option<HashMap<String, FloatOrString>>,
}

impl ModelArgs {
    /// Number of head dims that get rotated (always even).
    pub fn rope_dim(&self) -> i32 {
        let raw = (self.head_dim as f32 * self.partial_rotary_factor).round() as i32;
        if raw % 2 != 0 { raw - 1 } else { raw }
    }
    /// linear_num_value_heads / linear_num_key_heads
    pub fn head_ratio(&self) -> i32 {
        self.linear_num_value_heads / self.linear_num_key_heads
    }
}

// ─── Per-layer state for GDN layers ─────────────────────────────────────────

/// Persistent state for one GDN layer during decode (or cross-call during generation).
pub struct LinearAttnState {
    /// S: [1, n_v_heads, v_head_dim, k_head_dim] — the GDN memory matrix.
    pub s: Array,
    /// Conv1d rolling buffer: last (kernel_size-1) input tokens, shape [1, K-1, conv_channels].
    pub conv_buf: Array,
    /// Tokens processed so far.
    pub offset: i32,
}

impl LinearAttnState {
    pub fn new(n_v_heads: i32, v_head_dim: i32, k_head_dim: i32, conv_channels: i32, conv_kernel: i32) -> Result<Self, Exception> {
        let s = Array::zeros::<f32>(&[1, n_v_heads, v_head_dim, k_head_dim])?;
        let conv_buf = Array::zeros::<f32>(&[1, conv_kernel - 1, conv_channels])?;
        Ok(Self { s, conv_buf, offset: 0 })
    }
}

/// Per-layer cache: KV store for self-attn or GDN state for linear-attn.
pub enum Qwen3_5LayerCache {
    SelfAttn(Qwen3LayerKv),
    LinearAttn(LinearAttnState),
}

// ─── MLP (same as Qwen3) ─────────────────────────────────────────────────────

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct Mlp {
    #[quantizable] #[param] pub gate_proj: MaybeQuantized<nn::Linear>,
    #[quantizable] #[param] pub down_proj: MaybeQuantized<nn::Linear>,
    #[quantizable] #[param] pub up_proj:   MaybeQuantized<nn::Linear>,
}

impl Mlp {
    pub fn new(dim: i32, hidden_dim: i32) -> Result<Self, Exception> {
        Ok(Self {
            gate_proj: MaybeQuantized::Original(nn::LinearBuilder::new(dim, hidden_dim).bias(false).build()?),
            down_proj: MaybeQuantized::Original(nn::LinearBuilder::new(hidden_dim, dim).bias(false).build()?),
            up_proj:   MaybeQuantized::Original(nn::LinearBuilder::new(dim, hidden_dim).bias(false).build()?),
        })
    }
}

impl Module<&Array> for Mlp {
    type Output = Array;
    type Error = Exception;
    fn forward(&mut self, x: &Array) -> Result<Array, Exception> {
        let d = nn::silu(self.gate_proj.forward(x)?)?.multiply(self.up_proj.forward(x)?)?;
        self.down_proj.forward(&d)
    }
    fn training_mode(&mut self, mode: bool) {
        self.gate_proj.training_mode(mode);
        self.down_proj.training_mode(mode);
        self.up_proj.training_mode(mode);
    }
}

// ─── Self-attention (every 4th layer) ────────────────────────────────────────

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct SelfAttention {
    pub n_heads: i32,
    pub n_kv_heads: i32,
    pub head_dim: i32,
    pub rope_dim: i32,
    pub scale: f32,
    #[quantizable] #[param] pub q_proj: MaybeQuantized<nn::Linear>,
    #[quantizable] #[param] pub k_proj: MaybeQuantized<nn::Linear>,
    #[quantizable] #[param] pub v_proj: MaybeQuantized<nn::Linear>,
    #[quantizable] #[param] pub o_proj: MaybeQuantized<nn::Linear>,
    #[param] pub q_norm: nn::RmsNorm,
    #[param] pub k_norm: nn::RmsNorm,
    #[param] pub rope: RopeVariant,
}

impl SelfAttention {
    pub fn new(args: &ModelArgs) -> Result<Self, Exception> {
        let dim = args.hidden_size;
        let n_heads = args.num_attention_heads;
        let n_kv_heads = args.num_key_value_heads;
        let head_dim = args.head_dim;
        let rope_dim = args.rope_dim();
        let scale = (head_dim as f32).sqrt().recip();
        let rope = initialize_rope(rope_dim, args.rope_theta, false, &args.rope_scaling, args.max_position_embeddings)?;
        Ok(Self {
            n_heads, n_kv_heads, head_dim, rope_dim, scale,
            q_proj: MaybeQuantized::Original(nn::LinearBuilder::new(dim, n_heads * head_dim).bias(false).build()?),
            k_proj: MaybeQuantized::Original(nn::LinearBuilder::new(dim, n_kv_heads * head_dim).bias(false).build()?),
            v_proj: MaybeQuantized::Original(nn::LinearBuilder::new(dim, n_kv_heads * head_dim).bias(false).build()?),
            o_proj: MaybeQuantized::Original(nn::LinearBuilder::new(n_heads * head_dim, dim).bias(false).build()?),
            q_norm: nn::RmsNormBuilder::new(head_dim).eps(args.rms_norm_eps).build()?,
            k_norm: nn::RmsNormBuilder::new(head_dim).eps(args.rms_norm_eps).build()?,
            rope,
        })
    }

    /// Apply RoPE to the first `rope_dim` dims, pass the rest unchanged.
    fn partial_rope(&mut self, x: Array, offset: i32) -> Result<Array, Exception> {
        if self.rope_dim == self.head_dim {
            let inp = nn::RopeInputBuilder::new(&x).offset(offset).build()?;
            return self.rope.forward(inp);
        }
        let sh = x.shape().to_vec();
        let b = sh[0] as i32;
        let h = sh[1] as i32;
        let l = sh[2] as i32;
        let d = sh[3] as i32;
        let rd = self.rope_dim;
        // Flatten batch+heads to apply rope
        let flat = x.reshape(&[b * h, l, d])?;
        let rot  = flat.index((.., .., ..rd));
        let rest = flat.index((.., .., rd..));
        let rot_inp = nn::RopeInputBuilder::new(&rot).offset(offset).build()?;
        let rotated = self.rope.forward(rot_inp)?;
        let full = concatenate_axis(&[rotated, rest], -1)?;
        full.reshape(&[b, h, l, d])
    }

    #[allow(non_snake_case)]
    pub fn forward_attn(&mut self, x: &Array, mask: Option<&Array>, cache: Option<&mut Qwen3LayerKv>) -> Result<Array, Exception> {
        let sh = x.shape();
        let B = sh[0] as i32;
        let L = sh[1] as i32;
        let hd = self.head_dim;

        let queries = self.q_proj.forward(x)?;
        let keys    = self.k_proj.forward(x)?;
        let values  = self.v_proj.forward(x)?;

        let mut queries = self.q_norm.forward(
            &queries.reshape(&[B, L, self.n_heads, hd])?.transpose_axes(&[0, 2, 1, 3])?
        )?;
        let mut keys = self.k_norm.forward(
            &keys.reshape(&[B, L, self.n_kv_heads, hd])?.transpose_axes(&[0, 2, 1, 3])?
        )?;
        let mut values = values.reshape(&[B, L, self.n_kv_heads, hd])?.transpose_axes(&[0, 2, 1, 3])?;

        let offset = cache.as_ref().map(|c| c.offset()).unwrap_or(0);
        let kv_past = offset;

        queries = self.partial_rope(queries, offset)?;
        keys    = self.partial_rope(keys, offset)?;

        let fetch = if let Some(c) = cache.as_mut() {
            c.update_and_fetch(keys, values)?
        } else {
            KvFetchResult::Fp16(keys, values)
        };

        let output = match fetch {
            KvFetchResult::TurboQuant => {
                let c = cache.as_mut().ok_or_else(|| Exception::custom("TQ requires cache"))?;
                c.turboquant_attention(queries, self.scale, mask, B, L, kv_past, self.n_heads, self.n_kv_heads, hd)?
            }
            KvFetchResult::Fp16(k, v) => {
                scaled_dot_product_attention(queries, k, v, None::<&mut Qwen3LayerKv>, self.scale, mask)?
            }
            KvFetchResult::Quantized { keys: qk, values: qv } => {
                use super::super::utils::quantized_scaled_dot_product_attention;
                quantized_scaled_dot_product_attention(queries, qk, qv, self.scale, mask,
                    cache.as_ref().and_then(|c| c.group_size()).unwrap_or(64),
                    cache.as_ref().and_then(|c| c.bits()).unwrap_or(4))?
            }
        }
        .transpose_axes(&[0, 2, 1, 3])?
        .reshape(&[B, L, -1])?;

        self.o_proj.forward(&output)
    }
}

// ─── Linear attention (Gated Delta Net) ─────────────────────────────────────

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct LinearAttention {
    pub n_k_heads: i32,
    pub n_v_heads: i32,
    pub k_head_dim: i32,
    pub v_head_dim: i32,
    pub conv_channels: i32, // 2*key_dim + value_dim
    pub conv_kernel: i32,

    #[quantizable] #[param] pub in_proj_qkv: MaybeQuantized<nn::Linear>,
    #[quantizable] #[param] pub in_proj_z:   MaybeQuantized<nn::Linear>,
    #[quantizable] #[param] pub in_proj_a:   MaybeQuantized<nn::Linear>,
    #[quantizable] #[param] pub in_proj_b:   MaybeQuantized<nn::Linear>,
    #[quantizable] #[param] pub out_proj:    MaybeQuantized<nn::Linear>,
    #[param]                pub norm:        nn::RmsNorm,

    // Non-param raw tensors: loaded directly by the weight loader
    pub conv1d_weight: Array, // [conv_channels, 1, conv_kernel]
    pub a_log: Array,         // [n_v_heads]
    pub dt_bias: Array,       // [n_v_heads]
}

impl LinearAttention {
    pub fn new(args: &ModelArgs) -> Result<Self, Exception> {
        let h = args.hidden_size;
        let nk = args.linear_num_key_heads;
        let nv = args.linear_num_value_heads;
        let kd = args.linear_key_head_dim;
        let vd = args.linear_value_head_dim;
        let ck = args.linear_conv_kernel_dim;
        let key_dim = nk * kd;
        let val_dim = nv * vd;
        let conv_ch = 2 * key_dim + val_dim;

        Ok(Self {
            n_k_heads: nk, n_v_heads: nv, k_head_dim: kd, v_head_dim: vd,
            conv_channels: conv_ch, conv_kernel: ck,
            in_proj_qkv: MaybeQuantized::Original(nn::LinearBuilder::new(h, conv_ch).bias(false).build()?),
            in_proj_z:   MaybeQuantized::Original(nn::LinearBuilder::new(h, val_dim).bias(false).build()?),
            in_proj_a:   MaybeQuantized::Original(nn::LinearBuilder::new(h, nv).bias(false).build()?),
            in_proj_b:   MaybeQuantized::Original(nn::LinearBuilder::new(h, nv).bias(false).build()?),
            out_proj:    MaybeQuantized::Original(nn::LinearBuilder::new(val_dim, h).bias(false).build()?),
            norm:        nn::RmsNormBuilder::new(vd).eps(args.rms_norm_eps).build()?,
            conv1d_weight: Array::zeros::<f32>(&[conv_ch, 1, ck])?,
            a_log:         Array::zeros::<f32>(&[nv])?,
            dt_bias:       Array::zeros::<f32>(&[nv])?,
        })
    }

    /// Causal depthwise conv1d (kernel K) on x:[B,L,C].
    /// Updates `state.conv_buf` if state is provided.
    #[allow(non_snake_case)]
    fn causal_conv1d(&self, x: &Array, state: Option<&mut LinearAttnState>) -> Result<Array, Exception> {
        let sh = x.shape();
        let B  = sh[0] as i32;
        let L  = sh[1] as i32;
        let C  = sh[2] as i32;
        let K  = self.conv_kernel; // 4
        let pad = K - 1;           // 3

        // Build padded input [B, L+pad, C]
        let x_pad = match state.as_ref() {
            Some(s) if s.offset > 0 => {
                // Prepend stored buffer
                concatenate_axis(&[s.conv_buf.clone(), x.clone()], 1)?
            }
            _ => {
                // First call: pad with zeros
                let zeros = Array::zeros::<f32>(&[B, pad, C])?;
                concatenate_axis(&[zeros, x.clone()], 1)?
            }
        };

        // Weight: [C, 1, K] → [C, K] for per-channel dot products
        let w = self.conv1d_weight.reshape(&[C, K])?;

        // Output: sum of K shifted windows weighted per-channel
        let w = |k: i32| -> Result<Array, Exception> {
            // w_k: [C] → [1, 1, C]
            w.index((.., k)).reshape(&[1i32, 1i32, C])
        };
        let L_pad = x_pad.shape()[1] as i32;
        let L_out = L_pad - pad;
        let mut out = x_pad.index((.., 0..L_out, ..)).multiply(&w(0)?)?;
        for k in 1..K {
            out = out.add(&x_pad.index((.., k..L_out + k, ..)).multiply(&w(k)?)?)?;
        }

        // Update rolling buffer: store last `pad` input tokens
        if let Some(s) = state {
            let buf_start = (L - pad).max(0);
            s.conv_buf = if buf_start > 0 {
                x.index((.., buf_start.., ..))
            } else {
                // L < pad: keep tail of existing buffer + all of x
                let keep = pad - L;
                concatenate_axis(&[s.conv_buf.index((.., keep.., ..)), x.clone()], 1)?
            };
        }

        Ok(out)
    }

    /// L2-normalize along last axis.
    fn l2_norm(x: &Array) -> Result<Array, Exception> {
        let norm = x.square()?.sum(-1, true)?.sqrt()?.add(Array::from_f32(1e-6))?;
        x.divide(&norm)
    }

    /// Vectorized GDN recurrence over L positions.
    ///
    /// Returns (output [B, L, n_v, vd], final S [B, n_v, vd, kd]).
    #[allow(non_snake_case)]
    fn gdn_recurrence(
        q: &Array,     // [B, n_v, L, kd]
        k: &Array,     // [B, n_v, L, kd]
        v: &Array,     // [B, n_v, L, vd]
        beta: &Array,  // [B, L, n_v]
        g: &Array,     // [B, L, n_v]
        s_init: Array, // [B, n_v, vd, kd]
        B: i32, L: i32, n_v: i32, vd: i32, kd: i32,
    ) -> Result<(Array, Array), Exception> {
        let mut S = s_init;
        let mut o_list: Vec<Array> = Vec::with_capacity(L as usize);

        for t in 0..(L as usize) {
            let ti = t as i32;
            // Per-position slices (integer idx drops the seq dim)
            let k_t    = k.index((.., .., ti, ..));     // [B, n_v, kd]
            let v_t    = v.index((.., .., ti, ..));     // [B, n_v, vd]
            let q_t    = q.index((.., .., ti, ..));     // [B, n_v, kd]
            let beta_t = beta.index((.., ti, ..));      // [B, n_v]
            let g_t    = g.index((.., ti, ..));         // [B, n_v]

            // pred = S @ k_t  →  [B, n_v, vd]
            let k_t_exp = k_t.reshape(&[B, n_v, 1, kd])?;
            let pred = S.multiply(&k_t_exp)?.sum(-1, false)?;

            // error = (v_t - pred) * beta_t  →  [B, n_v, vd]
            let beta_t_exp = beta_t.reshape(&[B, n_v, 1])?;
            let error = v_t.subtract(&pred)?.multiply(&beta_t_exp)?;

            // S = S * g + outer(error, k_t)  →  [B, n_v, vd, kd]
            let g_t_exp  = g_t.reshape(&[B, n_v, 1, 1])?;
            let err_exp  = error.reshape(&[B, n_v, vd, 1])?;
            let k_t_exp2 = k_t.reshape(&[B, n_v, 1, kd])?;
            S = S.multiply(&g_t_exp)?.add(&err_exp.multiply(&k_t_exp2)?)?;

            // o_t = S @ q_t  →  [B, n_v, vd]
            let q_t_exp = q_t.reshape(&[B, n_v, 1, kd])?;
            let o_t = S.multiply(&q_t_exp)?.sum(-1, false)?;

            o_list.push(o_t);
        }

        // Stack outputs: Vec<[B, n_v, vd]> → [B, n_v, L, vd] → [B, L, n_v, vd]
        let o_seq = if L == 1 {
            o_list.remove(0).reshape(&[B, n_v, 1, vd])?
        } else {
            let expanded: Vec<Array> = o_list.into_iter()
                .map(|o| o.reshape(&[B, n_v, 1, vd]))
                .collect::<Result<_, _>>()?;
            concatenate_axis(&expanded, 2)?
        };
        // [B, n_v, L, vd] → [B, L, n_v, vd]
        let o = o_seq.transpose_axes(&[0, 2, 1, 3])?;
        Ok((o, S))
    }

    #[allow(non_snake_case)]
    pub fn forward_gdn(&mut self, x: &Array, state: Option<&mut LinearAttnState>) -> Result<Array, Exception> {
        let sh = x.shape();
        let B = sh[0] as i32;
        let L = sh[1] as i32;

        let nk = self.n_k_heads;
        let nv = self.n_v_heads;
        let kd = self.k_head_dim;
        let vd = self.v_head_dim;
        let head_ratio = nv / nk;
        let key_dim = nk * kd;
        let val_dim = nv * vd;

        // ── Projections ──────────────────────────────────────────────────
        let mixed    = self.in_proj_qkv.forward(x)?;  // [B, L, 2*key_dim+val_dim]
        let z        = self.in_proj_z.forward(x)?;    // [B, L, val_dim]
        let a_in     = self.in_proj_a.forward(x)?;    // [B, L, nv]
        let b_in     = self.in_proj_b.forward(x)?;    // [B, L, nv]

        // ── Causal conv1d ────────────────────────────────────────────────
        let mixed = self.causal_conv1d(&mixed, state)?;  // [B, L, conv_channels]

        // ── Split q / k / v ─────────────────────────────────────────────
        let q_flat = mixed.index((.., .., ..key_dim));             // [B, L, key_dim]
        let k_flat = mixed.index((.., .., key_dim..2 * key_dim));  // [B, L, key_dim]
        let v_flat = mixed.index((.., .., 2 * key_dim..));         // [B, L, val_dim]

        // ── Per-head layout: [B, n_heads, L, head_dim] ──────────────────
        let q = q_flat.reshape(&[B, L, nk, kd])?.transpose_axes(&[0, 2, 1, 3])?; // [B, nk, L, kd]
        let k = k_flat.reshape(&[B, L, nk, kd])?.transpose_axes(&[0, 2, 1, 3])?; // [B, nk, L, kd]
        let v = v_flat.reshape(&[B, L, nv, vd])?.transpose_axes(&[0, 2, 1, 3])?; // [B, nv, L, vd]

        // ── L2-normalise q and k ─────────────────────────────────────────
        let q = Self::l2_norm(&q)?;
        let k = Self::l2_norm(&k)?;

        // ── Expand k/q: nk heads → nv heads (repeat_interleave) ─────────
        // [B, nk, L, kd] → [B, nk, hr, L, kd] via broadcast → [B, nv, L, kd]
        let q = q.reshape(&[B, nk, 1, L, kd])?
            .add(&Array::zeros::<f32>(&[B, nk, head_ratio, L, kd])?)?
            .reshape(&[B, nv, L, kd])?;
        let k = k.reshape(&[B, nk, 1, L, kd])?
            .add(&Array::zeros::<f32>(&[B, nk, head_ratio, L, kd])?)?
            .reshape(&[B, nv, L, kd])?;

        // ── Gate g and beta ──────────────────────────────────────────────
        // g = exp(-exp(A_log) * softplus(a + dt_bias))
        // beta = sigmoid(b)
        let a_log_pos = self.a_log.exp()?;                               // [nv]
        let sp_arg    = a_in.add(&self.dt_bias)?;                        // [B, L, nv]
        let softplus  = Array::from_f32(1.0).add(&sp_arg.exp()?)?.log()?; // [B, L, nv]
        let g_raw     = a_log_pos.multiply(&softplus)?;                  // [B, L, nv]
        let g         = g_raw.negative()?.exp()?;                        // [B, L, nv] ∈ (0,1]

        let beta = {
            // sigmoid(x) = 1 / (1 + exp(-x))
            let one = Array::from_f32(1.0);
            one.divide(&one.add(&b_in.negative()?.exp()?)?)?
        };                                                               // [B, L, nv]

        // ── GDN state ────────────────────────────────────────────────────
        let s_init = match state.as_ref() {
            Some(s) if s.offset > 0 => s.s.clone(),
            _ => Array::zeros::<f32>(&[B, nv, vd, kd])?,
        };

        // ── Recurrence ───────────────────────────────────────────────────
        let (o, s_final) = Self::gdn_recurrence(&q, &k, &v, &beta, &g, s_init, B, L, nv, vd, kd)?;
        // o: [B, L, nv, vd]

        if let Some(s) = state {
            s.s = s_final;
            s.offset += L;
        }

        // ── Gated RMSNorm: rms_norm(o_per_head) * silu(z) ───────────────
        let o_reshaped = o.reshape(&[B, L, nv, vd])?;
        let o_normed   = self.norm.forward(&o_reshaped)?.reshape(&[B, L, val_dim])?;
        let z_gate     = nn::silu(z)?;
        let out        = o_normed.multiply(&z_gate)?;

        // ── Output projection ────────────────────────────────────────────
        self.out_proj.forward(&out)
    }
}

// ─── Transformer block ───────────────────────────────────────────────────────

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct TransformerBlock {
    // Exactly one of these is Some per instance
    #[quantizable] #[param] pub self_attn:    Option<SelfAttention>,
    #[quantizable] #[param] pub linear_attn:  Option<LinearAttention>,
    #[quantizable] #[param] pub mlp:          Mlp,
    #[param]                pub input_layernorm:          nn::RmsNorm,
    #[param]                pub post_attention_layernorm: nn::RmsNorm,
}

impl TransformerBlock {
    fn new_self_attn(args: &ModelArgs) -> Result<Self, Exception> {
        Ok(Self {
            self_attn:    Some(SelfAttention::new(args)?),
            linear_attn:  None,
            mlp:          Mlp::new(args.hidden_size, args.intermediate_size)?,
            input_layernorm:          nn::RmsNormBuilder::new(args.hidden_size).eps(args.rms_norm_eps).build()?,
            post_attention_layernorm: nn::RmsNormBuilder::new(args.hidden_size).eps(args.rms_norm_eps).build()?,
        })
    }

    fn new_linear_attn(args: &ModelArgs) -> Result<Self, Exception> {
        Ok(Self {
            self_attn:    None,
            linear_attn:  Some(LinearAttention::new(args)?),
            mlp:          Mlp::new(args.hidden_size, args.intermediate_size)?,
            input_layernorm:          nn::RmsNormBuilder::new(args.hidden_size).eps(args.rms_norm_eps).build()?,
            post_attention_layernorm: nn::RmsNormBuilder::new(args.hidden_size).eps(args.rms_norm_eps).build()?,
        })
    }

    pub fn forward_block(&mut self, x: &Array, mask: Option<&Array>, cache: Option<&mut Qwen3_5LayerCache>) -> Result<Array, Exception> {
        let norm_in = self.input_layernorm.forward(x)?;
        let attn_out = match (&mut self.linear_attn, &mut self.self_attn, cache) {
            (Some(la), None, Some(Qwen3_5LayerCache::LinearAttn(s))) => la.forward_gdn(&norm_in, Some(s))?,
            (Some(la), None, None)                                    => la.forward_gdn(&norm_in, None)?,
            (None, Some(sa), Some(Qwen3_5LayerCache::SelfAttn(kv)))  => sa.forward_attn(&norm_in, mask, Some(kv))?,
            (None, Some(sa), None)                                    => sa.forward_attn(&norm_in, mask, None)?,
            _ => return Err(Exception::custom("TransformerBlock: layer/cache type mismatch")),
        };
        let h = x.add(&attn_out)?;
        let r = self.mlp.forward(&self.post_attention_layernorm.forward(&h)?)?;
        h.add(&r)
    }
}

// ─── Model ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct Qwen3_5Model {
    pub vocab_size: i32,
    pub num_hidden_layers: i32,
    #[quantizable] #[param] pub embed_tokens: MaybeQuantized<nn::Embedding>,
    #[quantizable] #[param] pub layers: Vec<TransformerBlock>,
    #[param]                pub norm: nn::RmsNorm,
}

impl Qwen3_5Model {
    pub fn new(args: &ModelArgs) -> Result<Self, Exception> {
        let layers = (0..args.num_hidden_layers)
            .map(|i| {
                if i % 4 == 3 {
                    TransformerBlock::new_self_attn(args)
                } else {
                    TransformerBlock::new_linear_attn(args)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            vocab_size: args.vocab_size,
            num_hidden_layers: args.num_hidden_layers,
            embed_tokens: MaybeQuantized::Original(nn::Embedding::new(args.vocab_size, args.hidden_size)?),
            layers,
            norm: nn::RmsNormBuilder::new(args.hidden_size).eps(args.rms_norm_eps).build()?,
        })
    }

    #[allow(non_snake_case)]
    pub fn forward(&mut self, inputs: &Array, cache: &mut Vec<Option<Qwen3_5LayerCache>>) -> Result<Array, Exception> {
        let mut h = self.embed_tokens.forward(inputs)?;
        let T = h.shape()[1] as i32;

        // Build causal mask for self-attn layers (GDN layers ignore it).
        // Use the KV cache offset from the first self-attn layer to set the position offset.
        let mask: Option<Array> = if T > 1 {
            let offset = cache.iter().find_map(|c| {
                if let Some(Qwen3_5LayerCache::SelfAttn(kv)) = c {
                    Some(kv.offset())
                } else {
                    None
                }
            }).unwrap_or(0);
            Some(create_causal_mask(T, Some(offset), None, None)?)
        } else {
            None
        };

        if cache.is_empty() {
            *cache = (0..self.layers.len()).map(|_| None).collect();
        }

        for (layer, c) in self.layers.iter_mut().zip(cache.iter_mut()) {
            h = layer.forward_block(&h, mask.as_ref(), c.as_mut())?;
        }
        self.norm.forward(&h)
    }
}

#[derive(Debug, Clone, ModuleParameters, Quantizable)]
pub struct Model {
    pub args: ModelArgs,
    #[quantizable] #[param] pub model: Qwen3_5Model,
    #[quantizable] #[param] pub lm_head: Option<MaybeQuantized<nn::Linear>>,
}

impl Model {
    pub fn new(args: ModelArgs) -> Result<Self, Exception> {
        let model = Qwen3_5Model::new(&args)?;
        let lm_head = if !args.tie_word_embeddings {
            Some(MaybeQuantized::Original(
                nn::LinearBuilder::new(args.hidden_size, args.vocab_size).bias(false).build()?
            ))
        } else {
            None
        };
        Ok(Self { args, model, lm_head })
    }

    pub fn forward(&mut self, inputs: &Array, cache: &mut Vec<Option<Qwen3_5LayerCache>>) -> Result<Array, Exception> {
        let out = self.model.forward(inputs, cache)?;
        match self.lm_head.as_mut() {
            Some(h) => h.forward(&out),
            None => match &mut self.model.embed_tokens {
                MaybeQuantized::Original(e) => e.as_linear(&out),
                MaybeQuantized::Quantized(e) => e.as_linear(&out),
            },
        }
    }
}

// ─── Cache construction ──────────────────────────────────────────────────────

/// Build per-layer caches for Qwen3.5 generation.
/// Self-attn layers get Qwen3LayerKv (dense or TQ), linear-attn layers get LinearAttnState.
pub fn build_layer_caches(args: &ModelArgs, kv_bits: Option<u8>) -> Result<Vec<Option<Qwen3_5LayerCache>>, Exception> {
    let nv = args.linear_num_value_heads;
    let vd = args.linear_value_head_dim;
    let kd = args.linear_key_head_dim;
    let ck = args.linear_conv_kernel_dim;
    let conv_ch = 2 * args.linear_num_key_heads * kd + nv * vd;

    (0..args.num_hidden_layers)
        .map(|i| -> Result<Option<Qwen3_5LayerCache>, Exception> {
            if i % 4 == 3 {
                let kv = match kv_bits {
                    Some(bits) => {
                        let b = if bits == 4 { 4u8 } else { 3u8 };
                        Qwen3LayerKv::turbo(b, args.num_key_value_heads, args.head_dim)?
                    }
                    None => Qwen3LayerKv::dense(),
                };
                Ok(Some(Qwen3_5LayerCache::SelfAttn(kv)))
            } else {
                let s = LinearAttnState::new(nv, vd, kd, conv_ch, ck)?;
                Ok(Some(Qwen3_5LayerCache::LinearAttn(s)))
            }
        })
        .collect()
}

// ─── Weight loading helpers ──────────────────────────────────────────────────

pub fn get_qwen3_5_model_args(model_dir: impl AsRef<Path>) -> Result<ModelArgs, Error> {
    let path = model_dir.as_ref().join("config.json");
    let f = std::fs::File::open(path)?;
    Ok(serde_json::from_reader(f)?)
}

#[derive(Debug, Clone, Deserialize)]
pub struct WeightMap {
    pub metadata: HashMap<String, Value>,
    pub weight_map: HashMap<String, String>,
}
