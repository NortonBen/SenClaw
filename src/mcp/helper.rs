//! MCP server config builder. Port target: src-old/mcp/mcpHelper.ts
//!
//! Builds config structs consumed by AgentPool when registering MCP servers.
//! Each builder takes typed parameters instead of env-vars; the env-var model
//! used in the TS subprocess architecture is replaced by direct function
//! arguments in Rust.

use std::collections::HashMap;

/// Describes how to launch and communicate with an MCP server subprocess.
/// Mirrors `MCPServerConfig` from sema-core.
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

impl McpServerConfig {
    pub fn new(name: &str, server_path: &str) -> Self {
        Self {
            name: name.to_owned(),
            transport: "stdio".to_owned(),
            command: std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "senclaw".to_owned()),
            args: vec![server_path.to_owned()],
            env: HashMap::new(),
        }
    }
}

// ===== ScheduleTool =====

pub fn schedule_mcp_config(db_path: &str, group_folder: &str, chat_jid: &str) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-schedule", "schedule-server");
    cfg.env.insert("SENCLAW_DB_PATH".into(), db_path.to_owned());
    cfg.env
        .insert("SENCLAW_GROUP_FOLDER".into(), group_folder.to_owned());
    cfg.env
        .insert("SENCLAW_CHAT_JID".into(), chat_jid.to_owned());
    cfg
}

// ===== WorkspaceTool =====

pub fn workspace_mcp_config(
    state_file: &str,
    default_workspace: &str,
    allowed_work_dirs: Option<&[String]>,
) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-workspace", "workspace-server");
    cfg.env
        .insert("SENCLAW_WORKSPACE_STATE_FILE".into(), state_file.to_owned());
    cfg.env.insert(
        "SENCLAW_DEFAULT_WORKSPACE".into(),
        default_workspace.to_owned(),
    );
    let dirs_str = match allowed_work_dirs {
        None => String::new(),
        Some(list) => serde_json::to_string(list).unwrap_or_default(),
    };
    cfg.env.insert("SENCLAW_ALLOWED_WORK_DIRS".into(), dirs_str);
    cfg
}

// ===== SpaceTool =====

pub fn space_mcp_config(db_path: &str, group_folder: &str, chat_jid: &str) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-space", "space-server");
    cfg.env.insert("SENCLAW_DB_PATH".into(), db_path.to_owned());
    cfg.env
        .insert("SENCLAW_GROUP_FOLDER".into(), group_folder.to_owned());
    cfg.env
        .insert("SENCLAW_CHAT_JID".into(), chat_jid.to_owned());
    cfg
}

// ===== MemoryTool =====

pub fn memory_mcp_config(
    db_path: &str,
    folder: &str,
    agents_dir: &str,
    embedding_provider: Option<&str>,
    openai_api_key: Option<&str>,
    openai_base_url: Option<&str>,
    custom_memory_dir: Option<&str>,
) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-memory", "memory-server");
    cfg.env.insert("SENCLAW_DB_PATH".into(), db_path.to_owned());
    cfg.env.insert("SENCLAW_FOLDER".into(), folder.to_owned());
    cfg.env
        .insert("SENCLAW_AGENTS_DIR".into(), agents_dir.to_owned());
    if let Some(p) = embedding_provider {
        cfg.env
            .insert("SENCLAW_EMBEDDING_PROVIDER".into(), p.to_owned());
    }
    if let Some(k) = openai_api_key {
        cfg.env
            .insert("SENCLAW_OPENAI_API_KEY".into(), k.to_owned());
    }
    if let Some(u) = openai_base_url {
        cfg.env
            .insert("SENCLAW_OPENAI_BASE_URL".into(), u.to_owned());
    }
    if let Some(d) = custom_memory_dir {
        cfg.env
            .insert("SENCLAW_CUSTOM_MEMORY_DIR".into(), d.to_owned());
    }
    cfg
}

// ===== SendTool =====

pub fn send_mcp_config(
    bridge_port: u16,
    chat_jid: &str,
    is_admin: bool,
    bot_token: Option<&str>,
    db_path: &str,
) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-send", "send-server");
    cfg.env
        .insert("SENCLAW_SEND_BRIDGE_PORT".into(), bridge_port.to_string());
    cfg.env
        .insert("SENCLAW_CHAT_JID".into(), chat_jid.to_owned());
    cfg.env.insert(
        "SENCLAW_IS_ADMIN".into(),
        if is_admin { "1".into() } else { "0".into() },
    );
    if let Some(tok) = bot_token {
        cfg.env.insert("SENCLAW_BOT_TOKEN".into(), tok.to_owned());
    }
    cfg.env.insert("SENCLAW_DB_PATH".into(), db_path.to_owned());
    cfg
}

// ===== DispatchTool =====

pub fn dispatch_mcp_config(
    state_path: &str,
    admin_folder: &str,
    agents_config_dir: Option<&str>,
    cowork_agents_json: Option<&str>,
) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-dispatch", "dispatch-server");
    cfg.env
        .insert("SENCLAW_DISPATCH_STATE_PATH".into(), state_path.to_owned());
    cfg.env
        .insert("SENCLAW_ADMIN_FOLDER".into(), admin_folder.to_owned());
    if let Some(d) = agents_config_dir {
        cfg.env
            .insert("SENCLAW_AGENTS_CONFIG_DIR".into(), d.to_owned());
    }
    if let Some(j) = cowork_agents_json {
        cfg.env
            .insert("SENCLAW_DISPATCH_COWORK_AGENTS_JSON".into(), j.to_owned());
    }
    cfg
}

// ===== VirtualAgent =====

pub fn virtual_mcp_config(
    agents_config_dir: &str,
    admin_folder: &str,
    default_workspace: &str,
) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-virtual", "virtual-server");
    cfg.env.insert(
        "SENCLAW_AGENTS_CONFIG_DIR".into(),
        agents_config_dir.to_owned(),
    );
    cfg.env
        .insert("SENCLAW_ADMIN_FOLDER".into(), admin_folder.to_owned());
    cfg.env.insert(
        "SENCLAW_DEFAULT_WORKSPACE".into(),
        default_workspace.to_owned(),
    );
    cfg
}

// ===== Wiki (local git, `crate::wiki`) =====

pub fn wiki_mcp_config(wiki_dir: &str) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-wiki", "wiki-server");
    cfg.env
        .insert("SENCLAW_WIKI_DIR".into(), wiki_dir.to_owned());
    cfg
}

// ===== Code Knowledge Graph =====

pub fn code_graph_mcp_config(db_path: &str, project_id: &str, workspace: &str) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-code-graph", "code-graph-server");
    cfg.env.insert("SENCLAW_DB_PATH".into(), db_path.to_owned());
    cfg.env.insert("SENCLAW_PROJECT_ID".into(), project_id.to_owned());
    cfg.env.insert("SENCLAW_WORKSPACE".into(), workspace.to_owned());
    cfg
}

// ===== Code Server =====

pub fn code_server_mcp_config(workspace: &str, project_id: &str) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-code", "code-server");
    cfg.env.insert("SENCLAW_CODE_WORKSPACE".into(), workspace.to_owned());
    cfg.env.insert("SENCLAW_CODE_PROJECT_ID".into(), project_id.to_owned());
    cfg
}

// ===== Browser =====

pub fn browser_mcp_config(ws_port: u16) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-browser", "browser-server");
    cfg.env
        .insert("SENCLAW_WS_PORT".into(), ws_port.to_string());
    cfg
}
