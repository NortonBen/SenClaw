//! ZenEngine — per-instance agent runtime orchestrator.
//!
//! Owns the [`EventBus`], [`StateManager`], tool registry, and config.
//! Implements [`ZenCore`] so SemaClaw's [`AgentPool`] can drive it without
//! knowing about internal engine details.
//!
//! Port of TS `ZenEngine` from sema-core.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::config_manager;
use super::hooks::{
    self as zen_hooks, ExecuteHooksOptions, HookEvent, HookInput, HookInputBase, HookManager,
    SessionInput, StopInput, UserPromptSubmitInput,
};
use super::*;
use crate::gateway::group_manager::load_llm_configs;
use crate::mcp::SharedMcpRegistry;
use crate::skills::SkillRegistry;
use crate::tools::{SkillTool, TaskTool, TodoWriteTool};
use events::ResponseRegistry;
use permissions::PermissionManager;

/// Per-instance agent execution engine.
///
/// Each chat JID gets one engine. The engine is driven by [`ZenCore`] method
/// calls from [`AgentPool`] and emits events back through the [`EventBus`].
pub struct ZenEngine {
    pub instance_id: String,
    pub event_bus: EventBus,
    state: Arc<Mutex<StateManager>>,
    response_registry: Arc<ResponseRegistry>,
    permission_manager: Arc<PermissionManager>,
    handlers: RwLock<ZenCoreHandlers>,

    // HTTP client for LLM calls
    http_client: Client,

    // Config
    pub(crate) options: RwLock<ZenCoreOptions>,

    // Tool registry
    builtin_tools: RwLock<Vec<Arc<dyn Tool>>>,

    // Skill registry (shared with Skill tool)
    skill_registry: Arc<SkillRegistry>,

    // MCP subprocess registry
    mcp_registry: SharedMcpRegistry,

    // External MCP server manager (bridges user-configured MCP tools)
    pub mcp_manager: Option<Arc<crate::mcp::manager::McpManager>>,

    // Session helpers
    session_id: RwLock<Option<String>>,

    // Hook system
    pub hook_manager: Arc<HookManager>,
}

impl ZenEngine {
    pub fn new(
        options: ZenCoreOptions,
        mcp_manager: Option<Arc<crate::mcp::manager::McpManager>>,
    ) -> Arc<Self> {
        let instance_id = options.instance_id.clone();
        let event_bus = EventBus::new();
        let response_registry = Arc::new(ResponseRegistry::new());
        let permission_manager = Arc::new(PermissionManager::new(
            event_bus.clone(),
            response_registry.clone(),
        ));

        let http_client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .expect("Failed to create HTTP client");

        let state = Arc::new(Mutex::new(StateManager::new()));
        let skill_registry = Arc::new(SkillRegistry::default());

        let engine = Arc::new(Self {
            instance_id,
            event_bus,
            state: state.clone(),
            response_registry,
            permission_manager,
            handlers: RwLock::new(ZenCoreHandlers::default()),
            http_client,
            options: RwLock::new(options),
            builtin_tools: RwLock::new(Vec::new()),
            skill_registry: skill_registry.clone(),
            mcp_registry: SharedMcpRegistry::new(),
            mcp_manager,
            session_id: RwLock::new(None),
            hook_manager: Arc::new(HookManager::empty()),
        });

        // Register engine-dependent tools
        engine.register_tool(Arc::new(TodoWriteTool::new(state)));
        engine.register_tool(Arc::new(SkillTool::new(skill_registry)));

        // Register static tools (no engine deps)
        engine.register_tools(crate::tools::all_tools());

        // Register TaskTool last so it knows about all other tools
        let all_tools = engine.builtin_tools.read().unwrap().clone();
        let profile = Self::resolve_model_profile();
        engine.register_tool(Arc::new(TaskTool::new(
            engine.http_client.clone(),
            engine.event_bus.clone(),
            engine.state.clone(),
            engine.permission_manager.clone(),
            crate::tools::task::default_agent_configs(),
            engine.options.read().unwrap().working_dir.clone(),
            engine.options.read().unwrap().agent_data_dir.clone(),
            all_tools,
            profile,
        )));

        engine
    }

    // ============================================================
    // Tool registry
    // ============================================================

    pub fn register_tool(&self, tool: Arc<dyn Tool>) {
        self.builtin_tools.write().unwrap().push(tool);
    }

    pub fn register_tools(&self, tools: Vec<Arc<dyn Tool>>) {
        self.builtin_tools.write().unwrap().extend(tools);
    }

    /// Refresh external MCP bridge tools in the tool roster.
    /// Removes previously-registered `mcp__` tools and re-fetches from the
    /// McpManager.
    pub async fn refresh_mcp_tools(self: &Arc<Self>) {
        if let Some(ref mgr) = self.mcp_manager {
            let bridge_tools = crate::mcp::bridge::McpBridgeTool::from_manager(mgr).await;
            let mut tools = self.builtin_tools.write().unwrap();
            // Remove old MCP bridge tools
            tools.retain(|t| !t.name().starts_with("mcp__"));
            // Add refreshed ones
            tools.extend(bridge_tools);
        }
    }

    pub fn get_tools(&self) -> Vec<Arc<dyn Tool>> {
        let opts = self.options.read().unwrap();
        let use_tools = &opts.use_tools;
        let is_plan = opts.agent_mode == AgentMode::Plan;
        let tools = self.builtin_tools.read().unwrap();

        let mut filtered: Vec<Arc<dyn Tool>> = if use_tools.is_empty() {
            tools.clone()
        } else {
            tools
                .iter()
                .filter(|t| use_tools.contains(&t.name().to_string()))
                .cloned()
                .collect()
        };

        // Plan mode removes TodoWrite
        if is_plan {
            filtered.retain(|t| t.name() != "TodoWrite");
        }

        filtered
    }

    // ============================================================
    // Event helpers — fire event on bus AND call registered handler
    // ============================================================

    fn fire(&self, event: EngineEvent) {
        self.event_bus.emit(event.clone());
        let handlers = self.handlers.read().unwrap();
        match event {
            EngineEvent::SessionReady(d) => {
                if let Some(ref h) = handlers.on_session_ready {
                    h(d);
                }
            }
            EngineEvent::MessageComplete(d) => {
                if let Some(ref h) = handlers.on_message_complete {
                    h(d);
                }
            }
            EngineEvent::StateUpdate(d) => {
                if let Some(ref h) = handlers.on_state_update {
                    h(d);
                }
            }
            EngineEvent::SessionError(d) => {
                if let Some(ref h) = handlers.on_session_error {
                    h(d);
                }
            }
            EngineEvent::SessionInterrupted(d) => {
                if let Some(ref h) = handlers.on_session_interrupted {
                    h(d);
                }
            }
            EngineEvent::TodosUpdate(d) => {
                if let Some(ref h) = handlers.on_todos_update {
                    h(d);
                }
            }
            EngineEvent::ConversationUsage(d) => {
                if let Some(ref h) = handlers.on_conversation_usage {
                    h(d);
                }
            }
            EngineEvent::CompactStart(d) => {
                if let Some(ref h) = handlers.on_compact_start {
                    h(d);
                }
            }
            EngineEvent::CompactExec(d) => {
                if let Some(ref h) = handlers.on_compact_exec {
                    h(d);
                }
            }
            EngineEvent::ToolPermissionRequest(d) => {
                if let Some(ref h) = handlers.on_tool_permission_request {
                    h(d);
                }
            }
            EngineEvent::ToolExecutionComplete(d) => {
                if let Some(ref h) = handlers.on_tool_execution_complete {
                    h(d);
                }
            }
            EngineEvent::ToolExecutionError(d) => {
                if let Some(ref h) = handlers.on_tool_execution_error {
                    h(d);
                }
            }
            EngineEvent::AskQuestionRequest(d) => {
                if let Some(ref h) = handlers.on_ask_question_request {
                    h(d);
                }
            }
            EngineEvent::PlanExitRequest(d) => {
                if let Some(ref h) = handlers.on_plan_exit_request {
                    h(d);
                }
            }
            EngineEvent::TaskAgentStart(d) => {
                if let Some(ref h) = handlers.on_task_agent_start {
                    h(d);
                }
            }
            EngineEvent::TaskAgentEnd(d) => {
                if let Some(ref h) = handlers.on_task_agent_end {
                    h(d);
                }
            }
            EngineEvent::TextChunk(d) => {
                if let Some(ref h) = handlers.on_text_chunk {
                    h(d);
                }
            }
            EngineEvent::ThinkingChunk(d) => {
                if let Some(ref h) = handlers.on_thinking_chunk {
                    h(d);
                }
            }
            // Internal events — bus only, no handler dispatch
            EngineEvent::SessionCleared { .. }
            | EngineEvent::ToolPermissionResponse(_)
            | EngineEvent::AskQuestionResponse(_)
            | EngineEvent::PlanExitResponse(_)
            | EngineEvent::PlanImplement(_)
            | EngineEvent::FileReference(_)
            | EngineEvent::TopicUpdate(_)
            | EngineEvent::ConfigNoModels(_) => {}
        }
    }

    // ============================================================
    // Session lifecycle — internal helpers
    // ============================================================

    fn generate_session_id() -> String {
        Uuid::new_v4().to_string()
    }

    pub fn abort_current(&self) {
        let mut state = self.state.lock().unwrap();
        if let Some(ref token) = state.current_abort {
            if !token.is_cancelled() {
                info!("Aborting current request via CancellationToken");
                token.cancel();
            }
        }
        state.current_abort = None;
    }

    /// Build `ExecuteHooksOptions` reusing the engine's HTTP client and active profile.
    fn hook_opts(&self) -> (Client, ModelProfile) {
        (self.http_client.clone(), Self::resolve_model_profile())
    }

    /// Build the base fields included in every hook input payload.
    fn hook_base(&self, event: HookEvent) -> HookInputBase {
        let sid = self.session_id.read().unwrap().clone().unwrap_or_default();
        let cwd = self.options.read().unwrap().working_dir.clone();
        HookInputBase {
            hook_event_name: event,
            session_id: sid,
            agent_id: MAIN_AGENT_ID.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            cwd,
        }
    }

    /// Resolve active model profile from global config first, then env fallback.
    fn resolve_model_profile() -> ModelProfile {
        let config_path = std::env::var("SENCLAW_CONFIG_PATH")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .map(|h| {
                        h.join(".senclaw")
                            .join("config.json")
                            .to_string_lossy()
                            .to_string()
                    })
                    .unwrap_or_else(|| ".senclaw/config.json".to_string())
            });

        let loaded = load_llm_configs(std::path::Path::new(&config_path));
        if !loaded.configs.is_empty() {
            let selected = loaded
                .active_id
                .as_ref()
                .and_then(|id| loaded.configs.iter().find(|c| &c.id == id))
                .cloned()
                .unwrap_or_else(|| loaded.configs[0].clone());

            let provider = if selected.provider.trim().is_empty() {
                if selected.adapt.trim().eq_ignore_ascii_case("anthropic") {
                    "anthropic".to_string()
                } else {
                    "openai".to_string()
                }
            } else {
                selected.provider.clone()
            };

            return ModelProfile {
                name: selected.label,
                provider,
                model_name: selected.model_name,
                base_url: selected.base_url,
                api_key: selected.api_key,
                max_tokens: selected.max_tokens,
                context_length: selected.context_length,
                adapt: if selected.adapt.trim().is_empty() {
                    None
                } else {
                    Some(selected.adapt)
                },
            };
        }

        // Fallback for legacy env-based setup.
        let base_url = std::env::var("SENCLAW_OPENAI_BASE_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                std::env::var("OPENAI_BASE_URL")
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            })
            .unwrap_or_else(|| "https://api.openai.com/v1".into());
        let api_key = std::env::var("SENCLAW_OPENAI_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| {
                std::env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|v| !v.trim().is_empty())
            })
            .unwrap_or_default();
        let model_name = std::env::var("SENCLAW_OPENAI_CHAT_MODEL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "gpt-4o-mini".into());

        ModelProfile {
            name: "default".into(),
            provider: "openai".into(),
            model_name,
            base_url,
            api_key,
            max_tokens: 4096,
            context_length: 128000,
            adapt: Some("openai".into()),
        }
    }
}

// ============================================================================
// ZenCore trait impl
// ============================================================================

impl ZenCore for ZenEngine {
    fn create_session(&self, session_id: Option<&str>) -> Result<()> {
        info!("[{}] create_session", self.instance_id);

        // Abort any in-flight request
        self.abort_current();

        // Clear state
        let mut state = self.state.lock().unwrap();
        state.clear_all();

        // Set session id
        let sid = session_id
            .map(|s| s.to_owned())
            .unwrap_or_else(Self::generate_session_id);
        state.set_session_id(sid.clone());
        *self.session_id.write().unwrap() = Some(sid.clone());
        drop(state);

        // Register working dir in ConfigManager (creates default config if new)
        let working_dir = self.options.read().unwrap().working_dir.clone();
        config_manager::with_conf_manager(|mgr| mgr.register_project(&working_dir));

        // Initialize plugins (skills + custom commands)
        self.initialize_plugins();

        // Fire SessionStart hook (fire-and-forget — non-blockable)
        if self
            .hook_manager
            .has_hooks_for_event(&HookEvent::SessionStart)
        {
            let hm = self.hook_manager.clone();
            let (client, profile) = self.hook_opts();
            let base = self.hook_base(HookEvent::SessionStart);
            tokio::spawn(async move {
                zen_hooks::execute_hooks(
                    &hm,
                    &HookEvent::SessionStart,
                    &HookInput::Session(SessionInput { base }),
                    &ExecuteHooksOptions {
                        client: Some(&client),
                        profile: Some(&profile),
                        ..Default::default()
                    },
                )
                .await;
            });
        }

        // Emit session:ready
        let opts = self.options.read().unwrap();
        self.fire(EngineEvent::SessionReady(SessionReadyData {
            working_dir: opts.working_dir.clone(),
            session_id: sid,
            history_loaded: session_id.is_some(),
            usage: UsageData {
                use_tokens: 0,
                max_tokens: 0,
                prompt_tokens: 0,
            },
            project_input_history: Vec::new(),
        }));

        // Transition main agent to idle
        let mut state = self.state.lock().unwrap();
        state.update_state(MAIN_AGENT_ID, SessionState::Idle);
        self.fire(EngineEvent::StateUpdate(StateUpdateData {
            state: SessionState::Idle,
        }));

        Ok(())
    }

    fn process_user_input(&self, prompt: &str, _original_input: Option<&str>) -> Result<()> {
        info!("[{}] process_user_input: {}", self.instance_id, prompt);

        {
            let mut state = self.state.lock().unwrap();
            state.update_state(MAIN_AGENT_ID, SessionState::Processing);
        }
        self.fire(EngineEvent::StateUpdate(StateUpdateData {
            state: SessionState::Processing,
        }));

        // Create abort token for this request
        let cancel = CancellationToken::new();
        {
            let mut state = self.state.lock().unwrap();
            state.current_abort = Some(cancel.clone());
        }

        // Clone shared resources for the spawned task
        let instance_id = self.instance_id.clone();
        let event_bus = self.event_bus.clone();
        let opts = self.options.read().unwrap().clone();
        let tools = self.get_tools();
        // Debug: log all tools being sent to LLM so we can verify browser tools appear
        {
            let tool_names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
            let mcp_tools: Vec<&str> = tool_names
                .iter()
                .filter(|n| n.starts_with("mcp__"))
                .copied()
                .collect();
            info!(
                "[{}] get_tools: {} total ({} mcp__*): {:?}",
                self.instance_id,
                tool_names.len(),
                mcp_tools.len(),
                mcp_tools
            );
        }
        let messages = {
            let state = self.state.lock().unwrap();
            state.message_history(MAIN_AGENT_ID)
        };
        let http_client = self.http_client.clone();
        let permission_manager = self.permission_manager.clone();
        let response_registry = self.response_registry.clone();

        // Build system prompt
        let system_prompt = if opts.system_prompt.is_empty() {
            "You are a helpful AI assistant.".to_string()
        } else {
            opts.system_prompt.clone()
        };

        // Resolve profile from active UI config first, env fallback second.
        let profile = Self::resolve_model_profile();

        // UserPromptSubmit hook — may update the prompt before it reaches the LLM.
        // Runs synchronously before the spawn so `updatedInput` can modify the prompt.
        let prompt = prompt.to_owned();
        let prompt = if self
            .hook_manager
            .has_hooks_for_event(&HookEvent::UserPromptSubmit)
        {
            let hm = self.hook_manager.clone();
            let (client, hook_profile) = self.hook_opts();
            let base = self.hook_base(HookEvent::UserPromptSubmit);
            let p = prompt.clone();
            let result = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async move {
                    zen_hooks::execute_hooks(
                        &hm,
                        &HookEvent::UserPromptSubmit,
                        &HookInput::UserPromptSubmit(UserPromptSubmitInput { base, prompt: p }),
                        &ExecuteHooksOptions {
                            client: Some(&client),
                            profile: Some(&hook_profile),
                            ..Default::default()
                        },
                    )
                    .await
                })
            });
            if let Some(updated) = result.updated_input {
                updated["prompt"].as_str().unwrap_or(&prompt).to_owned()
            } else {
                prompt
            }
        } else {
            prompt
        };

        // Persist prompt to project history
        let working_dir_for_hist = self.options.read().unwrap().working_dir.clone();
        config_manager::with_conf_manager(|mgr| {
            mgr.save_user_input_to_history(&working_dir_for_hist, &prompt);
        });

        // Build the user message (after hooks may have modified prompt)
        let user_msg = create_user_message(vec![ContentBlock::Text {
            text: prompt.clone(),
        }]);
        let mut messages = messages;
        messages.push(user_msg);

        // Spawn the conversation loop — runs in background, emits events
        let event_bus_spawn = event_bus.clone();
        let hook_manager_spawn = self.hook_manager.clone();
        let session_id_spawn = self.session_id.read().unwrap().clone().unwrap_or_default();
        let cwd_spawn = opts.working_dir.clone();
        tokio::spawn(async move {
            let eb = event_bus_spawn.clone();
            let config = conversation::QueryConfig {
                agent_id: MAIN_AGENT_ID.to_string(),
                working_dir: opts.working_dir.clone(),
                agent_data_dir: opts.agent_data_dir.clone(),
                system_prompt: system_prompt.clone(),
                tools: tools.clone(),
                http_client: http_client.clone(),
                event_bus: event_bus_spawn,
                response_registry: Some(response_registry.clone()),
                permission_checker: permission_manager.clone(),
                profile: profile.clone(),
                thinking: opts.thinking,
                stream: opts.stream,
                is_subagent: false,
                hook_manager: Some(hook_manager_spawn.clone()),
                hook_client: Some(http_client.clone()),
                hook_profile: Some(profile.clone()),
                session_id: session_id_spawn.clone(),
            };

            let result = conversation::query(messages, &config, &cancel).await;

            let stop_reason = match &result {
                Ok(_) => {
                    info!("[{instance_id}] conversation loop completed");
                    None
                }
                Err(e) => {
                    let msg = e.to_string();
                    warn!("[{instance_id}] conversation loop error: {msg}");
                    let classified = query_llm::LlmError::classify(e);
                    if classified.should_emit() {
                        eb.emit(EngineEvent::SessionError(classified.to_session_error()));
                    }
                    Some(msg)
                }
            };

            // Fire Stop hook (non-blockable)
            if hook_manager_spawn.has_hooks_for_event(&HookEvent::Stop) {
                let base = HookInputBase {
                    hook_event_name: HookEvent::Stop,
                    session_id: session_id_spawn.clone(),
                    agent_id: MAIN_AGENT_ID.to_string(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    cwd: cwd_spawn.clone(),
                };
                zen_hooks::execute_hooks(
                    &hook_manager_spawn,
                    &HookEvent::Stop,
                    &HookInput::Stop(StopInput { base, stop_reason }),
                    &ExecuteHooksOptions {
                        client: Some(&http_client),
                        profile: Some(&profile),
                        ..Default::default()
                    },
                )
                .await;
            }

            // Signal idle (unless cancelled)
            if !cancel.is_cancelled() {
                eb.emit(EngineEvent::StateUpdate(StateUpdateData {
                    state: SessionState::Idle,
                }));
            }
        });

        Ok(())
    }

    fn pause_session(&self) {
        info!("[{}] pause_session", self.instance_id);
        self.abort_current();
        let mut state = self.state.lock().unwrap();
        state.update_state(MAIN_AGENT_ID, SessionState::Paused);
        self.fire(EngineEvent::StateUpdate(StateUpdateData {
            state: SessionState::Paused,
        }));
    }

    fn interrupt_session(&self, target_state: SessionState) {
        info!(
            "[{}] interrupt_session → {:?}",
            self.instance_id, target_state
        );
        self.abort_current();
        let mut state = self.state.lock().unwrap();
        state.update_state(MAIN_AGENT_ID, target_state);
        self.fire(EngineEvent::StateUpdate(StateUpdateData {
            state: target_state,
        }));
    }

    fn dispose(&self) {
        info!("[{}] dispose", self.instance_id);
        self.abort_current();
        self.state.lock().unwrap().clear_all();
        self.response_registry.clear();

        // Fire SessionEnd hook (non-blockable, fire-and-forget)
        if self
            .hook_manager
            .has_hooks_for_event(&HookEvent::SessionEnd)
        {
            let hm = self.hook_manager.clone();
            let (client, profile) = self.hook_opts();
            let base = self.hook_base(HookEvent::SessionEnd);
            tokio::spawn(async move {
                zen_hooks::execute_hooks(
                    &hm,
                    &HookEvent::SessionEnd,
                    &HookInput::Session(SessionInput { base }),
                    &ExecuteHooksOptions {
                        client: Some(&client),
                        profile: Some(&profile),
                        ..Default::default()
                    },
                )
                .await;
            });
        }
    }

    fn set_working_dir(&self, dir: &str) {
        info!("[{}] set_working_dir: {dir}", self.instance_id);
        self.options.write().unwrap().working_dir = dir.to_owned();
    }

    fn clear_working_dir(&self) {
        info!("[{}] clear_working_dir", self.instance_id);
    }

    fn update_skip_permissions(&self, skip: bool) {
        let mut opts = self.options.write().unwrap();
        opts.skip_file_edit_permission = skip;
        opts.skip_bash_exec_permission = skip;
        opts.skip_skill_permission = skip;
        opts.skip_mcp_tool_permission = skip;
    }

    fn update_thinking(&self, enabled: bool) {
        self.options.write().unwrap().thinking = enabled;
    }

    fn reload_skills(&self, disabled: &[String]) {
        info!(
            "[{}] reload_skills ({} disabled)",
            self.instance_id,
            disabled.len()
        );
        let config = crate::config::Config::from_env();
        let mut entries = crate::skills::scan::load_all_local_skills(&config);
        if !disabled.is_empty() {
            entries.retain(|e| !disabled.iter().any(|d| d == &e.name));
        }
        self.skill_registry.load_entries(&entries);
        info!(
            "[{}] reload_skills: {} skills loaded",
            self.instance_id,
            self.skill_registry.len()
        );
    }

    fn has_session_tool_results(&self) -> bool {
        let state = self.state.lock().unwrap();
        let history = state.message_history(MAIN_AGENT_ID);
        history.iter().any(|msg| {
            if msg.msg_type != "assistant" {
                return false;
            }
            msg.message
                .content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
        })
    }

    fn add_or_update_mcp_server(&self, cfg: &McpServerConfig, _scope: &str) -> Result<()> {
        info!(
            "[{}] add_or_update_mcp_server: {}",
            self.instance_id, cfg.name
        );
        let instance_id = self.instance_id.clone();
        let registry = self.mcp_registry.clone();
        let builtin_tools_lock = Arc::new(std::sync::RwLock::<Vec<Arc<dyn Tool>>>::new(vec![]));
        // We can't move self into a 'static future. Instead we use a Mutex-protected
        // Vec that we'll merge into builtin_tools synchronously after spawn returns.
        // Simpler approach: use a oneshot channel to send bridge_tools back.
        let name = cfg.name.clone();
        let command = cfg.command.clone();
        let args = cfg.args.clone();
        let env = cfg.env.clone();
        // Shared buffer for the spawned task to write tool objects into.
        let tools_buffer: Arc<Mutex<Vec<Arc<dyn Tool>>>> = Arc::new(Mutex::new(Vec::new()));
        let tools_buffer_clone = Arc::clone(&tools_buffer);
        let _ = builtin_tools_lock;

        let handle = tokio::spawn(async move {
            match registry.spawn(&name, &command, &args, &env).await {
                Ok(tool_infos) => {
                    let count = tool_infos.len();
                    let server_name = name.clone();
                    let reg_clone = registry.clone();
                    let bridge_tools: Vec<Arc<dyn Tool>> = tool_infos
                        .into_iter()
                        .map(|ti| {
                            let full_name = if server_name.starts_with("senclaw-") {
                                let clean_server = &server_name["senclaw-".len()..];
                                let mut clean_tool = ti.name.clone();
                                let prefix = format!("{}_", clean_server);
                                if clean_tool.starts_with(&prefix) {
                                    clean_tool = clean_tool[prefix.len()..].to_string();
                                }
                                format!("mcp__{}__{}", clean_server, clean_tool)
                            } else {
                                format!("mcp__{}__{}", server_name, ti.name)
                            };

                            Arc::new(McpRegistryBridgeTool {
                                full_name,
                                tool_name: ti.name,
                                server_name: server_name.clone(),
                                desc: ti.description,
                                schema: ti.input_schema,
                                registry: reg_clone.clone(),
                            }) as Arc<dyn Tool>
                        })
                        .collect();
                    *tools_buffer_clone.lock().unwrap() = bridge_tools;
                    info!("[{instance_id}] MCP {name}: {count} tool(s) spawned");
                    count
                }
                Err(e) => {
                    warn!("[{instance_id}] MCP {name} spawn failed: {e}");
                    0
                }
            }
        });

        // Synchronously wait for spawn to complete then merge tools into builtin_tools.
        // We're on a tokio thread so we block_in_place to avoid deadlocking.
        tokio::task::block_in_place(|| {
            let rt = tokio::runtime::Handle::current();
            let count = rt.block_on(handle).unwrap_or(0);
            if count > 0 {
                let new_tools = std::mem::take(&mut *tools_buffer.lock().unwrap());
                let prefix = format!("mcp__{}__", cfg.name);
                let mut tools = self.builtin_tools.write().unwrap();
                tools.retain(|t| !t.name().starts_with(&prefix));
                tools.extend(new_tools);
                info!(
                    "[{}] MCP {}: {count} tool(s) added to builtin_tools",
                    self.instance_id, cfg.name
                );
            }
        });
        Ok(())
    }

    fn add_allowed_tool(&self, key: &str) {
        self.permission_manager.add_allowed_tool(key);
    }

    fn respond_to_tool_permission(&self, response: ToolPermissionResponseData) {
        self.response_registry.deliver_tool_permission(response);
    }

    fn respond_to_ask_question(&self, response: AskQuestionResponseData) {
        self.response_registry.deliver_ask_question(response);
    }

    fn respond_to_plan_exit(&self, _response: PlanExitResponseData) {
        // Plan exit responses are handled via event bus
        self.fire(EngineEvent::PlanExitResponse(_response));
    }

    fn set_handlers(&self, handlers: ZenCoreHandlers) {
        *self.handlers.write().unwrap() = handlers;
    }

    fn update_agent_mode(&self, mode: AgentMode) {
        let mut opts = self.options.write().unwrap();
        let changed = opts.agent_mode != mode;
        opts.agent_mode = mode;
        if changed && mode == AgentMode::Plan {
            let mut state = self.state.lock().unwrap();
            state.reset_plan_mode_info_sent();
        }
    }

    fn get_tool_infos(&self) -> Vec<ToolInfo> {
        self.get_tools()
            .iter()
            .map(|t| ToolInfo {
                name: t.name().to_string(),
                description: t.description().to_string(),
                status: "enable".to_string(),
            })
            .collect()
    }
}

// ============================================================================
// Plugin initialization (skills + custom commands)
// ============================================================================

impl ZenEngine {
    pub fn initialize_plugins(&self) {
        let opts = self.options.read().unwrap();
        debug!(
            "[{}] initialize_plugins: working_dir={}",
            self.instance_id, opts.working_dir
        );

        // Build a minimal config for skill scanning
        let config = crate::config::Config::from_env();
        let entries = crate::skills::scan::load_all_local_skills(&config);
        debug!(
            "[{}] scanned {} skill entries",
            self.instance_id,
            entries.len()
        );

        if !entries.is_empty() {
            self.skill_registry.load_entries(&entries);
            debug!(
                "[{}] registered {} skills",
                self.instance_id,
                self.skill_registry.len()
            );
        }
    }
}

// ===== McpRegistryBridgeTool =====
// Wraps a single tool from a spawned MCP subprocess so it can participate in
// the ZenEngine's builtin_tools roster and be visible to the LLM.

struct McpRegistryBridgeTool {
    /// Full tool name sent to the LLM, e.g. `mcp__senclaw-browser__navigate`
    full_name: String,
    /// Short tool name used for subprocess `tools/call`
    tool_name: String,
    /// MCP server name (key in registry)
    server_name: String,
    desc: String,
    schema: Value,
    registry: SharedMcpRegistry,
}

#[async_trait::async_trait]
impl Tool for McpRegistryBridgeTool {
    fn name(&self) -> &str {
        &self.full_name
    }

    fn description(&self) -> &str {
        &self.desc
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    fn is_read_only(&self) -> bool {
        false
    }

    async fn call(&self, input: Value, _ctx: &ToolContext<'_>) -> Result<Vec<ToolOutput>> {
        let result = self
            .registry
            .call_tool(&self.server_name, &self.tool_name, input)
            .await?;
        let summary = match &result {
            Value::String(s) => s.clone(),
            other => serde_json::to_string_pretty(other).unwrap_or_else(|_| format!("{other:?}")),
        };
        Ok(vec![ToolOutput::Result {
            data: result,
            result_for_assistant: summary,
        }])
    }

    fn gen_tool_result_message(&self, data: &Value, input: &Value) -> ToolResultMessage {
        let summary = match data {
            Value::String(s) => s.clone(),
            other => serde_json::to_string_pretty(other).unwrap_or_else(|_| format!("{other:?}")),
        };
        ToolResultMessage {
            title: self.get_display_title(input),
            summary,
            content: data.clone(),
        }
    }

    fn get_display_title(&self, _input: &Value) -> String {
        let name = self.tool_name.replace('_', " ");
        // Capitalize words
        let capitalized = name
            .split_whitespace()
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        let server_display = if self.server_name.starts_with("senclaw-") {
            &self.server_name["senclaw-".len()..]
        } else {
            &self.server_name
        };

        format!("{}: {}", server_display.to_uppercase(), capitalized)
    }

    fn gen_tool_permission(&self, input: &Value) -> Option<ToolPermissionInfo> {
        Some(ToolPermissionInfo {
            title: self.get_display_title(input),
            content: input.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_creation_and_basic_state() {
        let opts = ZenCoreOptions {
            instance_id: "test-1".into(),
            agent_data_dir: "/tmp/test".into(),
            working_dir: "/tmp/test".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        assert_eq!(engine.instance_id, "test-1");
    }

    #[test]
    fn create_session_generates_id() {
        let opts = ZenCoreOptions {
            instance_id: "test-2".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        engine.create_session(None).unwrap();
        let sid = engine.session_id.read().unwrap();
        assert!(sid.is_some());
    }

    #[test]
    fn create_session_with_provided_id() {
        let opts = ZenCoreOptions {
            instance_id: "test-3".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        engine.create_session(Some("my-session")).unwrap();
        let sid = engine.session_id.read().unwrap();
        assert_eq!(sid.as_deref(), Some("my-session"));
    }

    #[test]
    fn pause_session_sets_state() {
        let opts = ZenCoreOptions {
            instance_id: "test-4".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        engine.create_session(None).unwrap();
        engine.pause_session();
        let state = engine.state.lock().unwrap();
        assert_eq!(state.current_state(MAIN_AGENT_ID), SessionState::Paused);
    }

    #[test]
    fn has_session_tool_results_false_on_empty() {
        let opts = ZenCoreOptions {
            instance_id: "test-5".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        assert!(!engine.has_session_tool_results());
    }

    #[test]
    fn update_skip_permissions_toggles_all() {
        let opts = ZenCoreOptions {
            instance_id: "test-6".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        engine.update_skip_permissions(true);
        let o = engine.options.read().unwrap();
        assert!(o.skip_file_edit_permission);
        assert!(o.skip_bash_exec_permission);
        assert!(o.skip_skill_permission);
    }

    #[tokio::test]
    async fn process_user_input_transitions_to_processing() {
        let opts = ZenCoreOptions {
            instance_id: "test-7".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        engine.create_session(None).unwrap();
        engine.process_user_input("hello", None).unwrap();
        let state = engine.state.lock().unwrap();
        assert_eq!(state.current_state(MAIN_AGENT_ID), SessionState::Processing);
    }
}
