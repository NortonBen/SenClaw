//! Top-level MCP manager — per-engine singleton.
//!
//! Provides CRUD, connection management, and tool dispatch for both
//! built-in and external MCP servers.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::Result;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::mcp::client::{McpToolInfo, SharedMcpRegistry};
use crate::mcp::config::{
    ExternalMcpServerConfig, McpConfigManager, McpScopeType, McpServerInfo, McpServerStatus,
    McpToolDef, McpTransportType,
};
use crate::mcp::external_client::ExternalMcpClient;

use super::types::{BuiltInServerInfo, ExternalServerState};
use super::utils::parse_mcp_tool_name;

/// Top-level MCP manager — per-engine singleton.
pub struct McpManager {
    /// Configuration persistence (user + project scopes).
    config_mgr: McpConfigManager,

    /// Built-in MCP servers (senclaw subprocesses).
    builtin_registry: SharedMcpRegistry,

    /// External server runtime state, keyed by name.
    external: RwLock<HashMap<String, ExternalServerState>>,

    /// Working directory for project-scope config.
    working_dir: PathBuf,
}

impl McpManager {
    /// Create a new manager for the given working directory.
    /// `user_config_dir` is typically `~/.senclaw/`.
    pub fn new(working_dir: PathBuf, user_config_dir: PathBuf) -> Self {
        let config_mgr = McpConfigManager::new(user_config_dir.join("mcp.json"));
        Self {
            config_mgr,
            builtin_registry: SharedMcpRegistry::new(),
            external: RwLock::new(HashMap::new()),
            working_dir,
        }
    }

    /// Access the built-in registry for senclaw subprocess servers.
    pub fn builtin_registry(&self) -> &SharedMcpRegistry {
        &self.builtin_registry
    }

    // ---- init ----

    /// Initialise: load external server configs from disk and auto-connect
    /// enabled servers.
    pub async fn init(&self) -> Result<()> {
        let merged = self.config_mgr.load_merged(&self.working_dir);

        let mut external = self.external.write().await;

        for (name, cfg) in &merged.mcp_servers {
            if !cfg.enabled {
                info!("MCP manager: skipping disabled external server {name}");
                continue;
            }
            // Determine scope: check if this name exists in project config
            let project_cfg = self.config_mgr.load_project_config(&self.working_dir);
            let scope = if project_cfg.mcp_servers.contains_key(name) {
                McpScopeType::Project
            } else {
                McpScopeType::User
            };

            let state = ExternalServerState::new(cfg.clone(), scope);
            external.insert(name.clone(), state);
        }
        drop(external);

        // Connect enabled servers in the background — failures are non-fatal.
        {
            let external = self.external.read().await;
            let names: Vec<String> = external.keys().cloned().collect();
            drop(external);

            for name in &names {
                if let Err(e) = self.connect_server(name).await {
                    warn!("MCP manager: auto-connect {name} failed: {e}");
                }
            }
        }

        info!(
            "MCP manager: {} external server(s) configured",
            self.external.read().await.len()
        );
        Ok(())
    }

    // ---- CRUD ----

    /// Add or update an external MCP server (persists config + connects).
    pub async fn add_or_update(
        &self,
        config: ExternalMcpServerConfig,
        scope: McpScopeType,
    ) -> Result<McpServerInfo> {
        // Validate
        config.validate().map_err(|e| anyhow::anyhow!(e))?;

        // Persist
        self.config_mgr
            .add_or_update_server(&self.working_dir, scope, &config)?;

        // Disconnect existing if present
        self.disconnect_server(&config.name).await.ok();

        // Insert new state
        let mut external = self.external.write().await;
        let state = ExternalServerState::new(config.clone(), scope);
        external.insert(config.name.clone(), state);
        drop(external);

        // Connect if enabled
        if config.enabled {
            if let Err(e) = self.connect_server(&config.name).await {
                warn!(
                    "MCP manager: connect after add/update {} failed: {e}",
                    config.name
                );
            }
        }

        Ok(self.get_server_info(&config.name).await)
    }

    /// Remove an external MCP server (disconnects + removes config).
    pub async fn remove(&self, name: &str, scope: McpScopeType) -> Result<bool> {
        // Disconnect first
        self.disconnect_server(name).await.ok();

        // Remove from runtime
        let existed = self.external.write().await.remove(name).is_some();

        // Remove from config file
        let persisted = self
            .config_mgr
            .remove_server(&self.working_dir, scope, name)?;

        Ok(existed || persisted)
    }

    /// Update the `use_tools` allowlist for an external server.
    pub async fn update_use_tools(
        &self,
        name: &str,
        scope: McpScopeType,
        tool_names: Option<Vec<String>>,
    ) -> Result<bool> {
        let ok = self
            .config_mgr
            .update_use_tools(&self.working_dir, scope, name, &tool_names)?;
        if ok {
            // Update runtime state
            let mut external = self.external.write().await;
            if let Some(state) = external.get_mut(name) {
                state.config.use_tools = tool_names;
            }
        }
        Ok(ok)
    }

    /// Update the `enabled` flag for an external server.
    pub async fn update_enabled(
        &self,
        name: &str,
        scope: McpScopeType,
        enabled: bool,
    ) -> Result<bool> {
        let ok = self
            .config_mgr
            .update_enabled(&self.working_dir, scope, name, enabled)?;
        if ok {
            if enabled {
                let _ = self.connect_server(name).await;
            } else {
                let _ = self.disconnect_server(name).await;
            }
            let mut external = self.external.write().await;
            if let Some(state) = external.get_mut(name) {
                state.config.enabled = enabled;
            }
        }
        Ok(ok)
    }

    /// Get info for a single external server.
    pub async fn get_server_info(&self, name: &str) -> McpServerInfo {
        let external = self.external.read().await;
        match external.get(name) {
            Some(state) => state.to_info(),
            None => {
                // Create a placeholder — might be from config not yet loaded.
                // Check both config files.
                let merged = self.config_mgr.load_merged(&self.working_dir);
                match merged.mcp_servers.get(name) {
                    Some(cfg) => {
                        let project = self.config_mgr.load_project_config(&self.working_dir);
                        let scope = if project.mcp_servers.contains_key(name) {
                            McpScopeType::Project
                        } else {
                            McpScopeType::User
                        };
                        McpServerInfo {
                            config: cfg.clone(),
                            scope,
                            status: McpServerStatus::Disconnected,
                            tools: None,
                            error: None,
                        }
                    }
                    None => McpServerInfo {
                        config: ExternalMcpServerConfig {
                            name: name.to_string(),
                            transport: McpTransportType::Stdio,
                            description: None,
                            enabled: false,
                            use_tools: None,
                            command: None,
                            args: vec![],
                            env: HashMap::new(),
                            url: None,
                            headers: HashMap::new(),
                        },
                        scope: McpScopeType::User,
                        status: McpServerStatus::Error,
                        tools: None,
                        error: Some("server not found".to_string()),
                    },
                }
            }
        }
    }

    /// Get all external server info summaries.
    pub async fn get_all_servers(&self) -> Vec<McpServerInfo> {
        let external = self.external.read().await;
        external.values().map(|s| s.to_info()).collect()
    }

    /// List built-in (senclaw-internal) MCP servers.
    pub fn get_builtin_servers(&self) -> Vec<BuiltInServerInfo> {
        // These are the servers created by helper.rs builders.
        // We return static descriptions since built-in servers are known.
        let t = |name: &str, description: &str| McpToolDef {
            name: name.to_string(),
            description: Some(description.to_string()),
            input_schema: None,
        };
        vec![
            BuiltInServerInfo {
                name: "senclaw-schedule".into(),
                transport: "stdio".into(),
                description: Some(
                    "Built-in scheduling service for recurring and one-off tasks.".into(),
                ),
                tools: vec![
                    t("add_schedule", "Schedule a new recurring or one-off task"),
                    t("list_schedules", "List all scheduled tasks for a group"),
                    t("pause_schedule", "Pause a scheduled task"),
                    t("cancel_schedule", "Cancel a scheduled task"),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-workspace".into(),
                transport: "stdio".into(),
                description: Some(
                    "Built-in workspace management for agent working directories.".into(),
                ),
                tools: vec![
                    t("workspace_switch", "Switch the agent workspace directory"),
                    t("workspace_reset", "Reset workspace to default directory"),
                    t("workspace_info", "Show current workspace info"),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-send".into(),
                transport: "stdio".into(),
                description: Some(
                    "Built-in messaging service for sending text and files to chats.".into(),
                ),
                tools: vec![
                    t("send_text", "Send a text message to a chat"),
                    t("send_file", "Send a file to a chat via HTTP bridge"),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-memory".into(),
                transport: "stdio".into(),
                description: Some(
                    "Built-in memory service using hybrid FTS5 + vector search.".into(),
                ),
                tools: vec![
                    t(
                        "memory_search",
                        "Search memories using hybrid FTS5 + vector search",
                    ),
                    t(
                        "memory_get",
                        "Retrieve a specific memory file by path and line range",
                    ),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-cognitive".into(),
                transport: "stdio".into(),
                description: Some(
                    "Cognitive memory — knowledge graph with Hebbian dynamics + spreading activation.".into(),
                ),
                tools: vec![
                    t("cog_add", "Ingest text as a chunk node (no LLM extraction)"),
                    t("cog_cognify", "Full pipeline: chunk → LLM triplet extraction → graph upsert"),
                    t("cog_search", "Search cognitive memory (modes: chunks | triplet | graph | spreading)"),
                    t("cog_recall", "Recall memories via spreading activation with Hebbian write-back"),
                    t("cog_forget", "Delete a node and its edges from cognitive memory"),
                    t("cog_memory_stats", "Return counts of nodes/edges in cognitive memory"),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-dispatch".into(),
                transport: "stdio".into(),
                description: Some(
                    "Built-in task dispatching and agent coordination service.".into(),
                ),
                tools: vec![
                    t("list_personas", "List all registered agents and personas"),
                    t(
                        "create_parent",
                        "Create a parent dispatch with multiple tasks",
                    ),
                    t(
                        "create_parent_and_run",
                        "Create parent and wait for all tasks; returns combined results",
                    ),
                    t(
                        "dispatch_task",
                        "Dispatch a task within a parent and wait for its result",
                    ),
                    t(
                        "dispatch_all_tasks",
                        "Run all tasks under a parent in dependency order; combined results",
                    ),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-virtual".into(),
                transport: "stdio".into(),
                description: Some("Built-in virtual persona execution service.".into()),
                tools: vec![
                    t("list_virtual_personas", "List available virtual personas"),
                    t("run_virtual_persona", "Run a virtual persona with a prompt"),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-wiki".into(),
                transport: "stdio".into(),
                description: Some("Built-in local git wiki (`~/.senclaw/wiki` by default).".into()),
                tools: vec![
                    t("wiki_status", "Show wiki root path and summary statistics"),
                    t("wiki_tree", "List the wiki directory tree"),
                    t("wiki_read", "Read a markdown page by relative path"),
                    t("wiki_write", "Create or update a markdown page"),
                    t(
                        "wiki_search",
                        "Search wiki pages by title, filename, or tags",
                    ),
                    t(
                        "wiki_stats",
                        "Detailed stats (categories, tags, recent files)",
                    ),
                    t("wiki_mkdir", "Create a subdirectory under the wiki"),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-space".into(),
                transport: "stdio".into(),
                description: Some(
                    "Built-in personal productivity service — notes, calendar, email, and recurring schedules.".into(),
                ),
                tools: vec![
                    t("space_note_create", "Create a new note with title, body, and tags"),
                    t("space_note_update", "Update title, body, or tags of an existing note"),
                    t("space_note_search", "Full-text search across all notes"),
                    t("space_note_list", "List notes filtered by folder or tag"),
                    t("space_note_delete", "Delete a note by ID"),
                    t("space_current_time", "Get current local system time with pre-computed ranges for today/week/month"),
                    t("space_event_create", "Create a calendar event with optional reminder"),
                    t("space_event_list", "List calendar events in a time range"),
                    t("space_event_update", "Update any field of an existing calendar event (title, time, location, reminder…)"),
                    t("space_event_search", "Search events by keyword and/or natural-language date (today, tomorrow, YYYY-MM-DD)"),
                    t("space_event_delete", "Delete a calendar event"),
                    t("space_set_reminder", "Set or update reminder minutes for an event"),
                    t("space_today_summary", "Get today's events and recent notes as a brief"),
                    t("space_email_inbox", "List recent emails from the inbox"),
                    t("space_email_read", "Read the full body of an email message"),
                    t("space_email_compose", "Send an email (draft and confirm with user first)"),
                    t("space_email_search", "Search emails by keyword"),
                    t("space_email_summary", "Summarize an email message"),
                    t("space_schedule_activity", "Create a recurring agent activity with a cron expression"),
                    t("space_list_schedules", "List recurring schedules for a group"),
                    t("space_sync_google_calendar", "Sync events from Google Calendar"),
                    t("space_sync_apple_calendar", "Sync events from Apple Calendar via CalDAV"),
                    t("space_sync_apple_notes", "Sync notes from Apple Notes via iCloud"),
                    t("space_sync_gmail", "Sync emails from Gmail"),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-code-graph".into(),
                transport: "stdio".into(),
                description: Some(
                    "Built-in code knowledge graph — callers, callees, impact, search, skeleton."
                        .into(),
                ),
                tools: vec![
                    t(
                        "graph_reindex",
                        "Index codebase into the knowledge graph (incremental by default)",
                    ),
                    t("graph_find_callers", "Find all symbols that call the given symbol"),
                    t("graph_find_callees", "Find symbols called by the given symbol"),
                    t(
                        "graph_impact",
                        "Blast radius: symbols/files affected by changing a symbol",
                    ),
                    t(
                        "graph_symbol_context",
                        "Full context: signature, callers, callees, file skeleton",
                    ),
                    t(
                        "graph_trace_flow",
                        "Trace call tree from an entry point (DFS over CALLS)",
                    ),
                    t(
                        "graph_search",
                        "Full-text search symbols by name or signature",
                    ),
                    t(
                        "graph_skeleton",
                        "File or project skeleton (signatures only, token-efficient)",
                    ),
                    t(
                        "graph_file_deps",
                        "File import graph: imports and imported-by",
                    ),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-code".into(),
                transport: "stdio".into(),
                description: Some(
                    "Built-in code editing server — read, write, edit, bash, search, skeleton for a sandboxed workspace.".into(),
                ),
                tools: vec![
                    t("read_file",    "Read a file (with optional line range)"),
                    t("write_file",   "Create or overwrite a file, returns unified diff"),
                    t("edit_file",    "Exact-string replacement in a file, returns unified diff"),
                    t("bash",         "Run a shell command inside the workspace sandbox"),
                    t("search_code",  "AST pattern search (ast-grep) with grep fallback"),
                    t("glob",         "Find files matching a glob pattern"),
                    t("get_skeleton", "Token-efficient skeleton: signatures only, no bodies"),
                    t("list_files",   "List workspace directory as an indented tree"),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-litho".into(),
                transport: "stdio".into(),
                description: Some(
                    "Built-in Litho (deepwiki-rs) wrapper — generate C4 architecture docs via CLI."
                        .into(),
                ),
                tools: vec![
                    t(
                        "litho_generate",
                        "Run deepwiki-rs to generate architecture markdown from a codebase",
                    ),
                    t(
                        "litho_sync_knowledge",
                        "Sync external knowledge into Litho cache (sync-knowledge)",
                    ),
                    t(
                        "litho_read_doc",
                        "Read a generated file from a Litho output directory",
                    ),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-browser".into(),
                transport: "stdio".into(),
                description: Some(
                    "Built-in headless browser automation and web scraping service.".into(),
                ),
                tools: vec![
                    t("browser_navigate", "Navigate to a URL in a browser tab"),
                    t("browser_new_tab", "Create a new browser tab"),
                    t("browser_close_tab", "Close a browser tab by its tab_id"),
                    t("browser_list_tabs", "List all open browser tabs"),
                    t("browser_switch_tab", "Switch to a specific tab"),
                    t("browser_go_back", "Go back to the previous page"),
                    t("browser_go_forward", "Go forward to the next page"),
                    t("browser_reload", "Reload the current page"),
                    t("browser_click", "Click on an element by its index"),
                    t("browser_type", "Type text into an input element"),
                    t(
                        "browser_select_option",
                        "Select an option in a dropdown element",
                    ),
                    t("browser_scroll", "Scroll the page up or down"),
                    t("browser_hover", "Hover the mouse over an element"),
                    t("browser_press_key", "Press a keyboard key"),
                    t(
                        "browser_upload_file",
                        "Upload files to a file input element",
                    ),
                    t("browser_execute_js", "Execute JavaScript on the page"),
                    t(
                        "browser_wait",
                        "Wait for a condition (time, text, navigation)",
                    ),
                    t(
                        "browser_snapshot",
                        "Capture accessibility snapshot and interactive elements",
                    ),
                    t(
                        "browser_screenshot",
                        "Take a screenshot of the page or element",
                    ),
                    t("browser_extract_text", "Extract text content from the page"),
                    t("browser_extract_links", "Extract all links from the page"),
                    t("browser_extract_table", "Extract an HTML table as JSON"),
                    t(
                        "browser_extract_structured",
                        "Extract structured data using a JSON schema",
                    ),
                    t("browser_search", "Search Google or Bing and return results"),
                    t("browser_crawl", "Start a deep crawl from a URL"),
                    t("browser_crawl_status", "Check the status of a crawl job"),
                    t("browser_fill_form", "Fill multiple form fields at once"),
                    t("browser_click_and_wait", "Click and wait for navigation"),
                    t("browser_get_status", "Get browser bridge status"),
                    t("browser_stop_task", "Stop an ongoing task on a tab"),
                ],
            },
        ]
    }

    // ---- connection management ----

    /// Connect (or reconnect) an external MCP server.
    pub async fn connect_server(&self, name: &str) -> Result<McpServerInfo> {
        let mut external = self.external.write().await;
        let state = external
            .get_mut(name)
            .ok_or_else(|| anyhow::anyhow!("external MCP server not found: {name}"))?;

        // Disconnect old client if present
        if let Some(mut old) = state.client.take() {
            let _ = old.disconnect().await;
        }

        state.status = McpServerStatus::Connecting;
        state.error = None;

        let cfg = state.config.clone();
        drop(external);

        // Connect new client
        match ExternalMcpClient::connect(
            &cfg.name,
            &cfg.transport,
            cfg.command.as_deref(),
            &cfg.args,
            &cfg.env,
            cfg.url.as_deref(),
            &cfg.headers,
        )
        .await
        {
            Ok(mut client) => {
                // Handshake + list tools
                if let Err(e) = client.initialize().await {
                    let mut external = self.external.write().await;
                    if let Some(s) = external.get_mut(name) {
                        s.status = McpServerStatus::Error;
                        s.error = Some(format!("initialize failed: {e}"));
                    }
                    return Err(e);
                }

                let tools = client.list_tools().await.unwrap_or_default();
                let tool_defs: Vec<McpToolDef> = tools
                    .iter()
                    .map(|t| McpToolDef {
                        name: t.name.clone(),
                        description: if t.description.is_empty() {
                            None
                        } else {
                            Some(t.description.clone())
                        },
                        input_schema: if t.input_schema.is_null()
                            || t.input_schema.is_object()
                                && t.input_schema.as_object().map_or(false, |o| o.is_empty())
                        {
                            None
                        } else {
                            Some(t.input_schema.clone())
                        },
                    })
                    .collect();

                let mut external = self.external.write().await;
                if let Some(s) = external.get_mut(name) {
                    s.client = Some(client);
                    s.tools = tool_defs;
                    s.status = McpServerStatus::Connected;
                    s.error = None;
                    info!(
                        "MCP external {} connected — {} tool(s)",
                        name,
                        s.tools.len()
                    );
                    Ok(s.to_info())
                } else {
                    // Removed while we were connecting
                    Err(anyhow::anyhow!("server removed during connection"))
                }
            }
            Err(e) => {
                let mut external = self.external.write().await;
                if let Some(s) = external.get_mut(name) {
                    s.status = McpServerStatus::Error;
                    s.error = Some(format!("connect failed: {e}"));
                }
                Err(e)
            }
        }
    }

    /// Disconnect an external MCP server.
    pub async fn disconnect_server(&self, name: &str) -> Result<()> {
        let mut external = self.external.write().await;
        if let Some(state) = external.get_mut(name) {
            if let Some(mut client) = state.client.take() {
                match client.disconnect().await {
                    Ok(()) => {
                        state.status = McpServerStatus::Disconnected;
                        state.tools.clear();
                        info!("MCP external {name} disconnected");
                    }
                    Err(e) => {
                        state.status = McpServerStatus::Error;
                        state.error = Some(format!("disconnect failed: {e}"));
                        warn!("MCP external {name} disconnect error: {e}");
                    }
                }
            }
        }
        Ok(())
    }

    // ---- tool access ----

    /// Get all external MCP tools as `McpToolInfo` with `mcp__` prefix.
    pub async fn get_external_tools(&self) -> Vec<McpToolInfo> {
        let external = self.external.read().await;
        let mut tools = Vec::new();
        for state in external.values() {
            if state.status != McpServerStatus::Connected {
                continue;
            }
            for t in &state.tools {
                let prefixed_name = format!("mcp__{}__{}", state.config.name, t.name);
                let allowlisted = match &state.config.use_tools {
                    Some(list) => list.iter().any(|n| n == &t.name),
                    None => true,
                };
                if allowlisted {
                    tools.push(McpToolInfo {
                        name: prefixed_name,
                        description: t.description.clone().unwrap_or_default(),
                        input_schema: serde_json::Value::Null,
                    });
                }
            }
        }
        tools
    }

    /// Get the full `McpToolInfo` list including proper input schemas
    /// for the agent tool roster. External tools have `input_schema: Null`
    /// until we bridge them properly (step 7).
    pub async fn get_external_tools_full(&self) -> Vec<McpToolInfo> {
        self.get_external_tools().await
    }

    /// Call an external MCP tool by its full `mcp__server__tool` name.
    pub async fn call_external_tool(
        &self,
        full_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let (server_name, tool_name) = parse_mcp_tool_name(full_name)
            .ok_or_else(|| anyhow::anyhow!("invalid MCP tool name: {full_name}"))?;

        let mut external = self.external.write().await;
        let state = external
            .get_mut(&server_name)
            .ok_or_else(|| anyhow::anyhow!("MCP server not found: {server_name}"))?;

        let client = state
            .client
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("MCP server not connected: {server_name}"))?;

        client.call_tool(&tool_name, arguments).await
    }

    /// Test a tool by calling it on a connected external server.
    /// Returns the tool's JSON response. Returns 400 error for built-in servers.
    pub async fn test_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let external = self.external.read().await;
        if !external.contains_key(server_name) {
            anyhow::bail!(
                "Built-in server tools cannot be tested from the UI. Use the agent to invoke them."
            );
        }
        drop(external);
        let full_name = format!("mcp__{}__{}", server_name, tool_name);
        self.call_external_tool(&full_name, arguments).await
    }

    /// Clean up all external connections.
    pub async fn dispose(&self) {
        let names: Vec<String> = { self.external.read().await.keys().cloned().collect() };
        for name in &names {
            let _ = self.disconnect_server(name).await;
        }
        self.builtin_registry.kill_all();
        info!("MCP manager disposed");
    }
}
