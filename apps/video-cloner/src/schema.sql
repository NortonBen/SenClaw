-- Video Cloner schema.
--
-- A "project" is one source video plus the clone configuration applied to it.
-- Scenes are the 8-second JSON prompts produced from that video; they are
-- appended across several model calls ("analyse the next segment"), so each row
-- keeps its position explicitly.

CREATE TABLE IF NOT EXISTS projects (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  name              TEXT NOT NULL,
  video_path        TEXT NOT NULL,
  video_mime        TEXT NOT NULL DEFAULT 'video/mp4',
  video_size        INTEGER NOT NULL DEFAULT 0,
  video_filename    TEXT NOT NULL DEFAULT '',
  -- Gemini Files API handle, reused across segment calls so a large video is
  -- uploaded once instead of once per request. Cleared when it expires.
  file_uri          TEXT NOT NULL DEFAULT '',
  file_uri_at       TEXT NOT NULL DEFAULT '',
  char_image_path   TEXT NOT NULL DEFAULT '',
  char_image_mime   TEXT NOT NULL DEFAULT '',
  style             TEXT NOT NULL DEFAULT 'Phân tích theo video gốc (Original Style)',
  model             TEXT NOT NULL DEFAULT 'gemini-3-flash-preview',
  char_description  TEXT NOT NULL DEFAULT '',
  custom_dialogue   TEXT NOT NULL DEFAULT '',
  bg_description    TEXT NOT NULL DEFAULT '',
  auto_magic        INTEGER NOT NULL DEFAULT 0,
  visual_similarity INTEGER NOT NULL DEFAULT 100,
  created_at        TEXT NOT NULL,
  updated_at        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS scenes (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  position    INTEGER NOT NULL,
  scene_id    TEXT NOT NULL DEFAULT '',
  json        TEXT NOT NULL,
  -- Which analysis run produced this scene. 0 for scenes that came back from a
  -- restore, since those predate the run that is current.
  job_id      INTEGER NOT NULL DEFAULT 0,
  created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_scenes_project ON scenes(project_id, position);

-- Point-in-time copies of a project's whole scene list, written immediately
-- before anything that overwrites it.
--
-- Three operations destroy work: re-analysing from the start, regenerating the
-- last segment, and a bulk find/replace. Without this table a mistyped rename
-- across 40 segments is unrecoverable — the model output cost minutes and money
-- to produce and cannot be reconstructed.
CREATE TABLE IF NOT EXISTS snapshots (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  -- analyze_start | analyze_regenerate | replace | restore
  reason      TEXT NOT NULL,
  label       TEXT NOT NULL DEFAULT '',
  scene_count INTEGER NOT NULL DEFAULT 0,
  -- The full scene list as a JSON array.
  scenes      TEXT NOT NULL,
  created_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_snapshots_project ON snapshots(project_id, id DESC);

CREATE TABLE IF NOT EXISTS jobs (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  project_id    INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
  -- start | continue | regenerate
  kind          TEXT NOT NULL,
  -- queued | processing | completed | failed | cancelled
  status        TEXT NOT NULL DEFAULT 'queued',
  -- scene_id the model was told to resume after (0 = from the beginning)
  from_scene    INTEGER NOT NULL DEFAULT 0,
  scenes_added  INTEGER NOT NULL DEFAULT 0,
  model         TEXT NOT NULL DEFAULT '',
  temperature   REAL NOT NULL DEFAULT 0,
  error         TEXT NOT NULL DEFAULT '',
  raw           TEXT NOT NULL DEFAULT '',
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_jobs_project ON jobs(project_id, id DESC);
CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);

CREATE TABLE IF NOT EXISTS app_settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
);

INSERT OR IGNORE INTO app_settings (key, value) VALUES ('gemini_api_key', '');
INSERT OR IGNORE INTO app_settings (key, value) VALUES ('default_model', 'gemini-3-flash-preview');
