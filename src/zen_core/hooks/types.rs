//! Hook types — mirror of TS sema-core `hooks/types.ts`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ============================================================================
// HookEvent
// ============================================================================

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HookEvent {
    UserPromptSubmit,
    PreToolUse,
    PostToolUse,
    PermissionRequest,
    Stop,
    SessionStart,
    SessionEnd,
    PreCompact,
    PostCompact,
    Notification,
    Error,
    SubagentStart,
    SubagentEnd,
}

impl HookEvent {
    /// Events that can never block the main flow even if a hook returns `blocked: true`.
    pub fn is_non_blockable(&self) -> bool {
        matches!(
            self,
            HookEvent::SessionEnd
                | HookEvent::Stop
                | HookEvent::PostToolUse
                | HookEvent::PostCompact
                | HookEvent::SubagentEnd
        )
    }
}

// ============================================================================
// Config
// ============================================================================

/// Top-level hook configuration loaded from `settings.json` / `CLAUDE.md`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookConfig {
    #[serde(default)]
    pub hooks: HashMap<HookEvent, Vec<HookEventConfig>>,
}

/// One entry under a hook event — optional matcher/if filter + list of hooks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEventConfig {
    /// Glob pattern matched against the tool name (or notification type).
    /// `*` matches everything. Comma-separated values are OR'd.
    pub matcher: Option<String>,

    /// Regex applied to the serialised `tool_input` (or message text).
    /// Matched with `Regex::is_match()`.
    #[serde(rename = "if")]
    pub if_condition: Option<String>,

    pub hooks: Vec<HookDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDefinition {
    #[serde(rename = "type")]
    pub hook_type: HookType,
    pub command: Option<String>,
    pub prompt: Option<String>,
    /// Timeout in seconds (default 10 for command, 30 for prompt).
    pub timeout: Option<u64>,
    /// Whether a non-zero exit / reject decision blocks the main flow.
    /// Defaults to `true`. Forced `false` when `async = true`.
    pub blocking: Option<bool>,
    /// Fire-and-forget — do not await completion.
    /// Prompt hooks are always sync.
    #[serde(rename = "async")]
    pub is_async: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookType {
    Command,
    Prompt,
}

impl HookDefinition {
    pub fn normalize(mut self) -> Self {
        if self.hook_type == HookType::Prompt {
            self.is_async = Some(false);
        }
        if self.is_async == Some(true) {
            self.blocking = Some(false);
        }
        if self.blocking.is_none() {
            self.blocking = Some(true);
        }
        if self.is_async.is_none() {
            self.is_async = Some(false);
        }
        self
    }

    pub fn is_blocking(&self) -> bool {
        self.blocking.unwrap_or(true)
    }

    pub fn is_fire_and_forget(&self) -> bool {
        self.is_async.unwrap_or(false)
    }
}

// ============================================================================
// HookInput — typed payloads sent to hook processes / prompts
// ============================================================================

/// Base fields present on every hook input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInputBase {
    pub hook_event_name: HookEvent,
    pub session_id: String,
    pub agent_id: String,
    pub timestamp: String,
    pub cwd: String,
}

/// All concrete hook input variants.
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum HookInput {
    UserPromptSubmit(UserPromptSubmitInput),
    PreToolUse(PreToolUseInput),
    PostToolUse(PostToolUseInput),
    PermissionRequest(PermissionRequestInput),
    PreCompact(PreCompactInput),
    Session(SessionInput),
    Stop(StopInput),
    Subagent(SubagentInput),
    Notification(NotificationInput),
    Error(ErrorInput),
}

impl HookInput {
    /// Extract the text that the `if` condition regex is matched against.
    pub fn extract_match_text(&self) -> String {
        match self {
            HookInput::PreToolUse(i) => {
                if let Some(cmd) = i.tool_input.get("command").and_then(|v| v.as_str()) {
                    cmd.to_string()
                } else {
                    i.tool_input.to_string()
                }
            }
            HookInput::PostToolUse(i) => {
                if let Some(cmd) = i.tool_input.get("command").and_then(|v| v.as_str()) {
                    cmd.to_string()
                } else {
                    i.tool_input.to_string()
                }
            }
            HookInput::PermissionRequest(i) => {
                if let Some(cmd) = i.tool_input.get("command").and_then(|v| v.as_str()) {
                    cmd.to_string()
                } else {
                    i.tool_input.to_string()
                }
            }
            HookInput::Notification(i) => i.message.clone(),
            HookInput::Error(i) => i.error_message.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        }
    }

    /// Return the tool_name if this is a tool-related event, for matcher matching.
    pub fn match_query(&self) -> Option<&str> {
        match self {
            HookInput::PreToolUse(i) => Some(&i.tool_name),
            HookInput::PostToolUse(i) => Some(&i.tool_name),
            HookInput::PermissionRequest(i) => Some(&i.tool_name),
            HookInput::Notification(i) => i.notification_type.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UserPromptSubmitInput {
    #[serde(flatten)]
    pub base: HookInputBase,
    pub prompt: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreToolUseInput {
    #[serde(flatten)]
    pub base: HookInputBase,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct PostToolUseInput {
    #[serde(flatten)]
    pub base: HookInputBase,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
    pub tool_response: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct PermissionRequestInput {
    #[serde(flatten)]
    pub base: HookInputBase,
    pub tool_name: String,
    pub tool_input: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreCompactInput {
    #[serde(flatten)]
    pub base: HookInputBase,
    pub message_count: usize,
    pub context_history: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInput {
    #[serde(flatten)]
    pub base: HookInputBase,
}

#[derive(Debug, Clone, Serialize)]
pub struct StopInput {
    #[serde(flatten)]
    pub base: HookInputBase,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubagentInput {
    #[serde(flatten)]
    pub base: HookInputBase,
    pub subagent_id: String,
    pub subagent_task: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NotificationInput {
    #[serde(flatten)]
    pub base: HookInputBase,
    pub notification_type: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorInput {
    #[serde(flatten)]
    pub base: HookInputBase,
    pub error_message: String,
    pub error_type: Option<String>,
}

// ============================================================================
// HookOutput / AggregatedHookResult
// ============================================================================

/// Response written to stdout by a command hook, or returned by a prompt hook.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct HookOutput {
    pub decision: Option<String>,
    pub blocked: Option<bool>,
    pub reason: Option<String>,
    pub abort: Option<bool>,
    #[serde(rename = "updatedInput")]
    pub updated_input: Option<serde_json::Value>,
    #[serde(rename = "additionalContext")]
    pub additional_context: Option<String>,
    pub response: Option<String>,
}

impl HookOutput {
    pub fn is_blocked(&self) -> bool {
        self.blocked.unwrap_or(false) || self.decision.as_deref() == Some("reject")
    }

    pub fn is_abort(&self) -> bool {
        self.abort.unwrap_or(false)
    }
}

#[derive(Debug, Clone)]
pub struct HookError {
    pub hook: HookDefinition,
    pub error: String,
}

/// Aggregated result of running all matching hooks for a single event.
#[derive(Debug, Clone)]
pub struct AggregatedHookResult {
    /// Whether a blocking hook requested the action be blocked.
    pub blocked: bool,
    /// Whether the session should be aborted entirely.
    pub abort: bool,
    /// Human-readable reason from the blocking hook.
    pub reason: Option<String>,
    /// Updated tool input from a hook (PreToolUse only).
    pub updated_input: Option<serde_json::Value>,
    /// Additional context strings to inject into the conversation.
    pub additional_context: Vec<String>,
    pub errors: Vec<HookError>,
}

impl AggregatedHookResult {
    pub fn empty() -> Self {
        Self {
            blocked: false,
            abort: false,
            reason: None,
            updated_input: None,
            additional_context: Vec::new(),
            errors: Vec::new(),
        }
    }
}
