//! zen-core: Rust-native replacement for sema-core.
//!
//! Provides the agent execution engine — event-driven LLM conversation loop
//! with tool execution, permission gating, streaming events, and session
//! lifecycle management.
//!
//! ## Architecture
//!
//! ```text
//! ZenCore (trait)
//!   └── ZenEngine
//!         ├── EventBus (tokio::broadcast)
//!         ├── StateManager (per-agent state)
//!         ├── Conversation::query() — core loop
//!         │     ├── query_llm() → assistant response
//!         │     └── run_tools() → tool execution
//!         ├── PermissionManager
//!         └── ModelManager
//! ```

pub mod config_manager;
pub mod context;
pub mod conversation;
pub mod engine;
pub mod events;
pub mod hooks;
pub mod model_manager;
pub mod permissions;
pub mod prompt;
pub mod query_llm;
pub mod run_tools;
pub mod state;
pub mod vision;
pub mod workbench;

use std::collections::HashMap;

use anyhow::Result;
use serde::{Deserialize, Serialize};

// Re-export key types
pub use config_manager::{with_conf_manager, ConfigManager, ProjectConfig, ProjectConfigPatch};
pub use context::{
    get_agent_data_dir, get_engine_store, get_event_bus, get_hook_manager, get_mcp_manager,
    get_model_profile, get_state_manager, get_working_dir, run_with_engine, CoreConfig,
    EngineStore,
};
pub use engine::ZenEngine;
pub use events::{EngineEvent, EventBus, ResponseRegistry};
pub use model_manager::{with_model_manager, ModelManager, ModelUpdateData, TaskConfig};
pub use state::StateManager;
pub use workbench::{
    ProcessInfo, StopReason, WorkbenchArtifact, WorkbenchFile, WorkbenchMode, WorkbenchNewData,
    WorkbenchService, WorkbenchServiceCrashedData, WorkbenchServiceReadyData,
    WorkbenchServiceStoppedData,
};

/// Agent id used for root-agent events.
pub const MAIN_AGENT_ID: &str = "main";

// ============================================================================
// Session state
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Processing,
    Paused,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionState::Idle => "idle",
            SessionState::Processing => "processing",
            SessionState::Paused => "paused",
        }
    }
}

// ============================================================================
// Message types (Anthropic-compatible content blocks)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        is_error: bool,
    },
    #[serde(rename = "thinking")]
    Thinking { thinking: String },
    #[serde(rename = "image")]
    Image { source: ImageSource },
    #[serde(rename = "control_signal")]
    ControlSignal {
        signal_type: String,
        payload: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,
    #[serde(rename = "media_type")]
    pub media_type: String,
    pub data: String,
}

/// Raw token usage as reported by an LLM API response.
///
/// Holds both Anthropic (`input_tokens`/`output_tokens`/`cache_*`) and
/// OpenAI (`prompt_tokens`/`completion_tokens`) shapes — only the fields the
/// provider actually returns are populated. Use [`RawUsage::input`] /
/// [`RawUsage::output`] for a provider-normalized count.
///
/// Port of the `usage` handling in TS `util/tokens.ts`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RawUsage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u64>,
}

impl RawUsage {
    /// Parse usage fields out of an arbitrary JSON object. Returns `None` if
    /// the value is not an object or carries no recognized token field.
    pub fn from_json(v: &serde_json::Value) -> Option<RawUsage> {
        if !v.is_object() {
            return None;
        }
        let get = |k: &str| v.get(k).and_then(|x| x.as_u64());
        let u = RawUsage {
            input_tokens: get("input_tokens"),
            output_tokens: get("output_tokens"),
            cache_creation_input_tokens: get("cache_creation_input_tokens"),
            cache_read_input_tokens: get("cache_read_input_tokens"),
            prompt_tokens: get("prompt_tokens"),
            completion_tokens: get("completion_tokens"),
        };
        if u.input_tokens.is_none()
            && u.output_tokens.is_none()
            && u.cache_creation_input_tokens.is_none()
            && u.cache_read_input_tokens.is_none()
            && u.prompt_tokens.is_none()
            && u.completion_tokens.is_none()
        {
            None
        } else {
            Some(u)
        }
    }

    /// Merge fields from `other` into `self`, preferring non-`None` values from
    /// `other`. Used to accumulate streamed usage (Anthropic splits input across
    /// `message_start` and output across `message_delta`).
    pub fn merge(&mut self, other: &RawUsage) {
        if other.input_tokens.is_some() {
            self.input_tokens = other.input_tokens;
        }
        if other.output_tokens.is_some() {
            self.output_tokens = other.output_tokens;
        }
        if other.cache_creation_input_tokens.is_some() {
            self.cache_creation_input_tokens = other.cache_creation_input_tokens;
        }
        if other.cache_read_input_tokens.is_some() {
            self.cache_read_input_tokens = other.cache_read_input_tokens;
        }
        if other.prompt_tokens.is_some() {
            self.prompt_tokens = other.prompt_tokens;
        }
        if other.completion_tokens.is_some() {
            self.completion_tokens = other.completion_tokens;
        }
    }

    /// Provider-normalized input (prompt) token count, including cache tokens.
    pub fn input(&self) -> u64 {
        if let Some(p) = self.prompt_tokens {
            p
        } else {
            self.input_tokens.unwrap_or(0)
                + self.cache_creation_input_tokens.unwrap_or(0)
                + self.cache_read_input_tokens.unwrap_or(0)
        }
    }

    /// Provider-normalized output (completion) token count.
    pub fn output(&self) -> u64 {
        self.completion_tokens.or(self.output_tokens).unwrap_or(0)
    }

    /// True when no real usage was reported (both directions zero).
    pub fn is_empty(&self) -> bool {
        self.input() == 0 && self.output() == 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    #[serde(rename = "type")]
    pub msg_type: String, // "user" | "assistant"
    pub message: MessagePayload,
    pub uuid: String,
    /// Token usage reported by the API for this (assistant) message. `None`
    /// for user messages and providers that don't report usage (e.g. local
    /// inference). Never serialized into LLM request bodies — the adapters
    /// build their own request payloads from `message.content`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<RawUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePayload {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

// ============================================================================
// Event data types (mirrors TS sema-core events)
// ============================================================================

/// Event: `session:ready`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionReadyData {
    pub working_dir: String,
    pub session_id: String,
    pub history_loaded: bool,
    pub usage: UsageData,
    pub project_input_history: Vec<String>,
}

/// Event: `session:interrupted`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInterruptedData {
    pub agent_id: String,
    pub content: String,
}

/// Event: `session:error`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionErrorData {
    #[serde(rename = "type")]
    pub error_type: String,
    pub error: SessionErrorDetail,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Event: `state:update`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateUpdateData {
    pub state: SessionState,
}

/// Event: `message:thinking:chunk`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingChunkData {
    pub content: String,
    pub delta: String,
}

/// Event: `message:text:chunk`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextChunkData {
    pub content: String,
    pub delta: String,
}

/// Event: `message:complete`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageCompleteData {
    pub agent_id: String,
    pub reasoning: String,
    pub content: String,
    pub has_tool_calls: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCallInfo>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallInfo {
    pub name: String,
    pub args: serde_json::Value,
}

/// Event: `conversation:usage`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationUsageData {
    pub usage: UsageData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageData {
    #[serde(rename = "useTokens")]
    pub use_tokens: u64,
    #[serde(rename = "maxTokens")]
    pub max_tokens: u64,
    #[serde(rename = "promptTokens")]
    pub prompt_tokens: u64,
}

/// Event: `tool:permission:request`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissionRequestData {
    pub agent_id: String,
    pub tool_name: String,
    pub title: String,
    pub content: serde_json::Value,
    pub options: HashMap<String, String>,
}

/// Event: `tool:permission:response`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPermissionResponseData {
    pub tool_name: String,
    pub selected: String,
}

/// Event: `tool:execution:complete`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionCompleteData {
    pub agent_id: String,
    pub tool_name: String,
    pub title: String,
    pub summary: String,
    pub content: serde_json::Value,
}

/// Event: `tool:execution:error`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolExecutionErrorData {
    pub agent_id: String,
    pub tool_name: String,
    pub title: String,
    pub content: String,
}

/// Event: `todos:update`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodosUpdateItem {
    pub content: String,
    pub status: String,
    #[serde(
        default,
        rename = "activeForm",
        skip_serializing_if = "Option::is_none"
    )]
    pub active_form: Option<String>,
}

/// Event: `topic:update`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicUpdateData {
    pub is_new_topic: bool,
    pub title: String,
}

/// Event: `compact:start`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactStartData {
    pub message_count: usize,
}

/// Event: `compact:exec`
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactExecData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub err_msg: Option<String>,
    pub token_before: u64,
    pub token_compact: u64,
    pub compact_rate: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Event: `file:reference`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReferenceData {
    pub references: Vec<FileReferenceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReferenceInfo {
    #[serde(rename = "type")]
    pub ref_type: String,
    pub name: String,
    pub content: String,
}

/// Event: `ask:question:request`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestionRequestData {
    pub agent_id: String,
    pub questions: Vec<AskQuestionItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<AskQuestionMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestionItem {
    pub question: String,
    pub header: String,
    pub options: Vec<AskQuestionOption>,
    #[serde(rename = "multiSelect")]
    pub multi_select: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestionMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Event: `ask:question:response`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskQuestionResponseData {
    pub agent_id: String,
    pub answers: HashMap<String, String>,
}

/// Event: `plan:exit:request`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanExitRequestData {
    pub agent_id: String,
    pub plan_file_path: String,
    pub plan_content: String,
    pub options: PlanExitOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanExitOptions {
    #[serde(rename = "startEditing")]
    pub start_editing: String,
    #[serde(rename = "clearContextAndStart")]
    pub clear_context_and_start: String,
}

/// Event: `plan:exit:response`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanExitResponseData {
    pub agent_id: String,
    pub selected: String,
}

/// Event: `plan:implement`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanImplementData {
    pub plan_file_path: String,
    pub plan_content: String,
}

/// Event: `task:agent:start`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAgentStartData {
    pub task_id: String,
    pub subagent_type: String,
    pub description: String,
    pub prompt: String,
}

/// Event: `task:agent:end`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAgentEndData {
    pub task_id: String,
    pub status: String,
    pub content: String,
}

/// Event: `config:no_models`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigNoModelsData {
    pub message: String,
    pub suggestion: String,
}

// ============================================================================
// Tool trait — the interface all tools must implement
// ============================================================================

/// Result from a tool's `call` method. Tools yield progress updates,
/// then terminate with a final `ToolOutput::Result`.
#[derive(Debug, Clone)]
pub enum ToolOutput {
    /// Intermediate progress (e.g. "reading file..."). Not sent to LLM.
    Progress { message: String },
    /// Final result sent back as `tool_result` to the LLM.
    Result {
        data: serde_json::Value,
        result_for_assistant: String,
    },
    /// A control signal that interrupts the loop and triggers a context rebuild.
    ClearContextAndStart {
        plan_file_path: String,
        plan_content: String,
    },
}

/// Display-ready summary emitted via `tool:execution:complete`.
#[derive(Debug, Clone)]
pub struct ToolResultMessage {
    pub title: String,
    pub summary: String,
    pub content: serde_json::Value,
}

/// Permission info for UI rendering.
#[derive(Debug, Clone)]
pub struct ToolPermissionInfo {
    pub title: String,
    pub content: serde_json::Value,
}

/// Context passed to tools during execution.
pub struct ToolContext<'a> {
    pub agent_id: &'a str,
    pub working_dir: &'a str,
    pub agent_data_dir: &'a str,
    pub abort: tokio_util::sync::CancellationToken,
    /// Event bus for tools that emit/receive events (AskUser, etc.)
    pub event_bus: Option<&'a EventBus>,
    /// Response registry for tools that need request-response (AskUser, etc.)
    pub response_registry: Option<&'a ResponseRegistry>,
}

/// Trait implemented by every tool available to agents.
///
/// Tools are async generators — they yield intermediate progress then a final
/// result. The engine wraps them in the permission / error-handling layer.
#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;

    /// Whether this tool only reads data (no side effects).
    /// Read-only tools can run concurrently.
    fn is_read_only(&self) -> bool;

    /// Validate input before execution. Return `Ok(())` or an error message.
    async fn validate_input(
        &self,
        _input: &serde_json::Value,
        _ctx: &ToolContext<'_>,
    ) -> std::result::Result<(), String> {
        Ok(())
    }

    /// Execute the tool. Returns an async generator of ToolOutput.
    /// The final item should be `ToolOutput::Result`.
    async fn call(
        &self,
        input: serde_json::Value,
        ctx: &ToolContext<'_>,
    ) -> Result<Vec<ToolOutput>>;

    /// Generate display info for the result.
    fn gen_tool_result_message(
        &self,
        data: &serde_json::Value,
        input: &serde_json::Value,
    ) -> ToolResultMessage;

    /// Generate display title for the tool invocation.
    fn get_display_title(&self, input: &serde_json::Value) -> String;

    /// Generate permission request UI info.
    fn gen_tool_permission(&self, input: &serde_json::Value) -> Option<ToolPermissionInfo> {
        let _ = input;
        None
    }

    // ========================================================================
    // Lazy-load metadata (mirrors `claude-code` tool discovery pattern)
    //
    // Most tools should set defaults below. Override only when:
    //   - The tool is rarely used → `should_defer() = true` to remove it from
    //     the initial tool list. The LLM can find it via `ToolSearch`.
    //   - The tool is core / always relevant → `always_load() = true` to force
    //     inclusion even in restricted modes.
    //   - The tool was renamed → expose `aliases()` so old tool_use calls
    //     still resolve.
    // ========================================================================

    /// Short keyword hint (3-10 words) used by `ToolSearch` to discover this
    /// tool when it's deferred. Default = first sentence of `description()`.
    fn search_hint(&self) -> String {
        self.description()
            .split('.')
            .next()
            .unwrap_or("")
            .trim()
            .to_string()
    }

    /// When true, this tool is excluded from the initial tool list sent to the
    /// LLM each turn. The LLM must call `ToolSearch` first to discover it,
    /// then it becomes available for subsequent turns.
    ///
    /// Massive token saver — 100+ MCP tools can be deferred so only ~14 core
    /// tools land in the initial prompt.
    fn should_defer(&self) -> bool {
        false
    }

    /// Override `should_defer()` — when true, this tool is **always** included
    /// in the initial prompt regardless of any defer policy. Use for
    /// session-critical tools (`ToolSearch` itself, `Task`, `Bash`, etc.).
    fn always_load(&self) -> bool {
        false
    }

    /// Alternative names this tool was previously known by. Lets old
    /// conversation history (tool_use blocks with renamed names) keep working.
    fn aliases(&self) -> &[&str] {
        &[]
    }
}

// ============================================================================
// MCP + Runtime config
// ============================================================================

/// MCP transport configuration used by zen-core runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub request_timeout_secs: Option<u64>,
}

/// Runtime options used while creating a zen-core instance.
#[derive(Debug, Clone)]
pub struct ZenCoreOptions {
    pub instance_id: String,
    pub agent_data_dir: String,
    pub working_dir: String,
    pub use_tools: Vec<String>,
    pub skills_extra_dirs: Vec<String>,
    pub skip_file_edit_permission: bool,
    pub skip_bash_exec_permission: bool,
    pub skip_skill_permission: bool,
    pub skip_mcp_tool_permission: bool,
    pub skip_mcp_init: bool,
    pub stream: bool,
    pub thinking: bool,
    pub system_prompt: String,
    pub custom_rules: String,
    pub enable_llm_cache: bool,
    pub agent_mode: AgentMode,
    /// Custom memory directory for this instance (e.g., for cowork workspaces)
    pub custom_memory_dir: Option<String>,
    /// When set with [`Self::custom_memory_dir`], registers that path under this SQLite folder key
    /// (e.g. shared `cowork-ws-{id}`) instead of [`Self::agent_data_dir`].
    pub memory_folder_override: Option<String>,
}

impl Default for ZenCoreOptions {
    fn default() -> Self {
        Self {
            instance_id: String::new(),
            agent_data_dir: String::new(),
            working_dir: String::new(),
            use_tools: Vec::new(),
            skills_extra_dirs: Vec::new(),
            skip_file_edit_permission: false,
            skip_bash_exec_permission: false,
            skip_skill_permission: false,
            skip_mcp_tool_permission: false,
            skip_mcp_init: false,
            stream: true,
            thinking: true,
            system_prompt: String::new(),
            custom_rules: String::new(),
            enable_llm_cache: true,
            agent_mode: AgentMode::Agent,
            custom_memory_dir: None,
            memory_folder_override: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    Agent,
    Plan,
}

impl AgentMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            AgentMode::Agent => "Agent",
            AgentMode::Plan => "Plan",
        }
    }
}

// ============================================================================
// Model types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProfile {
    pub name: String,
    pub provider: String,
    #[serde(rename = "modelName")]
    pub model_name: String,
    #[serde(rename = "baseURL")]
    pub base_url: String,
    #[serde(rename = "apiKey")]
    pub api_key: String,
    #[serde(rename = "maxTokens")]
    pub max_tokens: u32,
    #[serde(rename = "contextLength")]
    pub context_length: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapt: Option<String>,
    /// Vision capability - explicit override. If None, inferred from model name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    #[serde(rename = "modelProfiles")]
    pub model_profiles: Vec<ModelProfile>,
    #[serde(rename = "modelPointers")]
    pub model_pointers: ModelPointers,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPointers {
    pub main: String,
    pub quick: String,
}

// ============================================================================
// Callback registry
// ============================================================================

/// Callback registry consumed by embedding layers (AgentPool adapter, etc.).
#[derive(Default)]
pub struct ZenCoreHandlers {
    pub on_session_ready: Option<Box<dyn Fn(SessionReadyData) + Send + Sync>>,
    pub on_message_complete: Option<Box<dyn Fn(MessageCompleteData) + Send + Sync>>,
    pub on_state_update: Option<Box<dyn Fn(StateUpdateData) + Send + Sync>>,
    pub on_session_error: Option<Box<dyn Fn(SessionErrorData) + Send + Sync>>,
    pub on_session_interrupted: Option<Box<dyn Fn(SessionInterruptedData) + Send + Sync>>,
    pub on_todos_update: Option<Box<dyn Fn(Vec<TodosUpdateItem>) + Send + Sync>>,
    pub on_conversation_usage: Option<Box<dyn Fn(ConversationUsageData) + Send + Sync>>,
    pub on_compact_start: Option<Box<dyn Fn(CompactStartData) + Send + Sync>>,
    pub on_compact_exec: Option<Box<dyn Fn(CompactExecData) + Send + Sync>>,
    pub on_tool_permission_request: Option<Box<dyn Fn(ToolPermissionRequestData) + Send + Sync>>,
    pub on_tool_execution_complete: Option<Box<dyn Fn(ToolExecutionCompleteData) + Send + Sync>>,
    pub on_tool_execution_error: Option<Box<dyn Fn(ToolExecutionErrorData) + Send + Sync>>,
    pub on_ask_question_request: Option<Box<dyn Fn(AskQuestionRequestData) + Send + Sync>>,
    pub on_plan_exit_request: Option<Box<dyn Fn(PlanExitRequestData) + Send + Sync>>,
    pub on_task_agent_start: Option<Box<dyn Fn(TaskAgentStartData) + Send + Sync>>,
    pub on_task_agent_end: Option<Box<dyn Fn(TaskAgentEndData) + Send + Sync>>,
    pub on_text_chunk: Option<Box<dyn Fn(TextChunkData) + Send + Sync>>,
    pub on_thinking_chunk: Option<Box<dyn Fn(ThinkingChunkData) + Send + Sync>>,
}

// ============================================================================
// ZenCore trait
// ============================================================================

/// zen-core trait designed for 1:1 compatibility with current sema-core call-sites.
///
/// P0 behavior expected by callers:
/// - listeners are registered before `process_user_input`
/// - `state:update:idle` resolves process-and-wait
/// - permission requests suspend tool execution until response arrives
pub trait ZenCore: Send + Sync {
    fn create_session(&self, session_id: Option<&str>) -> Result<()>;
    fn process_user_input(&self, prompt: &str, original_input: Option<&str>) -> Result<()>;
    fn pause_session(&self);
    fn interrupt_session(&self, target_state: SessionState);
    fn dispose(&self);

    fn set_working_dir(&self, dir: &str);
    fn clear_working_dir(&self);
    fn update_skip_permissions(&self, skip: bool);
    fn update_thinking(&self, enabled: bool);
    fn set_use_tools(&self, tools: Vec<String>);
    fn reload_skills(&self, disabled: &[String]);
    fn has_session_tool_results(&self) -> bool;

    fn add_or_update_mcp_server(&self, cfg: &McpServerConfig, scope: &str) -> Result<()>;
    /// Pre-seed the permission allowlist (never-ask-again) from stored group config.
    fn add_allowed_tool(&self, key: &str);
    fn respond_to_tool_permission(&self, response: ToolPermissionResponseData);
    fn respond_to_ask_question(&self, response: AskQuestionResponseData);
    fn respond_to_plan_exit(&self, response: PlanExitResponseData);

    fn set_handlers(&self, handlers: ZenCoreHandlers);
    fn update_agent_mode(&self, mode: AgentMode);
    fn get_tool_infos(&self) -> Vec<ToolInfo>;
}

/// Lightweight tool metadata exposed to UIs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub status: String,
}

// ============================================================================
// Shared message helpers
// ============================================================================

/// Create a user message from content blocks.
pub fn create_user_message(blocks: Vec<ContentBlock>) -> Message {
    Message {
        msg_type: "user".to_string(),
        message: MessagePayload {
            role: "user".to_string(),
            content: blocks,
        },
        uuid: uuid::Uuid::new_v4().to_string(),
        usage: None,
    }
}

/// Create a tool_result stop message (sent when a tool is interrupted).
pub fn create_tool_result_stop(tool_use_id: &str) -> ContentBlock {
    ContentBlock::ToolResult {
        tool_use_id: tool_use_id.to_string(),
        content: "Tool execution was interrupted.".to_string(),
        is_error: false,
    }
}

/// Normalize messages for API consumption (filter internal metadata).
pub fn normalize_messages_for_api(messages: &[Message]) -> Vec<Message> {
    messages
        .iter()
        .map(|m| {
            let mut new_m = m.clone();
            new_m
                .message
                .content
                .retain(|b| !matches!(b, ContentBlock::ControlSignal { .. }));
            new_m
        })
        .collect()
}
