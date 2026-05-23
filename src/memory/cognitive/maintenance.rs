//! Periodic maintenance sweep — keeps the cognitive graph tidy.
//!
//! Runs [`run_maintenance`] every `cfg.interval` hours. Each pass:
//!   1. `cleanup_junk` — drop envelope-wrapped chunks + orphan entities.
//!   2. `merge_duplicate_entities` — collapse entities sharing a normalised
//!      name onto a canonical survivor and re-point their edges.
//!
//! Cheap on small graphs (full-table scans of a few thousand rows finish
//! sub-ms). On large graphs the cadence dominates — default 24 h.
//!
//! Disabled when `cfg.interval_hours == 0`; the user can still trigger a
//! sweep manually via `POST /api/cognitive/maintenance`.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::task::JoinHandle;

use super::graph_store::{CleanupReport, GraphStore, MergeReport};

#[derive(Debug, Clone)]
pub struct MaintenanceConfig {
    /// Cadence between sweeps. `Duration::ZERO` disables the loop.
    pub interval: Duration,
}

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct MaintenanceReport {
    pub cleanup: CleanupReport,
    pub merge: MergeReport,
    pub duration_ms: i64,
}

pub fn run_maintenance(graph: &dyn GraphStore) -> Result<MaintenanceReport> {
    let started = std::time::Instant::now();
    let cleanup = graph.cleanup_junk()?;
    let merge = graph.merge_duplicate_entities()?;
    Ok(MaintenanceReport {
        cleanup,
        merge,
        duration_ms: started.elapsed().as_millis() as i64,
    })
}

/// Spawn a background loop that runs [`run_maintenance`] every
/// `cfg.interval`. Returns `None` when the cadence is zero (caller treats
/// this as "disabled").
pub fn start_maintenance_ticker(
    graph: Arc<dyn GraphStore>,
    cfg: MaintenanceConfig,
) -> Option<JoinHandle<()>> {
    if cfg.interval.is_zero() {
        tracing::info!("[cognitive] maintenance ticker disabled (interval=0)");
        return None;
    }
    tracing::info!(
        interval_hours = cfg.interval.as_secs() / 3600,
        "[cognitive] maintenance ticker started"
    );
    Some(tokio::spawn(async move {
        let mut ticker = tokio::time::interval(cfg.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip the immediate tick so daemon boot stays snappy.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let graph_ref = Arc::clone(&graph);
            let res = tokio::task::spawn_blocking(move || run_maintenance(&*graph_ref)).await;
            match res {
                Ok(Ok(rep)) => tracing::info!(
                    envelope_chunks = rep.cleanup.envelope_chunks_removed,
                    orphans = rep.cleanup.orphan_entities_removed,
                    groups_merged = rep.merge.groups_merged,
                    entities_merged = rep.merge.entities_merged,
                    duration_ms = rep.duration_ms,
                    "[cognitive] maintenance sweep complete"
                ),
                Ok(Err(e)) => tracing::error!(error = %e, "[cognitive] maintenance sweep failed"),
                Err(e) => {
                    tracing::error!(error = %e, "[cognitive] maintenance sweep task panicked")
                }
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Db;
    use crate::memory::cognitive::data_point::DataPoint;
    use crate::memory::cognitive::graph_store::SqliteGraphStore;
    use crate::memory::cognitive::triplet::RelationshipEdge;
    use chrono::Utc;
    use std::sync::Arc;

    fn store() -> (Arc<Db>, Arc<SqliteGraphStore>) {
        let cfg = Config::from_env();
        let db = Arc::new(Db::open_in_memory(&cfg).unwrap());
        let g = Arc::new(SqliteGraphStore::new(Arc::clone(&db)));
        (db, g)
    }

    #[test]
    fn maintenance_merges_duplicate_entities_and_redirects_edges() {
        let (_db, g) = store();
        let now = Utc::now().timestamp();

        // Two entity nodes with the same name (case + whitespace variant).
        let mut a = DataPoint::entity("Acme", now - 1000);
        a.mention_count = 3;
        let mut b = DataPoint::entity("  acme ", now);
        b.mention_count = 1;
        let c = DataPoint::entity("Bob", now);
        let d = DataPoint::entity("Carol", now);
        g.upsert_node(&a).unwrap();
        g.upsert_node(&b).unwrap();
        g.upsert_node(&c).unwrap();
        g.upsert_node(&d).unwrap();

        // Edges: carol -> a (canonical), bob -> b (duplicate). After merge
        // both must point at a; the bob->b edge is the one being redirected.
        // Without an edge on `a` cleanup_junk (which runs before merge in
        // run_maintenance) would prune it as an orphan first.
        g.upsert_edge(&RelationshipEdge::new(d.id, a.id, "founded", now))
            .unwrap();
        g.upsert_edge(&RelationshipEdge::new(c.id, b.id, "works_at", now))
            .unwrap();

        let rep = run_maintenance(&*g).unwrap();
        assert_eq!(rep.merge.groups_merged, 1);
        assert_eq!(rep.merge.entities_merged, 1);
        assert_eq!(rep.merge.edges_redirected, 1);

        // b is gone; the edge from c now points at a.
        let nbrs = g.neighbors(c.id, 10).unwrap();
        assert_eq!(nbrs.len(), 1);
        // Edge previously pointed at b; after merge it points at a.
        assert_eq!(nbrs[0].dst, a.id);
        assert_eq!(nbrs[0].src, c.id);
    }

    #[test]
    fn maintenance_is_noop_when_no_duplicates() {
        let (_db, g) = store();
        let now = Utc::now().timestamp();
        let a = DataPoint::entity("Alice", now);
        let b = DataPoint::entity("Bob", now);
        g.upsert_node(&a).unwrap();
        g.upsert_node(&b).unwrap();
        let edge = RelationshipEdge::new(a.id, b.id, "knows", now);
        g.upsert_edge(&edge).unwrap();

        let rep = run_maintenance(&*g).unwrap();
        assert_eq!(rep.merge.groups_merged, 0);
        assert_eq!(rep.merge.entities_merged, 0);
        // The edge survives untouched.
        assert_eq!(g.count_edges().unwrap(), 1);
    }
}
