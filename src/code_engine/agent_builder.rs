//! Code-session agent wiring.
//!
//! Builds the [`GroupBinding`] + per-session `system_prompt` + tool whitelist
//! that ZenEngine uses when an agent is spawned for a code chat. Centralizing
//! this here means [`crate::gateway::ui_server::code`] no longer has to know
//! about ZenEngine internals.
//!
//! Flow (per turn):
//!   1. Caller has a [`CodeSession`] + parsed user prompt.
//!   2. Caller asks `CodeAgentSpec::for_session(...)` for a spec.
//!   3. Spec exposes:
//!      - `jid()`   — synthetic JID `code-chat:{group_id}`
//!      - `binding()` — `GroupBinding` with code-specific tool whitelist
//!      - `system_prompt()` — base + code section + session metadata
//!      - `user_prompt(parsed, refs)` — wrapped with `<context>` block
//!   4. Caller calls `AgentPool::process_and_wait(jid, binding, user_prompt)`.
//!
//! The engine's normal filter funnel (Layer 1 use_tools, Layer 5 defer) then
//! applies: code MCP tools auto-load (see [`always_loaded_code_mcp_tools`]),
//! browser/space/etc stay deferred unless the user references them.

use chrono::Utc;

use crate::code_engine::prompt::PromptParseResult;
use crate::code_engine::session::CodeSession;
use crate::code_engine::system_prompt::{build_code_system_prompt, build_user_prompt};
use crate::types::GroupBinding;

/// Core tools always available in code sessions. Subset of the global builtin
/// pool, tuned for typical coding tasks.
///
/// Maps to claude-code's `getAllBaseTools()` "simple mode" + project-mode pack:
/// it keeps file ops, search, shell, agents, todos, plan, and discovery.
pub const CODE_SESSION_TOOLS: &[&str] = &[
    // File ops (sandboxed by working_dir)
    "Read",
    "Write",
    "Edit",
    "NotebookEdit",
    // Discovery
    "Glob",
    "Grep",
    "Bash",
    // Agent orchestration
    "Task",
    "TodoWrite",
    "AskUserQuestion",
    "ExitPlanMode",
    // Tool discovery for the long tail
    "ToolSearch",
    // Skill invocation (slash commands)
    "Skill",
    // External fetch (often needed for docs)
    "WebFetch",
    // Time (cheap to include)
    "Time",
];

/// MCP tools always loaded for code sessions (not deferred). Layered on top
/// of [`crate::mcp::bridge::ALWAYS_LOADED_MCP_TOOLS`] for code-specific JIDs.
///
/// We pin code-graph and code-editing MCPs so the agent can reach for them
/// without round-tripping through `ToolSearch`.
pub fn always_loaded_code_mcp_tools() -> &'static [&'static str] {
    &[
        // Code-graph (AST + symbol search) — frequent for navigation
        "mcp__senclaw-code-graph__symbol_lookup",
        "mcp__senclaw-code-graph__file_skeleton",
        "mcp__senclaw-code-graph__find_references",
        // Code-server (project-aware operations) — frequent for refactor
        "mcp__senclaw-code__read_file",
        "mcp__senclaw-code__write_file",
        "mcp__senclaw-code__edit_file",
        "mcp__senclaw-code__search_code",
    ]
}

/// JID convention used for code-chat agents. One JID per chat group so engine
/// state, history, and prompt cache all flow per group.
pub fn code_session_jid(group_id: &str) -> String {
    format!("code-chat:{group_id}")
}

/// Builder produced once per turn. Owns the resolved strings so the caller
/// can clone them into the engine call.
pub struct CodeAgentSpec {
    jid: String,
    binding: GroupBinding,
    system_prompt: String,
}

impl CodeAgentSpec {
    pub fn for_session(
        session: &CodeSession,
        session_name: &str,
        group_id: &str,
    ) -> Self {
        let jid = code_session_jid(group_id);
        let binding = build_code_group_binding(&jid, session, session_name);
        let system_prompt = build_code_system_prompt(session, session_name);
        Self {
            jid,
            binding,
            system_prompt,
        }
    }

    pub fn jid(&self) -> &str {
        &self.jid
    }

    pub fn binding(&self) -> &GroupBinding {
        &self.binding
    }

    /// System prompt for `ZenCoreOptions::system_prompt`. Note: zen_core
    /// engine will prepend its own `SYSTEM_PROMPT` (Safety + Conduct +
    /// Tools) and append the system context block, so this is the
    /// **middle** segment — code-specific guidance + session metadata.
    pub fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    /// Wrap the user's plain prompt with the `<context>` block.
    pub fn user_prompt(
        &self,
        parsed: &PromptParseResult,
        resolved_refs: &[String],
    ) -> String {
        build_user_prompt(parsed, resolved_refs)
    }
}

/// Build a `GroupBinding` configured for code sessions:
/// - `folder = code/{session_id}` so engine namespace doesn't collide with
///   regular chat agents.
/// - `is_admin = true` so dispatch tools work (admins can spawn subagents).
/// - `allowed_tools` = [`CODE_SESSION_TOOLS`] — engine Layer 2 filter trims
///   anything outside this whitelist (token saver).
/// - `allowed_work_dirs` = `[workspace]` so any workspace switching stays
///   inside the sandbox.
pub fn build_code_group_binding(
    jid: &str,
    session: &CodeSession,
    session_name: &str,
) -> GroupBinding {
    let workspace_str = session.workspace.to_string_lossy().into_owned();

    // Build allowed_tools = builtins + always-loaded code MCP tools.
    // The engine will further apply Layer 5 (defer filter); MCP tools not in
    // this list stay deferred and accessible only via `ToolSearch`.
    let mut allowed: Vec<String> = CODE_SESSION_TOOLS
        .iter()
        .map(|s| s.to_string())
        .collect();
    allowed.extend(
        always_loaded_code_mcp_tools()
            .iter()
            .map(|s| s.to_string()),
    );

    GroupBinding {
        jid: jid.to_string(),
        folder: format!("code/{}", session.session_id),
        name: format!("Code::{session_name}"),
        channel: "web".to_string(),
        group_type: "code".to_string(),
        is_admin: true,
        requires_trigger: false,
        allowed_tools: Some(allowed),
        allowed_paths: Some(vec![workspace_str.clone()]),
        allowed_work_dirs: Some(vec![workspace_str]),
        bot_token: None,
        max_messages: None,
        last_active: None,
        added_at: Utc::now().to_rfc3339(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mk(path: &str) -> CodeSession {
        CodeSession {
            session_id: "sess-1".into(),
            workspace: PathBuf::from(path),
            git_enabled: true,
            tracker: Default::default(),
        }
    }

    #[test]
    fn jid_namespace_per_group() {
        assert_eq!(code_session_jid("grp-abc"), "code-chat:grp-abc");
    }

    #[test]
    fn binding_sets_folder_and_is_admin() {
        let s = mk("/tmp/x");
        let b = build_code_group_binding("code-chat:g1", &s, "My Proj");
        assert_eq!(b.folder, "code/sess-1");
        assert!(b.is_admin);
        assert_eq!(b.group_type, "code");
        assert!(!b.requires_trigger);
        assert_eq!(b.channel, "web");
    }

    #[test]
    fn binding_restricts_to_workspace() {
        let s = mk("/tmp/proj-root");
        let b = build_code_group_binding("code-chat:g2", &s, "P");
        assert_eq!(b.allowed_paths, Some(vec!["/tmp/proj-root".into()]));
        assert_eq!(b.allowed_work_dirs, Some(vec!["/tmp/proj-root".into()]));
    }

    #[test]
    fn binding_allowed_tools_contains_core_and_mcp() {
        let s = mk("/tmp/x");
        let b = build_code_group_binding("code-chat:g3", &s, "P");
        let allowed = b.allowed_tools.unwrap();
        for must in ["Read", "Write", "Edit", "Bash", "Glob", "Grep", "ToolSearch", "Task"] {
            assert!(allowed.iter().any(|t| t == must), "missing {must}");
        }
        assert!(allowed
            .iter()
            .any(|t| t == "mcp__senclaw-code-graph__symbol_lookup"));
        assert!(allowed
            .iter()
            .any(|t| t == "mcp__senclaw-code__read_file"));
    }

    #[test]
    fn spec_exposes_all_components() {
        let s = mk("/tmp/spec-test");
        let spec = CodeAgentSpec::for_session(&s, "Spec Proj", "grp-x");
        assert_eq!(spec.jid(), "code-chat:grp-x");
        assert_eq!(spec.binding().folder, "code/sess-1");
        assert!(spec.system_prompt().contains("/tmp/spec-test"));
        assert!(spec.system_prompt().contains("Spec Proj"));
    }
}
