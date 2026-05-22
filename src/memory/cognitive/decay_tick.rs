//! Periodic decay sweep — the "make this layer alive" piece.
//!
//! Ported from shodh-memory: every N seconds, walk all edges, apply decay,
//! prune the dead ones, advance LTP states for the survivors, and record a
//! summary row in `cog_decay_log`.
//!
//! ## Default cadence
//!
//! 300s (5 min). At that interval the cache stays warm for callers and the
//! per-edge IO cost is amortised. Override via [`DecayConfig::interval`].
//!
//! ## Boot wiring
//!
//! The daemon spawns one ticker per `GraphStore` instance:
//!
//! ```ignore
//! let handle = start_decay_ticker(graph.clone(), DecayConfig::default());
//! ```
//!
//! Dropping `handle` (or `abort()`) stops the loop. No graceful shutdown is
//! needed — decay is idempotent, a half-finished sweep just resumes next tick.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Utc;
use tokio::task::JoinHandle;

use super::graph_store::GraphStore;

const BATCH_SIZE: usize = 256;

#[derive(Debug, Clone)]
pub struct DecayConfig {
    pub interval: Duration,
    /// Cap on edges processed per tick. `0` = unlimited (whole table).
    pub max_edges_per_tick: usize,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(300),
            max_edges_per_tick: 0,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct DecayReport {
    pub edges_scanned: usize,
    pub edges_pruned: usize,
    pub edges_promoted: usize,
    pub duration_ms: i64,
}

/// Run one decay sweep over the graph. Returns a report; also persists into
/// `cog_decay_log`.
pub fn run_decay(graph: &dyn GraphStore, cfg: &DecayConfig) -> Result<DecayReport> {
    let started = std::time::Instant::now();
    let now = Utc::now().timestamp();
    let total = graph.count_edges()?;
    let cap = if cfg.max_edges_per_tick == 0 {
        total
    } else {
        cfg.max_edges_per_tick.min(total)
    };

    let mut scanned = 0usize;
    let mut pruned = 0usize;
    let mut promoted = 0usize;
    let mut offset = 0usize;

    while scanned < cap {
        let batch_size = BATCH_SIZE.min(cap - scanned);
        let batch = graph.scan_edges(batch_size, offset)?;
        if batch.is_empty() {
            break;
        }
        // Stable offset advance: only count *survivors* toward offset so
        // pruning during the sweep doesn't make us skip rows. Track moves
        // separately.
        let mut survivors_this_batch = 0usize;
        for mut edge in batch {
            scanned += 1;
            let prev_tier = edge.tier;
            let should_prune = edge.decay(now);
            if should_prune {
                graph.delete_edge(edge.src, edge.dst, &edge.predicate)?;
                pruned += 1;
            } else {
                if edge.tier != prev_tier {
                    promoted += 1;
                }
                graph.upsert_edge(&edge)?;
                survivors_this_batch += 1;
            }
        }
        offset += survivors_this_batch;
        // If a batch returns nothing new (all pruned), we still advance via
        // the loop condition on `scanned`.
        if survivors_this_batch == 0 {
            // Edges were pruned — next scan_edges with the same offset will
            // return the next "page" because pruned rows are gone.
        }
    }

    let duration_ms = started.elapsed().as_millis() as i64;
    graph.record_decay_run(now, scanned, pruned, promoted, duration_ms)?;

    Ok(DecayReport {
        edges_scanned: scanned,
        edges_pruned: pruned,
        edges_promoted: promoted,
        duration_ms,
    })
}

/// Spawn a background loop running [`run_decay`] every `cfg.interval`.
/// Drop the handle (or call `abort()`) to stop.
pub fn start_decay_ticker(graph: Arc<dyn GraphStore>, cfg: DecayConfig) -> JoinHandle<()> {
    tracing::info!(
        interval_sec = cfg.interval.as_secs(),
        "[cognitive] decay ticker started"
    );
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(cfg.interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip the immediate first tick so daemon boot stays snappy.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let graph_ref = Arc::clone(&graph);
            let cfg_ref = cfg.clone();
            let res = tokio::task::spawn_blocking(move || run_decay(&*graph_ref, &cfg_ref))
                .await;
            match res {
                Ok(Ok(rep)) => tracing::debug!(
                    scanned = rep.edges_scanned,
                    pruned = rep.edges_pruned,
                    promoted = rep.edges_promoted,
                    duration_ms = rep.duration_ms,
                    "[cognitive] decay sweep complete"
                ),
                Ok(Err(e)) => tracing::error!(error = %e, "[cognitive] decay sweep failed"),
                Err(e) => tracing::error!(error = %e, "[cognitive] decay sweep task panicked"),
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Db;
    use crate::memory::cognitive::data_point::DataPoint;
    use crate::memory::cognitive::graph_store::SqliteGraphStore;
    use crate::memory::cognitive::ltp::LtpStatus;
    use crate::memory::cognitive::triplet::RelationshipEdge;
    use std::sync::Arc;

    fn store() -> (Arc<Db>, Arc<SqliteGraphStore>) {
        let cfg = Config::from_env();
        let db = Arc::new(Db::open_in_memory(&cfg).unwrap());
        let g = Arc::new(SqliteGraphStore::new(Arc::clone(&db)));
        (db, g)
    }

    #[test]
    fn weak_stale_edges_get_pruned() {
        let (_db, g) = store();
        let now = Utc::now().timestamp();
        let stale = now - 10 * 86_400; // 10 days ago (L1 prune-eligible)

        let a = DataPoint::entity("A", stale);
        let b = DataPoint::entity("B", stale);
        g.upsert_node(&a).unwrap();
        g.upsert_node(&b).unwrap();

        let mut edge = RelationshipEdge::new(a.id, b.id, "rel", stale);
        edge.strength = 0.04; // below L1 prune threshold (0.05) after any decay
        edge.last_activated = stale;
        g.upsert_edge(&edge).unwrap();

        let report = run_decay(&*g, &DecayConfig::default()).unwrap();
        assert_eq!(report.edges_scanned, 1);
        assert_eq!(report.edges_pruned, 1);
        assert_eq!(g.count_edges().unwrap(), 0);
    }

    #[test]
    fn full_ltp_edges_survive_sweep() {
        let (_db, g) = store();
        let now = Utc::now().timestamp();
        // L1 max age is 1 day. We want the *age* check to want to prune,
        // but Full LTP to override it. Strength stays well above zombie
        // floor so LTP doesn't get auto-stripped.
        let stale = now - 25 * 3_600; // 25 hours ago

        let a = DataPoint::entity("A", stale);
        let b = DataPoint::entity("B", stale);
        g.upsert_node(&a).unwrap();
        g.upsert_node(&b).unwrap();

        let mut edge = RelationshipEdge::new(a.id, b.id, "rel", stale);
        edge.strength = 0.5;
        edge.ltp_status = LtpStatus::Full;
        edge.last_activated = stale;
        g.upsert_edge(&edge).unwrap();

        let report = run_decay(&*g, &DecayConfig::default()).unwrap();
        assert_eq!(report.edges_pruned, 0, "Full LTP edges must survive past max_age");
        assert_eq!(g.count_edges().unwrap(), 1);
    }

    #[test]
    fn decay_log_row_is_written() {
        let (db, g) = store();
        let report = run_decay(&*g, &DecayConfig::default()).unwrap();
        assert_eq!(report.edges_scanned, 0);

        // cog_decay_log lives on the cognitive connection (DB split — see
        // db/mod.rs::cog_conn). Probe the right handle.
        let count: i64 = db
            .with_cog_conn(|conn| {
                conn.query_row("SELECT COUNT(*) FROM cog_decay_log", [], |r| r.get(0))
                    .map_err(Into::into)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
}
