//! IsolatedRunner — one-shot disposable agent execution.
//!
//! Port of `code-old/SenClaw/src/agent/IsolatedRunner.ts`.
//!
//! Decoupled from AgentPool / GroupBinding / ScheduleTool. Use cases:
//!   1. AgentPool::run_isolated — scheduled tasks (caller wraps ScheduleTool MCP + broadcastReply)
//!   2. `senclaw agent-task` CLI — hook scripts (reflection, summarization, analysis)
//!
//! Behavior:
//!   - skip_mcp_init = true (avoid concurrent MCP race, matches AgentPool)
//!   - skip_*_permission default true (unattended)
//!   - Collects `MessageComplete` events for agent_id == "main"
//!   - Resolves on `StateUpdate(Idle)`
//!   - Forces resolve with `timed_out: true` on timeout (no error)

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::time::timeout as tokio_timeout;

use crate::zen_core::{
    AgentMode, EngineEvent, McpServerConfig, MessageCompleteData, SessionState, StateUpdateData,
    ZenCore, ZenCoreOptions, ZenEngine, MAIN_AGENT_ID,
};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// MCP server configuration registered before session creation.
#[derive(Debug, Clone)]
pub struct McpInject {
    pub config: McpServerConfig,
    pub scope: String,
}

/// Permission skip flags. All default to `true` for unattended one-shot runs.
#[derive(Debug, Clone)]
pub struct SkipPermissions {
    pub file_edit: bool,
    pub bash_exec: bool,
    pub skill: bool,
    pub mcp_tool: bool,
}

impl Default for SkipPermissions {
    fn default() -> Self {
        Self {
            file_edit: true,
            bash_exec: true,
            skill: true,
            mcp_tool: true,
        }
    }
}

/// One-shot agent execution options.
#[derive(Debug, Clone)]
pub struct OneShotOptions {
    /// User prompt (required).
    pub prompt: String,
    /// Working directory (file I/O, Bash, etc.).
    pub working_dir: String,
    /// Agent data dir (CLAUDE.md, .sema/). Defaults to `working_dir` when `None`.
    pub agent_data_dir: Option<String>,
    /// Multi-tenant instance key. Auto-generated when `None`.
    pub instance_id: Option<String>,
    /// Tool whitelist. Empty = all tools.
    pub use_tools: Vec<String>,
    /// Extra skills directories.
    pub skills_extra_dirs: Vec<String>,
    /// System prompt override.
    pub system_prompt: Option<String>,
    /// Custom user rules appended to system prompt.
    pub custom_rules: Option<String>,
    /// Agent mode.
    pub agent_mode: AgentMode,
    /// MCP servers registered before session creation.
    pub mcp_configs: Vec<McpInject>,
    /// Timeout (defaults to 5 minutes).
    pub timeout: Option<Duration>,
    /// Permission skip flags (all default true).
    pub skip_permissions: SkipPermissions,
}

impl Default for OneShotOptions {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            working_dir: String::new(),
            agent_data_dir: None,
            instance_id: None,
            use_tools: Vec::new(),
            skills_extra_dirs: Vec::new(),
            system_prompt: None,
            custom_rules: None,
            agent_mode: AgentMode::Agent,
            mcp_configs: Vec::new(),
            timeout: None,
            skip_permissions: SkipPermissions::default(),
        }
    }
}

/// Result of one-shot execution.
#[derive(Debug, Clone)]
pub struct OneShotResult {
    /// Last `MessageComplete` content from agent_id == "main".
    pub text: String,
    /// All non-empty `MessageComplete` contents from agent_id == "main", in order.
    pub all_texts: Vec<String>,
    /// Wall-clock duration.
    pub duration: Duration,
    /// Number of `message:complete` events on the main agent.
    pub turn_count: u32,
    /// `true` if execution ended via timeout (engine forcibly aborted).
    pub timed_out: bool,
}

/// Build a unique instance id like `oneshot-{millis}-{rand}` if caller didn't supply one.
fn gen_instance_id() -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let rand: u32 = rand::random();
    format!("oneshot-{millis}-{:x}", rand & 0xFFFFF)
}

/// Run a single prompt to idle, then dispose. Mirrors TS `runOneShot`.
pub async fn run_one_shot(opts: OneShotOptions) -> Result<OneShotResult> {
    let started_at = Instant::now();
    let timeout = opts.timeout.unwrap_or(DEFAULT_TIMEOUT);
    let instance_id = opts.instance_id.unwrap_or_else(gen_instance_id);
    let agent_data_dir = opts
        .agent_data_dir
        .clone()
        .unwrap_or_else(|| opts.working_dir.clone());

    let zen_opts = ZenCoreOptions {
        instance_id: instance_id.clone(),
        agent_data_dir,
        working_dir: opts.working_dir.clone(),
        use_tools: opts.use_tools.clone(),
        skills_extra_dirs: opts.skills_extra_dirs.clone(),
        skip_file_edit_permission: opts.skip_permissions.file_edit,
        skip_bash_exec_permission: opts.skip_permissions.bash_exec,
        skip_skill_permission: opts.skip_permissions.skill,
        skip_mcp_tool_permission: opts.skip_permissions.mcp_tool,
        skip_mcp_init: true,
        system_prompt: opts.system_prompt.clone().unwrap_or_default(),
        custom_rules: opts.custom_rules.clone().unwrap_or_default(),
        agent_mode: opts.agent_mode,
        ..Default::default()
    };

    let engine = ZenEngine::new(zen_opts, None);

    for inject in &opts.mcp_configs {
        if let Err(e) = engine.add_or_update_mcp_server(&inject.config, &inject.scope) {
            tracing::warn!(
                "[IsolatedRunner:{instance_id}] add_or_update_mcp_server '{}' failed: {e}",
                inject.config.name
            );
        }
    }

    let mut rx = engine.event_bus.subscribe();
    engine.create_session(Some(&format!("session-{instance_id}")))?;
    engine.process_user_input(&opts.prompt, None)?;

    let mut all_texts: Vec<String> = Vec::new();
    let mut turn_count: u32 = 0;
    let mut timed_out = false;
    let deadline = Instant::now() + timeout;

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            engine.abort_current();
            break;
        }

        match tokio_timeout(remaining, rx.recv()).await {
            // Timed out waiting for next event
            Err(_) => {
                timed_out = true;
                engine.abort_current();
                break;
            }
            // Channel closed — treat as terminal.
            Ok(Err(_)) => break,
            Ok(Ok(event)) => match event {
                EngineEvent::MessageComplete(MessageCompleteData {
                    agent_id, content, ..
                }) if agent_id == MAIN_AGENT_ID => {
                    turn_count += 1;
                    if !content.trim().is_empty() {
                        all_texts.push(content);
                    }
                }
                EngineEvent::StateUpdate(StateUpdateData { state })
                    if state == SessionState::Idle =>
                {
                    break;
                }
                EngineEvent::SessionError(err) => {
                    tracing::warn!(
                        "[IsolatedRunner:{instance_id}] session error: {} ({})",
                        err.error.message,
                        err.error.code
                    );
                    // Don't break — wait for Idle to flush remaining state.
                }
                _ => {}
            },
        }
    }

    // Fire-and-forget dispose (engine handles SessionEnd hooks internally).
    {
        let engine: Arc<ZenEngine> = engine;
        tokio::task::spawn_blocking(move || engine.dispose());
    }

    // Prefer the engine's last-visible-text helper if available; otherwise
    // fall back to the last collected MessageComplete.
    let text = all_texts.last().cloned().unwrap_or_default();

    Ok(OneShotResult {
        text,
        all_texts,
        duration: started_at.elapsed(),
        turn_count,
        timed_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_skip_permissions_all_true() {
        let s = SkipPermissions::default();
        assert!(s.file_edit && s.bash_exec && s.skill && s.mcp_tool);
    }

    #[test]
    fn gen_instance_id_unique() {
        let a = gen_instance_id();
        let b = gen_instance_id();
        assert_ne!(a, b);
        assert!(a.starts_with("oneshot-"));
    }

    #[test]
    fn one_shot_options_default_has_5min_timeout_fallback() {
        let opts = OneShotOptions::default();
        let t = opts.timeout.unwrap_or(DEFAULT_TIMEOUT);
        assert_eq!(t, Duration::from_secs(300));
    }
}
