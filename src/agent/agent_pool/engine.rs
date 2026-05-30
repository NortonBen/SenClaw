//! ZenCoreApi — production [`CoreApi`] backed by [`ZenEngine`] (the zen-core runtime).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use reqwest::Client;

use super::traits::{AgentToolInfo, CoreApi, CoreHandlers};
use super::types::{
    AskQuestionRequestData, CompactExecData, CompactStartData, MessageCompleteData,
    SessionErrorData, StateUpdateData, TodosUpdateItem, ToolPermissionRequestData,
};
use crate::agent::permission_bridge::{AskQuestionData, AskQuestionOption};
use crate::config::Config;
use crate::mcp::helper::McpServerConfig;
use crate::types::GroupBinding;
use crate::zen_core::{
    AskQuestionResponseData, EngineEvent, PlanExitResponseData, SessionState,
    ToolPermissionResponseData, ZenCore, ZenCoreOptions, ZenEngine,
};
use tokio::sync::broadcast::error::RecvError;

/// Production [`CoreApi`] backed by [`ZenEngine`] (the zen-core runtime).
///
/// Manages one [`ZenEngine`] per JID, bridges engine events to CoreApi
/// handler callbacks, and delegates lifecycle operations to the engine.
pub struct ZenCoreApi {
    engines: Mutex<HashMap<String, Arc<ZenEngine>>>,
    handlers: Arc<Mutex<HashMap<String, CoreHandlers>>>,
    http_client: Client,
    mcp_manager: Option<Arc<crate::mcp::manager::McpManager>>,
    /// Optional WorkbenchBridge — when set, `ensure_engine` binds each new
    /// engine's event stream so artifacts surface in the UI / IM fallback.
    workbench_bridge: Mutex<Option<Arc<crate::agent::workbench_bridge::WorkbenchBridge>>>,
    /// Per-jid bot tokens used by the workbench-bridge IM fallback.
    bot_tokens: Mutex<HashMap<String, Option<String>>>,
    /// Callback invoked when an engine emits a plan-exit request. Caller
    /// (lib.rs) uses it to broadcast the event over WS so the UI can render
    /// the plan-approval modal. `Arc<Mutex>` so spawned event loops can hold
    /// a cheap clone and pick up callbacks set after engine creation.
    on_plan_exit_request: Arc<
        Mutex<Option<Arc<dyn Fn(String, crate::zen_core::PlanExitRequestData) + Send + Sync>>>,
    >,
    /// Callback fired for every `EngineEvent::ToolExecutionComplete` and
    /// `ToolExecutionError`. Lets `lib.rs` push a `tool:execution` WS event so
    /// the chat UI can render a claude-code-style collapsible "Read 3 files,
    /// ran 1 command" tool-group card.
    on_tool_execution: Arc<
        Mutex<Option<Arc<dyn Fn(String, ToolExecutionEvent) + Send + Sync>>>,
    >,
}

/// Wire-format tool-execution event used by the AgentPool → WS gateway path.
/// `ok = true` for `ToolExecutionComplete`, `ok = false` for `Error`.
#[derive(Debug, Clone)]
pub struct ToolExecutionEvent {
    pub agent_id: String,
    pub tool_name: String,
    pub title: String,
    pub summary: String,
    pub content: serde_json::Value,
    pub ok: bool,
}

impl ZenCoreApi {
    pub fn new(mcp_manager: Option<Arc<crate::mcp::manager::McpManager>>) -> Self {
        Self {
            engines: Mutex::new(HashMap::new()),
            handlers: Arc::new(Mutex::new(HashMap::new())),
            http_client: Client::builder()
                .connect_timeout(Duration::from_secs(30))
                .timeout(Duration::from_secs(120))
                .build()
                .expect("ZenCoreApi http client"),
            mcp_manager,
            workbench_bridge: Mutex::new(None),
            bot_tokens: Mutex::new(HashMap::new()),
            on_plan_exit_request: Arc::new(Mutex::new(None)),
            on_tool_execution: Arc::new(Mutex::new(None)),
        }
    }

    /// Wire a callback fired when any engine emits `EngineEvent::PlanExitRequest`.
    /// Used by lib.rs to broadcast `plan:exit:request` over the WebSocket gateway.
    pub fn set_on_plan_exit_request(
        &self,
        cb: Arc<dyn Fn(String, crate::zen_core::PlanExitRequestData) + Send + Sync>,
    ) {
        *self.on_plan_exit_request.lock().unwrap() = Some(cb);
    }

    /// Wire a callback fired for every tool execution (complete or error).
    /// Used by lib.rs to broadcast `tool:execution` over the WebSocket gateway
    /// so the chat UI can render tool-call activity inline.
    pub fn set_on_tool_execution(
        &self,
        cb: Arc<dyn Fn(String, ToolExecutionEvent) + Send + Sync>,
    ) {
        *self.on_tool_execution.lock().unwrap() = Some(cb);
    }

    /// Inject the WorkbenchBridge so future-created engines emit artifact
    /// events into the bridge. Idempotent — last setter wins.
    pub fn set_workbench_bridge(
        &self,
        bridge: Arc<crate::agent::workbench_bridge::WorkbenchBridge>,
    ) {
        *self.workbench_bridge.lock().unwrap() = Some(bridge);
    }

    /// Update the cached `bot_token` for a JID. AgentPool calls this when
    /// loading group bindings so the workbench-bridge IM fallback can target
    /// the right bot when artifacts are published.
    pub fn set_bot_token(&self, jid: &str, bot_token: Option<String>) {
        self.bot_tokens
            .lock()
            .unwrap()
            .insert(jid.to_string(), bot_token);
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
        let engine = ZenEngine::new(opts, self.mcp_manager.clone());
        engine.initialize_plugins();
        // Refresh MCP bridge tools in the background
        {
            let engine = engine.clone();
            tokio::spawn(async move {
                engine.refresh_mcp_tools().await;
            });
        }
        // Bind WorkbenchBridge if wired (relays artifact events to UI + IM fallback).
        if let Some(bridge) = self.workbench_bridge.lock().unwrap().clone() {
            let bot_token = self.bot_tokens.lock().unwrap().get(jid).cloned().flatten();
            bridge.bind_engine(engine.clone(), jid, bot_token);
        }
        engines.insert(jid.to_string(), engine.clone());
        drop(engines);
        // Subscribe the event-bus forwarder exactly ONCE per engine — here, at
        // creation. Previously this lived in `create_session`, which AgentPool
        // calls more than once for the same jid (bind_group + stop_agent). Each
        // call spawned ANOTHER subscriber on the SAME cached engine's event bus,
        // so every think / skill / reply event was forwarded twice and the Web
        // UI rendered everything twice. Tying the bridge to engine creation makes
        // it 1:1 with the engine: a reused engine never double-bridges, and a
        // destroyed + recreated engine (the old loop sees `Closed` and exits)
        // re-bridges cleanly.
        self.bridge_events(jid, &engine);
        engine
    }

    /// Subscribe to the engine's EventBus and forward events to handlers.
    /// Called exactly once per engine, from `ensure_engine` at creation time.
    fn bridge_events(&self, jid: &str, engine: &Arc<ZenEngine>) {
        let jid = jid.to_string();
        let handlers_map = Arc::clone(&self.handlers);
        // Snapshot the plan-exit callback shared Mutex so the spawned loop
        // can fire it without re-locking through `&self`.
        let plan_callback_for_loop = self.on_plan_exit_request.clone();
        let tool_exec_callback_for_loop = self.on_tool_execution.clone();
        let mut rx = engine.event_bus.subscribe();

        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let handlers = handlers_map.lock().unwrap().get(&jid).cloned();
                        let h = match handlers {
                            Some(h) => h,
                            None => continue,
                        };
                        match event {
                            EngineEvent::MessageComplete(data) => {
                                if let Some(ref cb) = h.message_complete {
                                    cb(MessageCompleteData {
                                        agent_id: data.agent_id,
                                        reasoning: data.reasoning,
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
                                tracing::info!(
                                    "[AgentPool] bridge_events TodosUpdate jid={jid} items={}",
                                    items.len()
                                );
                                if let Some(ref cb) = h.todos_update {
                                    cb(items
                                        .iter()
                                        .map(|item| TodosUpdateItem {
                                            content: item.content.clone(),
                                            status: item.status.clone(),
                                            active_form: item.active_form.clone(),
                                        })
                                        .collect());
                                } else {
                                    tracing::warn!(
                                        "[AgentPool] bridge_events TodosUpdate for {jid} but NO handler registered"
                                    );
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
                                        questions: data
                                            .questions
                                            .into_iter()
                                            .map(|q| AskQuestionData {
                                                question: q.question,
                                                header: q.header,
                                                options: q
                                                    .options
                                                    .into_iter()
                                                    .map(|o| AskQuestionOption {
                                                        label: o.label,
                                                        description: o.description,
                                                    })
                                                    .collect(),
                                                multi_select: q.multi_select,
                                            })
                                            .collect(),
                                    });
                                }
                            }
                            EngineEvent::ConversationUsage(data) => {
                                if let Some(ref cb) = h.conversation_usage {
                                    cb(data);
                                }
                            }
                            EngineEvent::PlanExitRequest(data) => {
                                // Independent of CoreHandlers — uses the global
                                // ZenCoreApi callback wired at startup. Forwards
                                // to lib.rs which broadcasts the WS event so the
                                // UI can render the PlanExitDialog.
                                let cb_opt = plan_callback_for_loop.lock().unwrap().clone();
                                if let Some(cb) = cb_opt {
                                    cb(jid.clone(), data);
                                }
                            }
                            EngineEvent::ToolExecutionComplete(data) => {
                                let cb_opt = tool_exec_callback_for_loop.lock().unwrap().clone();
                                if let Some(cb) = cb_opt {
                                    cb(jid.clone(), ToolExecutionEvent {
                                        agent_id: data.agent_id,
                                        tool_name: data.tool_name,
                                        title: data.title,
                                        summary: data.summary,
                                        content: data.content,
                                        ok: true,
                                    });
                                }
                            }
                            EngineEvent::ToolExecutionError(data) => {
                                let cb_opt = tool_exec_callback_for_loop.lock().unwrap().clone();
                                if let Some(cb) = cb_opt {
                                    cb(jid.clone(), ToolExecutionEvent {
                                        agent_id: data.agent_id,
                                        tool_name: data.tool_name,
                                        title: data.title,
                                        summary: String::new(),
                                        content: serde_json::Value::String(data.content),
                                        ok: false,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    Err(RecvError::Lagged(n)) => {
                        tracing::warn!("[ZenCoreApi] event bus lagged by {} for {}", n, jid);
                        continue;
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }
}

impl CoreApi for ZenCoreApi {
    fn process_message(&self, jid: &str, prompt: &str, _group: &GroupBinding) -> Result<String> {
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

    fn set_use_tools(&self, jid: &str, tools: Vec<String>) {
        let engine = self.ensure_engine(jid);
        engine.set_use_tools(tools);
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
            request_timeout_secs: None,
        };
        engine.add_or_update_mcp_server(&zc_cfg, "project")?;
        Ok(())
    }

    fn create_session(&self, jid: &str) -> Result<()> {
        // Do NOT bridge events here — `ensure_engine` already subscribes the
        // forwarder exactly once per engine. AgentPool calls create_session
        // more than once per jid (bind_group + stop_agent), so bridging here
        // would spawn a duplicate subscriber and double every emitted event.
        let engine = self.ensure_engine(jid);
        engine.create_session(None)?;
        Ok(())
    }

    fn get_tool_infos(&self, jid: &str) -> Vec<AgentToolInfo> {
        let engine = self.engines.lock().unwrap().get(jid).cloned();
        let Some(engine) = engine else {
            return Vec::new();
        };
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
        self.engines
            .lock()
            .unwrap()
            .get(jid)
            .map(|e| e.has_session_tool_results())
            .unwrap_or(false)
    }

    fn update_agent_mode(&self, jid: &str, mode: &str) {
        use crate::zen_core::AgentMode;
        let parsed = match mode {
            "Plan" => AgentMode::Plan,
            "Agent" => AgentMode::Agent,
            other => {
                tracing::warn!(
                    "[ZenCoreApi] update_agent_mode: unknown mode '{other}' for {jid}, ignored"
                );
                return;
            }
        };
        if let Some(engine) = self.engines.lock().unwrap().get(jid).cloned() {
            engine.update_agent_mode(parsed);
        } else {
            tracing::warn!("[ZenCoreApi] update_agent_mode: no engine for {jid}");
        }
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

    fn on_state_update(&self, jid: &str, handler: Box<dyn Fn(StateUpdateData) + Send + Sync>) {
        self.with_handlers(jid, |entry| {
            entry.state_update = Some(Arc::from(handler));
        });
    }

    fn on_todos_update(&self, jid: &str, handler: Box<dyn Fn(Vec<TodosUpdateItem>) + Send + Sync>) {
        self.with_handlers(jid, |entry| {
            entry.todos_update = Some(Arc::from(handler));
        });
    }

    fn on_compact_start(&self, jid: &str, handler: Box<dyn Fn(CompactStartData) + Send + Sync>) {
        self.with_handlers(jid, |entry| {
            entry.compact_start = Some(Arc::from(handler));
        });
    }

    fn on_compact_exec(&self, jid: &str, handler: Box<dyn Fn(CompactExecData) + Send + Sync>) {
        self.with_handlers(jid, |entry| {
            entry.compact_exec = Some(Arc::from(handler));
        });
    }

    fn on_session_error(&self, jid: &str, handler: Box<dyn Fn(SessionErrorData) + Send + Sync>) {
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

    fn on_conversation_usage(
        &self,
        jid: &str,
        handler: Box<dyn Fn(crate::zen_core::ConversationUsageData) + Send + Sync>,
    ) {
        self.with_handlers(jid, |entry| {
            entry.conversation_usage = Some(Arc::from(handler));
        });
    }

    fn add_allowed_tool(&self, jid: &str, tool: &str) {
        if let Some(engine) = self.engines.lock().unwrap().get(jid) {
            engine.add_allowed_tool(tool);
        }
    }

    fn respond_to_tool_permission(&self, jid: &str, tool_name: &str, selected: &str) -> Result<()> {
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

    fn respond_to_plan_exit(
        &self,
        jid: &str,
        agent_id: &str,
        selected: &str,
    ) -> Result<()> {
        if let Some(engine) = self.engines.lock().unwrap().get(jid) {
            engine.respond_to_plan_exit(PlanExitResponseData {
                agent_id: agent_id.to_string(),
                selected: selected.to_string(),
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
