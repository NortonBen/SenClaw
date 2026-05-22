//! GraphStore trait + SQLite implementation.
//!
//! Backend default is SQLite (no extra deps). The trait keeps the door open
//! for optional Kuzu (feature `cognitive-kuzu`) later — see plan P5.

use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::Db;

use super::data_point::{DataPoint, NodeKind};
use super::ltp::LtpStatus;
use super::node_set::NodeSet;
use super::tiers::EdgeTier;
use super::triplet::RelationshipEdge;

/// Storage backend for the cognitive graph. Each method is sync because the
/// senclaw `Db` holds a `Mutex<Connection>` — callers wrap in `spawn_blocking`
/// when serving async contexts.
pub trait GraphStore: Send + Sync {
    fn upsert_node(&self, node: &DataPoint) -> Result<()>;
    /// Update the extraction-state machine on a chunk node. Called by
    /// the cognify pipeline after each LLM attempt. No-op on missing id.
    fn set_extraction_state(
        &self,
        id: Uuid,
        state: crate::memory::cognitive::ExtractionState,
        at: i64,
    ) -> Result<()>;
    fn get_node(&self, id: Uuid) -> Result<Option<DataPoint>>;
    fn find_node_by_content_hash(&self, hash: &str) -> Result<Option<DataPoint>>;
    fn find_entity_by_name(&self, name: &str) -> Result<Option<DataPoint>>;
    fn delete_node(&self, id: Uuid) -> Result<()>;

    fn upsert_edge(&self, edge: &RelationshipEdge) -> Result<()>;
    fn delete_edge(&self, src: Uuid, dst: Uuid, predicate: &str) -> Result<()>;
    fn neighbors(&self, node: Uuid, max: usize) -> Result<Vec<RelationshipEdge>>;

    /// Pull a batch of edges ordered by `last_activated ASC` (stalest first).
    /// `offset` lets the decay tick page through the whole table in chunks
    /// without holding everything in memory.
    fn scan_edges(&self, limit: usize, offset: usize) -> Result<Vec<RelationshipEdge>>;
    fn count_edges(&self) -> Result<usize>;
    /// Write the result of a decay sweep into `cog_decay_log`.
    fn record_decay_run(
        &self,
        run_at: i64,
        edges_scanned: usize,
        edges_pruned: usize,
        edges_promoted: usize,
        duration_ms: i64,
    ) -> Result<()>;

    fn tag_node(&self, node: Uuid, set: &NodeSet) -> Result<()>;
    fn nodes_in_set(&self, set: &NodeSet, limit: usize) -> Result<Vec<DataPoint>>;

    /// Paginated node listing for the Web UI. `kind=None` returns all kinds.
    fn list_nodes(
        &self,
        kind: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<DataPoint>>;
    fn count_nodes(&self, kind: Option<&str>) -> Result<usize>;
    fn recent_decay_runs(&self, limit: usize) -> Result<Vec<DecayLogRow>>;

    /// Return the top-`limit` nodes ordered by incident-edge count
    /// descending. Used by the Graph Explorer to surface "interesting"
    /// nodes — high-degree entities are usually the natural seeds for
    /// browsing a knowledge graph.
    fn top_nodes_by_degree(&self, limit: usize) -> Result<Vec<NodeWithDegree>>;

    /// Return edges that originate from a node inside the given NodeSet.
    /// Used by the persona-consolidate path to find "what the agent has
    /// learned about itself" and pour it back into SOUL.md.
    ///
    /// `min_strength` filters out weak/decaying edges; `require_ltp` keeps
    /// only edges that have hit any LTP state (Burst/Weekly/Full) so the
    /// resulting facts are ones the graph considers durable.
    fn edges_from_set(
        &self,
        set: &NodeSet,
        min_strength: f32,
        require_ltp: bool,
        limit: usize,
    ) -> Result<Vec<(RelationshipEdge, DataPoint, DataPoint)>>;
}

/// `DataPoint` paired with its incident-edge count. Returned by
/// [`GraphStore::top_nodes_by_degree`] for the Graph Explorer UI.
#[derive(Debug, Clone)]
pub struct NodeWithDegree {
    pub node: DataPoint,
    pub degree: usize,
}

/// Row shape returned by [`GraphStore::recent_decay_runs`].
#[derive(Debug, Clone, serde::Serialize)]
pub struct DecayLogRow {
    pub run_at: i64,
    pub edges_scanned: usize,
    pub edges_pruned: usize,
    pub edges_promoted: usize,
    pub duration_ms: i64,
}

pub struct SqliteGraphStore {
    db: Arc<Db>,
}

impl SqliteGraphStore {
    pub fn new(db: Arc<Db>) -> Self {
        Self { db }
    }
}

// ===== helpers =====

fn uuid_bytes(u: Uuid) -> [u8; 16] {
    *u.as_bytes()
}

fn bytes_uuid(b: Vec<u8>) -> Result<Uuid> {
    let arr: [u8; 16] = b
        .as_slice()
        .try_into()
        .context("uuid blob must be 16 bytes")?;
    Ok(Uuid::from_bytes(arr))
}

fn row_to_node(row: &rusqlite::Row<'_>) -> rusqlite::Result<DataPoint> {
    let id_bytes: Vec<u8> = row.get("id")?;
    let id = bytes_uuid(id_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, e.into())
    })?;
    let kind: String = row.get("kind")?;
    let props_str: String = row.get("props_json")?;
    let props: Value = serde_json::from_str(&props_str).unwrap_or(Value::Null);
    Ok(DataPoint {
        id,
        kind: NodeKind::from_str(&kind),
        type_name: row.get("type_name")?,
        name: row.get("name")?,
        summary: row.get("summary")?,
        content_hash: row.get::<_, Option<String>>("content_hash")?,
        props,
        salience: row.get::<_, f64>("salience")? as f32,
        mention_count: row.get::<_, i64>("mention_count")? as u32,
        is_proper_noun: row.get::<_, i64>("is_proper_noun")? != 0,
        selectivity: row.get::<_, Option<f64>>("selectivity")?.map(|v| v as f32),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_seen_at: row.get("last_seen_at")?,
        extraction_state: super::data_point::ExtractionState::from_i64(
            row.get::<_, i64>("extraction_state").unwrap_or(0),
        ),
        extracted_at: row.get::<_, Option<i64>>("extracted_at").unwrap_or(None),
    })
}

fn row_to_edge(row: &rusqlite::Row<'_>) -> rusqlite::Result<RelationshipEdge> {
    let to_uuid = |col: &str| -> rusqlite::Result<Uuid> {
        let b: Vec<u8> = row.get(col)?;
        bytes_uuid(b).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, e.into())
        })
    };
    let src = to_uuid("src")?;
    let dst = to_uuid("dst")?;
    let props_str: String = row.get("props_json")?;
    let props: Value = serde_json::from_str(&props_str).unwrap_or(Value::Null);
    let act_str: String = row.get("activation_timestamps")?;
    let activation_timestamps: Vec<i64> = serde_json::from_str(&act_str).unwrap_or_default();
    let src_ep: Option<Vec<u8>> = row.get("source_episode_id")?;
    let source_episode_id = src_ep
        .map(bytes_uuid)
        .transpose()
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, e.into()))?;
    Ok(RelationshipEdge {
        src,
        dst,
        predicate: row.get("predicate")?,
        props,
        valid_from: row.get("valid_from")?,
        valid_to: row.get("valid_to")?,
        strength: row.get::<_, f64>("strength")? as f32,
        tier: EdgeTier::from_u8(row.get::<_, i64>("tier")? as u8),
        activation_count: row.get::<_, i64>("activation_count")? as u32,
        last_activated: row.get("last_activated")?,
        ltp_status: LtpStatus::from_u8(row.get::<_, i64>("ltp_status")? as u8),
        ltp_detected_at: row.get("ltp_detected_at")?,
        entity_confidence: row.get::<_, Option<f64>>("entity_confidence")?.map(|v| v as f32),
        endpoint_selectivity: row.get::<_, Option<f64>>("endpoint_selectivity")?.map(|v| v as f32),
        forman_curvature: row.get::<_, Option<f64>>("forman_curvature")?.map(|v| v as f32),
        activation_timestamps,
        source_episode_id,
        context: row.get("context")?,
        created_at: row.get("created_at")?,
    })
}

// ===== impl =====

impl GraphStore for SqliteGraphStore {
    fn upsert_node(&self, node: &DataPoint) -> Result<()> {
        let id = uuid_bytes(node.id).to_vec();
        let props_json = serde_json::to_string(&node.props).unwrap_or_else(|_| "{}".into());
        self.db.with_cog_conn(|conn| {
            conn.execute(
                r#"INSERT INTO cog_nodes
                   (id, kind, type_name, name, summary, content_hash, props_json,
                    salience, mention_count, is_proper_noun, selectivity,
                    created_at, updated_at, last_seen_at,
                    extraction_state, extracted_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                   ON CONFLICT(id) DO UPDATE SET
                     summary          = excluded.summary,
                     props_json       = excluded.props_json,
                     salience         = excluded.salience,
                     mention_count    = excluded.mention_count,
                     selectivity      = excluded.selectivity,
                     updated_at       = excluded.updated_at,
                     last_seen_at     = excluded.last_seen_at,
                     -- Persist state advances on conflict, but never demote:
                     -- a `done` row stays `done` even if the caller passes
                     -- `pending` (e.g. building a DataPoint from a partial
                     -- in-memory copy without consulting the DB first).
                     extraction_state = MAX(extraction_state, excluded.extraction_state),
                     extracted_at     = COALESCE(excluded.extracted_at, extracted_at)"#,
                params![
                    id,
                    node.kind.as_str(),
                    node.type_name,
                    node.name,
                    node.summary,
                    node.content_hash,
                    props_json,
                    node.salience as f64,
                    node.mention_count as i64,
                    node.is_proper_noun as i64,
                    node.selectivity.map(|v| v as f64),
                    node.created_at,
                    node.updated_at,
                    node.last_seen_at,
                    node.extraction_state as i64,
                    node.extracted_at,
                ],
            )?;
            Ok(())
        })
    }

    fn set_extraction_state(
        &self,
        id: Uuid,
        state: crate::memory::cognitive::ExtractionState,
        at: i64,
    ) -> Result<()> {
        let id_blob = uuid_bytes(id).to_vec();
        self.db.with_cog_conn(|conn| {
            conn.execute(
                "UPDATE cog_nodes
                 SET extraction_state = ?1,
                     extracted_at     = ?2,
                     updated_at       = ?2
                 WHERE id = ?3",
                params![state as i64, at, id_blob],
            )?;
            Ok(())
        })
    }

    fn get_node(&self, id: Uuid) -> Result<Option<DataPoint>> {
        let id_blob = uuid_bytes(id).to_vec();
        self.db.with_cog_conn(|conn| {
            let row = conn
                .query_row(
                    "SELECT * FROM cog_nodes WHERE id = ?1",
                    params![id_blob],
                    row_to_node,
                )
                .optional()?;
            Ok(row)
        })
    }

    fn find_node_by_content_hash(&self, hash: &str) -> Result<Option<DataPoint>> {
        self.db.with_cog_conn(|conn| {
            let row = conn
                .query_row(
                    "SELECT * FROM cog_nodes WHERE content_hash = ?1 LIMIT 1",
                    params![hash],
                    row_to_node,
                )
                .optional()?;
            Ok(row)
        })
    }

    fn find_entity_by_name(&self, name: &str) -> Result<Option<DataPoint>> {
        self.db.with_cog_conn(|conn| {
            let row = conn
                .query_row(
                    "SELECT * FROM cog_nodes WHERE kind = 'entity' AND name = ?1 LIMIT 1",
                    params![name],
                    row_to_node,
                )
                .optional()?;
            Ok(row)
        })
    }

    fn delete_node(&self, id: Uuid) -> Result<()> {
        let id_blob = uuid_bytes(id).to_vec();
        self.db.with_cog_conn(|conn| {
            conn.execute("DELETE FROM cog_nodes WHERE id = ?1", params![id_blob])?;
            Ok(())
        })
    }

    fn upsert_edge(&self, edge: &RelationshipEdge) -> Result<()> {
        let src = uuid_bytes(edge.src).to_vec();
        let dst = uuid_bytes(edge.dst).to_vec();
        let props = serde_json::to_string(&edge.props).unwrap_or_else(|_| "{}".into());
        let acts = serde_json::to_string(&edge.activation_timestamps).unwrap_or_else(|_| "[]".into());
        let ep_id = edge.source_episode_id.map(|u| uuid_bytes(u).to_vec());
        self.db.with_cog_conn(|conn| {
            conn.execute(
                r#"INSERT INTO cog_edges
                   (src, dst, predicate, props_json, valid_from, valid_to,
                    strength, tier, activation_count, last_activated,
                    ltp_status, ltp_detected_at, entity_confidence,
                    endpoint_selectivity, forman_curvature, activation_timestamps,
                    source_episode_id, context, created_at)
                   VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)
                   ON CONFLICT(src, dst, predicate) DO UPDATE SET
                     props_json            = excluded.props_json,
                     valid_to              = excluded.valid_to,
                     strength              = excluded.strength,
                     tier                  = excluded.tier,
                     activation_count      = excluded.activation_count,
                     last_activated        = excluded.last_activated,
                     ltp_status            = excluded.ltp_status,
                     ltp_detected_at       = excluded.ltp_detected_at,
                     entity_confidence     = excluded.entity_confidence,
                     endpoint_selectivity  = excluded.endpoint_selectivity,
                     forman_curvature      = excluded.forman_curvature,
                     activation_timestamps = excluded.activation_timestamps,
                     context               = excluded.context"#,
                params![
                    src,
                    dst,
                    edge.predicate,
                    props,
                    edge.valid_from,
                    edge.valid_to,
                    edge.strength as f64,
                    edge.tier as u8 as i64,
                    edge.activation_count as i64,
                    edge.last_activated,
                    edge.ltp_status as u8 as i64,
                    edge.ltp_detected_at,
                    edge.entity_confidence.map(|v| v as f64),
                    edge.endpoint_selectivity.map(|v| v as f64),
                    edge.forman_curvature.map(|v| v as f64),
                    acts,
                    ep_id,
                    edge.context,
                    edge.created_at,
                ],
            )?;
            Ok(())
        })
    }

    fn delete_edge(&self, src: Uuid, dst: Uuid, predicate: &str) -> Result<()> {
        let src = uuid_bytes(src).to_vec();
        let dst = uuid_bytes(dst).to_vec();
        self.db.with_cog_conn(|conn| {
            conn.execute(
                "DELETE FROM cog_edges WHERE src = ?1 AND dst = ?2 AND predicate = ?3",
                params![src, dst, predicate],
            )?;
            Ok(())
        })
    }

    fn neighbors(&self, node: Uuid, max: usize) -> Result<Vec<RelationshipEdge>> {
        let id = uuid_bytes(node).to_vec();
        self.db.with_cog_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM cog_edges
                 WHERE src = ?1 OR dst = ?1
                 ORDER BY last_activated DESC
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![id, max as i64], row_to_edge)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    fn scan_edges(&self, limit: usize, offset: usize) -> Result<Vec<RelationshipEdge>> {
        self.db.with_cog_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM cog_edges
                 ORDER BY last_activated ASC
                 LIMIT ?1 OFFSET ?2",
            )?;
            let rows = stmt
                .query_map(params![limit as i64, offset as i64], row_to_edge)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    fn count_edges(&self) -> Result<usize> {
        self.db.with_cog_conn(|conn| {
            let n: i64 = conn.query_row("SELECT COUNT(*) FROM cog_edges", [], |r| r.get(0))?;
            Ok(n as usize)
        })
    }

    fn record_decay_run(
        &self,
        run_at: i64,
        edges_scanned: usize,
        edges_pruned: usize,
        edges_promoted: usize,
        duration_ms: i64,
    ) -> Result<()> {
        self.db.with_cog_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO cog_decay_log
                 (run_at, edges_scanned, edges_pruned, edges_promoted, duration_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    run_at,
                    edges_scanned as i64,
                    edges_pruned as i64,
                    edges_promoted as i64,
                    duration_ms,
                ],
            )?;
            Ok(())
        })
    }

    fn tag_node(&self, node: Uuid, set: &NodeSet) -> Result<()> {
        let node_blob = uuid_bytes(node).to_vec();
        let now = chrono::Utc::now().timestamp();
        self.db.with_cog_conn(|conn| {
            // upsert the node_set, get its id
            conn.execute(
                "INSERT OR IGNORE INTO cog_node_sets (scope_kind, scope_id, tag, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![set.scope_kind.as_str(), set.scope_id, set.tag, now],
            )?;
            let set_id: i64 = conn.query_row(
                "SELECT id FROM cog_node_sets
                 WHERE scope_kind = ?1 AND scope_id = ?2 AND tag = ?3",
                params![set.scope_kind.as_str(), set.scope_id, set.tag],
                |r| r.get(0),
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO cog_node_tags (node_id, node_set_id) VALUES (?1, ?2)",
                params![node_blob, set_id],
            )?;
            Ok(())
        })
    }

    fn list_nodes(
        &self,
        kind: Option<&str>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<DataPoint>> {
        self.db.with_cog_conn(|conn| {
            if let Some(k) = kind {
                let mut stmt = conn.prepare(
                    "SELECT * FROM cog_nodes WHERE kind = ?1
                     ORDER BY last_seen_at DESC LIMIT ?2 OFFSET ?3",
                )?;
                let rows: Vec<DataPoint> = stmt
                    .query_map(params![k, limit as i64, offset as i64], row_to_node)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            } else {
                let mut stmt = conn.prepare(
                    "SELECT * FROM cog_nodes
                     ORDER BY last_seen_at DESC LIMIT ?1 OFFSET ?2",
                )?;
                let rows: Vec<DataPoint> = stmt
                    .query_map(params![limit as i64, offset as i64], row_to_node)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            }
        })
    }

    fn count_nodes(&self, kind: Option<&str>) -> Result<usize> {
        self.db.with_cog_conn(|conn| {
            let n: i64 = if let Some(k) = kind {
                conn.query_row(
                    "SELECT COUNT(*) FROM cog_nodes WHERE kind = ?1",
                    params![k],
                    |r| r.get(0),
                )?
            } else {
                conn.query_row("SELECT COUNT(*) FROM cog_nodes", [], |r| r.get(0))?
            };
            Ok(n as usize)
        })
    }

    fn edges_from_set(
        &self,
        set: &NodeSet,
        min_strength: f32,
        require_ltp: bool,
        limit: usize,
    ) -> Result<Vec<(RelationshipEdge, DataPoint, DataPoint)>> {
        self.db.with_cog_conn(|conn| {
            // Edges whose `src` is tagged with this NodeSet. We skip the
            // MENTIONS provenance predicate — those tie chunks to entities
            // and aren't statements *about* the agent in a way SOUL.md
            // should consume.
            let mut stmt = conn.prepare(
                "SELECT e.* FROM cog_edges e
                 JOIN cog_node_tags t ON t.node_id = e.src
                 JOIN cog_node_sets s ON s.id = t.node_set_id
                 WHERE s.scope_kind = ?1 AND s.scope_id = ?2 AND s.tag = ?3
                   AND e.predicate <> 'MENTIONS'
                   AND e.strength >= ?4
                   AND (?5 = 0 OR e.ltp_status > 0)
                 ORDER BY e.strength DESC, e.activation_count DESC
                 LIMIT ?6",
            )?;
            let raw_rows: Vec<RelationshipEdge> = stmt
                .query_map(
                    params![
                        set.scope_kind.as_str(),
                        set.scope_id,
                        set.tag,
                        min_strength as f64,
                        require_ltp as i64,
                        limit as i64,
                    ],
                    row_to_edge,
                )?
                .collect::<rusqlite::Result<_>>()?;
            drop(stmt);
            // Resolve src + dst nodes for each edge so the caller can
            // format readable bullets without a second round-trip.
            let mut out = Vec::with_capacity(raw_rows.len());
            for edge in raw_rows {
                let src_blob = uuid_bytes(edge.src).to_vec();
                let dst_blob = uuid_bytes(edge.dst).to_vec();
                let src: Option<DataPoint> = conn
                    .query_row(
                        "SELECT * FROM cog_nodes WHERE id = ?1",
                        params![src_blob],
                        row_to_node,
                    )
                    .optional()?;
                let dst: Option<DataPoint> = conn
                    .query_row(
                        "SELECT * FROM cog_nodes WHERE id = ?1",
                        params![dst_blob],
                        row_to_node,
                    )
                    .optional()?;
                if let (Some(s), Some(d)) = (src, dst) {
                    out.push((edge, s, d));
                }
            }
            Ok(out)
        })
    }

    fn top_nodes_by_degree(&self, limit: usize) -> Result<Vec<NodeWithDegree>> {
        self.db.with_cog_conn(|conn| {
            // Degree = count of incident edges in cog_edges (src or dst).
            // We pre-aggregate per-node via UNION ALL so SQLite can use the
            // (src) and (dst) indexes; doing OR in the WHERE clause forces
            // a full scan and is slow on big graphs.
            let mut stmt = conn.prepare(
                "WITH deg AS (
                   SELECT src AS id, COUNT(*) AS c FROM cog_edges GROUP BY src
                   UNION ALL
                   SELECT dst AS id, COUNT(*) AS c FROM cog_edges GROUP BY dst
                 ), totals AS (
                   SELECT id, SUM(c) AS degree FROM deg GROUP BY id
                 )
                 SELECT n.*, COALESCE(t.degree, 0) AS degree
                 FROM cog_nodes n
                 LEFT JOIN totals t ON t.id = n.id
                 ORDER BY degree DESC, n.last_seen_at DESC
                 LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![limit as i64], |row| {
                    let node = row_to_node(row)?;
                    let degree: i64 = row.get("degree")?;
                    Ok(NodeWithDegree { node, degree: degree as usize })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    fn recent_decay_runs(&self, limit: usize) -> Result<Vec<DecayLogRow>> {
        self.db.with_cog_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT run_at, edges_scanned, edges_pruned, edges_promoted, duration_ms
                 FROM cog_decay_log
                 ORDER BY run_at DESC
                 LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![limit as i64], |row| {
                    Ok(DecayLogRow {
                        run_at: row.get(0)?,
                        edges_scanned: row.get::<_, i64>(1)? as usize,
                        edges_pruned: row.get::<_, i64>(2)? as usize,
                        edges_promoted: row.get::<_, i64>(3)? as usize,
                        duration_ms: row.get(4)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    fn nodes_in_set(&self, set: &NodeSet, limit: usize) -> Result<Vec<DataPoint>> {
        self.db.with_cog_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT n.* FROM cog_nodes n
                 JOIN cog_node_tags t ON t.node_id = n.id
                 JOIN cog_node_sets s ON s.id = t.node_set_id
                 WHERE s.scope_kind = ?1 AND s.scope_id = ?2 AND s.tag = ?3
                 ORDER BY n.last_seen_at DESC
                 LIMIT ?4",
            )?;
            let rows = stmt
                .query_map(
                    params![set.scope_kind.as_str(), set.scope_id, set.tag, limit as i64],
                    row_to_node,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::db::Db;

    fn test_db() -> Arc<Db> {
        let cfg = Config::from_env();
        Arc::new(Db::open_in_memory(&cfg).expect("open in-memory db"))
    }

    #[test]
    fn upsert_and_get_node_roundtrip() {
        let store = SqliteGraphStore::new(test_db());
        let node = DataPoint::entity("Ada Lovelace", 100);
        store.upsert_node(&node).unwrap();
        let fetched = store.get_node(node.id).unwrap().expect("node exists");
        assert_eq!(fetched.name, "Ada Lovelace");
        assert_eq!(fetched.kind, NodeKind::Entity);
        assert!(fetched.is_proper_noun);
    }

    #[test]
    fn upsert_and_query_edge() {
        let store = SqliteGraphStore::new(test_db());
        let a = DataPoint::entity("Alice", 1);
        let b = DataPoint::entity("Bob", 1);
        store.upsert_node(&a).unwrap();
        store.upsert_node(&b).unwrap();

        let mut edge = RelationshipEdge::new(a.id, b.id, "knows", 1);
        edge.strength = 0.5;
        store.upsert_edge(&edge).unwrap();

        let nbrs = store.neighbors(a.id, 10).unwrap();
        assert_eq!(nbrs.len(), 1);
        assert_eq!(nbrs[0].predicate, "knows");
        assert!((nbrs[0].strength - 0.5).abs() < 1e-3);
    }

    #[test]
    fn node_set_tagging_and_lookup() {
        let store = SqliteGraphStore::new(test_db());
        let chunk = DataPoint::chunk("hello world", Some("abc".into()), 10);
        store.upsert_node(&chunk).unwrap();

        let set = NodeSet::group("group_jid_1", "default_memory");
        store.tag_node(chunk.id, &set).unwrap();

        let nodes = store.nodes_in_set(&set, 10).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, chunk.id);
    }

    #[test]
    fn dedupe_by_content_hash() {
        let store = SqliteGraphStore::new(test_db());
        let chunk = DataPoint::chunk("payload", Some("hash-1".into()), 1);
        store.upsert_node(&chunk).unwrap();
        let dup = store.find_node_by_content_hash("hash-1").unwrap();
        assert!(dup.is_some());
        assert_eq!(dup.unwrap().id, chunk.id);
    }

    #[test]
    fn top_nodes_by_degree_orders_correctly() {
        // hub --(rel)--> a, b, c   (degree 3)
        // a   --(rel)--> b         (degree 2)
        // b                         (degree 2 from above)
        // c                         (degree 1)
        // → expected ordering by degree desc: hub, a, b, c
        let store = SqliteGraphStore::new(test_db());
        let hub = DataPoint::entity("hub", 1);
        let a = DataPoint::entity("a", 1);
        let b = DataPoint::entity("b", 1);
        let c = DataPoint::entity("c", 1);
        let lonely = DataPoint::entity("lonely", 1);
        for n in [&hub, &a, &b, &c, &lonely] {
            store.upsert_node(n).unwrap();
        }
        let mk = |src, dst| {
            let mut e = RelationshipEdge::new(src, dst, "rel", 1);
            e.last_activated = 1;
            e
        };
        store.upsert_edge(&mk(hub.id, a.id)).unwrap();
        store.upsert_edge(&mk(hub.id, b.id)).unwrap();
        store.upsert_edge(&mk(hub.id, c.id)).unwrap();
        store.upsert_edge(&mk(a.id, b.id)).unwrap();

        let top = store.top_nodes_by_degree(10).unwrap();
        // 5 nodes, ordered by degree desc
        assert_eq!(top.len(), 5);
        assert_eq!(top[0].node.name, "hub");
        assert_eq!(top[0].degree, 3);
        // a and b both have degree 2 — order between them depends on
        // last_seen_at tiebreaker, but both must precede c (deg 1).
        let degrees: Vec<usize> = top.iter().map(|x| x.degree).collect();
        assert_eq!(degrees, vec![3, 2, 2, 1, 0]);
        // The lonely node lands last with degree 0.
        assert_eq!(top[4].node.name, "lonely");
        assert_eq!(top[4].degree, 0);
    }
}
