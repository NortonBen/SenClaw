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
        rename = "thinkingEnabled"
    )]
    pub(super) thinking_enabled: Option<bool>,
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
}
