//! Config types for group_manager module.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct GlobalAgentConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) allowed_work_dirs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct GroupConfigEntry {
    pub(super) jid: String,
    pub(super) folder: String,
    pub(super) name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) group_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) requires_trigger: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) allowed_paths: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) allowed_work_dirs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) bot_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) max_messages: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuAppConfig {
    #[serde(rename = "appSecret")]
    pub app_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QqAppConfig {
    #[serde(rename = "appSecret")]
    pub(super) app_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) sandbox: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WechatAccountConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramBotConfig {
    pub token: String,
    #[serde(rename = "adminUserId")]
    pub admin_user_id: String,
    pub folder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Whisper ASR UI settings: selected model id + default language.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WhisperSettings {
    #[serde(rename = "modelId", default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

/// TTS (Text-to-Speech) UI settings: selected model, voice preset, speed, language.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TtsSettings {
    /// HuggingFace model id of the selected TTS model.
    #[serde(rename = "modelId", default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    /// Voice preset (model-specific string, e.g. speaker id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voice: Option<String>,
    /// Playback speed multiplier (0.5– 2.0). `None` = model default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    /// Language code: `"vi"` | `"en"`. `None` = model default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Provider: "openai" | "openrouter" | "ollama" | "local" | "none"
    pub provider: String,
    #[serde(rename = "apiKey", default, skip_serializing_if = "String::is_empty")]
    pub api_key: String,
    #[serde(rename = "baseURL", default, skip_serializing_if = "String::is_empty")]
    pub base_url: String,
    #[serde(
        rename = "modelName",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub model_name: String,
    /// Local model path (only for provider="local")
    #[serde(
        rename = "modelPath",
        default,
        skip_serializing_if = "String::is_empty"
    )]
    pub model_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    pub id: String,
    pub label: String,
    pub provider: String,
    #[serde(rename = "baseURL")]
    pub base_url: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    #[serde(rename = "modelName")]
    pub model_name: String,
    /// "openai" or "anthropic"
    pub adapt: String,
    #[serde(rename = "maxTokens")]
    pub max_tokens: u32,
    #[serde(rename = "contextLength")]
    pub context_length: u32,
    /// Explicitly declare whether vision input is supported; undefined = auto-infer from modelName
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct AdminPermissionsSection {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "skipMainAgentPermissions"
    )]
    pub(super) skip_main_agent_permissions: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "skipAllAgentsPermissions"
    )]
    pub(super) skip_all_agents_permissions: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct GlobalConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) agents: Option<HashMap<String, GlobalAgentConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "adminPermissions"
    )]
    pub(super) admin_permissions: Option<AdminPermissionsSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) groups: Option<Vec<GroupConfigEntry>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "feishuApps"
    )]
    pub(super) feishu_apps: Option<HashMap<String, FeishuAppConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "qqApps")]
    pub(super) qq_apps: Option<HashMap<String, QqAppConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "wechatAccounts"
    )]
    pub(super) wechat_accounts: Option<HashMap<String, WechatAccountConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "telegramBots"
    )]
    pub(super) telegram_bots: Option<Vec<TelegramBotConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "llmConfigs"
    )]
    pub(super) llm_configs: Option<Vec<LlmConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "activeLlmConfigId"
    )]
    pub(super) active_llm_config_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "activeQuickLlmConfigId"
    )]
    pub(super) active_quick_llm_config_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "activeCognitiveLlmConfigId"
    )]
    pub(super) active_cognitive_llm_config_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "thinkingEnabled"
    )]
    pub(super) thinking_enabled: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "embeddingConfig"
    )]
    pub(super) embedding_config: Option<EmbeddingConfig>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "whisperConfig"
    )]
    pub(super) whisper_config: Option<WhisperSettings>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "ttsConfig"
    )]
    pub(super) tts_config: Option<TtsSettings>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "cognitiveConfig"
    )]
    pub(super) cognitive_config: Option<PersistedCognitiveConfig>,
}

/// Settings → Cognitive UI form. Maps 1:1 onto [`crate::config::CognitiveConfig`]
/// at boot via `apply_persisted_overrides`. All fields optional so older
/// config files keep working (missing fields fall back to env / defaults).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PersistedCognitiveConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "maxConcurrent"
    )]
    pub max_concurrent: Option<usize>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "maxOutputChars"
    )]
    pub max_output_chars: Option<usize>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "reflectMinChars"
    )]
    pub reflect_min_chars: Option<usize>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "reflectMaxChars"
    )]
    pub reflect_max_chars: Option<usize>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "reflectCooldownMs"
    )]
    pub reflect_cooldown_ms: Option<u64>,
    /// Toggle for `MemoryConfig.cognitive_reflection` — auto-cognify
    /// every user message. Off = manual CogAdd only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "autoReflection"
    )]
    pub auto_reflection: Option<bool>,
    /// Cadence (hours) for the periodic maintenance sweep that runs
    /// `cleanup_junk` + `merge_duplicate_entities`. `0` disables it.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "maintenanceIntervalHours"
    )]
    pub maintenance_interval_hours: Option<u64>,
}

/// Fields that can be updated on a [`GroupBinding`].
/// All fields are optional; `None` means "keep existing value".
#[derive(Debug, Clone, Default)]
pub struct GroupBindingUpdate {
    pub folder: Option<String>,
    pub name: Option<String>,
    pub channel: Option<String>,
    pub group_type: Option<String>,
    pub is_admin: Option<bool>,
    pub requires_trigger: Option<bool>,
    pub allowed_tools: Option<Option<Vec<String>>>,
    pub allowed_paths: Option<Option<Vec<String>>>,
    pub allowed_work_dirs: Option<Option<Vec<String>>>,
    pub bot_token: Option<Option<String>>,
    pub max_messages: Option<Option<u32>>,
}

#[derive(Debug, Clone)]
pub struct AdminPermissions {
    pub skip_main_agent_permissions: bool,
    pub skip_all_agents_permissions: bool,
}

pub struct LlmConfigResult {
    pub configs: Vec<LlmConfig>,
    pub active_id: Option<String>,
    pub active_quick_id: Option<String>,
    pub active_cognitive_id: Option<String>,
}
