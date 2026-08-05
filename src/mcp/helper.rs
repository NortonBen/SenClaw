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
        // Desktop app (senclaw-app) is a different binary that cannot dispatch
        // `*-server` subcommands, so it sets SENCLAW_BIN to the bundled CLI.
        let command = std::env::var("SENCLAW_BIN").ok().unwrap_or_else(|| {
            std::env::current_exe()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| "senclaw".to_owned())
        });
        Self {
            name: name.to_owned(),
            transport: "stdio".to_owned(),
            command,
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

// ===== BackgroundTool =====

/// MCP config for `senclaw-background` — lets a chat create/manage autonomous
/// background tasks. Owner is pinned from `group_folder`, never a tool param.
pub fn background_mcp_config(db_path: &str, group_folder: &str, chat_jid: &str) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-background", "background-server");
    cfg.env.insert("SENCLAW_DB_PATH".into(), db_path.to_owned());
    cfg.env
        .insert("SENCLAW_GROUP_FOLDER".into(), group_folder.to_owned());
    cfg.env
        .insert("SENCLAW_CHAT_JID".into(), chat_jid.to_owned());
    cfg
}

// ===== UsageTool =====

/// MCP config for `senclaw-usage` — read-only token/cost accounting queries
/// (usage_overview / usage_breakdown / usage_query).
pub fn usage_mcp_config(db_path: &str) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-usage", "usage-server");
    cfg.env.insert("SENCLAW_DB_PATH".into(), db_path.to_owned());
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

// ===== CognitiveTool =====

pub fn cognitive_mcp_config(
    db_path: &str,
    group_folder: &str,
    llm_disabled: bool,
) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-cognitive", "cognitive-server");
    cfg.env.insert("SENCLAW_DB_PATH".into(), db_path.to_owned());
    cfg.env
        .insert("SENCLAW_GROUP_FOLDER".into(), group_folder.to_owned());
    if llm_disabled {
        cfg.env
            .insert("SENCLAW_COG_LLM_DISABLED".into(), "1".to_owned());
    }
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
    bot_token: Option<&str>,
    db_path: &str,
) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-send", "send-server");
    cfg.env
        .insert("SENCLAW_SEND_BRIDGE_PORT".into(), bridge_port.to_string());
    cfg.env
        .insert("SENCLAW_CHAT_JID".into(), chat_jid.to_owned());
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

// ===== OCR =====

/// Build the MCP config for the OCR subprocess. The subprocess does not link
/// MNN — it forwards image bytes to the daemon's `/api/ocr/recognize` endpoint.
pub fn ocr_mcp_config(ui_port: u16) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-ocr", "ocr-server");
    cfg.env.insert(
        "SENCLAW_OCR_BRIDGE_URL".into(),
        format!("http://127.0.0.1:{ui_port}"),
    );
    cfg
}

// ===== OS sandbox (sbx_* tools) =====

/// OS-sandbox executor (`senclaw-sandbox`). State is the shared engine DB
/// under `~/.senclaw/sandbox` — the subprocess opens it itself, so the only
/// thing to forward is a non-default data root.
pub fn sandbox_mcp_config() -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-sandbox", "sandbox-server");
    for key in ["SENCLAW_SANDBOX_DATA_DIR", "SANDBOX_DATA_DIR"] {
        if let Ok(v) = std::env::var(key) {
            if !v.trim().is_empty() {
                cfg.env.insert(key.into(), v);
            }
        }
    }
    cfg
}

// ===== JS executor =====

/// Sandboxed JavaScript executor (QuickJS). No state is shared with the daemon;
/// the subprocess just needs its default timeout / memory limits.
pub fn js_mcp_config(default_timeout_ms: u64, default_memory_mb: u64) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-js", "js-server");
    cfg.env.insert(
        "SENCLAW_JS_TIMEOUT_MS".into(),
        default_timeout_ms.to_string(),
    );
    cfg.env
        .insert("SENCLAW_JS_MEMORY_MB".into(), default_memory_mb.to_string());
    cfg
}

// ===== Browser =====

/// `agent_id` identifies the agent this server instance serves; the extension
/// allocates one tab per agent_id so concurrent agents don't share a tab.
pub fn browser_mcp_config(ws_port: u16, agent_id: &str) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-browser", "browser-server");
    cfg.env
        .insert("SENCLAW_WS_PORT".into(), ws_port.to_string());
    if !agent_id.is_empty() {
        cfg.env
            .insert("SENCLAW_AGENT_ID".into(), agent_id.to_string());
    }
    cfg
}

// ===== Litho (deepwiki-rs CLI) =====

/// Litho documentation generator — wraps the external `deepwiki-rs` binary.
pub fn litho_mcp_config(
    litho_binary: &str,
    llm_api_base_url: Option<&str>,
    llm_api_key: Option<&str>,
    model_efficient: Option<&str>,
) -> McpServerConfig {
    let mut cfg = McpServerConfig::new("senclaw-litho", "litho-server");
    cfg.env
        .insert("SENCLAW_LITHO_BINARY".into(), litho_binary.to_owned());
    if let Some(u) = llm_api_base_url {
        if !u.is_empty() {
            cfg.env
                .insert("SENCLAW_LITHO_LLMAPI_BASE_URL".into(), u.to_owned());
        }
    }
    if let Some(k) = llm_api_key {
        if !k.is_empty() {
            cfg.env
                .insert("SENCLAW_LITHO_LLMAPI_KEY".into(), k.to_owned());
        }
    }
    if let Some(m) = model_efficient {
        if !m.is_empty() {
            cfg.env
                .insert("SENCLAW_LITHO_MODEL_EFFICIENT".into(), m.to_owned());
        }
    }
    cfg
}
