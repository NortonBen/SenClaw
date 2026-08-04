//! SQLite access — a single serialized connection behind a mutex.

use crate::claims::{Claim, Contradiction};
use crate::model::{Evidence, SourceOutcome};
use crate::pipeline::SearchOutcome;
use crate::sources::mcp_source::McpSourceSpec;
use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

const SCHEMA: &str = include_str!("schema.sql");

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn new_id(prefix: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let t = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    format!("{prefix}_{t:x}{n:x}")
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CorpusHit {
    pub chunk_id: String,
    pub doc_id: String,
    pub doc_name: String,
    pub ord: usize,
    pub text: String,
    pub score: f32,
}

#[derive(Debug, Serialize)]
pub struct RunSummary {
    pub id: String,
    pub query: String,
    pub status: String,
    pub depth: i64,
    pub verify_level: String,
    pub evidence_count: i64,
    pub total_before_dedupe: i64,
    pub deepened: i64,
    pub ms: i64,
    pub error: Option<String>,
    pub created_at: String,
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let guard = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db mutex poisoned: {e}"))?;
        f(&guard)
    }

    fn with_conn_mut<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("db mutex poisoned: {e}"))?;
        f(&mut guard)
    }

    /// Persist a completed run: header, per-source outcomes, evidence.
    ///
    /// One transaction — a half-written run would misreport which sources ran,
    /// which is exactly the thing `run_sources` exists to make trustworthy.
    pub fn save_run(
        &self,
        out: &SearchOutcome,
        params: &Value,
        verify_level: &str,
    ) -> Result<String> {
        let run_id = new_id("run");
        let created = now();
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO runs (id, query, params_json, status, depth, verify_level,
                                   evidence_count, total_before_dedupe, deepened, ms, created_at)
                 VALUES (?1, ?2, ?3, 'ok', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    run_id,
                    out.query,
                    params.to_string(),
                    params.get("depth").and_then(Value::as_i64).unwrap_or(1),
                    verify_level,
                    out.evidence.len() as i64,
                    out.total_before_dedupe as i64,
                    out.deepened as i64,
                    out.ms as i64,
                    created,
                ],
            )?;

            for s in &out.sources {
                tx.execute(
                    "INSERT INTO run_sources (run_id, source_id, sub_query, status,
                                              item_count, dropped_count, ms, error)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        run_id,
                        s.source_id,
                        s.sub_query,
                        s.status,
                        s.item_count as i64,
                        s.dropped_count as i64,
                        s.ms as i64,
                        s.error,
                    ],
                )?;
            }

            for (ord, e) in out.evidence.iter().enumerate() {
                tx.execute(
                    "INSERT OR REPLACE INTO evidence
                       (id, run_id, title, url, canonical_url, domain, snippet, full_text,
                        author, published_at, retrieved_at, lang, meta_json, hits_json,
                        fused_score, independent_kinds, independent_domains, ord)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
                    params![
                        e.id,
                        run_id,
                        e.title,
                        e.url,
                        e.canonical_url,
                        e.domain,
                        e.snippet,
                        e.full_text,
                        e.author,
                        e.published_at,
                        e.retrieved_at,
                        e.lang,
                        e.meta.to_string(),
                        serde_json::to_string(&e.hits).unwrap_or_else(|_| "[]".into()),
                        e.fused_score as f64,
                        e.independent_kinds as i64,
                        e.independent_domains as i64,
                        ord as i64,
                    ],
                )?;
            }

            tx.commit()?;
            Ok(())
        })?;
        Ok(run_id)
    }

    pub fn list_runs(&self, limit: usize) -> Result<Vec<RunSummary>> {
        self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT id, query, status, depth, verify_level, evidence_count,
                        total_before_dedupe, deepened, ms, error, created_at
                 FROM runs ORDER BY created_at DESC LIMIT ?1",
            )?;
            let rows = st.query_map([limit as i64], |r| {
                Ok(RunSummary {
                    id: r.get(0)?,
                    query: r.get(1)?,
                    status: r.get(2)?,
                    depth: r.get(3)?,
                    verify_level: r.get(4)?,
                    evidence_count: r.get(5)?,
                    total_before_dedupe: r.get(6)?,
                    deepened: r.get(7)?,
                    ms: r.get(8)?,
                    error: r.get(9)?,
                    created_at: r.get(10)?,
                })
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        })
    }

    pub fn get_run(&self, run_id: &str) -> Result<Option<Value>> {
        let summary = self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT id, query, params_json, status, depth, verify_level, evidence_count,
                        total_before_dedupe, deepened, ms, error, created_at
                 FROM runs WHERE id = ?1",
            )?;
            let mut rows = st.query([run_id])?;
            Ok(match rows.next()? {
                Some(r) => Some(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "query": r.get::<_, String>(1)?,
                    "params": serde_json::from_str::<Value>(&r.get::<_, String>(2)?)
                        .unwrap_or(Value::Null),
                    "status": r.get::<_, String>(3)?,
                    "depth": r.get::<_, i64>(4)?,
                    "verify_level": r.get::<_, String>(5)?,
                    "evidence_count": r.get::<_, i64>(6)?,
                    "total_before_dedupe": r.get::<_, i64>(7)?,
                    "deepened": r.get::<_, i64>(8)?,
                    "ms": r.get::<_, i64>(9)?,
                    "error": r.get::<_, Option<String>>(10)?,
                    "created_at": r.get::<_, String>(11)?,
                })),
                None => None,
            })
        })?;
        let Some(mut summary) = summary else {
            return Ok(None);
        };
        summary["sources"] = serde_json::to_value(self.run_sources(run_id)?)?;
        summary["evidence"] = serde_json::to_value(self.run_evidence(run_id)?)?;
        Ok(Some(summary))
    }

    pub fn run_sources(&self, run_id: &str) -> Result<Vec<SourceOutcome>> {
        self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT source_id, sub_query, status, item_count, dropped_count, ms, error
                 FROM run_sources WHERE run_id = ?1 ORDER BY id",
            )?;
            let rows = st.query_map([run_id], |r| {
                Ok(SourceOutcome {
                    source_id: r.get(0)?,
                    sub_query: r.get(1)?,
                    status: r.get(2)?,
                    item_count: r.get::<_, i64>(3)? as usize,
                    dropped_count: r.get::<_, i64>(4)? as usize,
                    ms: r.get::<_, i64>(5)? as u64,
                    error: r.get(6)?,
                })
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        })
    }

    pub fn run_evidence(&self, run_id: &str) -> Result<Vec<Evidence>> {
        self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT id, title, url, canonical_url, domain, snippet, full_text, author,
                        published_at, retrieved_at, lang, meta_json, hits_json, fused_score,
                        independent_kinds, independent_domains
                 FROM evidence WHERE run_id = ?1 ORDER BY ord",
            )?;
            let rows = st.query_map([run_id], |r| {
                Ok(Evidence {
                    id: r.get(0)?,
                    title: r.get(1)?,
                    url: r.get(2)?,
                    canonical_url: r.get(3)?,
                    domain: r.get(4)?,
                    snippet: r.get(5)?,
                    full_text: r.get(6)?,
                    author: r.get(7)?,
                    published_at: r.get(8)?,
                    retrieved_at: r.get(9)?,
                    lang: r.get(10)?,
                    meta: serde_json::from_str(&r.get::<_, String>(11)?).unwrap_or(Value::Null),
                    hits: serde_json::from_str(&r.get::<_, String>(12)?).unwrap_or_default(),
                    fused_score: r.get::<_, f64>(13)? as f32,
                    independent_kinds: r.get::<_, i64>(14)? as usize,
                    independent_domains: r.get::<_, i64>(15)? as usize,
                })
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        })
    }

    pub fn delete_run(&self, run_id: &str) -> Result<bool> {
        self.with_conn(|conn| {
            let n = conn.execute("DELETE FROM runs WHERE id = ?1", [run_id])?;
            Ok(n > 0)
        })
    }

    // --- source config persistence ---------------------------------------

    pub fn load_source_config(
        &self,
    ) -> Result<Vec<(String, Option<bool>, Option<f32>, Option<i64>, Option<i64>)>> {
        self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT source_id, enabled, weight, max_results, timeout_ms FROM source_config",
            )?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<i64>>(1)?.map(|v| v != 0),
                    r.get::<_, Option<f64>>(2)?.map(|v| v as f32),
                    r.get::<_, Option<i64>>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                ))
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        })
    }

    pub fn save_source_config(
        &self,
        source_id: &str,
        enabled: Option<bool>,
        weight: Option<f32>,
        max_results: Option<usize>,
        timeout_ms: Option<u64>,
    ) -> Result<()> {
        self.with_conn(|conn| {
            // COALESCE so a partial update never blanks the other fields.
            conn.execute(
                "INSERT INTO source_config (source_id, enabled, weight, max_results, timeout_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(source_id) DO UPDATE SET
                   enabled     = COALESCE(excluded.enabled, source_config.enabled),
                   weight      = COALESCE(excluded.weight, source_config.weight),
                   max_results = COALESCE(excluded.max_results, source_config.max_results),
                   timeout_ms  = COALESCE(excluded.timeout_ms, source_config.timeout_ms)",
                params![
                    source_id,
                    enabled.map(|v| v as i64),
                    weight.map(|v| v as f64),
                    max_results.map(|v| v as i64),
                    timeout_ms.map(|v| v as i64),
                ],
            )?;
            Ok(())
        })
    }

    // --- claims -----------------------------------------------------------

    /// Persist a run's claims, their evidence bindings and any contradictions.
    pub fn save_claims(
        &self,
        run_id: &str,
        claims: &[Claim],
        contradictions: &[Contradiction],
    ) -> Result<()> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            for (ord, c) in claims.iter().enumerate() {
                tx.execute(
                    "INSERT OR REPLACE INTO claims
                       (id, run_id, text, tier, confidence, independent_count, agreement,
                        high_stakes, verdict_json, ord)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    params![
                        c.id,
                        run_id,
                        c.text,
                        c.tier.as_str(),
                        c.confidence as f64,
                        c.independent_count as i64,
                        c.agreement as f64,
                        c.high_stakes as i64,
                        serde_json::to_string(&serde_json::json!({
                            "dropped_citations": c.dropped_citations
                        }))
                        .unwrap_or_else(|_| "null".into()),
                        ord as i64,
                    ],
                )?;
                for (ids, stance) in [(&c.supports, "supports"), (&c.refutes, "refutes")] {
                    for ev_id in ids {
                        tx.execute(
                            "INSERT OR REPLACE INTO claim_evidence
                               (claim_id, evidence_id, run_id, stance) VALUES (?1,?2,?3,?4)",
                            params![c.id, ev_id, run_id, stance],
                        )?;
                    }
                }
            }
            for ct in contradictions {
                tx.execute(
                    "INSERT OR REPLACE INTO contradictions (id, run_id, claim_a, claim_b, summary)
                     VALUES (?1,?2,?3,?4,?5)",
                    params![ct.id, run_id, ct.claim_a, ct.claim_b, ct.summary],
                )?;
            }
            tx.execute(
                "UPDATE runs SET claim_count = ?1 WHERE id = ?2",
                params![claims.len() as i64, run_id],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn run_claims(&self, run_id: &str) -> Result<Vec<Value>> {
        self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT id, text, tier, confidence, independent_count, agreement,
                        high_stakes, verdict_json
                 FROM claims WHERE run_id = ?1 ORDER BY ord",
            )?;
            let claims: Vec<(String, Value)> = st
                .query_map([run_id], |r| {
                    let id: String = r.get(0)?;
                    Ok((
                        id.clone(),
                        serde_json::json!({
                            "id": id,
                            "text": r.get::<_, String>(1)?,
                            "tier": r.get::<_, String>(2)?,
                            "confidence": r.get::<_, f64>(3)?,
                            "independent_count": r.get::<_, i64>(4)?,
                            "agreement": r.get::<_, f64>(5)?,
                            "high_stakes": r.get::<_, i64>(6)? != 0,
                            "verdict": serde_json::from_str::<Value>(&r.get::<_, String>(7)?)
                                .unwrap_or(Value::Null),
                        }),
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            let mut bind =
                conn.prepare("SELECT evidence_id, stance FROM claim_evidence WHERE claim_id = ?1")?;
            let mut out = Vec::with_capacity(claims.len());
            for (id, mut json) in claims {
                let mut supports = Vec::new();
                let mut refutes = Vec::new();
                let rows = bind.query_map([&id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?;
                for (ev, stance) in rows.flatten() {
                    if stance == "refutes" {
                        refutes.push(ev);
                    } else {
                        supports.push(ev);
                    }
                }
                json["supports"] = serde_json::to_value(supports).unwrap_or(Value::Null);
                json["refutes"] = serde_json::to_value(refutes).unwrap_or(Value::Null);
                out.push(json);
            }
            Ok(out)
        })
    }

    pub fn run_contradictions(&self, run_id: &str) -> Result<Vec<Value>> {
        self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT id, claim_a, claim_b, summary FROM contradictions WHERE run_id = ?1",
            )?;
            let rows = st.query_map([run_id], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "claim_a": r.get::<_, String>(1)?,
                    "claim_b": r.get::<_, String>(2)?,
                    "summary": r.get::<_, String>(3)?,
                }))
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        })
    }

    // --- corpus -----------------------------------------------------------

    /// Store a document and index its chunks. Returns `(doc_id, chunk_count)`.
    ///
    /// One transaction: a document row whose chunks failed to index would be
    /// listed in the UI while matching nothing — the exact "looks fine, finds
    /// nothing" failure this app exists to prevent.
    pub fn add_document(
        &self,
        name: &str,
        mime: &str,
        bytes: usize,
        sha256: &str,
        chunks: &[String],
    ) -> Result<(String, usize)> {
        let doc_id = new_id("doc");
        let uploaded = now();
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO corpus_docs (id, name, mime, bytes, sha256, status, uploaded_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'indexed', ?6)",
                params![doc_id, name, mime, bytes as i64, sha256, uploaded],
            )?;
            for (ord, text) in chunks.iter().enumerate() {
                let chunk_id = new_id("ch");
                tx.execute(
                    "INSERT INTO corpus_chunks (id, doc_id, ord, page, text)
                     VALUES (?1, ?2, ?3, NULL, ?4)",
                    params![chunk_id, doc_id, ord as i64, text],
                )?;
                tx.execute(
                    "INSERT INTO corpus_fts (chunk_id, text) VALUES (?1, ?2)",
                    params![chunk_id, text],
                )?;
            }
            tx.commit()?;
            Ok(())
        })?;
        Ok((doc_id, chunks.len()))
    }

    /// A document already stored with the same content hash, if any.
    pub fn document_by_hash(&self, sha256: &str) -> Result<Option<(String, String)>> {
        self.with_conn(|conn| {
            let mut st =
                conn.prepare("SELECT id, name FROM corpus_docs WHERE sha256 = ?1 LIMIT 1")?;
            let mut rows = st.query([sha256])?;
            Ok(match rows.next()? {
                Some(r) => Some((r.get(0)?, r.get(1)?)),
                None => None,
            })
        })
    }

    pub fn list_documents(&self) -> Result<Vec<Value>> {
        self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT d.id, d.name, d.mime, d.bytes, d.status, d.uploaded_at,
                        (SELECT COUNT(*) FROM corpus_chunks c WHERE c.doc_id = d.id)
                 FROM corpus_docs d ORDER BY d.uploaded_at DESC",
            )?;
            let rows = st.query_map([], |r| {
                Ok(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "mime": r.get::<_, String>(2)?,
                    "bytes": r.get::<_, i64>(3)?,
                    "status": r.get::<_, String>(4)?,
                    "uploaded_at": r.get::<_, String>(5)?,
                    "chunks": r.get::<_, i64>(6)?,
                }))
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        })
    }

    /// Delete a document, its chunks and its FTS rows.
    ///
    /// `corpus_fts` is an external-content-free FTS5 table with no foreign key,
    /// so `ON DELETE CASCADE` does not reach it. Deleting the doc without this
    /// would leave orphan rows that keep matching queries and cite a document
    /// that no longer exists.
    pub fn delete_document(&self, doc_id: &str) -> Result<bool> {
        self.with_conn_mut(|conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "DELETE FROM corpus_fts WHERE chunk_id IN
                   (SELECT id FROM corpus_chunks WHERE doc_id = ?1)",
                [doc_id],
            )?;
            let n = tx.execute("DELETE FROM corpus_docs WHERE id = ?1", [doc_id])?;
            tx.commit()?;
            Ok(n > 0)
        })
    }

    /// Full-text search over indexed chunks. `match_expr` must already be a
    /// safe FTS5 expression (see `corpus::fts_query`).
    pub fn search_corpus(&self, match_expr: &str, limit: usize) -> Result<Vec<CorpusHit>> {
        self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT c.id, c.doc_id, d.name, c.ord, c.text, bm25(corpus_fts) AS score
                 FROM corpus_fts f
                 JOIN corpus_chunks c ON c.id = f.chunk_id
                 JOIN corpus_docs   d ON d.id = c.doc_id
                 WHERE corpus_fts MATCH ?1
                 ORDER BY score
                 LIMIT ?2",
            )?;
            let rows = st.query_map(params![match_expr, limit as i64], |r| {
                Ok(CorpusHit {
                    chunk_id: r.get(0)?,
                    doc_id: r.get(1)?,
                    doc_name: r.get(2)?,
                    ord: r.get::<_, i64>(3)? as usize,
                    text: r.get(4)?,
                    // bm25 is negative, lower = better. Flip it so callers can
                    // treat every raw_score as "higher is better".
                    score: -r.get::<_, f64>(5)? as f32,
                })
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        })
    }

    // --- user-registered MCP sources --------------------------------------

    /// Every stored spec, paired with its enabled flag.
    ///
    /// A spec that no longer deserializes (hand-edited, or written by an older
    /// build) is skipped with a warning rather than failing boot — one bad row
    /// must not take down every other source.
    pub fn list_mcp_sources(&self) -> Result<Vec<(McpSourceSpec, bool)>> {
        self.with_conn(|conn| {
            let mut st =
                conn.prepare("SELECT id, spec_json, enabled FROM mcp_sources ORDER BY created_at")?;
            let rows = st.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)? != 0,
                ))
            })?;
            let mut out = Vec::new();
            for row in rows.flatten() {
                match serde_json::from_str::<McpSourceSpec>(&row.1) {
                    Ok(spec) => out.push((spec, row.2)),
                    Err(e) => eprintln!("[search] bỏ qua nguồn MCP `{}` (spec hỏng): {e}", row.0),
                }
            }
            Ok(out)
        })
    }

    pub fn save_mcp_source(&self, spec: &McpSourceSpec, enabled: bool) -> Result<()> {
        let json = serde_json::to_string(spec)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO mcp_sources (id, spec_json, enabled, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET
                   spec_json = excluded.spec_json,
                   enabled   = excluded.enabled",
                params![spec.id, json, enabled as i64, now()],
            )?;
            Ok(())
        })
    }

    pub fn delete_mcp_source(&self, id: &str) -> Result<bool> {
        self.with_conn(|conn| {
            let n = conn.execute("DELETE FROM mcp_sources WHERE id = ?1", [id])?;
            Ok(n > 0)
        })
    }

    pub fn stats(&self) -> Result<Value> {
        self.with_conn(|conn| {
            let runs: i64 = conn.query_row("SELECT COUNT(*) FROM runs", [], |r| r.get(0))?;
            let evidence: i64 =
                conn.query_row("SELECT COUNT(*) FROM evidence", [], |r| r.get(0))?;
            let reports: i64 = conn.query_row("SELECT COUNT(*) FROM reports", [], |r| r.get(0))?;
            let docs: i64 = conn.query_row("SELECT COUNT(*) FROM corpus_docs", [], |r| r.get(0))?;
            Ok(serde_json::json!({
                "runs": runs, "evidence": evidence, "reports": reports, "corpus_docs": docs
            }))
        })
    }

    /// Persist a synthesized report against its run. Each save bumps `version`
    /// so re-running synthesis on the same run keeps a history rather than
    /// silently overwriting the previous write.
    pub fn save_report(
        &self,
        run_id: &str,
        title: &str,
        body_md: &str,
        body_json: &Value,
    ) -> Result<(String, i64)> {
        let id = new_id("rep");
        let created = now();
        self.with_conn_mut(|conn| {
            let version: i64 = conn
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) + 1 FROM reports WHERE run_id = ?1",
                    [run_id],
                    |r| r.get(0),
                )
                .unwrap_or(1);
            conn.execute(
                "INSERT INTO reports (id, run_id, version, title, body_md, body_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    run_id,
                    version,
                    title,
                    body_md,
                    body_json.to_string(),
                    created
                ],
            )?;
            Ok((id.clone(), version))
        })
    }

    /// Read the latest report for a run, with its run summary, claims and
    /// contradictions folded in — everything a reader needs to trust it.
    pub fn get_report(&self, run_id: &str) -> Result<Option<Value>> {
        let report = self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT id, version, title, body_md, body_json, created_at
                 FROM reports WHERE run_id = ?1 ORDER BY version DESC LIMIT 1",
            )?;
            let mut rows = st.query([run_id])?;
            Ok(match rows.next()? {
                Some(r) => Some(serde_json::json!({
                    "id": r.get::<_, String>(0)?,
                    "run_id": run_id,
                    "version": r.get::<_, i64>(1)?,
                    "title": r.get::<_, String>(2)?,
                    "body_md": r.get::<_, String>(3)?,
                    "body_json": serde_json::from_str::<Value>(&r.get::<_, String>(4)?)
                        .unwrap_or(Value::Null),
                    "created_at": r.get::<_, String>(5)?,
                })),
                None => None,
            })
        })?;
        let Some(mut report) = report else {
            return Ok(None);
        };
        if let Ok(Some(run)) = self.get_run(run_id) {
            report["query"] = run.get("query").cloned().unwrap_or(Value::Null);
            report["run"] = run;
        }
        report["claims"] = serde_json::to_value(self.run_claims(run_id)?)?;
        report["contradictions"] = serde_json::to_value(self.run_contradictions(run_id)?)?;
        Ok(Some(report))
    }

    /// Recent reports (latest version each), newest first — for the reports list.
    pub fn list_reports(&self, limit: usize) -> Result<Vec<Value>> {
        self.with_conn(|conn| {
            let mut st = conn.prepare(
                "SELECT r.run_id, r.title, r.version, r.created_at, runs.query
                 FROM reports r
                 JOIN runs ON runs.id = r.run_id
                 WHERE r.version = (SELECT MAX(version) FROM reports WHERE run_id = r.run_id)
                 ORDER BY r.created_at DESC LIMIT ?1",
            )?;
            let rows = st.query_map([limit as i64], |r| {
                Ok(serde_json::json!({
                    "run_id": r.get::<_, String>(0)?,
                    "title": r.get::<_, String>(1)?,
                    "version": r.get::<_, i64>(2)?,
                    "created_at": r.get::<_, String>(3)?,
                    "query": r.get::<_, String>(4)?,
                }))
            })?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Evidence, SourceKind};
    use crate::pipeline::SearchOutcome;

    fn db() -> Db {
        Db::open(":memory:").expect("open in-memory db")
    }

    fn outcome() -> SearchOutcome {
        SearchOutcome {
            query: "lãi suất".into(),
            evidence: vec![Evidence::new(
                "web",
                SourceKind::Web,
                0,
                1.0,
                "Tiêu đề",
                "đoạn trích",
                Some("https://example.com/a".into()),
            )],
            sources: vec![
                SourceOutcome {
                    source_id: "web".into(),
                    sub_query: "lãi suất".into(),
                    status: "ok".into(),
                    item_count: 1,
                    dropped_count: 0,
                    ms: 12,
                    error: None,
                },
                SourceOutcome {
                    source_id: "wiki".into(),
                    sub_query: "lãi suất".into(),
                    status: "skipped".into(),
                    item_count: 0,
                    dropped_count: 0,
                    ms: 1,
                    error: Some("wiki không phản hồi".into()),
                },
            ],
            unknown_sources: vec![],
            total_before_dedupe: 3,
            deepened: 0,
            ms: 40,
        }
    }

    #[test]
    fn a_saved_run_round_trips_with_its_evidence_and_source_outcomes() {
        let db = db();
        let id = db
            .save_run(&outcome(), &serde_json::json!({ "depth": 1 }), "cited")
            .unwrap();
        let run = db.get_run(&id).unwrap().expect("run exists");
        assert_eq!(run["query"], "lãi suất");
        assert_eq!(run["evidence"].as_array().unwrap().len(), 1);
        assert_eq!(run["evidence"][0]["title"], "Tiêu đề");
        assert_eq!(run["sources"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn a_skipped_source_survives_persistence_with_its_reason() {
        // The whole point of run_sources: a degraded run must stay legible.
        let db = db();
        let id = db
            .save_run(&outcome(), &serde_json::json!({}), "cited")
            .unwrap();
        let sources = db.run_sources(&id).unwrap();
        let wiki = sources.iter().find(|s| s.source_id == "wiki").unwrap();
        assert_eq!(wiki.status, "skipped");
        assert_eq!(wiki.error.as_deref(), Some("wiki không phản hồi"));
    }

    #[test]
    fn deleting_a_run_cascades_to_its_evidence() {
        let db = db();
        let id = db
            .save_run(&outcome(), &serde_json::json!({}), "cited")
            .unwrap();
        assert!(db.delete_run(&id).unwrap());
        assert!(db.get_run(&id).unwrap().is_none());
        assert!(db.run_evidence(&id).unwrap().is_empty());
    }

    #[test]
    fn partial_source_config_updates_do_not_blank_other_fields() {
        let db = db();
        db.save_source_config("web", Some(false), Some(2.0), Some(15), Some(9000))
            .unwrap();
        db.save_source_config("web", Some(true), None, None, None)
            .unwrap();
        let cfg = db.load_source_config().unwrap();
        let (_, enabled, weight, max_results, timeout) =
            cfg.into_iter().find(|c| c.0 == "web").unwrap();
        assert_eq!(enabled, Some(true));
        assert_eq!(weight, Some(2.0), "weight must survive a partial update");
        assert_eq!(max_results, Some(15));
        assert_eq!(timeout, Some(9000));
    }

    fn add_doc(db: &Db, name: &str, text: &str) -> String {
        let chunks = crate::corpus::chunk(text);
        db.add_document(name, "text/plain", text.len(), name, &chunks)
            .unwrap()
            .0
    }

    fn find(db: &Db, query: &str) -> Vec<CorpusHit> {
        let expr = crate::corpus::fts_query(query).expect("query has tokens");
        db.search_corpus(&expr, 10).unwrap()
    }

    #[test]
    fn an_indexed_document_is_findable() {
        let db = db();
        add_doc(
            &db,
            "báo cáo.txt",
            "Lãi suất điều hành giảm còn 4,5% trong quý ba.",
        );
        let hits = find(&db, "lãi suất");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_name, "báo cáo.txt");
        assert!(hits[0].text.contains("4,5%"));
    }

    #[test]
    fn vietnamese_search_works_without_diacritics() {
        // This is the whole reason corpus_fts declares `remove_diacritics 2`.
        // Without it, "lai suat" finds nothing and users conclude the document
        // was never indexed.
        let db = db();
        add_doc(&db, "d.txt", "Lãi suất điều hành giảm.");
        assert_eq!(
            find(&db, "lai suat").len(),
            1,
            "unaccented query must match"
        );
        assert_eq!(find(&db, "LÃI SUẤT").len(), 1, "case must not matter");
    }

    #[test]
    fn a_query_that_matches_nothing_returns_empty_not_an_error() {
        let db = db();
        add_doc(&db, "d.txt", "Nội dung về vàng.");
        assert!(find(&db, "hoàn toàn khác biệt xyz").is_empty());
    }

    #[test]
    fn fts_operators_in_a_query_do_not_blow_up_the_match() {
        // Raw interpolation of any of these is a SQLite syntax error.
        let db = db();
        add_doc(&db, "d.txt", "Giá vàng SJC hôm nay.");
        for raw in [
            "giá \"vàng\"",
            "vàng - SJC",
            "vàng AND SJC",
            "vàng*",
            "(vàng",
        ] {
            let expr = crate::corpus::fts_query(raw).unwrap();
            let hits = db.search_corpus(&expr, 10);
            assert!(hits.is_ok(), "`{raw}` produced a broken MATCH: {hits:?}");
            assert!(
                !hits.unwrap().is_empty(),
                "`{raw}` should still find the doc"
            );
        }
    }

    #[test]
    fn deleting_a_document_also_clears_its_fts_rows() {
        // corpus_fts has no foreign key, so CASCADE does not reach it. Orphan
        // rows would keep matching and cite a document that no longer exists.
        let db = db();
        let id = add_doc(&db, "d.txt", "Lãi suất điều hành giảm.");
        assert_eq!(find(&db, "lãi suất").len(), 1);
        assert!(db.delete_document(&id).unwrap());
        assert!(
            find(&db, "lãi suất").is_empty(),
            "orphan FTS rows left behind"
        );
        assert!(db.list_documents().unwrap().is_empty());
    }

    #[test]
    fn a_multi_chunk_document_indexes_every_chunk() {
        let db = db();
        let long = (0..40)
            .map(|i| format!("Đoạn {i}: nội dung về thị trường vàng và lãi suất."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let id = add_doc(&db, "dài.txt", &long);
        let docs = db.list_documents().unwrap();
        let chunks = docs.iter().find(|d| d["id"] == id.as_str()).unwrap()["chunks"]
            .as_i64()
            .unwrap();
        assert!(chunks > 1, "long document must produce several chunks");
    }

    #[test]
    fn the_same_content_is_detected_by_hash() {
        let db = db();
        let text = "Nội dung trùng lặp.";
        db.add_document(
            "a.txt",
            "text/plain",
            10,
            "hash-abc",
            &crate::corpus::chunk(text),
        )
        .unwrap();
        let found = db.document_by_hash("hash-abc").unwrap();
        assert_eq!(found.map(|(_, name)| name), Some("a.txt".to_string()));
        assert_eq!(db.document_by_hash("hash-other").unwrap(), None);
    }

    fn mcp_spec(id: &str) -> McpSourceSpec {
        use crate::sources::mcp_source::{FieldMap, McpTarget};
        McpSourceSpec {
            id: id.into(),
            label: "Threads".into(),
            kind: SourceKind::Social,
            weight: 1.4,
            target: McpTarget::App {
                app_id: "social".into(),
            },
            tool: "social_search".into(),
            query_arg: "query".into(),
            limit_arg: Some("limit".into()),
            extra_args: serde_json::json!({ "platform": "threads", "handle": "@me" }),
            map: FieldMap {
                url_template: Some("https://threads.net/{id}".into()),
                ..Default::default()
            },
        }
    }

    #[test]
    fn a_registered_mcp_source_survives_a_restart_intact() {
        // The spec is stored as one JSON blob; a field lost here would come
        // back as a silently different source after the next restart.
        let db = db();
        db.save_mcp_source(&mcp_spec("social:threads"), true)
            .unwrap();
        let rows = db.list_mcp_sources().unwrap();
        assert_eq!(rows.len(), 1);
        let (spec, enabled) = &rows[0];
        assert!(enabled);
        assert_eq!(spec.tool, "social_search");
        assert_eq!(spec.extra_args["handle"], "@me");
        assert_eq!(spec.limit_arg.as_deref(), Some("limit"));
        assert_eq!(spec.weight, 1.4);
        assert_eq!(
            spec.map.url_template.as_deref(),
            Some("https://threads.net/{id}")
        );
    }

    #[test]
    fn re_registering_the_same_id_updates_rather_than_duplicates() {
        let db = db();
        db.save_mcp_source(&mcp_spec("x"), true).unwrap();
        let mut changed = mcp_spec("x");
        changed.tool = "other_search".into();
        db.save_mcp_source(&changed, false).unwrap();
        let rows = db.list_mcp_sources().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.tool, "other_search");
        assert!(!rows[0].1);
    }

    #[test]
    fn one_corrupt_spec_does_not_hide_the_healthy_ones() {
        // A hand-edited or older-format row must not take the whole list down.
        let db = db();
        db.save_mcp_source(&mcp_spec("good"), true).unwrap();
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO mcp_sources (id, spec_json, enabled, created_at)
                 VALUES ('broken', '{not json', 1, '2026-07-20')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let rows = db.list_mcp_sources().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.id, "good");
    }

    #[test]
    fn removing_a_source_reports_whether_it_existed() {
        let db = db();
        db.save_mcp_source(&mcp_spec("x"), true).unwrap();
        assert!(db.delete_mcp_source("x").unwrap());
        assert!(!db.delete_mcp_source("x").unwrap());
    }

    #[test]
    fn missing_runs_return_none_rather_than_erroring() {
        assert!(db().get_run("run_nope").unwrap().is_none());
    }
}
