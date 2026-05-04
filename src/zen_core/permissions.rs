//! Permission manager — gates tool execution behind user approval.
//!
//! Four tool categories (mirrors TS `PermissionManager.ts`):
//! 1. **File edit** tools (Write, Edit, NotebookEdit)
//! 2. **Bash** tool — safe-command whitelist + prefix allowlisting
//! 3. **Skill** tool — per-skill allowlisting
//! 4. **MCP** tools — per-tool allowlisting
//!
//! Non-readonly tools that don't match any skip rule or allowlist emit
//! `tool:permission:request` and suspend until a response arrives via the
//! [`ResponseRegistry`].

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use super::events::ResponseRegistry;
use super::run_tools::PermissionChecker;
use super::*;

// ============================================================================
// Constants (mirrors TS)
// ============================================================================

/// Commands that are always allowed without prompting.
const SAFE_COMMANDS: &[&str] = &[
    "git status",
    "git diff",
    "git log",
    "git branch",
    "pwd",
    "tree",
    "date",
    "which",
    "ls",
    "find",
    "grep",
    "head",
    "tail",
    "cat",
    "du",
    "wc",
    "echo",
    "env",
    "printenv",
];

/// Tool names that are treated as file-editing tools.
const FILE_EDIT_TOOLS: &[&str] = &["Edit", "Write", "NotebookEdit"];

/// Skill tool name.
const SKILL_TOOL_NAME: &str = "Skill";

/// MCP tool prefix.
const MCP_TOOL_PREFIX: &str = "mcp__";

// ============================================================================
// Permission manager
// ============================================================================

pub struct PermissionManager {
    /// Skip flags from engine options.
    skip_file_edit: bool,
    skip_bash: bool,
    skip_skill: bool,
    skip_mcp: bool,
    /// Per-project allowed tools list (tool_name or "Bash(cmd)" keys).
    allowed_tools: Mutex<HashSet<String>>,
    /// Whether global edit permission has been granted this session.
    global_edit_granted: Mutex<bool>,
    /// Check whether the working dir contains a file path.
    /// (Simplified: always returns true for now; full impl checks file paths.)
    pub(crate) is_in_working_dir: Box<dyn Fn(&str) -> bool + Send + Sync>,
    /// One-shot response channels (shared with engine).
    response_registry: Arc<ResponseRegistry>,
    /// Event emitter.
    event_bus: EventBus,
}

impl PermissionManager {
    pub(crate) fn new(event_bus: EventBus, response_registry: Arc<ResponseRegistry>) -> Self {
        Self {
            skip_file_edit: false,
            skip_bash: false,
            skip_skill: false,
            skip_mcp: false,
            allowed_tools: Mutex::new(HashSet::new()),
            global_edit_granted: Mutex::new(false),
            is_in_working_dir: Box::new(|_| true),
            response_registry,
            event_bus,
        }
    }

    pub fn update_skip_flags(&mut self, file_edit: bool, bash: bool, skill: bool, mcp: bool) {
        self.skip_file_edit = file_edit;
        self.skip_bash = bash;
        self.skip_skill = skill;
        self.skip_mcp = mcp;
    }

    pub fn grant_global_edit(&self) {
        *self.global_edit_granted.lock().unwrap() = true;
        info!("Global edit permission granted for session");
    }

    pub fn add_allowed_tool(&self, key: &str) {
        self.allowed_tools.lock().unwrap().insert(key.to_owned());
    }

    // ============================================================
    // Internal helpers
    // ============================================================

    fn is_file_edit_tool(name: &str) -> bool {
        FILE_EDIT_TOOLS.contains(&name)
    }

    fn is_skill_tool(name: &str) -> bool {
        name == SKILL_TOOL_NAME
    }

    fn is_mcp_tool(name: &str) -> bool {
        name.starts_with(MCP_TOOL_PREFIX)
    }

    fn is_allowed(&self, key: &str) -> bool {
        self.allowed_tools.lock().unwrap().contains(key)
    }

    fn get_permission_key(
        tool: &dyn Tool,
        input: &serde_json::Value,
        prefix: Option<&str>,
    ) -> String {
        let name = tool.name();
        if name == "Bash" {
            if let Some(p) = prefix {
                return format!("Bash({p}:*)");
            }
            let cmd = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            return format!("Bash({cmd})");
        }
        if Self::is_skill_tool(name) {
            let skill_name = input.get("skill").and_then(|v| v.as_str()).unwrap_or("");
            return if skill_name.is_empty() {
                name.to_string()
            } else {
                format!("{name}({skill_name})")
            };
        }
        name.to_string()
    }

    fn is_safe_command(command: &str) -> bool {
        let cmd = command.trim();
        // Exact match
        if SAFE_COMMANDS.contains(&cmd) {
            return true;
        }
        // Prefix match (e.g. "ls -la" matches "ls")
        let main = cmd.split(' ').next().unwrap_or("");
        SAFE_COMMANDS.contains(&main)
    }

    fn build_options(
        tool: &dyn Tool,
        input: &serde_json::Value,
        prefix: Option<&str>,
    ) -> HashMap<String, String> {
        let name = tool.name();
        if name == "Bash" {
            let command = input
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if let Some(p) = prefix {
                let mut opts = HashMap::new();
                opts.insert("agree".into(), "Confirm".into());
                opts.insert(
                    "allow".into(),
                    format!("Confirm, never ask for `{p}` commands in this project"),
                );
                opts.insert("refuse".into(), "Reject".into());
                return opts;
            }
            let allow_text = if command.is_empty() {
                "Confirm, never ask for this command in this project".into()
            } else {
                format!("Confirm, never ask for `{command}` in this project")
            };
            let mut opts = HashMap::new();
            opts.insert("agree".into(), "Confirm".into());
            opts.insert("allow".into(), allow_text);
            opts.insert("refuse".into(), "Reject".into());
            return opts;
        }

        if Self::is_file_edit_tool(name) {
            let mut opts = HashMap::new();
            opts.insert("agree".into(), "Confirm".into());
            opts.insert(
                "allow".into(),
                "Confirm, never ask for file editing in this project".into(),
            );
            opts.insert("refuse".into(), "Reject".into());
            return opts;
        }

        if Self::is_skill_tool(name) {
            let skill_name = input.get("skill").and_then(|v| v.as_str()).unwrap_or("");
            let mut opts = HashMap::new();
            opts.insert("agree".into(), "Confirm".into());
            opts.insert(
                "allow".into(),
                format!("Confirm, never ask for {skill_name} Skill in this project"),
            );
            opts.insert("refuse".into(), "Reject".into());
            return opts;
        }

        if Self::is_mcp_tool(name) {
            let mut opts = HashMap::new();
            opts.insert("agree".into(), "Confirm".into());
            opts.insert(
                "allow".into(),
                format!("Confirm, never ask for {name} in this project"),
            );
            opts.insert("refuse".into(), "Reject".into());
            return opts;
        }

        let mut opts = HashMap::new();
        opts.insert("agree".into(), "Allow".into());
        opts.insert(
            "allow".into(),
            format!("Allow, never ask for {name} in this project"),
        );
        opts.insert("refuse".into(), "Reject".into());
        opts
    }

    /// Request permission via event and wait for response.
    async fn request_permission(
        &self,
        tool: &dyn Tool,
        input: &serde_json::Value,
        prefix: Option<&str>,
        cancel: &CancellationToken,
        agent_id: &str,
    ) -> Result<bool> {
        let name = tool.name().to_string();
        let permission_info = tool.gen_tool_permission(input);
        let options = Self::build_options(tool, input, prefix);

        let request = ToolPermissionRequestData {
            agent_id: agent_id.to_string(),
            tool_name: name.clone(),
            title: permission_info
                .as_ref()
                .map_or(name.clone(), |p| p.title.clone()),
            content: permission_info.map_or(serde_json::Value::Null, |p| p.content),
            options,
        };

        // Emit to event bus (for UI)
        self.event_bus
            .emit(EngineEvent::ToolPermissionRequest(request));

        // Register response waiter
        let mut rx = self.response_registry.register_tool_permission(&name);

        // Wait for response or cancellation
        tokio::select! {
            _ = cancel.cancelled() => {
                Ok(false)
            }
            result = &mut rx => {
                match result {
                    Ok(response) => {
                        match response.selected.as_str() {
                            "agree" => Ok(true),
                            "allow" => {
                                let key = Self::get_permission_key(tool, input, prefix.as_deref());
                                self.add_allowed_tool(&key);
                                if Self::is_file_edit_tool(tool.name()) {
                                    self.grant_global_edit();
                                }
                                Ok(true)
                            }
                            "refuse" => Ok(false),
                            _ => {
                                // Custom feedback — allow but with message
                                Ok(false)
                            }
                        }
                    }
                    Err(_) => {
                        // Sender dropped (engine disposed)
                        Ok(false)
                    }
                }
            }
        }
    }
}

// ============================================================================
// PermissionChecker trait impl
// ============================================================================

#[async_trait]
impl PermissionChecker for PermissionManager {
    async fn check(
        &self,
        tool: &dyn Tool,
        input: &serde_json::Value,
        cancel: &CancellationToken,
        agent_id: &str,
    ) -> Result<bool> {
        let name = tool.name();

        // 1. File edit tools
        if Self::is_file_edit_tool(name) {
            if self.skip_file_edit {
                debug!("[{name}] skip file edit permission");
                return Ok(true);
            }
            if *self.global_edit_granted.lock().unwrap() {
                debug!("[{name}] global edit permission active");
                return Ok(true);
            }
            return self
                .request_permission(tool, input, None, cancel, agent_id)
                .await;
        }

        // 2. Bash tool
        if name == "Bash" {
            if self.skip_bash {
                return Ok(true);
            }
            let command = input.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let key = Self::get_permission_key(tool, input, None);
            if Self::is_safe_command(command) || self.is_allowed(&key) {
                return Ok(true);
            }
            return self
                .request_permission(tool, input, None, cancel, agent_id)
                .await;
        }

        // 3. Skill tool
        if Self::is_skill_tool(name) {
            if self.skip_skill {
                return Ok(true);
            }
            let key = Self::get_permission_key(tool, input, None);
            if self.is_allowed(&key) {
                return Ok(true);
            }
            return self
                .request_permission(tool, input, None, cancel, agent_id)
                .await;
        }

        // 4. MCP tools
        if Self::is_mcp_tool(name) {
            if self.skip_mcp {
                return Ok(true);
            }
            if self.is_allowed(name) {
                return Ok(true);
            }
            return self
                .request_permission(tool, input, None, cancel, agent_id)
                .await;
        }

        // Other non-readonly tools — default allow
        debug!("[{name}] non-standard tool, default allow");
        Ok(true)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBashTool;
    #[async_trait::async_trait]
    impl Tool for TestBashTool {
        fn name(&self) -> &str {
            "Bash"
        }
        fn description(&self) -> &str {
            "Execute bash"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}, "required": ["command"]})
        }
        fn is_read_only(&self) -> bool {
            false
        }
        async fn call(
            &self,
            _input: serde_json::Value,
            _ctx: &ToolContext<'_>,
        ) -> Result<Vec<ToolOutput>> {
            Ok(vec![])
        }
        fn gen_tool_result_message(
            &self,
            _data: &serde_json::Value,
            _input: &serde_json::Value,
        ) -> ToolResultMessage {
            ToolResultMessage {
                title: "Bash".into(),
                summary: "".into(),
                content: serde_json::json!({}),
            }
        }
        fn get_display_title(&self, _input: &serde_json::Value) -> String {
            "Bash".into()
        }
    }

    struct TestEditTool;
    #[async_trait::async_trait]
    impl Tool for TestEditTool {
        fn name(&self) -> &str {
            "Edit"
        }
        fn description(&self) -> &str {
            "Edit file"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn is_read_only(&self) -> bool {
            false
        }
        async fn call(
            &self,
            _input: serde_json::Value,
            _ctx: &ToolContext<'_>,
        ) -> Result<Vec<ToolOutput>> {
            Ok(vec![])
        }
        fn gen_tool_result_message(
            &self,
            _data: &serde_json::Value,
            _input: &serde_json::Value,
        ) -> ToolResultMessage {
            ToolResultMessage {
                title: "Edit".into(),
                summary: "".into(),
                content: serde_json::json!({}),
            }
        }
        fn get_display_title(&self, _input: &serde_json::Value) -> String {
            "Edit".into()
        }
    }

    #[test]
    fn safe_command_detection() {
        assert!(PermissionManager::is_safe_command("ls"));
        assert!(PermissionManager::is_safe_command("ls -la"));
        assert!(PermissionManager::is_safe_command("git status"));
        assert!(!PermissionManager::is_safe_command("rm -rf /"));
        assert!(!PermissionManager::is_safe_command("curl evil.com"));
    }

    #[test]
    fn file_edit_tool_detection() {
        assert!(PermissionManager::is_file_edit_tool("Edit"));
        assert!(PermissionManager::is_file_edit_tool("Write"));
        assert!(PermissionManager::is_file_edit_tool("NotebookEdit"));
        assert!(!PermissionManager::is_file_edit_tool("Read"));
    }

    #[test]
    fn permission_key_for_bash() {
        let tool = TestBashTool;
        let key = PermissionManager::get_permission_key(
            &tool,
            &serde_json::json!({"command": "npm test"}),
            None,
        );
        assert_eq!(key, "Bash(npm test)");

        let key_with_prefix = PermissionManager::get_permission_key(
            &tool,
            &serde_json::json!({"command": "npm test"}),
            Some("npm"),
        );
        assert_eq!(key_with_prefix, "Bash(npm:*)");
    }

    #[tokio::test]
    async fn skip_bash_bypasses_permission() {
        let bus = EventBus::new();
        let reg = Arc::new(ResponseRegistry::new());
        let mut pm = PermissionManager::new(bus, reg);
        pm.update_skip_flags(false, true, false, false);

        let tool = TestBashTool;
        let cancel = CancellationToken::new();
        assert!(pm
            .check(
                &tool,
                &serde_json::json!({"command": "rm -rf /"}),
                &cancel,
                "main"
            )
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn safe_command_bypasses_permission() {
        let bus = EventBus::new();
        let reg = Arc::new(ResponseRegistry::new());
        let pm = PermissionManager::new(bus, reg);

        let tool = TestBashTool;
        let cancel = CancellationToken::new();
        assert!(pm
            .check(
                &tool,
                &serde_json::json!({"command": "ls"}),
                &cancel,
                "main"
            )
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn allowed_tool_bypasses_permission() {
        let bus = EventBus::new();
        let reg = Arc::new(ResponseRegistry::new());
        let pm = PermissionManager::new(bus, reg);
        pm.add_allowed_tool("Bash(npm test)");

        let tool = TestBashTool;
        let cancel = CancellationToken::new();
        assert!(pm
            .check(
                &tool,
                &serde_json::json!({"command": "npm test"}),
                &cancel,
                "main"
            )
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn edit_tool_with_skip_flag_bypasses() {
        let bus = EventBus::new();
        let reg = Arc::new(ResponseRegistry::new());
        let mut pm = PermissionManager::new(bus, reg);
        pm.update_skip_flags(true, false, false, false);

        let tool = TestEditTool;
        let cancel = CancellationToken::new();
        assert!(pm
            .check(&tool, &serde_json::json!({}), &cancel, "main")
            .await
            .unwrap());
    }
}
