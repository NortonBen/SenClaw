-- Search app schema.
--
-- P0 persists runs, per-source outcomes and evidence. Claims, contradictions,
-- reports, corpus and generic MCP sources arrive in P1–P3; their tables are
-- created here so the shape is fixed and migrations stay additive.

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS runs (
  id                  TEXT PRIMARY KEY,
  query               TEXT NOT NULL,
  params_json         TEXT NOT NULL DEFAULT '{}',
  -- running | ok | error
  status              TEXT NOT NULL DEFAULT 'running',
  depth               INTEGER NOT NULL DEFAULT 1,
  -- cited | corroborate | adversarial
  verify_level        TEXT NOT NULL DEFAULT 'cited',
  evidence_count      INTEGER NOT NULL DEFAULT 0,
  claim_count         INTEGER NOT NULL DEFAULT 0,
  total_before_dedupe INTEGER NOT NULL DEFAULT 0,
  deepened            INTEGER NOT NULL DEFAULT 0,
  ms                  INTEGER NOT NULL DEFAULT 0,
  error               TEXT,
  created_at          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_runs_created ON runs(created_at DESC);

-- One row per (source × sub-query). This is the run's honesty record: a source
-- that timed out, errored or was skipped is visible here instead of the run
-- just looking thin.
CREATE TABLE IF NOT EXISTS run_sources (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id        TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  source_id     TEXT NOT NULL,
  sub_query     TEXT NOT NULL,
  -- ok | timeout | error | skipped
  status        TEXT NOT NULL,
  item_count    INTEGER NOT NULL DEFAULT 0,
  dropped_count INTEGER NOT NULL DEFAULT 0,
  ms            INTEGER NOT NULL DEFAULT 0,
  error         TEXT
);
CREATE INDEX IF NOT EXISTS idx_run_sources_run ON run_sources(run_id);

CREATE TABLE IF NOT EXISTS evidence (
  id                  TEXT NOT NULL,
  run_id              TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  title               TEXT NOT NULL DEFAULT '',
  url                 TEXT,
  canonical_url       TEXT,
  domain              TEXT,
  snippet             TEXT NOT NULL DEFAULT '',
  full_text           TEXT,
  author              TEXT,
  published_at        INTEGER,
  retrieved_at        INTEGER NOT NULL,
  lang                TEXT,
  meta_json           TEXT NOT NULL DEFAULT 'null',
  -- provenance: [{source_id, kind, rank, raw_score}]
  hits_json           TEXT NOT NULL DEFAULT '[]',
  fused_score         REAL NOT NULL DEFAULT 0,
  independent_kinds   INTEGER NOT NULL DEFAULT 0,
  independent_domains INTEGER NOT NULL DEFAULT 0,
  ord                 INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (run_id, id)
);
CREATE INDEX IF NOT EXISTS idx_evidence_run ON evidence(run_id, ord);

-- ---------------------------------------------------------------------------
-- P2+ — created now so the shape is settled.
-- ---------------------------------------------------------------------------

CREATE TABLE IF NOT EXISTS claims (
  id                TEXT PRIMARY KEY,
  run_id            TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  text              TEXT NOT NULL,
  -- verified | supported | single-source | disputed | unverified
  tier              TEXT NOT NULL DEFAULT 'unverified',
  confidence        REAL NOT NULL DEFAULT 0,
  independent_count INTEGER NOT NULL DEFAULT 0,
  agreement         REAL NOT NULL DEFAULT 0,
  high_stakes       INTEGER NOT NULL DEFAULT 0,
  verdict_json      TEXT NOT NULL DEFAULT 'null',
  ord               INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_claims_run ON claims(run_id, ord);

CREATE TABLE IF NOT EXISTS claim_evidence (
  claim_id    TEXT NOT NULL REFERENCES claims(id) ON DELETE CASCADE,
  evidence_id TEXT NOT NULL,
  run_id      TEXT NOT NULL,
  -- supports | refutes
  stance      TEXT NOT NULL DEFAULT 'supports',
  PRIMARY KEY (claim_id, evidence_id)
);

CREATE TABLE IF NOT EXISTS contradictions (
  id      TEXT PRIMARY KEY,
  run_id  TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  claim_a TEXT NOT NULL,
  claim_b TEXT NOT NULL,
  summary TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS reports (
  id         TEXT PRIMARY KEY,
  run_id     TEXT NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
  version    INTEGER NOT NULL DEFAULT 1,
  title      TEXT NOT NULL DEFAULT '',
  body_md    TEXT NOT NULL DEFAULT '',
  body_json  TEXT NOT NULL DEFAULT 'null',
  created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_reports_run ON reports(run_id, version DESC);

CREATE TABLE IF NOT EXISTS corpus_docs (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  mime        TEXT NOT NULL DEFAULT '',
  bytes       INTEGER NOT NULL DEFAULT 0,
  sha256      TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'pending',
  uploaded_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS corpus_chunks (
  id     TEXT PRIMARY KEY,
  doc_id TEXT NOT NULL REFERENCES corpus_docs(id) ON DELETE CASCADE,
  ord    INTEGER NOT NULL DEFAULT 0,
  page   INTEGER,
  text   TEXT NOT NULL
);

-- remove_diacritics 2 so "lai suat" finds "lãi suất" — the wiki uses this and
-- memory does not, which is why the same query behaves differently across them.
CREATE VIRTUAL TABLE IF NOT EXISTS corpus_fts USING fts5(
  chunk_id UNINDEXED,
  text,
  tokenize='unicode61 remove_diacritics 2'
);

-- User-registered MCP sources: any MCP tool becomes a search source with no code.
--
-- The spec is stored as one JSON blob rather than a column per field. The shape
-- of `McpSourceSpec` will keep growing (new mapper options, new arg handling),
-- and a JSON column absorbs that additively instead of needing a migration per
-- field. `id` / `enabled` stay real columns because they are queried.
CREATE TABLE IF NOT EXISTS mcp_sources (
  id         TEXT PRIMARY KEY,
  spec_json  TEXT NOT NULL,
  enabled    INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL
);

-- Per-source overrides that survive restarts.
CREATE TABLE IF NOT EXISTS source_config (
  source_id    TEXT PRIMARY KEY,
  enabled      INTEGER,
  weight       REAL,
  max_results  INTEGER,
  timeout_ms   INTEGER,
  options_json TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE IF NOT EXISTS monitors (
  id          TEXT PRIMARY KEY,
  query       TEXT NOT NULL,
  params_json TEXT NOT NULL DEFAULT '{}',
  cron        TEXT NOT NULL DEFAULT '',
  last_run_id TEXT,
  notify_json TEXT NOT NULL DEFAULT '{}',
  enabled     INTEGER NOT NULL DEFAULT 1,
  created_at  TEXT NOT NULL
);
