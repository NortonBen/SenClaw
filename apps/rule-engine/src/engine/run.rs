//! Run bookkeeping.
//!
//! A run is one external event travelling through the graph. It ends when
//! nothing is in flight for it — including messages parked in a join barrier,
//! which is why a stuck barrier shows as `running` until its TTL fires.
//!
//! The Go engine instead counted nodes flagged `end: true` up front and
//! decremented per terminal message, so a fan-in drove the counter negative and
//! a chain with an `Infinity()` rule never finished at all.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::services::{EngineEvent, EventBus};
use super::types::*;
use crate::db::Db;
use crate::model::RunStatus;

pub struct RunState {
    pub id: RunId,
    pub chain_id: ChainId,
    pub trigger: String,
    pub started_ms: i64,
    pub debug: bool,
    in_flight: AtomicI64,
    hops: AtomicU64,
    seq: AtomicU64,
    /// Wall-clock of the last dequeue. The TTL reaper measures idleness from
    /// here, not from `started_ms`, so a legitimately long-running chain (many
    /// steps, slow HTTP calls) is not killed just for taking a while — only a
    /// truly stuck run (e.g. a join barrier that never fills) goes idle.
    last_activity_ms: AtomicI64,
    error: Mutex<Option<String>>,
    finished: AtomicI64,
}

impl RunState {
    pub fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::Relaxed) + 1
    }
    pub fn hops(&self) -> u64 {
        self.hops.load(Ordering::Relaxed)
    }
    /// Returns the new hop count. The caller fails the run past the budget.
    /// Every dequeue counts as activity, keeping the TTL reaper off a busy run.
    pub fn bump_hops(&self) -> u64 {
        self.last_activity_ms.store(now_ms(), Ordering::Relaxed);
        self.hops.fetch_add(1, Ordering::Relaxed) + 1
    }
    pub fn set_error(&self, msg: impl Into<String>) {
        let mut g = self.error.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_none() {
            *g = Some(msg.into());
        }
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
    pub fn in_flight(&self) -> i64 {
        self.in_flight.load(Ordering::SeqCst)
    }
}

pub struct RunTable {
    runs: Mutex<HashMap<RunId, Arc<RunState>>>,
    db: Arc<Db>,
    bus: EventBus,
    max_hops: u64,
}

impl RunTable {
    pub fn new(db: Arc<Db>, bus: EventBus, max_hops: u64) -> Self {
        Self {
            runs: Mutex::new(HashMap::new()),
            db,
            bus,
            max_hops,
        }
    }

    pub fn max_hops(&self) -> u64 {
        self.max_hops
    }

    pub fn start(&self, chain_id: ChainId, trigger: &str, debug: bool) -> Arc<RunState> {
        let id = next_id();
        let state = Arc::new(RunState {
            id,
            chain_id,
            trigger: trigger.to_string(),
            started_ms: now_ms(),
            debug,
            in_flight: AtomicI64::new(0),
            hops: AtomicU64::new(0),
            seq: AtomicU64::new(0),
            last_activity_ms: AtomicI64::new(now_ms()),
            error: Mutex::new(None),
            finished: AtomicI64::new(0),
        });
        self.runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, state.clone());
        let _ = self.db.insert_run(id as i64, chain_id, trigger);
        self.bus.publish(EngineEvent::RunStart {
            run_id: id,
            chain_id,
            node: trigger.to_string(),
        });
        state
    }

    pub fn get(&self, id: RunId) -> Option<Arc<RunState>> {
        self.runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&id)
            .cloned()
    }

    /// Count a message as in flight. Must be paired with `release`.
    pub fn retain(&self, run: &RunState, n: i64) {
        run.in_flight.fetch_add(n, Ordering::SeqCst);
    }

    /// Drop `n` in-flight messages; finishes the run when the count reaches 0.
    pub fn release(&self, run: &Arc<RunState>, n: i64) {
        let left = run.in_flight.fetch_sub(n, Ordering::SeqCst) - n;
        if left <= 0 {
            let status = if run.error().is_some() {
                RunStatus::Failed
            } else {
                RunStatus::Done
            };
            self.finish(run, status);
        }
    }

    pub fn finish(&self, run: &Arc<RunState>, status: RunStatus) {
        // Only the first finisher reports; a late release must not double-log.
        if run.finished.swap(1, Ordering::SeqCst) != 0 {
            return;
        }
        self.runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&run.id);
        let err = run.error();
        let _ = self
            .db
            .finish_run(run.id as i64, status, run.hops() as i64, err.as_deref());
        self.bus.publish(EngineEvent::RunEnd {
            run_id: run.id,
            chain_id: run.chain_id,
            status: status.as_str().to_string(),
            hops: run.hops(),
            error: err,
        });
    }

    /// Runs idle past the TTL — usually a barrier that never filled. Idleness is
    /// measured from the last dequeue, so an active run is never reclaimed.
    pub fn expired(&self, ttl_secs: i64) -> Vec<Arc<RunState>> {
        let cutoff = now_ms() - ttl_secs * 1000;
        self.runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|r| r.last_activity_ms.load(Ordering::Relaxed) < cutoff)
            .cloned()
            .collect()
    }

    pub fn active(&self) -> usize {
        self.runs.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// Runs of one chain that are still in flight.
    pub fn active_for(&self, chain_id: ChainId) -> usize {
        self.runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|r| r.chain_id == chain_id)
            .count()
    }

    /// Abandon every run of a chain (it was stopped or re-deployed).
    pub fn drop_chain(&self, chain_id: ChainId) {
        let victims: Vec<Arc<RunState>> = self
            .runs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .filter(|r| r.chain_id == chain_id)
            .cloned()
            .collect();
        for r in victims {
            r.set_error("chain đã dừng hoặc được nạp lại");
            self.finish(&r, RunStatus::Failed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> (Arc<Db>, RunTable) {
        let db = Arc::new(Db::open(":memory:").unwrap());
        db.create_chain(1, "c", "").unwrap();
        let t = RunTable::new(db.clone(), EventBus::new(), 100);
        (db, t)
    }

    #[test]
    fn run_finishes_when_the_last_message_is_released() {
        let (db, t) = table();
        let run = t.start(1, "src", false);
        t.retain(&run, 1);
        assert_eq!(t.active(), 1);
        t.release(&run, 1);
        assert_eq!(t.active(), 0);
        let rows = db.list_runs(Some(1), 10).unwrap();
        assert_eq!(rows[0].status, "done");
    }

    #[test]
    fn a_fan_out_keeps_the_run_open_until_every_branch_ends() {
        let (_db, t) = table();
        let run = t.start(1, "src", false);
        t.retain(&run, 1);
        // node emits to two targets: +2 then -1 for itself
        t.retain(&run, 2);
        t.release(&run, 1);
        assert_eq!(t.active(), 1, "two branches still running");
        t.release(&run, 1);
        assert_eq!(t.active(), 1);
        t.release(&run, 1);
        assert_eq!(t.active(), 0);
    }

    #[test]
    fn an_error_marks_the_run_failed_not_done() {
        let (db, t) = table();
        let run = t.start(1, "src", false);
        t.retain(&run, 1);
        run.set_error("boom");
        t.release(&run, 1);
        let rows = db.list_runs(Some(1), 10).unwrap();
        assert_eq!(rows[0].status, "failed");
        assert_eq!(rows[0].error.as_deref(), Some("boom"));
    }

    #[test]
    fn finish_is_idempotent() {
        let (db, t) = table();
        let run = t.start(1, "src", false);
        t.retain(&run, 1);
        t.release(&run, 1);
        t.finish(&run, RunStatus::Timeout);
        let rows = db.list_runs(Some(1), 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, "done", "the first finisher wins");
    }

    #[test]
    fn seq_is_monotonic() {
        let (_db, t) = table();
        let run = t.start(1, "s", false);
        assert_eq!(run.next_seq(), 1);
        assert_eq!(run.next_seq(), 2);
    }
}
