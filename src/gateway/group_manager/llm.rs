//! LLM config, thinking config, and admin permissions.

use std::path::Path;

use anyhow::Result;

use super::config::{load_global_config, save_global_config};
use super::types::{
    AdminPermissions, AdminPermissionsSection, DefaultsConfig, EmbeddingConfig, LlmConfig,
    LlmConfigResult, OcrSettings, TtsSettings, WhisperSettings,
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

// ===== Pre-process stage toggles (global, user-set) =====

/// Pre-trigger-skill stage. Default OFF — opt-in deterministic skill force-load.
pub fn get_pre_trigger_skill_enabled(config_path: &Path) -> bool {
    load_global_config(config_path)
        .pre_trigger_skill
        .unwrap_or(false)
}

pub fn save_pre_trigger_skill_enabled(config_path: &Path, enabled: bool) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.pre_trigger_skill = Some(enabled);
    save_global_config(config_path, &cfg)
}

/// Pre-cognitive stage. Default OFF — opt-in cognitive-memory injection.
pub fn get_pre_cognitive_enabled(config_path: &Path) -> bool {
    load_global_config(config_path)
        .pre_cognitive
        .unwrap_or(false)
}

pub fn save_pre_cognitive_enabled(config_path: &Path, enabled: bool) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.pre_cognitive = Some(enabled);
    save_global_config(config_path, &cfg)
}

// ===== After-process stage toggle (global, user-set) =====

/// After-process / context-update stage. Default OFF — opt-in.
///
/// When enabled, after the main agent turn completes the conversation is
/// summarised and context is updated (Claude-Code style) so the agent retains
/// a compact, optimised understanding of the whole dialogue for future turns.
pub fn get_after_process_enabled(config_path: &Path) -> bool {
    load_global_config(config_path)
        .after_process
        .unwrap_or(false)
}

pub fn save_after_process_enabled(config_path: &Path, enabled: bool) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.after_process = Some(enabled);
    save_global_config(config_path, &cfg)
}

// ===== Curated-memory stage toggle (global, user-set) =====

/// Curated-memory stage. Default OFF — opt-in.
///
/// When enabled: (a) history dropped by compaction is consolidated into
/// curated `memory/*.md` files, and (b) each request injects relevant curated
/// memories found via hybrid FTS5/vector search (Claude-Code-style auto-memory).
pub fn get_memory_recall_enabled(config_path: &Path) -> bool {
    load_global_config(config_path)
        .memory_recall
        .unwrap_or(false)
}

pub fn save_memory_recall_enabled(config_path: &Path, enabled: bool) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.memory_recall = Some(enabled);
    save_global_config(config_path, &cfg)
}

// ===== Default flows + widget disable list (global, user-set) =====

/// The `defaults` section (open-link / media / search / note handlers + the
/// per-widget disable list). Absent section → all-`None` config whose
/// `effective_*()` fallbacks reproduce today's behavior exactly.
pub fn get_defaults_config(config_path: &Path) -> DefaultsConfig {
    load_global_config(config_path).defaults.unwrap_or_default()
}

/// Merge-save: only `Some` fields in `patch` replace the stored values, so the
/// UI can update one dropdown without resending the rest. Returns the merged
/// result.
pub fn save_defaults_config(config_path: &Path, patch: &DefaultsConfig) -> Result<DefaultsConfig> {
    let mut cfg = load_global_config(config_path);
    let mut current = cfg.defaults.take().unwrap_or_default();
    if patch.open_link.is_some() {
        current.open_link = patch.open_link.clone();
    }
    if patch.media.is_some() {
        current.media = patch.media.clone();
    }
    if patch.search.is_some() {
        current.search = patch.search.clone();
    }
    if patch.search_engine.is_some() {
        current.search_engine = patch.search_engine.clone();
    }
    if patch.note.is_some() {
        current.note = patch.note.clone();
    }
    if patch.disabled_widgets.is_some() {
        current.disabled_widgets = patch.disabled_widgets.clone();
    }
    cfg.defaults = Some(current.clone());
    save_global_config(config_path, &cfg)?;
    Ok(current)
}

/// Toggle one widget id in `defaults.disabledWidgets`.
pub fn set_widget_disabled(config_path: &Path, widget_id: &str, disabled: bool) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    let mut current = cfg.defaults.take().unwrap_or_default();
    let mut list = current.disabled_widgets.take().unwrap_or_default();
    list.retain(|id| id != widget_id);
    if disabled {
        list.push(widget_id.to_string());
    }
    current.disabled_widgets = Some(list);
    cfg.defaults = Some(current);
    save_global_config(config_path, &cfg)
}

// ===== MCP dispatcher toggle (global, user-set) =====

/// Autonomous MCP dispatcher. Default OFF — opt-in autonomous task execution:
/// when enabled, ready tasks on dispatch sources (the Kanban board) are picked
/// up and run by persona worker agents.
pub fn get_dispatch_enabled(config_path: &Path) -> bool {
    load_global_config(config_path)
        .dispatch_enabled
        .unwrap_or(false)
}

pub fn save_dispatch_enabled(config_path: &Path, enabled: bool) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.dispatch_enabled = Some(enabled);
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

// ===== OCR settings =====

pub fn load_ocr_settings(config_path: &Path) -> OcrSettings {
    load_global_config(config_path)
        .ocr_config
        .unwrap_or_default()
}

pub fn save_ocr_settings(config_path: &Path, s: &OcrSettings) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    cfg.ocr_config = Some(s.clone());
    save_global_config(config_path, &cfg)
}
