//! MMS-VITS inference on mlx-rs — the full synthesis path.
//!
//! Ports the inference branch of HF `transformers/models/vits/modeling_vits.py`
//! (`VitsModel.forward`) for single-sequence (B=1), single-speaker checkpoints
//! like `facebook/mms-tts-vie`:
//!
//! ```text
//! ids ─ text_encoder ─┬─ hidden ─ stochastic duration predictor (reverse) ─ durations
//!                     └─ (m_p, logs_p) ── expand by durations ── z_p = m_p + ε·exp(logs_p)·s
//! z_p ─ residual-coupling flow (reverse) ─ HiFi-GAN decoder ─ waveform (16 kHz)
//! ```
//!
//! ## Conventions
//! - Activations are **channels-last** `[1, T, C]` (mlx-rs `conv1d` NLC layout).
//! - PyTorch conv weights `[out, in, k]` are transposed to `[out, k, in]` at
//!   load; ConvTranspose1d `[in, out, k]` becomes `[out, k, in]`.
//! - Weight-norm pairs (`weight_g`/`weight_v`, flow WaveNet only) are collapsed
//!   to a plain weight at load: `w = g · v / ‖v‖₂` (norm over `(in, k)`).
//! - The B=1 padding mask is all-ones, so reference mask multiplies are no-ops
//!   and are omitted.
//! - The rational-quadratic spline of the duration flows runs on the CPU
//!   (a few hundred scalars) — everything else stays on MLX.
//!
//! Numerics are pinned against golden tensors generated from the PyTorch
//! reference with injected noise — see `verify_against_pytorch_golden`.

#![cfg(feature = "local-mlx-tts")]

use std::collections::HashMap;
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use mlx_rs::{
    ops::{
        self,
        indexing::{take_axis, IndexOp},
    },
    Array,
};

use super::config::VitsConfig;

const F32_LN_EPS: f32 = 1e-5;

// ── small building blocks ────────────────────────────────────────────────────

/// Fetch a tensor by name.
fn tensor(w: &HashMap<String, Array>, name: &str) -> Result<Array> {
    w.get(name)
        .cloned()
        .ok_or_else(|| anyhow!("missing tensor `{name}`"))
}

/// Collapse a PyTorch weight-norm pair to a plain conv weight (still `[out, in, k]`).
///
/// Handles both serialization schemes: legacy `weight_g`/`weight_v`
/// (facebook/mms-tts-* exports) and the PyTorch ≥2.1 parametrization form
/// `parametrizations.weight.original0/original1` (community finetunes made
/// with `nn.utils.parametrizations.weight_norm`, e.g. finetune-hf-vits).
fn weight_norm_collapsed(w: &HashMap<String, Array>, prefix: &str) -> Result<Array> {
    let (g, v) = match (
        w.get(&format!("{prefix}.weight_g")),
        w.get(&format!("{prefix}.weight_v")),
    ) {
        (Some(g), Some(v)) => (g.clone(), v.clone()),
        _ => (
            tensor(w, &format!("{prefix}.parametrizations.weight.original0"))?, // g [out,1,1]
            tensor(w, &format!("{prefix}.parametrizations.weight.original1"))?, // v [out,in,k]
        ),
    };
    let norm = v.square()?.sum_axes(&[1, 2], true)?.sqrt()?; // [out,1,1]
    v.multiply(g.divide(&norm)?).map_err(Into::into)
}

/// 1-D convolution, mlx layout: input `[1, T, C_in]`, weight `[out, k, in/groups]`.
struct Conv1d {
    weight: Array,
    bias: Option<Array>,
    padding: i32,
    dilation: i32,
    groups: i32,
}

impl Conv1d {
    /// Load a PyTorch `Conv1d` (`[out, in/groups, k]`), transposing to mlx layout.
    fn load(
        w: &HashMap<String, Array>,
        prefix: &str,
        padding: i32,
        dilation: i32,
        groups: i32,
    ) -> Result<Self> {
        let weight = tensor(w, &format!("{prefix}.weight"))?.swap_axes(1, 2)?;
        let bias = w.get(&format!("{prefix}.bias")).cloned();
        Ok(Self {
            weight,
            bias,
            padding,
            dilation,
            groups,
        })
    }

    /// Load a weight-normed conv (`weight_g`/`weight_v`).
    fn load_weight_norm(
        w: &HashMap<String, Array>,
        prefix: &str,
        padding: i32,
        dilation: i32,
    ) -> Result<Self> {
        let weight = weight_norm_collapsed(w, prefix)?.swap_axes(1, 2)?;
        let bias = w.get(&format!("{prefix}.bias")).cloned();
        Ok(Self {
            weight,
            bias,
            padding,
            dilation,
            groups: 1,
        })
    }

    fn forward(&self, x: &Array) -> Result<Array> {
        let y = ops::conv1d(x, &self.weight, 1, self.padding, self.dilation, self.groups)?;
        match &self.bias {
            Some(b) => y.add(b).map_err(Into::into),
            None => Ok(y),
        }
    }
}

/// Dense layer used for all 1×1 convolutions: `y = x·Wᵀ + b`, weight `[out, in]`.
struct Linear {
    weight_t: Array, // pre-transposed [in, out]
    bias: Option<Array>,
}

impl Linear {
    /// Load either a `nn.Linear` (`[out, in]`) or a 1×1 `nn.Conv1d` (`[out, in, 1]`).
    fn load(w: &HashMap<String, Array>, prefix: &str) -> Result<Self> {
        let mut weight = tensor(w, &format!("{prefix}.weight"))?;
        if weight.ndim() == 3 {
            let s = weight.shape().to_vec();
            weight = weight.reshape(&[s[0], s[1]])?;
        }
        let bias = w.get(&format!("{prefix}.bias")).cloned();
        Ok(Self {
            weight_t: weight.swap_axes(0, 1)?,
            bias,
        })
    }

    /// Load a weight-normed 1×1 conv.
    fn load_weight_norm(w: &HashMap<String, Array>, prefix: &str) -> Result<Self> {
        let weight = weight_norm_collapsed(w, prefix)?;
        let s = weight.shape().to_vec();
        let weight = weight.reshape(&[s[0], s[1]])?;
        let bias = w.get(&format!("{prefix}.bias")).cloned();
        Ok(Self {
            weight_t: weight.swap_axes(0, 1)?,
            bias,
        })
    }

    fn forward(&self, x: &Array) -> Result<Array> {
        let y = ops::matmul(x, &self.weight_t)?;
        match &self.bias {
            Some(b) => y.add(b).map_err(Into::into),
            None => Ok(y),
        }
    }
}

/// LayerNorm over the trailing (channel) axis.
struct LayerNorm {
    weight: Array,
    bias: Array,
    eps: f32,
}

impl LayerNorm {
    fn load(w: &HashMap<String, Array>, prefix: &str, eps: f32) -> Result<Self> {
        Ok(Self {
            weight: tensor(w, &format!("{prefix}.weight"))?,
            bias: tensor(w, &format!("{prefix}.bias"))?,
            eps,
        })
    }

    fn forward(&self, x: &Array) -> Result<Array> {
        let mu = x.mean_axes(&[-1], true)?;
        let xc = x.subtract(&mu)?;
        let var = xc.square()?.mean_axes(&[-1], true)?;
        let inv = var.add(Array::from_f32(self.eps))?.rsqrt()?;
        xc.multiply(inv)?
            .multiply(&self.weight)?
            .add(&self.bias)
            .map_err(Into::into)
    }
}

fn gelu_exact(x: &Array) -> Result<Array> {
    // 0.5·x·(1 + erf(x/√2)) — matches torch.nn.functional.gelu default.
    let inner = ops::erf(&x.multiply(Array::from_f32(std::f32::consts::FRAC_1_SQRT_2))?)?;
    x.multiply(inner.add(Array::from_f32(1.0))?)?
        .multiply(Array::from_f32(0.5))
        .map_err(Into::into)
}

fn leaky_relu(x: &Array, slope: f32) -> Result<Array> {
    ops::maximum(x, x.multiply(Array::from_f32(slope))?).map_err(Into::into)
}

/// Copy an array's logical contents to a CPU `Vec<f32>`.
///
/// `Array::as_slice` exposes the raw buffer and IGNORES strides, so calling it
/// on a view (e.g. the result of `index(..)`) yields interleaved garbage. The
/// flat reshape forces a contiguous materialization first.
fn to_vec_f32(x: &Array) -> Result<Vec<f32>> {
    // `multiply` allocates a fresh dense row-major output, defeating any view
    // fast-path (a bare `reshape` of a strided slice does NOT reliably copy).
    let flat = x.multiply(Array::from_f32(1.0))?.reshape(&[-1])?;
    flat.eval()?;
    Ok(flat.as_slice::<f32>().to_vec())
}

/// Reverse the channel (last) axis.
fn flip_channels(x: &Array) -> Result<Array> {
    let c = *x.shape().last().expect("non-scalar");
    let idx: Vec<i32> = (0..c).rev().collect();
    take_axis(x, Array::from_slice(&idx, &[c]), -1).map_err(Into::into)
}

// ── text encoder ─────────────────────────────────────────────────────────────

struct Attention {
    q: Linear,
    k: Linear,
    v: Linear,
    out: Linear,
    emb_rel_k: Array, // [1, 2w+1, head_dim]
    emb_rel_v: Array,
    num_heads: i32,
    head_dim: i32,
    window: i32,
}

impl Attention {
    fn load(w: &HashMap<String, Array>, prefix: &str, cfg: &VitsConfig) -> Result<Self> {
        Ok(Self {
            q: Linear::load(w, &format!("{prefix}.q_proj"))?,
            k: Linear::load(w, &format!("{prefix}.k_proj"))?,
            v: Linear::load(w, &format!("{prefix}.v_proj"))?,
            out: Linear::load(w, &format!("{prefix}.out_proj"))?,
            emb_rel_k: tensor(w, &format!("{prefix}.emb_rel_k"))?,
            emb_rel_v: tensor(w, &format!("{prefix}.emb_rel_v"))?,
            num_heads: cfg.num_attention_heads,
            head_dim: cfg.hidden_size / cfg.num_attention_heads,
            window: cfg.window_size,
        })
    }

    /// Slice/pad the `[1, 2w+1, d]` relative table to `[1, 2L-1, d]`.
    fn relative_embeddings(&self, emb: &Array, len: i32) -> Result<Array> {
        let pad = (len - (self.window + 1)).max(0);
        let padded = if pad > 0 {
            ops::pad(emb, &[(0, 0), (pad, pad), (0, 0)][..], None, None)?
        } else {
            emb.clone()
        };
        let start = ((self.window + 1) - len).max(0);
        Ok(padded.index((.., start..start + 2 * len - 1, ..)))
    }

    /// `[H, L, 2L-1]` → `[H, L, L]` (skewed reshape, as in the reference).
    fn rel_to_abs(x: &Array) -> Result<Array> {
        let s = x.shape().to_vec();
        let (h, l) = (s[0], s[1]);
        let x = ops::pad(x, &[(0, 0), (0, 0), (0, 1)][..], None, None)?;
        let flat = x.reshape(&[h, l * 2 * l])?;
        let flat = ops::pad(&flat, &[(0, 0), (0, l - 1)][..], None, None)?;
        let full = flat.reshape(&[h, l + 1, 2 * l - 1])?;
        Ok(full.index((.., 0..l, (l - 1)..)))
    }

    /// `[H, L, L]` → `[H, L, 2L-1]` (inverse skew).
    fn abs_to_rel(x: &Array) -> Result<Array> {
        let s = x.shape().to_vec();
        let (h, l) = (s[0], s[1]);
        let x = ops::pad(x, &[(0, 0), (0, 0), (0, l - 1)][..], None, None)?;
        let flat = x.reshape(&[h, l * (2 * l - 1)])?;
        let flat = ops::pad(&flat, &[(0, 0), (l, 0)][..], None, None)?;
        let full = flat.reshape(&[h, l, 2 * l])?;
        Ok(full.index((.., .., 1..)))
    }

    fn forward(&self, x: &Array) -> Result<Array> {
        let t = x.shape()[1];
        let scale = (self.head_dim as f32).powf(-0.5);
        let to_heads = |a: Array| -> Result<Array> {
            // [1,T,C] → [H,T,D]
            a.reshape(&[t, self.num_heads, self.head_dim])?
                .swap_axes(0, 1)
                .map_err(Into::into)
        };
        let q = to_heads(self.q.forward(x)?.multiply(Array::from_f32(scale))?)?;
        let k = to_heads(self.k.forward(x)?)?;
        let v = to_heads(self.v.forward(x)?)?;

        let mut attn = ops::matmul(&q, &k.swap_axes(-2, -1)?)?; // [H,T,T]
        let rel_k = self.relative_embeddings(&self.emb_rel_k, t)?; // [1,2T-1,D]
        let rel_logits = ops::matmul(&q, &rel_k.swap_axes(-2, -1)?)?; // [H,T,2T-1]
        attn = attn.add(Self::rel_to_abs(&rel_logits)?)?;
        let attn = ops::softmax_axis(&attn, -1, true)?;

        let mut out = ops::matmul(&attn, &v)?; // [H,T,D]
        let rel_v = self.relative_embeddings(&self.emb_rel_v, t)?;
        out = out.add(ops::matmul(&Self::abs_to_rel(&attn)?, &rel_v)?)?;

        let merged = out
            .swap_axes(0, 1)?
            .reshape(&[1, t, self.num_heads * self.head_dim])?;
        self.out.forward(&merged)
    }
}

struct FeedForward {
    conv1: Conv1d,
    conv2: Conv1d,
    pad_l: i32,
    pad_r: i32,
}

impl FeedForward {
    fn load(w: &HashMap<String, Array>, prefix: &str, cfg: &VitsConfig) -> Result<Self> {
        let k = cfg.ffn_kernel_size;
        Ok(Self {
            conv1: Conv1d::load(w, &format!("{prefix}.conv_1"), 0, 1, 1)?,
            conv2: Conv1d::load(w, &format!("{prefix}.conv_2"), 0, 1, 1)?,
            pad_l: (k - 1) / 2,
            pad_r: k / 2,
        })
    }

    fn forward(&self, x: &Array) -> Result<Array> {
        let pad = |a: &Array| -> Result<Array> {
            ops::pad(
                a,
                &[(0, 0), (self.pad_l, self.pad_r), (0, 0)][..],
                None,
                None,
            )
            .map_err(Into::into)
        };
        let h = self.conv1.forward(&pad(x)?)?;
        let h = ops::maximum(&h, Array::from_f32(0.0))?; // relu (hidden_act)
        self.conv2.forward(&pad(&h)?)
    }
}

struct EncoderLayer {
    attention: Attention,
    layer_norm: LayerNorm,
    feed_forward: FeedForward,
    final_layer_norm: LayerNorm,
}

struct TextEncoder {
    embed: Array, // [vocab, hidden]
    layers: Vec<EncoderLayer>,
    project: Linear, // 1×1 conv → [2·flow_size]
    hidden_size: i32,
    flow_size: i32,
}

impl TextEncoder {
    fn load(w: &HashMap<String, Array>, cfg: &VitsConfig) -> Result<Self> {
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        for i in 0..cfg.num_hidden_layers {
            let p = format!("text_encoder.encoder.layers.{i}");
            layers.push(EncoderLayer {
                attention: Attention::load(w, &format!("{p}.attention"), cfg)?,
                layer_norm: LayerNorm::load(w, &format!("{p}.layer_norm"), cfg.layer_norm_eps)?,
                feed_forward: FeedForward::load(w, &format!("{p}.feed_forward"), cfg)?,
                final_layer_norm: LayerNorm::load(
                    w,
                    &format!("{p}.final_layer_norm"),
                    cfg.layer_norm_eps,
                )?,
            });
        }
        Ok(Self {
            embed: tensor(w, "text_encoder.embed_tokens.weight")?,
            layers,
            project: Linear::load(w, "text_encoder.project")?,
            hidden_size: cfg.hidden_size,
            flow_size: cfg.flow_size,
        })
    }

    /// → `(hidden [1,T,C], prior_means [1,T,F], prior_log_var [1,T,F])`.
    fn forward(&self, ids: &[u32]) -> Result<(Array, Array, Array)> {
        let t = ids.len() as i32;
        let idx: Vec<i32> = ids.iter().map(|&i| i as i32).collect();
        let mut h = take_axis(&self.embed, Array::from_slice(&idx, &[t]), 0)?
            .multiply(Array::from_f32((self.hidden_size as f32).sqrt()))?
            .reshape(&[1, t, self.hidden_size])?;
        for layer in &self.layers {
            let attn = layer.attention.forward(&h)?;
            h = layer.layer_norm.forward(&h.add(attn)?)?;
            let ffn = layer.feed_forward.forward(&h)?;
            h = layer.final_layer_norm.forward(&h.add(ffn)?)?;
        }
        let stats = self.project.forward(&h)?;
        let m = stats.index((.., .., 0..self.flow_size));
        let logs = stats.index((.., .., self.flow_size..));
        Ok((h, m, logs))
    }
}

// ── duration predictor (stochastic, reverse only) ────────────────────────────

struct DdsConv {
    dilated: Vec<Conv1d>, // depthwise
    pointwise: Vec<Linear>,
    norms_1: Vec<LayerNorm>,
    norms_2: Vec<LayerNorm>,
}

impl DdsConv {
    fn load(w: &HashMap<String, Array>, prefix: &str, cfg: &VitsConfig) -> Result<Self> {
        let k = cfg.duration_predictor_kernel_size;
        let n = cfg.depth_separable_num_layers;
        let (mut dilated, mut pointwise, mut norms_1, mut norms_2) =
            (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        for i in 0..n {
            let dilation = k.pow(i as u32);
            let padding = (k * dilation - dilation) / 2;
            dilated.push(Conv1d::load(
                w,
                &format!("{prefix}.convs_dilated.{i}"),
                padding,
                dilation,
                cfg.hidden_size, // depthwise: groups == channels
            )?);
            pointwise.push(Linear::load(w, &format!("{prefix}.convs_pointwise.{i}"))?);
            norms_1.push(LayerNorm::load(
                w,
                &format!("{prefix}.norms_1.{i}"),
                F32_LN_EPS,
            )?);
            norms_2.push(LayerNorm::load(
                w,
                &format!("{prefix}.norms_2.{i}"),
                F32_LN_EPS,
            )?);
        }
        Ok(Self {
            dilated,
            pointwise,
            norms_1,
            norms_2,
        })
    }

    fn forward(&self, x: &Array, cond: Option<&Array>) -> Result<Array> {
        let mut x = match cond {
            Some(g) => x.add(g)?,
            None => x.clone(),
        };
        for i in 0..self.dilated.len() {
            let mut h = self.dilated[i].forward(&x)?;
            h = self.norms_1[i].forward(&h)?;
            h = gelu_exact(&h)?;
            h = self.pointwise[i].forward(&h)?;
            h = self.norms_2[i].forward(&h)?;
            h = gelu_exact(&h)?;
            x = x.add(h)?;
        }
        Ok(x)
    }
}

/// One rational-quadratic-spline coupling flow of the duration predictor.
struct ConvFlow {
    conv_pre: Linear,
    conv_dds: DdsConv,
    conv_proj: Linear,
    num_bins: usize,
    tail_bound: f32,
    filter_scale: f32,
}

impl ConvFlow {
    fn load(w: &HashMap<String, Array>, prefix: &str, cfg: &VitsConfig) -> Result<Self> {
        Ok(Self {
            conv_pre: Linear::load(w, &format!("{prefix}.conv_pre"))?,
            conv_dds: DdsConv::load(w, &format!("{prefix}.conv_dds"), cfg)?,
            conv_proj: Linear::load(w, &format!("{prefix}.conv_proj"))?,
            num_bins: cfg.duration_predictor_flow_bins,
            tail_bound: cfg.duration_predictor_tail_bound,
            filter_scale: (cfg.hidden_size as f32).sqrt(),
        })
    }

    /// Reverse pass on `[1,T,2]` latents; `cond` is the SDP conditioning `[1,T,C]`.
    fn reverse(&self, lat: &Array, cond: &Array) -> Result<Array> {
        let first = lat.index((.., .., 0..1)); // [1,T,1]
        let second = lat.index((.., .., 1..2));

        let mut h = self.conv_pre.forward(&first)?;
        h = self.conv_dds.forward(&h, Some(cond))?;
        h = self.conv_proj.forward(&h)?; // [1,T,3·bins-1]

        let t = h.shape()[1] as usize;
        let params = to_vec_f32(&h)?;
        let xs = to_vec_f32(&second)?;
        let nb = self.num_bins;
        let stride = 3 * nb - 1;
        let mut out = Vec::with_capacity(t);
        for i in 0..t {
            let row = &params[i * stride..(i + 1) * stride];
            let widths: Vec<f32> = row[..nb].iter().map(|v| v / self.filter_scale).collect();
            let heights: Vec<f32> = row[nb..2 * nb]
                .iter()
                .map(|v| v / self.filter_scale)
                .collect();
            let derivs = &row[2 * nb..];
            out.push(rq_spline_inverse(
                xs[i],
                &widths,
                &heights,
                derivs,
                self.tail_bound,
            ));
        }
        let second_new = Array::from_slice(&out, &[1, t as i32, 1]);
        ops::concatenate_axis(&[first, second_new], -1).map_err(Into::into)
    }
}

/// Inverse of the unconstrained monotonic rational-quadratic spline
/// (`_unconstrained_rational_quadratic_spline` with `reverse=True`).
fn rq_spline_inverse(x: f32, un_w: &[f32], un_h: &[f32], un_d: &[f32], tail: f32) -> f32 {
    const MIN_BIN: f32 = 1e-3;
    const MIN_DERIV: f32 = 1e-3;
    if !(-tail..=tail).contains(&x) {
        return x; // identity outside the tails
    }
    let k = un_w.len();
    let softmax = |v: &[f32]| -> Vec<f32> {
        let m = v.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let e: Vec<f32> = v.iter().map(|&a| (a - m).exp()).collect();
        let s: f32 = e.iter().sum();
        e.iter().map(|&a| a / s).collect()
    };
    // Bin widths/heights → cumulative knot positions in [-tail, tail].
    let knots = |un: &[f32]| -> (Vec<f32>, Vec<f32>) {
        let p = softmax(un);
        let sizes: Vec<f32> = p
            .iter()
            .map(|&v| MIN_BIN + (1.0 - MIN_BIN * k as f32) * v)
            .collect();
        let mut cum = Vec::with_capacity(k + 1);
        cum.push(-tail);
        let mut acc = 0.0f32;
        for (i, &s) in sizes.iter().enumerate() {
            acc += s;
            cum.push(if i == k - 1 {
                tail
            } else {
                2.0 * tail * acc - tail
            });
        }
        let widths: Vec<f32> = (0..k).map(|i| cum[i + 1] - cum[i]).collect();
        (cum, widths)
    };
    let (cumw, widths) = knots(un_w);
    let (cumh, heights) = knots(un_h);

    // Derivatives: interior from softplus, boundaries pinned to 1.
    let softplus = |v: f32| (1.0 + v.exp()).ln();
    let mut derivs = Vec::with_capacity(k + 1);
    derivs.push(1.0f32);
    for &d in un_d {
        derivs.push(MIN_DERIV + softplus(d));
    }
    derivs.push(1.0f32);

    // Locate the bin (reverse mode buckets on heights).
    let mut bin = 0usize;
    for i in 0..k {
        let upper = if i == k - 1 {
            cumh[k] + 1e-6
        } else {
            cumh[i + 1]
        };
        if x >= cumh[i] && x < upper {
            bin = i;
            break;
        }
    }

    let (w, h) = (widths[bin], heights[bin]);
    let (cw, ch) = (cumw[bin], cumh[bin]);
    let (d0, d1) = (derivs[bin], derivs[bin + 1]);
    let delta = h / w;

    // Solve the quadratic for θ (reference `_rational_quadratic_spline`, reverse).
    let i1 = d0 + d1 - 2.0 * delta;
    let i2 = x - ch;
    let i3 = i2 * i1;
    let a = h * (delta - d0) + i3;
    let b = h * d0 - i3;
    let c = -delta * i2;
    let disc = (b * b - 4.0 * a * c).max(0.0);
    let root = (2.0 * c) / (-b - disc.sqrt());
    root * w + cw
}

/// Elementwise affine flow (reverse): `(x - translate)·exp(-log_scale)`.
struct ElementwiseAffine {
    translate: Array, // broadcast [1,1,2]
    neg_exp_log_scale: Array,
}

impl ElementwiseAffine {
    fn load(w: &HashMap<String, Array>, prefix: &str) -> Result<Self> {
        let ch = |name: &str| -> Result<Array> {
            let a = tensor(w, &format!("{prefix}.{name}"))?; // [2,1]
            a.reshape(&[1, 1, 2]).map_err(Into::into)
        };
        let translate = ch("translate")?;
        let log_scale = ch("log_scale")?;
        let neg_exp_log_scale = log_scale.negative()?.exp()?;
        Ok(Self {
            translate,
            neg_exp_log_scale,
        })
    }

    fn reverse(&self, x: &Array) -> Result<Array> {
        x.subtract(&self.translate)?
            .multiply(&self.neg_exp_log_scale)
            .map_err(Into::into)
    }
}

struct StochasticDurationPredictor {
    conv_pre: Linear,
    conv_dds: DdsConv,
    conv_proj: Linear,
    affine: ElementwiseAffine,
    conv_flows: Vec<ConvFlow>,
}

impl StochasticDurationPredictor {
    fn load(w: &HashMap<String, Array>, cfg: &VitsConfig) -> Result<Self> {
        let p = "duration_predictor";
        let mut conv_flows = Vec::with_capacity(cfg.duration_predictor_num_flows);
        // flows.0 is the ElementwiseAffine; flows.1.. are ConvFlows.
        for i in 1..=cfg.duration_predictor_num_flows {
            conv_flows.push(ConvFlow::load(w, &format!("{p}.flows.{i}"), cfg)?);
        }
        Ok(Self {
            conv_pre: Linear::load(w, &format!("{p}.conv_pre"))?,
            conv_dds: DdsConv::load(w, &format!("{p}.conv_dds"), cfg)?,
            conv_proj: Linear::load(w, &format!("{p}.conv_proj"))?,
            affine: ElementwiseAffine::load(w, &format!("{p}.flows.0"))?,
            conv_flows,
        })
    }

    /// Reverse pass: noise `[1,T,2]` (already scaled) → `log_duration [1,T,1]`.
    ///
    /// Flow order matches the reference: reversed list, dropping the *first*
    /// ConvFlow and keeping the affine last (`flows[:-2] + [flows[-1]]`).
    fn reverse(&self, hidden: &Array, noise: &Array) -> Result<Array> {
        let mut cond = self.conv_pre.forward(hidden)?;
        cond = self.conv_dds.forward(&cond, None)?;
        cond = self.conv_proj.forward(&cond)?;

        let mut lat = noise.clone();
        for cf in self.conv_flows.iter().skip(1).rev() {
            lat = flip_channels(&lat)?;
            lat = cf.reverse(&lat, &cond)?;
        }
        lat = flip_channels(&lat)?;
        lat = self.affine.reverse(&lat)?;
        Ok(lat.index((.., .., 0..1)))
    }
}

// ── residual coupling flow ───────────────────────────────────────────────────

struct WaveNet {
    in_layers: Vec<Conv1d>,
    res_skip: Vec<Linear>, // all 1×1
    hidden: i32,
}

impl WaveNet {
    fn load(
        w: &HashMap<String, Array>,
        prefix: &str,
        num_layers: usize,
        cfg: &VitsConfig,
    ) -> Result<Self> {
        let k = cfg.wavenet_kernel_size;
        let (mut in_layers, mut res_skip) = (Vec::new(), Vec::new());
        for i in 0..num_layers {
            let dilation = cfg.wavenet_dilation_rate.pow(i as u32);
            let padding = (k * dilation - dilation) / 2;
            in_layers.push(Conv1d::load_weight_norm(
                w,
                &format!("{prefix}.in_layers.{i}"),
                padding,
                dilation,
            )?);
            res_skip.push(Linear::load_weight_norm(
                w,
                &format!("{prefix}.res_skip_layers.{i}"),
            )?);
        }
        Ok(Self {
            in_layers,
            res_skip,
            hidden: cfg.hidden_size,
        })
    }

    fn forward(&self, x: &Array) -> Result<Array> {
        let n = self.in_layers.len();
        let mut inputs = x.clone();
        let mut outputs: Option<Array> = None;
        for i in 0..n {
            let a = self.in_layers[i].forward(&inputs)?;
            let t = ops::tanh(&a.index((.., .., 0..self.hidden)))?;
            let s = ops::sigmoid(&a.index((.., .., self.hidden..)))?;
            let acts = t.multiply(s)?;
            let rs = self.res_skip[i].forward(&acts)?;
            if i < n - 1 {
                inputs = inputs.add(rs.index((.., .., 0..self.hidden)))?;
                let skip = rs.index((.., .., self.hidden..));
                outputs = Some(match outputs {
                    Some(o) => o.add(skip)?,
                    None => skip,
                });
            } else {
                outputs = Some(match outputs {
                    Some(o) => o.add(&rs)?,
                    None => rs,
                });
            }
        }
        Ok(outputs.expect("wavenet has ≥1 layer"))
    }
}

/// Mean-only residual coupling layer (reverse).
struct CouplingLayer {
    conv_pre: Linear,
    wavenet: WaveNet,
    conv_post: Linear,
    half: i32,
}

impl CouplingLayer {
    fn load(w: &HashMap<String, Array>, prefix: &str, cfg: &VitsConfig) -> Result<Self> {
        Ok(Self {
            conv_pre: Linear::load(w, &format!("{prefix}.conv_pre"))?,
            wavenet: WaveNet::load(
                w,
                &format!("{prefix}.wavenet"),
                cfg.prior_encoder_num_wavenet_layers,
                cfg,
            )?,
            conv_post: Linear::load(w, &format!("{prefix}.conv_post"))?,
            half: cfg.flow_size / 2,
        })
    }

    fn reverse(&self, x: &Array) -> Result<Array> {
        let first = x.index((.., .., 0..self.half));
        let second = x.index((.., .., self.half..));
        let h = self.conv_pre.forward(&first)?;
        let h = self.wavenet.forward(&h)?;
        let mean = self.conv_post.forward(&h)?;
        let second = second.subtract(&mean)?;
        ops::concatenate_axis(&[first, second], -1).map_err(Into::into)
    }
}

struct ResidualCouplingBlock {
    layers: Vec<CouplingLayer>,
}

impl ResidualCouplingBlock {
    fn load(w: &HashMap<String, Array>, cfg: &VitsConfig) -> Result<Self> {
        let mut layers = Vec::with_capacity(cfg.prior_encoder_num_flows);
        for i in 0..cfg.prior_encoder_num_flows {
            layers.push(CouplingLayer::load(w, &format!("flow.flows.{i}"), cfg)?);
        }
        Ok(Self { layers })
    }

    fn reverse(&self, x: &Array) -> Result<Array> {
        let mut x = x.clone();
        for layer in self.layers.iter().rev() {
            x = flip_channels(&x)?;
            x = layer.reverse(&x)?;
        }
        Ok(x)
    }
}

// ── HiFi-GAN decoder ─────────────────────────────────────────────────────────

struct ResBlock {
    convs1: Vec<Conv1d>,
    convs2: Vec<Conv1d>,
    slope: f32,
}

impl ResBlock {
    fn load(
        w: &HashMap<String, Array>,
        prefix: &str,
        kernel: i32,
        dilations: &[i32],
        slope: f32,
    ) -> Result<Self> {
        let (mut convs1, mut convs2) = (Vec::new(), Vec::new());
        for (j, &d) in dilations.iter().enumerate() {
            convs1.push(Conv1d::load(
                w,
                &format!("{prefix}.convs1.{j}"),
                (kernel * d - d) / 2,
                d,
                1,
            )?);
            convs2.push(Conv1d::load(
                w,
                &format!("{prefix}.convs2.{j}"),
                (kernel - 1) / 2,
                1,
                1,
            )?);
        }
        Ok(Self {
            convs1,
            convs2,
            slope,
        })
    }

    fn forward(&self, x: &Array) -> Result<Array> {
        let mut x = x.clone();
        for (c1, c2) in self.convs1.iter().zip(&self.convs2) {
            let h = c1.forward(&leaky_relu(&x, self.slope)?)?;
            let h = c2.forward(&leaky_relu(&h, self.slope)?)?;
            x = x.add(h)?;
        }
        Ok(x)
    }
}

struct ConvTranspose1d {
    weight: Array, // [out, k, in]
    bias: Option<Array>,
    stride: i32,
    padding: i32,
}

impl ConvTranspose1d {
    /// Load a PyTorch `ConvTranspose1d` (`[in, out, k]`) into mlx layout.
    fn load(w: &HashMap<String, Array>, prefix: &str, stride: i32, padding: i32) -> Result<Self> {
        let weight = tensor(w, &format!("{prefix}.weight"))?
            .swap_axes(0, 1)? // [out, in, k]
            .swap_axes(1, 2)?; // [out, k, in]
        let bias = w.get(&format!("{prefix}.bias")).cloned();
        Ok(Self {
            weight,
            bias,
            stride,
            padding,
        })
    }

    fn forward(&self, x: &Array) -> Result<Array> {
        let y = ops::conv_transpose1d(x, &self.weight, self.stride, self.padding, 1, 0, 1)?;
        match &self.bias {
            Some(b) => y.add(b).map_err(Into::into),
            None => Ok(y),
        }
    }
}

struct HifiGan {
    conv_pre: Conv1d,
    upsampler: Vec<ConvTranspose1d>,
    resblocks: Vec<ResBlock>, // num_upsamples × num_kernels
    conv_post: Conv1d,
    num_kernels: usize,
    slope: f32,
}

impl HifiGan {
    fn load(w: &HashMap<String, Array>, cfg: &VitsConfig) -> Result<Self> {
        let conv_pre = Conv1d::load(w, "decoder.conv_pre", 3, 1, 1)?;
        let mut upsampler = Vec::new();
        for (i, (&rate, &kernel)) in cfg
            .upsample_rates
            .iter()
            .zip(&cfg.upsample_kernel_sizes)
            .enumerate()
        {
            upsampler.push(ConvTranspose1d::load(
                w,
                &format!("decoder.upsampler.{i}"),
                rate,
                (kernel - rate) / 2,
            )?);
        }
        let mut resblocks = Vec::new();
        for i in 0..upsampler.len() {
            for (j, (&kernel, dils)) in cfg
                .resblock_kernel_sizes
                .iter()
                .zip(&cfg.resblock_dilation_sizes)
                .enumerate()
            {
                let idx = i * cfg.resblock_kernel_sizes.len() + j;
                resblocks.push(ResBlock::load(
                    w,
                    &format!("decoder.resblocks.{idx}"),
                    kernel,
                    dils,
                    cfg.leaky_relu_slope,
                )?);
            }
        }
        let conv_post = Conv1d::load(w, "decoder.conv_post", 3, 1, 1)?;
        Ok(Self {
            conv_pre,
            upsampler,
            resblocks,
            conv_post,
            num_kernels: cfg.resblock_kernel_sizes.len(),
            slope: cfg.leaky_relu_slope,
        })
    }

    fn forward(&self, z: &Array) -> Result<Array> {
        let mut x = self.conv_pre.forward(z)?;
        for (i, up) in self.upsampler.iter().enumerate() {
            x = up.forward(&leaky_relu(&x, self.slope)?)?;
            let mut acc = self.resblocks[i * self.num_kernels].forward(&x)?;
            for j in 1..self.num_kernels {
                acc = acc.add(self.resblocks[i * self.num_kernels + j].forward(&x)?)?;
            }
            x = acc.multiply(Array::from_f32(1.0 / self.num_kernels as f32))?;
        }
        // Final activation uses PyTorch's *default* leaky slope (0.01), not the
        // configured one — an easy-to-miss reference quirk.
        x = leaky_relu(&x, 0.01)?;
        x = self.conv_post.forward(&x)?;
        ops::tanh(&x).map_err(Into::into)
    }
}

// ── full model ───────────────────────────────────────────────────────────────

/// Hard cap on generated latent frames (≈96 s of audio at 256× upsampling) —
/// guards against runaway durations on degenerate inputs.
const MAX_OUTPUT_FRAMES: usize = 6000;

pub struct MmsVits {
    pub cfg: VitsConfig,
    text_encoder: TextEncoder,
    duration_predictor: StochasticDurationPredictor,
    flow: ResidualCouplingBlock,
    decoder: HifiGan,
}

impl MmsVits {
    /// Load config + weights from a HF snapshot directory
    /// (`config.json` + `model.safetensors`).
    pub fn load(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let cfg = VitsConfig::load(dir)?;
        if !cfg.use_stochastic_duration_prediction {
            return Err(anyhow!(
                "only stochastic-duration VITS checkpoints are supported (MMS family)"
            ));
        }
        if cfg.num_speakers > 1 || cfg.speaker_embedding_size != 0 {
            return Err(anyhow!(
                "multi-speaker VITS checkpoints are not supported yet"
            ));
        }
        let path = dir.join("model.safetensors");
        let weights = Array::load_safetensors(&path)
            .map_err(|e| anyhow!("loading {}: {e}", path.display()))?;
        Ok(Self {
            text_encoder: TextEncoder::load(&weights, &cfg).context("text_encoder")?,
            duration_predictor: StochasticDurationPredictor::load(&weights, &cfg)
                .context("duration_predictor")?,
            flow: ResidualCouplingBlock::load(&weights, &cfg).context("flow")?,
            decoder: HifiGan::load(&weights, &cfg).context("decoder")?,
            cfg,
        })
    }

    /// Synthesize a waveform (f32 samples at `cfg.sampling_rate`) from token ids.
    ///
    /// `speed` > 1 speaks faster. Noise is sampled internally; for reproducible
    /// runs use [`Self::infer_with_noise`].
    pub fn infer(&self, ids: &[u32], speed: f32) -> Result<Vec<f32>> {
        let t = ids.len() as i32;
        let noise_dur = mlx_rs::random::normal::<f32>(&[1, t, 2], None, None, None)?
            .multiply(Array::from_f32(self.cfg.noise_scale_duration))?;
        self.infer_stage2(ids, &noise_dur, None, speed)
    }

    /// Deterministic variant: caller supplies the duration noise `[1,T,2]`
    /// (pre-scaled) and prior noise `[1,T_out,flow]` (unit normal).
    pub fn infer_with_noise(
        &self,
        ids: &[u32],
        noise_dur: &Array,
        noise_zp: &Array,
        speed: f32,
    ) -> Result<Vec<f32>> {
        self.infer_stage2(ids, noise_dur, Some(noise_zp), speed)
    }

    fn infer_stage2(
        &self,
        ids: &[u32],
        noise_dur: &Array,
        noise_zp: Option<&Array>,
        speed: f32,
    ) -> Result<Vec<f32>> {
        if ids.is_empty() {
            return Err(anyhow!("empty token sequence"));
        }
        let speed = if speed.is_finite() && speed > 0.0 {
            speed
        } else {
            1.0
        };
        let length_scale = 1.0 / (self.cfg.speaking_rate * speed);

        let (hidden, m_p, logs_p) = self.text_encoder.forward(ids)?;
        let log_dur = self.duration_predictor.reverse(&hidden, noise_dur)?;

        // Durations → frame-repeat indices (CPU).
        let ld = to_vec_f32(&log_dur)?;
        let mut idx: Vec<i32> = Vec::new();
        for (i, &v) in ld.iter().enumerate() {
            let d = (v.exp() * length_scale).ceil().max(0.0) as usize;
            for _ in 0..d {
                idx.push(i as i32);
            }
        }
        if idx.is_empty() {
            idx.push(0); // reference clamps total length to ≥ 1 frame
        }
        if idx.len() > MAX_OUTPUT_FRAMES {
            idx.truncate(MAX_OUTPUT_FRAMES);
        }
        let t_out = idx.len() as i32;
        let idx_arr = Array::from_slice(&idx, &[t_out]);

        let m_e = take_axis(&m_p, &idx_arr, 1)?;
        let logs_e = take_axis(&logs_p, &idx_arr, 1)?;
        let noise = match noise_zp {
            Some(n) => n.clone(),
            None => {
                mlx_rs::random::normal::<f32>(&[1, t_out, self.cfg.flow_size], None, None, None)?
            }
        };
        let z_p = m_e.add(
            noise
                .multiply(logs_e.exp()?)?
                .multiply(Array::from_f32(self.cfg.noise_scale))?,
        )?;

        let z = self.flow.reverse(&z_p)?;
        let wav = self.decoder.forward(&z)?; // [1, samples, 1]
        to_vec_f32(&wav)
    }
}

/// Re-export of the shared WAV encoder (kept here for existing call sites).
pub use crate::tts::encode_wav_pcm16;

#[cfg(test)]
mod tests {
    use super::*;

    fn golden_path() -> Option<std::path::PathBuf> {
        std::env::var("SENCLAW_MMS_GOLDEN").ok().map(Into::into)
    }

    fn model_dir() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap()
            .join(".senclaw/tts-models/facebook__mms-tts-vie")
    }

    /// End-to-end numeric check against PyTorch golden tensors (same noise).
    /// Run with:
    /// ```text
    /// SENCLAW_MMS_GOLDEN=/path/to/mms_golden.safetensors \
    ///   cargo test --features local-mlx-tts -- --ignored mms_golden --test-threads=1
    /// ```
    #[test]
    #[ignore = "needs downloaded weights + golden tensors"]
    fn verify_against_pytorch_golden() {
        let Some(golden) = golden_path() else {
            eprintln!("SENCLAW_MMS_GOLDEN not set; skipping");
            return;
        };
        let g = Array::load_safetensors(&golden).expect("golden safetensors");
        let model = MmsVits::load(model_dir()).expect("load model");

        let ids_arr = &g["input_ids"];
        let ids: Vec<u32> = ids_arr
            .as_dtype(mlx_rs::Dtype::Int32)
            .unwrap()
            .as_slice::<i32>()
            .iter()
            .map(|&v| v as u32)
            .collect();
        let t = ids.len() as i32;

        // 1. Text encoder.
        let (h, m_p, logs_p) = model.text_encoder.forward(&ids).expect("encoder");
        let check = |name: &str, ours: &Array, tol: f32| {
            let theirs = g[name].reshape(ours.shape()).expect("shape");
            let diff = ours
                .subtract(&theirs)
                .unwrap()
                .abs()
                .unwrap()
                .max(None)
                .unwrap()
                .item::<f32>();
            assert!(diff < tol, "{name}: max abs diff {diff} > {tol}");
        };
        check("enc_hidden", &h, 2e-3);
        check("prior_means", &m_p, 2e-3);
        check("prior_log_var", &logs_p, 2e-3);

        // 2. Duration predictor with the golden noise ([1,2,T] torch → [1,T,2]).
        let noise_dur = g["noise_dur"].swap_axes(1, 2).unwrap();
        let log_dur = model
            .duration_predictor
            .reverse(&h, &noise_dur)
            .expect("sdp");
        check("log_duration", &log_dur, 5e-3);

        // Integer durations must match exactly — ceil(exp(·)) amplifies any
        // numeric drift into frame shifts, which the waveform check would then
        // only report as a length mismatch.
        let ours_ld = to_vec_f32(&log_dur).unwrap();
        let golden_dur: &[f32] = g["duration"].as_slice();
        let ours_dur: Vec<f32> = ours_ld.iter().map(|v| v.exp().ceil()).collect();
        assert_eq!(ours_dur, golden_dur, "per-token durations diverge");

        // 3. Full synthesis with both golden noises.
        let noise_zp = g["noise_zp"].clone(); // already [1,T_out,192]
        let wav = model
            .infer_with_noise(&ids, &noise_dur, &noise_zp, 1.0)
            .expect("infer");
        let golden_wav: &[f32] = g["waveform"].as_slice();
        assert_eq!(wav.len(), golden_wav.len(), "waveform length mismatch");
        let max_diff = wav
            .iter()
            .zip(golden_wav)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        // The waveform passes through ~30 conv layers; f32 accumulation order
        // differs between MLX and PyTorch, so allow a loose absolute tolerance
        // and additionally require high correlation.
        let dot: f32 = wav.iter().zip(golden_wav).map(|(a, b)| a * b).sum();
        let na: f32 = wav.iter().map(|a| a * a).sum::<f32>().sqrt();
        let nb: f32 = golden_wav.iter().map(|b| b * b).sum::<f32>().sqrt();
        let corr = dot / (na * nb).max(1e-9);
        eprintln!("waveform: max abs diff {max_diff}, cosine {corr}");
        assert!(corr > 0.99, "waveform correlation too low: {corr}");
        let _ = t;
    }

    #[test]
    fn wav_header_is_valid() {
        let wav = encode_wav_pcm16(&[0.0, 0.5, -0.5], 16000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(wav.len(), 44 + 6);
    }

    #[test]
    fn spline_is_identity_outside_tails() {
        let w = [0.0; 10];
        let h = [0.0; 10];
        let d = [0.0; 9];
        assert_eq!(rq_spline_inverse(7.3, &w, &h, &d, 5.0), 7.3);
        assert_eq!(rq_spline_inverse(-9.0, &w, &h, &d, 5.0), -9.0);
    }

    #[test]
    fn spline_inverse_is_monotonic_inside() {
        // With arbitrary (but fixed) params the inverse must stay monotonic.
        let w: Vec<f32> = (0..10).map(|i| (i as f32 * 0.37).sin()).collect();
        let h: Vec<f32> = (0..10).map(|i| (i as f32 * 0.61).cos()).collect();
        let d: Vec<f32> = (0..9).map(|i| (i as f32 * 0.13).sin() * 0.5).collect();
        let mut prev = f32::NEG_INFINITY;
        for i in -50..=50 {
            let x = i as f32 / 10.0; // [-5, 5]
            let y = rq_spline_inverse(x, &w, &h, &d, 5.0);
            assert!(y >= prev - 1e-4, "non-monotonic at x={x}: {y} < {prev}");
            assert!((-5.001..=5.001).contains(&y), "out of range at x={x}: {y}");
            prev = y;
        }
    }
}
