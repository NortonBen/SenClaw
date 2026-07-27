//! SQLite layer. One `Mutex<Connection>`, no ORM, no migration framework —
//! same shape as the other Space Apps.

use std::sync::Mutex;

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::engine::types::{now_ms, ChainId, Edge, PortRef};
use crate::model::*;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;

CREATE TABLE IF NOT EXISTS chains (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'INACTIVE',
  debug       INTEGER NOT NULL DEFAULT 0,
  version     INTEGER NOT NULL DEFAULT 1,
  created_at  TEXT NOT NULL DEFAULT '',
  updated_at  TEXT NOT NULL DEFAULT ''
);

-- Edges live in their own table, so a connection is stored once. The Go schema
-- kept them as a JSON-string array on BOTH endpoints of every edge.
CREATE TABLE IF NOT EXISTS nodes (
  chain_id INTEGER NOT NULL,
  id       TEXT NOT NULL,
  rule     TEXT NOT NULL,
  name     TEXT NOT NULL DEFAULT '',
  config   TEXT NOT NULL DEFAULT '{}',
  opts     TEXT NOT NULL DEFAULT '{}',
  x        REAL NOT NULL DEFAULT 0,
  y        REAL NOT NULL DEFAULT 0,
  debug    INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (chain_id, id)
);

CREATE TABLE IF NOT EXISTS edges (
  chain_id  INTEGER NOT NULL,
  id        TEXT NOT NULL,
  from_node TEXT NOT NULL,
  from_port TEXT NOT NULL,
  to_node   TEXT NOT NULL,
  to_port   TEXT NOT NULL,
  PRIMARY KEY (chain_id, id)
);
CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(chain_id, from_node, from_port);

CREATE TABLE IF NOT EXISTS runs (
  id           INTEGER PRIMARY KEY,
  chain_id     INTEGER NOT NULL,
  status       TEXT NOT NULL,
  trigger_node TEXT NOT NULL DEFAULT '',
  started_at   INTEGER NOT NULL,
  ended_at     INTEGER,
  hops         INTEGER NOT NULL DEFAULT 0,
  error        TEXT
);
CREATE INDEX IF NOT EXISTS idx_runs_chain ON runs(chain_id, started_at DESC);

CREATE TABLE IF NOT EXISTS run_hops (
  id       INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id   INTEGER NOT NULL,
  chain_id INTEGER NOT NULL,
  seq      INTEGER NOT NULL,
  node     TEXT NOT NULL,
  rule     TEXT NOT NULL,
  in_port  TEXT NOT NULL DEFAULT '',
  out_port TEXT NOT NULL DEFAULT '',
  kind     TEXT NOT NULL DEFAULT 'data',
  data     TEXT NOT NULL DEFAULT '',
  error    TEXT NOT NULL DEFAULT '',
  ts       INTEGER NOT NULL,
  dur_ms   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_hops_run ON run_hops(run_id, seq);

CREATE TABLE IF NOT EXISTS node_state (
  chain_id   INTEGER NOT NULL,
  node       TEXT NOT NULL,
  scope      TEXT NOT NULL,
  value      TEXT NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (chain_id, node, scope)
);

CREATE TABLE IF NOT EXISTS logs (
  id       INTEGER PRIMARY KEY AUTOINCREMENT,
  chain_id INTEGER NOT NULL,
  run_id   INTEGER,
  level    TEXT NOT NULL,
  node     TEXT,
  message  TEXT NOT NULL,
  ts       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_logs_chain ON logs(chain_id, ts DESC);
"#;

/// Columns added after v1. Errors ("duplicate column") are ignored on purpose.
const MIGRATIONS: &[&str] = &[
    // v2 placeholder — keep the mechanism exercised so the first real
    // migration is not also the first time this code path runs.
    "ALTER TABLE chains ADD COLUMN notes TEXT NOT NULL DEFAULT ''",
];

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        for m in MIGRATIONS {
            let _ = conn.execute(m, []);
        }
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self.conn.lock().map_err(|_| anyhow!("db mutex poisoned"))?;
        f(&guard)
    }

    // ---------------------------------------------------------------- chains

    pub fn list_chains(&self) -> Result<Vec<Chain>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, name, description, status, debug, version, created_at, updated_at
                 FROM chains ORDER BY updated_at DESC, id DESC",
            )?;
            let rows = st.query_map([], row_to_chain)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn get_chain(&self, id: ChainId) -> Result<Option<Chain>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, name, description, status, debug, version, created_at, updated_at
                 FROM chains WHERE id = ?1",
            )?;
            Ok(st.query_row(params![id], row_to_chain).optional()?)
        })
    }

    pub fn create_chain(&self, id: ChainId, name: &str, description: &str) -> Result<Chain> {
        let now = iso_now();
        self.with(|c| {
            c.execute(
                "INSERT INTO chains (id, name, description, status, debug, version, created_at, updated_at)
                 VALUES (?1, ?2, ?3, 'INACTIVE', 0, 1, ?4, ?4)",
                params![id, name, description, now],
            )?;
            Ok(())
        })?;
        Ok(Chain {
            id,
            name: name.to_string(),
            description: description.to_string(),
            status: ChainStatus::Inactive,
            debug: false,
            version: 1,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    pub fn update_chain_meta(
        &self,
        id: ChainId,
        name: Option<&str>,
        description: Option<&str>,
        debug: Option<bool>,
    ) -> Result<()> {
        self.with(|c| {
            if let Some(n) = name {
                c.execute("UPDATE chains SET name = ?2 WHERE id = ?1", params![id, n])?;
            }
            if let Some(d) = description {
                c.execute(
                    "UPDATE chains SET description = ?2 WHERE id = ?1",
                    params![id, d],
                )?;
            }
            if let Some(d) = debug {
                c.execute(
                    "UPDATE chains SET debug = ?2 WHERE id = ?1",
                    params![id, if d { 1 } else { 0 }],
                )?;
            }
            c.execute(
                "UPDATE chains SET updated_at = ?2 WHERE id = ?1",
                params![id, iso_now()],
            )?;
            Ok(())
        })
    }

    pub fn set_chain_status(&self, id: ChainId, status: ChainStatus) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE chains SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, status.as_str(), iso_now()],
            )?;
            Ok(())
        })
    }

    pub fn delete_chain(&self, id: ChainId) -> Result<()> {
        self.with(|c| {
            for sql in [
                "DELETE FROM chains WHERE id = ?1",
                "DELETE FROM nodes WHERE chain_id = ?1",
                "DELETE FROM edges WHERE chain_id = ?1",
                "DELETE FROM node_state WHERE chain_id = ?1",
                "DELETE FROM run_hops WHERE chain_id = ?1",
                "DELETE FROM runs WHERE chain_id = ?1",
                "DELETE FROM logs WHERE chain_id = ?1",
            ] {
                c.execute(sql, params![id])?;
            }
            Ok(())
        })
    }

    // ----------------------------------------------------------------- graph

    pub fn list_nodes(&self, chain_id: ChainId) -> Result<Vec<Node>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT chain_id, id, rule, name, config, opts, x, y, debug
                 FROM nodes WHERE chain_id = ?1 ORDER BY id",
            )?;
            let rows = st.query_map(params![chain_id], row_to_node)?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn list_edges(&self, chain_id: ChainId) -> Result<Vec<Edge>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, from_node, from_port, to_node, to_port
                 FROM edges WHERE chain_id = ?1 ORDER BY id",
            )?;
            let rows = st.query_map(params![chain_id], |r| {
                Ok(Edge {
                    id: r.get(0)?,
                    from: PortRef::new(r.get::<_, String>(1)?, r.get::<_, String>(2)?),
                    to: PortRef::new(r.get::<_, String>(3)?, r.get::<_, String>(4)?),
                })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Full replace — the editor always sends the whole graph.
    ///
    /// Wrapped in a transaction: the delete-then-insert must be atomic, or a
    /// failure part-way (a bad row, a disk error) would leave the chain with the
    /// old nodes gone and only some new ones in — a graph that neither matches
    /// what the user saved nor what was running.
    pub fn replace_graph(&self, chain_id: ChainId, nodes: &[Node], edges: &[Edge]) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute("DELETE FROM nodes WHERE chain_id = ?1", params![chain_id])?;
            tx.execute("DELETE FROM edges WHERE chain_id = ?1", params![chain_id])?;
            for n in nodes {
                tx.execute(
                    "INSERT INTO nodes (chain_id, id, rule, name, config, opts, x, y, debug)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        chain_id,
                        n.id,
                        n.rule,
                        n.name,
                        n.config.to_string(),
                        serde_json::to_string(&n.opts).unwrap_or_else(|_| "{}".into()),
                        n.x,
                        n.y,
                        if n.debug { 1 } else { 0 },
                    ],
                )?;
            }
            for e in edges {
                tx.execute(
                    "INSERT INTO edges (chain_id, id, from_node, from_port, to_node, to_port)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        chain_id,
                        e.id,
                        e.from.node,
                        e.from.port,
                        e.to.node,
                        e.to.port
                    ],
                )?;
            }
            tx.execute(
                "UPDATE chains SET version = version + 1, updated_at = ?2 WHERE id = ?1",
                params![chain_id, iso_now()],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    // ------------------------------------------------------------------ runs

    pub fn insert_run(&self, id: i64, chain_id: ChainId, trigger_node: &str) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO runs (id, chain_id, status, trigger_node, started_at, hops)
                 VALUES (?1, ?2, 'running', ?3, ?4, 0)",
                params![id, chain_id, trigger_node, now_ms()],
            )?;
            Ok(())
        })
    }

    pub fn finish_run(
        &self,
        id: i64,
        status: RunStatus,
        hops: i64,
        error: Option<&str>,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE runs SET status = ?2, ended_at = ?3, hops = ?4, error = ?5 WHERE id = ?1",
                params![id, status.as_str(), now_ms(), hops, error],
            )?;
            Ok(())
        })
    }

    pub fn list_runs(&self, chain_id: Option<ChainId>, limit: i64) -> Result<Vec<RunRow>> {
        self.with(|c| {
            let (sql, has_chain) = match chain_id {
                Some(_) => (
                    "SELECT id, chain_id, status, trigger_node, started_at, ended_at, hops, error
                     FROM runs WHERE chain_id = ?1 ORDER BY started_at DESC LIMIT ?2",
                    true,
                ),
                None => (
                    "SELECT id, chain_id, status, trigger_node, started_at, ended_at, hops, error
                     FROM runs ORDER BY started_at DESC LIMIT ?1",
                    false,
                ),
            };
            let mut st = c.prepare(sql)?;
            let rows = if has_chain {
                st.query_map(params![chain_id.unwrap_or(0), limit], row_to_run)?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            } else {
                st.query_map(params![limit], row_to_run)?
                    .collect::<std::result::Result<Vec<_>, _>>()?
            };
            Ok(rows)
        })
    }

    pub fn insert_hop(&self, h: &HopRow) -> Result<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO run_hops (run_id, chain_id, seq, node, rule, in_port, out_port, kind, data, error, ts, dur_ms)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                params![
                    h.run_id, h.chain_id, h.seq, h.node, h.rule, h.in_port, h.out_port,
                    h.kind, h.data, h.error, h.ts, h.dur_ms
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_hops(&self, run_id: i64) -> Result<Vec<HopRow>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, run_id, chain_id, seq, node, rule, in_port, out_port, kind, data, error, ts, dur_ms
                 FROM run_hops WHERE run_id = ?1 ORDER BY seq, id",
            )?;
            let rows = st.query_map(params![run_id], |r| {
                Ok(HopRow {
                    id: r.get(0)?,
                    run_id: r.get(1)?,
                    chain_id: r.get(2)?,
                    seq: r.get(3)?,
                    node: r.get(4)?,
                    rule: r.get(5)?,
                    in_port: r.get(6)?,
                    out_port: r.get(7)?,
                    kind: r.get(8)?,
                    data: r.get(9)?,
                    error: r.get(10)?,
                    ts: r.get(11)?,
                    dur_ms: r.get(12)?,
                })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    /// Keep the newest `keep_per_chain` runs of each chain and drop the rest,
    /// then delete any hop whose run is gone.
    ///
    /// The previous version pruned hops per-chain but runs globally, so runs
    /// beyond the global cut kept their hops forever — orphans that nothing ever
    /// reclaimed. Deleting hops by "run no longer exists" closes that gap.
    pub fn prune_runs(&self, keep_per_chain: i64) -> Result<()> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM runs WHERE id IN (
                   SELECT id FROM (
                     SELECT id, ROW_NUMBER() OVER (
                       PARTITION BY chain_id ORDER BY started_at DESC, id DESC) AS rn
                     FROM runs
                   ) WHERE rn > ?1)",
                params![keep_per_chain],
            )?;
            tx.execute(
                "DELETE FROM run_hops WHERE run_id NOT IN (SELECT id FROM runs)",
                [],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    // ------------------------------------------------------------ node state

    pub fn state_get(&self, chain_id: ChainId, node: &str, scope: &str) -> Option<Value> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT value FROM node_state WHERE chain_id = ?1 AND node = ?2 AND scope = ?3",
            )?;
            let v: Option<String> = st
                .query_row(params![chain_id, node, scope], |r| r.get(0))
                .optional()?;
            Ok(v.and_then(|s| serde_json::from_str(&s).ok()))
        })
        .ok()
        .flatten()
    }

    pub fn state_set(&self, chain_id: ChainId, node: &str, scope: &str, value: &Value) {
        let _ = self.with(|c| {
            c.execute(
                "INSERT INTO node_state (chain_id, node, scope, value, updated_at)
                 VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(chain_id, node, scope) DO UPDATE SET value = ?4, updated_at = ?5",
                params![chain_id, node, scope, value.to_string(), now_ms()],
            )?;
            Ok(())
        });
    }

    pub fn state_clear(&self, chain_id: ChainId, node: Option<&str>) -> Result<()> {
        self.with(|c| {
            match node {
                Some(n) => c.execute(
                    "DELETE FROM node_state WHERE chain_id = ?1 AND node = ?2",
                    params![chain_id, n],
                )?,
                None => c.execute(
                    "DELETE FROM node_state WHERE chain_id = ?1",
                    params![chain_id],
                )?,
            };
            Ok(())
        })
    }

    // ------------------------------------------------------------------ logs

    pub fn insert_log(&self, l: &LogRow) {
        let _ = self.with(|c| {
            c.execute(
                "INSERT INTO logs (chain_id, run_id, level, node, message, ts)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![l.chain_id, l.run_id, l.level, l.node, l.message, l.ts],
            )?;
            Ok(())
        });
    }

    pub fn list_logs(&self, chain_id: ChainId, limit: i64) -> Result<Vec<LogRow>> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, chain_id, run_id, level, node, message, ts
                 FROM logs WHERE chain_id = ?1 ORDER BY ts DESC, id DESC LIMIT ?2",
            )?;
            let rows = st.query_map(params![chain_id, limit], |r| {
                Ok(LogRow {
                    id: r.get(0)?,
                    chain_id: r.get(1)?,
                    run_id: r.get(2)?,
                    level: r.get(3)?,
                    node: r.get(4)?,
                    message: r.get(5)?,
                    ts: r.get(6)?,
                })
            })?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
    }

    pub fn prune_logs(&self, keep: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "DELETE FROM logs WHERE id NOT IN (
                   SELECT id FROM (SELECT id FROM logs ORDER BY ts DESC LIMIT ?1))",
                params![keep],
            )?;
            Ok(())
        })
    }
}

fn row_to_chain(r: &rusqlite::Row<'_>) -> rusqlite::Result<Chain> {
    Ok(Chain {
        id: r.get(0)?,
        name: r.get(1)?,
        description: r.get(2)?,
        status: ChainStatus::parse(&r.get::<_, String>(3)?),
        debug: r.get::<_, i64>(4)? != 0,
        version: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

fn row_to_node(r: &rusqlite::Row<'_>) -> rusqlite::Result<Node> {
    let config: String = r.get(4)?;
    let opts: String = r.get(5)?;
    Ok(Node {
        chain_id: r.get(0)?,
        id: r.get(1)?,
        rule: r.get(2)?,
        name: r.get(3)?,
        config: serde_json::from_str(&config).unwrap_or_else(|_| serde_json::json!({})),
        opts: serde_json::from_str(&opts).unwrap_or_default(),
        x: r.get(6)?,
        y: r.get(7)?,
        debug: r.get::<_, i64>(8)? != 0,
    })
}

fn row_to_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<RunRow> {
    Ok(RunRow {
        id: r.get(0)?,
        chain_id: r.get(1)?,
        status: r.get(2)?,
        trigger_node: r.get(3)?,
        started_at: r.get(4)?,
        ended_at: r.get(5)?,
        hops: r.get(6)?,
        error: r.get(7)?,
    })
}

fn iso_now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn db() -> Db {
        Db::open(":memory:").expect("open in-memory db")
    }

    #[test]
    fn chain_crud_roundtrip() {
        let db = db();
        let c = db.create_chain(1, "Luồng A", "mô tả").unwrap();
        assert_eq!(c.status, ChainStatus::Inactive);
        db.set_chain_status(1, ChainStatus::Active).unwrap();
        let got = db.get_chain(1).unwrap().unwrap();
        assert_eq!(got.status, ChainStatus::Active);
        assert_eq!(db.list_chains().unwrap().len(), 1);
        db.delete_chain(1).unwrap();
        assert!(db.get_chain(1).unwrap().is_none());
    }

    #[test]
    fn graph_replace_stores_each_edge_once() {
        let db = db();
        db.create_chain(7, "g", "").unwrap();
        let nodes = vec![
            Node {
                id: "a".into(),
                chain_id: 7,
                rule: "manual".into(),
                name: "A".into(),
                config: json!({}),
                opts: NodeOpts::default(),
                x: 1.0,
                y: 2.0,
                debug: false,
            },
            Node {
                id: "b".into(),
                chain_id: 7,
                rule: "log".into(),
                name: "B".into(),
                config: json!({}),
                opts: NodeOpts::default(),
                x: 3.0,
                y: 4.0,
                debug: true,
            },
        ];
        let edges = vec![Edge {
            id: "e1".into(),
            from: PortRef::new("a", "out"),
            to: PortRef::new("b", "in"),
        }];
        db.replace_graph(7, &nodes, &edges).unwrap();
        assert_eq!(db.list_nodes(7).unwrap().len(), 2);
        let got = db.list_edges(7).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].from.port, "out");
        // Replacing again must not duplicate.
        db.replace_graph(7, &nodes, &edges).unwrap();
        assert_eq!(db.list_edges(7).unwrap().len(), 1);
        assert_eq!(db.get_chain(7).unwrap().unwrap().version, 3);
    }

    #[test]
    fn node_config_and_opts_survive_a_roundtrip() {
        let db = db();
        db.create_chain(9, "g", "").unwrap();
        let n = Node {
            id: "n".into(),
            chain_id: 9,
            rule: "conditional".into(),
            name: "cond".into(),
            config: json!({ "expr": "a > 3" }),
            opts: NodeOpts {
                join: JoinPolicy::All,
                concurrency: 4,
                ..Default::default()
            },
            x: 0.0,
            y: 0.0,
            debug: false,
        };
        db.replace_graph(9, &[n], &[]).unwrap();
        let got = &db.list_nodes(9).unwrap()[0];
        assert_eq!(got.config["expr"], "a > 3");
        assert_eq!(got.opts.join, JoinPolicy::All);
        assert_eq!(got.opts.concurrency, 4);
    }

    #[test]
    fn node_state_upserts() {
        let db = db();
        db.state_set(1, "n", "win", &json!([1, 2, 3]));
        db.state_set(1, "n", "win", &json!([4]));
        assert_eq!(db.state_get(1, "n", "win").unwrap(), json!([4]));
        db.state_clear(1, None).unwrap();
        assert!(db.state_get(1, "n", "win").is_none());
    }

    #[test]
    fn runs_and_hops_are_queryable() {
        let db = db();
        db.create_chain(3, "c", "").unwrap();
        db.insert_run(100, 3, "src").unwrap();
        db.insert_hop(&HopRow {
            id: 0,
            run_id: 100,
            chain_id: 3,
            seq: 1,
            node: "a".into(),
            rule: "manual".into(),
            in_port: "".into(),
            out_port: "out".into(),
            kind: "data".into(),
            data: "{}".into(),
            error: "".into(),
            ts: 1,
            dur_ms: 2,
        })
        .unwrap();
        db.finish_run(100, RunStatus::Done, 1, None).unwrap();
        let runs = db.list_runs(Some(3), 10).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, "done");
        assert_eq!(db.list_hops(100).unwrap().len(), 1);
    }

    #[test]
    fn prune_keeps_newest_per_chain_and_leaves_no_orphan_hops() {
        let db = db();
        db.create_chain(1, "a", "").unwrap();
        db.create_chain(2, "b", "").unwrap();
        // 5 runs per chain, each with one hop. started_at derives from now_ms,
        // so insert in ascending id order to fix the "newest" ordering.
        for chain in [1i64, 2] {
            for k in 0..5 {
                let run = chain * 1000 + k;
                db.insert_run(run, chain, "src").unwrap();
                db.insert_hop(&HopRow {
                    id: 0,
                    run_id: run,
                    chain_id: chain,
                    seq: 1,
                    node: "n".into(),
                    rule: "manual".into(),
                    in_port: String::new(),
                    out_port: "out".into(),
                    kind: "data".into(),
                    data: "{}".into(),
                    error: String::new(),
                    ts: run,
                    dur_ms: 0,
                })
                .unwrap();
                db.finish_run(run, RunStatus::Done, 1, None).unwrap();
            }
        }
        db.prune_runs(2).unwrap();
        // 2 kept per chain.
        assert_eq!(db.list_runs(Some(1), 100).unwrap().len(), 2);
        assert_eq!(db.list_runs(Some(2), 100).unwrap().len(), 2);
        // Every surviving hop belongs to a surviving run — no orphans. The old
        // implementation pruned hops per-chain but runs globally, stranding
        // hops whose run had been deleted.
        let kept_runs: std::collections::HashSet<i64> = db
            .list_runs(None, 100)
            .unwrap()
            .into_iter()
            .map(|r| r.id)
            .collect();
        for chain in [1i64, 2] {
            for k in 0..5 {
                let run = chain * 1000 + k;
                let hops = db.list_hops(run).unwrap();
                if kept_runs.contains(&run) {
                    assert_eq!(hops.len(), 1, "run {run} phải còn hop");
                } else {
                    assert!(hops.is_empty(), "run {run} bị xoá nhưng hop còn mồ côi");
                }
            }
        }
    }
}
