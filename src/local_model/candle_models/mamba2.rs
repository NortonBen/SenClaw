//! Mamba 2 (SSM — Structured State Space Duality) inference.
//!
//! Architecture: ["Transformers are SSMs"](https://arxiv.org/abs/2405.21060).
//! Targets `model_type = "mamba2"` checkpoints from the `state-spaces` org
//! (weights stored under the `backbone.*` prefix in safetensors files).
//!
//! Inference is fully sequential — one token at a time.  Each layer maintains
//! two state tensors:
//!   - `conv_state`: depthwise-conv ring buffer `[d_conv_channels, d_conv - 1]`
//!   - `ssm_state`:  SSM hidden state `[n_heads, d_state, head_dim]`
//!
//! This is O(L × n_layers) per generation step but requires no parallel scan.

use candle_core::{DType, Device, Module, Result, Tensor};
use candle_nn::{embedding, linear_no_bias, rms_norm, Embedding, Linear, RmsNorm, VarBuilder};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

fn default_expand() -> usize {
    2
}
fn default_d_state() -> usize {
    128
}
fn default_d_conv() -> usize {
    4
}
fn default_ngroups() -> usize {
    1
}
fn default_rms_norm_eps() -> f64 {
    1e-5
}
fn default_pad_vocab() -> usize {
    8
}

/// Deserialises `config.json` for `state-spaces/mamba2-*` checkpoints.
///
/// Supports both the original state-spaces field names (`d_model`, `headdim`,
/// `ngroups`, `d_state`, `d_conv`) and the HuggingFace conversion aliases
/// (`hidden_size`, `head_dim`, `n_groups`, `state_size`, `conv_kernel`).
#[derive(Debug, Deserialize, Clone)]
pub struct Mamba2Config {
    #[serde(alias = "hidden_size")]
    pub d_model: usize,
    #[serde(alias = "num_hidden_layers")]
    pub n_layer: usize,
    pub vocab_size: usize,
    #[serde(default = "default_pad_vocab")]
    pub pad_vocab_size_multiple: usize,
    /// Head dimension (field called `headdim` in state-spaces checkpoints).
    #[serde(alias = "headdim", alias = "head_dim")]
    pub d_head: usize,
    #[serde(default = "default_d_state", alias = "state_size")]
    pub d_state: usize,
    #[serde(default = "default_d_conv", alias = "conv_kernel")]
    pub d_conv: usize,
    #[serde(default = "default_expand")]
    pub expand: usize,
    #[serde(default = "default_ngroups", alias = "n_groups")]
    pub ngroups: usize,
    #[serde(default = "default_rms_norm_eps")]
    pub rms_norm_eps: f64,
    /// Whether `embedding.weight` and `lm_head.weight` are tied.
    #[serde(default, alias = "tie_embeddings", alias = "tie_word_embeddings")]
    pub tie_embeddings: bool,
}

impl Mamba2Config {
    /// `d_inner = expand × d_model`
    pub fn d_inner(&self) -> usize {
        self.d_model * self.expand
    }
    /// Number of SSM heads = `d_inner / head_dim`
    pub fn n_heads(&self) -> usize {
        self.d_inner() / self.d_head
    }
    /// Channels fed through the depthwise conv: `d_inner + 2 × ngroups × d_state`
    pub fn d_conv_channels(&self) -> usize {
        self.d_inner() + 2 * self.ngroups * self.d_state
    }
    /// Padded vocab size
    pub fn vocab_size_padded(&self) -> usize {
        let p = self.pad_vocab_size_multiple.max(1);
        self.vocab_size.div_ceil(p) * p
    }
    /// in_proj output width: `2×d_inner + 2×ngroups×d_state + n_heads`
    pub fn in_proj_out(&self) -> usize {
        self.d_inner() + self.d_conv_channels() + self.n_heads()
    }
}

// ---------------------------------------------------------------------------
// Per-generation recurrent state
// ---------------------------------------------------------------------------

/// Recurrent inference state for one generation.  Create fresh before each
/// `Mamba2Model::forward_token` call sequence; discard after.
pub struct Mamba2State {
    /// Per-layer depthwise-conv ring buffer: `[d_conv_channels, d_conv − 1]`
    pub conv_states: Vec<Tensor>,
    /// Per-layer SSM hidden state: `[n_heads, d_state, head_dim]`
    pub ssm_states: Vec<Tensor>,
}

impl Mamba2State {
    pub fn new(cfg: &Mamba2Config, dtype: DType, device: &Device) -> Result<Self> {
        let dc = cfg.d_conv_channels();
        let d_c = cfg.d_conv.saturating_sub(1);
        let nh = cfg.n_heads();
        let conv_states = (0..cfg.n_layer)
            .map(|_| Tensor::zeros((dc, d_c), dtype, device))
            .collect::<Result<Vec<_>>>()?;
        let ssm_states = (0..cfg.n_layer)
            .map(|_| Tensor::zeros((nh, cfg.d_state, cfg.d_head), dtype, device))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            conv_states,
            ssm_states,
        })
    }
}

// ---------------------------------------------------------------------------
// Mamba2Mixer — core SSM block (single-token recurrent step)
// ---------------------------------------------------------------------------

struct Mamba2Mixer {
    in_proj: Linear,
    conv1d_weight: Tensor, // [d_conv_channels, 1, d_conv]
    conv1d_bias: Tensor,   // [d_conv_channels]
    dt_bias: Tensor,       // [n_heads]
    a_log: Tensor,         // [n_heads]
    d_param: Tensor,       // [n_heads]
    norm: RmsNorm,
    out_proj: Linear,
    // config-derived dims (avoid cloning the whole config)
    d_inner: usize,
    d_conv_channels: usize,
    n_heads: usize,
    n_groups: usize,
    d_state: usize,
    d_head: usize,
    d_conv: usize,
}

impl Mamba2Mixer {
    fn new(cfg: &Mamba2Config, vb: VarBuilder) -> Result<Self> {
        let d_inner = cfg.d_inner();
        let dc = cfg.d_conv_channels();
        let nh = cfg.n_heads();
        Ok(Self {
            in_proj: linear_no_bias(cfg.d_model, cfg.in_proj_out(), vb.pp("in_proj"))?,
            conv1d_weight: vb.get((dc, 1, cfg.d_conv), "conv1d.weight")?,
            conv1d_bias: vb.get(dc, "conv1d.bias")?,
            dt_bias: vb.get(nh, "dt_bias")?,
            a_log: vb.get(nh, "A_log")?,
            d_param: vb.get(nh, "D")?,
            norm: rms_norm(d_inner, cfg.rms_norm_eps, vb.pp("norm"))?,
            out_proj: linear_no_bias(d_inner, cfg.d_model, vb.pp("out_proj"))?,
            d_inner,
            d_conv_channels: dc,
            n_heads: nh,
            n_groups: cfg.ngroups,
            d_state: cfg.d_state,
            d_head: cfg.d_head,
            d_conv: cfg.d_conv,
        })
    }

    /// Single-token recurrent step.
    ///
    /// `x`:   `[1, d_model]`  (batch=1 assumed throughout)
    /// Returns `[1, d_model]`
    fn forward_token(
        &self,
        x: &Tensor,
        layer_idx: usize,
        state: &mut Mamba2State,
    ) -> Result<Tensor> {
        // ── 1. in_proj ─────────────────────────────────────────────────────
        let proj = self.in_proj.forward(x)?.squeeze(0)?; // [in_proj_out]
        let z = proj.narrow(0, 0, self.d_inner)?; // [d_inner]  — gate
        let xbc = proj.narrow(0, self.d_inner, self.d_conv_channels)?; // [d_conv_channels]
        let dt = proj.narrow(0, self.d_inner + self.d_conv_channels, self.n_heads)?; // [n_heads]

        // ── 2. Depthwise conv (ring-buffer update) ──────────────────────────
        let conv_state = &state.conv_states[layer_idx]; // [dc, d_conv-1]
        let window = Tensor::cat(&[conv_state, &xbc.unsqueeze(1)?], 1)?; // [dc, d_conv]
        // Shift ring buffer: drop oldest slot
        if self.d_conv > 1 {
            state.conv_states[layer_idx] = window.narrow(1, 1, self.d_conv - 1)?.contiguous()?;
        }
        // conv1d output: element-wise weight × window, summed over kernel dim
        let w = self.conv1d_weight.squeeze(1)?; // [dc, d_conv]
        let conv_out = ((&w * &window)?.sum(1)? + &self.conv1d_bias)?; // [dc]

        // ── 3. Split conv output and activate ──────────────────────────────
        let x_ssm = conv_out.narrow(0, 0, self.d_inner)?.silu()?; // [d_inner]
        let b_flat =
            conv_out.narrow(0, self.d_inner, self.n_groups * self.d_state)?; // [ng*ds]
        let c_flat = conv_out.narrow(
            0,
            self.d_inner + self.n_groups * self.d_state,
            self.n_groups * self.d_state,
        )?; // [ng*ds]

        // ── 4. dt = softplus(dt + dt_bias) ─────────────────────────────────
        let dt = (&dt + &self.dt_bias)?;
        let dt = dt.to_dtype(DType::F32)?;
        let dt = (dt.exp()? + 1.0f64)?.log()?.to_dtype(x_ssm.dtype())?; // softplus

        // ── 5. Discrete-time A: dA = exp(A × dt), A = -exp(A_log) ──────────
        let a = self.a_log.to_dtype(x_ssm.dtype())?.exp()?.neg()?; // [n_heads]
        let da = (&a * &dt)?.exp()?; // [n_heads]

        // ── 6. Reshape for vectorised SSM update ────────────────────────────
        let x_head = x_ssm.reshape((self.n_heads, self.d_head))?; // [nh, dh]
        let hpg = self.n_heads / self.n_groups; // heads per group
        // Expand B, C from [ngroups, d_state] → [n_heads, d_state]
        let b_per = b_flat
            .reshape((self.n_groups, self.d_state))?
            .unsqueeze(1)?
            .expand((self.n_groups, hpg, self.d_state))?
            .reshape((self.n_heads, self.d_state))?;
        let c_per = c_flat
            .reshape((self.n_groups, self.d_state))?
            .unsqueeze(1)?
            .expand((self.n_groups, hpg, self.d_state))?
            .reshape((self.n_heads, self.d_state))?;

        // ── 7. SSM state update ─────────────────────────────────────────────
        // new_state[h] = dA[h] × state[h] + dB[h].outer(x[h])
        //   where dB[h] = dt[h] × B[h]
        let ssm = state.ssm_states[layer_idx].clone(); // [nh, ds, dh]
        let da3 = da.reshape((self.n_heads, 1usize, 1usize))?; // [nh,1,1]
        let dt3 = dt.reshape((self.n_heads, 1usize, 1usize))?; // [nh,1,1]
        // outer product B[h] ⊗ x[h]: [nh, ds, 1] × [nh, 1, dh] → via matmul
        let b3 = b_per.unsqueeze(2)?; // [nh, ds, 1]
        let x3 = x_head.unsqueeze(1)?; // [nh, 1, dh]
        let outer = b3.matmul(&x3)?; // [nh, ds, dh]
        let new_ssm = (ssm.broadcast_mul(&da3)? + dt3.broadcast_mul(&outer)?)?; // [nh,ds,dh]
        state.ssm_states[layer_idx] = new_ssm.clone();

        // ── 8. Output y = C × state + D × x ────────────────────────────────
        // y[h] = C[h] @ state[h]  →  [1, ds] × [ds, dh] = [1, dh] via matmul
        let c3 = c_per.unsqueeze(1)?; // [nh, 1, ds]
        let y = c3.matmul(&new_ssm)?.squeeze(1)?; // [nh, dh]
        let y_flat = y.reshape(self.d_inner)?; // [d_inner]
        // D skip: D[h] broadcast over each head's d_head outputs
        let d_exp = self.d_param
            .to_dtype(x_ssm.dtype())?
            .unsqueeze(1)?
            .expand((self.n_heads, self.d_head))?
            .reshape(self.d_inner)?;
        let y_flat = (y_flat + d_exp.mul(&x_ssm)?)?;

        // ── 9. Norm + gate + out_proj ────────────────────────────────────────
        let y_normed = self.norm.forward(&y_flat.unsqueeze(0)?)?.squeeze(0)?; // [d_inner]
        let gated = (y_normed * z.silu()?)?; // [d_inner]
        self.out_proj.forward(&gated.unsqueeze(0)?) // [1, d_model]
    }
}

// ---------------------------------------------------------------------------
// Residual block: RMSNorm → Mixer → residual add
// ---------------------------------------------------------------------------

struct Mamba2Block {
    norm: RmsNorm,
    mixer: Mamba2Mixer,
}

impl Mamba2Block {
    fn new(_layer_idx: usize, cfg: &Mamba2Config, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            norm: rms_norm(cfg.d_model, cfg.rms_norm_eps, vb.pp("norm"))?,
            mixer: Mamba2Mixer::new(cfg, vb.pp("mixer"))?,
        })
    }

    fn forward(
        &self,
        x: &Tensor,
        layer_idx: usize,
        state: &mut Mamba2State,
    ) -> Result<Tensor> {
        let delta = self.mixer.forward_token(&self.norm.forward(x)?, layer_idx, state)?;
        (x + &delta)
    }
}

// ---------------------------------------------------------------------------
// Full Mamba 2 model
// ---------------------------------------------------------------------------

pub struct Mamba2Model {
    embedding: Embedding,
    layers: Vec<Mamba2Block>,
    norm_f: RmsNorm,
    lm_head: Linear,
    pub cfg: Mamba2Config,
}

impl Mamba2Model {
    /// Load from a VarBuilder already positioned at the `backbone` prefix
    /// (i.e. the caller passes `vb.pp("backbone")`).
    pub fn from_vb(cfg: &Mamba2Config, vb: VarBuilder) -> Result<Self> {
        let vocab = cfg.vocab_size_padded();
        let embedding = embedding(vocab, cfg.d_model, vb.pp("embedding"))?;
        let layers = (0..cfg.n_layer)
            .map(|i| Mamba2Block::new(i, cfg, vb.pp(format!("layers.{i}"))))
            .collect::<Result<Vec<_>>>()?;
        let norm_f = rms_norm(cfg.d_model, cfg.rms_norm_eps, vb.pp("norm_f"))?;
        let lm_head = if cfg.tie_embeddings {
            Linear::new(embedding.embeddings().clone(), None)
        } else {
            linear_no_bias(cfg.d_model, vocab, vb.pp("lm_head"))?
        };
        Ok(Self {
            embedding,
            layers,
            norm_f,
            lm_head,
            cfg: cfg.clone(),
        })
    }

    /// Process a single token, update `state`, and return logits `[1, vocab_size]`.
    pub fn forward_token(&self, token: u32, state: &mut Mamba2State) -> Result<Tensor> {
        let tok = Tensor::new(&[token], self.embedding.embeddings().device())?;
        let mut x = self.embedding.forward(&tok)?; // [1, d_model]
        for (i, layer) in self.layers.iter().enumerate() {
            x = layer.forward(&x, i, state)?;
        }
        self.lm_head.forward(&self.norm_f.forward(&x)?) // [1, vocab]
    }

    pub fn n_layers(&self) -> usize {
        self.layers.len()
    }

    pub fn dtype(&self) -> DType {
        self.embedding.embeddings().dtype()
    }

    pub fn device(&self) -> &Device {
        self.embedding.embeddings().device()
    }
}
