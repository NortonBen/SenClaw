//! `config.json` parsing for HF `VitsModel` checkpoints (`facebook/mms-tts-*`).
//!
//! Only the hyperparameters the inference path needs are modelled; unknown
//! fields are ignored so newer transformers exports still parse.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

fn default_one() -> f32 {
    1.0
}

/// VITS hyperparameters (subset used at inference).
#[derive(Debug, Clone, Deserialize)]
pub struct VitsConfig {
    pub vocab_size: usize,
    /// Transformer / conv channel width (192 for MMS).
    pub hidden_size: i32,
    pub num_hidden_layers: usize,
    pub num_attention_heads: i32,
    /// Relative-attention window (`4` for MMS). `0`/absent disables rel-pos.
    #[serde(default)]
    pub window_size: i32,
    pub ffn_dim: i32,
    pub ffn_kernel_size: i32,
    #[serde(default = "default_layer_norm_eps")]
    pub layer_norm_eps: f32,
    pub leaky_relu_slope: f32,

    /// Latent channels through the flow (192 for MMS).
    pub flow_size: i32,
    pub prior_encoder_num_flows: usize,
    pub prior_encoder_num_wavenet_layers: usize,
    pub wavenet_kernel_size: i32,
    pub wavenet_dilation_rate: i32,

    pub use_stochastic_duration_prediction: bool,
    pub duration_predictor_kernel_size: i32,
    pub duration_predictor_num_flows: usize,
    pub duration_predictor_flow_bins: usize,
    pub duration_predictor_tail_bound: f32,
    pub depth_separable_channels: i32,
    pub depth_separable_num_layers: usize,

    pub upsample_initial_channel: i32,
    pub upsample_rates: Vec<i32>,
    pub upsample_kernel_sizes: Vec<i32>,
    pub resblock_kernel_sizes: Vec<i32>,
    pub resblock_dilation_sizes: Vec<Vec<i32>>,

    pub sampling_rate: u32,
    pub num_speakers: usize,
    pub speaker_embedding_size: i32,

    #[serde(default = "default_one")]
    pub speaking_rate: f32,
    #[serde(default = "default_noise_scale")]
    pub noise_scale: f32,
    #[serde(default = "default_noise_scale_duration")]
    pub noise_scale_duration: f32,
}

fn default_layer_norm_eps() -> f32 {
    1e-5
}
fn default_noise_scale() -> f32 {
    0.667
}
fn default_noise_scale_duration() -> f32 {
    0.8
}

impl VitsConfig {
    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).context("parsing VITS config.json")
    }

    pub fn load(dir: impl AsRef<Path>) -> Result<Self> {
        let path = dir.as_ref().join("config.json");
        let s = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::from_json(&s)
    }

    /// Total upsampling factor (latent frame → audio samples). 256 for MMS.
    pub fn total_upsample(&self) -> usize {
        self.upsample_rates.iter().map(|&r| r as usize).product()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mms_vie_config() {
        let s = r#"{
            "vocab_size": 95, "hidden_size": 192, "num_hidden_layers": 6,
            "num_attention_heads": 2, "window_size": 4, "ffn_dim": 768,
            "ffn_kernel_size": 3, "layer_norm_eps": 1e-05, "leaky_relu_slope": 0.1,
            "flow_size": 192, "prior_encoder_num_flows": 4,
            "prior_encoder_num_wavenet_layers": 4, "wavenet_kernel_size": 5,
            "wavenet_dilation_rate": 1, "use_stochastic_duration_prediction": true,
            "duration_predictor_kernel_size": 3, "duration_predictor_num_flows": 4,
            "duration_predictor_flow_bins": 10, "duration_predictor_tail_bound": 5.0,
            "depth_separable_channels": 2, "depth_separable_num_layers": 3,
            "upsample_initial_channel": 512, "upsample_rates": [8, 8, 2, 2],
            "upsample_kernel_sizes": [16, 16, 4, 4], "resblock_kernel_sizes": [3, 7, 11],
            "resblock_dilation_sizes": [[1,3,5],[1,3,5],[1,3,5]],
            "sampling_rate": 16000, "num_speakers": 1, "speaker_embedding_size": 0,
            "noise_scale": 0.667, "noise_scale_duration": 0.8, "speaking_rate": 1.0
        }"#;
        let c = VitsConfig::from_json(s).unwrap();
        assert_eq!(c.vocab_size, 95);
        assert_eq!(c.total_upsample(), 256);
        assert!(c.use_stochastic_duration_prediction);
        assert_eq!(c.sampling_rate, 16000);
    }
}
