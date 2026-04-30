//! Agent lifecycle management.
//! Agents represent AI personas with their own folder, tools, and permissions.

use std::fs;
use std::sync::Mutex;

use anyhow::Result;

use crate::config::Config;
use crate::db::Db;
use crate::types::Agent;
use crate::gateway::group_manager::ensure_agent_dirs;

pub struct AgentManager {
    on_changed: Mutex<Option<Box<dyn Fn() + Send + 'static>>>,
}

impl AgentManager {
    pub fn new() -> Self {
        Self {
            on_changed: Mutex::new(None),
        }
    }

    pub fn set_on_changed(&self, cb: Box<dyn Fn() + Send + 'static>) {
        if let Ok(mut guard) = self.on_changed.lock() {
            *guard = Some(cb);
        }
    }

    fn fire_changed(&self) {
        if let Ok(guard) = self.on_changed.lock() {
            if let Some(ref cb) = *guard {
                cb();
            }
        }
    }

    pub fn create(
        &self,
        db: &Db,
        config: &Config,
        folder: &str,
        name: &str,
        requires_trigger: bool,
        allowed_tools: Option<&Vec<String>>,
        allowed_work_dirs: Option<&Vec<String>>,
        now: &str,
    ) -> Result<Agent> {
        // Ensure agent directory + SOUL.md + MEMORY.md + workspace
        ensure_agent_dirs(config, folder, name);

        let id = db.insert_agent(folder, name, requires_trigger, allowed_tools, allowed_work_dirs, now)?;
        self.fire_changed();
        db.get_agent(id)?.ok_or_else(|| anyhow::anyhow!("Agent {id} not found after insert"))
    }

    pub fn get(&self, db: &Db, id: i64) -> Result<Option<Agent>> {
        db.get_agent(id)
    }

    pub fn get_by_folder(&self, db: &Db, folder: &str) -> Result<Option<Agent>> {
        db.get_agent_by_folder(folder)
    }

    pub fn list(&self, db: &Db) -> Result<Vec<Agent>> {
        db.list_agents()
    }

    pub fn delete(&self, db: &Db, id: i64) -> Result<()> {
        // Note: does NOT delete agent directory on disk (data safety).
        db.delete_agent(id)?;
        self.fire_changed();
        Ok(())
    }

    pub fn update(
        &self,
        db: &Db,
        id: i64,
        name: Option<&str>,
        requires_trigger: Option<bool>,
        allowed_tools: Option<&Vec<String>>,
        allowed_work_dirs: Option<&Vec<String>>,
        now: &str,
    ) -> Result<()> {
        db.update_agent(id, name, requires_trigger, allowed_tools, allowed_work_dirs, now)?;
        self.fire_changed();
        Ok(())
    }
}

impl Default for AgentManager {
    fn default() -> Self {
        Self::new()
    }
}
