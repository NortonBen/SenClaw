//! Zipformer2 scaling primitives, ported to `mlx-rs`.
//!
//! Faithful inference-path ports of `k2-fsa/ZipVoice`
//! (`zipvoice/models/modules/scaling.py`). Training-only machinery (Balancer,
//! Whiten, the custom autograd quantized backward passes) collapses to the
//! identity at inference, so only the forward math is reproduced here.
//!
//! Reference forward forms (inference):
//! ```text
//!   SwooshL(x) = logaddexp(0, x - 4) - 0.08*x - 0.035
//!   SwooshR(x) = logaddexp(0, x - 1) - 0.08*x - 0.313261687
//!   BiasNorm(x) = x * (mean((x - bias)^2, dim=-1, keepdim)^-0.5 * exp(log_scale))
//! ```
//! `log_scale` is a scalar parameter clamped to `[-1.5, 1.5]`.

#![cfg(feature = "local-mlx-tts")]

use mlx_rs::{error::Exception, ops, Array};

/// Swoosh-L activation: `logaddexp(0, x-4) - 0.08*x - 0.035`.
///
/// Derivatives lie in `(-0.08, 0.92)`. Used in feed-forward / conv modules.
pub fn swoosh_l(x: &Array) -> Result<Array, Exception> {
    let zero = Array::from_f32(0.0);
    let shifted = x.subtract(Array::from_f32(4.0))?;
    let lae = ops::logaddexp(&zero, &shifted)?;
    lae.subtract(x.multiply(Array::from_f32(0.08))?)?
        .subtract(Array::from_f32(0.035))
}

/// Swoosh-R activation: `logaddexp(0, x-1) - 0.08*x - 0.313261687`.
pub fn swoosh_r(x: &Array) -> Result<Array, Exception> {
    let zero = Array::from_f32(0.0);
    let shifted = x.subtract(Array::from_f32(1.0))?;
    let lae = ops::logaddexp(&zero, &shifted)?;
    lae.subtract(x.multiply(Array::from_f32(0.08))?)?
        .subtract(Array::from_f32(0.313_261_687))
}

/// BiasNorm normalization layer (Zipformer2's cheaper LayerNorm replacement).
///
/// Holds the learned `bias` (`[num_channels]`) and scalar `log_scale`. The
/// channel dimension is the last axis (`channel_dim = -1`), which is how every
/// ZipVoice usage instantiates it.
#[derive(Debug, Clone)]
pub struct BiasNorm {
    pub bias: Array,
    pub log_scale: Array,
}

/// Clamp bounds on `log_scale` from the reference (`log_scale_min/max`).
const LOG_SCALE_MIN: f32 = -1.5;
const LOG_SCALE_MAX: f32 = 1.5;

impl BiasNorm {
    pub fn new(bias: Array, log_scale: Array) -> Self {
        Self { bias, log_scale }
    }

    /// `x * (mean((x - bias)^2, dim=-1, keepdim)^-0.5 * exp(clamp(log_scale)))`.
    pub fn forward(&self, x: &Array) -> Result<Array, Exception> {
        // scales over the last axis.
        let diff = x.subtract(&self.bias)?;
        let var = diff.square()?.mean_axis(-1, true)?; // [..., 1]
        let inv_rms = var.rsqrt()?; // var^-0.5
        let clamped = ops::clip(
            &self.log_scale,
            (Array::from_f32(LOG_SCALE_MIN), Array::from_f32(LOG_SCALE_MAX)),
        )?;
        let scale = clamped.exp()?;
        let scales = inv_rms.multiply(&scale)?;
        x.multiply(&scales)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32, tol: f32) {
        assert!((a - b).abs() < tol, "expected {b}, got {a} (tol {tol})");
    }

    // logaddexp(0, z) = ln(1 + e^z).
    fn ref_swoosh_l(x: f32) -> f32 {
        (1.0 + (x - 4.0).exp()).ln() - 0.08 * x - 0.035
    }
    fn ref_swoosh_r(x: f32) -> f32 {
        (1.0 + (x - 1.0).exp()).ln() - 0.08 * x - 0.313_261_687
    }

    #[test]
    fn swoosh_l_matches_reference() {
        let xs = [-3.0f32, -1.0, 0.0, 0.5, 2.0, 5.0, 8.0];
        let x = Array::from_slice(&xs, &[xs.len() as i32]);
        let y = swoosh_l(&x).unwrap();
        let got = y.as_slice::<f32>();
        for (i, &xv) in xs.iter().enumerate() {
            approx(got[i], ref_swoosh_l(xv), 1e-4);
        }
    }

    #[test]
    fn swoosh_r_matches_reference() {
        let xs = [-3.0f32, -1.0, 0.0, 0.5, 2.0, 5.0, 8.0];
        let x = Array::from_slice(&xs, &[xs.len() as i32]);
        let y = swoosh_r(&x).unwrap();
        let got = y.as_slice::<f32>();
        for (i, &xv) in xs.iter().enumerate() {
            approx(got[i], ref_swoosh_r(xv), 1e-4);
        }
    }

    #[test]
    fn bias_norm_matches_hand_computed() {
        // One vector of 4 channels; bias = 0, log_scale = 0 → scale = 1.
        let xs = [1.0f32, 2.0, 3.0, 4.0];
        let x = Array::from_slice(&xs, &[1, 4]);
        let bias = Array::from_slice(&[0.0f32; 4], &[4]);
        let log_scale = Array::from_f32(0.0);
        let bn = BiasNorm::new(bias, log_scale);
        let y = bn.forward(&x).unwrap();
        let got = y.as_slice::<f32>();

        // mean(x^2) = (1+4+9+16)/4 = 7.5 ; inv_rms = 7.5^-0.5
        let inv_rms = 7.5f32.powf(-0.5);
        for (i, &xv) in xs.iter().enumerate() {
            approx(got[i], xv * inv_rms, 1e-4);
        }
    }

    #[test]
    fn bias_norm_applies_bias_and_log_scale() {
        let xs = [1.0f32, 2.0, 3.0, 4.0];
        let x = Array::from_slice(&xs, &[1, 4]);
        let bias_v = [0.5f32, 0.5, 0.5, 0.5];
        let bias = Array::from_slice(&bias_v, &[4]);
        let log_scale = Array::from_f32(0.5);
        let bn = BiasNorm::new(bias, log_scale);
        let y = bn.forward(&x).unwrap();
        let got = y.as_slice::<f32>();

        // var = mean((x-0.5)^2) over [0.5,1.5,2.5,3.5] = (0.25+2.25+6.25+12.25)/4 = 5.25
        let inv_rms = 5.25f32.powf(-0.5);
        let scale = 0.5f32.exp();
        for (i, &xv) in xs.iter().enumerate() {
            approx(got[i], xv * inv_rms * scale, 1e-4);
        }
    }

    #[test]
    fn bias_norm_clamps_log_scale() {
        // log_scale far above the max must clamp to exp(1.5).
        let xs = [1.0f32, 2.0, 3.0, 4.0];
        let x = Array::from_slice(&xs, &[1, 4]);
        let bias = Array::from_slice(&[0.0f32; 4], &[4]);
        let bn = BiasNorm::new(bias, Array::from_f32(10.0));
        let y = bn.forward(&x).unwrap();
        let got = y.as_slice::<f32>();
        let inv_rms = 7.5f32.powf(-0.5);
        let scale = 1.5f32.exp(); // clamped
        for (i, &xv) in xs.iter().enumerate() {
            approx(got[i], xv * inv_rms * scale, 1e-4);
        }
    }
}
