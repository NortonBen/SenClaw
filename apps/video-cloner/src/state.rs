//! Shared application state.

use crate::config;
use crate::dashws::DashHub;
use crate::db::Db;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

pub struct Core {
    pub db: Db,
    pub dash: DashHub,
    /// Projects with an analysis currently running.
    ///
    /// Segments must be appended in order, so a project may only have one run
    /// in flight — a second concurrent call would interleave scenes.
    busy: Mutex<HashSet<i64>>,
}

impl Core {
    pub fn boot() -> Result<Arc<Core>> {
        let dir = config::data_dir();
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("tạo thư mục dữ liệu {}", dir.display()))?;
        std::fs::create_dir_all(config::media_dir()).context("tạo thư mục media")?;

        let db = Db::open(&config::db_path()).context("mở SQLite")?;

        Ok(Arc::new(Core {
            db,
            dash: DashHub::new(),
            busy: Mutex::new(HashSet::new()),
        }))
    }

    /// Build a core around an already-open database, skipping the data-directory
    /// setup `boot` does. Used by tests and by nothing else.
    #[cfg(test)]
    pub fn for_test(db: Db, dash: DashHub) -> Arc<Core> {
        Arc::new(Core {
            db,
            dash,
            busy: Mutex::new(HashSet::new()),
        })
    }

    /// Claim the analysis slot for a project. `None` means one is already running.
    pub fn try_claim(self: &Arc<Self>, project_id: i64) -> Option<BusyGuard> {
        let mut busy = self.busy.lock().unwrap();
        if !busy.insert(project_id) {
            return None;
        }
        Some(BusyGuard {
            core: self.clone(),
            project_id,
        })
    }

    pub fn is_busy(&self, project_id: i64) -> bool {
        self.busy.lock().unwrap().contains(&project_id)
    }

    fn release(&self, project_id: i64) {
        self.busy.lock().unwrap().remove(&project_id);
    }
}

/// Releases the project's slot even if the worker task panics.
pub struct BusyGuard {
    core: Arc<Core>,
    project_id: i64,
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.core.release(self.project_id);
    }
}

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<Core>,
    pub mcp_tx: broadcast::Sender<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core() -> Arc<Core> {
        Arc::new(Core {
            db: Db::open_memory().unwrap(),
            dash: DashHub::new(),
            busy: Mutex::new(HashSet::new()),
        })
    }

    #[test]
    fn a_project_can_only_have_one_run_in_flight() {
        let c = core();
        let g = c.try_claim(1).unwrap();
        assert!(c.try_claim(1).is_none());
        assert!(c.try_claim(2).is_some(), "other projects are unaffected");
        drop(g);
        assert!(c.try_claim(1).is_some());
    }

    #[test]
    fn the_slot_is_released_when_the_guard_is_dropped() {
        let c = core();
        {
            let _g = c.try_claim(7).unwrap();
            assert!(c.is_busy(7));
        }
        assert!(!c.is_busy(7));
    }
}
