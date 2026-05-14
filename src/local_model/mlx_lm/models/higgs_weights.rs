//! Safetensors weight loading for vendored Higgs-style MLX checkpoints.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use mlx_rs::module::ModuleParametersExt;
use mlx_rs::Array;
use serde::Deserialize;
use serde_json::Value;

use crate::local_model::higgs_error::ModelError;

/// Weight map index from `model.safetensors.index.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct WeightMapIndex {
    pub metadata: HashMap<String, Value>,
    pub weight_map: HashMap<String, String>,
}

/// Load a tokenizer from a model directory.
pub fn load_tokenizer<P: AsRef<Path>>(model_dir: P) -> Result<tokenizers::Tokenizer, ModelError> {
    let file = model_dir.as_ref().join("tokenizer.json");
    tokenizers::Tokenizer::from_file(file).map_err(|e| ModelError::Io(std::io::Error::other(e.to_string())))
}

/// Load safetensors weights into any model that implements `ModuleParametersExt`.
pub fn load_safetensors_weights<M: ModuleParametersExt>(
    model: &mut M,
    model_path: &Path,
) -> Result<(), ModelError> {
    load_quantized_safetensors_weights(model, model_path, false)
}

/// Load safetensors with optional `.weight` → `.inner.weight` remap for quantized checkpoints.
pub fn load_quantized_safetensors_weights<M: ModuleParametersExt>(
    model: &mut M,
    model_path: &Path,
    quantized: bool,
) -> Result<(), ModelError> {
    let safetensors_files = collect_safetensors_files(model_path)?;
    let mut params = model.parameters_mut().flatten();

    for file_path in &safetensors_files {
        tracing::debug!(file = %file_path.display(), "Loading weights");
        let loaded = Array::load_safetensors(file_path)
            .map_err(|e| ModelError::Io(std::io::Error::other(e.to_string())))?;

        for (key, value) in loaded {
            if let Some(param) = params.get_mut(&*key) {
                **param = value;
            } else if quantized {
                if let Some(remapped) = remap_quantized_key(&key) {
                    if let Some(param) = params.get_mut(&*remapped) {
                        **param = value;
                    } else {
                        tracing::warn!(key = %key, remapped = %remapped, "Weight key remapped but target parameter not found");
                    }
                } else {
                    tracing::warn!(key = %key, "Weight key not found in model parameters (quantized remap failed)");
                }
            } else {
                tracing::warn!(key = %key, "Weight key not found in model parameters");
            }
        }
    }

    model
        .eval()
        .map_err(|e| ModelError::Io(std::io::Error::other(e.to_string())))?;

    Ok(())
}

/// Like [`load_quantized_safetensors_weights`] but only keys starting with `prefix` (VLMs).
pub fn load_quantized_safetensors_weights_with_prefix<M: ModuleParametersExt>(
    model: &mut M,
    model_path: &Path,
    quantized: bool,
    prefix: &str,
) -> Result<(), ModelError> {
    const MAX_UNMATCHED_WARNS: usize = 5;
    let safetensors_files = collect_safetensors_files(model_path)?;
    let mut params = model.parameters_mut().flatten();
    let mut total_unmatched_warns = 0usize;

    for file_path in &safetensors_files {
        tracing::debug!(file = %file_path.display(), prefix, "Loading weights with prefix");
        let loaded = Array::load_safetensors(file_path)
            .map_err(|e| ModelError::Io(std::io::Error::other(e.to_string())))?;

        let mut matched = 0usize;
        let mut unmatched = 0usize;
        for (key, value) in loaded {
            let Some(stripped) = key.strip_prefix(prefix) else {
                continue;
            };
            if let Some(param) = params.get_mut(stripped) {
                **param = value;
                matched += 1;
            } else if quantized {
                if let Some(remapped) = remap_quantized_key(stripped) {
                    if let Some(param) = params.get_mut(&*remapped) {
                        **param = value;
                        matched += 1;
                    } else {
                        unmatched += 1;
                        total_unmatched_warns += 1;
                        if total_unmatched_warns <= MAX_UNMATCHED_WARNS {
                            tracing::debug!(
                                stripped,
                                remapped = &*remapped,
                                "weight key unmatched after remap"
                            );
                        }
                    }
                } else {
                    unmatched += 1;
                    total_unmatched_warns += 1;
                    if total_unmatched_warns <= MAX_UNMATCHED_WARNS {
                        tracing::debug!(stripped, "weight key unmatched (no remap)");
                    }
                }
            } else {
                unmatched += 1;
                total_unmatched_warns += 1;
                if total_unmatched_warns <= MAX_UNMATCHED_WARNS {
                    tracing::debug!(stripped, "weight key unmatched");
                }
            }
        }
        tracing::info!(matched, unmatched, "Weight loading stats for shard");
    }

    model
        .eval()
        .map_err(|e| ModelError::Io(std::io::Error::other(e.to_string())))?;

    Ok(())
}

pub fn collect_safetensors_files(model_path: &Path) -> Result<Vec<std::path::PathBuf>, ModelError> {
    let index_path = model_path.join("model.safetensors.index.json");
    if index_path.exists() {
        let json = std::fs::read_to_string(&index_path)?;
        let index: WeightMapIndex = serde_json::from_str(&json)?;
        let weight_files: HashSet<&String> = index.weight_map.values().collect();
        let mut files: Vec<_> = weight_files.into_iter().map(|f| model_path.join(f)).collect();
        files.sort();
        Ok(files)
    } else {
        let single_path = model_path.join("model.safetensors");
        if single_path.exists() {
            Ok(vec![single_path])
        } else {
            Err(ModelError::MissingWeight(
                "No safetensors files found".to_owned(),
            ))
        }
    }
}

#[allow(clippy::case_sensitive_file_extension_comparisons)]
fn remap_quantized_key(key: &str) -> Option<String> {
    if let Some(prefix) = key.strip_suffix(".weight") {
        Some(format!("{prefix}.inner.weight"))
    } else if let Some(prefix) = key.strip_suffix(".scales") {
        Some(format!("{prefix}.inner.scales"))
    } else if let Some(prefix) = key.strip_suffix(".biases") {
        Some(format!("{prefix}.inner.biases"))
    } else if key.ends_with(".bias") {
        let prefix = key.strip_suffix(".bias")?;
        Some(format!("{prefix}.inner.bias"))
    } else {
        None
    }
}
