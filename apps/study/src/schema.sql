-- SenClaw Study — schema.
--
-- Ids are TEXT uuids everywhere except `chunks`, which uses INTEGER so it can
-- back an FTS5 external-content table by rowid.

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

-- ── Documents ───────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS docs (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    filename    TEXT NOT NULL,
    ext         TEXT NOT NULL,
    bytes       INTEGER NOT NULL DEFAULT 0,
    chars       INTEGER NOT NULL DEFAULT 0,
    extract_note TEXT,
    body        TEXT NOT NULL,          -- full extracted text; char offsets index into this
    summary     TEXT,                   -- whole-document synthesis (LLM)
    status      TEXT NOT NULL DEFAULT 'new',  -- new | outlined | enriched | error
    error       TEXT,
    -- Repeated short lines the cleaner flagged but did NOT remove, as JSON.
    -- The user confirms which are page furniture (see docs/:id/strip-lines).
    suspects    TEXT,
    added_at    INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

-- ── Sections (the unit a study session schedules) ───────────────────────────

CREATE TABLE IF NOT EXISTS sections (
    id          TEXT PRIMARY KEY,
    doc_id      TEXT NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
    ord         INTEGER NOT NULL,
    title       TEXT NOT NULL,
    level       INTEGER NOT NULL DEFAULT 1,
    char_start  INTEGER NOT NULL,
    char_end    INTEGER NOT NULL,
    summary     TEXT,
    key_points  TEXT,                   -- JSON array of strings
    difficulty  INTEGER NOT NULL DEFAULT 3,   -- 1..5
    est_minutes INTEGER NOT NULL DEFAULT 15,
    prereq      TEXT,                   -- JSON array of section ids
    enriched_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_sections_doc ON sections(doc_id, ord);

-- ── Chunks (the unit a citation points at) ──────────────────────────────────

CREATE TABLE IF NOT EXISTS chunks (
    id          INTEGER PRIMARY KEY,
    doc_id      TEXT NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
    section_id  TEXT,
    ord         INTEGER NOT NULL,
    char_start  INTEGER NOT NULL,
    char_end    INTEGER NOT NULL,
    text        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chunks_doc ON chunks(doc_id, ord);
CREATE INDEX IF NOT EXISTS idx_chunks_section ON chunks(section_id);

-- `fold` holds the search-normalised copy: FTS5's unicode61 strips combining
-- marks but leaves `đ` alone, so "dong" never matches "đông" without this.
-- Deliberately NOT an external-content table: a contentless FTS5 index can only
-- be deleted from by replaying the original column values, so deleting a
-- document would silently leave its rows searchable. Storing the folded copy
-- costs disk and buys correct deletes.
CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    fold,
    tokenize="unicode61 remove_diacritics 2"
);

-- ── Concepts ────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS concepts (
    id          TEXT PRIMARY KEY,
    doc_id      TEXT NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    norm        TEXT NOT NULL,          -- folded name, for dedupe
    created_at  INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_concepts_norm ON concepts(doc_id, norm);

CREATE TABLE IF NOT EXISTS concept_sections (
    concept_id  TEXT NOT NULL REFERENCES concepts(id) ON DELETE CASCADE,
    section_id  TEXT NOT NULL REFERENCES sections(id) ON DELETE CASCADE,
    PRIMARY KEY (concept_id, section_id)
);

-- ── Plans ───────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS plan_templates (
    key         TEXT PRIMARY KEY,
    label       TEXT NOT NULL,
    detail      TEXT,
    days        INTEGER NOT NULL,
    min_per_day INTEGER NOT NULL,
    review_offsets TEXT NOT NULL,       -- JSON array of day offsets
    blocks      TEXT NOT NULL,          -- JSON array of block kinds per session
    content_ratio REAL NOT NULL DEFAULT 0.7,
    builtin     INTEGER NOT NULL DEFAULT 1,
    sort        INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS plans (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    goal        TEXT,
    doc_ids     TEXT NOT NULL,          -- JSON array
    template_key TEXT,
    start_date  TEXT NOT NULL,          -- YYYY-MM-DD, local
    days        INTEGER NOT NULL,
    min_per_day INTEGER NOT NULL,
    weekdays    TEXT NOT NULL DEFAULT '1,2,3,4,5,6,7',
    slot_hm     TEXT NOT NULL DEFAULT '20:00',
    tz          TEXT NOT NULL DEFAULT 'Asia/Ho_Chi_Minh',
    status      TEXT NOT NULL DEFAULT 'active',  -- active | done | archived
    note        TEXT,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    plan_id     TEXT NOT NULL REFERENCES plans(id) ON DELETE CASCADE,
    ord         INTEGER NOT NULL,
    date        TEXT NOT NULL,          -- YYYY-MM-DD local
    start_hm    TEXT NOT NULL,
    minutes     INTEGER NOT NULL,
    title       TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'planned', -- planned | done | missed
    event_id    TEXT,                   -- space_events id in the daemon
    completed_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_sessions_plan ON sessions(plan_id, ord);
CREATE INDEX IF NOT EXISTS idx_sessions_date ON sessions(date);

CREATE TABLE IF NOT EXISTS session_items (
    id          TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    ord         INTEGER NOT NULL,
    kind        TEXT NOT NULL,          -- read | flashcard | quiz | review | recall
    section_id  TEXT,
    section_title TEXT NOT NULL DEFAULT '',
    est_minutes INTEGER NOT NULL DEFAULT 10,
    part        INTEGER NOT NULL DEFAULT 1,
    parts       INTEGER NOT NULL DEFAULT 1,
    done_at     INTEGER
);
CREATE INDEX IF NOT EXISTS idx_items_session ON session_items(session_id, ord);

-- ── Flashcards + SRS ────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS cards (
    id          TEXT PRIMARY KEY,
    doc_id      TEXT REFERENCES docs(id) ON DELETE CASCADE,
    section_id  TEXT,
    chunk_id    INTEGER,
    concept_id  TEXT,
    front       TEXT NOT NULL,
    back        TEXT NOT NULL,
    kind        TEXT NOT NULL DEFAULT 'qa',  -- qa | cloze | define
    source      TEXT NOT NULL DEFAULT 'ai',  -- ai | highlight | quiz-miss | manual
    created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cards_doc ON cards(doc_id);
CREATE INDEX IF NOT EXISTS idx_cards_section ON cards(section_id);

CREATE TABLE IF NOT EXISTS card_progress (
    card_id     TEXT PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    level       INTEGER NOT NULL DEFAULT 0,
    next_review TEXT NOT NULL,          -- RFC3339 UTC
    is_urgent   INTEGER NOT NULL DEFAULT 0,
    last_reviewed TEXT NOT NULL,
    first_due_at TEXT,
    reviews     INTEGER NOT NULL DEFAULT 0,
    lapses      INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_progress_due ON card_progress(next_review);

-- ── Quiz ────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS questions (
    id          TEXT PRIMARY KEY,
    doc_id      TEXT NOT NULL REFERENCES docs(id) ON DELETE CASCADE,
    section_id  TEXT,
    concept_id  TEXT,
    kind        TEXT NOT NULL,          -- single | multi | truefalse | cloze | match | order
    stem        TEXT NOT NULL,
    options     TEXT,                   -- JSON array
    answer      TEXT NOT NULL,          -- JSON (shape depends on kind)
    explain     TEXT,
    chunk_id    INTEGER NOT NULL,       -- verified: exists, and `quote` is inside it
    quote       TEXT NOT NULL,
    difficulty  INTEGER NOT NULL DEFAULT 3,
    created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_questions_doc ON questions(doc_id);
CREATE INDEX IF NOT EXISTS idx_questions_section ON questions(section_id);

CREATE TABLE IF NOT EXISTS attempts (
    id          TEXT PRIMARY KEY,
    question_id TEXT NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
    quiz_id     TEXT,
    session_id  TEXT,
    chosen      TEXT,                   -- JSON
    correct     INTEGER NOT NULL,
    answered_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_attempts_q ON attempts(question_id);
CREATE INDEX IF NOT EXISTS idx_attempts_quiz ON attempts(quiz_id);

-- ── Ask / research history ──────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS asks (
    id          TEXT PRIMARY KEY,
    question    TEXT NOT NULL,
    scope       TEXT,                   -- JSON {doc_ids?, section_id?}
    answer_md   TEXT NOT NULL,
    evidence    TEXT NOT NULL,          -- JSON array, index+1 == citation [n]
    external    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);

-- User-registered external MCP search sources. Built-ins are discovered at
-- call time from the daemon, so nothing here is required.
CREATE TABLE IF NOT EXISTS mcp_sources (
    key         TEXT PRIMARY KEY,
    label       TEXT NOT NULL,
    rpc_url     TEXT,
    app_id      TEXT,
    tool        TEXT NOT NULL,
    query_arg   TEXT,
    field_map   TEXT,                   -- JSON FieldMap
    weight      REAL NOT NULL DEFAULT 1.0,
    enabled     INTEGER NOT NULL DEFAULT 1
);

-- ── Misc ────────────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS tts_cache (
    hash        TEXT PRIMARY KEY,
    voice       TEXT,
    speed       REAL,
    path        TEXT NOT NULL,
    bytes       INTEGER NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    k           TEXT PRIMARY KEY,
    v           TEXT NOT NULL
);
