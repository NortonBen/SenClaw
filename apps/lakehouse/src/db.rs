//! Catalog SQLite — manifest + flow/run/state (docs/data-lake-app-design.md §4).
//!
//! Một connection serialize sau `Mutex`, WAL, typed structs — pattern
//! `apps/rewrite-story/src/db.rs`. Hai quy tắc load-bearing:
//!
//!   * Guarded write (§6.5): claim, "terminal không hồi sinh", "watermark không
//!     lùi" đều là predicate NGAY TRONG UPDATE, trả về `bool` áp dụng/không.
//!     Check trong Rust rồi mới ghi là lỗ TOCTOU (sự cố thật ở rewrite-story).
//!   * `Mutex<Connection>` KHÔNG reentrant: collect xong drop guard mới gọi hàm
//!     Db khác; không giữ guard qua `.await`.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

const SCHEMA: &str = include_str!("schema.sql");

/// Trạng thái run. String được persist và xuất hiện trên wire (REST/MCP/UI).
pub mod run_status {
    pub const QUEUED: &str = "queued";
    pub const RUNNING: &str = "running";
    pub const SUCCESS: &str = "success";
    pub const FAILED: &str = "failed";
    pub const PARTIAL: &str = "partial";
    pub const CANCELLED: &str = "cancelled";

    pub fn is_terminal(s: &str) -> bool {
        matches!(s, SUCCESS | FAILED | PARTIAL | CANCELLED)
    }

    pub fn is_active(s: &str) -> bool {
        matches!(s, QUEUED | RUNNING)
    }
}

/// Nguồn kích hoạt run (cột `run."trigger"`) — taxonomy chuẩn theo design §4.
/// `MCP` và `BACKFILL` là giá trị DỰ PHÒNG: lake_flow_run qua MCP hiện gắn `MANUAL`
/// (chung `logic_flow_run` với REST), và backfill là interval-accounting KHÔNG tạo run
/// row nên không sinh trigger 'backfill'. Giữ đủ 5 để taxonomy khớp design ở một chỗ.
#[allow(dead_code)]
pub mod trigger {
    pub const MANUAL: &str = "manual";
    pub const SCHEDULE: &str = "schedule";
    pub const MCP: &str = "mcp";
    pub const BACKFILL: &str = "backfill";
    pub const COMPACTION: &str = "compaction";
}

// ---- typed rows ----

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionInfo {
    pub id: String,
    pub kind: String,
    /// DSN nguyên văn — CHỈ dùng nội bộ để mở kết nối. REST/MCP phải redact
    /// trước khi trả (§11), không bao giờ serialize thẳng struct này ra client.
    pub dsn: String,
    pub created_at: String,
    pub last_ok_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Dataset {
    pub id: i64,
    pub namespace: String,
    pub name: String,
    pub format: String,
    pub layer: Option<String>,
    pub partition_cols: Option<String>,
    pub owner_flow_id: Option<String>,
    pub current_schema_version: Option<i64>,
    pub row_count: i64,
    pub byte_size: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DatasetFile {
    pub id: i64,
    pub dataset_id: i64,
    pub path: String,
    pub run_id: String,
    pub partition: Option<String>,
    pub row_count: i64,
    pub byte_size: i64,
    pub stats: Option<String>,
    pub state: String,
    pub created_at: String,
    pub tombstoned_at: Option<String>,
}

/// File mới chuẩn bị vào manifest (đã nằm trên đĩa, chưa "hiện hình").
#[derive(Debug, Clone)]
pub struct NewDatasetFile {
    pub path: String,
    pub partition: Option<String>,
    pub row_count: i64,
    pub byte_size: i64,
    pub stats: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaVersion {
    pub dataset_id: i64,
    pub version: i64,
    pub arrow_schema: String,
    pub change: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub id: String,
    pub flow_id: String,
    pub trigger: String,
    pub status: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub error: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FlowRow {
    pub id: String,
    pub name: Option<String>,
    /// JSON canonical của flow (§6.1). Runner đọc rồi parse qua flow::parse.
    pub def: String,
    pub def_version: i64,
    pub enabled: bool,
    pub schedule: Option<String>,
    pub last_scheduled_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepRun {
    pub run_id: String,
    pub step_id: String,
    pub status: String,
    pub rows_read: i64,
    pub rows_written: i64,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepInterval {
    pub flow_id: String,
    pub step_id: String,
    pub def_version: i64,
    pub interval_start: String,
    pub interval_end: String,
    pub run_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamState {
    pub flow_id: String,
    pub step_id: String,
    pub cursor_column: Option<String>,
    pub last_value: Option<String>,
    pub boundary_hashes: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunLogLine {
    pub seq: i64,
    pub ts: String,
    pub level: String,
    pub step_id: Option<String>,
    pub message: String,
}

/// Tổng quan cho `/api/status` + `lake_stats`.
#[derive(Debug, Clone, Serialize)]
pub struct LakeStats {
    pub datasets: i64,
    pub total_rows: i64,
    pub total_bytes: i64,
    pub runs_active: i64,
    pub runs_24h: i64,
}

/// Kết quả `run_create`: unique index `ux_run_flow_active` biến "flow đang có run
/// active" thành lỗi constraint — map sang biến thể phân biệt được để API trả 409
/// và scheduler tick skip lặng (§6.5), thay vì lẫn vào lỗi 500.
#[derive(Debug)]
pub enum RunCreate {
    Created(String),
    FlowBusy,
}

/// Settings client được phép ghi. REST và MCP dùng chung allowlist này để hai
/// surface không drift (bài học rewrite-story: HTTP từng nhận key tùy ý).
/// `schema_version` cố ý KHÔNG nằm đây — của migrate().
pub const WRITABLE_SETTINGS: &[&str] = &[
    "max_concurrent",
    "memory_limit_mb",
    "target_partitions",
    "query_max_seconds",
    "gc_grace_seconds",
    "log_retention_days",
    "import_base64_max_mb",
    "import_paths",
];

/// Từ chối key ngoài allowlist và value sai kiểu/ngoài biên.
pub fn validate_setting(key: &str, value: &str) -> Result<()> {
    if !WRITABLE_SETTINGS.contains(&key) {
        anyhow::bail!(
            "cấu hình '{key}' không hợp lệ; hợp lệ: {}",
            WRITABLE_SETTINGS.join(", ")
        );
    }
    let range = |lo: i64, hi: i64| -> Result<()> {
        let n: i64 = value
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("'{key}' phải là số nguyên, nhận '{value}'"))?;
        if n < lo || n > hi {
            anyhow::bail!("'{key}' phải trong khoảng {lo}..{hi}, nhận {n}");
        }
        Ok(())
    };
    match key {
        "max_concurrent" => range(1, 8),
        "memory_limit_mb" => range(256, 65_536),
        "target_partitions" => range(1, 64),
        "query_max_seconds" => range(5, 3_600),
        // Nên giữ ≥ 2× query_max_seconds (reader isolation §7) — không cross-check
        // ở đây, GC tick tự lấy max(gc_grace, 2×query_max) khi chạy.
        "gc_grace_seconds" => range(60, 86_400),
        "log_retention_days" => range(1, 365),
        // Trần thật là DefaultBodyLimit 64MB của route /import (§8).
        "import_base64_max_mb" => range(1, 64),
        "import_paths" => {
            // JSON array đường dẫn — allowlist cho lake_import_file{path} (§9).
            // Mảng rỗng hợp lệ = user chủ động tắt import qua path.
            let v: Result<Vec<String>, _> = serde_json::from_str(value);
            v.map_err(|_| {
                anyhow::anyhow!("'{key}' phải là JSON array chuỗi đường dẫn, nhận '{value}'")
            })?;
            Ok(())
        }
        _ => Ok(()),
    }
}

pub struct Db {
    conn: Mutex<Connection>,
}

fn now() -> String {
    // Cùng format với datetime('now') của SQLite để so sánh chuỗi dùng được trong SQL.
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // Catalog chứa DSN plaintext (§11) — siết 0600 ngay khi file tồn tại (§3.3).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        // 'import_paths' không seed được trong schema.sql: giá trị mặc định chứa
        // data_dir tuyệt đối của máy này. INSERT OR IGNORE — không đè user sửa.
        let inbox = crate::config::inbox_dir().to_string_lossy().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value) VALUES ('import_paths', ?1)",
            params![serde_json::json!([inbox]).to_string()],
        )?;
        let db = Db {
            conn: Mutex::new(conn),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Tầng 3 của migrations. Ba tầng (pattern rewrite-story):
    ///   1. schema.sql — DDL idempotent (`IF NOT EXISTS` + `INSERT OR IGNORE`);
    ///   2. `add_column` — ALTER nuốt "duplicate column" cho DB cũ nâng tại chỗ;
    ///   3. data-fix một lần, gate bằng key 'schema_version' trong app_settings.
    /// Chưa có data-fix nào — giữ khung để lần sửa đầu tiên không phát minh lại.
    fn migrate(&self) -> Result<()> {
        if self.setting_i64("schema_version", 0) < 1 {
            self.set_setting("schema_version", "1")?;
        }
        Ok(())
    }

    /// `ALTER TABLE … ADD COLUMN` idempotent — nuốt "duplicate column name" để
    /// chạy được trên cả DB mới (schema.sql đã có cột) lẫn DB cũ. Khung migration
    /// tầng 2 (§migrate) — chưa migration nào cần, giữ để lần sửa schema đầu không
    /// phát minh lại; gọi từ migrate() khi cần.
    #[allow(dead_code)]
    fn add_column(&self, ddl: &str) -> Result<()> {
        match self.with_conn(|c| c.execute_batch(ddl)) {
            Ok(()) => Ok(()),
            Err(e) if e.to_string().contains("duplicate column name") => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Chạy `f` với connection (đã khóa). KHÔNG gọi lồng hàm Db khác bên trong
    /// `f` — Mutex không reentrant, sẽ deadlock.
    pub fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        Ok(f(&conn)?)
    }

    // ---- settings ----

    pub fn setting(&self, key: &str, default: &str) -> String {
        self.with_conn(|c| {
            c.query_row(
                "SELECT value FROM app_settings WHERE key = ?1",
                params![key],
                |r| r.get::<_, String>(0),
            )
            .optional()
        })
        .ok()
        .flatten()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
    }

    pub fn setting_i64(&self, key: &str, default: i64) -> i64 {
        self.setting(key, "").parse().unwrap_or(default)
    }

    /// Ghi thô — caller phía client (REST/MCP) PHẢI `validate_setting` trước.
    /// Migrate/seed nội bộ đi thẳng vào đây (schema_version không nằm allowlist).
    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT OR REPLACE INTO app_settings (key, value) VALUES (?1, ?2)",
                params![key, value],
            )
        })?;
        Ok(())
    }

    pub fn all_settings(&self) -> Result<Vec<(String, String)>> {
        self.with_conn(|c| {
            let mut st = c.prepare("SELECT key, value FROM app_settings ORDER BY key")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    // ---- connection ----

    /// Thêm hoặc cập nhật (cùng id thì đè kind/dsn, giữ created_at).
    pub fn connection_add(&self, id: &str, kind: &str, dsn: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO connection (id, kind, dsn, created_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, dsn = excluded.dsn",
                params![id, kind, dsn, now()],
            )
        })?;
        Ok(())
    }

    pub fn connection_list(&self) -> Result<Vec<ConnectionInfo>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id, kind, dsn, created_at, last_ok_at FROM connection ORDER BY id",
            )?;
            let rows = st.query_map([], connection_from_row)?;
            rows.collect()
        })
    }

    pub fn connection_get(&self, id: &str) -> Result<Option<ConnectionInfo>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT id, kind, dsn, created_at, last_ok_at FROM connection WHERE id = ?1",
                params![id],
                connection_from_row,
            )
            .optional()
        })
    }

    /// Ghi nhận lần test thành công gần nhất.
    pub fn connection_mark_ok(&self, id: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE connection SET last_ok_at = ?2 WHERE id = ?1",
                params![id, now()],
            )
        })?;
        Ok(())
    }

    /// Delete-guard "còn flow tham chiếu" (§6.5) nằm ở tầng API (đọc flow.def) —
    /// đây chỉ là thao tác xóa thô.
    pub fn connection_delete(&self, id: &str) -> Result<usize> {
        self.with_conn(|c| c.execute("DELETE FROM connection WHERE id = ?1", params![id]))
    }

    // ---- dataset ----

    /// Tạo nếu chưa có, trả id. Với dataset có sẵn: chỉ bổ sung layer/partition_cols
    /// còn thiếu; KHÔNG đổi `format` (đường nâng parquet→delta là migration riêng, §2.2).
    pub fn dataset_upsert(
        &self,
        namespace: &str,
        name: &str,
        format: Option<&str>,
        layer: Option<&str>,
        partition_cols: Option<&str>,
    ) -> Result<i64> {
        self.with_conn(|c| {
            c.query_row(
                "INSERT INTO dataset (namespace, name, format, layer, partition_cols,
                                      created_at, updated_at)
                 VALUES (?1, ?2, COALESCE(?3, 'parquet'), ?4, ?5, ?6, ?6)
                 ON CONFLICT(namespace, name) DO UPDATE SET
                    layer          = COALESCE(excluded.layer, dataset.layer),
                    partition_cols = COALESCE(excluded.partition_cols, dataset.partition_cols),
                    updated_at     = excluded.updated_at
                 RETURNING id",
                params![namespace, name, format, layer, partition_cols, now()],
                |r| r.get(0),
            )
        })
    }

    pub fn dataset_list(
        &self,
        namespace: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Dataset>> {
        let limit = limit.clamp(1, 500);
        let offset = offset.max(0);
        self.with_conn(|c| match namespace {
            Some(ns) => {
                let mut st = c.prepare(&format!(
                    "{DATASET_SELECT} WHERE namespace = ?1 ORDER BY namespace, name LIMIT ?2 OFFSET ?3"
                ))?;
                let rows = st.query_map(params![ns, limit, offset], dataset_from_row)?;
                rows.collect()
            }
            None => {
                let mut st = c.prepare(&format!(
                    "{DATASET_SELECT} ORDER BY namespace, name LIMIT ?1 OFFSET ?2"
                ))?;
                let rows = st.query_map(params![limit, offset], dataset_from_row)?;
                rows.collect()
            }
        })
    }

    pub fn dataset_get(&self, namespace: &str, name: &str) -> Result<Option<Dataset>> {
        self.with_conn(|c| {
            c.query_row(
                &format!("{DATASET_SELECT} WHERE namespace = ?1 AND name = ?2"),
                params![namespace, name],
                dataset_from_row,
            )
            .optional()
        })
    }

    pub fn dataset_get_by_id(&self, id: i64) -> Result<Option<Dataset>> {
        self.with_conn(|c| {
            c.query_row(
                &format!("{DATASET_SELECT} WHERE id = ?1"),
                params![id],
                dataset_from_row,
            )
            .optional()
        })
    }

    /// Xóa dataset + manifest + schema history trong MỘT transaction. File vật lý
    /// trên đĩa do lake.rs dọn (GC/reconcile) — catalog không đụng filesystem.
    pub fn dataset_delete(&self, id: i64) -> Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM dataset_file WHERE dataset_id = ?1", params![id])?;
        tx.execute("DELETE FROM schema_version WHERE dataset_id = ?1", params![id])?;
        let n = tx.execute("DELETE FROM dataset WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(n)
    }

    /// Guarded ownership (§6.1): một dataset chỉ đúng 1 flow ghi. Set trả `false`
    /// khi dataset đã thuộc flow KHÁC — validate flow phải từ chối, không đè.
    /// `None` = thả ownership (flow bị xóa; dataset giữ lại — §6.3).
    pub fn dataset_set_owner(&self, id: i64, flow_id: Option<&str>) -> Result<bool> {
        let n = self.with_conn(|c| match flow_id {
            Some(f) => c.execute(
                "UPDATE dataset SET owner_flow_id = ?2, updated_at = ?3
                 WHERE id = ?1 AND (owner_flow_id IS NULL OR owner_flow_id = ?2)",
                params![id, f, now()],
            ),
            None => c.execute(
                "UPDATE dataset SET owner_flow_id = NULL, updated_at = ?2 WHERE id = ?1",
                params![id, now()],
            ),
        })?;
        Ok(n == 1)
    }

    // ---- manifest (dataset_file) ----

    /// Thêm file active vào manifest — MỘT transaction, kèm tính lại aggregate
    /// của dataset. File chỉ "hiện hình" với query sau commit này (§2.2).
    pub fn manifest_add_files(
        &self,
        dataset_id: i64,
        run_id: &str,
        files: &[NewDatasetFile],
    ) -> Result<()> {
        if files.is_empty() {
            return Ok(());
        }
        let ts = now();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for f in files {
            tx.execute(
                "INSERT INTO dataset_file (dataset_id, path, run_id, \"partition\",
                                           row_count, byte_size, stats, state, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8)",
                params![
                    dataset_id, f.path, run_id, f.partition, f.row_count, f.byte_size,
                    f.stats, ts
                ],
            )?;
        }
        recompute_dataset_stats(&tx, dataset_id, &ts)?;
        tx.commit()?;
        Ok(())
    }

    /// Chuyển file active → tombstone (guarded `state='active'` — tombstone lần
    /// hai không đổi gì). Trả số file thực sự chuyển. GC xóa vật lý về sau.
    /// Primitive cấp thấp: prod dùng `manifest_replace_files`/`manifest_replace_partition`/
    /// `manifest_swap_files` (tombstone + add trong một txn); hàm này để test + dành sẵn.
    #[allow(dead_code)]
    pub fn manifest_tombstone_files(&self, dataset_id: i64, file_ids: &[i64]) -> Result<usize> {
        if file_ids.is_empty() {
            return Ok(0);
        }
        let ts = now();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut n = 0;
        for id in file_ids {
            n += tx.execute(
                "UPDATE dataset_file SET state = 'tombstone', tombstoned_at = ?3
                 WHERE id = ?1 AND dataset_id = ?2 AND state = 'active'",
                params![id, dataset_id, ts],
            )?;
        }
        recompute_dataset_stats(&tx, dataset_id, &ts)?;
        tx.commit()?;
        Ok(n)
    }

    /// Swap nguyên tử full_refresh (§6.2): tombstone MỌI file active hiện tại +
    /// thêm `files` mới active — MỘT transaction. "Hoán dữ liệu" của full_refresh
    /// là đúng một txn trên manifest, không rename thư mục (schema.sql §2.2). Trả
    /// số file cũ đã tombstone. `files` rỗng vẫn hợp lệ (dataset thành rỗng).
    pub fn manifest_swap_files(
        &self,
        dataset_id: i64,
        run_id: &str,
        files: &[NewDatasetFile],
    ) -> Result<usize> {
        let ts = now();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let tombstoned = tx.execute(
            "UPDATE dataset_file SET state = 'tombstone', tombstoned_at = ?2
             WHERE dataset_id = ?1 AND state = 'active'",
            params![dataset_id, ts],
        )?;
        for f in files {
            tx.execute(
                "INSERT INTO dataset_file (dataset_id, path, run_id, \"partition\",
                                           row_count, byte_size, stats, state, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8)",
                params![
                    dataset_id, f.path, run_id, f.partition, f.row_count, f.byte_size,
                    f.stats, ts
                ],
            )?;
        }
        recompute_dataset_stats(&tx, dataset_id, &ts)?;
        tx.commit()?;
        Ok(tombstoned)
    }

    /// Thay TOÀN BỘ file active của MỘT partition (§6.2 incremental_by_time): tombstone
    /// mọi file active có `partition = ?` rồi thêm `files` mới active — MỘT transaction
    /// ("delete interval + insert" idempotent). Trả số file cũ đã tombstone. `files`
    /// rỗng vẫn hợp lệ (partition thành rỗng). Guarded `state='active'`.
    pub fn manifest_replace_partition(
        &self,
        dataset_id: i64,
        run_id: &str,
        partition: &str,
        files: &[NewDatasetFile],
    ) -> Result<usize> {
        let ts = now();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        // Guard partition: khớp cả row `partition IS NULL` khi key rỗng — file import
        // (append) lưu partition=NULL, `= ''` không khớp NULL trong SQLite nên bản cũ
        // không bị tombstone → NULL-partition double-count (BUG merge NULL-partition).
        let tombstoned = tx.execute(
            "UPDATE dataset_file SET state = 'tombstone', tombstoned_at = ?3
             WHERE dataset_id = ?1 AND state = 'active'
               AND (\"partition\" = ?2 OR (?2 = '' AND \"partition\" IS NULL))",
            params![dataset_id, partition, ts],
        )?;
        for f in files {
            tx.execute(
                "INSERT INTO dataset_file (dataset_id, path, run_id, \"partition\",
                                           row_count, byte_size, stats, state, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8)",
                params![
                    dataset_id, f.path, run_id, f.partition, f.row_count, f.byte_size,
                    f.stats, ts
                ],
            )?;
        }
        recompute_dataset_stats(&tx, dataset_id, &ts)?;
        tx.commit()?;
        Ok(tombstoned)
    }

    /// Compaction swap (§12 Phase 4): tombstone ĐÚNG các file cũ theo `old_ids` +
    /// thêm `new_files` active — MỘT transaction. Khác `manifest_swap_files` (tombstone
    /// TẤT CẢ active) và `manifest_replace_partition` (guard theo cột partition, không
    /// khớp partition=NULL): compaction chỉ gộp một nhóm file cụ thể, giữ nguyên các
    /// file active khác. Guarded `state='active'` để không tombstone nhầm file đã bị
    /// một run khác thay. Trả số file cũ đã tombstone.
    pub fn manifest_replace_files(
        &self,
        dataset_id: i64,
        run_id: &str,
        old_ids: &[i64],
        new_files: &[NewDatasetFile],
    ) -> Result<usize> {
        let ts = now();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut tombstoned = 0;
        for id in old_ids {
            tombstoned += tx.execute(
                "UPDATE dataset_file SET state = 'tombstone', tombstoned_at = ?3
                 WHERE id = ?1 AND dataset_id = ?2 AND state = 'active'",
                params![id, dataset_id, ts],
            )?;
        }
        for f in new_files {
            tx.execute(
                "INSERT INTO dataset_file (dataset_id, path, run_id, \"partition\",
                                           row_count, byte_size, stats, state, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8)",
                params![
                    dataset_id, f.path, run_id, f.partition, f.row_count, f.byte_size,
                    f.stats, ts
                ],
            )?;
        }
        recompute_dataset_stats(&tx, dataset_id, &ts)?;
        tx.commit()?;
        Ok(tombstoned)
    }

    /// Tạo một run row TERMINAL cho compaction (trigger='compaction', status='success').
    /// KHÔNG dùng `run_create` vì compaction không gắn flow và không cần hàng đợi/claim;
    /// status 'success' nằm NGOÀI partial index `ux_run_flow_active` nên không đụng
    /// exclusion per-flow. run_id này gắn vào file mới trong manifest — reconcile thấy
    /// run có hàng manifest → GIỮ file (không xóa nhầm output compaction). `label` chỉ
    /// để tra cứu (vd "raw.orders"). Trả run_id (uuidv7 = tên file part-<run_id>-0).
    pub fn run_create_compaction(&self, label: &str) -> Result<String> {
        let id = uuid::Uuid::now_v7().to_string();
        let ts = now();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO run (id, flow_id, \"trigger\", status, started_at, ended_at, updated_at)
                 VALUES (?1, ?2, ?4, 'success', ?3, ?3, ?3)",
                params![id, label, ts, trigger::COMPACTION],
            )
        })?;
        Ok(id)
    }

    /// Danh sách file active — đầu vào dựng ListingTable (§7). Query KHÔNG BAO GIỜ
    /// tự quét thư mục.
    pub fn manifest_active_files(&self, dataset_id: i64) -> Result<Vec<DatasetFile>> {
        self.with_conn(|c| {
            let mut st = c.prepare(&format!(
                "{FILE_SELECT} WHERE dataset_id = ?1 AND state = 'active' ORDER BY id"
            ))?;
            let rows = st.query_map(params![dataset_id], file_from_row)?;
            rows.collect()
        })
    }

    /// Mọi file (mọi state) của một run — boot reconcile đối chiếu đĩa vs manifest.
    pub fn manifest_files_for_run(&self, run_id: &str) -> Result<Vec<DatasetFile>> {
        self.with_conn(|c| {
            let mut st =
                c.prepare(&format!("{FILE_SELECT} WHERE run_id = ?1 ORDER BY id"))?;
            let rows = st.query_map(params![run_id], file_from_row)?;
            rows.collect()
        })
    }

    /// File tombstone quá hạn grace (`tombstoned_at < cutoff`) — GC lấy để xóa
    /// vật lý. Cutoff tính ở lake.rs (max(gc_grace, 2×query_max)).
    pub fn manifest_tombstones_before(&self, cutoff: &str) -> Result<Vec<DatasetFile>> {
        self.with_conn(|c| {
            let mut st = c.prepare(&format!(
                "{FILE_SELECT} WHERE state = 'tombstone' AND tombstoned_at IS NOT NULL
                   AND tombstoned_at < ?1 ORDER BY id"
            ))?;
            let rows = st.query_map(params![cutoff], file_from_row)?;
            rows.collect()
        })
    }

    /// Gỡ hẳn một hàng manifest (sau khi GC đã xóa file vật lý). Chỉ hàng
    /// tombstone mới nên đi đường này — aggregate dataset không đổi (chỉ tính file
    /// active) nên không cần recompute.
    pub fn manifest_delete_file(&self, id: i64) -> Result<usize> {
        self.with_conn(|c| c.execute("DELETE FROM dataset_file WHERE id = ?1", params![id]))
    }

    // ---- schema_version ----

    /// Version kế tiếp (1-based) + cập nhật `dataset.current_schema_version`,
    /// cùng transaction.
    pub fn schema_version_add(
        &self,
        dataset_id: i64,
        arrow_schema: &str,
        change: Option<&str>,
    ) -> Result<i64> {
        let ts = now();
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let version: i64 = tx.query_row(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM schema_version WHERE dataset_id = ?1",
            params![dataset_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO schema_version (dataset_id, version, arrow_schema, change, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![dataset_id, version, arrow_schema, change, ts],
        )?;
        tx.execute(
            "UPDATE dataset SET current_schema_version = ?2, updated_at = ?3 WHERE id = ?1",
            params![dataset_id, version, ts],
        )?;
        tx.commit()?;
        Ok(version)
    }

    pub fn schema_version_current(&self, dataset_id: i64) -> Result<Option<SchemaVersion>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT dataset_id, version, arrow_schema, change, created_at
                 FROM schema_version WHERE dataset_id = ?1
                 ORDER BY version DESC LIMIT 1",
                params![dataset_id],
                schema_version_from_row,
            )
            .optional()
        })
    }

    pub fn schema_version_history(&self, dataset_id: i64) -> Result<Vec<SchemaVersion>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT dataset_id, version, arrow_schema, change, created_at
                 FROM schema_version WHERE dataset_id = ?1 ORDER BY version ASC",
            )?;
            let rows = st.query_map(params![dataset_id], schema_version_from_row)?;
            rows.collect()
        })
    }

    // ---- flow ----

    /// Upsert flow theo id. Insert đặt `def_version = 1`; update GIỮ `def_version`
    /// hiện tại (bump là việc riêng của flow edit §6.3, không đụng ở đây). `def` là
    /// JSON canonical (caller normalize qua flow::to_canonical_json trước khi gọi).
    pub fn flow_upsert(
        &self,
        id: &str,
        name: Option<&str>,
        def: &str,
        enabled: bool,
        schedule: Option<&str>,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO flow (id, name, def, def_version, enabled, schedule,
                                   created_at, updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                    name       = excluded.name,
                    def        = excluded.def,
                    enabled    = excluded.enabled,
                    schedule   = excluded.schedule,
                    updated_at = excluded.updated_at",
                params![id, name, def, enabled as i64, schedule, now()],
            )
        })?;
        Ok(())
    }

    /// Ghi mốc lịch chạy gần nhất (§6.6). Scheduler tick cập nhật sau khi enqueue để
    /// không lặp cùng một slot. Guarded theo id — flow biến mất thì UPDATE 0 dòng (im lặng).
    pub fn flow_set_last_scheduled(&self, id: &str, ts: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE flow SET last_scheduled_at = ?2, updated_at = ?3 WHERE id = ?1",
                params![id, ts, now()],
            )
        })?;
        Ok(())
    }

    /// Bump `def_version` (flow edit state-resetting §6.3) — trả version mới.
    pub fn flow_bump_def_version(&self, id: &str) -> Result<i64> {
        self.with_conn(|c| {
            c.query_row(
                "UPDATE flow SET def_version = def_version + 1, updated_at = ?2
                 WHERE id = ?1 RETURNING def_version",
                params![id, now()],
                |r| r.get(0),
            )
        })
    }

    pub fn flow_get(&self, id: &str) -> Result<Option<FlowRow>> {
        self.with_conn(|c| {
            c.query_row(&format!("{FLOW_SELECT} WHERE id = ?1"), params![id], flow_from_row)
                .optional()
        })
    }

    pub fn flow_list(&self) -> Result<Vec<FlowRow>> {
        self.with_conn(|c| {
            let mut st = c.prepare(&format!("{FLOW_SELECT} ORDER BY id"))?;
            let rows = st.query_map([], flow_from_row)?;
            rows.collect()
        })
    }

    pub fn flow_delete(&self, id: &str) -> Result<usize> {
        self.with_conn(|c| c.execute("DELETE FROM flow WHERE id = ?1", params![id]))
    }

    // ---- lineage ----

    /// Ghi một cạnh lineage (§4). `direction` = 'in'|'out'; runner ghi 'out' cho
    /// dataset một step vừa land.
    pub fn lineage_add(
        &self,
        run_id: &str,
        step_id: &str,
        direction: &str,
        dataset_id: i64,
        schema_version: Option<i64>,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO lineage_edge (run_id, step_id, direction, dataset_id, schema_version)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![run_id, step_id, direction, dataset_id, schema_version],
            )
        })?;
        Ok(())
    }

    // ---- run ----

    /// Enqueue run mới (status 'queued', id = uuidv7 == load_id). Flow còn run
    /// active → `RunCreate::FlowBusy` (unique index chặn, không phải check-then-insert).
    pub fn run_create(&self, flow_id: &str, trigger: &str) -> Result<RunCreate> {
        let id = uuid::Uuid::now_v7().to_string();
        let r = self.with_conn(|c| {
            c.execute(
                "INSERT INTO run (id, flow_id, \"trigger\", status, updated_at)
                 VALUES (?1, ?2, ?3, 'queued', ?4)",
                params![id, flow_id, trigger, now()],
            )
        });
        match r {
            Ok(_) => Ok(RunCreate::Created(id)),
            Err(e) => {
                // SQLite báo vi phạm unique là "UNIQUE constraint failed: run.flow_id"
                // (tên cột, KHÔNG tên partial index ux_run_flow_active). Chỉ có đúng
                // một unique index dính flow_id nên message này = flow đang chạy.
                let msg = e
                    .downcast_ref::<rusqlite::Error>()
                    .map(|re| re.to_string())
                    .unwrap_or_default();
                let busy = msg.contains("run.flow_id") || msg.contains("ux_run_flow_active");
                if busy {
                    Ok(RunCreate::FlowBusy)
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Claim nguyên tử: predicate `status='queued'` là cái làm nên claim — hai
    /// worker đua nhau không thể cùng nhận `true`.
    pub fn run_claim(&self, id: &str) -> Result<bool> {
        let n = self.with_conn(|c| {
            c.execute(
                "UPDATE run SET status = 'running',
                        started_at = COALESCE(started_at, ?2), updated_at = ?2
                 WHERE id = ?1 AND status = 'queued'",
                params![id, now()],
            )
        })?;
        Ok(n == 1)
    }

    /// Guarded: run terminal không hồi sinh (retry = run MỚI, §6.5); mọi lần ghi
    /// bump `updated_at` (watchdog quét theo cột này). Trả `false` = bị từ chối.
    pub fn run_update_status_guarded(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<bool> {
        let ts = now();
        let ended_at = run_status::is_terminal(status).then(|| ts.clone());
        let n = self.with_conn(|c| {
            c.execute(
                "UPDATE run SET status = ?2, error = COALESCE(?3, error),
                        ended_at = COALESCE(?4, ended_at), updated_at = ?5
                 WHERE id = ?1
                   AND status NOT IN ('success', 'failed', 'partial', 'cancelled')",
                params![id, status, error, ended_at, ts],
            )
        })?;
        Ok(n == 1)
    }

    /// Heartbeat giữa batch — watchdog chỉ giết run có `updated_at` cũ (§6.5).
    pub fn run_touch(&self, id: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE run SET updated_at = ?2 WHERE id = ?1 AND status = 'running'",
                params![id, now()],
            )
        })?;
        Ok(())
    }

    /// Boot reconcile (§2.2): run 'running' mồ côi sau crash/restart → failed.
    pub fn run_reconcile_orphans(&self, message: &str) -> Result<usize> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE run SET status = 'failed', error = ?1, ended_at = ?2, updated_at = ?2
                 WHERE status = 'running'",
                params![message, now()],
            )
        })
    }

    /// Watchdog (§6.5): run 'running' có `updated_at` cũ hơn `cutoff` (kẹt, không
    /// heartbeat) → failed. Quét THEO `updated_at`, không `created_at`.
    pub fn run_fail_stuck_running(&self, cutoff: &str, message: &str) -> Result<usize> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE run SET status = 'failed', error = ?2, ended_at = ?3, updated_at = ?3
                 WHERE status = 'running' AND updated_at < ?1",
                params![cutoff, message, now()],
            )
        })
    }

    /// Watchdog (§6.5): run 'queued' bỏ rơi lâu hơn `cutoff` → cancelled.
    pub fn run_cancel_stale_queued(&self, cutoff: &str, message: &str) -> Result<usize> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE run SET status = 'cancelled', error = ?2, ended_at = ?3, updated_at = ?3
                 WHERE status = 'queued' AND updated_at < ?1",
                params![cutoff, message, now()],
            )
        })
    }

    /// Danh sách id run đang 'queued' (poller claim lần lượt).
    pub fn run_list_queued(&self, limit: i64) -> Result<Vec<String>> {
        let limit = limit.clamp(1, 500);
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT id FROM run WHERE status = 'queued' ORDER BY updated_at ASC LIMIT ?1",
            )?;
            let rows = st.query_map(params![limit], |r| r.get::<_, String>(0))?;
            rows.collect()
        })
    }

    pub fn run_get(&self, id: &str) -> Result<Option<Run>> {
        self.with_conn(|c| {
            c.query_row(
                &format!("{RUN_SELECT} WHERE id = ?1"),
                params![id],
                run_from_row,
            )
            .optional()
        })
    }

    pub fn run_list(
        &self,
        flow_id: Option<&str>,
        status: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Run>> {
        let limit = limit.clamp(1, 500);
        let offset = offset.max(0);
        self.with_conn(|c| {
            let mut sql = String::from(RUN_SELECT);
            let mut clauses: Vec<&str> = Vec::new();
            let mut binds: Vec<&dyn rusqlite::ToSql> = Vec::new();
            if let Some(ref f) = flow_id {
                clauses.push("flow_id = ?");
                binds.push(f);
            }
            if let Some(ref s) = status {
                clauses.push("status = ?");
                binds.push(s);
            }
            if !clauses.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&clauses.join(" AND "));
            }
            sql.push_str(" ORDER BY updated_at DESC LIMIT ? OFFSET ?");
            binds.push(&limit);
            binds.push(&offset);
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map(&binds[..], run_from_row)?;
            rows.collect()
        })
    }

    pub fn runs_active_count(&self) -> Result<i64> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM run WHERE status IN ('queued', 'running')",
                [],
                |r| r.get(0),
            )
        })
    }

    // ---- step_run ----

    /// Upsert theo (run_id, step_id): giữ `started_at` đầu tiên; `ended_at` set
    /// khi status rời queued/running.
    pub fn step_run_upsert(
        &self,
        run_id: &str,
        step_id: &str,
        status: &str,
        rows_read: i64,
        rows_written: i64,
        error: Option<&str>,
    ) -> Result<()> {
        let ts = now();
        let ended = !matches!(status, "queued" | "running");
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO step_run (run_id, step_id, status, rows_read, rows_written,
                                       started_at, ended_at, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, CASE WHEN ?7 THEN ?6 END, ?8)
                 ON CONFLICT(run_id, step_id) DO UPDATE SET
                    status       = excluded.status,
                    rows_read    = excluded.rows_read,
                    rows_written = excluded.rows_written,
                    started_at   = COALESCE(step_run.started_at, excluded.started_at),
                    ended_at     = CASE WHEN ?7 THEN COALESCE(step_run.ended_at, ?6)
                                        ELSE step_run.ended_at END,
                    error        = excluded.error",
                params![run_id, step_id, status, rows_read, rows_written, ts, ended, error],
            )
        })?;
        Ok(())
    }

    pub fn step_runs_for(&self, run_id: &str) -> Result<Vec<StepRun>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT run_id, step_id, status, rows_read, rows_written,
                        started_at, ended_at, error
                 FROM step_run WHERE run_id = ?1 ORDER BY step_id",
            )?;
            let rows = st.query_map(params![run_id], step_run_from_row)?;
            rows.collect()
        })
    }

    // ---- step_interval ----

    /// INSERT OR REPLACE từng interval (§4) — run sau đè kết quả run trước cho
    /// cùng interval_start; không read-modify-write JSON trong Rust.
    pub fn step_interval_upsert(
        &self,
        flow_id: &str,
        step_id: &str,
        def_version: i64,
        interval_start: &str,
        interval_end: &str,
        run_id: &str,
        status: &str,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT OR REPLACE INTO step_interval
                    (flow_id, step_id, def_version, interval_start, interval_end, run_id, status)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![flow_id, step_id, def_version, interval_start, interval_end, run_id, status],
            )
        })?;
        Ok(())
    }

    /// Interval đã success của đúng (flow, step, def_version) — resume/backfill
    /// skip các interval này (§6.5); def đổi (version bump) thì không skip.
    pub fn step_interval_list_success(
        &self,
        flow_id: &str,
        step_id: &str,
        def_version: i64,
    ) -> Result<Vec<StepInterval>> {
        self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT flow_id, step_id, def_version, interval_start, interval_end, run_id, status
                 FROM step_interval
                 WHERE flow_id = ?1 AND step_id = ?2 AND def_version = ?3 AND status = 'success'
                 ORDER BY interval_start ASC",
            )?;
            let rows = st.query_map(params![flow_id, step_id, def_version], |r| {
                Ok(StepInterval {
                    flow_id: r.get(0)?,
                    step_id: r.get(1)?,
                    def_version: r.get(2)?,
                    interval_start: r.get(3)?,
                    interval_end: r.get(4)?,
                    run_id: r.get(5)?,
                    status: r.get(6)?,
                })
            })?;
            rows.collect()
        })
    }

    // ---- stream_state ----

    pub fn stream_state_get(&self, flow_id: &str, step_id: &str) -> Result<Option<StreamState>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT flow_id, step_id, cursor_column, last_value, boundary_hashes, updated_at
                 FROM stream_state WHERE flow_id = ?1 AND step_id = ?2",
                params![flow_id, step_id],
                |r| {
                    Ok(StreamState {
                        flow_id: r.get(0)?,
                        step_id: r.get(1)?,
                        cursor_column: r.get(2)?,
                        last_value: r.get(3)?,
                        boundary_hashes: r.get(4)?,
                        updated_at: r.get(5)?,
                    })
                },
            )
            .optional()
        })
    }

    /// Watermark monotonic (§4): chỉ áp dụng khi `last_value` cũ NULL hoặc NHỎ HƠN
    /// giá trị mới (so sánh chuỗi — cursor ISO-8601/lexicographic theo thiết kế).
    /// Run chậm về sau không đè watermark mới; giá trị BẰNG cũng bị từ chối —
    /// dedupe biên closed-range là việc của `boundary_hashes`, không phải ghi lại
    /// watermark. Trả `false` = bị từ chối.
    pub fn stream_state_set_monotonic(
        &self,
        flow_id: &str,
        step_id: &str,
        cursor_column: &str,
        last_value: &str,
        boundary_hashes: Option<&str>,
    ) -> Result<bool> {
        let n = self.with_conn(|c| {
            c.execute(
                "INSERT INTO stream_state
                    (flow_id, step_id, cursor_column, last_value, boundary_hashes, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(flow_id, step_id) DO UPDATE SET
                    cursor_column   = excluded.cursor_column,
                    last_value      = excluded.last_value,
                    boundary_hashes = excluded.boundary_hashes,
                    updated_at      = excluded.updated_at
                 WHERE stream_state.last_value IS NULL
                    OR stream_state.last_value < excluded.last_value",
                params![flow_id, step_id, cursor_column, last_value, boundary_hashes, now()],
            )
        })?;
        Ok(n == 1)
    }

    /// Xóa watermark của một step (flow edit state-resetting §6.3). Trả số dòng xóa.
    pub fn stream_state_delete(&self, flow_id: &str, step_id: &str) -> Result<usize> {
        self.with_conn(|c| {
            c.execute(
                "DELETE FROM stream_state WHERE flow_id = ?1 AND step_id = ?2",
                params![flow_id, step_id],
            )
        })
    }

    /// Xóa mọi interval của một step (flow edit state-resetting §6.3). Trả số dòng xóa.
    pub fn step_interval_delete(&self, flow_id: &str, step_id: &str) -> Result<usize> {
        self.with_conn(|c| {
            c.execute(
                "DELETE FROM step_interval WHERE flow_id = ?1 AND step_id = ?2",
                params![flow_id, step_id],
            )
        })
    }

    // ---- run_log ----

    /// Append 1 dòng, seq tự tăng per-run trong chính câu INSERT (connection
    /// serialize sau Mutex nên subquery MAX(seq)+1 không đua).
    pub fn run_log_append(
        &self,
        run_id: &str,
        level: &str,
        step_id: Option<&str>,
        message: &str,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO run_log (run_id, seq, ts, level, step_id, message)
                 VALUES (?1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM run_log WHERE run_id = ?1),
                         ?2, ?3, ?4, ?5)",
                params![run_id, now(), level, step_id, message],
            )
        })?;
        Ok(())
    }

    /// `tail` dòng cuối, thứ tự tăng dần theo seq. Clamp 1..=500 (§8) — log là
    /// đầu ra LLM-safe, không bao giờ trả cả triệu dòng.
    pub fn run_log_tail(&self, run_id: &str, tail: i64) -> Result<Vec<RunLogLine>> {
        let tail = tail.clamp(1, 500);
        let mut rows: Vec<RunLogLine> = self.with_conn(|c| {
            let mut st = c.prepare(
                "SELECT seq, ts, level, step_id, message FROM run_log
                 WHERE run_id = ?1 ORDER BY seq DESC LIMIT ?2",
            )?;
            let rows = st.query_map(params![run_id, tail], |r| {
                Ok(RunLogLine {
                    seq: r.get(0)?,
                    ts: r.get(1)?,
                    level: r.get(2)?,
                    step_id: r.get(3)?,
                    message: r.get(4)?,
                })
            })?;
            rows.collect()
        })?;
        rows.reverse();
        Ok(rows)
    }

    /// Xóa log cũ hơn `retention_days` (maintenance tick, §8). Cutoff tính ở Rust
    /// để không build chuỗi SQL động.
    pub fn run_log_sweep(&self, retention_days: i64) -> Result<usize> {
        let days = retention_days.max(1);
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(days))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        self.with_conn(|c| c.execute("DELETE FROM run_log WHERE ts < ?1", params![cutoff]))
    }

    // ---- stats ----

    pub fn stats(&self) -> Result<LakeStats> {
        let cutoff_24h = (chrono::Utc::now() - chrono::Duration::hours(24))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        self.with_conn(|c| {
            let (datasets, total_rows, total_bytes) = c.query_row(
                "SELECT COUNT(*), COALESCE(SUM(row_count), 0), COALESCE(SUM(byte_size), 0)
                 FROM dataset",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let runs_active = c.query_row(
                "SELECT COUNT(*) FROM run WHERE status IN ('queued', 'running')",
                [],
                |r| r.get(0),
            )?;
            let runs_24h = c.query_row(
                "SELECT COUNT(*) FROM run WHERE updated_at >= ?1",
                params![cutoff_24h],
                |r| r.get(0),
            )?;
            Ok(LakeStats {
                datasets,
                total_rows,
                total_bytes,
                runs_active,
                runs_24h,
            })
        })
    }
}

// ---- SELECT lists + row mappers (index-based, khớp thứ tự cột bên dưới) ----

const DATASET_SELECT: &str = "SELECT id, namespace, name, format, layer, partition_cols,
        owner_flow_id, current_schema_version, row_count, byte_size, created_at, updated_at
 FROM dataset";

const FILE_SELECT: &str = "SELECT id, dataset_id, path, run_id, \"partition\", row_count,
        byte_size, stats, state, created_at, tombstoned_at
 FROM dataset_file";

const RUN_SELECT: &str =
    "SELECT id, flow_id, \"trigger\", status, started_at, ended_at, error, updated_at FROM run";

const FLOW_SELECT: &str = "SELECT id, name, def, def_version, enabled, schedule,
        last_scheduled_at, created_at, updated_at
 FROM flow";

/// Aggregate `row_count`/`byte_size` của dataset = tổng file ACTIVE — gọi trong
/// CÙNG transaction với mọi thay đổi manifest để hai con số không bao giờ lệch.
fn recompute_dataset_stats(c: &Connection, dataset_id: i64, ts: &str) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE dataset SET
            row_count = (SELECT COALESCE(SUM(row_count), 0) FROM dataset_file
                         WHERE dataset_id = ?1 AND state = 'active'),
            byte_size = (SELECT COALESCE(SUM(byte_size), 0) FROM dataset_file
                         WHERE dataset_id = ?1 AND state = 'active'),
            updated_at = ?2
         WHERE id = ?1",
        params![dataset_id, ts],
    )?;
    Ok(())
}

fn connection_from_row(r: &Row<'_>) -> rusqlite::Result<ConnectionInfo> {
    Ok(ConnectionInfo {
        id: r.get(0)?,
        kind: r.get(1)?,
        dsn: r.get(2)?,
        created_at: r.get(3)?,
        last_ok_at: r.get(4)?,
    })
}

fn dataset_from_row(r: &Row<'_>) -> rusqlite::Result<Dataset> {
    Ok(Dataset {
        id: r.get(0)?,
        namespace: r.get(1)?,
        name: r.get(2)?,
        format: r.get(3)?,
        layer: r.get(4)?,
        partition_cols: r.get(5)?,
        owner_flow_id: r.get(6)?,
        current_schema_version: r.get(7)?,
        row_count: r.get(8)?,
        byte_size: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

fn file_from_row(r: &Row<'_>) -> rusqlite::Result<DatasetFile> {
    Ok(DatasetFile {
        id: r.get(0)?,
        dataset_id: r.get(1)?,
        path: r.get(2)?,
        run_id: r.get(3)?,
        partition: r.get(4)?,
        row_count: r.get(5)?,
        byte_size: r.get(6)?,
        stats: r.get(7)?,
        state: r.get(8)?,
        created_at: r.get(9)?,
        tombstoned_at: r.get(10)?,
    })
}

fn schema_version_from_row(r: &Row<'_>) -> rusqlite::Result<SchemaVersion> {
    Ok(SchemaVersion {
        dataset_id: r.get(0)?,
        version: r.get(1)?,
        arrow_schema: r.get(2)?,
        change: r.get(3)?,
        created_at: r.get(4)?,
    })
}

fn flow_from_row(r: &Row<'_>) -> rusqlite::Result<FlowRow> {
    Ok(FlowRow {
        id: r.get(0)?,
        name: r.get(1)?,
        def: r.get(2)?,
        def_version: r.get(3)?,
        enabled: r.get::<_, i64>(4)? != 0,
        schedule: r.get(5)?,
        last_scheduled_at: r.get(6)?,
        created_at: r.get(7)?,
        updated_at: r.get(8)?,
    })
}

fn run_from_row(r: &Row<'_>) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?,
        flow_id: r.get(1)?,
        trigger: r.get(2)?,
        status: r.get(3)?,
        started_at: r.get(4)?,
        ended_at: r.get(5)?,
        error: r.get(6)?,
        updated_at: r.get(7)?,
    })
}

fn step_run_from_row(r: &Row<'_>) -> rusqlite::Result<StepRun> {
    Ok(StepRun {
        run_id: r.get(0)?,
        step_id: r.get(1)?,
        status: r.get(2)?,
        rows_read: r.get(3)?,
        rows_written: r.get(4)?,
        started_at: r.get(5)?,
        ended_at: r.get(6)?,
        error: r.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, rows: i64, bytes: i64) -> NewDatasetFile {
        NewDatasetFile {
            path: path.to_string(),
            partition: None,
            row_count: rows,
            byte_size: bytes,
            stats: None,
        }
    }

    fn created_id(r: RunCreate) -> String {
        match r {
            RunCreate::Created(id) => id,
            RunCreate::FlowBusy => panic!("expected Created, got FlowBusy"),
        }
    }

    #[test]
    fn open_twice_is_idempotent_and_keeps_user_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.sqlite");
        {
            let db = Db::open(&path).unwrap();
            assert_eq!(db.setting_i64("max_concurrent", 0), 2, "seed mặc định");
            db.dataset_upsert("raw", "đơn_hàng", None, None, None).unwrap();
            db.set_setting("max_concurrent", "5").unwrap();
        }
        // Mở lần 2: schema idempotent, seed không đè giá trị user, data còn nguyên.
        let db = Db::open(&path).unwrap();
        assert!(db.dataset_get("raw", "đơn_hàng").unwrap().is_some());
        assert_eq!(db.setting_i64("max_concurrent", 0), 5);
        assert_eq!(db.setting_i64("schema_version", 0), 1);
        // import_paths được seed từ config (JSON array).
        let paths: Vec<String> =
            serde_json::from_str(&db.setting("import_paths", "[]")).unwrap();
        assert!(!paths.is_empty());
    }

    #[test]
    fn unique_active_run_per_flow() {
        let db = Db::open_memory().unwrap();
        let id1 = created_id(db.run_create("f1", trigger::MANUAL).unwrap());
        // Run thứ hai cùng flow khi run đầu còn queued → lỗi phân biệt được.
        assert!(matches!(
            db.run_create("f1", trigger::SCHEDULE).unwrap(),
            RunCreate::FlowBusy
        ));
        // Vẫn busy khi run đầu đã sang running.
        assert!(db.run_claim(&id1).unwrap());
        assert!(matches!(
            db.run_create("f1", trigger::MCP).unwrap(),
            RunCreate::FlowBusy
        ));
        // Flow khác không bị ảnh hưởng.
        assert!(matches!(
            db.run_create("f2", trigger::MANUAL).unwrap(),
            RunCreate::Created(_)
        ));
        // Run đầu kết thúc → flow enqueue lại được.
        assert!(db
            .run_update_status_guarded(&id1, run_status::SUCCESS, None)
            .unwrap());
        assert!(matches!(
            db.run_create("f1", trigger::MANUAL).unwrap(),
            RunCreate::Created(_)
        ));
    }

    #[test]
    fn run_claim_is_atomic() {
        let db = Db::open_memory().unwrap();
        let id = created_id(db.run_create("f1", trigger::MANUAL).unwrap());

        assert!(db.run_claim(&id).unwrap(), "claim đầu phải thắng");
        assert!(!db.run_claim(&id).unwrap(), "claim thứ hai phải thua");
        assert_eq!(db.run_get(&id).unwrap().unwrap().status, run_status::RUNNING);
    }

    #[test]
    fn terminal_run_cannot_be_resurrected() {
        let db = Db::open_memory().unwrap();
        let id = created_id(db.run_create("f1", trigger::MANUAL).unwrap());
        db.run_claim(&id).unwrap();

        assert!(db
            .run_update_status_guarded(&id, run_status::FAILED, Some("lỗi kết nối"))
            .unwrap());
        // Worker in-flight cố ghi tiếp sau khi run đã terminal.
        assert!(!db
            .run_update_status_guarded(&id, run_status::RUNNING, None)
            .unwrap());
        assert!(!db
            .run_update_status_guarded(&id, run_status::SUCCESS, None)
            .unwrap());

        let run = db.run_get(&id).unwrap().unwrap();
        assert_eq!(run.status, run_status::FAILED);
        assert!(run.ended_at.is_some());
        assert_eq!(run.error.as_deref(), Some("lỗi kết nối"));
    }

    #[test]
    fn watermark_is_monotonic() {
        let db = Db::open_memory().unwrap();
        // Insert đầu (chưa có row) luôn được.
        assert!(db
            .stream_state_set_monotonic("f", "s", "updated_at", "2024-01-02 00:00:00", None)
            .unwrap());
        // Set LÙI bị từ chối — run chậm không đè watermark mới.
        assert!(!db
            .stream_state_set_monotonic("f", "s", "updated_at", "2024-01-01 00:00:00", None)
            .unwrap());
        // Bằng cũng từ chối (dedupe biên là việc của boundary_hashes).
        assert!(!db
            .stream_state_set_monotonic("f", "s", "updated_at", "2024-01-02 00:00:00", None)
            .unwrap());
        // Tiến thì được.
        assert!(db
            .stream_state_set_monotonic("f", "s", "updated_at", "2024-01-03 00:00:00", Some("[\"h1\"]"))
            .unwrap());
        let st = db.stream_state_get("f", "s").unwrap().unwrap();
        assert_eq!(st.last_value.as_deref(), Some("2024-01-03 00:00:00"));
        assert_eq!(st.boundary_hashes.as_deref(), Some("[\"h1\"]"));
    }

    #[test]
    fn manifest_active_to_tombstone_updates_aggregates() {
        let db = Db::open_memory().unwrap();
        let ds = db.dataset_upsert("raw", "orders", None, None, None).unwrap();

        db.manifest_add_files(
            ds,
            "run-1",
            &[file("raw/orders/part-run-1-0.parquet", 10, 100),
              file("raw/orders/part-run-1-1.parquet", 5, 50)],
        )
        .unwrap();
        let active = db.manifest_active_files(ds).unwrap();
        assert_eq!(active.len(), 2);
        let got = db.dataset_get_by_id(ds).unwrap().unwrap();
        assert_eq!((got.row_count, got.byte_size), (15, 150));

        // Tombstone file đầu — guarded state='active'.
        let first = active[0].id;
        assert_eq!(db.manifest_tombstone_files(ds, &[first]).unwrap(), 1);
        // Tombstone lại lần nữa không đổi gì.
        assert_eq!(db.manifest_tombstone_files(ds, &[first]).unwrap(), 0);

        let active = db.manifest_active_files(ds).unwrap();
        assert_eq!(active.len(), 1);
        let got = db.dataset_get_by_id(ds).unwrap().unwrap();
        assert_eq!((got.row_count, got.byte_size), (5, 50));

        // files_for_run thấy MỌI state — boot reconcile đối chiếu đĩa cần vậy.
        let all = db.manifest_files_for_run("run-1").unwrap();
        assert_eq!(all.len(), 2);
        let tomb = all.iter().find(|f| f.id == first).unwrap();
        assert_eq!(tomb.state, "tombstone");
        assert!(tomb.tombstoned_at.is_some());
    }

    #[test]
    fn dataset_owner_is_exclusive() {
        let db = Db::open_memory().unwrap();
        let ds = db.dataset_upsert("raw", "orders", None, None, None).unwrap();

        assert!(db.dataset_set_owner(ds, Some("f1")).unwrap());
        // Idempotent với cùng flow.
        assert!(db.dataset_set_owner(ds, Some("f1")).unwrap());
        // Flow thứ hai trỏ vào cùng target phải bị từ chối (§6.1).
        assert!(!db.dataset_set_owner(ds, Some("f2")).unwrap());
        assert_eq!(
            db.dataset_get_by_id(ds).unwrap().unwrap().owner_flow_id.as_deref(),
            Some("f1")
        );
        // Thả ownership rồi flow khác nhận được.
        assert!(db.dataset_set_owner(ds, None).unwrap());
        assert!(db.dataset_set_owner(ds, Some("f2")).unwrap());
    }

    #[test]
    fn dataset_upsert_does_not_change_format() {
        let db = Db::open_memory().unwrap();
        let a = db
            .dataset_upsert("raw", "orders", Some("parquet"), None, Some("[\"date\"]"))
            .unwrap();
        // Upsert lần 2 cùng (ns, name) trả cùng id, không đổi format có sẵn (§2.2).
        let b = db.dataset_upsert("raw", "orders", Some("delta"), Some("bronze"), None).unwrap();
        assert_eq!(a, b);
        let got = db.dataset_get("raw", "orders").unwrap().unwrap();
        assert_eq!(got.format, "parquet");
        assert_eq!(got.layer.as_deref(), Some("bronze"));
        assert_eq!(got.partition_cols.as_deref(), Some("[\"date\"]"));
    }

    #[test]
    fn schema_versions_increment_and_track_current() {
        let db = Db::open_memory().unwrap();
        let ds = db.dataset_upsert("raw", "orders", None, None, None).unwrap();
        assert!(db.schema_version_current(ds).unwrap().is_none());

        let v1 = db
            .schema_version_add(ds, "[{\"name\":\"id\",\"type\":\"Int64\"}]", Some("init"))
            .unwrap();
        let v2 = db
            .schema_version_add(ds, "[{\"name\":\"id\"},{\"name\":\"tên\"}]", Some("add tên"))
            .unwrap();
        assert_eq!((v1, v2), (1, 2));
        assert_eq!(db.schema_version_current(ds).unwrap().unwrap().version, 2);
        assert_eq!(db.schema_version_history(ds).unwrap().len(), 2);
        assert_eq!(
            db.dataset_get_by_id(ds).unwrap().unwrap().current_schema_version,
            Some(2)
        );
    }

    #[test]
    fn step_interval_upsert_replaces_and_filters_by_def_version() {
        let db = Db::open_memory().unwrap();
        db.step_interval_upsert("f", "s", 1, "2024-01-01", "2024-01-02", "r1", "failed")
            .unwrap();
        assert!(db.step_interval_list_success("f", "s", 1).unwrap().is_empty());

        // Run sau đè cùng interval (INSERT OR REPLACE theo PK).
        db.step_interval_upsert("f", "s", 1, "2024-01-01", "2024-01-02", "r2", "success")
            .unwrap();
        let ok = db.step_interval_list_success("f", "s", 1).unwrap();
        assert_eq!(ok.len(), 1);
        assert_eq!(ok[0].run_id, "r2");
        // def_version khác không được skip-lookup thấy.
        assert!(db.step_interval_list_success("f", "s", 2).unwrap().is_empty());
    }

    #[test]
    fn settings_validation_rejects_bad_keys_and_values() {
        assert!(validate_setting("nonsense", "1").is_err());
        assert!(validate_setting("schema_version", "9").is_err(), "key nội bộ");
        assert!(validate_setting("max_concurrent", "lots").is_err());
        assert!(validate_setting("max_concurrent", "0").is_err());
        assert!(validate_setting("max_concurrent", "9").is_err());
        assert!(validate_setting("max_concurrent", "1").is_ok());
        assert!(validate_setting("max_concurrent", "8").is_ok());
        assert!(validate_setting("memory_limit_mb", "128").is_err());
        assert!(validate_setting("memory_limit_mb", "2048").is_ok());
        assert!(validate_setting("query_max_seconds", "4").is_err());
        assert!(validate_setting("query_max_seconds", "600").is_ok());
        assert!(validate_setting("import_base64_max_mb", "65").is_err());
        // import_paths phải là JSON array chuỗi.
        assert!(validate_setting("import_paths", "không phải json").is_err());
        assert!(validate_setting("import_paths", "{\"a\":1}").is_err());
        assert!(validate_setting("import_paths", "[\"/tmp/hộp_thư\"]").is_ok());
        assert!(validate_setting("import_paths", "[]").is_ok());
    }

    #[test]
    fn run_log_tail_clamps_to_500_and_orders_ascending() {
        let db = Db::open_memory().unwrap();
        for i in 1..=600 {
            db.run_log_append("r1", "info", None, &format!("dòng {i}")).unwrap();
        }
        let t = db.run_log_tail("r1", 9_999).unwrap();
        assert_eq!(t.len(), 500, "clamp 500");
        assert_eq!(t.first().unwrap().seq, 101);
        assert_eq!(t.last().unwrap().seq, 600);

        let t = db.run_log_tail("r1", 3).unwrap();
        assert_eq!(t.iter().map(|l| l.seq).collect::<Vec<_>>(), vec![598, 599, 600]);
        assert_eq!(t.last().unwrap().message, "dòng 600");

        // tail=0 clamp lên 1, không phải câu SQL LIMIT 0.
        assert_eq!(db.run_log_tail("r1", 0).unwrap().len(), 1);
    }

    #[test]
    fn run_log_sweep_removes_only_old_lines() {
        let db = Db::open_memory().unwrap();
        db.run_log_append("r1", "info", Some("s1"), "dòng mới").unwrap();
        // Chèn thẳng 1 dòng cũ (append luôn stamp now nên phải đi cửa sau).
        db.with_conn(|c| {
            c.execute(
                "INSERT INTO run_log (run_id, seq, ts, level, step_id, message)
                 VALUES ('r1', 99, '2020-01-01 00:00:00', 'info', NULL, 'dòng cổ')",
                [],
            )
        })
        .unwrap();

        assert_eq!(db.run_log_sweep(14).unwrap(), 1);
        let left = db.run_log_tail("r1", 500).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].message, "dòng mới");
    }

    #[test]
    fn stats_counts_datasets_and_runs() {
        let db = Db::open_memory().unwrap();
        let ds = db.dataset_upsert("raw", "orders", None, None, None).unwrap();
        db.manifest_add_files(ds, "run-1", &[file("p.parquet", 7, 70)]).unwrap();
        let id = created_id(db.run_create("f1", trigger::MANUAL).unwrap());
        let done = created_id(db.run_create("f2", trigger::MANUAL).unwrap());
        db.run_claim(&done).unwrap();
        db.run_update_status_guarded(&done, run_status::SUCCESS, None).unwrap();

        let s = db.stats().unwrap();
        assert_eq!(s.datasets, 1);
        assert_eq!(s.total_rows, 7);
        assert_eq!(s.total_bytes, 70);
        assert_eq!(s.runs_active, 1, "chỉ run queued còn active");
        assert_eq!(s.runs_24h, 2);
        assert_eq!(db.runs_active_count().unwrap(), 1);
        let _ = id;
    }

    #[test]
    fn manifest_swap_replaces_active_atomically() {
        let db = Db::open_memory().unwrap();
        let ds = db.dataset_upsert("raw", "orders", None, None, None).unwrap();
        // Lần 1: 2 file active (10 + 5 dòng).
        db.manifest_add_files(
            ds,
            "run-1",
            &[file("a.parquet", 10, 100), file("b.parquet", 5, 50)],
        )
        .unwrap();
        assert_eq!(db.manifest_active_files(ds).unwrap().len(), 2);

        // Swap sang file mới: file cũ tombstone HẾT, chỉ file mới active — không cộng dồn.
        let tombstoned = db
            .manifest_swap_files(ds, "run-2", &[file("c.parquet", 7, 70)])
            .unwrap();
        assert_eq!(tombstoned, 2);
        let active = db.manifest_active_files(ds).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].run_id, "run-2");
        let got = db.dataset_get_by_id(ds).unwrap().unwrap();
        assert_eq!((got.row_count, got.byte_size), (7, 70), "aggregate = chỉ file active mới");
    }

    #[test]
    fn flow_upsert_keeps_def_version_on_update() {
        let db = Db::open_memory().unwrap();
        db.flow_upsert("f1", Some("Shop"), "{\"flow\":\"f1\"}", false, None).unwrap();
        let f = db.flow_get("f1").unwrap().unwrap();
        assert_eq!(f.def_version, 1);
        assert!(!f.enabled);

        db.flow_bump_def_version("f1").unwrap();
        // Upsert (edit) không được reset def_version về 1.
        db.flow_upsert("f1", Some("Shop"), "{\"flow\":\"f1\",\"v\":2}", true, Some("{\"every_minutes\":5}"))
            .unwrap();
        let f = db.flow_get("f1").unwrap().unwrap();
        assert_eq!(f.def_version, 2, "update giữ def_version đã bump");
        assert!(f.enabled);
        assert!(f.def.contains("\"v\":2"));
        assert_eq!(db.flow_list().unwrap().len(), 1);
        assert_eq!(db.flow_delete("f1").unwrap(), 1);
    }

    #[test]
    fn watchdog_sweeps_stuck_and_stale_runs() {
        let db = Db::open_memory().unwrap();
        // Run running kẹt (updated_at cũ) → failed.
        let stuck = created_id(db.run_create("f1", trigger::MANUAL).unwrap());
        db.run_claim(&stuck).unwrap();
        db.with_conn(|c| {
            c.execute(
                "UPDATE run SET updated_at = '2020-01-01 00:00:00' WHERE id = ?1",
                params![stuck],
            )
        })
        .unwrap();
        // Run queued bỏ rơi (updated_at cũ) → cancelled.
        let stale = created_id(db.run_create("f2", trigger::SCHEDULE).unwrap());
        db.with_conn(|c| {
            c.execute(
                "UPDATE run SET updated_at = '2020-01-01 00:00:00' WHERE id = ?1",
                params![stale],
            )
        })
        .unwrap();
        // Run queued mới (updated_at now) — KHÔNG bị đụng.
        let fresh = created_id(db.run_create("f3", trigger::MANUAL).unwrap());

        let cutoff = "2020-06-01 00:00:00";
        assert_eq!(db.run_fail_stuck_running(cutoff, "kẹt").unwrap(), 1);
        assert_eq!(db.run_cancel_stale_queued(cutoff, "bỏ rơi").unwrap(), 1);
        assert_eq!(db.run_get(&stuck).unwrap().unwrap().status, run_status::FAILED);
        assert_eq!(db.run_get(&stale).unwrap().unwrap().status, run_status::CANCELLED);
        assert_eq!(db.run_get(&fresh).unwrap().unwrap().status, run_status::QUEUED);
        assert_eq!(db.run_list_queued(10).unwrap(), vec![fresh]);
    }

    #[test]
    fn connection_roundtrip_and_delete() {
        let db = Db::open_memory().unwrap();
        db.connection_add("pg_main", "postgres", "postgres://u:secret@localhost/db")
            .unwrap();
        // Cùng id → cập nhật, không nhân đôi.
        db.connection_add("pg_main", "postgres", "postgres://u:mới@localhost/db")
            .unwrap();
        assert_eq!(db.connection_list().unwrap().len(), 1);
        let got = db.connection_get("pg_main").unwrap().unwrap();
        assert!(got.dsn.contains("mới"));
        assert!(got.last_ok_at.is_none());

        db.connection_mark_ok("pg_main").unwrap();
        assert!(db.connection_get("pg_main").unwrap().unwrap().last_ok_at.is_some());

        assert_eq!(db.connection_delete("pg_main").unwrap(), 1);
        assert!(db.connection_get("pg_main").unwrap().is_none());
    }

    #[test]
    fn run_list_filters_by_flow_and_status() {
        let db = Db::open_memory().unwrap();
        let a = created_id(db.run_create("f1", trigger::MANUAL).unwrap());
        db.run_claim(&a).unwrap();
        db.run_update_status_guarded(&a, run_status::SUCCESS, None).unwrap();
        let _b = created_id(db.run_create("f1", trigger::SCHEDULE).unwrap());
        let _c = created_id(db.run_create("f2", trigger::MANUAL).unwrap());

        assert_eq!(db.run_list(None, None, 100, 0).unwrap().len(), 3);
        assert_eq!(db.run_list(Some("f1"), None, 100, 0).unwrap().len(), 2);
        assert_eq!(
            db.run_list(Some("f1"), Some(run_status::SUCCESS), 100, 0).unwrap().len(),
            1
        );
        assert_eq!(db.run_list(None, Some(run_status::QUEUED), 100, 0).unwrap().len(), 2);
    }

    #[test]
    fn step_run_upsert_keeps_started_at_and_sets_ended_at() {
        let db = Db::open_memory().unwrap();
        db.step_run_upsert("r1", "s1", "running", 0, 0, None).unwrap();
        let first = db.step_runs_for("r1").unwrap()[0].clone();
        assert!(first.started_at.is_some());
        assert!(first.ended_at.is_none());

        db.step_run_upsert("r1", "s1", "success", 100, 100, None).unwrap();
        let done = db.step_runs_for("r1").unwrap()[0].clone();
        assert_eq!(done.status, "success");
        assert_eq!(done.started_at, first.started_at, "started_at giữ lần đầu");
        assert!(done.ended_at.is_some());
        assert_eq!((done.rows_read, done.rows_written), (100, 100));
    }
}
