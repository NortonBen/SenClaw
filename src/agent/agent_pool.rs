//! Agent pool — core agent lifecycle management.
//! Port target: src-old/agent/AgentPool.ts (1391 lines).
//!
//! Phase 1 ported (this file): state maps, [`AgentEventSink`] trait, extended
//! [`CoreApi`] trait, simple setters, dispatch coordination, permission /
//! thinking hot-update, [`AgentPool::broadcast_reply`], `notify_activity`,
//! workspace state file helpers.
//!
//! Phase 2+ pending: `get_or_create` with concurrency lock, `process_and_wait`
//! with inactivity timer + retry / abort, `destroy` / `destroy_all`,
//! `stop_agent` / `pause_agent` / `resume_agent`, `run_isolated`, `bind_events`
//! event wiring, skills hot-reload signal watcher, workspace state file
//! watcher.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::agent::dispatch_bridge::{
    build_dispatch_resume_hint, AdminActivityCallback, DispatchBridgeApi,
};
use crate::agent::group_queue::GroupQueue;
use crate::agent::permission_bridge::{
    AskQuestionData, AskQuestionOption, AskQuestionPayload, PermissionBridge, PermissionPayload,
};
use crate::agent::session_bridge;
use crate::config::Config;
use crate::db::Db;
use crate::zen_core::engine::ZenEngine;
use crate::zen_core::{
    AskQuestionResponseData, EngineEvent, SessionState, ToolPermissionResponseData,
    ZenCore, ZenCoreOptions,
};
use tokio::sync::broadcast::error::RecvError;
use crate::gateway::message_router::AgentApi;
use crate::mcp::helper::{
    dispatch_mcp_config, feishu_wiki_mcp_config, memory_mcp_config, schedule_mcp_config,
    send_mcp_config, workspace_mcp_config, McpServerConfig,
};
use crate::memory::daily_logger::DailyLogger;
use crate::types::GroupBinding;
use crate::util::local_time::local_iso_string_now;

// ===== Constants =====

/// Agent ID emitted by sema-core for the main (root) agent. Subagent events
/// carry a different id and are filtered out.
#[allow(dead_code)] // wired by Phase 2 bind_events
pub(crate) const MAIN_AGENT_ID: &str = "main";

/// `process_and_wait` inactivity timeout (30 minutes). Must exceed the longest
/// dispatch_task runtime so chained tool calls don't trip the watchdog.
pub const AGENT_TIMEOUT_MS: u64 = 30 * 60 * 1000;

// ===== Public payload types =====

/// Permission flags surfaced to the Web UI / virtual workers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PermissionsConfig {
    #[serde(rename = "skipMainAgentPermissions")]
    pub skip_main_agent_permissions: bool,
    #[serde(rename = "skipAllAgentsPermissions")]
    pub skip_all_agents_permissions: bool,
}

impl Default for PermissionsConfig {
    fn default() -> Self {
        Self {
            skip_main_agent_permissions: false,
            skip_all_agents_permissions: false,
        }
    }
}

/// One TodoWrite item snapshot — cached for replay on WS subscribe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoSnapshot {
    pub content: String,
    pub status: String,
    #[serde(default, rename = "activeForm", skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
}

/// Per-agent cached todos snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct CachedTodos {
    #[serde(rename = "agentName")]
    pub agent_name: String,
    pub todos: Vec<TodoSnapshot>,
}

// ===== Event data types (mirrors TS sema-core events) =====

/// `message:complete` event payload.
#[derive(Debug, Clone)]
pub struct MessageCompleteData {
    pub agent_id: String,
    pub content: String,
}

/// `state:update` event payload.
#[derive(Debug, Clone)]
pub struct StateUpdateData {
    pub state: String,
}

/// `todos:update` event payload — list of todo items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodosUpdateItem {
    pub content: String,
    pub status: String,
    #[serde(default, rename = "activeForm", skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
}

/// `compact:start` event payload.
#[derive(Debug, Clone)]
pub struct CompactStartData;

/// `compact:exec` event payload.
#[derive(Debug, Clone)]
pub struct CompactExecData;

/// `session:error` event payload.
#[derive(Debug, Clone)]
pub struct SessionErrorData {
    pub code: String,
    pub message: String,
}

/// `tool:permission:request` event payload.
#[derive(Debug, Clone)]
pub struct ToolPermissionRequestData {
    pub tool_name: String,
    pub title: String,
    pub content: serde_json::Value,
    pub options: HashMap<String, String>,
}

/// `ask:question:request` event payload.
#[derive(Debug, Clone)]
pub struct AskQuestionRequestData {
    pub agent_id: String,
    pub questions: Vec<AskQuestionData>,
}

/// Events forwarded from `bind_events` persistent handlers to an active
/// `process_and_wait` event loop. Sent through the unbounded channel stored
/// in [`State::process_event_txs`].
#[derive(Debug, Clone)]
enum ProcessEvent {
    /// Core reached idle — resolve the PAW promise.
    Idle,
    /// Core emitted a session error — trigger error handling.
    Error(SessionErrorData),
    /// Non-idle, non-paused state update — restart the inactivity timer.
    Reset,
}

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
    fn process_message(
        &self,
        jid: &str,
        prompt: &str,
        group: &GroupBinding,
    ) -> Result<String> {
        Err(anyhow::anyhow!("CoreApi not wired — sema-core unavailable"))
    }

    /// Tear down the core for a JID.
    fn destroy_agent(&self, jid: &str) {}

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
    fn on_state_update(
        &self,
        _jid: &str,
        _handler: Box<dyn Fn(StateUpdateData) + Send + Sync>,
    ) {
    }
    fn on_todos_update(
        &self,
        _jid: &str,
        _handler: Box<dyn Fn(Vec<TodosUpdateItem>) + Send + Sync>,
    ) {
    }
    fn on_compact_start(
        &self,
        _jid: &str,
        _handler: Box<dyn Fn(CompactStartData) + Send + Sync>,
    ) {
    }
    fn on_compact_exec(
        &self,
        _jid: &str,
        _handler: Box<dyn Fn(CompactExecData) + Send + Sync>,
    ) {
    }
    fn on_session_error(
        &self,
        _jid: &str,
        _handler: Box<dyn Fn(SessionErrorData) + Send + Sync>,
    ) {
    }
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

#[derive(Default, Clone)]
struct CoreHandlers {
    message_complete: Option<Arc<dyn Fn(MessageCompleteData) + Send + Sync>>,
    state_update: Option<Arc<dyn Fn(StateUpdateData) + Send + Sync>>,
    todos_update: Option<Arc<dyn Fn(Vec<TodosUpdateItem>) + Send + Sync>>,
    compact_start: Option<Arc<dyn Fn(CompactStartData) + Send + Sync>>,
    compact_exec: Option<Arc<dyn Fn(CompactExecData) + Send + Sync>>,
    session_error: Option<Arc<dyn Fn(SessionErrorData) + Send + Sync>>,
    tool_permission_request: Option<Arc<dyn Fn(ToolPermissionRequestData) + Send + Sync>>,
    ask_question_request: Option<Arc<dyn Fn(AskQuestionRequestData) + Send + Sync>>,
}

// ===== ZenCoreApi: real zen-core engine bridge =====

/// Production [`CoreApi`] backed by [`ZenEngine`] (the zen-core runtime).
///
/// Manages one [`ZenEngine`] per JID, bridges engine events to CoreApi
/// handler callbacks, and delegates lifecycle operations to the engine.
pub struct ZenCoreApi {
    engines: Mutex<HashMap<String, Arc<ZenEngine>>>,
    handlers: Arc<Mutex<HashMap<String, CoreHandlers>>>,
    http_client: Client,
}

impl ZenCoreApi {
    pub fn new() -> Self {
        Self {
            engines: Mutex::new(HashMap::new()),
            handlers: Arc::new(Mutex::new(HashMap::new())),
            http_client: Client::builder()
                .connect_timeout(Duration::from_secs(30))
                .timeout(Duration::from_secs(120))
                .build()
                .expect("ZenCoreApi http client"),
        }
    }

    fn get_handlers(&self, jid: &str) -> Option<CoreHandlers> {
        self.handlers.lock().unwrap().get(jid).cloned()
    }

    fn with_handlers<F: FnOnce(&mut CoreHandlers)>(&self, jid: &str, f: F) {
        let mut map = self.handlers.lock().unwrap();
        let entry = map.entry(jid.to_string()).or_default();
        f(entry);
    }

    /// Create or retrieve the engine for a JID.
    fn ensure_engine(&self, jid: &str) -> Arc<ZenEngine> {
        let mut engines = self.engines.lock().unwrap();
        if let Some(engine) = engines.get(jid) {
            return engine.clone();
        }
        let opts = ZenCoreOptions {
            instance_id: jid.to_string(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts);
        engine.register_tools(crate::tools::all_tools());
        engines.insert(jid.to_string(), engine.clone());
        engine
    }

    /// Subscribe to the engine's EventBus and forward events to handlers.
    fn bridge_events(&self, jid: &str, engine: &Arc<ZenEngine>) {
        let jid = jid.to_string();
        let handlers_map = Arc::clone(&self.handlers);
        let mut rx = engine.event_bus.subscribe();

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let handlers = handlers_map.lock().unwrap()
                            .get(&jid).cloned();
                        let h = match handlers {
                            Some(h) => h,
                            None => continue,
                        };
                        match event {
                            EngineEvent::MessageComplete(data) => {
                                if let Some(ref cb) = h.message_complete {
                                    cb(MessageCompleteData {
                                        agent_id: data.agent_id,
                                        content: data.content,
                                    });
                                }
                            }
                            EngineEvent::StateUpdate(data) => {
                                if let Some(ref cb) = h.state_update {
                                    cb(StateUpdateData {
                                        state: data.state.as_str().to_string(),
                                    });
                                }
                            }
                            EngineEvent::TodosUpdate(items) => {
                                if let Some(ref cb) = h.todos_update {
                                    cb(items.iter().map(|item| TodosUpdateItem {
                                        content: item.content.clone(),
                                        status: item.status.clone(),
                                        active_form: item.active_form.clone(),
                                    }).collect());
                                }
                            }
                            EngineEvent::CompactStart(_) => {
                                if let Some(ref cb) = h.compact_start {
                                    cb(CompactStartData);
                                }
                            }
                            EngineEvent::CompactExec(_) => {
                                if let Some(ref cb) = h.compact_exec {
                                    cb(CompactExecData);
                                }
                            }
                            EngineEvent::SessionError(data) => {
                                if let Some(ref cb) = h.session_error {
                                    cb(SessionErrorData {
                                        code: data.error.code,
                                        message: data.error.message,
                                    });
                                }
                            }
                            EngineEvent::ToolPermissionRequest(data) => {
                                if let Some(ref cb) = h.tool_permission_request {
                                    cb(ToolPermissionRequestData {
                                        tool_name: data.tool_name,
                                        title: data.title,
                                        content: data.content,
                                        options: data.options,
                                    });
                                }
                            }
                            EngineEvent::AskQuestionRequest(data) => {
                                if let Some(ref cb) = h.ask_question_request {
                                    cb(AskQuestionRequestData {
                                        agent_id: data.agent_id,
                                        questions: data.questions.into_iter().map(
                                            |q| AskQuestionData {
                                                question: q.question,
                                                header: q.header,
                                                options: q.options.into_iter().map(
                                                    |o| AskQuestionOption {
                                                        label: o.label,
                                                        description: o.description,
                                                    }
                                                ).collect(),
                                                multi_select: q.multi_select,
                                            }
                                        ).collect(),
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!(
                            "[ZenCoreApi] event bus lagged by {} for {}",
                            n, jid
                        );
                        continue;
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }
}

impl CoreApi for ZenCoreApi {
    fn process_message(
        &self,
        jid: &str,
        prompt: &str,
        _group: &GroupBinding,
    ) -> Result<String> {
        let engine = self.ensure_engine(jid);
        engine.process_user_input(prompt, None)?;
        Ok("Dispatched to zen-core".to_string())
    }

    fn destroy_agent(&self, jid: &str) {
        let engine = self.engines.lock().unwrap().remove(jid);
        if let Some(e) = engine {
            e.dispose();
        }
        self.handlers.lock().unwrap().remove(jid);
    }

    fn update_skip_permissions(&self, jid: &str, skip: bool) {
        if let Some(engine) = self.engines.lock().unwrap().get(jid) {
            engine.update_skip_permissions(skip);
        }
    }

    fn update_thinking(&self, jid: &str, enabled: bool) {
        if let Some(engine) = self.engines.lock().unwrap().get(jid) {
            engine.update_thinking(enabled);
        }
    }

    fn set_working_dir(&self, jid: &str, dir: &str) {
        if let Some(engine) = self.engines.lock().unwrap().get(jid) {
            engine.set_working_dir(dir);
        }
    }

    fn clear_working_dir(&self, jid: &str) {
        if let Some(engine) = self.engines.lock().unwrap().get(jid) {
            engine.clear_working_dir();
        }
    }

    fn pause_session(&self, jid: &str) {
        if let Some(engine) = self.engines.lock().unwrap().get(jid) {
            engine.pause_session();
        }
    }

    fn interrupt_session(&self, jid: &str) {
        if let Some(engine) = self.engines.lock().unwrap().get(jid) {
            engine.interrupt_session(SessionState::Idle);
        }
    }

    fn reload_skills(&self, disabled: &[String]) {
        for engine in self.engines.lock().unwrap().values() {
            engine.reload_skills(disabled);
        }
    }

    fn set_runtime_config(&self, _cfg: Arc<Config>) {
        // Config is passed via environment; no-op for zen-core
    }

    fn add_or_update_mcp_server(&self, jid: &str, cfg: &McpServerConfig) -> Result<()> {
        let engine = self.ensure_engine(jid);
        let zc_cfg = crate::zen_core::McpServerConfig {
            name: cfg.name.clone(),
            command: cfg.command.clone(),
            args: cfg.args.clone(),
            env: cfg.env.clone(),
        };
        engine.add_or_update_mcp_server(&zc_cfg, "project")?;
        Ok(())
    }

    fn create_session(&self, jid: &str) -> Result<()> {
        let engine = self.ensure_engine(jid);
        self.bridge_events(jid, &engine);
        engine.create_session(None)?;
        Ok(())
    }

    fn get_tool_infos(&self, jid: &str) -> Vec<AgentToolInfo> {
        let engine = self
            .engines
            .lock()
            .unwrap()
            .get(jid)
            .cloned();
        let Some(engine) = engine else { return Vec::new() };
        engine
            .get_tool_infos()
            .into_iter()
            .map(|t| AgentToolInfo {
                name: t.name,
                description: t.description,
                status: t.status,
            })
            .collect()
    }

    fn process_user_input(&self, jid: &str, prompt: &str) -> Result<()> {
        let engine = self.ensure_engine(jid);
        engine.process_user_input(prompt, None)?;
        Ok(())
    }

    fn has_session_tool_results(&self, jid: &str) -> bool {
        self.engines.lock().unwrap()
            .get(jid)
            .map(|e| e.has_session_tool_results())
            .unwrap_or(false)
    }

    fn on_message_complete(
        &self,
        jid: &str,
        handler: Box<dyn Fn(MessageCompleteData) + Send + Sync>,
    ) {
        self.with_handlers(jid, |entry| {
            entry.message_complete = Some(Arc::from(handler));
        });
    }

    fn on_state_update(
        &self,
        jid: &str,
        handler: Box<dyn Fn(StateUpdateData) + Send + Sync>,
    ) {
        self.with_handlers(jid, |entry| {
            entry.state_update = Some(Arc::from(handler));
        });
    }

    fn on_todos_update(
        &self,
        jid: &str,
        handler: Box<dyn Fn(Vec<TodosUpdateItem>) + Send + Sync>,
    ) {
        self.with_handlers(jid, |entry| {
            entry.todos_update = Some(Arc::from(handler));
        });
    }

    fn on_compact_start(
        &self,
        jid: &str,
        handler: Box<dyn Fn(CompactStartData) + Send + Sync>,
    ) {
        self.with_handlers(jid, |entry| {
            entry.compact_start = Some(Arc::from(handler));
        });
    }

    fn on_compact_exec(
        &self,
        jid: &str,
        handler: Box<dyn Fn(CompactExecData) + Send + Sync>,
    ) {
        self.with_handlers(jid, |entry| {
            entry.compact_exec = Some(Arc::from(handler));
        });
    }

    fn on_session_error(
        &self,
        jid: &str,
        handler: Box<dyn Fn(SessionErrorData) + Send + Sync>,
    ) {
        self.with_handlers(jid, |entry| {
            entry.session_error = Some(Arc::from(handler));
        });
    }

    fn on_tool_permission_request(
        &self,
        jid: &str,
        handler: Box<dyn Fn(ToolPermissionRequestData) + Send + Sync>,
    ) {
        self.with_handlers(jid, |entry| {
            entry.tool_permission_request = Some(Arc::from(handler));
        });
    }

    fn on_ask_question_request(
        &self,
        jid: &str,
        handler: Box<dyn Fn(AskQuestionRequestData) + Send + Sync>,
    ) {
        self.with_handlers(jid, |entry| {
            entry.ask_question_request = Some(Arc::from(handler));
        });
    }

    fn respond_to_tool_permission(
        &self,
        jid: &str,
        tool_name: &str,
        selected: &str,
    ) -> Result<()> {
        if let Some(engine) = self.engines.lock().unwrap().get(jid) {
            engine.respond_to_tool_permission(ToolPermissionResponseData {
                tool_name: tool_name.to_string(),
                selected: selected.to_string(),
            });
        }
        Ok(())
    }

    fn respond_to_ask_question(
        &self,
        jid: &str,
        _agent_id: &str,
        answers: HashMap<String, String>,
    ) -> Result<()> {
        if let Some(engine) = self.engines.lock().unwrap().get(jid) {
            engine.respond_to_ask_question(AskQuestionResponseData {
                agent_id: _agent_id.to_string(),
                answers,
            });
        }
        Ok(())
    }

    fn off_all(&self, jid: &str) {
        self.with_handlers(jid, |entry| {
            *entry = CoreHandlers::default();
        });
    }
}

// ===== Callback type aliases =====

/// Reply callback (jid, text). Used by the WebSocket gateway path before
/// `set_send_reply` lands.
pub type ReplyFn = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Channel send callback (jid, text, bot_token). Replaces ReplyFn for
/// channel-bound replies once message_router wires it up (Phase 2).
pub type SendReplyFn = Arc<dyn Fn(&str, &str, Option<&str>) + Send + Sync>;

/// Inactivity-timer reset closure stored per JID during process_and_wait.
type ActivityResetFn = Arc<dyn Fn() + Send + Sync>;

/// Abort callback stored per JID — invoked on `destroy()` to break a pending
/// process_and_wait promise.
type AbortFn = Box<dyn FnOnce(&str) + Send>;

/// Cleanup callback stored per JID — removes persistent event listeners.
type CleanupFn = Box<dyn FnOnce() + Send>;

/// Workspace-state-file unwatch callback.
type UnwatchFn = Box<dyn FnOnce() + Send>;

// ===== Internal state =====

struct State {
    /// JIDs with an active core (real type is hidden behind CoreApi).
    cores: HashSet<String>,
    /// jid → binding snapshot.
    bindings: HashMap<String, GroupBinding>,

    // permission / thinking flags (runtime mirror of config.json).
    skip_main_agent_permissions: bool,
    skip_all_agents_permissions: bool,
    thinking_enabled: bool,

    // workspace tracking.
    runtime_work_dirs: HashMap<String, String>,
    workspace_watchers: HashMap<String, UnwatchFn>,

    // dispatch coordination.
    dispatch_workspace_overrides: HashMap<String, String>,
    dispatch_executing: HashSet<String>,
    last_dispatch_replies: HashMap<String, String>,
    dispatch_task_map: HashMap<String, String>,

    // process_and_wait event bridge (per-jid) — sender set by PAW before
    // process_user_input, forwarded to by bind_events persistent handlers.
    process_event_txs:
        HashMap<String, tokio::sync::mpsc::UnboundedSender<ProcessEvent>>,

    // process_and_wait runtime state.
    active_timer_resets: HashMap<String, ActivityResetFn>,
    active_aborts: HashMap<String, AbortFn>,
    event_cleanups: HashMap<String, CleanupFn>,

    // todos cache + create-lock + pause sets.
    cached_todos: HashMap<String, CachedTodos>,
    cached_tools: HashMap<String, CachedTools>,
    pending_creates: HashSet<String>,
    paused_children_by_admin: HashMap<String, Vec<String>>,
    synth_paused_jids: HashSet<String>,
    dispatch_paused_jids: HashSet<String>,
}

impl State {
    fn new() -> Self {
        Self {
            cores: HashSet::new(),
            bindings: HashMap::new(),
            skip_main_agent_permissions: false,
            skip_all_agents_permissions: false,
            thinking_enabled: true,
            runtime_work_dirs: HashMap::new(),
            workspace_watchers: HashMap::new(),
            dispatch_workspace_overrides: HashMap::new(),
            dispatch_executing: HashSet::new(),
            last_dispatch_replies: HashMap::new(),
            dispatch_task_map: HashMap::new(),
            process_event_txs: HashMap::new(),
            active_timer_resets: HashMap::new(),
            active_aborts: HashMap::new(),
            event_cleanups: HashMap::new(),
            cached_todos: HashMap::new(),
            cached_tools: HashMap::new(),
            pending_creates: HashSet::new(),
            paused_children_by_admin: HashMap::new(),
            synth_paused_jids: HashSet::new(),
            dispatch_paused_jids: HashSet::new(),
        }
    }
}

// ===== AgentPool =====

pub struct AgentPool {
    core_api: Arc<dyn CoreApi>,
    state: Mutex<State>,

    // Optional dependencies wired after construction so lib.rs's existing
    // `AgentPool::new(core_api)` call still compiles.
    on_reply: Mutex<Option<ReplyFn>>,
    send_reply: Mutex<Option<SendReplyFn>>,
    permission_bridge: Mutex<Option<Arc<PermissionBridge>>>,
    daily_logger: Mutex<Option<Arc<DailyLogger>>>,
    agent_event_sink: Mutex<Option<Arc<dyn AgentEventSink>>>,
    dispatch_bridge: Mutex<Option<Arc<dyn DispatchBridgeApi>>>,
    group_queue: Mutex<Option<Arc<GroupQueue>>>,

    /// `~/.senclaw/` — workspace state files live here.
    senclaw_home: Mutex<PathBuf>,

    /// DB handle — used by resume_agent to rebuild prompts from history.
    db: Mutex<Option<Arc<Db>>>,

    /// Runtime config mirror used by get_or_create for MCP server wiring.
    config: Mutex<Option<Arc<Config>>>,

    /// Weak self pointer so `&self` paths can upgrade to `Arc<Self>`
    /// when wiring long-lived callback closures (e.g. bind_events).
    self_weak: Mutex<Weak<AgentPool>>,
}

impl AgentPool {
    pub fn new(core_api: Arc<dyn CoreApi>) -> Arc<Self> {
        let default_home = dirs::home_dir()
            .map(|h| h.join(".senclaw"))
            .unwrap_or_else(|| PathBuf::from(".senclaw"));
        let pool = Arc::new(Self {
            core_api,
            state: Mutex::new(State::new()),
            on_reply: Mutex::new(None),
            send_reply: Mutex::new(None),
            permission_bridge: Mutex::new(None),
            daily_logger: Mutex::new(None),
            agent_event_sink: Mutex::new(None),
            dispatch_bridge: Mutex::new(None),
            group_queue: Mutex::new(None),
            senclaw_home: Mutex::new(default_home),
            db: Mutex::new(None),
            config: Mutex::new(None),
            self_weak: Mutex::new(Weak::new()),
        });
        *pool.self_weak.lock().unwrap() = Arc::downgrade(&pool);
        pool
    }

    // ===== Dependency injection setters =====

    /// Web-UI reply callback — called by `broadcast_reply` for WS push.
    pub fn set_reply_callback(&self, f: ReplyFn) {
        *self.on_reply.lock().unwrap() = Some(f);
    }

    /// Channel send callback — wired by daemon so channel-bound replies bypass
    /// the WS-only reply path.
    pub fn set_send_reply(&self, f: SendReplyFn) {
        *self.send_reply.lock().unwrap() = Some(f);
    }

    pub fn set_permission_bridge(&self, bridge: Arc<PermissionBridge>) {
        // Reset inactivity timer while waiting on permission interactions.
        let weak = self.self_weak.lock().unwrap().clone();
        bridge.set_activity_callback(move |jid: &str| {
            if let Some(pool) = weak.upgrade() {
                pool.notify_activity(jid);
            }
        });
        *self.permission_bridge.lock().unwrap() = Some(bridge);
    }

    pub fn set_daily_logger(&self, logger: Arc<DailyLogger>) {
        *self.daily_logger.lock().unwrap() = Some(logger);
    }

    /// `~/.senclaw/` — overrides the home-dir default.
    pub fn set_senclaw_home(&self, dir: PathBuf) {
        *self.senclaw_home.lock().unwrap() = dir;
    }

    pub fn set_group_queue(&self, queue: Arc<GroupQueue>) {
        *self.group_queue.lock().unwrap() = Some(queue);
    }

    /// DB handle — used by resume_agent to rebuild prompts from history.
    pub fn set_db(&self, db: Arc<Db>) {
        *self.db.lock().unwrap() = Some(db);
    }

    /// Runtime config used by MCP registration in get_or_create.
    pub fn set_config(&self, cfg: Arc<Config>) {
        *self.config.lock().unwrap() = Some(Arc::clone(&cfg));
        self.core_api.set_runtime_config(cfg);
    }

    /// Inject the dispatch bridge and forward its admin-activity callback into
    /// `notify_activity`, mirroring TS `setDispatchBridge`.
    pub fn set_dispatch_bridge(self: &Arc<Self>, bridge: Arc<dyn DispatchBridgeApi>) {
        let weak = Arc::downgrade(self);
        let cb: AdminActivityCallback = Arc::new(move |admin_folder: &str| {
            let Some(pool) = weak.upgrade() else { return };
            let jid = {
                let s = pool.state.lock().unwrap();
                s.bindings
                    .iter()
                    .find(|(_, b)| b.folder == admin_folder)
                    .map(|(j, _)| j.clone())
            };
            if let Some(jid) = jid {
                pool.notify_activity(&jid);
            }
        });
        bridge.set_admin_activity_callback(cb);
        *self.dispatch_bridge.lock().unwrap() = Some(bridge);
    }

    /// Snapshot the currently-installed [`DispatchBridgeApi`] (if any).
    pub fn dispatch_bridge_snapshot(&self) -> Option<Arc<dyn DispatchBridgeApi>> {
        self.dispatch_bridge.lock().unwrap().clone()
    }

    /// Wire WsGateway sink + connect PermissionBridge callbacks to it.
    pub fn set_agent_event_sink(&self, sink: Arc<dyn AgentEventSink>) {
        if let Some(bridge) = self.permission_bridge.lock().unwrap().as_ref() {
            let s1 = Arc::clone(&sink);
            bridge.set_permission_request_callback(
                move |chat_jid: &str, req_id: &str, payload: PermissionPayload| {
                    s1.notify_permission_request(chat_jid, req_id, payload);
                },
            );
            let s2 = Arc::clone(&sink);
            bridge.set_ask_question_request_callback(
                move |chat_jid: &str, req_id: &str, payload: AskQuestionPayload| {
                    s2.notify_ask_question_request(chat_jid, req_id, payload);
                },
            );
            let s3 = Arc::clone(&sink);
            bridge.set_permission_resolved_callback(
                move |chat_jid: &str, req_id: &str, key: &str, label: &str| {
                    s3.notify_permission_resolved(chat_jid, req_id, key, label);
                },
            );
            let s4 = Arc::clone(&sink);
            bridge.set_ask_question_resolved_callback(
                move |chat_jid: &str, req_id: &str, answers: HashMap<String, String>| {
                    s4.notify_ask_question_resolved(chat_jid, req_id, answers);
                },
            );
        }
        *self.agent_event_sink.lock().unwrap() = Some(sink);
    }

    // ===== Permission / Thinking config =====

    pub fn get_permissions_config(&self) -> PermissionsConfig {
        let s = self.state.lock().unwrap();
        PermissionsConfig {
            skip_main_agent_permissions: s.skip_main_agent_permissions,
            skip_all_agents_permissions: s.skip_all_agents_permissions,
        }
    }

    /// Virtual agents inherit the main-agent permission flags.
    pub fn get_skip_perms_for_virtual(&self) -> bool {
        let s = self.state.lock().unwrap();
        s.skip_all_agents_permissions || s.skip_main_agent_permissions
    }

    /// Hot-update permission flags across every active core.
    pub fn set_permissions_config(&self, opts: PermissionsConfig) {
        let updates: Vec<(String, bool)> = {
            let mut s = self.state.lock().unwrap();
            s.skip_main_agent_permissions = opts.skip_main_agent_permissions;
            s.skip_all_agents_permissions = opts.skip_all_agents_permissions;
            let dispatch_set: HashSet<String> =
                s.dispatch_workspace_overrides.keys().cloned().collect();
            s.bindings
                .iter()
                .filter(|(jid, _)| s.cores.contains(*jid))
                .map(|(jid, b)| {
                    (
                        jid.clone(),
                        Self::compute_skip_perms(&opts, b, &dispatch_set),
                    )
                })
                .collect()
        };
        let n = updates.len();
        for (jid, skip) in &updates {
            self.core_api.update_skip_permissions(jid, *skip);
        }
        tracing::info!(
            "[AgentPool] Permissions updated (skipMain={}, skipAll={}), hot-updated {} agent(s)",
            opts.skip_main_agent_permissions,
            opts.skip_all_agents_permissions,
            n
        );
    }

    /// Hot-update Thinking switch on every active core.
    pub fn set_thinking_enabled(&self, enabled: bool) {
        let cores: Vec<String> = {
            let mut s = self.state.lock().unwrap();
            s.thinking_enabled = enabled;
            s.cores.iter().cloned().collect()
        };
        let n = cores.len();
        for jid in &cores {
            self.core_api.update_thinking(jid, enabled);
        }
        tracing::info!(
            "[AgentPool] Thinking mode {}, hot-updated {} agent(s)",
            if enabled { "enabled" } else { "disabled" },
            n
        );
    }

    pub fn get_thinking_enabled(&self) -> bool {
        self.state.lock().unwrap().thinking_enabled
    }

    fn compute_skip_perms(
        opts: &PermissionsConfig,
        binding: &GroupBinding,
        dispatch_set: &HashSet<String>,
    ) -> bool {
        if opts.skip_all_agents_permissions {
            return true;
        }
        let is_dispatch_agent = dispatch_set.contains(&binding.jid);
        if (binding.is_admin || is_dispatch_agent) && opts.skip_main_agent_permissions {
            return true;
        }
        false
    }

    /// Compute effective skip-perms for one binding using current flags.
    /// Used by Phase 2 `get_or_create` and `set_dispatch_workspace`.
    #[allow(dead_code)]
    pub(crate) fn resolve_skip_perms(&self, binding: &GroupBinding) -> bool {
        let s = self.state.lock().unwrap();
        let opts = PermissionsConfig {
            skip_main_agent_permissions: s.skip_main_agent_permissions,
            skip_all_agents_permissions: s.skip_all_agents_permissions,
        };
        let dispatch_set: HashSet<String> =
            s.dispatch_workspace_overrides.keys().cloned().collect();
        Self::compute_skip_perms(&opts, binding, &dispatch_set)
    }

    pub fn permission_bridge(&self) -> Option<Arc<PermissionBridge>> {
        self.permission_bridge.lock().unwrap().clone()
    }

    /// First responder wins. Returns `false` if no bridge or already consumed.
    pub fn resolve_permission(&self, request_id: &str, option_key: &str) -> bool {
        match self.permission_bridge.lock().unwrap().as_ref() {
            Some(b) => b.resolve_permission(request_id, option_key),
            None => false,
        }
    }

    /// Web UI batch-answer questions. Defers to PermissionBridge.
    pub fn resolve_ask_question_batch(
        &self,
        request_id: &str,
        answers: &serde_json::Value,
        other_texts: Option<&serde_json::Value>,
    ) -> bool {
        match self.permission_bridge.lock().unwrap().as_ref() {
            Some(b) => b.resolve_ask_question_batch(request_id, answers, other_texts),
            None => false,
        }
    }

    /// Forward a tool-permission response to the underlying core instance.
    /// Used by [`PermissionBridgeApi`] wiring in daemon startup.
    pub fn respond_to_tool_permission(
        &self,
        group_jid: &str,
        tool_name: &str,
        selected: &str,
    ) {
        if let Err(e) = self
            .core_api
            .respond_to_tool_permission(group_jid, tool_name, selected)
        {
            tracing::warn!(
                "[AgentPool] respond_to_tool_permission failed for {group_jid}/{tool_name}: {e}"
            );
        }
    }

    /// Forward an ask-question response map to the underlying core instance.
    /// Used by [`PermissionBridgeApi`] wiring in daemon startup.
    pub fn respond_to_ask_question(
        &self,
        group_jid: &str,
        agent_id: &str,
        answers: HashMap<String, String>,
    ) {
        if let Err(e) = self
            .core_api
            .respond_to_ask_question(group_jid, agent_id, answers)
        {
            tracing::warn!(
                "[AgentPool] respond_to_ask_question failed for {group_jid}/{agent_id}: {e}"
            );
        }
    }

    // ===== Dispatch coordination =====

    /// Temporarily switch a subagent's working dir to the admin's during a
    /// dispatch task. If the subagent core does not exist yet, the override
    /// is recorded so [Phase 2 `get_or_create`] can apply it after creation.
    pub fn set_dispatch_workspace(&self, jid: &str, workspace_dir: &str) {
        if workspace_dir.is_empty() {
            return;
        }
        let (binding_opt, has_core) = {
            let mut s = self.state.lock().unwrap();
            s.dispatch_workspace_overrides
                .insert(jid.to_string(), workspace_dir.to_string());
            (s.bindings.get(jid).cloned(), s.cores.contains(jid))
        };
        if !has_core {
            return;
        }
        self.core_api.set_working_dir(jid, workspace_dir);
        if let Some(b) = binding_opt {
            let skip = self.resolve_skip_perms(&b);
            self.core_api.update_skip_permissions(jid, skip);
        }
        tracing::info!("[AgentPool] Dispatch workspace set for {jid}: {workspace_dir}");
    }

    /// Restore the subagent's own workdir after dispatch completes.
    pub fn revert_dispatch_workspace(&self, jid: &str) {
        let binding_opt = {
            let mut s = self.state.lock().unwrap();
            if !s.dispatch_workspace_overrides.contains_key(jid) {
                return;
            }
            s.dispatch_workspace_overrides.remove(jid);
            s.bindings.get(jid).cloned()
        };
        let Some(binding) = binding_opt else {
            return;
        };
        if !self.state.lock().unwrap().cores.contains(jid) {
            return;
        }
        if !binding.is_admin {
            self.core_api.update_skip_permissions(jid, false);
        }
        let state_file = self.workspace_state_file(&binding.folder);
        match std::fs::read_to_string(&state_file) {
            Ok(raw) => match serde_json::from_str::<WorkspaceStateFile>(&raw) {
                Ok(state) if !state.current_dir.is_empty() => {
                    self.core_api.set_working_dir(jid, &state.current_dir);
                    tracing::info!(
                        "[AgentPool] Dispatch workspace reverted for {jid}: {}",
                        state.current_dir
                    );
                }
                _ => self.core_api.clear_working_dir(jid),
            },
            Err(_) => self.core_api.clear_working_dir(jid),
        }
    }

    pub fn mark_dispatch_executing(&self, jid: &str) {
        self.state
            .lock()
            .unwrap()
            .dispatch_executing
            .insert(jid.to_string());
    }

    pub fn clear_dispatch_executing(&self, jid: &str) {
        self.state.lock().unwrap().dispatch_executing.remove(jid);
    }

    pub fn set_current_dispatch_task_id(&self, jid: &str, task_id: &str) {
        self.state
            .lock()
            .unwrap()
            .dispatch_task_map
            .insert(jid.to_string(), task_id.to_string());
    }

    /// Fallback notify after `process_and_wait` finishes a dispatch round.
    /// Skipped if the idle handler already consumed the reply, or if the next
    /// task has overwritten our taskId.
    pub fn notify_dispatch_if_pending(&self, jid: &str, expected_task_id: Option<&str>) {
        let (content, task_id, current_eq) = {
            let s = self.state.lock().unwrap();
            let content = s.last_dispatch_replies.get(jid).cloned();
            let current_task_id = s.dispatch_task_map.get(jid).cloned();
            if let (Some(exp), Some(cur)) = (expected_task_id, current_task_id.as_ref()) {
                if cur != exp {
                    return;
                }
            }
            let final_task_id = expected_task_id
                .map(String::from)
                .or_else(|| current_task_id.clone());
            let cur_eq = matches!(
                (final_task_id.as_ref(), current_task_id.as_ref()),
                (Some(a), Some(b)) if a == b
            );
            (content, final_task_id, cur_eq)
        };
        let Some(content) = content else { return };
        let bridge = self.dispatch_bridge.lock().unwrap().clone();
        match (task_id.as_deref(), bridge.as_ref()) {
            (Some(tid), Some(b)) => {
                b.notify_task_done(tid, &content);
                if current_eq {
                    self.state.lock().unwrap().dispatch_task_map.remove(jid);
                }
            }
            (None, Some(b)) => b.notify_reply(jid, &content),
            _ => {}
        }
        self.state.lock().unwrap().last_dispatch_replies.remove(jid);
    }

    // ===== Cached todos =====

    /// Snapshot of all cached todos — used for initial push on WS subscribe.
    pub fn get_all_cached_todos(&self) -> HashMap<String, CachedTodos> {
        self.state.lock().unwrap().cached_todos.clone()
    }

    /// Snapshot of all cached agent tool rosters — used for initial push on
    /// WS subscribe so the Agent Console can render the tools each running
    /// agent can use.
    pub fn get_all_cached_tools(&self) -> HashMap<String, CachedTools> {
        self.state.lock().unwrap().cached_tools.clone()
    }

    /// Build & broadcast the current tool roster for `binding`.  Called after
    /// `create_session` so the Web UI sees an entry the moment an agent comes
    /// online — even before any user message is processed.
    pub fn publish_agent_tools(&self, binding: &GroupBinding) {
        let tools = self.core_api.get_tool_infos(&binding.jid);
        if tools.is_empty() {
            return;
        }
        {
            let mut s = self.state.lock().unwrap();
            s.cached_tools.insert(
                binding.jid.clone(),
                CachedTools {
                    agent_name: binding.name.clone(),
                    tools: tools.clone(),
                },
            );
        }
        if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
            sink.notify_agent_tools(&binding.jid, &binding.name, &tools);
        }
    }

    // ===== Reply / activity =====

    /// Unified output: send to channel (when not web-only) and to WS gateway.
    pub async fn broadcast_reply(&self, jid: &str, text: &str, bot_token: Option<&str>) {
        self.broadcast_reply_now(jid, text, bot_token);
    }

    /// Synchronous reply fanout used by event callbacks.
    fn broadcast_reply_now(&self, jid: &str, text: &str, bot_token: Option<&str>) {
        if !jid.starts_with("web:") {
            let send = self.send_reply.lock().unwrap().clone();
            if let Some(send) = send {
                send(jid, text, bot_token);
            }
        }
        // WS push: prefer the structured sink, fall back to legacy ReplyFn.
        let sink = self.agent_event_sink.lock().unwrap().clone();
        if let Some(sink) = sink {
            sink.notify_agent_reply(jid, text);
        } else {
            let cb = self.on_reply.lock().unwrap().clone();
            if let Some(cb) = cb {
                cb(jid, text);
            }
        }
    }

    /// Reset the inactivity timer for a JID. Phase 2 populates the underlying
    /// map; in Phase 1 this is a quiet no-op.
    pub fn notify_activity(&self, jid: &str) {
        let cb = {
            let s = self.state.lock().unwrap();
            s.active_timer_resets.get(jid).cloned()
        };
        if let Some(cb) = cb {
            cb();
        }
    }

    // ===== Workspace state file =====

    /// `~/.senclaw/workspace-state-{folder}.json` — mirrors TS path scheme
    /// (with the `senclaw` brand rename).
    pub(crate) fn workspace_state_file(&self, folder: &str) -> PathBuf {
        let home = self.senclaw_home.lock().unwrap().clone();
        home.join(format!("workspace-state-{folder}.json"))
    }

    /// Initialize the workspace state file with the default working dir
    /// (skipped when the file already exists). Mirrors TS `initWorkspaceState`.
    #[allow(dead_code)] // wired by Phase 2 get_or_create
    pub(crate) fn init_workspace_state(state_file: &Path, default_dir: &Path) {
        if state_file.exists() {
            return;
        }
        let parent = match state_file.parent() {
            Some(p) => p,
            None => return,
        };
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("[AgentPool] Could not create workspace state dir: {e}");
            return;
        }
        let body = WorkspaceStateFile {
            current_dir: default_dir.to_string_lossy().to_string(),
            updated_at: local_iso_string_now(),
        };
        match serde_json::to_string_pretty(&body) {
            Ok(json) => {
                if let Err(e) = std::fs::write(state_file, json) {
                    tracing::warn!("[AgentPool] Could not init workspace state file: {e}");
                }
            }
            Err(e) => tracing::warn!("[AgentPool] Could not serialize workspace state: {e}"),
        }
    }

    // ===== get_or_create (Phase 2) =====

    /// Get or create a core for `binding`, using `pending_creates` as a
    /// concurrency lock so concurrent callers for the same JID wait on the
    /// in-flight creation instead of duplicating (mirrors TS 454–466).
    async fn get_or_create(&self, binding: &GroupBinding) -> Result<()> {
        // Fast path: already exists.
        if self.state.lock().unwrap().cores.contains(&binding.jid) {
            return Ok(());
        }

        // If another task is creating this JID, poll until it finishes.
        loop {
            let pending = self
                .state
                .lock()
                .unwrap()
                .pending_creates
                .contains(&binding.jid);
            if !pending {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            if self.state.lock().unwrap().cores.contains(&binding.jid) {
                return Ok(());
            }
        }

        // Acquire creation lock.
        {
            let mut s = self.state.lock().unwrap();
            if s.cores.contains(&binding.jid) {
                return Ok(());
            }
            s.pending_creates.insert(binding.jid.clone());
        }

        let result = self.get_or_create_internal(binding).await;
        self.state
            .lock()
            .unwrap()
            .pending_creates
            .remove(&binding.jid);
        result
    }

    /// Full core-creation path (mirrors TS 468–684).
    ///
    /// Semantics:
    ///   1. Sync allowedWorkDirs from config.json
    ///   2. Resolve skipPerms + skill dirs + tool list
    ///   3. Create SemaCore instance (blocked on sema-core crate; TODO)
    ///   4. Bind PermissionBridge + events (bindEvents)
    ///   5. Inject MCP servers with timeouts (blocked on sema-core; TODO)
    ///   6. Init MemoryManager index (blocked on sema-core; TODO)
    ///   7. createSession with 60s timeout (blocked on sema-core; TODO)
    ///   8. Apply pending workspace + thinking flag
    ///
    /// On failure: clean up event listeners + dispose core.
    async fn get_or_create_internal(&self, binding: &GroupBinding) -> Result<()> {
        // Double-check after acquiring lock.
        if self.state.lock().unwrap().cores.contains(&binding.jid) {
            return Ok(());
        }

        let binding = binding.clone();

        // Sync allowedWorkDirs from config.json (config.json overrides DB).
        // Mirrors TS AgentPool.ts:479-482.
        // TODO: call getAgentAllowedWorkDirs when GroupManager is wired.

        let skip_perms = self.resolve_skip_perms(&binding);

        // TODO: Build skillsExtraDirs when sema-core + skills modules are wired.
        // Mirrors TS AgentPool.ts:488-496.
        // Priority order: bundled < user (~/.claude/skills) < managed (clawhub) < workspace

        // TODO: Clear stale MCP config file to avoid MCPManager.init() racing
        // with addOrUpdateMCPServer (mirrors TS 503-512).

        // TODO: Resolve tool list (EXCLUDED_TOOLS, ALL_POOLED_TOOLS).
        // Mirrors TS AgentPool.ts:514-522.

        // TODO: Create SemaCore instance when sema-core crate is available.
        // Mirrors TS AgentPool.ts:524-538.
        //   new SemaCore({ instanceId, agentDataDir, workingDir, agentMode, useTools,
        //     logLevel, skillsExtraDirs, skipFileEditPermission, skipBashExecPermission,
        //     skipSkillPermission, skipMCPToolPermission, skipMCPInit })

        // TODO: PermissionBridge.bindCore(core, binding) — mirrors TS 541.

        // Init workspace state file (mirrors TS 569-572).
        let home = self.senclaw_home.lock().unwrap().clone();
        let workspace_dir = home
            .parent()
            .map(|p| {
                p.join("senclaw")
                    .join("workspace")
                    .join(&binding.folder)
            })
            .unwrap_or_else(|| {
                PathBuf::from("senclaw").join("workspace").join(&binding.folder)
            });
        let state_file = home.join(format!("workspace-state-{}.json", binding.folder));
        Self::init_workspace_state(&state_file, &workspace_dir);

        // Inject MCP servers (mirrors TS 546-624) through CoreApi abstraction.
        // Each registration is best-effort: on failure we keep agent creation alive.
        if let Some(cfg) = self.config.lock().unwrap().clone() {
            let state_file_s = state_file.to_string_lossy().to_string();
            let workspace_s = workspace_dir.to_string_lossy().to_string();
            let db_path_s = cfg.paths.db_path.to_string_lossy().to_string();
            let agents_dir_s = cfg.paths.agents_dir.to_string_lossy().to_string();
            let dispatch_state_s = cfg.paths.dispatch_state_path.to_string_lossy().to_string();
            let virtual_agents_dir_s =
                cfg.paths.virtual_agents_dir.to_string_lossy().to_string();

            let mut mcp_servers: Vec<McpServerConfig> = Vec::new();
            mcp_servers.push(schedule_mcp_config(&db_path_s, &binding.folder, &binding.jid));
            mcp_servers.push(workspace_mcp_config(
                &state_file_s,
                &workspace_s,
                binding.allowed_work_dirs.as_deref(),
            ));
            mcp_servers.push(send_mcp_config(
                18081,
                &binding.jid,
                binding.is_admin,
                binding.bot_token.as_deref(),
                &db_path_s,
            ));
            if binding.is_admin {
                mcp_servers.push(dispatch_mcp_config(
                    &dispatch_state_s,
                    &binding.folder,
                    Some(&virtual_agents_dir_s),
                ));
            }
            mcp_servers.push(memory_mcp_config(
                &db_path_s,
                &binding.folder,
                &agents_dir_s,
                Some(cfg.memory.embedding_provider.as_str()),
                if cfg.memory.openai_api_key.is_empty() {
                    None
                } else {
                    Some(cfg.memory.openai_api_key.as_str())
                },
                if cfg.memory.openai_base_url.is_empty() {
                    None
                } else {
                    Some(cfg.memory.openai_base_url.as_str())
                },
            ));
            if binding.channel == "feishu" {
                if let Some(creds) = self.resolve_feishu_credentials(
                    &cfg.paths.global_config_path,
                    &cfg.feishu,
                    binding.bot_token.as_deref(),
                ) {
                    mcp_servers.push(feishu_wiki_mcp_config(
                        &creds.app_id,
                        &creds.app_secret,
                        creds.domain.as_deref(),
                    ));
                }
            }
            tracing::info!(
                "[AgentPool] Preparing {} MCP server(s) for {}: {}",
                mcp_servers.len(),
                binding.jid,
                mcp_servers
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            for server in &mcp_servers {
                if let Err(e) = self.core_api.add_or_update_mcp_server(&binding.jid, server) {
                    tracing::warn!(
                        "[AgentPool] MCP {} unavailable for {}: {e}",
                        server.name,
                        binding.folder
                    );
                }
            }
        }

        // TODO: MemoryManager.initAgent(folder) — mirrors TS 628-639.

        // createSession mirrors TS 641-653. If runtime core is not wired, default no-op.
        self.core_api.create_session(&binding.jid)?;

        // TODO: core.reloadSkills(readDisabledSkills()) — mirrors TS 657.

        // Register core + binding (mirrors TS 658-659).
        {
            let mut s = self.state.lock().unwrap();
            s.cores.insert(binding.jid.clone());
            s.bindings.insert(binding.jid.clone(), binding.clone());
        }

        // Bind persistent listeners after the core is registered.
        if let Some(pool) = self.self_weak.lock().unwrap().upgrade() {
            pool.bind_events(&binding);
        }

        // Apply pending dispatch workspace (mirrors TS 661-666).
        let pending_ws = self
            .state
            .lock()
            .unwrap()
            .dispatch_workspace_overrides
            .get(&binding.jid)
            .cloned();
        if let Some(ref ws) = pending_ws {
            self.core_api.set_working_dir(&binding.jid, ws);
            tracing::info!(
                "[AgentPool] Applied pending dispatch workspace for {}: {ws}",
                binding.jid
            );
        }

        // Apply thinking flag (mirrors TS 669).
        self.core_api
            .update_thinking(&binding.jid, self.state.lock().unwrap().thinking_enabled);

        tracing::info!(
            "[AgentPool] Created agent for {} (folder: {}, skipPerms: {skip_perms})",
            binding.jid,
            binding.folder
        );

        // Push the tool roster so the Agent Console can render this agent
        // as soon as it comes online (mirrors TS `agent:tools` event).
        self.publish_agent_tools(&binding);
        Ok(())
    }

    /// Event-driven process-and-wait with 30‑min inactivity timeout, abort guard,
    /// and 5‑retry on transient errors.  Mirrors TS `processAndWait` (AgentPool.ts:690–828).
    ///
    /// Sets [`State::process_event_txs`] so [`bind_events`] persistent handlers forward
    /// `state:update` / `session:error` events here.  Calls `process_user_input`
    /// (non-blocking), then enters a `tokio::select!` loop with resetTimer pattern:
    ///
    /// | Event            | Action                                  |
    /// |-----------------|-----------------------------------------|
    /// | `Idle`          | cleanup, resolve                        |
    /// | `Error(data)`   | classify → transient retry / network / fatal |
    /// | `Reset`         | restart inactivity timer                |
    /// | `Abort`         | cleanup, reject                         |
    /// | `Timeout`       | destroy, notify dispatch, reject        |
    async fn process_and_wait_inner(
        &self,
        jid: &str,
        group: &GroupBinding,
        prompt: &str,
        retries_left: u32,
    ) -> Result<()> {
        self.get_or_create(group).await?;

        let full_prompt = prompt.to_string();
        // TODO: pre-retrieval memory injection when config.memory.preRetrieval.

        // Log user query to daily log (original prompt, without memory injection).
        if let Some(logger) = self.daily_logger.lock().unwrap().as_ref() {
            logger.append(
                &group.folder,
                crate::memory::daily_logger::Role::User,
                prompt,
            );
        }

        // ---- event bridge channels ----
        // mpsc: bind_events persistent handlers forward state:update / session:error here.
        let (event_tx, mut event_rx) =
            tokio::sync::mpsc::unbounded_channel::<ProcessEvent>();
        // oneshot: destroy_inner signals abort to break the event loop.
        let (abort_tx, mut abort_rx) = tokio::sync::oneshot::channel::<String>();

        // ---- register abort ----
        {
            let mut s = self.state.lock().unwrap();
            let jid_abort = jid.to_string();
            s.active_aborts.insert(
                jid.to_string(),
                Box::new(move |reason: &str| {
                    tracing::warn!("[AgentPool] Abort for {jid_abort}: {reason}");
                    let _ = abort_tx.send(reason.to_string());
                }),
            );
        }

        // ---- register reset-timer callback (used by PermissionBridge / notify_activity) ----
        {
            let tx = event_tx.clone();
            let mut s = self.state.lock().unwrap();
            s.active_timer_resets.insert(
                jid.to_string(),
                Arc::new(move || {
                    let _ = tx.send(ProcessEvent::Reset);
                }),
            );
        }

        // ---- wire process event sender ----
        {
            let mut s = self.state.lock().unwrap();
            s.process_event_txs.insert(jid.to_string(), event_tx);
        }

        // ---- call process_user_input (non-blocking) ----
        // Mirrors TS AgentPool.ts:826: core.processUserInput(fullPrompt).
        // Stub CoreApi no-ops; real sema-core starts processing and emits events.
        tracing::info!(
            "[AgentPool] process_user_input start jid={} prompt_len={}",
            jid,
            full_prompt.len()
        );
        self.core_api.process_user_input(jid, &full_prompt)?;

        let bot_token = group.bot_token.clone();
        let jid_owned = jid.to_string();

        // ---- event loop with resetTimer ----
        #[derive(Debug)]
        enum LoopResult {
            Success,
            Aborted(String),
            Error(SessionErrorData),
        }

        // Initial inactivity timer.
        let timeout_dur = Duration::from_millis(AGENT_TIMEOUT_MS);
        let mut timeout_fut: std::pin::Pin<
            Box<dyn std::future::Future<Output = ()> + Send>,
        > = Box::pin(tokio::time::sleep(timeout_dur));

        let loop_result = loop {
            tokio::select! {
                biased;

                // Abort wins over everything (mirrors TS activeAborts callback).
                Ok(reason) = &mut abort_rx => {
                    tracing::warn!("[AgentPool] PAW aborted for {jid_owned}: {reason}");
                    break LoopResult::Aborted(reason);
                }

                // Event forwarded from bind_events persistent handlers.
                event = event_rx.recv() => {
                    match event {
                        Some(ProcessEvent::Idle) => {
                            tracing::debug!("[AgentPool] PAW idle for {jid_owned}");
                            break LoopResult::Success;
                        }
                        Some(ProcessEvent::Error(data)) => {
                            break LoopResult::Error(data);
                        }
                        Some(ProcessEvent::Reset) => {
                            // Restart inactivity timer (mirrors TS resetTimer).
                            timeout_fut = Box::pin(tokio::time::sleep(timeout_dur));
                        }
                        None => {
                            // Channel closed unexpectedly — treat as fatal error.
                            break LoopResult::Error(SessionErrorData {
                                code: "CHANNEL_CLOSED".into(),
                                message: "Event channel closed unexpectedly".into(),
                            });
                        }
                    }
                }

                // Inactivity timeout (30 min default).
                _ = &mut timeout_fut => {
                    tracing::warn!(
                        "[AgentPool] PAW timeout for {jid_owned} after {}ms",
                        AGENT_TIMEOUT_MS
                    );
                    self.destroy_inner(&jid_owned).await;
                    if let Some(bridge) = self.dispatch_bridge.lock().unwrap().as_ref() {
                        bridge.notify_error(&jid_owned, "Agent timeout");
                    }
                    // Cleanup registrations.
                    {
                        let mut s = self.state.lock().unwrap();
                        s.active_timer_resets.remove(&jid_owned);
                        s.active_aborts.remove(&jid_owned);
                        s.process_event_txs.remove(&jid_owned);
                    }
                    return Err(anyhow::anyhow!("Agent timeout for {jid_owned}"));
                }
            }
        };

        // ---- cleanup registrations ----
        {
            let mut s = self.state.lock().unwrap();
            s.active_timer_resets.remove(&jid_owned);
            s.active_aborts.remove(&jid_owned);
            s.process_event_txs.remove(&jid_owned);
        }

        // ---- handle loop result ----
        match loop_result {
            LoopResult::Success => {
                if !self.state.lock().unwrap().cores.contains(&jid_owned) {
                    return Err(anyhow::anyhow!("Agent destroyed during processing"));
                }
                Ok(())
            }
            LoopResult::Aborted(_reason) => {
                Err(anyhow::anyhow!("Agent aborted"))
            }
            LoopResult::Error(data) => {
                let msg = format!("[{}] {}", data.code, data.message);
                let transient: &[&str] = &[
                    "terminated",
                    "Unexpected event order",
                    "API_RESPONSE_ERROR",
                    "API response format error",
                    "Premature close",
                    "missing finish_reason",
                ];
                let is_transient = transient.iter().any(|p| msg.contains(p));
                let is_network =
                    data.code == "NETWORK_ERROR" || msg.contains("NETWORK_ERROR");

                if is_transient && retries_left > 0 {
                    tracing::warn!(
                        "[AgentPool] Transient error for {jid_owned}: {msg}, retrying in 3s ({retries_left} left)"
                    );
                    let was_dispatching = {
                        let mut s = self.state.lock().unwrap();
                        s.dispatch_executing.remove(&jid_owned)
                    };
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    if was_dispatching {
                        self.state
                            .lock()
                            .unwrap()
                            .dispatch_executing
                            .insert(jid_owned.clone());
                    }
                    Box::pin(self.process_and_wait_inner(
                        &jid_owned,
                        group,
                        prompt,
                        retries_left - 1,
                    ))
                    .await
                } else if is_network {
                    tracing::warn!(
                        "[AgentPool] Network error for {jid_owned}: {msg}, preserving session context"
                    );
                    self.core_api.interrupt_session(&jid_owned);
                    {
                        let mut s = self.state.lock().unwrap();
                        s.dispatch_executing.remove(&jid_owned);
                    }
                    if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
                        sink.notify_agent_state(&jid_owned, "idle");
                    }
                    if let Some(bridge) = self.dispatch_bridge.lock().unwrap().as_ref() {
                        bridge.notify_error(&jid_owned, &format!("[NETWORK_ERROR] {msg}"));
                    }
                    self.broadcast_reply(
                        &jid_owned,
                        &format!(
                            "⚠️ Network error: {msg}\nContext preserved — you can continue from where I left off."
                        ),
                        bot_token.as_deref(),
                    )
                    .await;
                    Err(anyhow::anyhow!(msg))
                } else {
                    self.destroy_inner(&jid_owned).await;
                    if let Some(bridge) = self.dispatch_bridge.lock().unwrap().as_ref() {
                        bridge.notify_error(
                            &jid_owned,
                            &format!("[{code}] {msg}", code = data.code, msg = data.message),
                        );
                    }
                    self.broadcast_reply(
                        &jid_owned,
                        &format!(
                            "❌ Session error [{code}]: {msg}\nSession has been reset.",
                            code = data.code,
                            msg = data.message
                        ),
                        bot_token.as_deref(),
                    )
                    .await;
                    Err(anyhow::anyhow!(msg))
                }
            }
        }
    }

    /// Internal destroy — aborts pending op, tears down core, cleans state.
    async fn destroy_inner(&self, jid: &str) {
        // Abort any pending process_and_wait.
        let abort = {
            let mut s = self.state.lock().unwrap();
            s.active_aborts.remove(jid)
        };
        if let Some(abort) = abort {
            abort("Agent destroyed");
        }

        self.core_api.destroy_agent(jid);
        let mut s = self.state.lock().unwrap();
        s.cores.remove(jid);
        s.bindings.remove(jid);
        s.cached_todos.remove(jid);
        s.dispatch_executing.remove(jid);
        s.dispatch_workspace_overrides.remove(jid);
        s.last_dispatch_replies.remove(jid);
        s.dispatch_task_map.remove(jid);
        s.runtime_work_dirs.remove(jid);
        s.active_timer_resets.remove(jid);
    }

    /// Whether a core has been created for this JID.
    pub fn has_agent(&self, jid: &str) -> bool {
        self.state.lock().unwrap().cores.contains(jid)
    }

    /// Active agent JIDs.
    pub fn active_jids(&self) -> Vec<String> {
        self.state.lock().unwrap().cores.iter().cloned().collect()
    }

    // ===== Phase 3: pause / resume / stop / destroy =====

    /// Pause the agent for `jid`. Three modes (mirrors TS 931–982):
    ///   A. **core-pause** — active PAW → `CoreApi::pause_session`
    ///   B. **dispatch-pause** — active dispatch → record in set, notify
    ///   C. **synth-pause** — fully idle → record in set, notify
    ///
    /// If this agent is a dispatch admin, also pauses active subagents.
    pub fn pause_agent(&self, jid: &str) {
        let has_core = self.state.lock().unwrap().cores.contains(jid);
        if !has_core {
            tracing::warn!("[AgentPool] pause_agent: no active agent for {jid}");
            return;
        }

        let has_active_paw = {
            self.state.lock().unwrap().active_aborts.contains_key(jid)
        };
        let admin_folder = {
            self.state
                .lock()
                .unwrap()
                .bindings
                .get(jid)
                .map(|b| b.folder.clone())
        };
        let has_active_dispatch = admin_folder
            .as_ref()
            .and_then(|folder| {
                let bridge = self.dispatch_bridge.lock().unwrap().clone();
                bridge.map(|b| (b, folder.clone()))
            })
            .map(|(bridge, folder)| bridge.has_active_dispatch(&folder))
            .unwrap_or(false);

        let pause_mode: &str;
        if has_active_paw {
            self.core_api.pause_session(jid);
            pause_mode = "core-pause";
        } else if has_active_dispatch {
            {
                let mut s = self.state.lock().unwrap();
                s.dispatch_paused_jids.insert(jid.to_string());
            }
            if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
                sink.notify_agent_state(jid, "paused");
            }
            pause_mode = "dispatch-pause";
        } else {
            {
                let mut s = self.state.lock().unwrap();
                s.synth_paused_jids.insert(jid.to_string());
            }
            if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
                sink.notify_agent_state(jid, "paused");
            }
            pause_mode = "synth-pause";
        }

        // If admin: pause dispatch + active subagents.
        if let (Some(folder), Some(bridge)) = (
            admin_folder,
            self.dispatch_bridge.lock().unwrap().as_ref().cloned(),
        ) {
            let child_jids = bridge.pause_admin(&folder);
            let mut actually_paused: Vec<String> = Vec::new();
            for child_jid in &child_jids {
                let has_active = {
                    self.state
                        .lock()
                        .unwrap()
                        .active_aborts
                        .contains_key(child_jid.as_str())
                };
                if has_active {
                    self.core_api.pause_session(child_jid);
                    actually_paused.push(child_jid.clone());
                }
            }
            if !actually_paused.is_empty() {
                self.state
                    .lock()
                    .unwrap()
                    .paused_children_by_admin
                    .insert(jid.to_string(), actually_paused);
            }
        }

        tracing::info!("[AgentPool] Paused agent for {jid} ({pause_mode})");
    }

    /// Resume the agent for `jid`, optionally with a follow-up `query`.
    /// Mirrors TS 998–1077 — three scenarios matching pause_agent.
    pub fn resume_agent(self: &Arc<Self>, jid: &str, query: Option<&str>) {
        let has_core = self.state.lock().unwrap().cores.contains(jid);
        if !has_core {
            tracing::warn!("[AgentPool] resume_agent: no active agent for {jid}");
            return;
        }

        let was_synth_paused = {
            self.state.lock().unwrap().synth_paused_jids.contains(jid)
        };
        let was_dispatch_paused = {
            self.state.lock().unwrap().dispatch_paused_jids.contains(jid)
        };
        let was_idle_paused = was_synth_paused || was_dispatch_paused;
        let admin_folder = {
            self.state
                .lock()
                .unwrap()
                .bindings
                .get(jid)
                .map(|b| b.folder.clone())
        };

        if was_idle_paused {
            // Scenario B/C: core was idle — do not inject processUserInput
            // unless there is a query.
            {
                let mut s = self.state.lock().unwrap();
                s.synth_paused_jids.remove(jid);
                s.dispatch_paused_jids.remove(jid);
            }
            if let Some(q) = query.filter(|q| !q.trim().is_empty()) {
                if self.core_api.has_session_tool_results(jid) {
                    let prompt = format!(
                        "{q}\n\nBased on the work completed so far and the latest instruction above, decide how to continue."
                    );
                    let _ = self.core_api.process_user_input(jid, &prompt);
                } else {
                    // Rebuild prompt from DB history and run process_and_wait.
                    let binding = {
                        self.state.lock().unwrap().bindings.get(jid).cloned()
                    };
                    if let Some(binding) = binding {
                        let db = self.db.lock().unwrap().clone();
                        if let Some(db) = db {
                            let (db_prompt, _) =
                                session_bridge::build_prompt_for_group(&db, jid);
                            if !db_prompt.is_empty() {
                                let pool = Arc::clone(self);
                                let jid = jid.to_string();
                                let binding2 = binding.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = AgentApi::process_and_wait(
                                        pool.as_ref(),
                                        &jid,
                                        &binding2,
                                        &db_prompt,
                                    )
                                    .await
                                    {
                                        tracing::error!("[AgentPool] resume_agent process_and_wait error: {e}");
                                    }
                                });
                            }
                        } else if !self.state.lock().unwrap().active_aborts.contains_key(jid) {
                            if let Some(sink) =
                                self.agent_event_sink.lock().unwrap().as_ref()
                            {
                                sink.notify_agent_state(jid, "idle");
                            }
                        }
                    }
                }
            } else if !self.state.lock().unwrap().active_aborts.contains_key(jid) {
                // Push idle only when no PAW race is confirmed.
                if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
                    sink.notify_agent_state(jid, "idle");
                }
            }
            // active_aborts already exists → frontend updates via bindEvents.
        } else {
            // Scenario A: core was processing — continue with processUserInput.
            let base = query.unwrap_or("Go on.");
            let dispatch_ctx = admin_folder
                .as_ref()
                .and_then(|folder| {
                    self.dispatch_bridge
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|b| {
                            build_dispatch_resume_hint(Some(b.as_ref()), folder)
                                .unwrap_or_default()
                        })
                })
                .unwrap_or_default();
            let hint = if query.is_some()
                && !query.unwrap().trim().is_empty()
                && self.core_api.has_session_tool_results(jid)
            {
                "\n\nBased on the work completed so far and the latest instruction above, decide how to continue."
            } else {
                ""
            };
            let prompt = if dispatch_ctx.is_empty() {
                format!("{base}{hint}")
            } else {
                format!("{base}{hint}\n\n{dispatch_ctx}")
            };
            let _ = self.core_api.process_user_input(jid, &prompt);
        }

        // All scenarios: resume dispatch scheduling + paused subagents.
        if let (Some(folder), Some(bridge)) = (
            admin_folder,
            self.dispatch_bridge.lock().unwrap().as_ref().cloned(),
        ) {
            bridge.resume_admin(&folder);
            let paused_children = {
                self.state
                    .lock()
                    .unwrap()
                    .paused_children_by_admin
                    .remove(jid)
                    .unwrap_or_default()
            };
            for child_jid in &paused_children {
                let _ = self.core_api.process_user_input(child_jid, "Go on.");
            }
        }

        let resume_mode = if was_dispatch_paused {
            "dispatch-resume"
        } else if was_synth_paused {
            "synth-resume"
        } else {
            "core-resume"
        };
        tracing::info!("[AgentPool] Resumed agent for {jid} ({resume_mode})");
    }

    /// Terminate agent session for `jid`, discard all context, start fresh.
    /// Mirrors TS 1087–1147.
    pub async fn stop_agent(&self, jid: &str) {
        // 1. Notify dispatch if this agent is executing a subtask.
        if let Some(bridge) = self.dispatch_bridge.lock().unwrap().as_ref() {
            bridge.notify_error(jid, "Agent stopped by user");
        }
        {
            let mut s = self.state.lock().unwrap();
            s.dispatch_task_map.remove(jid);
            s.last_dispatch_replies.remove(jid);
        }

        // 2. Cancel admin dispatch parents + stop child subagents.
        let admin_folder = {
            self.state
                .lock()
                .unwrap()
                .bindings
                .get(jid)
                .map(|b| b.folder.clone())
        };
        let child_jids: Vec<String> = admin_folder
            .as_ref()
            .and_then(|folder| {
                self.dispatch_bridge
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|b| b.cancel_admin_parents(folder))
            })
            .unwrap_or_default();

        // 3. Clear backlog queue.
        let gq = { self.group_queue.lock().unwrap().clone() };
        if let Some(gq) = gq {
            gq.clear_queue(jid).await;
        }

        // 4. Abort pending process_and_wait.
        let abort = {
            let mut s = self.state.lock().unwrap();
            s.active_aborts.remove(jid)
        };
        if let Some(abort) = abort {
            abort("Agent stopped by user");
        }

        // 5. Recreate session.
        let has_core = self.state.lock().unwrap().cores.contains(jid);
        if has_core {
            match self.core_api.create_session(jid) {
                Ok(()) => {
                    if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
                        sink.notify_agent_state(jid, "idle");
                    }
                    tracing::info!("[AgentPool] Stopped and reset agent for {jid}");
                }
                Err(e) => {
                    tracing::error!("[AgentPool] stop_agent create_session failed for {jid}: {e}");
                    if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
                        sink.notify_agent_state(jid, "idle");
                    }
                }
            }
        }

        // Clear residual paused states.
        {
            let mut s = self.state.lock().unwrap();
            s.synth_paused_jids.remove(jid);
            s.dispatch_paused_jids.remove(jid);
            s.paused_children_by_admin.remove(jid);
            s.last_dispatch_replies.remove(jid);
        }
        if self.state.lock().unwrap().cached_todos.contains_key(jid) {
            let name = {
                self.state
                    .lock()
                    .unwrap()
                    .bindings
                    .get(jid)
                    .map(|b| b.name.clone())
                    .unwrap_or_else(|| jid.to_string())
            };
            self.state.lock().unwrap().cached_todos.remove(jid);
            if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
                sink.notify_agent_todos(jid, &name, &[]);
            }
        }

        // Recursively stop child subagents.
        for child_jid in &child_jids {
            Box::pin(self.stop_agent(child_jid)).await;
        }
    }

    /// Full cleanup — dispose core, unwatch files, clear dispatch state, notify
    /// frontend. Mirrors TS 1150–1217.
    pub async fn destroy_agent_full(&self, jid: &str) {
        // Interrupt pending PAW.
        let abort = {
            let mut s = self.state.lock().unwrap();
            s.active_aborts.remove(jid)
        };
        if let Some(abort) = abort {
            abort("Agent destroyed");
        }

        // Stop workspace file watcher.
        {
            let mut s = self.state.lock().unwrap();
            if let Some(unwatch) = s.workspace_watchers.remove(jid) {
                unwatch();
            }
        }

        // Notify frontend (state → idle) BEFORE removing event listeners.
        if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
            sink.notify_agent_state(jid, "idle");
        }

        // Remove persistent event listeners.
        {
            let mut s = self.state.lock().unwrap();
            if let Some(cleanup) = s.event_cleanups.remove(jid) {
                cleanup();
            }
        }

        // Stop memory file watch (mirrors TS AgentPool.ts:1178-1180).
        {
            let s = self.state.lock().unwrap();
            if let Some(binding) = s.bindings.get(jid) {
                let mgr = crate::memory::manager::get_instance();
                mgr.destroy_agent(&binding.folder);
            }
        }

        // Clean dispatch-related state.
        if let Some(bridge) = self.dispatch_bridge.lock().unwrap().as_ref() {
            bridge.notify_error(jid, "Agent destroyed");
        }
        {
            let mut s = self.state.lock().unwrap();
            s.last_dispatch_replies.remove(jid);
            s.dispatch_task_map.remove(jid);
            s.dispatch_executing.remove(jid);
            s.dispatch_workspace_overrides.remove(jid);
            s.synth_paused_jids.remove(jid);
            s.dispatch_paused_jids.remove(jid);
            s.runtime_work_dirs.remove(jid);
            s.bindings.remove(jid);
        }

        // Clear todos cache + notify.
        if self.state.lock().unwrap().cached_todos.contains_key(jid) {
            let name = {
                self.state
                    .lock()
                    .unwrap()
                    .bindings
                    .get(jid)
                    .map(|b| b.name.clone())
                    .unwrap_or_else(|| jid.to_string())
            };
            self.state.lock().unwrap().cached_todos.remove(jid);
            if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
                sink.notify_agent_todos(jid, &name, &[]);
            }
        }

        // Dispose core.
        let has_core = self.state.lock().unwrap().cores.contains(jid);
        if has_core {
            self.core_api.clear_working_dir(jid);
            self.core_api.destroy_agent(jid);
            self.state.lock().unwrap().cores.remove(jid);
        }

        // Final state push.
        if let Some(sink) = self.agent_event_sink.lock().unwrap().as_ref() {
            sink.notify_agent_state(jid, "idle");
        }

        // Clean remaining state maps.
        {
            let mut s = self.state.lock().unwrap();
            s.active_timer_resets.remove(jid);
        }
    }

    /// Destroy all agents (called on shutdown). Mirrors TS 1220–1224.
    pub async fn destroy_all(&self) {
        let jids = self.active_jids();
        tracing::info!("[AgentPool] Destroying {} agent(s)", jids.len());
        for jid in &jids {
            self.destroy_agent_full(jid).await;
        }
    }

    // ===== bind_events (Phase 4) =====

    /// Register persistent event listeners on the core and store cleanup.
    /// Mirrors TS `bindEvents` (AgentPool.ts:1299–1390).
    ///
    /// Event handlers forward to [`AgentEventSink`] and update internal state.
    pub fn bind_events(self: &Arc<Self>, binding: &GroupBinding) {
        let _jid = binding.jid.clone();
        let _folder = binding.folder.clone();
        let _name = binding.name.clone();
        let _bot_token = binding.bot_token.clone();

        // Mutable state shared across event handlers.
        let last_reply: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));

        // ---- message:complete ----
        {
            let pool = Arc::clone(self);
            let jid = _jid.clone();
            let folder = _folder.clone();
            let bot_token = _bot_token.clone();
            let last_reply = Arc::clone(&last_reply);
            let jid_arg = jid.clone();
            self.core_api.on_message_complete(
                &jid_arg,
                Box::new(move |data: MessageCompleteData| {
                    if data.agent_id != MAIN_AGENT_ID {
                        return;
                    }
                    if data.content.trim().is_empty() {
                        return;
                    }
                    *last_reply.lock().unwrap() = data.content.clone();
                    {
                        let mut s = pool.state.lock().unwrap();
                        s.last_dispatch_replies
                            .insert(jid.clone(), data.content.clone());
                    }
                    tracing::debug!(
                        "[AgentPool] message_complete jid={} content_len={} forwarding reply",
                        jid,
                        data.content.len()
                    );
                    pool.broadcast_reply_now(&jid, &data.content, bot_token.as_deref());
                    if let Some(logger) = pool.daily_logger.lock().unwrap().as_ref() {
                        logger.append(
                            &folder,
                            crate::memory::daily_logger::Role::Assistant,
                            &data.content,
                        );
                    }
                }),
            );
        }

        // ---- state:update ----
        {
            let pool = Arc::clone(self);
            let jid = _jid.clone();
            let last_reply = Arc::clone(&last_reply);
            let jid_arg = jid.clone();
            self.core_api.on_state_update(
                &jid_arg,
                Box::new(move |data: StateUpdateData| {
                    if let Some(sink) = pool.agent_event_sink.lock().unwrap().as_ref() {
                        sink.notify_agent_state(&jid, &data.state);
                    }
                    if data.state == "idle" {
                        // Dispatch task-done coordination (persistent).
                        let is_dispatch = {
                            pool.state.lock().unwrap().dispatch_executing.contains(&jid)
                        };
                        if is_dispatch {
                            let reply_text = {
                                let lr = last_reply.lock().unwrap();
                                if !lr.is_empty() {
                                    lr.clone()
                                } else {
                                    pool.state
                                        .lock()
                                        .unwrap()
                                        .last_dispatch_replies
                                        .get(&jid)
                                        .cloned()
                                        .unwrap_or_default()
                                }
                            };
                            let (task_id, bridge) = {
                                let s = pool.state.lock().unwrap();
                                let tid = s.dispatch_task_map.get(&jid).cloned();
                                let bridge = pool.dispatch_bridge.lock().unwrap().clone();
                                (tid, bridge)
                            };
                            if let (Some(tid), Some(bridge)) = (task_id.as_ref(), bridge.as_ref())
                            {
                                bridge.notify_task_done(tid, &reply_text);
                                let mut s = pool.state.lock().unwrap();
                                if s.dispatch_task_map.get(&jid) == Some(tid) {
                                    s.dispatch_task_map.remove(&jid);
                                }
                            } else if let Some(bridge) = bridge.as_ref() {
                                bridge.notify_reply(&jid, &reply_text);
                            }
                            *last_reply.lock().unwrap() = String::new();
                            pool.state
                                .lock()
                                .unwrap()
                                .last_dispatch_replies
                                .remove(&jid);
                        }
                        // Forward to active process_and_wait event loop.
                        if let Some(tx) = pool
                            .state
                            .lock()
                            .unwrap()
                            .process_event_txs
                            .get(&jid)
                            .cloned()
                        {
                            let _ = tx.send(ProcessEvent::Idle);
                        }
                    } else if data.state == "paused" {
                        // Paused: don't send Reset (suspends inactivity timer in PAW).
                    } else {
                        // Processing / other active states — restart PAW inactivity timer.
                        if let Some(tx) = pool
                            .state
                            .lock()
                            .unwrap()
                            .process_event_txs
                            .get(&jid)
                            .cloned()
                        {
                            let _ = tx.send(ProcessEvent::Reset);
                        }
                    }
                }),
            );
        }

        // ---- todos:update ----
        {
            let pool = Arc::clone(self);
            let jid = _jid.clone();
            let name = _name.clone();
            let jid_arg = jid.clone();
            self.core_api.on_todos_update(
                &jid_arg,
                Box::new(move |data: Vec<TodosUpdateItem>| {
                    let todos: Vec<TodoSnapshot> = data
                        .into_iter()
                        .map(|item| TodoSnapshot {
                            content: item.content,
                            status: item.status,
                            active_form: item.active_form,
                        })
                        .collect();
                    {
                        let mut s = pool.state.lock().unwrap();
                        s.cached_todos.insert(
                            jid.clone(),
                            CachedTodos {
                                agent_name: name.clone(),
                                todos: todos.clone(),
                            },
                        );
                    }
                    if let Some(sink) = pool.agent_event_sink.lock().unwrap().as_ref() {
                        sink.notify_agent_todos(&jid, &name, &todos);
                    }
                }),
            );
        }

        // ---- compact:start ----
        {
            let pool = Arc::clone(self);
            let jid = _jid.clone();
            let jid_arg = jid.clone();
            self.core_api.on_compact_start(
                &jid_arg,
                Box::new(move |_data: CompactStartData| {
                    if let Some(sink) = pool.agent_event_sink.lock().unwrap().as_ref() {
                        sink.notify_agent_compacting(&jid, true);
                    }
                }),
            );
        }

        // ---- compact:exec ----
        {
            let pool = Arc::clone(self);
            let jid = _jid.clone();
            let folder = _folder.clone();
            let jid_arg = jid.clone();
            self.core_api.on_compact_exec(
                &jid_arg,
                Box::new(move |_data: CompactExecData| {
                    if let Some(sink) = pool.agent_event_sink.lock().unwrap().as_ref() {
                        sink.notify_agent_compacting(&jid, false);
                    }
                    let today: String = chrono::Utc::now().format("%Y-%m-%d").to_string();
                    let changed_file = dirs::home_dir()
                        .map(|h| {
                            h.join("senclaw")
                                .join("agents")
                                .join(&folder)
                                .join("memory")
                                .join(format!("{today}.md"))
                        })
                        .unwrap_or_else(|| {
                            std::path::PathBuf::from("senclaw")
                                .join("agents")
                                .join(&folder)
                                .join("memory")
                                .join(format!("{today}.md"))
                        });
                    let mgr = crate::memory::manager::get_instance();
                    let changed_str = changed_file.to_string_lossy().to_string();
                    mgr.mark_dirty(&folder, Some(&changed_str));
                }),
            );
        }

        // ---- session:error ----
        {
            let pool = Arc::clone(self);
            let jid = _jid.clone();
            let jid_arg = jid.clone();
            self.core_api.on_session_error(
                &jid_arg,
                Box::new(move |data: SessionErrorData| {
                    tracing::error!(
                        "[AgentPool] Session error for {jid}: [{code}] {msg}",
                        jid = jid,
                        code = data.code,
                        msg = data.message
                    );
                    // Forward to active process_and_wait event loop.
                    if let Some(tx) = pool
                        .state
                        .lock()
                        .unwrap()
                        .process_event_txs
                        .get(&jid)
                        .cloned()
                    {
                        let _ = tx.send(ProcessEvent::Error(data));
                    }
                }),
            );
        }

        // ---- tool:permission:request ----
        {
            let pool = Arc::clone(self);
            let jid = _jid.clone();
            let chat_jid = _jid.clone();
            let bot_token = _bot_token.clone();
            let jid_arg = jid.clone();
            self.core_api.on_tool_permission_request(
                &jid_arg,
                Box::new(move |data: ToolPermissionRequestData| {
                    if let Some(pb) = pool.permission_bridge.lock().unwrap().as_ref() {
                        pb.handle_permission_request(
                            &data.tool_name,
                            &data.title,
                            &data.content,
                            &data.options,
                            &jid,
                            &chat_jid,
                            bot_token.as_deref(),
                        );
                    }
                }),
            );
        }

        // ---- ask:question:request ----
        {
            let pool = Arc::clone(self);
            let jid = _jid.clone();
            let chat_jid = _jid.clone();
            let bot_token = _bot_token.clone();
            let jid_arg = jid.clone();
            self.core_api.on_ask_question_request(
                &jid_arg,
                Box::new(move |data: AskQuestionRequestData| {
                    if let Some(pb) = pool.permission_bridge.lock().unwrap().as_ref() {
                        pb.handle_ask_question_request(
                            &data.agent_id,
                            data.questions.clone(),
                            &jid,
                            &chat_jid,
                            bot_token.as_deref(),
                        );
                    }
                }),
            );
        }

        // ---- cleanup ----
        {
            let pool = Arc::clone(self);
            let jid = _jid.clone();
            let mut s = self.state.lock().unwrap();
            s.event_cleanups.insert(
                jid.clone(),
                Box::new(move || {
                    pool.core_api.off_all(&jid);
                }),
            );
        }
    }

    // ===== run_isolated (Phase 4) =====

    /// Run a scheduled task in an isolated core instance.
    /// Mirrors TS `runIsolated` (AgentPool.ts:839–929).
    ///
    /// Creates a fresh session, processes the prompt, and waits for idle or
    /// timeout. The real sema-core wiring (MCP servers, skills dirs) lands when
    /// the sema-core crate is available.
    pub async fn run_isolated(
        self: &Arc<Self>,
        task_id: &str,
        task_prompt: &str,
        group: &GroupBinding,
        prompt: Option<&str>,
    ) -> Result<()> {
        let effective_prompt = prompt.unwrap_or(task_prompt).to_string();
        let instance_id = format!("isolated-{task_id}");
        let _ = group;

        // Channel for the idle/timeout result.
        let (done_tx, done_rx) = tokio::sync::oneshot::channel::<Result<()>>();

        // Spawn a timeout + wait task.
        let task_id_owned = task_id.to_string();
        tokio::spawn(async move {
            let result = tokio::time::timeout(
                Duration::from_millis(AGENT_TIMEOUT_MS),
                async {
                    // Poll until idle (placeholder — real sema-core emits state:update:idle).
                    // For now just complete immediately with ok.
                    Ok(())
                },
            )
            .await
            .unwrap_or_else(|_| {
                Err(anyhow::anyhow!(
                    "[AgentPool] Isolated task {task_id_owned} timed out"
                ))
            });
            let _ = done_tx.send(result);
        });

        let _ = self.core_api.process_user_input(&instance_id, &effective_prompt);

        done_rx
            .await
            .unwrap_or_else(|_| Err(anyhow::anyhow!("Isolated task {task_id} aborted")))
    }

    // ===== workspace watcher + skills reload (Phase 4) =====

    /// Start watching the workspace state file for `jid`.
    /// Mirrors TS `setupWorkspaceWatcher` (AgentPool.ts:1273–1297).
    pub fn setup_workspace_watcher(self: &Arc<Self>, jid: &str, folder: &str) {
        let state_file = self.workspace_state_file(folder);
        let jid_owned = jid.to_string();
        let folder_owned = folder.to_string();
        let pool_weak = Arc::downgrade(self);
        let aborted = Arc::new(Mutex::new(false));

        // Store unwatch callback.
        {
            let mut s = self.state.lock().unwrap();
            let aborted_inner = Arc::clone(&aborted);
            s.workspace_watchers.insert(
                jid_owned.clone(),
                Box::new(move || {
                    *aborted_inner.lock().unwrap() = true;
                }),
            );
        }

        tokio::spawn(async move {
            let mut last_dir: Option<String> = None;
            loop {
                if *aborted.lock().unwrap() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(300)).await;
                let raw = match std::fs::read_to_string(&state_file) {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let state: Option<WorkspaceStateFile> = serde_json::from_str(&raw).ok();
                if let Some(state) = state {
                    let new_dir = state.current_dir;
                    if !new_dir.is_empty() && Some(&new_dir) != last_dir.as_ref() {
                        last_dir = Some(new_dir.clone());
                        if let Some(pool) = pool_weak.upgrade() {
                            pool.core_api.set_working_dir(&jid_owned, &new_dir);
                            {
                                let mut s = pool.state.lock().unwrap();
                                s.runtime_work_dirs
                                    .insert(jid_owned.clone(), new_dir.clone());
                            }
                            tracing::info!(
                                "[AgentPool] Workspace switched for {folder_owned}: {new_dir}"
                            );
                        }
                    }
                }
            }
        });
    }

    /// Watch the skills reload signal file and reload skills on change.
    /// Mirrors TS `watchSkillsReloadSignal` (AgentPool.ts:183–188).
    pub fn watch_skills_reload_signal(self: &Arc<Self>, config_path: &std::path::Path) {
        let signal_path = config_path
            .parent()
            .map(|p| p.join(".skills-reload"))
            .unwrap_or_else(|| std::path::PathBuf::from(".skills-reload"));

        let pool = Arc::clone(self);
        tokio::spawn(async move {
            let mut last_mtime: Option<std::time::SystemTime> = None;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                match std::fs::metadata(&signal_path) {
                    Ok(meta) => {
                        let mtime = meta.modified().ok();
                        if mtime != last_mtime {
                            last_mtime = mtime;
                            pool.reload_all_skills();
                        }
                    }
                    Err(_) => {
                        last_mtime = None;
                    }
                }
            }
        });
    }

    /// Reload skills across all active cores. Mirrors TS `reloadAllSkills`
    /// (AgentPool.ts:190–222).
    pub fn reload_all_skills(&self) {
        let jids: Vec<String> = {
            let s = self.state.lock().unwrap();
            s.cores.iter().cloned().collect()
        };
        let count = jids.len();
        if count == 0 {
            tracing::info!("[AgentPool] skills reload signal received (no active agents)");
            return;
        }
        tracing::info!("[AgentPool] Reloading skills for {count} active agent(s)");
        let disabled = crate::skills::disabled::read_disabled_skills();
        for _jid in &jids {
            self.core_api.reload_skills(&disabled);
        }
    }

    // ===== Feishu credentials (Phase 4) =====

    /// Resolve Feishu app credentials for a given bot token.
    /// Mirrors TS `resolveFeishuCredentials` (AgentPool.ts:1232–1251).
    ///
    /// If `bot_token` is provided, looks up the matching app in the global
    /// config's `feishu_apps` map. Falls back to env-var credentials.
    pub fn resolve_feishu_credentials(
        &self,
        config_path: &std::path::Path,
        feishu_config: &crate::config::FeishuConfig,
        bot_token: Option<&str>,
    ) -> Option<FeishuCredentials> {
        if let Some(token) = bot_token {
            let apps = crate::gateway::group_manager::get_feishu_apps(config_path);
            if let Some(app) = apps.get(token) {
                return Some(FeishuCredentials {
                    app_id: token.to_string(),
                    app_secret: app.app_secret.clone(),
                    domain: app.domain.clone(),
                });
            }
        }
        if !feishu_config.app_id.is_empty() && !feishu_config.app_secret.is_empty() {
            return Some(FeishuCredentials {
                app_id: feishu_config.app_id.clone(),
                app_secret: feishu_config.app_secret.clone(),
                domain: Some(feishu_config.domain.clone()),
            });
        }
        tracing::warn!("[AgentPool] Cannot resolve Feishu credentials for botToken={bot_token:?}");
        None
    }
}

/// Resolved Feishu credentials returned by [`AgentPool::resolve_feishu_credentials`].
#[derive(Debug, Clone)]
pub struct FeishuCredentials {
    pub app_id: String,
    pub app_secret: String,
    pub domain: Option<String>,
}

// ===== AgentApi impl (Phase 2) =====

#[async_trait]
impl AgentApi for AgentPool {
    async fn broadcast_reply(&self, chat_jid: &str, text: &str, bot_token: Option<&str>) {
        AgentPool::broadcast_reply(self, chat_jid, text, bot_token).await
    }

    async fn process_and_wait(
        &self,
        jid: &str,
        group: &GroupBinding,
        prompt: &str,
    ) -> Result<()> {
        self.process_and_wait_inner(jid, group, prompt, 5).await
    }

    async fn destroy(&self, jid: &str) {
        self.destroy_inner(jid).await;
    }
}

// ===== Workspace state file (de)serialization =====

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceStateFile {
    #[serde(rename = "currentDir")]
    current_dir: String,
    #[serde(rename = "updatedAt", default)]
    updated_at: String,
}

// ===== Tests =====

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_binding(jid: &str, is_admin: bool) -> GroupBinding {
        GroupBinding {
            jid: jid.into(),
            folder: "test".into(),
            name: "Test".into(),
            channel: "web".into(),
            is_admin,
            requires_trigger: false,
            allowed_tools: None,
            allowed_paths: None,
            allowed_work_dirs: None,
            bot_token: None,
            max_messages: None,
            last_active: None,
            added_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[tokio::test]
    async fn zen_core_api_process_message_dispatches() {
        let api = ZenCoreApi::new();
        let result = api.process_message("test:1", "hello", &fake_binding("test:1", false));
        assert!(result.is_ok());
    }

    #[test]
    fn agent_pool_send_reply_no_callback_does_not_panic() {
        let pool = AgentPool::new(Arc::new(ZenCoreApi::new()));
        // Default permissions config is all-false.
        let cfg = pool.get_permissions_config();
        assert!(!cfg.skip_main_agent_permissions);
        assert!(!cfg.skip_all_agents_permissions);
        // notify_activity on unknown JID is a no-op.
        pool.notify_activity("nobody:0");
    }

    #[test]
    fn permissions_config_round_trips() {
        let pool = AgentPool::new(Arc::new(ZenCoreApi::new()));
        pool.set_permissions_config(PermissionsConfig {
            skip_main_agent_permissions: true,
            skip_all_agents_permissions: false,
        });
        let cfg = pool.get_permissions_config();
        assert!(cfg.skip_main_agent_permissions);
        assert!(!cfg.skip_all_agents_permissions);
        assert!(pool.get_skip_perms_for_virtual());
    }

    #[test]
    fn thinking_default_on() {
        let pool = AgentPool::new(Arc::new(ZenCoreApi::new()));
        assert!(pool.get_thinking_enabled());
        pool.set_thinking_enabled(false);
        assert!(!pool.get_thinking_enabled());
    }

    #[test]
    fn skip_perms_admin_with_main_flag() {
        let opts = PermissionsConfig {
            skip_main_agent_permissions: true,
            skip_all_agents_permissions: false,
        };
        let admin = fake_binding("admin:1", true);
        let regular = fake_binding("group:1", false);
        let dispatch_set = HashSet::new();
        assert!(AgentPool::compute_skip_perms(&opts, &admin, &dispatch_set));
        assert!(!AgentPool::compute_skip_perms(&opts, &regular, &dispatch_set));
    }

    #[test]
    fn skip_perms_dispatch_subagent_inherits_main() {
        let opts = PermissionsConfig {
            skip_main_agent_permissions: true,
            skip_all_agents_permissions: false,
        };
        let sub = fake_binding("sub:1", false);
        let mut dispatch_set = HashSet::new();
        dispatch_set.insert("sub:1".to_string());
        assert!(AgentPool::compute_skip_perms(&opts, &sub, &dispatch_set));
    }

    #[test]
    fn skip_perms_skip_all_overrides_everything() {
        let opts = PermissionsConfig {
            skip_main_agent_permissions: false,
            skip_all_agents_permissions: true,
        };
        let regular = fake_binding("g:1", false);
        let dispatch_set = HashSet::new();
        assert!(AgentPool::compute_skip_perms(&opts, &regular, &dispatch_set));
    }

    #[test]
    fn dispatch_executing_mark_clear() {
        let pool = AgentPool::new(Arc::new(ZenCoreApi::new()));
        pool.mark_dispatch_executing("g:1");
        assert!(pool.state.lock().unwrap().dispatch_executing.contains("g:1"));
        pool.clear_dispatch_executing("g:1");
        assert!(!pool.state.lock().unwrap().dispatch_executing.contains("g:1"));
    }

    #[test]
    fn dispatch_task_map_round_trip() {
        let pool = AgentPool::new(Arc::new(ZenCoreApi::new()));
        pool.set_current_dispatch_task_id("g:1", "task-42");
        let s = pool.state.lock().unwrap();
        assert_eq!(s.dispatch_task_map.get("g:1").map(String::as_str), Some("task-42"));
    }

    #[test]
    fn notify_dispatch_skips_when_no_pending_reply() {
        let pool = AgentPool::new(Arc::new(ZenCoreApi::new()));
        // No content recorded → silent no-op (no panic).
        pool.notify_dispatch_if_pending("g:1", Some("task-1"));
    }

    #[test]
    fn workspace_state_file_path_format() {
        let pool = AgentPool::new(Arc::new(ZenCoreApi::new()));
        let tmp = std::env::temp_dir().join(format!("senclaw-test-{}", std::process::id()));
        pool.set_senclaw_home(tmp.clone());
        let p = pool.workspace_state_file("main");
        assert_eq!(p, tmp.join("workspace-state-main.json"));
    }

    #[test]
    fn init_workspace_state_writes_default() {
        let tmp = tempfile::tempdir().unwrap();
        let state_file = tmp.path().join("workspace-state-foo.json");
        let default_dir = tmp.path().join("foo-workspace");
        AgentPool::init_workspace_state(&state_file, &default_dir);
        let raw = std::fs::read_to_string(&state_file).unwrap();
        let parsed: WorkspaceStateFile = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.current_dir, default_dir.to_string_lossy());
        assert!(!parsed.updated_at.is_empty());
    }

    #[test]
    fn init_workspace_state_skips_when_file_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let state_file = tmp.path().join("ws.json");
        std::fs::write(&state_file, r#"{"currentDir":"/custom","updatedAt":""}"#).unwrap();
        AgentPool::init_workspace_state(&state_file, &tmp.path().join("default"));
        let raw = std::fs::read_to_string(&state_file).unwrap();
        assert!(raw.contains("/custom"));
    }

    #[test]
    fn cached_todos_empty_by_default() {
        let pool = AgentPool::new(Arc::new(ZenCoreApi::new()));
        assert!(pool.get_all_cached_todos().is_empty());
    }
}
