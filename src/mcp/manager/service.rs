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
            let project_cfg = self
                .config_mgr
                .load_project_config(&self.working_dir);
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
                        let project = self
                            .config_mgr
                            .load_project_config(&self.working_dir);
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

    /// Get built-in server info summaries.
    pub fn get_builtin_servers(&self) -> Vec<BuiltInServerInfo> {
        // These are the servers created by helper.rs builders.
        // We return static descriptions since built-in servers are known.
        vec![
            BuiltInServerInfo {
                name: "senclaw-schedule".into(),
                transport: "stdio".into(),
                description: Some("Schedule task execution (cron/interval/once)".into()),
                tools: vec![
                    "schedule_task".into(),
                    "list_tasks".into(),
                    "pause_task".into(),
                    "cancel_task".into(),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-workspace".into(),
                transport: "stdio".into(),
                description: Some("Workspace directory switching".into()),
                tools: vec![
                    "workspace_switch".into(),
                    "workspace_reset".into(),
                    "workspace_info".into(),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-send".into(),
                transport: "stdio".into(),
                description: Some("Send messages/files via HTTP bridge".into()),
                tools: vec!["send_message".into(), "send_file".into()],
            },
            BuiltInServerInfo {
                name: "senclaw-memory".into(),
                transport: "stdio".into(),
                description: Some("FTS5 + vector memory search".into()),
                tools: vec!["memory_search".into(), "memory_get".into()],
            },
            BuiltInServerInfo {
                name: "senclaw-dispatch".into(),
                transport: "stdio".into(),
                description: Some("DAG task orchestration (admin only)".into()),
                tools: vec![
                    "list_agents".into(),
                    "create_parent".into(),
                    "dispatch_task".into(),
                ],
            },
            BuiltInServerInfo {
                name: "senclaw-virtual".into(),
                transport: "stdio".into(),
                description: Some("Virtual persona execution".into()),
                tools: vec!["list_personas".into(), "run_persona".into()],
            },
            BuiltInServerInfo {
                name: "senclaw-feishu-wiki".into(),
                transport: "stdio".into(),
                description: Some("Feishu/Lark wiki integration".into()),
                tools: vec![
                    "wiki_list_spaces".into(),
                    "wiki_get_space".into(),
                    "wiki_list_nodes".into(),
                    "wiki_get_node".into(),
                    "wiki_create_node".into(),
                    "wiki_search".into(),
                    "doc_read_blocks".into(),
                    "doc_write_blocks".into(),
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

    /// Clean up all external connections.
    pub async fn dispose(&self) {
        let names: Vec<String> = {
            self.external.read().await.keys().cloned().collect()
        };
        for name in &names {
            let _ = self.disconnect_server(name).await;
        }
        self.builtin_registry.kill_all();
        info!("MCP manager disposed");
    }
}
