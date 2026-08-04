-- BA Studio schema. Timestamps are unix milliseconds. `source` phân biệt nội
-- dung người dùng tự viết ('user') với nội dung AI sinh ('ai') — AI không bao
-- giờ lặng lẽ ghi đè bản người dùng đã sửa (upsert giữ version cũ trong
-- doc_versions).

CREATE TABLE IF NOT EXISTS projects (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  slug        TEXT NOT NULL UNIQUE,
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  context     TEXT NOT NULL DEFAULT '',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS features (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  slug        TEXT NOT NULL,
  name        TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  priority    TEXT NOT NULL DEFAULT 'P1',
  status      TEXT NOT NULL DEFAULT 'active',
  sort        INTEGER NOT NULL DEFAULT 0,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  UNIQUE(project_id, slug)
);
CREATE INDEX IF NOT EXISTS idx_features_project ON features(project_id, status);

-- Một tài liệu "sống" cho mỗi (project, feature, doc_type, subtype); sinh lại
-- tạo version mới (bản cũ vào doc_versions). feature_id NULL = tài liệu cấp
-- project (prd, roadmap, discover, overview, meeting...).
CREATE TABLE IF NOT EXISTS documents (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  feature_id  INTEGER REFERENCES features(id) ON DELETE CASCADE,
  doc_type    TEXT NOT NULL,
  subtype     TEXT NOT NULL DEFAULT '',
  title       TEXT NOT NULL,
  content     TEXT NOT NULL DEFAULT '',
  format      TEXT NOT NULL DEFAULT 'markdown',
  status      TEXT NOT NULL DEFAULT 'draft',
  version     INTEGER NOT NULL DEFAULT 1,
  source      TEXT NOT NULL DEFAULT 'user',
  confidence  TEXT NOT NULL DEFAULT '',
  meta        TEXT NOT NULL DEFAULT '{}',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_documents_proj ON documents(project_id, feature_id, doc_type, subtype);
CREATE INDEX IF NOT EXISTS idx_documents_status ON documents(status);

CREATE TABLE IF NOT EXISTS doc_versions (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  version     INTEGER NOT NULL,
  content     TEXT NOT NULL,
  note        TEXT NOT NULL DEFAULT '',
  created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_doc_versions ON doc_versions(document_id, version);

-- Chỉ mục ID truy vết, đánh lại toàn bộ mỗi lần nội dung doc đổi.
-- role='def' — mục được định nghĩa tại doc này; role='ref' — được nhắc tới,
-- from_ident là ID của mục chứa nó (dòng bảng) nếu xác định được.
CREATE TABLE IF NOT EXISTS doc_ids (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  kind        TEXT NOT NULL,
  ident       TEXT NOT NULL,
  role        TEXT NOT NULL,
  from_ident  TEXT NOT NULL DEFAULT '',
  resolved    INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_doc_ids_doc ON doc_ids(document_id);
CREATE INDEX IF NOT EXISTS idx_doc_ids_kind ON doc_ids(kind, role);

CREATE TABLE IF NOT EXISTS workflows (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  feature_id  INTEGER NOT NULL REFERENCES features(id) ON DELETE CASCADE,
  name        TEXT NOT NULL,
  template    TEXT NOT NULL DEFAULT 'custom',
  steps       TEXT NOT NULL DEFAULT '[]',
  status      TEXT NOT NULL DEFAULT 'active',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workflows_feature ON workflows(feature_id, status);

CREATE TABLE IF NOT EXISTS change_requests (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  feature_id  INTEGER REFERENCES features(id) ON DELETE CASCADE,
  code        TEXT NOT NULL UNIQUE,
  title       TEXT NOT NULL,
  description TEXT NOT NULL,
  severity    TEXT NOT NULL DEFAULT 'medium',
  status      TEXT NOT NULL DEFAULT 'open',
  analysis    TEXT NOT NULL DEFAULT '',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS cr_impacts (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  cr_id       INTEGER NOT NULL REFERENCES change_requests(id) ON DELETE CASCADE,
  document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  summary     TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'pending',
  applied_at  INTEGER,
  created_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_cr_impacts ON cr_impacts(cr_id, status);

CREATE TABLE IF NOT EXISTS qa_log (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  feature_id  INTEGER,
  question    TEXT NOT NULL,
  answer      TEXT NOT NULL,
  citations   TEXT NOT NULL DEFAULT '[]',
  created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS activity (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  actor       TEXT NOT NULL,
  action      TEXT NOT NULL,
  detail      TEXT NOT NULL DEFAULT '',
  created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

-- FTS đứng riêng (không external-content): text được fold đ→d trong Rust
-- trước khi ghi (trigger SQL không gọi được hàm fold), rowid = documents.id.
CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
  title, content,
  tokenize='unicode61 remove_diacritics 2'
);
