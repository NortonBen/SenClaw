//! Domain models — ported from internal/domain/*.go.
//! JSON field names match the Go `json:"..."` tags (camelCase) so the existing
//! React frontend and any stored data stay wire-compatible.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// String-keyed config/param map. BTreeMap keeps a stable key order in output.
pub type StrMap = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TikTokAccount {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// legacy: direct proxy URL when not using proxy_id.
    #[serde(default)]
    pub proxy: String,
    /// legacy user-data dir when not using browser_profile_id.
    #[serde(default)]
    pub profile_path: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub proxy_id: String,
    #[serde(default)]
    pub browser_profile_id: String,
    #[serde(default)]
    pub created_at: String,
    // Filled only when resolving before a run (never stored on the account row).
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub viewport_width: i32,
    #[serde(default, skip_serializing_if = "is_zero_i32")]
    pub viewport_height: i32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub locale: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub timezone_id: String,
}

fn is_zero_i32(v: &i32) -> bool {
    *v == 0
}

/// A small atomic step mapping to a browser action: click, fill, wait, goto, …
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlowAtomic {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub kind: String,
    /// Deserialised leniently: numbers/bools/objects from AI payloads are
    /// coerced to strings (see `deserialize_string_map`).
    #[serde(
        default,
        deserialize_with = "de_string_map_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub params: Option<StrMap>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowAction {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default)]
    pub type_: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, deserialize_with = "de_string_map")]
    pub config: StrMap,
    #[serde(rename = "timeoutSeconds", default)]
    pub timeout: i64,
    #[serde(
        default,
        deserialize_with = "de_string_map_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub params: Option<StrMap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub atomics: Vec<FlowAtomic>,
}

impl FlowAction {
    pub fn config_get(&self, key: &str) -> Option<&str> {
        self.config.get(key).map(|s| s.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flow {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<StrMap>,
    #[serde(default)]
    pub actions: Vec<FlowAction>,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFlowAction {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub step: FlowAction,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPostInteractionRow {
    pub id: String,
    pub account_id: String,
    pub post_key: String,
    pub interaction_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub post_url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub author_username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub extra_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountFriendEventRow {
    pub id: String,
    pub account_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target_username: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target_user_id: String,
    pub event_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountKVMetaRow {
    pub account_id: String,
    pub key: String,
    pub value: String,
    pub updated_at: String,
}

pub const RUN_QUEUED: &str = "queued";
pub const RUN_RUNNING: &str = "running";
pub const RUN_DONE: &str = "done";
pub const RUN_FAILED: &str = "failed";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowRun {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub flow_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub schedule_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub logs: Vec<String>,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub ended_at: String,
}

// ---- Proxy / Profile ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedProxy {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProfile {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub user_data_dir: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub viewport_width: i32,
    #[serde(default)]
    pub viewport_height: i32,
    #[serde(default)]
    pub locale: String,
    #[serde(default)]
    pub timezone_id: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

// ---- Notifications ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRule {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub flow_id: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub message_template: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub rule_id: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub flow_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub read_at: String,
    #[serde(default)]
    pub created_at: String,
}

pub const EVENT_RUN_FAILED: &str = "run_failed";
pub const EVENT_RUN_DONE: &str = "run_done";
pub const EVENT_FLOW_ACTION: &str = "flow_action";

// ---- Schedules ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub flow_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<StrMap>,
    #[serde(default)]
    pub all_accounts: bool,
    #[serde(default)]
    pub account_ids: Vec<String>,
    #[serde(rename = "type", default)]
    pub type_: String,
    #[serde(default)]
    pub daily_at: String,
    #[serde(default)]
    pub once_at: String,
    #[serde(default)]
    pub timezone_id: String,
    #[serde(default)]
    pub last_run_at: String,
    #[serde(default)]
    pub next_run_at: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

pub const SCHEDULE_RUN_NOW: &str = "run_now";
pub const SCHEDULE_DAILY_AT: &str = "daily_at";
pub const SCHEDULE_ONCE_AT: &str = "once_at";

// ---- Dashboard ----

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyRunCount {
    pub date: String,
    pub done: i64,
    pub failed: i64,
    pub running: i64,
    pub queued: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowRunRank {
    pub flow_id: String,
    pub count: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardRunStats {
    pub last7_days: Vec<DailyRunCount>,
    pub status_totals7d: BTreeMap<String, i64>,
    pub top_flows7d: Vec<FlowRunRank>,
}

// ---- App settings ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub llm_provider: String,
    #[serde(default, rename = "openaiApiKey")]
    pub openai_api_key: String,
    #[serde(default, rename = "openaiBaseUrl")]
    pub openai_base_url: String,
    #[serde(default, rename = "openaiModel")]
    pub openai_model: String,
    #[serde(default, rename = "openrouterApiKey")]
    pub openrouter_api_key: String,
    #[serde(default, rename = "openrouterModel")]
    pub openrouter_model: String,
    #[serde(default, rename = "openrouterBaseUrl")]
    pub openrouter_base_url: String,
    #[serde(default, rename = "openrouterHttpReferer")]
    pub openrouter_http_referer: String,
    #[serde(default, rename = "openrouterAppName")]
    pub openrouter_app_name: String,
    #[serde(default, rename = "deepseekApiKey")]
    pub deepseek_api_key: String,
    #[serde(default, rename = "deepseekBaseUrl")]
    pub deepseek_base_url: String,
    #[serde(default, rename = "deepseekModel")]
    pub deepseek_model: String,
    #[serde(default, rename = "lmStudioUrl")]
    pub lm_studio_url: String,
    #[serde(default, rename = "lmStudioModel")]
    pub lm_studio_model: String,
    #[serde(default, rename = "lmStudioApiKey")]
    pub lm_studio_api_key: String,
    #[serde(default, rename = "aiMemoryExtractModel")]
    pub ai_memory_extract_model: String,
    #[serde(default, rename = "aiMemoryEmbeddingProvider")]
    pub ai_memory_embedding_provider: String,
    #[serde(default, rename = "aiMemoryEmbeddingModel")]
    pub ai_memory_embedding_model: String,
    #[serde(default, rename = "aiMemoryEmbeddingBaseUrl")]
    pub ai_memory_embedding_base_url: String,
    #[serde(default, rename = "aiMemoryEmbeddingApiKey")]
    pub ai_memory_embedding_api_key: String,
}

// ---- Agent skills ----

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSkill {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

// ---- lenient string-map deserialisation (numbers/bools/objects -> strings) ----

fn coerce_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

pub fn de_string_map<'de, D>(d: D) -> Result<StrMap, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = de_string_map_opt(d)?;
    Ok(opt.unwrap_or_default())
}

pub fn de_string_map_opt<'de, D>(d: D) -> Result<Option<StrMap>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<BTreeMap<String, serde_json::Value>> = Option::deserialize(d)?;
    Ok(raw.map(|m| {
        m.into_iter()
            .map(|(k, v)| (k, coerce_to_string(&v)))
            .collect()
    }))
}
