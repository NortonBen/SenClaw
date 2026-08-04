//! Shared application state.

use crate::config;
use crate::dashws::DashHub;
use crate::db::Db;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// A YouTube (or other URL) download in progress.
///
/// Kept in memory only: a download that a restart interrupts is abandoned, and
/// there is nothing worth resuming — the user simply pastes the link again.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportState {
    pub id: i64,
    /// probing | downloading | completed | failed
    pub status: String,
    pub message: String,
    pub url: String,
    pub title: String,
    pub project_id: Option<i64>,
    pub updated_at: String,
}

pub struct Core {
    pub db: Db,
    pub dash: DashHub,
    /// Projects with an analysis currently running.
    ///
    /// Segments must be appended in order, so a project may only have one run
    /// in flight — a second concurrent call would interleave scenes.
    busy: Mutex<HashSet<i64>>,
    imports: Mutex<HashMap<i64, ImportState>>,
    import_seq: AtomicI64,
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
            imports: Mutex::new(HashMap::new()),
            import_seq: AtomicI64::new(1),
        }))
    }

    // ---- URL imports (YouTube etc.) ----

    /// Register a new import and return its id.
    pub fn start_import(&self, url: &str) -> ImportState {
        let id = self.import_seq.fetch_add(1, Ordering::SeqCst);
        let st = ImportState {
            id,
            status: "probing".into(),
            message: "đang lấy thông tin video".into(),
            url: url.to_string(),
            title: String::new(),
            project_id: None,
            updated_at: crate::db::now(),
        };
        self.imports.lock().unwrap().insert(id, st.clone());
        st
    }

    pub fn update_import(
        &self,
        id: i64,
        status: &str,
        message: &str,
        title: Option<&str>,
        project_id: Option<i64>,
    ) {
        let mut map = self.imports.lock().unwrap();
        if let Some(st) = map.get_mut(&id) {
            st.status = status.to_string();
            st.message = message.to_string();
            if let Some(t) = title {
                st.title = t.to_string();
            }
            if project_id.is_some() {
                st.project_id = project_id;
            }
            st.updated_at = crate::db::now();
            self.dash.emit(
                "youtube:progress",
                serde_json::to_value(&*st).unwrap_or_default(),
            );
        }
        // Keep the map from growing without bound over a long-lived process.
        if map.len() > 64 {
            let mut done: Vec<i64> = map
                .values()
                .filter(|s| s.status == "completed" || s.status == "failed")
                .map(|s| s.id)
                .collect();
            done.sort_unstable();
            for old in done.iter().take(done.len().saturating_sub(16)) {
                map.remove(old);
            }
        }
    }

    pub fn get_import(&self, id: i64) -> Option<ImportState> {
        self.imports.lock().unwrap().get(&id).cloned()
    }

    /// Build a core around an already-open database, skipping the data-directory
    /// setup `boot` does. Used by tests and by nothing else.
    #[cfg(test)]
    pub fn for_test(db: Db, dash: DashHub) -> Arc<Core> {
        Arc::new(Core {
            db,
            dash,
            busy: Mutex::new(HashSet::new()),
            imports: Mutex::new(HashMap::new()),
            import_seq: AtomicI64::new(1),
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
            imports: Mutex::new(HashMap::new()),
            import_seq: AtomicI64::new(1),
        })
    }

    #[test]
    fn an_import_moves_through_its_states() {
        let c = core();
        let st = c.start_import("https://youtu.be/x");
        assert_eq!(st.status, "probing");
        assert_eq!(c.get_import(st.id).unwrap().url, "https://youtu.be/x");

        c.update_import(st.id, "completed", "xong", Some("Tiêu đề"), Some(7));
        let done = c.get_import(st.id).unwrap();
        assert_eq!(done.status, "completed");
        assert_eq!(done.title, "Tiêu đề");
        assert_eq!(done.project_id, Some(7));
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
