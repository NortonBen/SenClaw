//! Zipformer2 per-layer building blocks (inference-only).
//!
//! Each Zipformer2 encoder layer is a stack of: 3× FeedForward, 1× ConvolutionModule
//! ×2, attention modules, and BiasNorm. This file ports the **inference math** of
//! the two simplest building blocks — `FeedforwardModule` and `ConvolutionModule`
//! — from `k2-fsa/ZipVoice` (`zipvoice/models/modules/zipformer.py`).
//!
//! Training-only machinery (Balancer, Whiten, dropout, custom autograd) collapses
//! to the identity at inference and is therefore omitted.
//!
//! ## Tensor convention
//! Everything in this port is **channels-last** `[B, T, C]` — matching mlx-rs's
//! `Conv1d` input layout. The reference uses `[T, B, C]`/`[B, C, T]` and permutes;
//! since we never train, we can drop the permutes without changing numerics.
//!
//! ## Weight layout
//! - `Linear`: weight `[out, in]`, bias `[out]` (PyTorch / mlx-rs convention).
//! - `Conv1d` (depthwise): weight `[channels, kernel, 1]`, bias `[channels]`
//!   (mlx-rs layout — the mlx-community converter already permuted from PyTorch's
//!   `[channels, 1, kernel]`).

#![cfg(feature = "local-mlx-tts")]

use std::collections::HashMap;

use anyhow::{anyhow, Context, Result};
use mlx_rs::{error::Exception, ops, Array};

use super::scaling::{swoosh_l, swoosh_r};

/// A bare Linear layer (`y = x @ weight.T + bias`).
///
/// Mirrors PyTorch / `mlx_rs::nn::Linear` semantics but with explicit weight
/// arrays rather than the builder pattern, so it loads cleanly from a tensor map.
#[derive(Debug, Clone)]
pub struct Linear {
    /// Shape `[out, in]`.
    pub weight: Array,
    /// Shape `[out]`. `None` if the layer was built `bias=False`.
    pub bias: Option<Array>,
}

impl Linear {
    pub fn new(weight: Array, bias: Option<Array>) -> Self {
        Self { weight, bias }
    }

    /// Load `<prefix>.weight` and (optionally) `<prefix>.bias` from a tensor map.
    pub fn load(weights: &HashMap<String, Array>, prefix: &str) -> Result<Self> {
        let w_key = format!("{prefix}.weight");
        let w = weights
            .get(&w_key)
            .ok_or_else(|| anyhow!("missing tensor {w_key}"))?
            .clone();
        let b_key = format!("{prefix}.bias");
        let bias = weights.get(&b_key).cloned();
        Ok(Self { weight: w, bias })
    }

    /// `(out_dim, in_dim)`.
    pub fn shape(&self) -> (i32, i32) {
        let s = self.weight.shape();
        (s[0], s[1])
    }

    pub fn forward(&self, x: &Array) -> Result<Array, Exception> {
        let wt = self.weight.swap_axes(-2, -1)?;
        let y = ops::matmul(x, &wt)?;
        if let Some(b) = &self.bias {
            y.add(b)
        } else {
            Ok(y)
        }
    }
}

/// `FeedforwardModule` from Zipformer2 — inference path.
///
/// Reference (`scaling.py::ActivationDropoutAndLinearFunction.forward`,
/// `zipformer.py::FeedforwardModule.forward`) collapses to:
/// ```text
///   y = Linear_out(SwooshL(Linear_in(x)))
/// ```
/// The `hidden_balancer` and `out_whiten` are identity at inference. Dropout is off.
#[derive(Debug, Clone)]
pub struct FeedForward {
    pub in_proj: Linear,
    pub out_proj: Linear,
}

impl FeedForward {
    /// Load `<prefix>.in_proj.*` and `<prefix>.out_proj.*` from a tensor map.
    pub fn load(weights: &HashMap<String, Array>, prefix: &str) -> Result<Self> {
        let in_proj = Linear::load(weights, &format!("{prefix}.in_proj"))
            .with_context(|| format!("loading {prefix}.in_proj"))?;
        let out_proj = Linear::load(weights, &format!("{prefix}.out_proj"))
            .with_context(|| format!("loading {prefix}.out_proj"))?;
        Ok(Self { in_proj, out_proj })
    }

    pub fn forward(&self, x: &Array) -> Result<Array, Exception> {
        let h = self.in_proj.forward(x)?;
        let h = swoosh_l(&h)?;
        self.out_proj.forward(&h)
    }
}

/// `ConvolutionModule` from Zipformer2 — inference path, channels-last.
///
/// Reference (`zipformer.py::ConvolutionModule.forward`) at inference simplifies
/// to:
/// ```text
///   h    = in_proj(x)                              # [B, T, 2C]
///   x, s = split(h, 2, axis=-1)                    # GLU gate
///   x    = x * sigmoid(s)
///   x    = depthwise_conv1d(x, k, padding=k//2)    # [B, T, C]
///   x    = SwooshR(x)
///   y    = out_proj(x)                             # [B, T, C]
/// ```
/// `balancer1`, `balancer2`, `activation1/2` (Identity), and `whiten` are training-only.
#[derive(Debug, Clone)]
pub struct ConvolutionModule {
    pub in_proj: Linear,
    pub depthwise_weight: Array, // [C, K, 1]
    pub depthwise_bias: Option<Array>,
    pub kernel_size: i32,
    pub channels: i32,
    pub out_proj: Linear,
}

impl ConvolutionModule {
    pub fn load(weights: &HashMap<String, Array>, prefix: &str) -> Result<Self> {
        let in_proj = Linear::load(weights, &format!("{prefix}.in_proj"))?;
        let out_proj = Linear::load(weights, &format!("{prefix}.out_proj"))?;

        let dw_w_key = format!("{prefix}.depthwise_conv.weight");
        let dw_b_key = format!("{prefix}.depthwise_conv.bias");
        let dw_w = weights
            .get(&dw_w_key)
            .ok_or_else(|| anyhow!("missing tensor {dw_w_key}"))?
            .clone();
        let dw_b = weights.get(&dw_b_key).cloned();

        // Depthwise weight shape: [channels, kernel, 1] in mlx-rs layout.
        let shape = dw_w.shape();
        if shape.len() != 3 || shape[2] != 1 {
            return Err(anyhow!(
                "{dw_w_key}: expected depthwise weight [C, K, 1], got {:?}",
                shape
            ));
        }
        let channels = shape[0];
        let kernel_size = shape[1];

        // `in_proj` projects channels → 2*channels (the GLU bottleneck).
        let (out_dim, in_dim) = in_proj.shape();
        if in_dim != channels || out_dim != 2 * channels {
            return Err(anyhow!(
                "{prefix}.in_proj shape {:?} inconsistent with depthwise channels {channels}",
                (out_dim, in_dim),
            ));
        }

        Ok(Self {
            in_proj,
            depthwise_weight: dw_w,
            depthwise_bias: dw_b,
            kernel_size,
            channels,
            out_proj,
        })
    }

    pub fn forward(&self, x: &Array) -> Result<Array, Exception> {
        // GLU bottleneck.
        let h = self.in_proj.forward(x)?; // [B, T, 2C]
        let parts = h.split(2, -1)?;
        let xb = &parts[0];
        let s = &parts[1];
        let gated = xb.multiply(ops::sigmoid(s)?)?; // [B, T, C]

        // Depthwise conv (channels-last). groups == channels so each channel is its own filter.
        let pad = self.kernel_size / 2;
        let conv = ops::conv1d(&gated, &self.depthwise_weight, 1, pad, 1, self.channels)?;
        let conv = if let Some(b) = &self.depthwise_bias {
            conv.add(b)?
        } else {
            conv
        };

        let act = swoosh_r(&conv)?;
        self.out_proj.forward(&act)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_weights(items: &[(&str, Array)]) -> HashMap<String, Array> {
        items
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    // ---------- Linear ----------

    #[test]
    fn linear_matches_handcomputed() {
        // y = x @ W^T + b, weight shape [out=2, in=3].
        let w = Array::from_slice(&[1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0], &[2, 3]);
        let b = Array::from_slice(&[0.5f32, -0.5], &[2]);
        let lin = Linear::new(w, Some(b));
        let x = Array::from_slice(&[1.0f32, 2.0, 3.0], &[1, 3]);
        let y = lin.forward(&x).unwrap();
        let got = y.as_slice::<f32>();
        // [1*1+2*0+3*0, 1*0+2*1+3*0] + [0.5, -0.5] = [1.5, 1.5]
        assert!((got[0] - 1.5).abs() < 1e-5);
        assert!((got[1] - 1.5).abs() < 1e-5);
    }

    #[test]
    fn linear_without_bias() {
        let w = Array::from_slice(&[2.0f32, 0.0, 0.0, 3.0], &[2, 2]);
        let lin = Linear::new(w, None);
        let x = Array::from_slice(&[1.0f32, 1.0], &[1, 2]);
        let y = lin.forward(&x).unwrap();
        let got = y.as_slice::<f32>();
        assert!((got[0] - 2.0).abs() < 1e-5);
        assert!((got[1] - 3.0).abs() < 1e-5);
    }

    #[test]
    fn linear_load_missing_weight_errs() {
        let w = make_weights(&[]);
        assert!(Linear::load(&w, "x").is_err());
    }

    // ---------- FeedForward ----------

    #[test]
    fn feed_forward_synthetic_numerics() {
        // identity-projection FFN: in_proj=eye(2), out_proj=eye(2), no biases.
        let eye = Array::from_slice(&[1.0f32, 0.0, 0.0, 1.0], &[2, 2]);
        let w = make_weights(&[
            ("ff.in_proj.weight", eye.clone()),
            ("ff.out_proj.weight", eye),
        ]);
        let ff = FeedForward::load(&w, "ff").expect("load");
        let x = Array::from_slice(&[1.0f32, -1.0], &[1, 2]);
        let y = ff.forward(&x).unwrap();
        let got = y.as_slice::<f32>();
        // After in_proj (identity): [1, -1]. SwooshL: logaddexp(0, x-4)-0.08x-0.035.
        let expected = |v: f32| (1.0 + (v - 4.0).exp()).ln() - 0.08 * v - 0.035;
        assert!((got[0] - expected(1.0)).abs() < 1e-4);
        assert!((got[1] - expected(-1.0)).abs() < 1e-4);
    }

    // ---------- ConvolutionModule ----------

    #[test]
    fn convolution_module_loads_and_runs_synthetic() {
        // channels=2, kernel=3. Build minimal weights that pass validation.
        let in_w = Array::from_slice(
            &[
                // shape [4, 2] - projects C=2 -> 2C=4
                1.0f32, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0,
            ],
            &[4, 2],
        );
        let dw_w = Array::from_slice(
            // [C=2, K=3, 1] - identity-ish filter (center=1, edges=0)
            &[0.0f32, 1.0, 0.0, 0.0, 1.0, 0.0],
            &[2, 3, 1],
        );
        let out_w = Array::from_slice(&[1.0f32, 0.0, 0.0, 1.0], &[2, 2]);
        let w = make_weights(&[
            ("c.in_proj.weight", in_w),
            ("c.depthwise_conv.weight", dw_w),
            ("c.out_proj.weight", out_w),
        ]);
        let cm = ConvolutionModule::load(&w, "c").expect("load");
        assert_eq!(cm.kernel_size, 3);
        assert_eq!(cm.channels, 2);

        // x: [B=1, T=4, C=2], sequence [[1,1],[2,2],[3,3],[4,4]].
        let x = Array::from_slice(&[1.0f32, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0], &[1, 4, 2]);
        let y = cm.forward(&x).expect("forward");
        let shape = y.shape();
        assert_eq!(shape, &[1, 4, 2], "shape must be [B,T,C] (channels-last)");
    }

    #[test]
    fn convolution_module_rejects_bad_depthwise_shape() {
        // depthwise weight last dim != 1 → must error.
        let in_w = Array::from_slice(&[0.0f32; 4 * 2], &[4, 2]);
        let dw_w = Array::from_slice(&[0.0f32; 2 * 3 * 2], &[2, 3, 2]); // bad
        let out_w = Array::from_slice(&[0.0f32; 4], &[2, 2]);
        let w = make_weights(&[
            ("c.in_proj.weight", in_w),
            ("c.depthwise_conv.weight", dw_w),
            ("c.out_proj.weight", out_w),
        ]);
        assert!(ConvolutionModule::load(&w, "c").is_err());
    }

    // ---------- Real-weights smoke (ignored by default) ----------

    /// Load the FeedForward + ConvolutionModule for layer 0 of the real
    /// `text_encoder` and run a forward pass. Verifies the load path against
    /// the actual checkpoint and that mlx-rs accepts the saved tensor shapes.
    /// Run with: `cargo test --features local-mlx-tts -- --ignored real_zipvoice_layer0 --test-threads=1`
    #[test]
    #[ignore = "requires the downloaded zipvoice-vietnamese checkpoint"]
    fn real_zipvoice_layer0_blocks_load_and_run() {
        use crate::tts::zipvoice::weights::{load_arrays, WeightIndex};

        let dir = std::path::PathBuf::from(std::env::var("HOME").unwrap())
            .join(".senclaw/tts-models/mlx-community__zipvoice-vietnamese");
        let idx = WeightIndex::from_dir(&dir).expect("index");
        let arrays = load_arrays(&dir, &idx).expect("arrays");

        // text_encoder dim = 192, conv kernel = 9 (per config.json). The 3 FFs in
        // a Zipformer2 layer use **scaled** inner dims (~0.75x / 1.0x / 1.25x of
        // text_encoder_feedforward_dim=512), so the actual saved inner dims are
        // 384 / 512 / 640. Verified against the safetensors header.
        let dim = 192;
        let ff1 = FeedForward::load(&arrays, "text_encoder.encoder.layers.0.feed_forward1")
            .expect("FF1 load");
        assert_eq!(ff1.in_proj.shape(), (384, 192));
        assert_eq!(ff1.out_proj.shape(), (192, 384));
        let ff2 = FeedForward::load(&arrays, "text_encoder.encoder.layers.0.feed_forward2")
            .expect("FF2 load");
        assert_eq!(ff2.in_proj.shape(), (512, 192));
        let ff3 = FeedForward::load(&arrays, "text_encoder.encoder.layers.0.feed_forward3")
            .expect("FF3 load");
        assert_eq!(ff3.in_proj.shape(), (640, 192));

        let cm = ConvolutionModule::load(&arrays, "text_encoder.encoder.layers.0.conv_module1")
            .expect("Conv load");
        assert_eq!(cm.channels, 192);
        assert_eq!(cm.kernel_size, 9);

        // Forward through both with a short synthetic input [B=1, T=8, C=192].
        let x = Array::ones::<f32>(&[1, 8, dim]).expect("ones");
        let yf1 = ff1.forward(&x).expect("FF1 forward");
        let yf2 = ff2.forward(&x).expect("FF2 forward");
        let yf3 = ff3.forward(&x).expect("FF3 forward");
        assert_eq!(yf1.shape(), &[1, 8, dim]);
        assert_eq!(yf2.shape(), &[1, 8, dim]);
        assert_eq!(yf3.shape(), &[1, 8, dim]);
        let yc = cm.forward(&x).expect("Conv forward");
        assert_eq!(yc.shape(), &[1, 8, dim]);

        // Sanity: output must be finite.
        for (label, arr) in [("ff1", &yf1), ("ff2", &yf2), ("ff3", &yf3), ("conv", &yc)] {
            let slice = arr.as_slice::<f32>();
            assert!(
                slice.iter().all(|v| v.is_finite()),
                "{label} produced non-finite outputs"
            );
        }
    }
}
