//! ZipVoice `config.json` parsing.
//!
//! Mirrors the `mlx-community/zipvoice-*` checkpoint layout (see the original
//! `k2-fsa/ZipVoice` config). The model is a flow-matching TTS: a Zipformer2
//! **text encoder** conditions a U-Net Zipformer **flow-matching decoder**
//! (`fm_decoder`) that predicts a `feat_dim`-channel mel, later turned into a
//! waveform by a *separate* Vocos vocoder (not part of this checkpoint).
//!
//! Pure-Rust / dependency-free so it is unit-testable without the MLX stack.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// `feature` block — describes the acoustic feature + target sample rate.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct FeatureConfig {
    pub sampling_rate: u32,
    /// Vocoder family the mel targets, e.g. `"vocos"`.
    #[serde(rename = "type")]
    pub feature_type: String,
}

/// `model` block — Zipformer2 + flow-matching hyperparameters.
///
/// `fm_decoder_*` list fields are per-stage (the U-Net has 5 stages); the
/// `text_encoder_*` fields describe its single Zipformer stack.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ModelConfig {
    pub fm_decoder_downsampling_factor: Vec<i32>,
    pub fm_decoder_num_layers: Vec<i32>,
    pub fm_decoder_cnn_module_kernel: Vec<i32>,
    pub fm_decoder_feedforward_dim: i32,
    pub fm_decoder_num_heads: i32,
    pub fm_decoder_dim: i32,

    pub text_encoder_num_layers: i32,
    pub text_encoder_feedforward_dim: i32,
    pub text_encoder_cnn_module_kernel: i32,
    pub text_encoder_num_heads: i32,
    pub text_encoder_dim: i32,

    pub query_head_dim: i32,
    pub value_head_dim: i32,
    pub pos_head_dim: i32,
    pub pos_dim: i32,
    pub time_embed_dim: i32,
    pub text_embed_dim: i32,
    pub feat_dim: i32,
}

/// Top-level ZipVoice config.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct ZipVoiceConfig {
    pub feature: FeatureConfig,
    pub model: ModelConfig,
    #[serde(default)]
    pub model_type: String,
}

impl ZipVoiceConfig {
    /// Parse a `config.json` string.
    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).context("parsing ZipVoice config.json")
    }

    /// Load `<dir>/config.json`.
    pub fn from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let path = dir.as_ref().join("config.json");
        let s = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        Self::from_json(&s)
    }

    /// Number of U-Net stages in the flow-matching decoder.
    pub fn num_fm_stages(&self) -> usize {
        self.model.fm_decoder_downsampling_factor.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact `config.json` shipped with `mlx-community/zipvoice-vietnamese`.
    const SAMPLE: &str = r#"{
        "feature": { "sampling_rate": 24000, "type": "vocos" },
        "model": {
            "fm_decoder_downsampling_factor": [1, 2, 4, 2, 1],
            "fm_decoder_num_layers": [2, 2, 4, 4, 4],
            "fm_decoder_cnn_module_kernel": [31, 15, 7, 15, 31],
            "fm_decoder_feedforward_dim": 1536,
            "fm_decoder_num_heads": 4,
            "fm_decoder_dim": 512,
            "text_encoder_num_layers": 4,
            "text_encoder_feedforward_dim": 512,
            "text_encoder_cnn_module_kernel": 9,
            "text_encoder_num_heads": 4,
            "text_encoder_dim": 192,
            "query_head_dim": 32,
            "value_head_dim": 12,
            "pos_head_dim": 4,
            "pos_dim": 48,
            "time_embed_dim": 192,
            "text_embed_dim": 192,
            "feat_dim": 100
        },
        "model_type": "zipvoice"
    }"#;

    #[test]
    fn parses_sample_config() {
        let c = ZipVoiceConfig::from_json(SAMPLE).expect("parse");
        assert_eq!(c.feature.sampling_rate, 24000);
        assert_eq!(c.feature.feature_type, "vocos");
        assert_eq!(c.model_type, "zipvoice");
        assert_eq!(c.model.feat_dim, 100);
        assert_eq!(c.model.fm_decoder_dim, 512);
        assert_eq!(c.model.text_encoder_dim, 192);
        assert_eq!(
            c.model.fm_decoder_downsampling_factor,
            vec![1, 2, 4, 2, 1]
        );
        assert_eq!(c.model.fm_decoder_num_layers, vec![2, 2, 4, 4, 4]);
        assert_eq!(c.num_fm_stages(), 5);
        // The 5 U-Net stages must agree across all per-stage lists.
        assert_eq!(c.model.fm_decoder_cnn_module_kernel.len(), c.num_fm_stages());
        assert_eq!(c.model.fm_decoder_num_layers.len(), c.num_fm_stages());
    }

    #[test]
    fn rejects_garbage() {
        assert!(ZipVoiceConfig::from_json("{ not json }").is_err());
    }
}
