-- Rewrite Story — SQLite schema.
--
-- Ported from the Go/Postgres original (`re-write-story/backend/models`). Deliberate
-- divergences from that schema, each for a reason:
--
--   * No `user_id` anywhere. Space Apps are single-user and local; JWT/auth is gone.
--   * `rewrite_chunks`, not `rewrite_store_chunk`. The Go table name is a typo
--     (`store`, and unpluralized) pinned by a `TableName()` override. Fresh DB, no
--     migration burden, so it is spelled correctly here.
--   * `UNIQUE(process_id, chunk_index)`. Go used `db.Save` on a zero-ID record, so a
--     re-run that raced the resume-skip logic inserted duplicate chunks and the final
--     concatenation silently doubled text. The constraint makes that impossible.
--   * Lengths are in **characters**, not bytes. Go's `len()` on UTF-8 Vietnamese runs
--     ~1.33x the character count (measured against the project's own corpus).
--   * Dropped as vestigial (persisted but never read by the rewrite job): `chunk_size`,
--     `rewrite_source`, `cognee_dataset_name`, `cognee_saved`, `cache_name`.
--
-- Chunking parameters live in `app_settings` rather than on the process: source chunks
-- are cached per story and shared across every rewrite run of it, so the split is a
-- story-level fact, not a per-run one.

CREATE TABLE IF NOT EXISTS stories (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    name                   TEXT    NOT NULL,
    -- Self-reference builds the version tree: NULL = an imported original.
    parent_story_id        INTEGER REFERENCES stories(id) ON DELETE CASCADE,
    version_number         INTEGER NOT NULL DEFAULT 1,
    original_text          TEXT    NOT NULL,
    original_length        INTEGER NOT NULL DEFAULT 0,
    -- 'human' = imported by the user, 'ai' = produced by a rewrite process.
    source_type            TEXT    NOT NULL DEFAULT 'human'
                                   CHECK (source_type IN ('human', 'ai')),
    -- Settings the rewrite that produced this version ran with (NULL for originals).
    creativity_ratio       INTEGER,
    target_length_variance INTEGER,
    processing_time        REAL,
    created_at             TEXT    NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_stories_parent  ON stories(parent_story_id);
CREATE INDEX IF NOT EXISTS idx_stories_created ON stories(created_at DESC);

-- Source-text chunks, produced once per story by the hybrid splitter and reused by
-- every rewrite run of that story.
CREATE TABLE IF NOT EXISTS story_chunks (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    story_id    INTEGER NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    content     TEXT    NOT NULL,
    UNIQUE (story_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_story_chunks_story ON story_chunks(story_id, chunk_index);

CREATE TABLE IF NOT EXISTS rewrite_processes (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    story_id               INTEGER NOT NULL REFERENCES stories(id) ON DELETE CASCADE,

    status                 TEXT    NOT NULL DEFAULT 'queued'
                                   CHECK (status IN ('queued', 'processing', 'completed',
                                                     'failed', 'cancelled')),
    current_stage          TEXT    NOT NULL DEFAULT 'pending',
    progress_percentage    INTEGER NOT NULL DEFAULT 0,
    total_chunks           INTEGER NOT NULL DEFAULT 0,
    current_chunk          INTEGER NOT NULL DEFAULT 0,
    error_message          TEXT,

    -- Rewrite parameters.
    creativity_ratio       INTEGER NOT NULL DEFAULT 40,
    target_length_variance INTEGER NOT NULL DEFAULT 5,
    -- Sent to the model as a real system prompt. In Go this reached the model only
    -- through Gemini's context cache, so a cache failure dropped it silently.
    system_instruction     TEXT,
    user_prompt            TEXT,
    version_plan           TEXT,
    -- Empty = let the SenClaw daemon pick the model.
    model                  TEXT,

    result_story_id        INTEGER REFERENCES stories(id) ON DELETE SET NULL,

    created_at             TEXT    NOT NULL DEFAULT (datetime('now')),
    updated_at             TEXT    NOT NULL DEFAULT (datetime('now')),
    started_at             TEXT,
    completed_at           TEXT
);

CREATE INDEX IF NOT EXISTS idx_processes_status  ON rewrite_processes(status, created_at);
CREATE INDEX IF NOT EXISTS idx_processes_story   ON rewrite_processes(story_id);
CREATE INDEX IF NOT EXISTS idx_processes_updated ON rewrite_processes(updated_at);

-- Per-chunk rewrite output, persisted as each chunk completes. This is what makes
-- retry a *resume* rather than a restart.
CREATE TABLE IF NOT EXISTS rewrite_chunks (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    process_id        INTEGER NOT NULL REFERENCES rewrite_processes(id) ON DELETE CASCADE,
    chunk_index       INTEGER NOT NULL,
    original_content  TEXT    NOT NULL,
    rewritten_content TEXT    NOT NULL,
    created_at        TEXT    NOT NULL DEFAULT (datetime('now')),
    UNIQUE (process_id, chunk_index)
);

CREATE INDEX IF NOT EXISTS idx_rewrite_chunks_process
    ON rewrite_chunks(process_id, chunk_index);

CREATE TABLE IF NOT EXISTS app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Splitter defaults, in characters.
--
-- `max_size` is bounded by what the model will actually write back, not by what
-- the Go config used (20000 bytes). Bigger chunks do not produce proportionally
-- bigger rewrites — the model condenses — so oversized chunks lose content
-- silently. See `llm::MAX_CHUNK_CHARS`.
INSERT OR IGNORE INTO app_settings (key, value) VALUES
    ('hybrid_split_min_size',  '1200'),
    ('hybrid_split_max_size',  '2000'),
    ('hybrid_split_threshold', '0.2'),
    ('default_creativity_ratio', '40'),
    ('default_length_variance',  '5'),
    ('max_concurrent_processes', '2'),
    -- Chunks rewritten concurrently within ONE story. 1 = strictly sequential,
    -- every chunk continuing from the rewritten tail of its predecessor. Above 1
    -- the in-flight siblings fall back to the source tail for continuity, which
    -- trades a little seam quality for a near-linear speedup on long novels.
    ('parallel_chunks', '1'),
    -- Output-token budget per chunk. Measured against the daemon bridge: the
    -- returned length tracks this almost linearly, so a low value silently
    -- yields a summary instead of a rewrite. See `llm::DEFAULT_MAX_OUTPUT_TOKENS`.
    ('max_output_tokens', '32000');
