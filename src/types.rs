//! Shared types. Mirrors `src-old/types.ts`.
//!
//! `IChannel` is a TS interface; in Rust it becomes a trait — left out for now
//! and lands together with the channel module ports.

use anyhow::Result;
use async_trait::async_trait;
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
    /// "chat" | "cowork" | "code"
    pub group_type: String,
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
    pub core_prompt: String,
    pub model_id: Option<String>,
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

// ===== Agent API trait =====

/// Operations that the message router and cowork manager need from the agent pool.
#[async_trait]
pub trait AgentApi: Send + Sync {
    /// Send a direct reply to a chat (for admin commands and unregistered notices).
    async fn broadcast_reply(&self, chat_jid: &str, text: &str, bot_token: Option<&str>);

    /// Process a prompt through the agent. Blocks until the agent finishes.
    async fn process_and_wait(&self, jid: &str, group: &GroupBinding, prompt: &str) -> Result<()>;

    /// Process a prompt with image attachments through the agent. Blocks until the agent finishes.
    async fn process_and_wait_with_images(&self, jid: &str, group: &GroupBinding, prompt: &str, _attachments: &[crate::agent::input_builder::ImageAttachment]) -> Result<()> {
        // Default implementation: ignore attachments and call the basic version
        self.process_and_wait(jid, group, prompt).await
    }

    /// Destroy/cleanup agent state for a JID (after JID migration).
    async fn destroy(&self, jid: &str);

    /// Return the last assistant reply text produced during `process_and_wait`
    /// for `jid`. Used to persist task results. Default returns `None`.
    fn get_last_reply_text(&self, _jid: &str) -> Option<String> {
        None
    }
}

/// No-op stub — used before AgentPool is ported or when agent execution is unavailable.
pub struct NoopAgentApi;

#[async_trait]
impl AgentApi for NoopAgentApi {
    async fn broadcast_reply(&self, _jid: &str, _text: &str, _token: Option<&str>) {}
    async fn process_and_wait(
        &self,
        _jid: &str,
        _group: &GroupBinding,
        _prompt: &str,
    ) -> Result<()> {
        Ok(())
    }
    async fn destroy(&self, _jid: &str) {}
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
    /// JSON-serialized array of image attachments (data_url, mime_type)
    pub attachments: Option<String>,
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

// ===== Cowork types =====

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoworkWorkspace {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub status: String,
    pub root_dir: String,
    pub working_dir: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// One typed resource directory attached to a workspace.
/// kind: "raw" | "wiki" | "reference" | "workdir"
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceResource {
    pub workspace_id: String,
    pub kind: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoworkMember {
    pub workspace_id: String,
    pub member_id: String,
    pub role: String,
    pub jid: Option<String>,
    pub subdir: Option<String>,
    pub persona: Option<String>,
    pub responsibilities: Option<String>,
    pub triggers: Option<String>,
    pub handoff_rules: Option<String>,
    pub acceptance_criteria: Option<String>,
    pub output_format: Option<String>,
    pub sla: Option<String>,
    pub limits: Option<String>,
    pub joined_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoworkBoardEntry {
    pub id: String,
    pub workspace_id: String,
    pub section: String,
    pub title: Option<String>,
    pub content: String,
    pub author: String,
    pub pinned: bool,
    pub tags: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoworkTask {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub assignee: Option<String>,
    pub reviewer: Option<String>,
    pub priority: String,
    pub depends_on: Option<String>,
    pub attachments: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
    pub due_at: Option<String>,
    pub completed_at: Option<String>,
    /// Brief summary of what the task was asked to do.
    pub input_summary: Option<String>,
    /// Final output/result text produced when the task completed.
    pub result_output: Option<String>,
    /// JSON array of resource references (wiki/raw/reference paths) used.
    pub references: Option<String>,
    /// JSON array of file paths written during execution.
    pub artifacts: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoworkTaskComment {
    pub id: i64,
    pub task_id: String,
    pub author: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoworkMessage {
    pub id: String,
    pub workspace_id: String,
    pub from_member: String,
    pub to_member: Option<String>,
    pub message_type: String,
    pub content: String,
    pub attachments: Option<String>,
    pub task_id: Option<String>,
    pub is_read: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoworkRecordingSession {
    pub id: String,
    pub workspace_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub event_count: i64,
    pub total_tokens: i64,
    pub agents: Option<String>,
}

// ===== Cowork Templates =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoworkTemplate {
    pub name: String,
    pub description: String,
    pub icon: Option<String>,
    pub members: Vec<TemplateMember>,
    pub board: Option<TemplateBoard>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateMember {
    #[serde(rename = "agentFolder")]
    pub agent_folder: String,
    pub role: String,
    pub subdir: Option<String>,
    pub persona: Option<String>,
    pub responsibilities: Option<Vec<String>>,
    pub triggers: Option<Vec<TemplateTrigger>>,
    pub handoff: Option<Vec<TemplateHandoffRule>>,
    #[serde(rename = "acceptanceCriteria")]
    pub acceptance_criteria: Option<Vec<String>>,
    pub output: Option<TemplateOutput>,
    pub sla: Option<TemplateSla>,
    pub limits: Option<TemplateLimits>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateTrigger {
    #[serde(rename = "type")]
    pub trigger_type: String,
    pub condition: Option<String>,
    pub from: Option<String>,
    #[serde(rename = "messageType")]
    pub message_type: Option<String>,
    pub status: Option<String>,
    pub assignee: Option<String>,
    pub cron: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateHandoffRule {
    pub when: String,
    pub to: String,
    #[serde(rename = "type")]
    pub message_type: String,
    #[serde(rename = "messageTemplate")]
    pub message_template: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateOutput {
    pub format: Option<String>,
    #[serde(rename = "requiredSections")]
    pub required_sections: Option<Vec<String>>,
    #[serde(rename = "attachDiff")]
    pub attach_diff: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateSla {
    #[serde(rename = "maxDurationPerTaskMinutes")]
    pub max_duration_minutes: Option<i64>,
    #[serde(rename = "maxTokenPerTask")]
    pub max_token_per_task: Option<i64>,
    #[serde(rename = "escalateAfterBlockedMinutes")]
    pub escalate_after_blocked_minutes: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateLimits {
    #[serde(rename = "maxFileSizeWriteKb")]
    pub max_file_size_write_kb: Option<i64>,
    #[serde(rename = "allowedBashCommands")]
    pub allowed_bash_commands: Option<Vec<String>>,
    #[serde(rename = "deniedTools")]
    pub denied_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateBoard {
    pub sections: Vec<TemplateBoardSection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateBoardSection {
    #[serde(rename = "type")]
    pub section_type: String,
    pub title: String,
    pub template: Option<String>,
}
