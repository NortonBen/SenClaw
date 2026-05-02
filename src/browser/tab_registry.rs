//! Tab state registry — tracks open browser tabs and their status.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::types::*;

/// Thread-safe registry of browser tabs.
#[derive(Clone)]
pub struct TabRegistry {
    tabs: Arc<RwLock<HashMap<TabId, TabState>>>,
    active_tab: Arc<RwLock<Option<TabId>>>,
    last_heartbeat: Arc<RwLock<Option<std::time::Instant>>>,
}

impl TabRegistry {
    pub fn new() -> Self {
        Self {
            tabs: Arc::new(RwLock::new(HashMap::new())),
            active_tab: Arc::new(RwLock::new(None)),
            last_heartbeat: Arc::new(RwLock::new(None)),
        }
    }

    /// Register a new tab.
    pub async fn register(&self, tab_id: TabId, url: String) {
        let mut tabs = self.tabs.write().await;
        tabs.insert(
            tab_id.clone(),
            TabState {
                tab_id,
                url,
                title: String::new(),
                status: TabStatus::Loading,
                created_at: std::time::Instant::now(),
            },
        );
    }

    /// Update tab metadata (URL, title, status).
    pub async fn update(&self, tab_id: &str, url: String, title: String, status: String) {
        let mut tabs = self.tabs.write().await;
        if let Some(tab) = tabs.get_mut(tab_id) {
            tab.url = url;
            tab.title = title;
            tab.status = match status.as_str() {
                "loading" => TabStatus::Loading,
                "complete" => TabStatus::Complete,
                other => TabStatus::Error(other.to_owned()),
            };
        }
    }

    /// Remove a closed tab.
    pub async fn remove(&self, tab_id: &str) {
        let mut tabs = self.tabs.write().await;
        tabs.remove(tab_id);
        // Clear active if it was the removed tab
        let mut active = self.active_tab.write().await;
        if active.as_deref() == Some(tab_id) {
            *active = None;
        }
    }

    /// Set the active tab.
    pub async fn set_active(&self, tab_id: &str) {
        *self.active_tab.write().await = Some(tab_id.to_owned());
    }

    /// Get the active tab ID.
    pub async fn get_active(&self) -> Option<TabId> {
        self.active_tab.read().await.clone()
    }

    /// Get a specific tab's state.
    pub async fn get(&self, tab_id: &str) -> Option<TabState> {
        self.tabs.read().await.get(tab_id).cloned()
    }

    /// List all tabs.
    pub async fn list(&self) -> Vec<TabState> {
        self.tabs.read().await.values().cloned().collect()
    }

    /// Count open tabs.
    pub async fn count(&self) -> usize {
        self.tabs.read().await.len()
    }

    /// Resolve a tab_id or return the active tab.
    pub async fn resolve(&self, tab_id: Option<&str>) -> Option<TabId> {
        match tab_id {
            Some(id) => {
                if self.tabs.read().await.contains_key(id) {
                    Some(id.to_owned())
                } else {
                    None
                }
            }
            None => self.active_tab.read().await.clone(),
        }
    }

    /// Update heartbeat timestamp.
    pub async fn heartbeat(&self, tab_count: u16) {
        *self.last_heartbeat.write().await = Some(std::time::Instant::now());
        tracing::debug!("[TabRegistry] Heartbeat: {tab_count} tabs");
    }

    /// Check if extension is alive (heartbeat within 30s).
    pub async fn is_alive(&self) -> bool {
        match *self.last_heartbeat.read().await {
            Some(t) => t.elapsed().as_secs() < 30,
            None => false,
        }
    }
}

impl Default for TabRegistry {
    fn default() -> Self {
        Self::new()
    }
}
