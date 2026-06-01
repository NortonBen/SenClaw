//! Group binding registry, directory management, and global config persistence.
//! Mirrors `src-old/gateway/GroupManager.ts`.

pub mod apps;
pub mod chat;
pub(crate) mod config;
pub mod dirs;
pub mod llm;
pub mod manager;
pub(crate) mod soul;
#[cfg(test)]
mod tests;
pub mod types;

// Re-exports for external consumers
pub use apps::{
    delete_feishu_app, delete_qq_app, get_feishu_apps, get_qq_apps, save_feishu_app, save_qq_app,
};
pub use chat::{delete_telegram_bot, get_telegram_bots, get_wechat_accounts, save_telegram_bot};
pub use dirs::{ensure_agent_dirs, write_soul_md};
pub use llm::{
    get_admin_permissions_config, get_thinking_enabled, load_embedding_config, load_llm_configs,
    remove_llm_config, save_admin_permissions_config, save_embedding_config, save_llm_config,
    load_cognitive_config, save_cognitive_config, save_thinking_enabled,
    set_active_cognitive_llm_config, set_active_llm_config, set_active_quick_llm_config,
    load_whisper_settings, save_whisper_settings,
};
pub use manager::{ensure_admin_group, ensure_app_group, ensure_wechat_admin_group, GroupManager};
pub use types::{
    AdminPermissions, EmbeddingConfig, FeishuAppConfig, GroupBindingUpdate, LlmConfig,
    LlmConfigResult, PersistedCognitiveConfig, QqAppConfig, TelegramBotConfig,
    WechatAccountConfig, WhisperSettings,
};

pub use config::{get_agent_allowed_work_dirs, sync_groups_from_config};
