//! Marketplace manager. Mirrors `src-old/marketplace/MarketplaceManager.ts`.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use uuid::Uuid;

use super::git_sync::clone_or_pull;
use super::types::{
    MarketplaceConfig, MarketplacePlugin, MarketplacePluginMCPServer, MarketplacePluginSkill,
    MarketplacePluginSubagent, MarketplaceSource, MarketplaceSourceInfo,
    MarketplaceSourceItemState, MarketplaceStateFile, SourceType,
};

/// Marketplace manager for git/local plugin sources
pub struct MarketplaceManager {
    config: MarketplaceConfig,
    state: MarketplaceStateFile,
    config_path: PathBuf,
    state_path: PathBuf,
    clones_dir: PathBuf,
}

/// Plugin definition for discovery
#[derive(Debug, Clone)]
struct PluginDef {
    dir: String,
    plugin_json_path: String,
}

/// Plugin JSON metadata
#[derive(Debug, Clone, serde::Deserialize)]
struct PluginJson {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    author: Option<serde_json::Value>,
    keywords: Option<Vec<String>>,
}

impl MarketplaceManager {
    /// Create a new marketplace manager with default paths
    pub fn new() -> Result<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Failed to get home directory"))?;
        let senclaw_home = home.join(".senclaw");

        let config_path = senclaw_home.join("marketplace.json");
        let state_path = senclaw_home.join("marketplace-state.json");
        let clones_dir = senclaw_home.join("marketplace");

        let mut manager = Self {
            config: MarketplaceConfig::default(),
            state: MarketplaceStateFile::default(),
            config_path,
            state_path,
            clones_dir,
        };

        manager.load_config()?;
        manager.load_state()?;

        Ok(manager)
    }

    /// Create a marketplace manager with custom paths (for testing)
    pub fn with_paths(
        config_path: PathBuf,
        state_path: PathBuf,
        clones_dir: PathBuf,
    ) -> Result<Self> {
        let mut manager = Self {
            config: MarketplaceConfig::default(),
            state: MarketplaceStateFile::default(),
            config_path,
            state_path,
            clones_dir,
        };

        manager.load_config()?;
        manager.load_state()?;

        Ok(manager)
    }

    // ── Config/State persistence ─────────────────────────────────────────────────────

    fn load_config(&mut self) -> Result<()> {
        if self.config_path.exists() {
            let raw = fs::read_to_string(&self.config_path)
                .with_context(|| format!("Failed to read config from {:?}", self.config_path))?;
            self.config =
                serde_json::from_str(&raw).with_context(|| "Failed to parse marketplace config")?;
        }
        Ok(())
    }

    fn save_config(&self) -> Result<()> {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {:?}", parent))?;
        }
        let json = serde_json::to_string_pretty(&self.config)?;
        fs::write(&self.config_path, json + "\n")
            .with_context(|| format!("Failed to write config to {:?}", self.config_path))?;
        Ok(())
    }

    fn load_state(&mut self) -> Result<()> {
        if self.state_path.exists() {
            let raw = fs::read_to_string(&self.state_path)
                .with_context(|| format!("Failed to read state from {:?}", self.state_path))?;
            self.state =
                serde_json::from_str(&raw).with_context(|| "Failed to parse marketplace state")?;
        }
        Ok(())
    }

    fn save_state(&self) -> Result<()> {
        if let Some(parent) = self.state_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create state directory {:?}", parent))?;
        }
        let json = serde_json::to_string_pretty(&self.state)?;
        fs::write(&self.state_path, json + "\n")
            .with_context(|| format!("Failed to write state to {:?}", self.state_path))?;
        Ok(())
    }

    // ── Source CRUD ─────────────────────────────────────────────────────────────────

    /// Get all sources sorted by priority (ascending)
    pub fn get_sources(&self) -> Vec<MarketplaceSource> {
        let mut sources = self.config.sources.clone();
        sources.sort_by_key(|s| s.priority);
        sources
    }

    /// Get a source by ID
    pub fn get_source(&self, id: &str) -> Option<MarketplaceSource> {
        self.config.sources.iter().find(|s| s.id == id).cloned()
    }

    /// Add a new source
    pub fn add_source(
        &mut self,
        name: String,
        source_type: SourceType,
        url: Option<String>,
        branch: Option<String>,
        local_path: Option<String>,
        priority: Option<i32>,
        enabled: Option<bool>,
    ) -> Result<MarketplaceSource> {
        let id = Uuid::new_v4().to_string();
        let max_priority = self
            .config
            .sources
            .iter()
            .map(|s| s.priority)
            .max()
            .unwrap_or(0);

        let local_path = match source_type {
            SourceType::Git => self.clones_dir.join(&id).to_string_lossy().to_string(),
            SourceType::Local => {
                let path = local_path.unwrap_or_else(|| ".".to_string());
                PathBuf::from(&path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&path))
                    .to_string_lossy()
                    .to_string()
            }
        };

        let source = MarketplaceSource {
            id: id.clone(),
            name,
            source_type,
            url,
            branch: branch.or(Some("main".to_string())),
            local_path,
            priority: priority.unwrap_or(max_priority + 1),
            enabled: enabled.unwrap_or(true),
            last_synced: None,
            sync_error: None,
        };

        self.config.sources.push(source.clone());
        self.save_config()?;
        Ok(source)
    }

    /// Update an existing source
    pub fn update_source(
        &mut self,
        id: &str,
        name: Option<String>,
        url: Option<Option<String>>,
        branch: Option<Option<String>>,
        local_path: Option<String>,
        priority: Option<i32>,
        enabled: Option<bool>,
        last_synced: Option<Option<String>>,
        sync_error: Option<Option<String>>,
    ) -> Result<Option<MarketplaceSource>> {
        let idx = self
            .config
            .sources
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| anyhow::anyhow!("Source not found: {}", id))?;

        let source = &mut self.config.sources[idx];
        if let Some(name) = name {
            source.name = name;
        }
        if let Some(url) = url {
            source.url = url;
        }
        if let Some(branch) = branch {
            source.branch = branch;
        }
        if let Some(local_path) = local_path {
            source.local_path = local_path;
        }
        if let Some(priority) = priority {
            source.priority = priority;
        }
        if let Some(enabled) = enabled {
            source.enabled = enabled;
        }
        if let Some(last_synced) = last_synced {
            source.last_synced = last_synced;
        }
        if let Some(sync_error) = sync_error {
            source.sync_error = sync_error;
        }

        let updated = source.clone();
        self.save_config()?;
        Ok(Some(updated))
    }

    /// Remove a source
    pub fn remove_source(&mut self, id: &str) -> Result<bool> {
        let idx = self
            .config
            .sources
            .iter()
            .position(|s| s.id == id)
            .ok_or_else(|| anyhow::anyhow!("Source not found: {}", id))?;

        let source = self.config.sources.remove(idx);
        self.state.sources.remove(id);
        self.save_config()?;
        self.save_state()?;

        // Clean up git clone directory
        if source.source_type == SourceType::Git {
            let clone_dir = self.clones_dir.join(&source.id);
            if clone_dir.exists() {
                let _ = fs::remove_dir_all(&clone_dir);
            }
        }

        Ok(true)
    }

    /// Sync a git source (clone or pull)
    pub fn sync_source(&mut self, id: &str) -> Result<()> {
        let source = self
            .get_source(id)
            .ok_or_else(|| anyhow::anyhow!("Source not found: {}", id))?;

        if source.source_type == SourceType::Git {
            let url = source
                .url
                .ok_or_else(|| anyhow::anyhow!("Git source missing URL"))?;
            let branch = source.branch.as_deref().unwrap_or("main");
            let local_path = Path::new(&source.local_path);

            clone_or_pull(&url, branch, local_path)?;

            let now = chrono::Utc::now().to_rfc3339();
            self.update_source(
                id,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(Some(now)),
                Some(None),
            )?;
        }

        Ok(())
    }

    /// Reorder sources by priority
    pub fn reorder_sources(&mut self, ordered_ids: Vec<String>) -> Result<()> {
        for (i, id) in ordered_ids.iter().enumerate() {
            if let Some(idx) = self.config.sources.iter().position(|s| &s.id == id) {
                self.config.sources[idx].priority = (i + 1) as i32;
            }
        }
        self.save_config()?;
        Ok(())
    }

    // ── Plugin state management ───────────────────────────────────────────────────────

    /// Get source state (migration-safe)
    fn get_source_state(&self, source_id: &str) -> MarketplaceSourceItemState {
        self.state
            .sources
            .get(source_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Ensure source state exists
    fn ensure_source_state(&mut self, source_id: &str) -> &mut MarketplaceSourceItemState {
        if !self.state.sources.contains_key(source_id) {
            self.state
                .sources
                .insert(source_id.to_string(), MarketplaceSourceItemState::default());
        }
        self.state.sources.get_mut(source_id).unwrap()
    }

    /// Set plugin enabled/disabled state
    pub fn set_plugin_enabled(
        &mut self,
        source_id: &str,
        plugin_name: &str,
        enabled: bool,
    ) -> Result<()> {
        let st = self.ensure_source_state(source_id);
        if enabled {
            st.plugins.insert(plugin_name.to_string(), true);
        } else {
            st.plugins.remove(plugin_name);
        }
        self.save_state()?;
        Ok(())
    }

    /// Enable all plugins in a source
    pub fn enable_all_in_source(&mut self, source_id: &str) -> Result<()> {
        let source = self
            .get_source(source_id)
            .ok_or_else(|| anyhow::anyhow!("Source not found: {}", source_id))?;

        // Collect plugin names first
        let plugins = self.find_plugins(&source.local_path)?;
        let mut plugin_names = Vec::new();
        for plugin in plugins {
            let meta = self.read_plugin_json(&plugin.plugin_json_path)?;
            let name = self.plugin_name(&meta, &plugin.dir);
            plugin_names.push(name);
        }

        // Then enable them
        let st = self.ensure_source_state(source_id);
        for name in plugin_names {
            st.plugins.insert(name, true);
        }
        self.save_state()?;
        Ok(())
    }

    /// Disable all plugins in a source
    pub fn disable_all_in_source(&mut self, source_id: &str) -> Result<()> {
        let st = self.ensure_source_state(source_id);
        st.plugins.clear();
        self.save_state()?;
        Ok(())
    }

    /// Enable all plugins across all sources
    pub fn enable_all(&mut self) -> Result<()> {
        let source_ids: Vec<String> = self.config.sources.iter().map(|s| s.id.clone()).collect();
        for id in source_ids {
            let _ = self.enable_all_in_source(&id);
        }
        Ok(())
    }

    /// Disable all plugins across all sources
    pub fn disable_all(&mut self) -> Result<()> {
        let source_ids: Vec<String> = self.config.sources.iter().map(|s| s.id.clone()).collect();
        for id in source_ids {
            let _ = self.disable_all_in_source(&id);
        }
        Ok(())
    }

    // ── Plugin discovery ──────────────────────────────────────────────────────────────

    /// Find all plugins in a directory
    fn find_plugins(&self, base_path: &str) -> Result<Vec<PluginDef>> {
        let base = Path::new(base_path);
        if !base.exists() {
            return Ok(Vec::new());
        }

        let mut plugins = Vec::new();
        let entries =
            fs::read_dir(base).with_context(|| format!("Failed to read directory {:?}", base))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if dir_name.starts_with('.') {
                continue;
            }

            let plugin_json = path.join("plugin.json");
            if plugin_json.exists() {
                plugins.push(PluginDef {
                    dir: path.to_string_lossy().to_string(),
                    plugin_json_path: plugin_json.to_string_lossy().to_string(),
                });
            }
        }

        Ok(plugins)
    }

    /// Read plugin.json metadata
    fn read_plugin_json(&self, path: &str) -> Result<PluginJson> {
        let raw = fs::read_to_string(path)
            .with_context(|| format!("Failed to read plugin.json from {:?}", path))?;
        let meta: PluginJson = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse plugin.json from {:?}", path))?;
        Ok(meta)
    }

    /// Get plugin name from metadata and directory
    fn plugin_name(&self, meta: &PluginJson, dir: &str) -> String {
        meta.name.clone().unwrap_or_else(|| {
            Path::new(dir)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
    }

    /// Parse author field from JSON (can be string or object with name field)
    fn parse_author(&self, author: &Option<serde_json::Value>) -> Option<String> {
        match author {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Object(obj)) => obj
                .get("name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            _ => None,
        }
    }

    /// Get source info with plugins
    pub fn get_source_info(&self, source_id: &str) -> Result<Option<MarketplaceSourceInfo>> {
        let source = self.get_source(source_id);
        let source = match source {
            Some(s) => s,
            None => return Ok(None),
        };

        let st = self.get_source_state(source_id);
        let plugins = self.find_plugins(&source.local_path)?;

        let mut plugin_list = Vec::new();
        for plugin in plugins {
            let meta = self.read_plugin_json(&plugin.plugin_json_path)?;
            let name = self.plugin_name(&meta, &plugin.dir);
            let enabled = st.plugins.get(&name).copied().unwrap_or(false);

            // Discover skills, subagents, MCP servers (simplified for now)
            let skills = self.discover_skills(&plugin.dir)?;
            let subagents = self.discover_subagents(&plugin.dir)?;
            let mcp_servers = self.discover_mcp_servers(&plugin.dir)?;
            let has_hooks = Path::new(&plugin.dir).join("hooks").exists();

            plugin_list.push(MarketplacePlugin {
                name: name.clone(),
                description: meta.description.unwrap_or_default(),
                version: meta.version,
                author: self.parse_author(&meta.author),
                keywords: meta.keywords,
                dir: plugin.dir,
                source_id: source.id.clone(),
                source_name: source.name.clone(),
                priority: source.priority,
                enabled,
                skill_count: skills.len(),
                subagent_count: subagents.len(),
                has_hooks,
                mcp_server_count: mcp_servers.len(),
                skills,
                subagents,
                mcp_servers,
            });
        }

        Ok(Some(MarketplaceSourceInfo {
            source,
            plugins: plugin_list,
        }))
    }

    /// Discover skills in a plugin directory
    fn discover_skills(&self, dir: &str) -> Result<Vec<MarketplacePluginSkill>> {
        let skills_dir = Path::new(dir).join("skills");
        if !skills_dir.exists() {
            return Ok(Vec::new());
        }

        let mut skills = Vec::new();
        // Simplified: just list directories
        if let Ok(entries) = fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    skills.push(MarketplacePluginSkill {
                        name: entry.file_name().to_string_lossy().to_string(),
                        description: String::new(),
                        disabled: false,
                    });
                }
            }
        }
        Ok(skills)
    }

    /// Discover subagents in a plugin directory
    fn discover_subagents(&self, dir: &str) -> Result<Vec<MarketplacePluginSubagent>> {
        let subagents_dir = Path::new(dir).join("subagents");
        if !subagents_dir.exists() {
            return Ok(Vec::new());
        }

        let mut subagents = Vec::new();
        if let Ok(entries) = fs::read_dir(&subagents_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    subagents.push(MarketplacePluginSubagent {
                        name: entry.file_name().to_string_lossy().to_string(),
                        description: String::new(),
                        disabled: false,
                    });
                }
            }
        }
        Ok(subagents)
    }

    /// Discover MCP servers in a plugin directory
    fn discover_mcp_servers(&self, dir: &str) -> Result<Vec<MarketplacePluginMCPServer>> {
        let mcp_dir = Path::new(dir).join("mcp");
        if !mcp_dir.exists() {
            return Ok(Vec::new());
        }

        let mut servers = Vec::new();
        if let Ok(entries) = fs::read_dir(&mcp_dir) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    servers.push(MarketplacePluginMCPServer {
                        name: entry.file_name().to_string_lossy().to_string(),
                        transport: "stdio".to_string(),
                        description: None,
                        use_tools: None,
                    });
                }
            }
        }
        Ok(servers)
    }

    /// Get all enabled MCP servers from all enabled plugins across all sources.
    /// Mirrors TS MarketplaceManager.getMCPServerDefs().
    pub fn get_enabled_mcp_servers(&self) -> Vec<MarketplacePluginMCPServer> {
        let mut all_servers = Vec::new();

        for source in &self.config.sources {
            if !source.enabled {
                continue;
            }

            let st = self.get_source_state(&source.id);
            if let Ok(plugins) = self.find_plugins(&source.local_path) {
                for plugin in plugins {
                    let meta = match self.read_plugin_json(&plugin.plugin_json_path) {
                        Ok(m) => m,
                        Err(_) => continue,
                    };
                    let name = self.plugin_name(&meta, &plugin.dir);

                    // Only include if plugin is enabled
                    if !st.plugins.get(&name).copied().unwrap_or(false) {
                        continue;
                    }

                    // Get MCP servers for this plugin
                    if let Ok(servers) = self.discover_mcp_servers(&plugin.dir) {
                        for mut server in servers {
                            // Prefix with plugin name to avoid conflicts
                            server.name = format!("mkt__{}__{}", name, server.name);
                            all_servers.push(server);
                        }
                    }
                }
            }
        }

        all_servers
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_manager_creation() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("config.json");
        let state_path = temp.path().join("state.json");
        let clones_dir = temp.path().join("clones");

        let manager = MarketplaceManager::with_paths(config_path, state_path, clones_dir).unwrap();
        assert!(manager.get_sources().is_empty());
    }

    #[test]
    fn test_add_source() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("config.json");
        let state_path = temp.path().join("state.json");
        let clones_dir = temp.path().join("clones");

        let mut manager =
            MarketplaceManager::with_paths(config_path, state_path, clones_dir).unwrap();
        let source = manager
            .add_source(
                "test".to_string(),
                SourceType::Local,
                None,
                None,
                Some(temp.path().to_string_lossy().to_string()),
                None,
                None,
            )
            .unwrap();

        assert_eq!(source.name, "test");
        assert_eq!(manager.get_sources().len(), 1);
    }

    #[test]
    fn test_remove_source() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("config.json");
        let state_path = temp.path().join("state.json");
        let clones_dir = temp.path().join("clones");

        let mut manager =
            MarketplaceManager::with_paths(config_path, state_path, clones_dir).unwrap();
        let source = manager
            .add_source(
                "test".to_string(),
                SourceType::Local,
                None,
                None,
                Some(temp.path().to_string_lossy().to_string()),
                None,
                None,
            )
            .unwrap();

        let result = manager.remove_source(&source.id).unwrap();
        assert!(result);
        assert!(manager.get_sources().is_empty());
    }
}
