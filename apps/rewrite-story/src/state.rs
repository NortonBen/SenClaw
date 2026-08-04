//! Shared process state. `Core` is everything below the HTTP layer (DB, the
//! dashboard hub, the running-job registry); `AppState` is what axum handlers
//! receive.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;

use self::cancel::CancelToken;
use crate::config;
use crate::dashws::DashHub;
use crate::db::Db;

/// Minimal cancellation primitive — one flag per running job, flipped by the
/// cancel endpoint and polled by the worker between chunks. Avoids pulling in
/// `tokio-util` for a single type.
pub mod cancel {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[derive(Clone, Default)]
    pub struct CancelToken(Arc<AtomicBool>);

    impl CancelToken {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn cancel(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
        pub fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }
}

pub struct Core {
    pub db: Db,
    pub dash: DashHub,
    /// Cancellation handles for jobs currently running in this process.
    jobs: Mutex<HashMap<i64, CancelToken>>,
}

impl Core {
    pub fn boot() -> Result<Arc<Core>> {
        let data_dir = config::data_dir();
        std::fs::create_dir_all(&data_dir).ok();
        let db = Db::open(&config::db_path())?;

        // Seed the LLM profile for the SenClaw bridge from persisted settings.
        crate::llm::set_profile(&db.setting("llm_profile", ""));

        Ok(Arc::new(Core {
            db,
            dash: DashHub::new(),
            jobs: Mutex::new(HashMap::new()),
        }))
    }

    #[cfg(test)]
    pub fn for_test(db: Db) -> Core {
        Core {
            db,
            dash: DashHub::new(),
            jobs: Mutex::new(HashMap::new()),
        }
    }

    /// Reserve the slot for `process_id`, returning `None` if a task for it is
    /// already live here.
    ///
    /// This must be taken *before* claiming the row in the DB. A cancelled job
    /// can sit for minutes inside an in-flight model call before it notices, and
    /// a retry during that window would otherwise let the poller start a second
    /// task for the same process — two workers writing the same chunk indices,
    /// with the dying one free to stamp its terminal status over the fresh run.
    pub fn register_job(&self, process_id: i64) -> Option<CancelToken> {
        let mut jobs = self.jobs.lock().unwrap();
        if jobs.contains_key(&process_id) {
            return None;
        }
        let token = CancelToken::new();
        jobs.insert(process_id, token.clone());
        Some(token)
    }

    pub fn finish_job(&self, process_id: i64) {
        self.jobs.lock().unwrap().remove(&process_id);
    }

    /// RAII handle that frees the slot even if the worker panics.
    ///
    /// Releasing the slot with a plain call after the job function returns is not
    /// enough: a panic unwinds past it, leaving the process permanently
    /// registered. `register_job` would then refuse forever, so the row sits in
    /// `queued` that no poller will ever pick up, while still counting against
    /// the concurrency limit.
    pub fn job_guard(self: &Arc<Self>, process_id: i64) -> Option<JobGuard> {
        let token = self.register_job(process_id)?;
        Some(JobGuard {
            core: self.clone(),
            process_id,
            token,
        })
    }

    /// Signal a running job to stop. Returns false if it isn't running here.
    pub fn cancel_job(&self, process_id: i64) -> bool {
        match self.jobs.lock().unwrap().get(&process_id) {
            Some(t) => {
                t.cancel();
                true
            }
            None => false,
        }
    }

    pub fn running_count(&self) -> usize {
        self.jobs.lock().unwrap().len()
    }
}

/// Holds a job slot for as long as the worker runs, releasing it on drop.
pub struct JobGuard {
    core: Arc<Core>,
    process_id: i64,
    token: CancelToken,
}

impl JobGuard {
    pub fn token(&self) -> &CancelToken {
        &self.token
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        self.core.finish_job(self.process_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression: a panicking worker must not strand its slot. Without the RAII
    /// guard the process would be unrunnable for the lifetime of the app.
    #[test]
    fn a_panicking_worker_still_releases_its_slot() {
        let core = Arc::new(Core::for_test(Db::open_memory().unwrap()));

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe({
            let core = core.clone();
            move || {
                let _guard = core.job_guard(42).expect("slot is free");
                panic!("worker blew up mid-chunk");
            }
        }));

        assert!(result.is_err(), "the panic should have propagated");
        assert_eq!(core.running_count(), 0, "slot must have been released");
        assert!(
            core.job_guard(42).is_some(),
            "process must be runnable again"
        );
    }

    /// Regression: the poller must not be able to start a second task for a
    /// process whose previous task is still winding down.
    #[test]
    fn a_process_cannot_be_registered_twice() {
        let core = Core::for_test(Db::open_memory().unwrap());

        let first = core.register_job(7);
        assert!(first.is_some());
        assert!(
            core.register_job(7).is_none(),
            "second registration must fail"
        );

        // Once the original task exits, the slot frees up.
        core.finish_job(7);
        assert!(core.register_job(7).is_some());
    }

    #[test]
    fn cancel_flips_only_the_registered_token() {
        let core = Core::for_test(Db::open_memory().unwrap());
        let token = core.register_job(1).unwrap();

        assert!(!token.is_cancelled());
        assert!(core.cancel_job(1));
        assert!(token.is_cancelled());

        // Nothing registered under 2.
        assert!(!core.cancel_job(2));
    }
}

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<Core>,
    /// MCP SSE fan-out channel.
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}
