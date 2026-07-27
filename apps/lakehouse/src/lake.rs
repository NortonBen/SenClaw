//! Ghi Parquet + manifest commit + GC + reconcile (design §2.2/§3.3).
//!
//! Nguyên tắc load-bearing (docs/data-lake-app-design.md §2.2):
//!   * **File trên đĩa TRƯỚC, manifest SAU** — file Parquet land xuống `lake/` là
//!     "vô hình" cho tới khi hàng của nó vào `dataset_file` (một transaction
//!     SQLite trong db.rs). Query không bao giờ quét thư mục.
//!   * Tên file chứa `run_id` (`part-<run_id>-<seq>.parquet`) nên boot reconcile
//!     đối chiếu được đĩa vs manifest: file thuộc run KHÔNG có hàng manifest =
//!     run không-commit → xóa vật lý.
//!   * GC chỉ xóa file tombstone quá grace ≥ 2× query_max_seconds (reader
//!     snapshot danh sách file lúc plan — §7).
//!
//! Codec schema (`schema_to_json`/`schema_from_json`) round-trip Arrow schema qua
//! catalog: đường đọc dựng ListingTable/MemTable từ **arrow_schema của catalog**,
//! CẤM inference (§6.4) — nên schema phải tái tạo đúng kiểu đã land.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde_json::{json, Map, Value};

use datafusion::arrow::array::{Array, ArrayRef, Float64Array, Int64Array, StringArray};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef, TimeUnit};
use datafusion::arrow::record_batch::RecordBatch;

use crate::api::AppState;
use crate::config;
use crate::db::{Db, NewDatasetFile};
use crate::ingest::IngestedTable;

/// Kết quả import một bảng vào lake.
#[derive(Debug, Clone)]
pub struct CreatedDataset {
    pub dataset_id: i64,
    pub row_count: i64,
}

// ---------------------------------------------------------------------------
// land + commit
// ---------------------------------------------------------------------------

/// Ghi `batches` thành MỘT file Parquet zstd vào `lake/<ns>/<ds>/`, trả manifest
/// entry (path TƯƠNG ĐỐI dưới lake/). File nằm trên đĩa nhưng CHƯA vào manifest —
/// caller commit riêng qua `db.manifest_add_files`. Batch rỗng (0 dòng) bị bỏ; nếu
/// không còn dòng nào → trả vec rỗng (không tạo file rác).
pub fn land_batches(
    ns: &str,
    ds: &str,
    run_id: &str,
    batches: &[RecordBatch],
    stats_cols: Option<&[String]>,
) -> Result<Vec<NewDatasetFile>> {
    land_batches_at(&config::lake_dir(), ns, ds, run_id, batches, stats_cols)
}

pub(crate) fn land_batches_at(
    root: &Path,
    ns: &str,
    ds: &str,
    run_id: &str,
    batches: &[RecordBatch],
    stats_cols: Option<&[String]>,
) -> Result<Vec<NewDatasetFile>> {
    let nonempty: Vec<RecordBatch> =
        batches.iter().filter(|b| b.num_rows() > 0).cloned().collect();
    if nonempty.is_empty() {
        return Ok(Vec::new());
    }
    let schema = nonempty[0].schema();

    let dir = root.join(ns).join(ds);
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow!("tạo thư mục lake '{}' thất bại: {e}", dir.display()))?;
    let seq = 0usize;
    let fname = format!("part-{run_id}-{seq}.parquet");
    let abspath = dir.join(&fname);
    let byte_size = write_parquet(&abspath, schema.clone(), &nonempty)?;

    let row_count: i64 = nonempty.iter().map(|b| b.num_rows() as i64).sum();
    let stats = compute_stats(&nonempty, &schema, stats_cols);
    // Manifest path luôn dùng '/' (portable, khớp reconcile parse tên file).
    let rel = format!("{ns}/{ds}/{fname}");

    Ok(vec![NewDatasetFile {
        path: rel,
        partition: None,
        row_count,
        byte_size: byte_size as i64,
        stats,
    }])
}

/// Ghi `batches` thành MỘT file Parquet cho MỘT partition `part_label` (§6.2
/// incremental_by_time). File nằm dưới `lake/<ns>/<ds>/<part>/part-<run_id>-0.parquet`
/// (subdir riêng để không đụng file partition khác cùng run). `partition` của manifest
/// entry = `part_label`. Batch rỗng → vec rỗng (không tạo file rác).
pub(crate) fn land_partition_at(
    root: &Path,
    ns: &str,
    ds: &str,
    run_id: &str,
    part_label: &str,
    batches: &[RecordBatch],
) -> Result<Vec<NewDatasetFile>> {
    let nonempty: Vec<RecordBatch> =
        batches.iter().filter(|b| b.num_rows() > 0).cloned().collect();
    if nonempty.is_empty() {
        return Ok(Vec::new());
    }
    let schema = nonempty[0].schema();
    // Tên thư mục partition đã vệ sinh (thay ký tự đường dẫn/khoảng trắng).
    let part_dir = sanitize_partition(part_label);
    let dir = root.join(ns).join(ds).join(&part_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow!("tạo thư mục partition '{}' thất bại: {e}", dir.display()))?;
    let fname = format!("part-{run_id}-0.parquet");
    let abspath = dir.join(&fname);
    let byte_size = write_parquet(&abspath, schema.clone(), &nonempty)?;
    let row_count: i64 = nonempty.iter().map(|b| b.num_rows() as i64).sum();
    let rel = format!("{ns}/{ds}/{part_dir}/{fname}");
    Ok(vec![NewDatasetFile {
        path: rel,
        partition: Some(part_label.to_string()),
        row_count,
        byte_size: byte_size as i64,
        stats: None,
    }])
}

/// Vệ sinh nhãn partition thành tên thư mục an toàn: chỉ giữ [a-z0-9_-], còn lại → '_'.
fn sanitize_partition(label: &str) -> String {
    let s: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "_".to_string()
    } else {
        s
    }
}

/// Import một `IngestedTable` thành dataset: upsert → land (đĩa) → schema_version →
/// manifest (commit). Thứ tự file-TRƯỚC-manifest-SAU là hợp đồng §2.2.
pub fn create_dataset_from_ingested(
    db: &Db,
    ns: &str,
    name: &str,
    table: &IngestedTable,
    run_id: &str,
) -> Result<CreatedDataset> {
    create_dataset_from_ingested_at(&config::lake_dir(), db, ns, name, table, run_id)
}

pub(crate) fn create_dataset_from_ingested_at(
    root: &Path,
    db: &Db,
    ns: &str,
    name: &str,
    table: &IngestedTable,
    run_id: &str,
) -> Result<CreatedDataset> {
    let dataset_id = db.dataset_upsert(ns, name, None, None, None)?;
    // 1. File xuống đĩa (vô hình).
    let files = land_batches_at(root, ns, name, run_id, &table.batches, None)?;
    // 2. Schema catalog — đường đọc dựng bảng từ đây, không infer (§6.4).
    db.schema_version_add(dataset_id, &schema_to_json(&table.schema), Some("import"))?;
    // 3. Manifest commit — file "hiện hình" sau bước này.
    db.manifest_add_files(dataset_id, run_id, &files)?;
    let row_count = files.iter().map(|f| f.row_count).sum();
    Ok(CreatedDataset {
        dataset_id,
        row_count,
    })
}

// ---------------------------------------------------------------------------
// GC — xóa vật lý file tombstone quá grace
// ---------------------------------------------------------------------------

/// Grace thực = max(gc_grace_seconds, 2× query_max_seconds) — reader snapshot
/// danh sách file lúc plan cần khoảng an toàn này (§2.2/§7).
fn grace_seconds(db: &Db) -> i64 {
    let g = db.setting_i64("gc_grace_seconds", 1200);
    let q = db.setting_i64("query_max_seconds", 600);
    g.max(2 * q)
}

pub fn gc(db: &Db) -> Result<usize> {
    gc_at(&config::lake_dir(), db)
}

pub(crate) fn gc_at(root: &Path, db: &Db) -> Result<usize> {
    let cutoff = (chrono::Utc::now() - chrono::Duration::seconds(grace_seconds(db)))
        .format("%Y-%m-%d %H:%M:%S")
        .to_string();
    let doomed = db.manifest_tombstones_before(&cutoff)?;
    let mut n = 0;
    for f in doomed {
        // Xóa file TRƯỚC, gỡ hàng manifest SAU: nếu crash ở giữa, lần GC sau thấy
        // file đã mất (bỏ qua) rồi vẫn gỡ được hàng — không kẹt tombstone vĩnh viễn.
        let abs = root.join(&f.path);
        if abs.exists() {
            let _ = std::fs::remove_file(&abs);
        }
        db.manifest_delete_file(f.id)?;
        n += 1;
    }
    Ok(n)
}

// ---------------------------------------------------------------------------
// compaction — gộp nhiều file nhỏ CÙNG partition thành 1 file (§12 Phase 4)
// ---------------------------------------------------------------------------

/// Báo cáo compaction một dataset.
#[derive(Debug, Clone, Default)]
pub struct CompactionReport {
    pub compacted: bool,
    /// Số partition đã gộp (mỗi partition >1 file → 1 file).
    pub partitions_compacted: usize,
    /// Số file active TRƯỚC khi gộp (chỉ trong các partition được gộp).
    pub files_before: usize,
    /// Số file mới sinh (= số partition được gộp).
    pub files_after: usize,
    pub rows: i64,
    /// run_id compaction (None nếu không có gì để gộp).
    pub run_id: Option<String>,
}

pub fn compact(db: &Db, dataset_id: i64) -> Result<CompactionReport> {
    compact_at(&config::lake_dir(), db, dataset_id)
}

/// Gộp file nhỏ theo partition. Nhóm file active theo `partition` (None = dataset
/// không phân vùng). Partition có >1 file → đọc union (ép về schema catalog §6.4) →
/// ghi 1 file mới dưới compaction run_id → tombstone file cũ + add file mới trong
/// MỘT transaction (`manifest_replace_files`). Idempotent: partition đã 1 file → bỏ
/// qua; không partition nào cần gộp → không tạo run, trả `compacted:false`.
pub(crate) fn compact_at(root: &Path, db: &Db, dataset_id: i64) -> Result<CompactionReport> {
    use std::collections::BTreeMap;

    let d = db
        .dataset_get_by_id(dataset_id)?
        .ok_or_else(|| anyhow!("dataset id {dataset_id} không tồn tại"))?;
    // Không có schema (chưa land gì) → không có gì để gộp.
    let schema = match db.schema_version_current(dataset_id)? {
        Some(sv) => schema_from_json(&sv.arrow_schema)?,
        None => return Ok(CompactionReport::default()),
    };

    let files = db.manifest_active_files(dataset_id)?;
    // Nhóm theo partition (Option<String>). BTreeMap giữ thứ tự ổn định (test tất định).
    let mut groups: BTreeMap<Option<String>, Vec<&crate::db::DatasetFile>> = BTreeMap::new();
    for f in &files {
        groups.entry(f.partition.clone()).or_default().push(f);
    }
    // Chỉ giữ partition có >1 file (nhóm 1 file đã "compact" — idempotent skip).
    let plan: Vec<(Option<String>, Vec<&crate::db::DatasetFile>)> = groups
        .into_iter()
        .filter(|(_, g)| g.len() > 1)
        .collect();
    if plan.is_empty() {
        return Ok(CompactionReport::default());
    }

    let label = format!("{}.{}", d.namespace, d.name);
    let run_id = db.run_create_compaction(&label)?;

    let mut old_ids: Vec<i64> = Vec::new();
    let mut new_files: Vec<NewDatasetFile> = Vec::new();
    let mut report = CompactionReport {
        compacted: true,
        run_id: Some(run_id.clone()),
        ..Default::default()
    };

    for (part, group) in plan {
        // Đọc union mọi file trong partition, ép về schema catalog (file cũ thiếu cột → NULL).
        let mut batches: Vec<RecordBatch> = Vec::new();
        for f in &group {
            let abs = root.join(&f.path);
            for b in read_parquet_file(&abs)? {
                batches.push(crate::engine::conform_batch(&b, &schema)?);
            }
        }
        // Ghi 1 file mới: partition None → thư mục dataset; Some → subdir partition.
        let written = match &part {
            None => land_batches_at(root, &d.namespace, &d.name, &run_id, &batches, None)?,
            Some(label) => {
                land_partition_at(root, &d.namespace, &d.name, &run_id, label, &batches)?
            }
        };
        report.files_before += group.len();
        report.partitions_compacted += 1;
        for f in &group {
            old_ids.push(f.id);
        }
        for nf in &written {
            report.rows += nf.row_count;
        }
        new_files.extend(written);
    }
    report.files_after = new_files.len();

    // Tombstone file cũ + add file mới trong MỘT transaction (atomic swap trên manifest).
    db.manifest_replace_files(dataset_id, &run_id, &old_ids, &new_files)?;
    Ok(report)
}

// ---------------------------------------------------------------------------
// boot reconcile
// ---------------------------------------------------------------------------

/// Run mồ côi → failed; file trên đĩa thuộc run KHÔNG có hàng manifest → xóa.
/// Gọi TRƯỚC khi runner nhận việc mới (main.rs). AppState hiện là scaffold chưa
/// giữ Db nên hàm tự mở catalog — idempotent, kết nối riêng (WAL cho phép).
pub fn boot_reconcile(_state: &AppState) {
    let db = match Db::open(&config::db_path()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("lakehouse reconcile: không mở được catalog: {e}");
            return;
        }
    };
    if let Err(e) = db.run_reconcile_orphans("daemon restart — run mồ côi đánh dấu failed") {
        eprintln!("lakehouse reconcile: đánh dấu run mồ côi thất bại: {e}");
    }
    match reconcile_orphan_files_at(&config::lake_dir(), &db) {
        Ok(n) if n > 0 => println!("lakehouse reconcile: xóa {n} file mồ côi ngoài manifest"),
        Ok(_) => {}
        Err(e) => eprintln!("lakehouse reconcile: quét file mồ côi thất bại: {e}"),
    }
}

pub(crate) fn reconcile_orphan_files_at(root: &Path, db: &Db) -> Result<usize> {
    let mut files = Vec::new();
    collect_parquet_files(root, &mut files);
    let mut removed = 0;
    for path in files {
        let Some(fname) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(run_id) = parse_run_id(fname) else {
            // Không đúng mẫu part-<run_id>-<seq>.parquet → không đụng vào.
            continue;
        };
        // Run có BẤT KỲ hàng manifest nào = đã commit → giữ mọi file của nó.
        let rows = db.manifest_files_for_run(&run_id)?;
        if rows.is_empty() {
            let _ = std::fs::remove_file(&path);
            removed += 1;
        }
    }
    Ok(removed)
}

/// `part-<run_id>-<seq>.parquet` → `run_id`. run_id là UUID (chứa '-') nên tách
/// seq bằng `rsplit_once('-')` từ phải, phần còn lại là run_id nguyên vẹn.
fn parse_run_id(fname: &str) -> Option<String> {
    let core = fname.strip_prefix("part-")?.strip_suffix(".parquet")?;
    let (run, seq) = core.rsplit_once('-')?;
    if run.is_empty() || seq.is_empty() {
        return None;
    }
    Some(run.to_string())
}

fn collect_parquet_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_parquet_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("parquet") {
            out.push(p);
        }
    }
}

// ---------------------------------------------------------------------------
// parquet I/O
// ---------------------------------------------------------------------------

pub(crate) fn write_parquet(path: &Path, schema: SchemaRef, batches: &[RecordBatch]) -> Result<u64> {
    use datafusion::parquet::arrow::ArrowWriter;
    use datafusion::parquet::basic::{Compression, ZstdLevel};
    use datafusion::parquet::file::properties::WriterProperties;

    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .build();
    let file = std::fs::File::create(path)
        .map_err(|e| anyhow!("tạo file parquet '{}' thất bại: {e}", path.display()))?;
    let mut writer = ArrowWriter::try_new(file, schema, Some(props))
        .map_err(|e| anyhow!("dựng parquet writer thất bại: {e}"))?;
    for b in batches {
        writer
            .write(b)
            .map_err(|e| anyhow!("ghi parquet batch thất bại: {e}"))?;
    }
    writer
        .close()
        .map_err(|e| anyhow!("đóng parquet writer thất bại: {e}"))?;
    Ok(std::fs::metadata(path)?.len())
}

/// Đọc lại một file Parquet thành RecordBatches (engine dùng, đường đọc từ manifest).
pub(crate) fn read_parquet_file(path: &Path) -> Result<Vec<RecordBatch>> {
    use datafusion::parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    let file = std::fs::File::open(path)
        .map_err(|e| anyhow!("mở parquet '{}' thất bại: {e}", path.display()))?;
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .map_err(|e| anyhow!("dựng parquet reader thất bại: {e}"))?
        .build()
        .map_err(|e| anyhow!("build parquet reader thất bại: {e}"))?;
    let mut out = Vec::new();
    for b in reader {
        out.push(b.map_err(|e| anyhow!("đọc parquet batch thất bại: {e}"))?);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// stats (min/max cho prune)
// ---------------------------------------------------------------------------

fn compute_stats(
    batches: &[RecordBatch],
    schema: &SchemaRef,
    stats_cols: Option<&[String]>,
) -> Option<String> {
    let cols = stats_cols?;
    let mut map = Map::new();
    for name in cols {
        let Ok(idx) = schema.index_of(name) else {
            continue;
        };
        let arrays: Vec<ArrayRef> = batches.iter().map(|b| b.column(idx).clone()).collect();
        if let Some((mn, mx)) = minmax(&arrays) {
            map.insert(name.clone(), json!({ "min": mn, "max": mx }));
        }
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map).to_string())
    }
}

/// Min/max của một cột (gộp qua các batch). Chỉ hỗ trợ Int64/Float64/Utf8 —
/// đủ cho prune PK/time_column; kiểu khác trả None (bỏ qua trong stats JSON).
fn minmax(arrays: &[ArrayRef]) -> Option<(Value, Value)> {
    let dt = arrays.first()?.data_type().clone();
    match dt {
        DataType::Int64 => {
            let mut lo: Option<i64> = None;
            let mut hi: Option<i64> = None;
            for a in arrays {
                let a = a.as_any().downcast_ref::<Int64Array>()?;
                for v in a.iter().flatten() {
                    lo = Some(lo.map_or(v, |x| x.min(v)));
                    hi = Some(hi.map_or(v, |x| x.max(v)));
                }
            }
            Some((json!(lo?), json!(hi?)))
        }
        DataType::Float64 => {
            let mut lo: Option<f64> = None;
            let mut hi: Option<f64> = None;
            for a in arrays {
                let a = a.as_any().downcast_ref::<Float64Array>()?;
                for v in a.iter().flatten() {
                    lo = Some(lo.map_or(v, |x| x.min(v)));
                    hi = Some(hi.map_or(v, |x| x.max(v)));
                }
            }
            Some((json!(lo?), json!(hi?)))
        }
        DataType::Utf8 => {
            let mut lo: Option<String> = None;
            let mut hi: Option<String> = None;
            for a in arrays {
                let a = a.as_any().downcast_ref::<StringArray>()?;
                for v in a.iter().flatten() {
                    if lo.as_deref().is_none_or(|x| v < x) {
                        lo = Some(v.to_string());
                    }
                    if hi.as_deref().is_none_or(|x| v > x) {
                        hi = Some(v.to_string());
                    }
                }
            }
            Some((json!(lo?), json!(hi?)))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// schema JSON codec — round-trip Arrow schema qua catalog (arrow_schema TEXT)
// ---------------------------------------------------------------------------

/// SchemaRef → JSON `[{"name","type","nullable"}]`. `type` là mã chuỗi ổn định
/// (xem `dtype_code`) để `schema_from_json` tái tạo đúng — đường đọc CẤM infer.
pub(crate) fn schema_to_json(schema: &SchemaRef) -> String {
    let fields: Vec<Value> = schema
        .fields()
        .iter()
        .map(|f| {
            json!({
                "name": f.name(),
                "type": dtype_code(f.data_type()),
                "nullable": f.is_nullable(),
            })
        })
        .collect();
    Value::Array(fields).to_string()
}

/// JSON `[{"name","type","nullable"}]` → SchemaRef. Kiểu lạ/thiếu → Utf8 nullable
/// (an toàn: cột đọc-ra-chuỗi thay vì fail dựng bảng).
pub(crate) fn schema_from_json(s: &str) -> Result<SchemaRef> {
    let v: Value = serde_json::from_str(s)
        .map_err(|e| anyhow!("arrow_schema catalog không phải JSON: {e}"))?;
    let arr = v
        .as_array()
        .ok_or_else(|| anyhow!("arrow_schema catalog phải là JSON array"))?;
    let mut fields = Vec::with_capacity(arr.len());
    for f in arr {
        let name = f
            .get("name")
            .and_then(|x| x.as_str())
            .ok_or_else(|| anyhow!("field thiếu 'name' trong arrow_schema"))?;
        let code = f.get("type").and_then(|x| x.as_str()).unwrap_or("utf8");
        let nullable = f.get("nullable").and_then(|x| x.as_bool()).unwrap_or(true);
        fields.push(Field::new(name, code_to_dtype(code), nullable));
    }
    Ok(Arc::new(Schema::new(fields)))
}

fn dtype_code(dt: &DataType) -> String {
    match dt {
        DataType::Boolean => "boolean".into(),
        DataType::Int8 => "int8".into(),
        DataType::Int16 => "int16".into(),
        DataType::Int32 => "int32".into(),
        DataType::Int64 => "int64".into(),
        DataType::UInt8 => "uint8".into(),
        DataType::UInt16 => "uint16".into(),
        DataType::UInt32 => "uint32".into(),
        DataType::UInt64 => "uint64".into(),
        DataType::Float32 => "float32".into(),
        DataType::Float64 => "float64".into(),
        DataType::Utf8 => "utf8".into(),
        DataType::LargeUtf8 => "large_utf8".into(),
        DataType::Date32 => "date32".into(),
        DataType::Date64 => "date64".into(),
        DataType::Binary => "binary".into(),
        DataType::Timestamp(unit, tz) => {
            let u = match unit {
                TimeUnit::Second => "s",
                TimeUnit::Millisecond => "ms",
                TimeUnit::Microsecond => "us",
                TimeUnit::Nanosecond => "ns",
            };
            match tz {
                Some(z) => format!("timestamp[{u},{z}]"),
                None => format!("timestamp[{u}]"),
            }
        }
        // Kiểu chưa ánh xạ → utf8 (đọc-ra-chuỗi khi register, không vỡ dựng bảng).
        _ => "utf8".into(),
    }
}

fn code_to_dtype(code: &str) -> DataType {
    match code {
        "boolean" => DataType::Boolean,
        "int8" => DataType::Int8,
        "int16" => DataType::Int16,
        "int32" => DataType::Int32,
        "int64" => DataType::Int64,
        "uint8" => DataType::UInt8,
        "uint16" => DataType::UInt16,
        "uint32" => DataType::UInt32,
        "uint64" => DataType::UInt64,
        "float32" => DataType::Float32,
        "float64" => DataType::Float64,
        "utf8" => DataType::Utf8,
        "large_utf8" => DataType::LargeUtf8,
        "date32" => DataType::Date32,
        "date64" => DataType::Date64,
        "binary" => DataType::Binary,
        other if other.starts_with("timestamp[") => {
            let inner = other.trim_start_matches("timestamp[").trim_end_matches(']');
            let (u, tz) = match inner.split_once(',') {
                Some((u, z)) => (u, Some(Arc::from(z))),
                None => (inner, None),
            };
            let unit = match u {
                "s" => TimeUnit::Second,
                "ms" => TimeUnit::Millisecond,
                "ns" => TimeUnit::Nanosecond,
                _ => TimeUnit::Microsecond,
            };
            DataType::Timestamp(unit, tz)
        }
        _ => DataType::Utf8,
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_ids_names() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("tên", DataType::Utf8, true),
        ]))
    }

    fn batch_ids_names(ids: Vec<Option<i64>>, names: Vec<Option<&str>>) -> RecordBatch {
        let names: Vec<Option<String>> = names.into_iter().map(|x| x.map(String::from)).collect();
        RecordBatch::try_new(
            schema_ids_names(),
            vec![
                Arc::new(Int64Array::from(ids)),
                Arc::new(StringArray::from(names)),
            ],
        )
        .unwrap()
    }

    #[test]
    fn schema_json_roundtrips_common_types() {
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("a", DataType::Int64, true),
            Field::new("b", DataType::Float64, true),
            Field::new("c", DataType::Boolean, false),
            Field::new("d", DataType::Date32, true),
            Field::new("e", DataType::Timestamp(TimeUnit::Microsecond, None), true),
            Field::new("tên", DataType::Utf8, true),
        ]));
        let js = schema_to_json(&schema);
        let back = schema_from_json(&js).unwrap();
        assert_eq!(back.fields().len(), schema.fields().len());
        for (x, y) in schema.fields().iter().zip(back.fields().iter()) {
            assert_eq!(x.name(), y.name());
            assert_eq!(x.data_type(), y.data_type(), "kiểu cột {}", x.name());
            assert_eq!(x.is_nullable(), y.is_nullable());
        }
    }

    #[test]
    fn land_writes_parquet_and_computes_stats() {
        let dir = tempfile::tempdir().unwrap();
        let b = batch_ids_names(vec![Some(3), Some(1), Some(2)], vec![Some("a"), None, Some("c")]);
        let stats_cols = vec!["id".to_string()];
        let files =
            land_batches_at(dir.path(), "raw", "orders", "run-x", &[b], Some(&stats_cols)).unwrap();
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.path, "raw/orders/part-run-x-0.parquet");
        assert_eq!(f.row_count, 3);
        assert!(f.byte_size > 0);
        assert!(dir.path().join("raw/orders/part-run-x-0.parquet").exists());
        let s: Value = serde_json::from_str(f.stats.as_ref().unwrap()).unwrap();
        assert_eq!(s["id"]["min"], json!(1));
        assert_eq!(s["id"]["max"], json!(3));
    }

    #[test]
    fn land_empty_batches_makes_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let empty = batch_ids_names(vec![], vec![]);
        let files = land_batches_at(dir.path(), "raw", "o", "run-e", &[empty], None).unwrap();
        assert!(files.is_empty());
        assert!(!dir.path().join("raw/o/part-run-e-0.parquet").exists());
    }

    #[test]
    fn create_dataset_lands_then_commits() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        let schema = schema_ids_names();
        let t = IngestedTable {
            name: "orders".into(),
            schema: schema.clone(),
            batches: vec![batch_ids_names(vec![Some(1), Some(2)], vec![Some("a"), Some("b")])],
            origin: "csv",
            note: "test".into(),
            rows: 2,
        };
        let created =
            create_dataset_from_ingested_at(dir.path(), &db, "raw", "orders", &t, "run-1").unwrap();
        assert_eq!(created.row_count, 2);
        let active = db.manifest_active_files(created.dataset_id).unwrap();
        assert_eq!(active.len(), 1);
        let ds = db.dataset_get_by_id(created.dataset_id).unwrap().unwrap();
        assert_eq!(ds.row_count, 2);
        assert_eq!(ds.current_schema_version, Some(1));
    }

    #[test]
    fn gc_removes_old_tombstone_keeps_recent() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        let ds = db.dataset_upsert("raw", "o", None, None, None).unwrap();

        let old = batch_ids_names(vec![Some(1)], vec![Some("x")]);
        let new = batch_ids_names(vec![Some(2)], vec![Some("y")]);
        let fold = land_batches_at(dir.path(), "raw", "o", "run-old", &[old], None).unwrap();
        let fnew = land_batches_at(dir.path(), "raw", "o", "run-new", &[new], None).unwrap();
        db.manifest_add_files(ds, "run-old", &fold).unwrap();
        db.manifest_add_files(ds, "run-new", &fnew).unwrap();
        let files = db.manifest_active_files(ds).unwrap();
        let id_old = files.iter().find(|f| f.run_id == "run-old").unwrap().id;
        let id_new = files.iter().find(|f| f.run_id == "run-new").unwrap().id;

        // Tombstone cả hai; dí tombstoned_at của cái cũ về quá khứ xa (>grace).
        db.manifest_tombstone_files(ds, &[id_old, id_new]).unwrap();
        db.with_conn(|c| {
            c.execute(
                "UPDATE dataset_file SET tombstoned_at = '2020-01-01 00:00:00' WHERE id = ?1",
                [id_old],
            )
        })
        .unwrap();

        let removed = gc_at(dir.path(), &db).unwrap();
        assert_eq!(removed, 1, "chỉ file tombstone cũ bị xóa");
        let path_old = dir.path().join(&fold[0].path);
        let path_new = dir.path().join(&fnew[0].path);
        assert!(!path_old.exists());
        assert!(path_new.exists());
        assert!(db.manifest_files_for_run("run-old").unwrap().is_empty());
        assert_eq!(db.manifest_files_for_run("run-new").unwrap().len(), 1);
    }

    #[test]
    fn reconcile_deletes_uncommitted_run_files_keeps_committed() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        let ds = db.dataset_upsert("raw", "o", None, None, None).unwrap();

        // run-good: land + commit vào manifest.
        let good = batch_ids_names(vec![Some(1)], vec![Some("g")]);
        let fgood = land_batches_at(dir.path(), "raw", "o", "run-good", &[good], None).unwrap();
        db.manifest_add_files(ds, "run-good", &fgood).unwrap();
        // run-bad: land nhưng KHÔNG commit (crash trước transaction).
        let bad = batch_ids_names(vec![Some(2)], vec![Some("b")]);
        let fbad = land_batches_at(dir.path(), "raw", "o", "run-bad", &[bad], None).unwrap();

        let good_path = dir.path().join(&fgood[0].path);
        let bad_path = dir.path().join(&fbad[0].path);
        assert!(good_path.exists() && bad_path.exists());

        let removed = reconcile_orphan_files_at(dir.path(), &db).unwrap();
        assert_eq!(removed, 1);
        assert!(good_path.exists(), "file đã commit phải sống");
        assert!(!bad_path.exists(), "file run không-commit phải bị xóa");
    }

    #[test]
    fn compact_merges_two_files_one_partition_preserves_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        let ds = db.dataset_upsert("raw", "o", None, None, None).unwrap();
        db.schema_version_add(ds, &schema_to_json(&schema_ids_names()), Some("init"))
            .unwrap();

        // Hai file KHÔNG partition (None), cùng dataset → hai lần append.
        let f1 = land_batches_at(dir.path(), "raw", "o", "r1", &[batch_ids_names(vec![Some(1), Some(2)], vec![Some("a"), Some("b")])], None).unwrap();
        let f2 = land_batches_at(dir.path(), "raw", "o", "r2", &[batch_ids_names(vec![Some(3)], vec![Some("c")])], None).unwrap();
        db.manifest_add_files(ds, "r1", &f1).unwrap();
        db.manifest_add_files(ds, "r2", &f2).unwrap();
        assert_eq!(db.manifest_active_files(ds).unwrap().len(), 2);

        let rep = compact_at(dir.path(), &db, ds).unwrap();
        assert!(rep.compacted);
        assert_eq!(rep.partitions_compacted, 1);
        assert_eq!(rep.files_before, 2);
        assert_eq!(rep.files_after, 1);
        assert_eq!(rep.rows, 3);

        // Manifest giờ đúng 1 file active, tổng dòng vẫn 3 (không mất/trùng).
        let active = db.manifest_active_files(ds).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active.iter().map(|f| f.row_count).sum::<i64>(), 3);

        // Query đọc lại: đúng 3 dòng phân biệt.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let page = rt
            .block_on(crate::engine::query_page_at(dir.path(), &db, "SELECT id FROM raw.o ORDER BY id", Some(100), None))
            .unwrap();
        assert_eq!(page.returned, 3);
        assert_eq!(page.rows[0][0], json!(1));
        assert_eq!(page.rows[2][0], json!(3));

        // Run compaction có trong DB với trigger 'compaction'.
        let run = db.run_get(rep.run_id.as_ref().unwrap()).unwrap().unwrap();
        assert_eq!(run.trigger, "compaction");
    }

    #[test]
    fn compact_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        let ds = db.dataset_upsert("raw", "o", None, None, None).unwrap();
        db.schema_version_add(ds, &schema_to_json(&schema_ids_names()), Some("init"))
            .unwrap();
        let f1 = land_batches_at(dir.path(), "raw", "o", "r1", &[batch_ids_names(vec![Some(1)], vec![Some("a")])], None).unwrap();
        let f2 = land_batches_at(dir.path(), "raw", "o", "r2", &[batch_ids_names(vec![Some(2)], vec![Some("b")])], None).unwrap();
        db.manifest_add_files(ds, "r1", &f1).unwrap();
        db.manifest_add_files(ds, "r2", &f2).unwrap();

        let r1 = compact_at(dir.path(), &db, ds).unwrap();
        assert!(r1.compacted);
        assert_eq!(db.manifest_active_files(ds).unwrap().len(), 1);

        // Lần hai: đã 1 file/partition → no-op, không tạo run.
        let r2 = compact_at(dir.path(), &db, ds).unwrap();
        assert!(!r2.compacted);
        assert_eq!(r2.run_id, None);
        assert_eq!(db.manifest_active_files(ds).unwrap().len(), 1);
    }

    #[test]
    fn compact_merges_per_partition() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        let ds = db.dataset_upsert("marts", "daily", None, None, None).unwrap();
        db.schema_version_add(ds, &schema_to_json(&schema_ids_names()), Some("init"))
            .unwrap();
        // Partition "2026-01-01": 2 file; partition "2026-01-02": 1 file.
        let a1 = land_partition_at(dir.path(), "marts", "daily", "r1", "2026-01-01", &[batch_ids_names(vec![Some(1)], vec![Some("a")])]).unwrap();
        let a2 = land_partition_at(dir.path(), "marts", "daily", "r2", "2026-01-01", &[batch_ids_names(vec![Some(2)], vec![Some("b")])]).unwrap();
        let b1 = land_partition_at(dir.path(), "marts", "daily", "r3", "2026-01-02", &[batch_ids_names(vec![Some(3)], vec![Some("c")])]).unwrap();
        db.manifest_add_files(ds, "r1", &a1).unwrap();
        db.manifest_add_files(ds, "r2", &a2).unwrap();
        db.manifest_add_files(ds, "r3", &b1).unwrap();
        assert_eq!(db.manifest_active_files(ds).unwrap().len(), 3);

        let rep = compact_at(dir.path(), &db, ds).unwrap();
        // Chỉ partition 2026-01-01 (2 file) được gộp; 2026-01-02 (1 file) bỏ qua.
        assert_eq!(rep.partitions_compacted, 1);
        assert_eq!(rep.files_before, 2);
        // Sau gộp: 1 (partition 1 gộp) + 1 (partition 2 giữ) = 2 file active.
        assert_eq!(db.manifest_active_files(ds).unwrap().len(), 2);
        assert_eq!(rep.rows, 2);
    }

    #[test]
    fn parse_run_id_handles_hyphenated_ids() {
        assert_eq!(parse_run_id("part-run-1-0.parquet").as_deref(), Some("run-1"));
        assert_eq!(
            parse_run_id("part-0190a1b2-c3d4-7abc-8def-000000000001-2.parquet").as_deref(),
            Some("0190a1b2-c3d4-7abc-8def-000000000001")
        );
        assert!(parse_run_id("random.parquet").is_none());
        assert!(parse_run_id("data.csv").is_none());
    }
}
