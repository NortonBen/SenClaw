-- AI Discuss Team — schema idempotent (CREATE IF NOT EXISTS, không migration runner).

CREATE TABLE IF NOT EXISTS discussions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  title TEXT NOT NULL,
  requirement TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'draft',      -- draft|running|paused|review|done
  mode TEXT NOT NULL DEFAULT 'sequential',   -- sequential|parallel
  pace_secs INTEGER NOT NULL DEFAULT 20,
  max_rounds INTEGER NOT NULL DEFAULT 12,
  round INTEGER NOT NULL DEFAULT 0,
  manager_score INTEGER NOT NULL DEFAULT 0,
  manager_missing TEXT NOT NULL DEFAULT '[]',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  concluded_at INTEGER
);

CREATE TABLE IF NOT EXISTS members (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  key TEXT NOT NULL UNIQUE,
  name TEXT NOT NULL,
  role TEXT NOT NULL DEFAULT 'member',       -- member|manager|secretary
  expertise TEXT NOT NULL DEFAULT '',
  style TEXT NOT NULL DEFAULT '',
  hat TEXT NOT NULL DEFAULT '',              -- thiên hướng mũ: white|red|black|yellow|green|blue|''
  use_tools INTEGER NOT NULL DEFAULT 1,
  tools TEXT,                                -- NULL = toàn bộ tool MCP hệ thống; JSON array = giới hạn (soft)
  model TEXT,
  enabled INTEGER NOT NULL DEFAULT 1,
  sort INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS discussion_members (
  discussion_id INTEGER NOT NULL,
  member_id INTEGER NOT NULL,
  PRIMARY KEY (discussion_id, member_id)
);

CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  discussion_id INTEGER NOT NULL,
  round INTEGER NOT NULL DEFAULT 0,
  author_kind TEXT NOT NULL,                 -- boss|member|manager|secretary|system
  member_id INTEGER,
  kind TEXT NOT NULL,                        -- opinion|reaction|boss|manager_note|minutes_note|system|result_note
  content TEXT NOT NULL,
  claim_type TEXT,                           -- evidence|inference|creative
  provability TEXT,                          -- practical|theoretical
  hat TEXT,
  stance TEXT,                               -- agree|disagree
  reply_to INTEGER,
  citations TEXT NOT NULL DEFAULT '[]',
  flags TEXT NOT NULL DEFAULT '{}',
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_messages_disc ON messages(discussion_id, id);

CREATE TABLE IF NOT EXISTS documents (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  discussion_id INTEGER,                     -- NULL = kho chung toàn app
  title TEXT NOT NULL,
  filename TEXT NOT NULL DEFAULT '',
  content TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'upload',     -- upload|paste|member|secretary|result
  created_by TEXT NOT NULL DEFAULT 'boss',
  created_at INTEGER NOT NULL
);
-- Cố ý KHÔNG external-content: contentless FTS5 chỉ xoá được bằng replay giá trị
-- gốc — xoá tài liệu sẽ để lại rác tìm được. Đồng bộ tay qua fts_sync.
CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(title, content, tokenize='unicode61 remove_diacritics 2');

CREATE TABLE IF NOT EXISTS member_memory (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  member_id INTEGER NOT NULL,
  discussion_id INTEGER,
  kind TEXT NOT NULL DEFAULT 'fact',         -- fact|stance|lesson
  content TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS member_memory_fts USING fts5(content, tokenize='unicode61 remove_diacritics 2');

CREATE TABLE IF NOT EXISTS member_thinking (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  member_id INTEGER NOT NULL,
  discussion_id INTEGER NOT NULL,
  round INTEGER NOT NULL,
  content TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_thinking_member ON member_thinking(member_id, discussion_id, id);

CREATE TABLE IF NOT EXISTS minutes (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  discussion_id INTEGER NOT NULL,
  round INTEGER NOT NULL,
  content TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_minutes_disc ON minutes(discussion_id, id);

CREATE TABLE IF NOT EXISTS results (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  discussion_id INTEGER NOT NULL,
  content TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'draft',      -- draft|approved|rejected
  feedback TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
