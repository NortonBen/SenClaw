-- Kaen — vocabulary SRS Space App. Idempotent schema, run on every boot.
--
-- Ported from kaizen's PostgreSQL/TypeORM schema, single-user edition:
-- no user_id anywhere. Space Apps are single-user and local; the whole
-- 26-column `users` table collapses into the one-row `settings` table.
-- All timestamps are UTC ISO-8601 TEXT ("YYYY-MM-DDTHH:MM:SS.mmmZ") — one
-- fixed format so lexicographic compare == chronological compare.

CREATE TABLE IF NOT EXISTS settings (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    native_language   TEXT    NOT NULL DEFAULT 'vi',
    -- JSON array of "HH:MM" study slots, e.g. ["08:00","20:00"]
    study_slots       TEXT    NOT NULL DEFAULT '["08:00","20:00"]',
    timezone          TEXT    NOT NULL DEFAULT 'Asia/Ho_Chi_Minh',
    daily_word_goal   INTEGER NOT NULL DEFAULT 10,
    current_streak    INTEGER NOT NULL DEFAULT 0,
    last_study_date   TEXT,
    total_xp          INTEGER NOT NULL DEFAULT 0,
    snooze_until      TEXT
);
INSERT OR IGNORE INTO settings (id) VALUES (1);

CREATE TABLE IF NOT EXISTS lessons (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    description TEXT,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS cards (
    id             TEXT PRIMARY KEY,
    lesson_id      TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    word           TEXT NOT NULL,
    image_url      TEXT,
    ipa            TEXT,
    part_of_speech TEXT,
    -- JSON array of example sentences
    examples       TEXT,
    -- English explanation (required in kaizen)
    explain        TEXT NOT NULL DEFAULT '',
    -- JSON object of per-language meanings, e.g. {"vi":"Quả táo"}
    meanings       TEXT
);
CREATE INDEX IF NOT EXISTS idx_cards_lesson ON cards(lesson_id);

-- kaizen's user_card_progress, keyed directly by card (single user).
CREATE TABLE IF NOT EXISTS card_progress (
    card_id              TEXT PRIMARY KEY REFERENCES cards(id) ON DELETE CASCADE,
    level                INTEGER NOT NULL DEFAULT 0,
    next_review          TEXT    NOT NULL,
    is_urgent            INTEGER NOT NULL DEFAULT 1,
    last_reviewed        TEXT    NOT NULL,
    first_due_at         TEXT,
    notification_sent_at TEXT,
    created_at           TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_progress_next_review ON card_progress(next_review);

-- kaizen's review_sessions: one row per practice sighting/answer, used to keep
-- a card out of Review/Matching/Listening/Writing pools for 24h.
CREATE TABLE IF NOT EXISTS review_sessions (
    id          TEXT PRIMARY KEY,
    card_id     TEXT NOT NULL REFERENCES cards(id) ON DELETE CASCADE,
    is_correct  INTEGER NOT NULL DEFAULT 0,
    reviewed_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_review_sessions_card_time ON review_sessions(card_id, reviewed_at);
CREATE INDEX IF NOT EXISTS idx_review_sessions_time ON review_sessions(reviewed_at);

-- ---- Grammar (Phase 2) ----

CREATE TABLE IF NOT EXISTS grammars (
    id            TEXT PRIMARY KEY,
    title         TEXT NOT NULL,
    content       TEXT NOT NULL DEFAULT '',
    description   TEXT,
    level         TEXT NOT NULL DEFAULT 'B1',
    thumbnail_url TEXT,
    view_count    INTEGER NOT NULL DEFAULT 0,
    idx           INTEGER NOT NULL DEFAULT 0,
    slug          TEXT NOT NULL UNIQUE,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS grammar_topics (
    id           TEXT PRIMARY KEY,
    name         TEXT NOT NULL,
    level        TEXT,
    description  TEXT,
    grammar_id   TEXT UNIQUE REFERENCES grammars(id) ON DELETE SET NULL,
    grammar_slug TEXT,
    created_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS grammar_questions (
    id                TEXT PRIMARY KEY,
    topic_id          TEXT NOT NULL REFERENCES grammar_topics(id) ON DELETE CASCADE,
    content           TEXT NOT NULL,
    -- JSON array [{"id":"A","text":"..."}]
    options           TEXT NOT NULL,
    correct_answer_id TEXT NOT NULL,
    explanation       TEXT,
    source            TEXT NOT NULL DEFAULT 'MANUAL',
    created_at        TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_grammar_questions_topic ON grammar_questions(topic_id);

CREATE TABLE IF NOT EXISTS grammar_test_sessions (
    id              TEXT PRIMARY KEY,
    topic_id        TEXT,
    score           INTEGER NOT NULL DEFAULT 0,
    total_questions INTEGER NOT NULL DEFAULT 0,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS grammar_test_results (
    id                 TEXT PRIMARY KEY,
    session_id         TEXT NOT NULL REFERENCES grammar_test_sessions(id) ON DELETE CASCADE,
    question_id        TEXT NOT NULL,
    selected_answer_id TEXT,
    is_correct         INTEGER NOT NULL DEFAULT 0,
    created_at         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_grammar_test_results_session ON grammar_test_results(session_id);

-- kaizen's user_grammar_progress, keyed by grammar (single user):
-- passed-test bookkeeping + 7-day review reminder.
CREATE TABLE IF NOT EXISTS grammar_progress (
    grammar_id       TEXT PRIMARY KEY REFERENCES grammars(id) ON DELETE CASCADE,
    first_passed_at  TEXT,
    last_test_at     TEXT,
    next_reminder_at TEXT,
    created_at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_grammar_progress_reminder ON grammar_progress(next_reminder_at);

-- ---- Story (Phase 3) ----

CREATE TABLE IF NOT EXISTS stories (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL,
    topic       TEXT,
    description TEXT,
    lesson_id   TEXT NOT NULL REFERENCES lessons(id) ON DELETE CASCADE,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS story_steps (
    id               TEXT PRIMARY KEY,
    story_id         TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    step_type        TEXT NOT NULL DEFAULT 'STEP1',
    primary_language TEXT NOT NULL DEFAULT 'en',
    content          TEXT NOT NULL,
    ord              INTEGER NOT NULL DEFAULT 1,
    audio_url        TEXT,
    UNIQUE(story_id, step_type)
);

CREATE TABLE IF NOT EXISTS story_progress (
    story_id           TEXT PRIMARY KEY REFERENCES stories(id) ON DELETE CASCADE,
    current_step       INTEGER NOT NULL DEFAULT 1,
    completed_steps    TEXT NOT NULL DEFAULT '[]',
    viewed_vocab_ids   TEXT NOT NULL DEFAULT '[]',
    listened_vocab_ids TEXT NOT NULL DEFAULT '[]',
    total_reading_time INTEGER NOT NULL DEFAULT 0,
    tts_sessions_count INTEGER NOT NULL DEFAULT 0,
    started_at         TEXT,
    last_accessed_at   TEXT,
    completed_at       TEXT,
    created_at         TEXT NOT NULL
);

-- ---- Dictation (Phase 3) ----

CREATE TABLE IF NOT EXISTS dictation_topics (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    slug        TEXT NOT NULL UNIQUE,
    description TEXT,
    level       TEXT,
    created_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS dictation_lessons (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    title            TEXT NOT NULL,
    topic            TEXT NOT NULL DEFAULT '',
    description      TEXT,
    level            TEXT,
    audio_url        TEXT,
    youtube_video_id TEXT,
    mode             TEXT NOT NULL DEFAULT 'dictation',
    topic_id         INTEGER REFERENCES dictation_topics(id) ON DELETE SET NULL,
    created_at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_dictation_lessons_topic ON dictation_lessons(topic_id);

CREATE TABLE IF NOT EXISTS dictation_segments (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    lesson_id   INTEGER NOT NULL REFERENCES dictation_lessons(id) ON DELETE CASCADE,
    content     TEXT,
    -- JSON array of accepted variant word-lists
    solutions   TEXT NOT NULL DEFAULT '[]',
    start_time  REAL NOT NULL DEFAULT 0,
    end_time    REAL NOT NULL DEFAULT 0,
    order_index INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_dictation_segments_lesson ON dictation_segments(lesson_id);

CREATE TABLE IF NOT EXISTS dictation_challenges (
    id        INTEGER PRIMARY KEY AUTOINCREMENT,
    lesson_id INTEGER NOT NULL REFERENCES dictation_lessons(id) ON DELETE CASCADE,
    options   TEXT,
    voices    TEXT
);

CREATE TABLE IF NOT EXISTS dictation_progress (
    lesson_id             INTEGER PRIMARY KEY REFERENCES dictation_lessons(id) ON DELETE CASCADE,
    current_index         INTEGER NOT NULL DEFAULT 0,
    completion_percentage INTEGER NOT NULL DEFAULT 0,
    -- JSON object { "0": "learned", "1": "marked" }
    segment_status        TEXT NOT NULL DEFAULT '{}',
    last_practiced_at     TEXT,
    created_at            TEXT NOT NULL
);

-- ---- Dictionary (Phase 3) ----

CREATE TABLE IF NOT EXISTS dictionary_entries (
    word           TEXT PRIMARY KEY,
    ipa            TEXT,
    part_of_speech TEXT,
    definition     TEXT,
    examples       TEXT,
    audio_url      TEXT,
    audio_us       TEXT,
    audio_uk       TEXT,
    created_at     TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS dictionary_translations (
    word        TEXT NOT NULL,
    target_lang TEXT NOT NULL,
    translation TEXT NOT NULL,
    PRIMARY KEY (word, target_lang)
);

CREATE TABLE IF NOT EXISTS study_logs (
    id                TEXT PRIMARY KEY,
    created_at        TEXT    NOT NULL,
    duration_seconds  INTEGER NOT NULL DEFAULT 0,
    new_words_learned INTEGER NOT NULL DEFAULT 0,
    cards_reviewed    INTEGER NOT NULL DEFAULT 0,
    game_score        INTEGER,
    xp_earned         INTEGER NOT NULL DEFAULT 0
);
