//! Marketplace type definitions. Mirrors `src-old/marketplace/types.ts`.

use serde::{Deserialize, Serialize};

/// A marketplace source (git repository or local directory)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceSource {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub source_type: SourceType,
    pub url: Option<String>,
    pub branch: Option<String>,
    #[serde(rename = "localPath")]
    pub local_path: String,
    pub priority: i32,
    pub enabled: bool,
    #[serde(rename = "lastSynced")]
    pub last_synced: Option<String>,
    #[serde(rename = "syncError")]
    pub sync_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Git,
    Local,
}

/// Plugin-level toggle state: plugins[name] = true → enabled; absent = disabled (default-off)
/// mcpUseTools key: `${pluginName}/${serverName}` → string[] to allowlist tools, null to clear override
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarketplaceSourceItemState {
    pub plugins: std::collections::HashMap<String, bool>,
    #[serde(rename = "mcpUseTools")]
    pub mcp_use_tools: Option<std::collections::HashMap<String, Option<Vec<String>>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceConfig {
    pub sources: Vec<MarketplaceSource>,
}

impl Default for MarketplaceConfig {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MarketplaceStateFile {
    #[serde(flatten)]
    pub sources: std::collections::HashMap<String, MarketplaceSourceItemState>,
}

// ===== API response types =====

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePluginSkill {
    pub name: String,
    pub description: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePluginSubagent {
    pub name: String,
    pub description: String,
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePluginMCPServer {
    pub name: String,
    pub transport: String,
    pub description: Option<String>,
    #[serde(rename = "useTools")]
    pub use_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplacePlugin {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub author: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub dir: String,
    #[serde(rename = "sourceId")]
    pub source_id: String,
    #[serde(rename = "sourceName")]
    pub source_name: String,
    pub priority: i32,
    pub enabled: bool,
    #[serde(rename = "skillCount")]
    pub skill_count: usize,
    #[serde(rename = "subagentCount")]
    pub subagent_count: usize,
    #[serde(rename = "hasHooks")]
    pub has_hooks: bool,
    #[serde(rename = "mcpServerCount")]
    pub mcp_server_count: usize,
    pub skills: Vec<MarketplacePluginSkill>,
    pub subagents: Vec<MarketplacePluginSubagent>,
    #[serde(rename = "mcpServers")]
    pub mcp_servers: Vec<MarketplacePluginMCPServer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceSourceInfo {
    #[serde(flatten)]
    pub source: MarketplaceSource,
    pub plugins: Vec<MarketplacePlugin>,
}
