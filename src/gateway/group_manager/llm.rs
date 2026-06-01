//! LLM config, thinking config, and admin permissions.

use std::path::Path;

use anyhow::Result;

use super::config::{load_global_config, save_global_config};
use super::types::{
    AdminPermissions, AdminPermissionsSection, EmbeddingConfig, LlmConfig, LlmConfigResult,
    TtsSettings, WhisperSettings,
};

// ===== Admin permissions config =====

pub fn get_admin_permissions_config(config_path: &Path) -> AdminPermissions {
    let cfg = load_global_config(config_path);
    let p = cfg.admin_permissions.unwrap_or_default();
    AdminPermissions {
        skip_main_agent_permissions: p.skip_main_agent_permissions.unwrap_or(false),
        skip_all_agents_permissions: p.skip_all_agents_permissions.unwrap_or(false),
    }
}

pub fn save_admin_permissions_config(config_path: &Path, opts: &AdminPermissions) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.admin_permissions = Some(AdminPermissionsSection {
        skip_main_agent_permissions: Some(opts.skip_main_agent_permissions),
        skip_all_agents_permissions: Some(opts.skip_all_agents_permissions),
    });
    save_global_config(config_path, &cfg)
}

// ===== Thinking config =====

pub fn get_thinking_enabled(config_path: &Path) -> bool {
    load_global_config(config_path)
        .thinking_enabled
        .unwrap_or(true)
}

pub fn save_thinking_enabled(config_path: &Path, enabled: bool) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.thinking_enabled = Some(enabled);
    save_global_config(config_path, &cfg)
}

// ===== LLM config =====

pub fn load_llm_configs(config_path: &Path) -> LlmConfigResult {
    let cfg = load_global_config(config_path);
    LlmConfigResult {
        configs: cfg.llm_configs.unwrap_or_default(),
        active_id: cfg.active_llm_config_id,
        active_quick_id: cfg.active_quick_llm_config_id,
        active_cognitive_id: cfg.active_cognitive_llm_config_id,
    }
}

pub fn save_llm_config(config_path: &Path, c: &LlmConfig) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    let configs = cfg.llm_configs.get_or_insert_with(Vec::new);
    if let Some(existing) = configs.iter_mut().find(|x| x.id == c.id) {
        *existing = c.clone();
    } else {
        configs.push(c.clone());
    }
    save_global_config(config_path, &cfg)
}

pub fn remove_llm_config(config_path: &Path, id: &str) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    if let Some(ref mut configs) = cfg.llm_configs {
        configs.retain(|x| x.id != id);
    }
    if cfg.active_llm_config_id.as_deref() == Some(id) {
        cfg.active_llm_config_id = None;
    }
    if cfg.active_quick_llm_config_id.as_deref() == Some(id) {
        cfg.active_quick_llm_config_id = None;
    }
    if cfg.active_cognitive_llm_config_id.as_deref() == Some(id) {
        cfg.active_cognitive_llm_config_id = None;
    }
    save_global_config(config_path, &cfg)
}

pub fn set_active_llm_config(config_path: &Path, id: Option<&str>) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.active_llm_config_id = id.map(|s| s.to_string());
    save_global_config(config_path, &cfg)
}

pub fn set_active_quick_llm_config(config_path: &Path, id: Option<&str>) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.active_quick_llm_config_id = id.map(|s| s.to_string());
    save_global_config(config_path, &cfg)
}

pub fn set_active_cognitive_llm_config(config_path: &Path, id: Option<&str>) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.active_cognitive_llm_config_id = id.map(|s| s.to_string());
    save_global_config(config_path, &cfg)
}
// ===== Embedding config =====

pub fn load_embedding_config(config_path: &Path) -> Option<EmbeddingConfig> {
    load_global_config(config_path).embedding_config
}

pub fn load_cognitive_config(config_path: &Path) -> Option<super::types::PersistedCognitiveConfig> {
    load_global_config(config_path).cognitive_config
}

pub fn save_cognitive_config(
    config_path: &Path,
    c: &super::types::PersistedCognitiveConfig,
) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.cognitive_config = Some(c.clone());
    save_global_config(config_path, &cfg)
}

pub fn save_embedding_config(config_path: &Path, c: &EmbeddingConfig) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.embedding_config = Some(c.clone());
    save_global_config(config_path, &cfg)
}

// ===== Whisper ASR settings =====

pub fn load_whisper_settings(config_path: &Path) -> WhisperSettings {
    load_global_config(config_path)
        .whisper_config
        .unwrap_or_default()
}

pub fn save_whisper_settings(config_path: &Path, s: &WhisperSettings) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.whisper_config = Some(s.clone());
    save_global_config(config_path, &cfg)
}

// ===== TTS settings =====

pub fn load_tts_settings(config_path: &Path) -> TtsSettings {
    load_global_config(config_path)
        .tts_config
        .unwrap_or_default()
}

pub fn save_tts_settings(config_path: &Path, s: &TtsSettings) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.tts_config = Some(s.clone());
    save_global_config(config_path, &cfg)
}
