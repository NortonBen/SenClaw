//! Shared types. Mirrors `src-old/types.ts`.
//!
//! `IChannel` is a TS interface; in Rust it becomes a trait — left out for now
//! and lands together with the channel module ports.

use serde::{Deserialize, Serialize};

// ===== Channel layer =====

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatType {
    Private,
    Group,
    Supergroup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    pub id: String,
    pub chat_jid: String,
    pub sender_name: String,
    pub sender_jid: String,
    pub content: String,
    pub timestamp: String,
    pub is_from_me: bool,
    pub chat_type: ChatType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mentions_bot_username: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_msg_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMeta {
    pub jid: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "type")]
    pub chat_type: ChatType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineButton {
    pub label: String,
    pub callback_data: String,
}

// ===== Gateway layer =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupBinding {
    pub jid: String,
    pub folder: String,
    pub name: String,
    /// "" = web-only (no channel binding).
    pub channel: String,
    pub is_admin: bool,
    pub requires_trigger: bool,
    /// `None` = all tools allowed.
    pub allowed_tools: Option<Vec<String>>,
    pub allowed_paths: Option<Vec<String>>,
    /// `None` = workspace switching disallowed.
    pub allowed_work_dirs: Option<Vec<String>>,
    /// `None` = use `TELEGRAM_BOT_TOKEN`.
    pub bot_token: Option<String>,
    /// `None` = use `MAX_MESSAGES_PER_GROUP`.
    pub max_messages: Option<u32>,
    pub last_active: Option<String>,
    pub added_at: String,
}

// ===== New entity model =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: i64,
    pub platform_type: String,
    pub name: String,
    pub credentials_json: String,
    pub connection_state: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub id: i64,
    pub folder: String,
    pub name: String,
    pub requires_trigger: bool,
    pub allowed_tools: Option<Vec<String>>,
    pub allowed_paths: Option<Vec<String>>,
    pub allowed_work_dirs: Option<Vec<String>>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Binding {
    pub id: i64,
    /// None = pending binding (auto-complete on first message)
    pub jid: Option<String>,
    pub agent_id: i64,
    pub channel_id: i64,
    pub is_admin: bool,
    pub bot_token_override: Option<String>,
    pub max_messages: Option<u32>,
    pub last_active: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingWithRelations {
    pub binding: Binding,
    pub agent: Agent,
    pub channel: Channel,
}

// ===== DB layer =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredMessage {
    pub message_id: String,
    pub chat_jid: String,
    pub sender_jid: String,
    pub sender_name: String,
    pub content: String,
    pub timestamp: String,
    pub is_from_me: bool,
    pub is_bot_reply: bool,
    pub reply_to_id: Option<String>,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Active,
    Paused,
    Completed,
    Error,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Error => "error",
        }
    }
    pub fn parse(raw: &str) -> Self {
        match raw {
            "paused" => Self::Paused,
            "completed" => Self::Completed,
            "error" => Self::Error,
            _ => Self::Active,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleType {
    Cron,
    Interval,
    Once,
}

impl ScheduleType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cron => "cron",
            Self::Interval => "interval",
            Self::Once => "once",
        }
    }
    pub fn parse(raw: &str) -> Self {
        match raw {
            "interval" => Self::Interval,
            "once" => Self::Once,
            _ => Self::Cron,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContextMode {
    Isolated,
    Group,
    Notify,
    Script,
    ScriptAgent,
}

impl ContextMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Isolated => "isolated",
            Self::Group => "group",
            Self::Notify => "notify",
            Self::Script => "script",
            Self::ScriptAgent => "script-agent",
        }
    }
    pub fn parse(raw: &str) -> Self {
        match raw {
            "group" => Self::Group,
            "notify" => Self::Notify,
            "script" => Self::Script,
            "script-agent" => Self::ScriptAgent,
            _ => Self::Isolated,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunStatus {
    Success,
    Error,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Error => "error",
        }
    }
    pub fn parse(raw: &str) -> Self {
        match raw {
            "error" => Self::Error,
            _ => Self::Success,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub id: String,
    pub group_folder: String,
    pub chat_jid: String,
    pub prompt: String,
    pub schedule_type: ScheduleType,
    pub schedule_value: String,
    pub context_mode: ContextMode,
    /// Bash command for `Script` / `ScriptAgent` modes.
    pub script_command: Option<String>,
    pub next_run: Option<String>,
    pub last_run: Option<String>,
    pub last_result: Option<String>,
    pub status: TaskStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskRunLog {
    pub id: i64,
    pub task_id: String,
    pub run_at: String,
    pub duration_ms: Option<i64>,
    pub status: RunStatus,
    pub result: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TaskRunLogInsert {
    pub task_id: String,
    pub run_at: String,
    pub duration_ms: Option<i64>,
    pub status: RunStatus,
    pub result: Option<String>,
    pub error: Option<String>,
}
