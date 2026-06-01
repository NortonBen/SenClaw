//! ZenEngine — per-instance agent runtime orchestrator.
//!
//! Owns the [`EventBus`], [`StateManager`], tool registry, and config.
//! Implements [`ZenCore`] so SemaClaw's [`AgentPool`] can drive it without
//! knowing about internal engine details.
//!
//! Port of TS `ZenEngine` from sema-core.

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

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
use crate::tools::{SkillTool, TaskTool, TodoWriteTool, ToolSearchTool};
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

    // Workbench artifact service (artifact publishing + reverse ops)
    pub workbench_service: Arc<crate::zen_core::workbench::WorkbenchService>,

    /// Tool names that became available via `ToolSearch` during this session.
    /// `tools_for_main_agent` includes these even when `should_defer() == true`,
    /// so the model can actually call what ToolSearch promised. Reset on
    /// `dispose()` / new session.
    pub(crate) discovered_tools:
        Arc<Mutex<std::collections::HashSet<String>>>,

    /// Weak self-reference set during construction. Lets `&self` methods hand
    /// out closures that re-fetch live engine state without holding a strong
    /// ref (which would prevent drop). Mirror of `AgentPool::self_weak`.
    self_weak: Mutex<std::sync::Weak<Self>>,
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

        let workbench_service = Arc::new(crate::zen_core::workbench::WorkbenchService::new(
            event_bus.clone(),
            instance_id.clone(),
            options.working_dir.clone(),
        ));

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
            workbench_service: workbench_service.clone(),
            discovered_tools: Arc::new(Mutex::new(std::collections::HashSet::new())),
            self_weak: Mutex::new(std::sync::Weak::new()),
        });
        *engine.self_weak.lock().unwrap() = Arc::downgrade(&engine);

        // Register engine-dependent tools
        engine.register_tool(Arc::new(TodoWriteTool::new(state)));
        let engine_for_skill = Arc::downgrade(&engine);
        let on_skill_load: crate::tools::skill::OnSkillLoadFn = Arc::new(move |skill_name| {
            if skill_name != "agent-browser" {
                return;
            }
            if let Some(e) = engine_for_skill.upgrade() {
                const BROWSER_TOOLS: &[&str] = &[
                    "mcp__browser__search",
                    "mcp__browser__navigate",
                    "mcp__browser__snapshot",
                    "mcp__browser__click",
                    "mcp__browser__type",
                    "mcp__browser__extract_text",
                    "mcp__browser__extract_structured",
                    "mcp__browser__screenshot",
                    "mcp__browser__fill_form",
                    "mcp__browser__click_and_wait",
                    "mcp__browser__wait",
                    "mcp__browser__new_tab",
                    "mcp__browser__close_tab",
                ];
                let mut set = e.discovered_tools.lock().unwrap();
                for name in BROWSER_TOOLS {
                    set.insert((*name).to_string());
                    tracing::info!("[Skill] pre-discovered browser tool: {name}");
                }
            }
        });
        engine.register_tool(
            Arc::new(SkillTool::new(skill_registry).with_on_load(on_skill_load)),
        );
        // LaunchUI — surfaces deliverables in the WebUI workbench panel.
        engine.register_tool(Arc::new(crate::tools::LaunchUITool::new(workbench_service)));

        // Register static tools (no engine deps)
        engine.register_tools(crate::tools::all_tools());

        // Register ToolSearch — discovery mechanism for deferred tools. Uses
        // Weak<Self> so the resolver re-fetches live `deferred_tools()` on
        // each call (use_tools / Plan / cowork filters apply).
        let engine_for_search = Arc::downgrade(&engine);
        let deferred_resolver: crate::tools::DeferredToolsFn = Arc::new(move || {
            engine_for_search
                .upgrade()
                .map(|e| e.deferred_tools())
                .unwrap_or_default()
        });
        let engine_for_discovery = Arc::downgrade(&engine);
        let register_discovered: crate::tools::tool_search::RegisterDiscoveredFn =
            Arc::new(move |name: &str| {
                if let Some(e) = engine_for_discovery.upgrade() {
                    e.discovered_tools
                        .lock()
                        .unwrap()
                        .insert(name.to_string());
                    tracing::info!("[ToolSearch] discovered tool: {name}");
                }
            });
        engine.register_tool(Arc::new(
            ToolSearchTool::new(deferred_resolver).with_discovery(register_discovered),
        ));

        // EnterPlanMode — flip the engine's `agent_mode` to Plan. Mirror of
        // `ExitPlanMode` (which requests approval). Both are `always_load`
        // builtins so the model doesn't need ToolSearch to find them.
        let engine_for_plan = Arc::downgrade(&engine);
        engine.register_tool(Arc::new(
            crate::tools::EnterPlanModeTool::for_engine(engine_for_plan),
        ));

        // Register TaskTool last so it knows about all other tools.
        // Pass a resolver closure (vs a snapshot) so spawned subagents inherit
        // `use_tools` / Plan-mode / cowork filters as they evolve at runtime.
        let profile = Self::resolve_model_profile();
        let engine_for_resolver = Arc::downgrade(&engine);
        let tools_resolver: crate::tools::task::ToolResolver = Arc::new(move || {
            engine_for_resolver
                .upgrade()
                .map(|e| e.tools_for_main_agent())
                .unwrap_or_default()
        });
        engine.register_tool(Arc::new(TaskTool::new(
            engine.http_client.clone(),
            engine.event_bus.clone(),
            engine.state.clone(),
            engine.permission_manager.clone(),
            crate::tools::task::default_agent_configs(),
            engine.options.read().unwrap().working_dir.clone(),
            engine.options.read().unwrap().agent_data_dir.clone(),
            tools_resolver,
            profile,
        )));

        // Register custom memory directory with MemoryManager if provided
        if let Some(ref custom_memory_dir) = engine.options.read().unwrap().custom_memory_dir {
            if let Some(memory_mgr) = crate::memory::manager::try_get_instance() {
                let opts = engine.options.read().unwrap();
                let folder_key = opts
                    .memory_folder_override
                    .as_deref()
                    .unwrap_or(opts.agent_data_dir.as_str());
                let instance_id_for_log = opts.instance_id.clone();
                memory_mgr.register_custom_memory_dir(
                    folder_key,
                    std::path::PathBuf::from(custom_memory_dir),
                );
                tracing::info!(
                    "[ZenEngine] Registered custom memory dir for instance '{}' (folder={folder_key}): {}",
                    instance_id_for_log,
                    custom_memory_dir
                );
            }
        }

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

    /// Resolve the tool list for the **main agent** turn. This is what gets
    /// serialized into the LLM API `tools` field every turn.
    ///
    /// Filter layers (applied in order — token-saving funnel):
    ///   1. **Registry**: all `builtin_tools` registered on the engine.
    ///   2. **`use_tools` whitelist**: empty = no restriction; otherwise keep
    ///      only names that appear. Mirrors sema-core `getAvailableBuiltinTools`.
    ///   3. **Plan-mode filter**: drops `TodoWrite` (Plan-mode policy).
    ///   4. **Cowork-mode filter**: drops interactive ask-tools for synthetic
    ///      `cowork:*` instance ids (no UI subscriber to answer questions).
    ///   5. **Defer filter** (claude-code pattern): drops tools whose
    ///      `should_defer() == true && !always_load()`. The LLM discovers
    ///      these via `ToolSearch`. Cuts ~80% of tool tokens for MCP-heavy
    ///      workloads.
    ///   6. **Stable sort**: alphabetical-by-name so the tool list is
    ///      byte-identical turn-over-turn — preserves Anthropic prompt cache
    ///      hits.
    ///
    /// This method does NOT apply the subagent exclusion list — call
    /// [`Self::tools_for_subagent`] for that path.
    pub fn tools_for_main_agent(&self) -> Vec<Arc<dyn Tool>> {
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

        // Plan mode is read-only: physically strip every mutating tool so
        // the agent CANNOT edit files, run shell commands, or write todos —
        // the system prompt asks nicely, this enforces. The only non-read-only
        // tool kept is `ExitPlanMode`, which is how the agent requests approval
        // to leave plan mode and begin executing. This closes the gap where an
        // aggressive model (or prompt injection) ignores the prompt-level
        // constraint and calls Edit/Write/Bash anyway.
        if is_plan {
            // Tools allowed in plan mode despite `is_read_only() == false`:
            // they're research/escape tools with no destructive local effect.
            // WebFetch/WebSearch fetch external info (the "research" in
            // "read-only research"); ExitPlanMode is the approval escape hatch.
            const PLAN_ALLOWED: &[&str] = &["ExitPlanMode", "WebFetch", "WebSearch"];
            filtered.retain(|t| t.is_read_only() || PLAN_ALLOWED.contains(&t.name()));
        }

        // Cowork agents use synthetic JIDs (`cowork:{workspace}:{member}`). Interactive
        // ask tools block on WS `question:request`; dispatch often has no subscriber
        // answering for that JID, so tasks appear "stuck" until timeout.
        if self.instance_id.starts_with("cowork:") {
            filtered.retain(|t| {
                let n = t.name();
                n != "AskUser" && n != "AskUserQuestion"
            });
        }

        // Layer 5 — defer filter. Deferred tools are excluded unless either:
        //   - they opted into `always_load()` (e.g. ToolSearch itself), OR
        //   - the model has already discovered them via a prior `ToolSearch`
        //     call this session. Discovery flips the tool into the active set
        //     so subsequent turns can actually invoke it. Mirrors claude-code's
        //     "lazy load" UX without breaking the dispatch lookup.
        let discovered = self.discovered_tools.lock().unwrap().clone();
        filtered.retain(|t| {
            t.always_load() || !t.should_defer() || discovered.contains(t.name())
        });

        // Layer 6 — stable sort for prompt-cache stability.
        filtered.sort_by(|a, b| a.name().cmp(b.name()));

        filtered
    }

    /// Return tools currently marked `should_defer() && !always_load()`. These
    /// are NOT sent in the initial prompt — the LLM finds them through
    /// `ToolSearch`. Layer 1-4 filters (`use_tools`, Plan, cowork) still apply
    /// so admins can completely disable a tool, not just defer it.
    pub fn deferred_tools(&self) -> Vec<Arc<dyn Tool>> {
        let opts = self.options.read().unwrap();
        let use_tools = &opts.use_tools;
        let is_plan = opts.agent_mode == AgentMode::Plan;
        let tools = self.builtin_tools.read().unwrap();

        let mut deferred: Vec<Arc<dyn Tool>> = tools
            .iter()
            .filter(|t| {
                let name = t.name();
                if !use_tools.is_empty() && !use_tools.contains(&name.to_string()) {
                    return false;
                }
                if is_plan && name == "TodoWrite" {
                    return false;
                }
                if self.instance_id.starts_with("cowork:")
                    && (name == "AskUser" || name == "AskUserQuestion")
                {
                    return false;
                }
                t.should_defer() && !t.always_load()
            })
            .cloned()
            .collect();
        deferred.sort_by(|a, b| a.name().cmp(b.name()));
        deferred
    }

    /// Backwards-compatible alias used by existing call sites. Prefer
    /// [`Self::tools_for_main_agent`] in new code.
    pub fn get_tools(&self) -> Vec<Arc<dyn Tool>> {
        self.tools_for_main_agent()
    }

    /// Resolve the tool list for a **subagent** turn. Applies all main-agent
    /// filters then layers two more:
    ///
    ///   5. **`SUBAGENT_EXCLUDED_TOOLS`** — strip Task / bg-job / picker /
    ///      plan-exit / todo tools (subagents must not spawn nested subagents
    ///      and can't surface UI prompts). Mirrors sema-core's
    ///      `SUBAGENT_EXCLUDED_TOOLS` set — saves ~7 tool definitions per
    ///      subagent turn.
    ///   6. **`agent_tools` whitelist** — when provided and not `["*"]`, keep
    ///      only the tools the subagent persona is declared to use. This is
    ///      the per-persona `tools` field from the agent config.
    ///
    /// Returns an empty list if every tool was filtered out.
    pub fn tools_for_subagent(&self, agent_tools: Option<&[String]>) -> Vec<Arc<dyn Tool>> {
        use crate::zen_core::prompt::SUBAGENT_EXCLUDED_TOOLS;
        let mut tools = self.tools_for_main_agent();
        tools.retain(|t| !SUBAGENT_EXCLUDED_TOOLS.contains(&t.name()));
        if let Some(allowed) = agent_tools {
            if !allowed.iter().any(|t| t == "*") {
                let set: std::collections::HashSet<&str> =
                    allowed.iter().map(|s| s.as_str()).collect();
                tools.retain(|t| set.contains(t.name()));
            }
        }
        tools
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
            | EngineEvent::ConfigNoModels(_)
            // Workbench events — consumed by WorkbenchBridge via event_bus subscription
            | EngineEvent::WorkbenchNew(_)
            | EngineEvent::WorkbenchServiceReady(_)
            | EngineEvent::WorkbenchServiceCrashed(_)
            | EngineEvent::WorkbenchServiceStopped(_) => {}
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
                vision: None,
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
            vision: None,
        }
    }

    // ============================================================
    // Context management for multi-tenant support
    // ============================================================

    /// Create an EngineStore from this engine's current state.
    ///
    /// This can be used with `run_with_engine()` to execute operations
    /// within this engine's context, enabling automatic access to
    /// the engine's resources without explicit passing.
    pub fn create_engine_store(&self, profile: ModelProfile) -> super::EngineStore {
        super::EngineStore {
            instance_id: self.instance_id.clone(),
            working_dir: self.options.read().unwrap().working_dir.clone(),
            agent_data_dir: self.options.read().unwrap().agent_data_dir.clone(),
            core_config: super::CoreConfig {
                model_profile: profile,
                thinking: self.options.read().unwrap().thinking,
                stream: self.options.read().unwrap().stream,
                agent_mode: self.options.read().unwrap().agent_mode.as_str().to_string(),
                use_tools: self.options.read().unwrap().use_tools.clone(),
            },
            event_bus: self.event_bus.clone(),
            state_manager: Arc::clone(&self.state),
            mcp_manager: self.mcp_manager.clone(),
            hook_manager: self.hook_manager.clone(),
        }
    }

    /// Run an operation within this engine's context.
    ///
    /// This is a convenience method that combines `create_engine_store()`
    /// with `run_with_engine()`.
    pub async fn run_in_context<F, Fut, T>(&self, profile: ModelProfile, f: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T> + Send,
        T: Send + 'static,
    {
        let store = self.create_engine_store(profile);
        super::run_with_engine(store, f).await
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
        let tools_initial = self.get_tools();
        // Resolver re-evaluates the live tool set each turn so ToolSearch
        // discoveries flow into subsequent turns within this same user input.
        let engine_for_tools = self.self_weak.lock().unwrap().clone();
        let tools_resolver: crate::zen_core::conversation::ToolsResolver = Arc::new(move || {
            engine_for_tools
                .upgrade()
                .map(|e| e.tools_for_main_agent())
                .unwrap_or_default()
        });
        // Keep `tools` binding for the existing log lines below.
        let _tools = tools_initial;
        // Debug: log all tools being sent to LLM so we can verify browser tools appear
        // {
        //     let tool_names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        //     let mcp_tools: Vec<&str> = tool_names
        //         .iter()
        //         .filter(|n| n.starts_with("mcp__"))
        //         .copied()
        //         .collect();
        //     info!(
        //         "[{}] get_tools: {} total ({} mcp__*): {:?}",
        //         self.instance_id,
        //         tool_names.len(),
        //         mcp_tools.len(),
        //         mcp_tools
        //     );
        // }
        let messages = {
            let state = self.state.lock().unwrap();
            state.message_history(MAIN_AGENT_ID)
        };
        let http_client = self.http_client.clone();
        let permission_manager = self.permission_manager.clone();
        let response_registry = self.response_registry.clone();
        let state_for_spawn = self.state.clone();

        // Build system prompt (stable base + dynamic system context appended).
        // When the Skill tool is registered we append a skills reminder so the
        // LLM can auto-trigger skills by metadata (`name` + `description` +
        // `when-to-use`) — mirrors sema-core `generateSkillsReminder`.
        let has_skill_tool = self
            .builtin_tools
            .read()
            .unwrap()
            .iter()
            .any(|t| t.name() == "Skill");
        let skills_reminder = if has_skill_tool {
            self.build_skills_reminder()
        } else {
            None
        };
        // Build deferred-tools reminder so the LLM knows ToolSearch can load
        // specialized tools on demand.
        let deferred_reminder = self.build_deferred_tools_reminder();
        
        let plan_mode_reminder = if opts.agent_mode == AgentMode::Plan {
            let plans_dir = std::path::Path::new(&opts.agent_data_dir)
                .join(".sema")
                .join("plans")
                .join("") // ensure trailing slash
                .to_string_lossy()
                .to_string();
            Some(crate::zen_core::prompt::plan_mode_reminder(&plans_dir))
        } else {
            None
        };

        let system_prompt = Self::assemble_system_prompt(
            &opts.system_prompt,
            &opts.working_dir,
            skills_reminder.as_deref(),
            deferred_reminder.as_deref(),
            plan_mode_reminder.as_deref(),
        );

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

        // Build the user message (after hooks may have modified prompt).
        // On the very first turn inject volatile context (SENCLAW.md, date) as a
        // hidden <system-reminder> block so it doesn't destabilise the system prompt
        // for prompt caching.
        //
        // SENCLAW.md is only relevant for sessions that operate on a real
        // workspace (code editing, cowork). For regular chat JIDs (`web:*`,
        // `app:*`, `virtual:*`, etc.) the project doc is noise — wastes ~2k
        // tokens per turn and dilutes the model's attention from the actual
        // user query. Date-only context is still injected for everyone.
        let user_msg = {
            let mut blocks = Vec::<ContentBlock>::new();
            if messages.is_empty() {
                let include_project_doc = Self::instance_uses_workspace(&self.instance_id);
                if let Some(ctx) = Self::collect_first_turn_context(
                    &opts.working_dir,
                    include_project_doc,
                ) {
                    blocks.push(ContentBlock::Text { text: ctx });
                }
            }
            // Skill pre-match: scan the prompt against loaded skill triggers
            // and surface a hard recommendation if any match. Mirrors the
            // claude-code pattern of a "preferred skill" hint, but driven by
            // keyword overlap (no LLM call). The reminder is part of the user
            // message so it gets the model's full attention on the very first
            // pass — vs the skill list at the end of the system prompt which
            // the model often skims.
            if let Some(hint) = self.build_skill_match_reminder(&prompt) {
                blocks.push(ContentBlock::Text { text: hint });
            }
            // Per-turn language lock. Small models can default to English or
            // Chinese even when the user writes another language. Keep this
            // reminder close to the prompt, but avoid mentioning "thinking
            // blocks" because some models copy that phrase into the visible
            // answer.
            if let Some(lang) = detect_user_language(&prompt) {
                blocks.push(ContentBlock::Text {
                    text: format!(
                        "<system-reminder>\nReply in {lang}. Do not include hidden reasoning, chain-of-thought, or thinking blocks in the final answer.\n</system-reminder>"
                    ),
                });
            }
            blocks.push(ContentBlock::Text {
                text: prompt.clone(),
            });
            create_user_message(blocks)
        };
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
                tools: tools_resolver.clone(),
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
                enable_cache: false,
            };

            let result = conversation::query(messages, &config, &cancel).await;

            if let Ok(msgs) = &result {
                let mut st = state_for_spawn.lock().unwrap();
                st.set_message_history(MAIN_AGENT_ID, msgs.clone());
            }

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
        self.workbench_service.shutdown();
        self.discovered_tools.lock().unwrap().clear();

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

    fn set_use_tools(&self, tools: Vec<String>) {
        info!("[{}] set_use_tools: {:?}", self.instance_id, tools);
        self.options.write().unwrap().use_tools = tools;
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
        let request_timeout_secs = cfg.request_timeout_secs;
        // Shared buffer for the spawned task to write tool objects into.
        let tools_buffer: Arc<Mutex<Vec<Arc<dyn Tool>>>> = Arc::new(Mutex::new(Vec::new()));
        let tools_buffer_clone = Arc::clone(&tools_buffer);
        let _ = builtin_tools_lock;

        let handle = tokio::spawn(async move {
            let timeout = Duration::from_secs(request_timeout_secs.unwrap_or(300));
            match registry.spawn(&name, &command, &args, &env, timeout).await {
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

    fn respond_to_plan_exit(&self, response: PlanExitResponseData) {
        // Deliver the user's choice to the suspended `ExitPlanMode` tool. The
        // tool registered a waiter via `register_ask_question(agent_id)` and
        // reads `answers["selected"]`, so we shape the response accordingly.
        // Without this delivery the tool blocks forever and the agent hangs.
        let mut answers = std::collections::HashMap::new();
        answers.insert("selected".to_string(), response.selected.clone());
        self.response_registry.deliver_ask_question(AskQuestionResponseData {
            agent_id: response.agent_id.clone(),
            answers,
        });

        // On approval, flip back to Agent mode so the agent can actually
        // execute the plan (read-only tool filtering is lifted). On
        // "cancelled" we stay in Plan mode — the user rejected, nothing to do.
        match response.selected.as_str() {
            "startEditing" | "clearContextAndStart" => {
                self.update_agent_mode(AgentMode::Agent);
            }
            _ => {}
        }

        // Keep the event-bus emit so any observers (logging, future hooks)
        // still see the resolution.
        self.fire(EngineEvent::PlanExitResponse(response));
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
    /// Last non-empty main-agent assistant text from persisted transcript (after `query` runs).
    pub fn last_main_assistant_visible_text(&self) -> String {
        let state = self.state.lock().unwrap();
        for msg in state.message_history(MAIN_AGENT_ID).iter().rev() {
            if msg.msg_type != "assistant" {
                continue;
            }
            let (text, _, _) = conversation::extract_content(msg);
            if !text.trim().is_empty() {
                return text;
            }
        }
        String::new()
    }

    /// Assemble the final system prompt.
    ///
    /// Structure:
    /// 1. Base prompt (caller-supplied or default) — kept stable so LLM caches it.
    /// 2. Core behavioural directives — static text, also stable.
    /// 3. System context (cwd, OS, shell, git status) — dynamic but small; appended
    ///    last so any prefix cache hit on (1)+(2) is preserved when context changes.
    fn assemble_system_prompt(
        base: &str,
        working_dir: &str,
        skills_reminder: Option<&str>,
        deferred_reminder: Option<&str>,
        plan_mode_reminder: Option<&str>,
    ) -> String {
        // Default to the full sema-core-compatible SYSTEM_PROMPT when caller
        // doesn't override. Matches `code-old/sema-code-core/prompt/system.ts`.
        let base = if base.trim().is_empty() {
            crate::zen_core::prompt::SYSTEM_PROMPT
        } else {
            base
        };

        let sys_ctx = Self::collect_system_context(working_dir);

        let mut out = format!("{base}\n\n# System\n{sys_ctx}");
        if let Some(reminder) = skills_reminder {
            out.push_str("\n\n");
            out.push_str(reminder);
        }
        if let Some(reminder) = deferred_reminder {
            out.push_str("\n\n");
            out.push_str(reminder);
        }
        if let Some(reminder) = plan_mode_reminder {
            out.push_str("\n\n");
            out.push_str(reminder);
        }
        out
    }

    /// Build the deferred-tools system reminder. Returns `None` when zero
    /// tools are deferred so callers skip the empty block.
    fn build_deferred_tools_reminder(&self) -> Option<String> {
        use crate::zen_core::prompt::{render_deferred_tools_reminder, DeferredToolHint};
        let deferred = self.deferred_tools();
        if deferred.is_empty() {
            return None;
        }
        // Materialize hints so we don't hold tool refs across closure bounds.
        let hints: Vec<(String, String)> = deferred
            .iter()
            .map(|t| (t.name().to_string(), t.search_hint()))
            .collect();
        let rows: Vec<DeferredToolHint<'_>> = hints
            .iter()
            .map(|(n, h)| DeferredToolHint {
                name: n.as_str(),
                search_hint: h.clone(),
            })
            .collect();
        render_deferred_tools_reminder(&rows)
    }

    /// Render the metadata-driven skills reminder block when the `Skill` tool
    /// is registered. Returns `None` when there are zero auto-invokable skills.
    fn build_skills_reminder(&self) -> Option<String> {
        use crate::zen_core::prompt::{render_skills_reminder, SkillReminderRow};
        // Snapshot skill names then re-fetch metadata so we don't hold the
        // registry lock across the borrow into SkillReminderRow.
        let names = self.skill_registry.names();
        let skills: Vec<_> = names
            .iter()
            .filter_map(|n| self.skill_registry.find(n))
            .collect();
        let rows: Vec<SkillReminderRow<'_>> = skills
            .iter()
            .map(|s| SkillReminderRow {
                name: s.metadata.name.as_str(),
                description: s.metadata.description.as_str(),
                when_to_use: s.metadata.when_to_use.as_deref(),
                disable_model_invocation: s.metadata.disable_model_invocation,
            })
            .collect();
        render_skills_reminder(&rows)
    }

    /// Collect runtime system context: working dir, OS, shell, git status.
    fn collect_system_context(working_dir: &str) -> String {
        let os = std::env::consts::OS;
        let shell = std::env::var("SHELL")
            .ok()
            .and_then(|s| {
                std::path::Path::new(&s)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        let git_status = std::process::Command::new("git")
            .args(["-C", working_dir, "status", "--short", "--branch"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let mut lines = vec![
            format!("- Working directory: {working_dir}"),
            format!("- OS: {os}"),
            format!("- Shell: {shell}"),
        ];
        if let Some(gs) = git_status {
            lines.push(format!("- Git status:\n{}", Self::cap_git_status(&gs)));
        }
        lines.join("\n")
    }

    /// Bound the `git status` block injected into the system prompt. A dirty
    /// monorepo (many modified/untracked files) can emit hundreds of lines that
    /// are resent on every request for no benefit — the model only needs the
    /// branch line and a representative sample. Keep the first
    /// [`GIT_STATUS_MAX_LINES`] lines (the `--branch` header is always line 1)
    /// and summarize the rest.
    fn cap_git_status(gs: &str) -> String {
        const GIT_STATUS_MAX_LINES: usize = 30;
        let total = gs.lines().count();
        if total <= GIT_STATUS_MAX_LINES {
            return gs.to_string();
        }
        let kept: Vec<&str> = gs.lines().take(GIT_STATUS_MAX_LINES).collect();
        format!(
            "{}\n… {} more changed path(s) omitted (run `git status` for the full list)",
            kept.join("\n"),
            total - GIT_STATUS_MAX_LINES
        )
    }

    /// Pre-match user prompt against installed skills' `when-to-use` triggers
    /// and return a hard skill recommendation block when one matches.
    ///
    /// The block is appended to the user message (not the system prompt) so it
    /// sits adjacent to the actual query — models attend much more strongly
    /// here than to the skill list buried in the system prompt.
    ///
    /// Match heuristic:
    ///   - lowercase both sides
    ///   - require ≥3 distinct word overlaps OR an explicit quoted-trigger
    ///     ("…") substring hit
    ///   - ignore skills with `disable_model_invocation` (user-only)
    fn build_skill_match_reminder(&self, prompt: &str) -> Option<String> {
        let prompt_lower = prompt.to_lowercase();
        let prompt_words: std::collections::HashSet<String> = prompt_lower
            .split(|c: char| !c.is_alphanumeric() && c != '\u{0301}' && c != '\u{0303}')
            .filter(|w| w.len() >= 3)
            .map(|w| w.to_string())
            .collect();

        let names = self.skill_registry.names();
        let mut best: Option<(u32, String, String)> = None;
        for n in &names {
            let Some(skill) = self.skill_registry.find(n) else { continue };
            if skill.metadata.disable_model_invocation {
                continue;
            }
            let Some(when) = skill.metadata.when_to_use.as_deref() else { continue };
            let when_lower = when.to_lowercase();

            // Score = exact quoted-trigger substring hits (heavy) + word overlap.
            let mut score: u32 = 0;
            for quote in extract_quoted_phrases(&when_lower) {
                if prompt_lower.contains(&quote) {
                    score += 50;
                }
            }
            for word in when_lower
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() >= 3)
            {
                if prompt_words.contains(word) {
                    score += 5;
                }
            }
            if score >= 15 {
                if best.as_ref().map(|(s, _, _)| score > *s).unwrap_or(true) {
                    best = Some((score, skill.metadata.name.clone(), skill.metadata.description.clone()));
                }
            }
        }

        let (_, name, desc) = best?;
        let first_sentence = desc.split('.').next().unwrap_or(&desc).trim();
        Some(format!(
            "<system-reminder>\n\
Skill hint: `{name}` may help with this request — {first_sentence}.\n\
\n\
**Workflow:**\n\
1. Invoke it with `Skill {{ \"skill\": \"{name}\" }}` to load its instructions.\n\
2. Follow the skill's workflow if it fits the user's exact request.\n\
3. For time-sensitive or external data, use a live data tool; do not answer from memory.\n\
</system-reminder>\n\n"
        ))
    }

    /// Whether the given instance id corresponds to an agent that operates
    /// inside a real workspace (code edits, file ops, project bash). Only
    /// these sessions get `SENCLAW.md` injected into their first user turn;
    /// regular chat agents (`web:*`, `app:*`, `virtual:*`) skip it to save
    /// ~2k tokens per turn and avoid polluting attention with project docs
    /// irrelevant to the chat query.
    fn instance_uses_workspace(instance_id: &str) -> bool {
        // Convention from `code_engine::agent_builder::code_session_jid`:
        // `code-chat:<group>`. Cowork agents (`cowork:<workspace>:<member>`)
        // also operate inside a workspace.
        instance_id.starts_with("code-chat:")
            || instance_id.starts_with("cowork:")
            || instance_id.starts_with("code:")
    }

    /// Read `SENCLAW.md` (walks up from `working_dir`) and current date.
    /// Returns a `<system-reminder>` block to inject into the first user turn,
    /// or `None` if nothing useful was found.
    ///
    /// When `include_project_doc` is `false`, only the date line is emitted —
    /// the project markdown is skipped entirely. Used for non-workspace agents.
    ///
    /// Reads `SENCLAW.md` first, then falls back to `CLAUDE.md` for backward
    /// compatibility with existing repos that haven't renamed yet.
    fn collect_first_turn_context(
        working_dir: &str,
        include_project_doc: bool,
    ) -> Option<String> {
        let date = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let mut parts: Vec<String> = vec![format!("Today's date is {date}.")];

        if include_project_doc {
            // Walk up the directory tree looking for SENCLAW.md, then CLAUDE.md.
            let mut dir = std::path::Path::new(working_dir).to_path_buf();
            let project_doc: Option<(&'static str, String)> = loop {
                let mut hit: Option<(&'static str, String)> = None;
                for fname in ["SENCLAW.md", "CLAUDE.md"] {
                    let candidate = dir.join(fname);
                    if candidate.exists() {
                        if let Ok(content) = std::fs::read_to_string(&candidate) {
                            hit = Some((fname, content));
                            break;
                        }
                    }
                }
                if let Some(found) = hit {
                    break Some(found);
                }
                if !dir.pop() {
                    break None;
                }
            };
            if let Some((fname, content)) = project_doc {
                if !content.trim().is_empty() {
                    parts.push(format!(
                        "Project instructions ({fname}):\n{}",
                        content.trim()
                    ));
                }
            }
        }

        if parts.len() == 1 && parts[0].contains("date") {
            // Only date — still inject so the model knows the day.
            return Some(format!("<system-reminder>\n{}\n</system-reminder>\n\n", parts[0]));
        }

        Some(format!(
            "<system-reminder>\n{}\n</system-reminder>\n\n",
            parts.join("\n\n")
        ))
    }

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

    // ===== Lazy-load policy =====
    //
    // Mirrors `McpBridgeTool::should_defer` (the *other* MCP wrapper used by
    // `refresh_mcp_tools`). Both bridge structs need identical defer policy
    // or some MCP tools will leak into the initial prompt.
    fn should_defer(&self) -> bool {
        !crate::mcp::bridge::ALWAYS_LOADED_MCP_TOOLS.contains(&self.full_name.as_str())
    }

    fn search_hint(&self) -> String {
        // `server_name — tool_name — first sentence of description`
        let first_sentence = self
            .desc
            .split('.')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        let server_display = self
            .server_name
            .strip_prefix("senclaw-")
            .unwrap_or(&self.server_name);
        if first_sentence.is_empty() {
            format!("{server_display} {tool}", tool = self.tool_name)
        } else {
            format!(
                "{server_display} {tool} — {first_sentence}",
                tool = self.tool_name
            )
        }
    }
}

/// Best-effort detection of the user's language from a message, returning a
/// human-readable name to lock into a per-turn reminder. Returns `None` when
/// the script is ambiguous (plain ASCII) — the system prompt's generic
/// "user's language" rule covers that case.
///
/// Scoped to the languages this deployment actually serves (Vietnamese,
/// Chinese, English); not a general language identifier.
fn detect_user_language(text: &str) -> Option<&'static str> {
    let mut has_cjk = false;
    let mut has_viet = false;
    for c in text.chars() {
        let u = c as u32;
        if (0x4E00..=0x9FFF).contains(&u) || (0x3400..=0x4DBF).contains(&u) {
            has_cjk = true;
        } else if (0x1EA0..=0x1EFF).contains(&u)        // Latin Extended Additional — almost all Vietnamese
            || matches!(u, 0x0110 | 0x0111             // Đ đ
                          | 0x01A0 | 0x01A1            // Ơ ơ
                          | 0x01AF | 0x01B0            // Ư ư
                          | 0x0102 | 0x0103)           // Ă ă
        {
            has_viet = true;
        }
    }
    if has_viet {
        Some("Vietnamese")
    } else if has_cjk {
        Some("Chinese")
    } else {
        None
    }
}

/// Extract substrings inside straight or curly double-quotes. Used by skill
/// pre-match to give high weight to explicit example triggers like
/// `e.g. "tìm giá vàng hôm nay"` in `when-to-use` descriptions.
fn extract_quoted_phrases(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut inside = false;
    for ch in s.chars() {
        match ch {
            '"' | '\u{201C}' | '\u{201D}' => {
                if inside {
                    let trimmed = current.trim().to_string();
                    if trimmed.len() >= 3 {
                        out.push(trimmed);
                    }
                    current.clear();
                    inside = false;
                } else {
                    inside = true;
                }
            }
            _ if inside => current.push(ch),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_user_language_covers_vi_zh_en() {
        assert_eq!(detect_user_language("tìm kiếm giá vàng hôm nay"), Some("Vietnamese"));
        assert_eq!(detect_user_language("đổi mật khẩu giúp tôi"), Some("Vietnamese"));
        assert_eq!(detect_user_language("今天黄金价格"), Some("Chinese"));
        // Plain ASCII is ambiguous → defer to the system prompt's generic rule.
        assert_eq!(detect_user_language("what is the gold price today"), None);
    }

    #[test]
    fn extract_quoted_phrases_straight_quotes() {
        let out = extract_quoted_phrases("e.g. \"tìm giá vàng hôm nay\", \"screenshot github\"");
        assert_eq!(out, vec!["tìm giá vàng hôm nay", "screenshot github"]);
    }

    #[test]
    fn extract_quoted_phrases_curly() {
        let out = extract_quoted_phrases("\u{201C}hello world\u{201D}");
        assert_eq!(out, vec!["hello world"]);
    }

    #[test]
    fn extract_quoted_phrases_empty_when_none() {
        assert!(extract_quoted_phrases("no quotes here").is_empty());
    }

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

    #[test]
    fn tools_for_main_agent_respects_use_tools_whitelist() {
        let opts = ZenCoreOptions {
            instance_id: "test-tools-1".into(),
            use_tools: vec!["Bash".into(), "Read".into()],
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let tools = engine.tools_for_main_agent();
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"Bash"));
        assert!(names.contains(&"Read"));
        assert!(!names.contains(&"Write"));
        assert!(!names.contains(&"Glob"));
    }

    #[test]
    fn tools_for_main_agent_empty_use_tools_returns_all() {
        let opts = ZenCoreOptions {
            instance_id: "test-tools-2".into(),
            use_tools: vec![],
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let tools = engine.tools_for_main_agent();
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"Bash"));
        assert!(names.contains(&"TodoWrite"));
    }

    #[test]
    fn tools_for_main_agent_plan_mode_drops_todo_write() {
        let opts = ZenCoreOptions {
            instance_id: "test-tools-3".into(),
            agent_mode: AgentMode::Plan,
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let tools = engine.tools_for_main_agent();
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"TodoWrite"));
    }

    #[test]
    fn tools_for_main_agent_plan_mode_strips_write_tools_keeps_readonly() {
        let opts = ZenCoreOptions {
            instance_id: "test-plan-enforce".into(),
            agent_mode: AgentMode::Plan,
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let names: Vec<String> = engine
            .tools_for_main_agent()
            .iter()
            .map(|t| t.name().to_string())
            .collect();
        let has = |n: &str| names.iter().any(|x| x == n);
        // Mutating tools are physically stripped in plan mode.
        for write_tool in ["Write", "Edit", "NotebookEdit", "Bash", "TodoWrite"] {
            assert!(!has(write_tool), "plan mode must strip {write_tool}");
        }
        // Read-only research tools survive.
        for ro in ["Read", "Grep", "Glob"] {
            assert!(has(ro), "plan mode must keep {ro}");
        }
        // ExitPlanMode (non-read-only escape hatch) survives.
        assert!(has("ExitPlanMode"), "plan mode must keep ExitPlanMode");
    }

    #[test]
    fn agent_mode_keeps_write_tools() {
        // Sanity: in Agent mode, write tools are present (the plan-mode
        // strip is mode-gated, not global).
        let opts = ZenCoreOptions {
            instance_id: "test-agent-mode".into(),
            agent_mode: AgentMode::Agent,
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let names: Vec<String> = engine
            .tools_for_main_agent()
            .iter()
            .map(|t| t.name().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "Write"));
        assert!(names.iter().any(|n| n == "Bash"));
    }

    #[test]
    fn respond_to_plan_exit_unblocks_tool_and_flips_mode() {
        // Engine starts in Plan mode. A waiter registered under the agent_id
        // (as ExitPlanMode does) must receive the "selected" answer, and the
        // mode must flip back to Agent on approval.
        let opts = ZenCoreOptions {
            instance_id: "test-plan-exit".into(),
            agent_mode: AgentMode::Plan,
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let mut rx = engine.response_registry.register_ask_question("agent-x");

        engine.respond_to_plan_exit(PlanExitResponseData {
            agent_id: "agent-x".into(),
            selected: "startEditing".into(),
        });

        // The waiter got the choice.
        let answer = rx.try_recv().expect("plan-exit response delivered to waiter");
        assert_eq!(answer.answers.get("selected").map(String::as_str), Some("startEditing"));
        // Mode flipped back to Agent.
        assert_eq!(engine.options.read().unwrap().agent_mode, AgentMode::Agent);
    }

    #[test]
    fn respond_to_plan_exit_cancel_keeps_plan_mode() {
        let opts = ZenCoreOptions {
            instance_id: "test-plan-cancel".into(),
            agent_mode: AgentMode::Plan,
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let mut rx = engine.response_registry.register_ask_question("agent-y");

        engine.respond_to_plan_exit(PlanExitResponseData {
            agent_id: "agent-y".into(),
            selected: "cancelled".into(),
        });

        let answer = rx.try_recv().expect("cancel still delivered");
        assert_eq!(answer.answers.get("selected").map(String::as_str), Some("cancelled"));
        // Cancel → stays in Plan mode.
        assert_eq!(engine.options.read().unwrap().agent_mode, AgentMode::Plan);
    }

    #[test]
    fn tools_for_main_agent_cowork_drops_ask_tools() {
        let opts = ZenCoreOptions {
            instance_id: "cowork:ws1:alice".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let tools = engine.tools_for_main_agent();
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"AskUser"));
        assert!(!names.contains(&"AskUserQuestion"));
    }

    #[test]
    fn tools_for_subagent_strips_excluded_set() {
        let opts = ZenCoreOptions {
            instance_id: "test-sub-1".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let tools = engine.tools_for_subagent(None);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        // SUBAGENT_EXCLUDED_TOOLS items must be gone.
        for excluded in ["Task", "TodoWrite", "PeekBgJob", "ExitPlanMode"] {
            assert!(!names.contains(&excluded), "should drop {excluded}");
        }
        // Read/Bash should still be there.
        assert!(names.contains(&"Read"));
        assert!(names.contains(&"Bash"));
    }

    #[test]
    fn tools_for_subagent_respects_agent_whitelist() {
        let opts = ZenCoreOptions {
            instance_id: "test-sub-2".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let allowed = vec!["Read".to_string(), "Glob".to_string()];
        let tools = engine.tools_for_subagent(Some(&allowed));
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert_eq!(
            names.iter().filter(|n| **n == "Read" || **n == "Glob").count(),
            2
        );
        assert!(!names.contains(&"Bash"));
        assert!(!names.contains(&"Write"));
    }

    #[test]
    fn tools_for_subagent_star_inherits_full_main_minus_excluded() {
        let opts = ZenCoreOptions {
            instance_id: "test-sub-3".into(),
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let star = vec!["*".to_string()];
        let tools_star = engine.tools_for_subagent(Some(&star));
        let names_star: std::collections::HashSet<&str> =
            tools_star.iter().map(|t| t.name()).collect();
        let tools_none = engine.tools_for_subagent(None);
        let names_none: std::collections::HashSet<&str> =
            tools_none.iter().map(|t| t.name()).collect();
        assert_eq!(names_star, names_none);
    }

    #[test]
    fn tools_for_subagent_inherits_use_tools_filter() {
        let opts = ZenCoreOptions {
            instance_id: "test-sub-4".into(),
            use_tools: vec!["Bash".into(), "Read".into(), "Task".into()],
            ..Default::default()
        };
        let engine = ZenEngine::new(opts, None);
        let tools = engine.tools_for_subagent(None);
        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        // Bash + Read survive use_tools filter. Task gets stripped by SUBAGENT_EXCLUDED_TOOLS.
        assert!(names.contains(&"Bash"));
        assert!(names.contains(&"Read"));
        assert!(!names.contains(&"Task"));
        assert!(!names.contains(&"Write")); // not in use_tools
    }

    #[test]
    fn cap_git_status_passes_short_through() {
        let gs = "## main\n M a.rs\n M b.rs";
        assert_eq!(ZenEngine::cap_git_status(gs), gs);
    }

    #[test]
    fn cap_git_status_truncates_long() {
        let mut s = String::from("## main");
        for i in 0..100 {
            s.push_str(&format!("\n M file{i}.rs"));
        }
        let out = ZenEngine::cap_git_status(&s);
        assert_eq!(out.lines().count(), 31); // 30 kept + 1 summary line
        assert!(out.contains("## main")); // branch header preserved
        assert!(out.contains("71 more changed path(s) omitted"));
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
