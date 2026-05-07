//! AgentEventSink and CoreApi traits — abstract the agent runtime and Web UI event sink.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use anyhow::Result;

use super::types::{
    AskQuestionRequestData, CompactExecData, CompactStartData, MessageCompleteData,
    SessionErrorData, StateUpdateData, TodoSnapshot, TodosUpdateItem, ToolPermissionRequestData,
};
use crate::agent::permission_bridge::{AskQuestionPayload, PermissionPayload};
use crate::config::Config;
use crate::mcp::helper::McpServerConfig;
use crate::types::GroupBinding;

// ===== AgentEventSink trait (mirrors TS interface) =====

/// Sink for agent-side events that the WebSocket gateway forwards to the Web UI.
/// Default impls are no-ops so partial wiring compiles.
#[allow(unused_variables)]
pub trait AgentEventSink: Send + Sync {
    fn notify_agent_reply(&self, chat_jid: &str, text: &str);
    fn notify_agent_state(&self, chat_jid: &str, state: &str);
    fn notify_permission_request(
        &self,
        chat_jid: &str,
        request_id: &str,
        payload: PermissionPayload,
    ) {
    }
    fn notify_ask_question_request(
        &self,
        chat_jid: &str,
        request_id: &str,
        payload: AskQuestionPayload,
    ) {
    }
    fn notify_permission_resolved(
        &self,
        chat_jid: &str,
        request_id: &str,
        option_key: &str,
        option_label: &str,
    ) {
    }
    fn notify_ask_question_resolved(
        &self,
        chat_jid: &str,
        request_id: &str,
        answers: HashMap<String, String>,
    ) {
    }
    fn notify_agent_todos(&self, agent_jid: &str, agent_name: &str, todos: &[TodoSnapshot]) {}
    fn notify_agent_compacting(&self, chat_jid: &str, is_compacting: bool) {}
    /// Broadcast the current tool roster for an agent (sent on agent creation
    /// and on snapshot replay for new admin clients).
    fn notify_agent_tools(&self, agent_jid: &str, agent_name: &str, tools: &[AgentToolInfo]) {}
    fn notify_agent_usage(&self, agent_jid: &str, usage: crate::zen_core::ConversationUsageData) {}
}

/// Lightweight tool descriptor exposed to the Web UI through `agent:tools`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolInfo {
    pub name: String,
    pub description: String,
    pub status: String,
}

/// Per-agent cached tool roster — used for snapshot replay on WS subscribe.
#[derive(Debug, Clone, Serialize)]
pub struct CachedTools {
    #[serde(rename = "agentName")]
    pub agent_name: String,
    pub tools: Vec<AgentToolInfo>,
}

// ===== CoreApi trait (extended) =====

/// Operations AgentPool needs from the agent runtime (sema-core or stub).
/// All methods default to no-op / error so partial wiring compiles.
#[allow(unused_variables)]
pub trait CoreApi: Send + Sync {
    /// Process a user prompt synchronously (Phase 1 stub path).
    fn process_message(&self, jid: &str, prompt: &str, group: &GroupBinding) -> Result<String> {
        Err(anyhow::anyhow!("CoreApi not wired — sema-core unavailable"))
    }

    /// Tear down the core for a JID.
    fn destroy_agent(&self, jid: &str) {}

    /// Restrict the tool whitelist for an existing core (empty = all tools).
    /// Called by AgentPool after computing use_tools from binding.allowed_tools.
    fn set_use_tools(&self, _jid: &str, _tools: Vec<String>) {}

    /// Hot-update skip-permission flags for an existing core.
    fn update_skip_permissions(&self, jid: &str, skip: bool) {}

    /// Hot-update Thinking-mode flag for an existing core.
    fn update_thinking(&self, jid: &str, enabled: bool) {}

    /// Switch the core's working directory (used by workspace_switch and dispatch).
    fn set_working_dir(&self, jid: &str, dir: &str) {}

    /// Reset working directory to the core's compile-time default.
    fn clear_working_dir(&self, jid: &str) {}

    /// Pause the live session (Phase 3).
    fn pause_session(&self, jid: &str) {}

    /// Soft-interrupt the session, preserving history (Phase 3).
    fn interrupt_session(&self, jid: &str) {}

    /// Reload skills registry across all cores after disable/enable.
    fn reload_skills(&self, disabled: &[String]) {}

    /// Runtime config used by core backend implementation.
    fn set_runtime_config(&self, _cfg: Arc<Config>) {}

    /// Register or update one MCP server for this core.
    fn add_or_update_mcp_server(&self, _jid: &str, _cfg: &McpServerConfig) -> Result<()> {
        Ok(())
    }

    /// Pre-seed the permission allowlist (never-ask-again) for a JID.
    /// Called on engine creation to load persisted group allowed_tools.
    fn add_allowed_tool(&self, _jid: &str, _tool: &str) {}

    /// Recreate session after stop (discards context, fresh session). Default no-op.
    fn create_session(&self, _jid: &str) -> Result<()> {
        Ok(())
    }

    /// Send user input to a running session (used by resume_agent). Default no-op.
    fn process_user_input(&self, _jid: &str, _prompt: &str) -> Result<()> {
        Ok(())
    }

    /// Whether the session has pending tool-call results. Default false.
    fn has_session_tool_results(&self, _jid: &str) -> bool {
        false
    }

    /// Register event listeners on the underlying core.
    /// Default no-ops — real sema-core will implement these.
    fn on_message_complete(
        &self,
        _jid: &str,
        _handler: Box<dyn Fn(MessageCompleteData) + Send + Sync>,
    ) {
    }
    fn on_state_update(&self, _jid: &str, _handler: Box<dyn Fn(StateUpdateData) + Send + Sync>) {}
    fn on_todos_update(
        &self,
        _jid: &str,
        _handler: Box<dyn Fn(Vec<TodosUpdateItem>) + Send + Sync>,
    ) {
    }
    fn on_compact_start(&self, _jid: &str, _handler: Box<dyn Fn(CompactStartData) + Send + Sync>) {}
    fn on_compact_exec(&self, _jid: &str, _handler: Box<dyn Fn(CompactExecData) + Send + Sync>) {}
    fn on_session_error(&self, _jid: &str, _handler: Box<dyn Fn(SessionErrorData) + Send + Sync>) {}
    fn on_tool_permission_request(
        &self,
        _jid: &str,
        _handler: Box<dyn Fn(ToolPermissionRequestData) + Send + Sync>,
    ) {
    }
    fn on_ask_question_request(
        &self,
        _jid: &str,
        _handler: Box<dyn Fn(AskQuestionRequestData) + Send + Sync>,
    ) {
    }
    fn on_conversation_usage(
        &self,
        _jid: &str,
        _handler: Box<dyn Fn(crate::zen_core::ConversationUsageData) + Send + Sync>,
    ) {
    }
    fn respond_to_tool_permission(
        &self,
        _jid: &str,
        _tool_name: &str,
        _selected: &str,
    ) -> Result<()> {
        Ok(())
    }
    fn respond_to_ask_question(
        &self,
        _jid: &str,
        _agent_id: &str,
        _answers: HashMap<String, String>,
    ) -> Result<()> {
        Ok(())
    }
    /// Remove all event listeners registered for `jid`.
    fn off_all(&self, _jid: &str) {}

    /// Snapshot the tool roster currently registered on the underlying engine
    /// for `jid`. Default implementation returns an empty list.
    fn get_tool_infos(&self, _jid: &str) -> Vec<AgentToolInfo> {
        Vec::new()
    }
}

/// Per-agent collection of registered event handlers, stored in [`ZenCoreApi`].
#[derive(Default, Clone)]
pub(crate) struct CoreHandlers {
    pub message_complete: Option<Arc<dyn Fn(MessageCompleteData) + Send + Sync>>,
    pub state_update: Option<Arc<dyn Fn(StateUpdateData) + Send + Sync>>,
    pub todos_update: Option<Arc<dyn Fn(Vec<TodosUpdateItem>) + Send + Sync>>,
    pub compact_start: Option<Arc<dyn Fn(CompactStartData) + Send + Sync>>,
    pub compact_exec: Option<Arc<dyn Fn(CompactExecData) + Send + Sync>>,
    pub session_error: Option<Arc<dyn Fn(SessionErrorData) + Send + Sync>>,
    pub tool_permission_request: Option<Arc<dyn Fn(ToolPermissionRequestData) + Send + Sync>>,
    pub ask_question_request: Option<Arc<dyn Fn(AskQuestionRequestData) + Send + Sync>>,
    pub conversation_usage:
        Option<Arc<dyn Fn(crate::zen_core::ConversationUsageData) + Send + Sync>>,
}
