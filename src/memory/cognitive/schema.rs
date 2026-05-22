//! Cognitive memory schema — graph of nodes + Hebbian edges.
//!
//! Combines:
//!   * **cognee** — DataPoint nodes, triplet edges, NodeSet scoping,
//!     ingestion via `add → cognify → search`.
//!   * **shodh-memory** — Hebbian strengthen/decay on edges, multi-tier
//!     consolidation (L1 Working / L2 Episodic / L3 Semantic), LTP states
//!     (None / Burst / Weekly / Full), endpoint selectivity, salience.
//!
//! Tables:
//!   * `cog_nodes`      — entities, chunks, summaries (port of DataPoint)
//!   * `cog_edges`      — typed relationships with Hebbian dynamics
//!   * `cog_node_sets`  — per-(group/persona/cowork) scope tags
//!   * `cog_node_tags`  — N:N join nodes ↔ node_sets
//!   * `cog_vec`        — sqlite-vec virtual table for node embeddings
//!                        (created only when `enable_vec=true`)
//!
//! All timestamps are unix-seconds (i64) to match shodh-memory and the rest of
//! the senclaw schema (chrono-free at the storage layer).

use anyhow::Result;
use rusqlite::Connection;

/// Apply cognitive schema. Idempotent.
///
/// `cog_vec` (sqlite-vec) is intentionally **not created here** — it is added
/// by [`apply_cognitive_vec_schema`] once dimensions are known (P2 / MLX
/// embedder), mirroring how `apply_memory_schema` defers `memory_chunks_vec`.
/// ALTER TABLE for existing DBs that pre-date the extraction_state /
/// extracted_at columns. Safe to call on fresh DBs (the CREATE TABLE
/// already includes them; ALTER will error → swallowed).
fn migrate_extraction_state(conn: &Connection) -> Result<()> {
    // Use `column_names` pattern from src/db/schema.rs: check before alter.
    let exists = |name: &str| -> bool {
        let mut stmt = match conn.prepare(&format!("PRAGMA table_info({name})")) {
            Ok(s) => s,
            Err(_) => return false,
        };
        let cols: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default();
        !cols.is_empty()
    };
    if !exists("cog_nodes") {
        return Ok(());
    }
    let mut stmt = conn.prepare("PRAGMA table_info(cog_nodes)")?;
    let cols: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<_>>()?;
    if !cols.iter().any(|c| c == "extraction_state") {
        conn.execute(
            "ALTER TABLE cog_nodes ADD COLUMN extraction_state INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        // Back-fill: every chunk that already has outgoing edges (other
        // than MENTIONS, which are auto-generated) was extracted before
        // this migration ran — mark them `done` so the dedupe-skip gate
        // works on existing data without re-LLMing the whole graph.
        conn.execute(
            "UPDATE cog_nodes SET extraction_state = 1
             WHERE kind = 'chunk'
               AND id IN (SELECT DISTINCT src FROM cog_edges WHERE predicate <> 'MENTIONS')",
            [],
        )?;
    }
    if !cols.iter().any(|c| c == "extracted_at") {
        conn.execute("ALTER TABLE cog_nodes ADD COLUMN extracted_at INTEGER", [])?;
    }
    Ok(())
}

pub fn apply_cognitive_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- ============================================================
        -- Nodes (DataPoint in cognee; EntityNode in shodh-memory)
        -- ============================================================
        CREATE TABLE IF NOT EXISTS cog_nodes (
            id              BLOB    PRIMARY KEY,           -- UUIDv4 bytes (16)
            kind            TEXT    NOT NULL,              -- entity | chunk | summary | custom
            type_name       TEXT    NOT NULL DEFAULT '',   -- user-defined DataPoint type
            name            TEXT    NOT NULL DEFAULT '',
            summary         TEXT    NOT NULL DEFAULT '',
            content_hash    TEXT,                          -- dedupe (shodh content-hash)
            props_json      TEXT    NOT NULL DEFAULT '{}',
            -- embedding: little-endian f32 blob; used as fallback when
            -- sqlite-vec is unavailable. Authoritative copy lives here.
            embedding       BLOB,
            embedding_model TEXT,
            -- shodh dynamics
            salience        REAL    NOT NULL DEFAULT 0.5,
            mention_count   INTEGER NOT NULL DEFAULT 1,
            is_proper_noun  INTEGER NOT NULL DEFAULT 0,
            selectivity     REAL,
            -- Triplet-extraction state machine (chunk nodes only — others
            -- are derived from extraction so they're implicitly "done"):
            --   0 = pending           — needs LLM extraction
            --   1 = done              — extraction completed; do not re-run
            --   2 = skipped_no_llm    — LLM was dormant; retry when one is up
            --   3 = skipped_no_facts  — LLM ran but returned 0 useful triplets
            -- Encoded as INTEGER (not TEXT) for cheap WHERE filtering.
            extraction_state INTEGER NOT NULL DEFAULT 0,
            extracted_at     INTEGER,
            -- tier / lifecycle
            created_at      INTEGER NOT NULL,
            updated_at      INTEGER NOT NULL,
            last_seen_at    INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_cog_nodes_kind
            ON cog_nodes(kind, last_seen_at DESC);
        CREATE INDEX IF NOT EXISTS idx_cog_nodes_content_hash
            ON cog_nodes(content_hash) WHERE content_hash IS NOT NULL;
        CREATE INDEX IF NOT EXISTS idx_cog_nodes_name
            ON cog_nodes(name) WHERE name <> '';

        -- ============================================================
        -- Edges — Hebbian, tier-aware, LTP-protected
        -- ============================================================
        --   tier:        0=L1Working, 1=L2Episodic, 2=L3Semantic
        --   ltp_status:  0=None, 1=Burst, 2=Weekly, 3=Full
        --   activation_timestamps: JSON array (ring buffer, max 32) of i64
        CREATE TABLE IF NOT EXISTS cog_edges (
            src                   BLOB    NOT NULL,
            dst                   BLOB    NOT NULL,
            predicate             TEXT    NOT NULL,
            props_json            TEXT    NOT NULL DEFAULT '{}',
            -- temporal validity (cognee temporal awareness)
            valid_from            INTEGER NOT NULL,
            valid_to              INTEGER,
            -- shodh Hebbian / LTP dynamics
            strength              REAL    NOT NULL DEFAULT 0.1,
            tier                  INTEGER NOT NULL DEFAULT 0,
            activation_count      INTEGER NOT NULL DEFAULT 0,
            last_activated        INTEGER NOT NULL,
            ltp_status            INTEGER NOT NULL DEFAULT 0,
            ltp_detected_at       INTEGER,
            entity_confidence     REAL,
            endpoint_selectivity  REAL,
            forman_curvature      REAL,
            activation_timestamps TEXT NOT NULL DEFAULT '[]',
            -- provenance
            source_episode_id     BLOB,
            context               TEXT NOT NULL DEFAULT '',
            created_at            INTEGER NOT NULL,
            PRIMARY KEY (src, dst, predicate),
            FOREIGN KEY (src) REFERENCES cog_nodes(id) ON DELETE CASCADE,
            FOREIGN KEY (dst) REFERENCES cog_nodes(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_cog_edges_src
            ON cog_edges(src, last_activated DESC);
        CREATE INDEX IF NOT EXISTS idx_cog_edges_dst
            ON cog_edges(dst, last_activated DESC);
        CREATE INDEX IF NOT EXISTS idx_cog_edges_tier_strength
            ON cog_edges(tier, strength DESC);
        CREATE INDEX IF NOT EXISTS idx_cog_edges_last_activated
            ON cog_edges(last_activated);

        -- ============================================================
        -- NodeSets — scope tagging (group / persona / cowork)
        -- ============================================================
        --   scope_kind:
        --     'group'    → scope_id = group jid
        --     'persona'  → scope_id = persona slug
        --     'cowork'   → scope_id = workspace id
        --     'global'   → scope_id = ''
        --     'custom'   → free-form tag (advanced)
        CREATE TABLE IF NOT EXISTS cog_node_sets (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            scope_kind  TEXT    NOT NULL,
            scope_id    TEXT    NOT NULL DEFAULT '',
            tag         TEXT    NOT NULL,
            created_at  INTEGER NOT NULL,
            UNIQUE (scope_kind, scope_id, tag)
        );

        CREATE TABLE IF NOT EXISTS cog_node_tags (
            node_id     BLOB    NOT NULL,
            node_set_id INTEGER NOT NULL,
            PRIMARY KEY (node_id, node_set_id),
            FOREIGN KEY (node_id)     REFERENCES cog_nodes(id)     ON DELETE CASCADE,
            FOREIGN KEY (node_set_id) REFERENCES cog_node_sets(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_cog_node_tags_set
            ON cog_node_tags(node_set_id);

        -- ============================================================
        -- Decay log — bookkeeping for periodic decay_tick runs
        -- ============================================================
        CREATE TABLE IF NOT EXISTS cog_decay_log (
            run_at         INTEGER PRIMARY KEY,
            edges_scanned  INTEGER NOT NULL DEFAULT 0,
            edges_pruned   INTEGER NOT NULL DEFAULT 0,
            edges_promoted INTEGER NOT NULL DEFAULT 0,
            duration_ms    INTEGER NOT NULL DEFAULT 0
        );
        "#,
    )?;
    // Migrations for older cog_nodes that pre-date extraction_state cols.
    migrate_extraction_state(conn)?;
    Ok(())
}

/// Create the `cog_vec` virtual table once embedder dimensions are known.
/// Called separately from [`apply_cognitive_schema`] because dimensions
/// depend on the configured embedding provider (see [`crate::memory::schema`]).
///
/// Returns `Ok(false)` and logs if `sqlite-vec` is not loaded (mirrors
/// `apply_memory_schema` behaviour).
pub fn apply_cognitive_vec_schema(conn: &Connection, dimensions: u32) -> Result<bool> {
    let sql = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS cog_vec USING vec0(
            node_id BLOB PRIMARY KEY,
            embedding float[{dimensions}]
         );"
    );
    match conn.execute_batch(&sql) {
        Ok(()) => Ok(true),
        Err(e) => {
            tracing::warn!(
                error = %e,
                "[cognitive] sqlite-vec not available; cog_vec table skipped"
            );
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn schema_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        apply_cognitive_schema(&conn).unwrap();
        // applying twice must not error
        apply_cognitive_schema(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'cog_%'")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(tables.iter().any(|t| t == "cog_nodes"));
        assert!(tables.iter().any(|t| t == "cog_edges"));
        assert!(tables.iter().any(|t| t == "cog_node_sets"));
        assert!(tables.iter().any(|t| t == "cog_node_tags"));
        assert!(tables.iter().any(|t| t == "cog_decay_log"));
    }
}
