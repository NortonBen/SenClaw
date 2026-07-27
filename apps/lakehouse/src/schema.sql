-- Lakehouse — catalog SQLite (nguồn sự thật). Thiết kế: docs/data-lake-app-design.md §4.
--
-- Ba nguyên tắc xuyên suốt:
--
--   * MANIFEST-as-catalog (§2.2): `dataset_file` giữ danh sách file active của từng
--     dataset; đường đọc KHÔNG BAO GIỜ quét thư mục — ListingTable dựng từ danh sách
--     file tường minh + arrow_schema của catalog. File Parquet land xuống đĩa là
--     "vô hình" cho tới khi vào manifest; "swap" dữ liệu (kể cả merge nhiều partition,
--     full_refresh) = MỘT transaction SQLite trên bảng này, không rename thư mục nào.
--   * Guarded write (§6.5): mọi rule ghi-state (claim, terminal không hồi sinh,
--     watermark không lùi) là predicate NGAY TRONG UPDATE — xem db.rs. Đọc-rồi-ghi
--     trong Rust là lỗ TOCTOU đã gây sự cố thật ở rewrite-story.
--   * Idempotent: CREATE TABLE/INDEX IF NOT EXISTS + INSERT OR IGNORE; data-fix
--     một lần đi qua migrate() trong db.rs (key 'schema_version' của app_settings).

CREATE TABLE IF NOT EXISTS connection (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL,             -- postgres|mysql|sqlite|mssql|clickhouse|…
    -- DSN lưu plaintext local (app single-user; catalog file chmod 0600 — §11).
    -- MỌI đầu ra REST/MCP phải redact (postgres://user:•••@host/db), không bao giờ
    -- trả nguyên văn.
    dsn        TEXT NOT NULL,
    created_at TEXT NOT NULL,
    last_ok_at TEXT
);

CREATE TABLE IF NOT EXISTS dataset (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    namespace              TEXT NOT NULL,
    name                   TEXT NOT NULL,
    -- 'parquet' mặc định — chừa đường nâng từng dataset lên 'delta' khi delta-rs
    -- bắt kịp DataFusion major (§2.2); vì vậy upsert KHÔNG đổi format có sẵn.
    format                 TEXT NOT NULL DEFAULT 'parquet',
    layer                  TEXT,           -- tag tự do cho UI grouping, KHÔNG machinery
    partition_cols         TEXT,           -- JSON ["date"] — bắt buộc cho merge/SCD2/incremental_by_time (§6.2)
    -- Một dataset chỉ đúng 1 flow ghi (NULL = import tay) — guard ở dataset_set_owner.
    owner_flow_id          TEXT,
    current_schema_version INTEGER,
    -- Aggregate của file ACTIVE, tính lại trong cùng transaction với mọi thay đổi manifest.
    row_count              INTEGER NOT NULL DEFAULT 0,
    byte_size              INTEGER NOT NULL DEFAULT 0,
    created_at             TEXT NOT NULL,
    updated_at             TEXT NOT NULL,
    UNIQUE (namespace, name)
);

-- MANIFEST — nguồn sự thật của mọi read (§2.2). File hiện diện trên đĩa ≠ file
-- active; chỉ state='active' được query. GC chỉ xóa vật lý file tombstone sau
-- gc_grace_seconds (≥ 2× query_max_seconds) — reader snapshot danh sách file lúc
-- plan nên grace period cho snapshot-isolation thực dụng (§7).
CREATE TABLE IF NOT EXISTS dataset_file (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    dataset_id    INTEGER NOT NULL,
    path          TEXT NOT NULL,           -- tương đối dưới lake/; tên chứa run_id để boot reconcile quét được
    run_id        TEXT NOT NULL,
    "partition"   TEXT,                    -- JSON {"date":"2024-01-01"}
    row_count     INTEGER NOT NULL DEFAULT 0,
    byte_size     INTEGER NOT NULL DEFAULT 0,
    stats         TEXT,                    -- JSON min/max theo PK + time_column (prune merge/query)
    state         TEXT NOT NULL CHECK (state IN ('active', 'tombstone')),
    created_at    TEXT NOT NULL,
    tombstoned_at TEXT
);
CREATE INDEX IF NOT EXISTS ix_file_dataset ON dataset_file(dataset_id, state);
CREATE INDEX IF NOT EXISTS ix_file_run     ON dataset_file(run_id);

CREATE TABLE IF NOT EXISTS schema_version (
    dataset_id   INTEGER NOT NULL,
    version      INTEGER NOT NULL,
    -- JSON [{column_id,name,type,nullable}] — column_id CHỈ để bookkeeping diff;
    -- Parquet match cột theo TÊN (§6.4), không có rename-safety.
    arrow_schema TEXT NOT NULL,
    change       TEXT,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (dataset_id, version)
);

CREATE TABLE IF NOT EXISTS flow (
    id                TEXT PRIMARY KEY,
    name              TEXT,
    def               TEXT NOT NULL,       -- JSON canonical (§6.1); YAML normalize trước khi lưu
    def_version       INTEGER NOT NULL DEFAULT 1,
    enabled           INTEGER NOT NULL DEFAULT 0,  -- flow AI sinh mặc định TẮT (§6.6)
    schedule          TEXT,                -- JSON {"every_minutes":N}|{"daily_at":"HH:MM"}|null
    last_scheduled_at TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS run (
    id         TEXT PRIMARY KEY,           -- uuidv7 == load_id; stamp cột _run_id vào mọi row land
    flow_id    TEXT NOT NULL,
    "trigger"  TEXT NOT NULL,              -- manual|schedule|mcp|backfill|compaction
    status     TEXT NOT NULL CHECK (status IN ('queued', 'running', 'success',
                                              'failed', 'partial', 'cancelled')),
    started_at TEXT,
    ended_at   TEXT,
    error      TEXT,
    updated_at TEXT NOT NULL               -- watchdog quét THEO CỘT NÀY, không created_at (§6.5)
);
-- PER-FLOW EXCLUSION (§6.5): tối đa 1 run active / flow, enforce tại DB thay vì
-- check trong Rust. Scheduler tick đụng conflict → skip lặng; manual/MCP → 409
-- "flow đang chạy" (map ở db.rs::run_create thành RunCreate::FlowBusy).
CREATE UNIQUE INDEX IF NOT EXISTS ux_run_flow_active ON run(flow_id)
    WHERE status IN ('queued', 'running');
CREATE INDEX IF NOT EXISTS ix_run_status ON run(status, updated_at);

CREATE TABLE IF NOT EXISTS step_run (
    run_id       TEXT NOT NULL,
    step_id      TEXT NOT NULL,
    status       TEXT NOT NULL,
    rows_read    INTEGER NOT NULL DEFAULT 0,
    rows_written INTEGER NOT NULL DEFAULT 0,
    started_at   TEXT,
    ended_at     TEXT,
    error        TEXT,
    PRIMARY KEY (run_id, step_id)
);

-- Interval accounting (SQLMesh) — nền của resume/backfill: interval success của cùng
-- (flow, step, def_version) được skip khi chạy lại (§6.5). Ghi bằng INSERT OR REPLACE
-- từng interval trong transaction commit — không read-modify-write JSON trong Rust (§4).
CREATE TABLE IF NOT EXISTS step_interval (
    flow_id        TEXT NOT NULL,
    step_id        TEXT NOT NULL,
    def_version    INTEGER NOT NULL,
    interval_start TEXT NOT NULL,
    interval_end   TEXT NOT NULL,
    run_id         TEXT NOT NULL,
    status         TEXT NOT NULL,          -- success|failed
    PRIMARY KEY (flow_id, step_id, interval_start)
);

-- CHỈ cursor sống; interval nằm ở step_interval (§4). Watermark chỉ tiến —
-- predicate monotonic nằm trong stream_state_set_monotonic (db.rs).
CREATE TABLE IF NOT EXISTS stream_state (
    flow_id         TEXT NOT NULL,
    step_id         TEXT NOT NULL,
    cursor_column   TEXT,
    last_value      TEXT,
    boundary_hashes TEXT,                  -- dedupe row trùng biên closed-range >= (§6.2)
    updated_at      TEXT NOT NULL,
    PRIMARY KEY (flow_id, step_id)
);

CREATE TABLE IF NOT EXISTS lineage_edge (
    run_id         TEXT NOT NULL,
    step_id        TEXT NOT NULL,
    direction      TEXT NOT NULL CHECK (direction IN ('in', 'out')),
    dataset_id     INTEGER NOT NULL,
    schema_version INTEGER
);
CREATE INDEX IF NOT EXISTS ix_lineage_dataset ON lineage_edge(dataset_id, direction);
CREATE INDEX IF NOT EXISTS ix_lineage_run     ON lineage_edge(run_id);

CREATE TABLE IF NOT EXISTS run_log (
    run_id  TEXT NOT NULL,
    seq     INTEGER NOT NULL,
    ts      TEXT NOT NULL,
    level   TEXT NOT NULL,
    step_id TEXT,
    message TEXT NOT NULL,
    PRIMARY KEY (run_id, seq)
);
CREATE INDEX IF NOT EXISTS ix_run_log_ts ON run_log(ts);   -- sweep theo log_retention_days

CREATE TABLE IF NOT EXISTS app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Seed mặc định (§4). INSERT OR IGNORE — không đè chỉnh sửa của user.
-- 'import_paths' seed ở db.rs (giá trị chứa data_dir tuyệt đối của máy này).
INSERT OR IGNORE INTO app_settings (key, value) VALUES
    ('max_concurrent',       '2'),
    ('memory_limit_mb',      '2048'),   -- GreedyMemoryPool budget (§7)
    ('target_partitions',    '4'),      -- thấp chủ đích: FairSpillPool chia budget theo partition
    ('query_max_seconds',    '600'),
    ('gc_grace_seconds',     '1200'),   -- ≥ 2× query_max_seconds — reader isolation (§2.2/§7)
    ('log_retention_days',   '14'),
    ('import_base64_max_mb', '10');
