use std::path::Path;

use super::higgs_error::ModelError;

/// Maximum config.json size (10 MiB) — prevents `DoS` from symlinked large files.
const MAX_CONFIG_SIZE: u64 = 10 * 1024 * 1024;

/// Detect the model architecture from config.json's `model_type` field.
pub fn detect_model_type<P: AsRef<Path>>(model_dir: P) -> Result<String, ModelError> {
    let config_path = model_dir.as_ref().join("config.json");
    let file = std::fs::File::open(&config_path)?;
    let file_size = file.metadata()?.len();
    if file_size > MAX_CONFIG_SIZE {
        return Err(ModelError::UnsupportedModel(format!(
            "config.json too large ({file_size} bytes, max {MAX_CONFIG_SIZE})"
        )));
    }
    let config: serde_json::Value = serde_json::from_reader(file)?;

    config
        .get("model_type")
        .and_then(|v| v.as_str())
        .map(ToOwned::to_owned)
        .ok_or_else(|| ModelError::UnsupportedModel("missing model_type in config.json".into()))
}

/// Supported model architectures.
pub fn is_supported(model_type: &str) -> bool {
    matches!(
        model_type,
        "qwen2"
            | "qwen3"
            | "llama"
            | "mistral"
            | "qwen3_next"
            | "qwen3_moe"
            | "qwen3_5"
            | "qwen3_5_moe"
            | "gemma2"
            | "phi3"
            | "starcoder2"
            | "llava-qwen2"
            | "deepseek_v2"
    )
}
