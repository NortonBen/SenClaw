//! Group binding registry, directory management, and global config persistence.
//! Mirrors `src-old/gateway/GroupManager.ts`.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::db::Db;
use crate::types::GroupBinding;

// ===== Config types =====

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct GlobalAgentConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowed_work_dirs: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupConfigEntry {
    jid: String,
    folder: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    requires_trigger: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    allowed_work_dirs: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bot_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    max_messages: Option<u32>,
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
    app_secret: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sandbox: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WechatAccountConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    name: Option<String>,
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
struct AdminPermissionsSection {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "skipMainAgentPermissions"
    )]
    skip_main_agent_permissions: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "skipAllAgentsPermissions"
    )]
    skip_all_agents_permissions: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct GlobalConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agents: Option<HashMap<String, GlobalAgentConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "adminPermissions"
    )]
    admin_permissions: Option<AdminPermissionsSection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    groups: Option<Vec<GroupConfigEntry>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "feishuApps"
    )]
    feishu_apps: Option<HashMap<String, FeishuAppConfig>>,
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "qqApps")]
    qq_apps: Option<HashMap<String, QqAppConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "wechatAccounts"
    )]
    wechat_accounts: Option<HashMap<String, WechatAccountConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "telegramBots"
    )]
    telegram_bots: Option<Vec<TelegramBotConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "llmConfigs"
    )]
    llm_configs: Option<Vec<LlmConfig>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "activeLlmConfigId"
    )]
    active_llm_config_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "activeQuickLlmConfigId"
    )]
    active_quick_llm_config_id: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "thinkingEnabled"
    )]
    thinking_enabled: Option<bool>,
}

// ===== Soul template =====

fn default_soul_md(folder: &str, name: &str) -> String {
    format!(
        r#"# {name}

You are a helpful AI assistant.

## Identity

Your agent ID is `{folder}`.
Your memory is stored in `memory/` within your agent directory.

## Guidelines

- Be helpful, concise, and friendly
- Respond in the language the user is using
- Keep responses focused and actionable

## Memory Management

Before answering, check `MEMORY.md` in your memory directory for relevant context.
After important interactions, update your memory with key information.

## Working Directory

Your default workspace is `~/senclaw/workspace/{folder}/`.
When the user mentions working on a specific project at a particular path,
use the WorkspaceTool to switch to that directory.
Return to your default workspace when the task is complete or the topic changes.
"#
    )
}

// ===== Directory management =====

pub fn ensure_agent_dirs(config: &Config, folder: &str, name: &str) -> (String, String) {
    let agent_data_dir = config.paths.agents_dir.join(folder);
    let workspace_dir = config.paths.workspace_dir.join(folder);

    fs::create_dir_all(agent_data_dir.join("memory")).ok();
    fs::create_dir_all(agent_data_dir.join(".sema").join("sessions")).ok();

    let soul_md = agent_data_dir.join("SOUL.md");
    if !soul_md.exists() {
        fs::write(&soul_md, default_soul_md(folder, name)).ok();
    }

    let memory_md = agent_data_dir.join("MEMORY.md");
    if !memory_md.exists() {
        fs::write(&memory_md, "# Memory\n\n").ok();
    }

    fs::create_dir_all(&workspace_dir).ok();

    (
        agent_data_dir.to_string_lossy().into_owned(),
        workspace_dir.to_string_lossy().into_owned(),
    )
}

// ===== Global config load/save =====

fn load_global_config(path: &Path) -> GlobalConfig {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => GlobalConfig::default(),
    }
}

fn save_global_config(path: &Path, cfg: &GlobalConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create dir {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(cfg)?;
    fs::write(path, json)?;
    Ok(())
}

// ===== Feishu app config =====

pub fn save_feishu_app(
    config_path: &Path,
    app_id: &str,
    app_secret: &str,
    domain: Option<&str>,
) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    let apps = cfg.feishu_apps.get_or_insert_with(HashMap::new);
    apps.insert(
        app_id.to_string(),
        FeishuAppConfig {
            app_secret: app_secret.to_string(),
            domain: domain.map(|s| s.to_string()),
        },
    );
    save_global_config(config_path, &cfg)
}

pub fn delete_feishu_app(config_path: &Path, app_id: &str) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    if let Some(ref mut apps) = cfg.feishu_apps {
        apps.remove(app_id);
    }
    save_global_config(config_path, &cfg)
}

pub fn get_feishu_apps(config_path: &Path) -> HashMap<String, FeishuAppConfig> {
    load_global_config(config_path)
        .feishu_apps
        .unwrap_or_default()
}

// ===== QQ multi-app config =====

pub fn save_qq_app(
    config_path: &Path,
    app_id: &str,
    app_secret: &str,
    sandbox: Option<bool>,
) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    let apps = cfg.qq_apps.get_or_insert_with(HashMap::new);
    apps.insert(
        app_id.to_string(),
        QqAppConfig {
            app_secret: app_secret.to_string(),
            sandbox,
        },
    );
    save_global_config(config_path, &cfg)
}

pub fn delete_qq_app(config_path: &Path, app_id: &str) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    if let Some(ref mut apps) = cfg.qq_apps {
        apps.remove(app_id);
    }
    save_global_config(config_path, &cfg)
}

pub fn get_qq_apps(config_path: &Path) -> HashMap<String, QqAppConfig> {
    load_global_config(config_path)
        .qq_apps
        .unwrap_or_default()
}

// ===== WeChat accounts =====

pub fn get_wechat_accounts(config_path: &Path) -> HashMap<String, WechatAccountConfig> {
    load_global_config(config_path)
        .wechat_accounts
        .unwrap_or_default()
}

// ===== Telegram multi-bot config =====

pub fn save_telegram_bot(config_path: &Path, entry: TelegramBotConfig) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    let bots = cfg.telegram_bots.get_or_insert_with(Vec::new);
    if let Some(existing) = bots.iter_mut().find(|b| b.token == entry.token) {
        *existing = entry;
    } else {
        bots.push(entry);
    }
    save_global_config(config_path, &cfg)
}

pub fn delete_telegram_bot(config_path: &Path, token: &str) -> Result<()> {
    let mut cfg = load_global_config(config_path);
    if let Some(ref mut bots) = cfg.telegram_bots {
        bots.retain(|b| b.token != token);
    }
    save_global_config(config_path, &cfg)
}

pub fn get_telegram_bots(config_path: &Path) -> Vec<TelegramBotConfig> {
    load_global_config(config_path)
        .telegram_bots
        .unwrap_or_default()
}

// ===== Group config persistence =====

fn save_group_to_config(config_path: &Path, binding: &GroupBinding) {
    if binding.is_admin {
        return;
    }
    let mut cfg = load_global_config(config_path);
    let groups = cfg.groups.get_or_insert_with(Vec::new);
    let entry = GroupConfigEntry {
        jid: binding.jid.clone(),
        folder: binding.folder.clone(),
        name: binding.name.clone(),
        channel: Some(binding.channel.clone()).filter(|c| !c.is_empty()),
        requires_trigger: Some(binding.requires_trigger),
        allowed_tools: binding.allowed_tools.clone(),
        allowed_work_dirs: binding.allowed_work_dirs.clone(),
        bot_token: binding.bot_token.clone(),
        max_messages: binding.max_messages,
    };
    if let Some(existing) = groups.iter_mut().find(|g| g.jid == entry.jid) {
        *existing = entry;
    } else {
        groups.push(entry);
    }
    let _ = save_global_config(config_path, &cfg);
}

fn remove_group_from_config(config_path: &Path, jid: &str) {
    let mut cfg = load_global_config(config_path);
    if let Some(ref mut groups) = cfg.groups {
        groups.retain(|g| g.jid != jid);
    }
    let _ = save_global_config(config_path, &cfg);
}

pub fn sync_groups_from_config(
    db: &Db,
    gm: &GroupManager,
    config: &Config,
) -> (usize, usize, usize) {
    let cfg = load_global_config(&config.paths.global_config_path);
    let config_groups = cfg.groups.unwrap_or_default();
    let config_jids: std::collections::HashSet<&str> =
        config_groups.iter().map(|g| g.jid.as_str()).collect();
    let now = chrono_now();
    let mut added = 0usize;
    let mut updated = 0usize;
    let mut removed = 0usize;

    for entry in &config_groups {
        // Prevent folder UNIQUE conflicts
        if let Ok(all) = db.list_groups() {
            if let Some(_conflict) = all
                .iter()
                .find(|g| g.folder == entry.folder && g.jid != entry.jid)
            {
                let _ = db.delete_group_by_folder(&entry.folder);
            }
        }

        let existing = gm.get(db, &entry.jid);
        let binding = GroupBinding {
            jid: entry.jid.clone(),
            folder: entry.folder.clone(),
            name: entry.name.clone(),
            channel: entry.channel.clone().unwrap_or_default(),
            is_admin: false,
            requires_trigger: entry.requires_trigger.unwrap_or(true),
            allowed_tools: entry.allowed_tools.clone(),
            allowed_paths: None,
            allowed_work_dirs: entry.allowed_work_dirs.clone(),
            bot_token: entry.bot_token.clone(),
            max_messages: entry.max_messages,
            last_active: existing.as_ref().and_then(|e| e.last_active.clone()),
            added_at: existing
                .as_ref()
                .map(|e| e.added_at.clone())
                .unwrap_or_else(|| now.clone()),
        };

        ensure_agent_dirs(config, &binding.folder, &binding.name);
        let _ = db.upsert_group(&binding);
        if existing.is_some() {
            updated += 1;
        } else {
            added += 1;
        }
    }

    // Delete non-admin DB groups not in config
    if let Ok(all) = gm.list(db) {
        for db_group in &all {
            if db_group.is_admin {
                continue;
            }
            if !config_jids.contains(db_group.jid.as_str()) {
                let _ = db.delete_group(&db_group.jid);
                removed += 1;
            }
        }
    }

    (added, updated, removed)
}

pub fn get_agent_allowed_work_dirs(
    config_path: &Path,
    folder: &str,
) -> Option<Option<Vec<String>>> {
    let cfg = load_global_config(config_path);
    match cfg.agents.and_then(|a| a.get(folder).cloned()) {
        None => None,                             // not present in config
        Some(entry) => Some(entry.allowed_work_dirs), // null = switching disallowed
    }
}

// ===== Admin permissions config =====

#[derive(Debug, Clone)]
pub struct AdminPermissions {
    pub skip_main_agent_permissions: bool,
    pub skip_all_agents_permissions: bool,
}

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

pub struct LlmConfigResult {
    pub configs: Vec<LlmConfig>,
    pub active_id: Option<String>,
    pub active_quick_id: Option<String>,
}

pub fn load_llm_configs(config_path: &Path) -> LlmConfigResult {
    let cfg = load_global_config(config_path);
    LlmConfigResult {
        configs: cfg.llm_configs.unwrap_or_default(),
        active_id: cfg.active_llm_config_id,
        active_quick_id: cfg.active_quick_llm_config_id,
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

// ===== GroupManager =====

pub struct GroupManager {
    on_groups_changed: Mutex<Option<Box<dyn Fn() + Send + 'static>>>,
}

impl GroupManager {
    pub fn new() -> Self {
        Self {
            on_groups_changed: Mutex::new(None),
        }
    }

    pub fn set_on_groups_changed(&self, cb: Box<dyn Fn() + Send + 'static>) {
        if let Ok(mut guard) = self.on_groups_changed.lock() {
            *guard = Some(cb);
        }
    }

    pub fn register(&self, db: &Db, config: &Config, binding: &GroupBinding) {
        ensure_agent_dirs(config, &binding.folder, &binding.name);
        let _ = db.upsert_group(binding);
        save_group_to_config(&config.paths.global_config_path, binding);
        self.fire_changed();
    }

    pub fn unregister(&self, db: &Db, config: &Config, jid: &str) {
        let _ = db.delete_group(jid);
        remove_group_from_config(&config.paths.global_config_path, jid);
        self.fire_changed();
    }

    pub fn update(
        &self,
        db: &Db,
        config: &Config,
        jid: &str,
        updates: GroupBindingUpdate,
    ) -> Result<GroupBinding> {
        let existing = db
            .get_group(jid)?
            .ok_or_else(|| anyhow::anyhow!("Group not found: {jid}"))?;
        let updated = GroupBinding {
            jid: existing.jid.clone(),
            folder: updates.folder.unwrap_or(existing.folder),
            name: updates.name.unwrap_or(existing.name),
            channel: updates.channel.unwrap_or(existing.channel),
            is_admin: updates.is_admin.unwrap_or(existing.is_admin),
            requires_trigger: updates.requires_trigger.unwrap_or(existing.requires_trigger),
            allowed_tools: merge_opt_opt(updates.allowed_tools, existing.allowed_tools),
            allowed_paths: merge_opt_opt(updates.allowed_paths, existing.allowed_paths),
            allowed_work_dirs: merge_opt_opt(updates.allowed_work_dirs, existing.allowed_work_dirs),
            bot_token: merge_opt_opt(updates.bot_token, existing.bot_token),
            max_messages: merge_opt_opt(updates.max_messages, existing.max_messages),
            last_active: existing.last_active,
            added_at: existing.added_at,
        };
        db.upsert_group(&updated)?;
        save_group_to_config(&config.paths.global_config_path, &updated);
        self.fire_changed();
        Ok(updated)
    }

    pub fn get(&self, db: &Db, jid: &str) -> Option<GroupBinding> {
        db.get_group(jid).ok().flatten()
    }

    pub fn list(&self, db: &Db) -> Result<Vec<GroupBinding>> {
        db.list_groups()
    }

    pub fn touch_active(&self, db: &Db, jid: &str, timestamp: &str) {
        let _ = db.touch_group_active(jid, timestamp);
    }

    pub fn find_pending_feishu_binding(&self, db: &Db, app_id: &str) -> Option<GroupBinding> {
        if app_id.is_empty() {
            return None;
        }
        db.get_group(&format!("feishu:pending:{app_id}")).ok().flatten()
    }

    pub fn find_pending_qq_binding(&self, db: &Db, app_id: &str) -> Option<GroupBinding> {
        if app_id.is_empty() {
            return None;
        }
        db.get_group(&format!("qq:pending:{app_id}")).ok().flatten()
    }

    pub fn find_pending_wechat_binding(&self, db: &Db, folder: &str) -> Option<GroupBinding> {
        if folder.is_empty() {
            return None;
        }
        db.get_group(&format!("wx:pending:{folder}")).ok().flatten()
    }

    pub fn migrate_jid(
        &self,
        db: &Db,
        config_path: &Path,
        old_jid: &str,
        new_jid: &str,
    ) -> Option<GroupBinding> {
        let new_binding = db.rename_group_jid(old_jid, new_jid).ok().flatten()?;
        remove_group_from_config(config_path, old_jid);
        save_group_to_config(config_path, &new_binding);
        self.fire_changed();
        Some(new_binding)
    }

    pub fn get_extra_bot_tokens(&self, db: &Db, default_token: &str) -> Vec<String> {
        let mut tokens = std::collections::HashSet::new();
        if let Ok(all) = db.list_groups() {
            for g in &all {
                if let Some(ref tok) = g.bot_token {
                    if tok != default_token {
                        tokens.insert(tok.clone());
                    }
                }
            }
        }
        tokens.into_iter().collect()
    }

    pub fn get_channel_for_jid(jid: &str) -> Option<&'static str> {
        if jid.starts_with("tg:") {
            Some("telegram")
        } else if jid.starts_with("feishu:") {
            Some("feishu")
        } else if jid.starts_with("qq:") {
            Some("qq")
        } else if jid.starts_with("wx:") {
            Some("wechat")
        } else if jid.ends_with("@s.whatsapp.net") || jid.ends_with("@g.us") {
            Some("whatsapp")
        } else {
            None
        }
    }

    fn fire_changed(&self) {
        if let Ok(guard) = self.on_groups_changed.lock() {
            if let Some(ref cb) = *guard {
                cb();
            }
        }
    }
}

impl Default for GroupManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Fields that can be updated on a [`GroupBinding`].
/// All fields are optional; `None` means "keep existing value".
#[derive(Debug, Clone, Default)]
pub struct GroupBindingUpdate {
    pub folder: Option<String>,
    pub name: Option<String>,
    pub channel: Option<String>,
    pub is_admin: Option<bool>,
    pub requires_trigger: Option<bool>,
    pub allowed_tools: Option<Option<Vec<String>>>,
    pub allowed_paths: Option<Option<Vec<String>>>,
    pub allowed_work_dirs: Option<Option<Vec<String>>>,
    pub bot_token: Option<Option<String>>,
    pub max_messages: Option<Option<u32>>,
}

// ===== Helpers =====

/// Merge an `Option<Option<T>>` update with an existing `Option<T>`.
/// - `None` = don't update (keep existing)
/// - `Some(None)` = set to null
/// - `Some(Some(v))` = set to value
fn merge_opt_opt<T>(update: Option<Option<T>>, existing: Option<T>) -> Option<T> {
    match update {
        None => existing,
        Some(v) => v,
    }
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format_unix_timestamp(now.as_secs())
}

fn format_unix_timestamp(secs: u64) -> String {
    let days_since_epoch = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_ymd(days_since_epoch as i64);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.000Z")
}

fn days_to_ymd(mut days: i64) -> (i64, u32, u32) {
    days += 719468;
    let era = if days >= 0 { days } else { days - 146096 } / 146097;
    let doe = (days - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

// ===== ensure_admin_group =====

pub fn ensure_admin_group(
    db: &Db,
    gm: &GroupManager,
    config: &Config,
    bot_user_id: Option<u64>,
) {
    let admin_user_id = &config.admin.telegram_user_id;
    let admin_feishu_open_id = &config.admin.feishu_open_id;

    let (mut jid, mut channel) = if !admin_user_id.is_empty() {
        let j = if let Some(bot_id) = bot_user_id {
            format!("tg:{bot_id}:user:{admin_user_id}")
        } else {
            format!("tg:user:{admin_user_id}")
        };
        (j, "telegram")
    } else if !admin_feishu_open_id.is_empty() {
        (
            format!("feishu:user:{admin_feishu_open_id}"),
            "feishu",
        )
    } else {
        ("web:main".to_string(), "")
    };

    let folder = &config.telegram.agent_folder;
    let now = chrono_now();

    // folder UNIQUE constraint
    let existing_by_folder = gm.list(db).ok().and_then(|all| {
        all.into_iter().find(|g| g.folder == *folder)
    });

    // When Telegram is disconnected, keep existing bot-aware jid
    if bot_user_id.is_none() && !admin_user_id.is_empty() {
        if let Some(ref existing) = existing_by_folder {
            let re = Regex::new(r"^tg:\d+:user:").unwrap();
            if re.is_match(&existing.jid) {
                jid = existing.jid.clone();
                channel = "telegram";
                tracing::info!("[GroupManager] Telegram disconnected; keeping existing jid {jid}");
            }
        }
    }

    // Migrate if folder occupied by different jid
    if let Some(ref existing) = existing_by_folder {
        if existing.jid != jid {
            gm.unregister(db, config, &existing.jid);
            tracing::info!(
                "[GroupManager] Migrated admin group from {} to {jid}",
                existing.jid
            );
        }
    }

    // Clean up legacy jid
    if !admin_user_id.is_empty() {
        let legacy_jid = format!("tg:user:{admin_user_id}");
        if jid != legacy_jid {
            if gm.get(db, &legacy_jid).is_some() {
                gm.unregister(db, config, &legacy_jid);
                tracing::info!("[GroupManager] Removed legacy jid {legacy_jid} (superseded by {jid})");
            }
        }
    }

    let existing = gm.get(db, &jid).or(existing_by_folder);
    let config_allowed_work_dirs =
        get_agent_allowed_work_dirs(&config.paths.global_config_path, folder);

    let binding = GroupBinding {
        jid,
        folder: folder.clone(),
        name: existing
            .as_ref()
            .map(|e| e.name.clone())
            .unwrap_or_else(|| format!("{folder} ({})", if channel.is_empty() { "web" } else { channel })),
        channel: channel.to_string(),
        is_admin: folder == "main",
        requires_trigger: false,
        allowed_tools: None,
        allowed_paths: existing.as_ref().and_then(|e| e.allowed_paths.clone()),
        allowed_work_dirs: match config_allowed_work_dirs {
            None => existing.as_ref().and_then(|e| e.allowed_work_dirs.clone()),
            Some(work_dirs) => work_dirs,
        },
        bot_token: existing.as_ref().and_then(|e| e.bot_token.clone()),
        max_messages: existing.as_ref().and_then(|e| e.max_messages),
        last_active: existing.as_ref().and_then(|e| e.last_active.clone()),
        added_at: existing
            .as_ref()
            .map(|e| e.added_at.clone())
            .unwrap_or_else(|| now.clone()),
    };

    gm.register(db, config, &binding);

    let action = if existing.is_some() {
        "updated"
    } else {
        "registered"
    };
    tracing::info!("[GroupManager] Admin group {action}: {} → agents/{folder}/", binding.jid);
}

pub fn ensure_wechat_admin_group(
    db: &Db,
    gm: &GroupManager,
    config: &Config,
    owner_jid: &str,
    folder: &str,
) {
    let now = chrono_now();

    // folder UNIQUE constraint
    let existing_by_folder = gm.list(db).ok().and_then(|all| {
        all.into_iter().find(|g| g.folder == *folder)
    });
    if let Some(ref existing) = existing_by_folder {
        if existing.jid != owner_jid {
            gm.unregister(db, config, &existing.jid);
            remove_group_from_config(&config.paths.global_config_path, &existing.jid);
            tracing::info!(
                "[GroupManager] Migrated WeChat group from {} to {owner_jid}",
                existing.jid
            );
        }
    }

    let existing = gm.get(db, owner_jid).or(existing_by_folder);
    let is_admin = folder == "main";

    let binding = GroupBinding {
        jid: owner_jid.to_string(),
        folder: folder.to_string(),
        name: existing
            .as_ref()
            .map(|e| e.name.clone())
            .unwrap_or_else(|| folder.to_string()),
        channel: "wechat".to_string(),
        is_admin,
        requires_trigger: false,
        allowed_tools: if is_admin { None } else { existing.as_ref().and_then(|e| e.allowed_tools.clone()) },
        allowed_paths: existing.as_ref().and_then(|e| e.allowed_paths.clone()),
        allowed_work_dirs: existing.as_ref().and_then(|e| e.allowed_work_dirs.clone()),
        bot_token: None,
        max_messages: existing.as_ref().and_then(|e| e.max_messages),
        last_active: existing.as_ref().and_then(|e| e.last_active.clone()),
        added_at: existing
            .as_ref()
            .map(|e| e.added_at.clone())
            .unwrap_or_else(|| now.clone()),
    };

    gm.register(db, config, &binding);

    let action = if existing.is_some() {
        "updated"
    } else {
        "registered"
    };
    tracing::info!(
        "[GroupManager] WeChat group {action}: {owner_jid} → agents/{folder}/ (isAdmin={is_admin})"
    );
}


