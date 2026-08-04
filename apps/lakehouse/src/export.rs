//! File export (§8 /query/export, §12 Phase 4) — ghi TOÀN BỘ kết quả một câu SELECT
//! (hoặc một dataset) ra file CSV/JSON/Parquet dưới `config::exports_dir()`, trả
//! đường dẫn + một cửa sổ preview nhỏ inline (pattern rs_story_export của
//! apps/rewrite-story: file đầy đủ trên đĩa, chỉ trả cửa sổ để agent không tràn context).
//!
//! Đây là nhánh **file** của export. Nhánh **DB-load** (ExportStep có `connection`) đã
//! có (Connector::load qua sqlx/rusqlite) và được `runner::execute_export` điều hướng —
//! không còn bị từ chối.
//!
//! Load-bearing:
//!   * Đọc qua `engine::collect_all_at` (SELECT-only, KHÔNG clamp limit) — export
//!     phải đầy đủ, khác query_page (phân trang 1000).
//!   * Cell KHÔNG cắt 500 ký tự (khác đường query): export phải nguyên vẹn. Preview
//!     inline thì cắt để gọn context.
//!   * Path file vệ sinh từ (namespace/dataset) — chỉ [a-z0-9_-]; chống path escape.

#![allow(dead_code)]

use std::path::Path;

use anyhow::{anyhow, Result};
use serde::Serialize;
use serde_json::Value;

use datafusion::arrow::array::{
    Array, ArrayRef, BooleanArray, Float32Array, Float64Array, Int16Array, Int32Array, Int64Array,
    Int8Array, LargeStringArray, StringArray, UInt16Array, UInt32Array, UInt64Array, UInt8Array,
};
use datafusion::arrow::compute::cast;
use datafusion::arrow::datatypes::{DataType, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;

use crate::config;
use crate::db::Db;
use crate::{engine, lake};

/// Số dòng tối đa trả inline trong report (cửa sổ preview — file đầy đủ vẫn trên đĩa).
const PREVIEW_ROWS: usize = 20;
/// Trần cắt cell trong PREVIEW (không áp cho file ghi ra đĩa).
const PREVIEW_CELL_MAX: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    Parquet,
}

impl ExportFormat {
    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_lowercase().as_str() {
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            "parquet" => Ok(Self::Parquet),
            other => Err(anyhow!(
                "format export không hợp lệ '{other}'; hợp lệ: csv, json, parquet"
            )),
        }
    }
    fn ext(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::Parquet => "parquet",
        }
    }
}

/// Kết quả export: file đầy đủ trên đĩa + cửa sổ preview inline.
#[derive(Debug, Clone, Serialize)]
pub struct ExportReport {
    /// Tên file dưới exports/ (dùng cho GET /api/exports/:file). KHÔNG phải đường dẫn tuyệt đối.
    pub file: String,
    /// Đường dẫn tuyệt đối trên đĩa (chẩn đoán).
    pub path: String,
    pub format: String,
    pub rows: i64,
    pub bytes: u64,
    pub columns: Vec<String>,
    /// Cửa sổ preview nhỏ (tối đa PREVIEW_ROWS dòng, cell cắt PREVIEW_CELL_MAX).
    pub preview: Vec<Vec<Value>>,
}

/// Export một dataset — `SELECT * FROM ns.dataset` mặc định, hoặc `sql` tùy chọn
/// (filter/projection). Ghi vào `config::exports_dir()`, đọc từ `config::lake_dir()`.
pub async fn export_dataset(
    db: &Db,
    ns: &str,
    dataset: &str,
    format: ExportFormat,
    sql: Option<&str>,
) -> Result<ExportReport> {
    export_dataset_at(
        &config::exports_dir(),
        &config::lake_dir(),
        db,
        ns,
        dataset,
        format,
        sql,
    )
    .await
}

pub(crate) async fn export_dataset_at(
    exports_root: &Path,
    lake_root: &Path,
    db: &Db,
    ns: &str,
    dataset: &str,
    format: ExportFormat,
    sql: Option<&str>,
) -> Result<ExportReport> {
    // 404-ý nghĩa: dataset không tồn tại → lỗi rõ (caller map sang ApiError).
    if db.dataset_get(ns, dataset)?.is_none() {
        return Err(anyhow!("không có dataset {ns}.{dataset}"));
    }
    let effective_sql = match sql {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => format!("SELECT * FROM \"{ns}\".\"{dataset}\""),
    };
    let slug = slugify(&format!("{ns}-{dataset}"));
    run_export(exports_root, lake_root, db, &effective_sql, format, &slug).await
}

/// Export kết quả một câu SELECT tùy ý (§8 /query/export). Slug cố định "query".
pub async fn export_query(db: &Db, sql: &str, format: ExportFormat) -> Result<ExportReport> {
    export_query_at(&config::exports_dir(), &config::lake_dir(), db, sql, format).await
}

pub(crate) async fn export_query_at(
    exports_root: &Path,
    lake_root: &Path,
    db: &Db,
    sql: &str,
    format: ExportFormat,
) -> Result<ExportReport> {
    if sql.trim().is_empty() {
        return Err(anyhow!("sql rỗng"));
    }
    run_export(exports_root, lake_root, db, sql.trim(), format, "query").await
}

/// Lõi export: collect toàn bộ → ghi file → dựng report + preview.
async fn run_export(
    exports_root: &Path,
    lake_root: &Path,
    db: &Db,
    sql: &str,
    format: ExportFormat,
    slug: &str,
) -> Result<ExportReport> {
    let (schema, batches) = engine::collect_all_at(lake_root, db, sql).await?;

    std::fs::create_dir_all(exports_root).map_err(|e| {
        anyhow!(
            "tạo thư mục exports '{}' thất bại: {e}",
            exports_root.display()
        )
    })?;
    let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let fname = format!("{slug}-{ts}.{}", format.ext());
    let abspath = exports_root.join(&fname);

    let bytes = match format {
        ExportFormat::Parquet => lake::write_parquet(&abspath, schema.clone(), &batches)?,
        ExportFormat::Csv => write_csv(&abspath, &schema, &batches)?,
        ExportFormat::Json => write_json(&abspath, &schema, &batches)?,
    };

    let columns: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    let rows: i64 = batches.iter().map(|b| b.num_rows() as i64).sum();
    let preview = build_preview(&batches);

    Ok(ExportReport {
        file: fname,
        path: abspath.to_string_lossy().to_string(),
        format: format.ext().to_string(),
        rows,
        bytes,
        columns,
        preview,
    })
}

/// Cửa sổ preview: tối đa PREVIEW_ROWS dòng đầu, cell cắt PREVIEW_CELL_MAX ký tự.
fn build_preview(batches: &[RecordBatch]) -> Vec<Vec<Value>> {
    let mut out = Vec::new();
    'outer: for b in batches {
        for r in 0..b.num_rows() {
            if out.len() >= PREVIEW_ROWS {
                break 'outer;
            }
            let mut row = Vec::with_capacity(b.num_columns());
            for c in 0..b.num_columns() {
                row.push(cell_preview(b.column(c), r));
            }
            out.push(row);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// CSV — cast từng cột về Utf8 rồi ghi qua crate `csv` (không cần arrow csv-writer feature)
// ---------------------------------------------------------------------------

fn write_csv(path: &Path, schema: &SchemaRef, batches: &[RecordBatch]) -> Result<u64> {
    let file = std::fs::File::create(path)
        .map_err(|e| anyhow!("tạo file csv '{}' thất bại: {e}", path.display()))?;
    let mut w = csv::Writer::from_writer(file);
    // Header.
    let headers: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    w.write_record(&headers)
        .map_err(|e| anyhow!("ghi header csv thất bại: {e}"))?;

    for b in batches {
        // Cast mọi cột về Utf8 một lần/ batch (rẻ hơn cast từng cell).
        let cols: Vec<ArrayRef> = b
            .columns()
            .iter()
            .map(|c| cast(c, &DataType::Utf8).unwrap_or_else(|_| c.clone()))
            .collect();
        for r in 0..b.num_rows() {
            let mut rec: Vec<String> = Vec::with_capacity(cols.len());
            for c in &cols {
                rec.push(utf8_cell(c, r));
            }
            w.write_record(&rec)
                .map_err(|e| anyhow!("ghi dòng csv thất bại: {e}"))?;
        }
    }
    w.flush().map_err(|e| anyhow!("flush csv thất bại: {e}"))?;
    Ok(std::fs::metadata(path)?.len())
}

/// Ô đã cast-về-Utf8 → String; null → chuỗi rỗng.
fn utf8_cell(arr: &ArrayRef, i: usize) -> String {
    if arr.is_null(i) {
        return String::new();
    }
    if let Some(s) = arr.as_any().downcast_ref::<StringArray>() {
        return s.value(i).to_string();
    }
    if let Some(s) = arr.as_any().downcast_ref::<LargeStringArray>() {
        return s.value(i).to_string();
    }
    String::new()
}

// ---------------------------------------------------------------------------
// JSON — mảng object {col: value}, giữ kiểu số/bool, KHÔNG cắt chuỗi (file đầy đủ)
// ---------------------------------------------------------------------------

fn write_json(path: &Path, schema: &SchemaRef, batches: &[RecordBatch]) -> Result<u64> {
    let names: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    let mut arr: Vec<Value> = Vec::new();
    for b in batches {
        for r in 0..b.num_rows() {
            let mut obj = serde_json::Map::new();
            for (c, name) in names.iter().enumerate() {
                obj.insert(name.clone(), cell_full(b.column(c), r));
            }
            arr.push(Value::Object(obj));
        }
    }
    let text = serde_json::to_string(&Value::Array(arr))
        .map_err(|e| anyhow!("serialize json export thất bại: {e}"))?;
    std::fs::write(path, text.as_bytes())
        .map_err(|e| anyhow!("ghi file json '{}' thất bại: {e}", path.display()))?;
    Ok(std::fs::metadata(path)?.len())
}

// ---------------------------------------------------------------------------
// cell → JSON
// ---------------------------------------------------------------------------

/// Ô Arrow → Value ĐẦY ĐỦ (không cắt chuỗi) — dùng cho file JSON.
fn cell_full(arr: &ArrayRef, i: usize) -> Value {
    cell_value(arr, i, None)
}

/// Ô Arrow → Value cho PREVIEW (cắt chuỗi PREVIEW_CELL_MAX ký tự trên char boundary).
fn cell_preview(arr: &ArrayRef, i: usize) -> Value {
    cell_value(arr, i, Some(PREVIEW_CELL_MAX))
}

fn cell_value(arr: &ArrayRef, i: usize, cell_max: Option<usize>) -> Value {
    if arr.is_null(i) {
        return Value::Null;
    }
    macro_rules! num {
        ($ty:ty) => {
            Value::from(arr.as_any().downcast_ref::<$ty>().unwrap().value(i))
        };
    }
    let str_val = |s: &str| -> Value {
        match cell_max {
            Some(max) if s.chars().count() > max => Value::String(s.chars().take(max).collect()),
            _ => Value::String(s.to_string()),
        }
    };
    match arr.data_type() {
        DataType::Boolean => Value::Bool(
            arr.as_any()
                .downcast_ref::<BooleanArray>()
                .unwrap()
                .value(i),
        ),
        DataType::Int8 => num!(Int8Array),
        DataType::Int16 => num!(Int16Array),
        DataType::Int32 => num!(Int32Array),
        DataType::Int64 => num!(Int64Array),
        DataType::UInt8 => num!(UInt8Array),
        DataType::UInt16 => num!(UInt16Array),
        DataType::UInt32 => num!(UInt32Array),
        DataType::UInt64 => num!(UInt64Array),
        DataType::Float32 => {
            let v = arr
                .as_any()
                .downcast_ref::<Float32Array>()
                .unwrap()
                .value(i) as f64;
            serde_json::Number::from_f64(v)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        DataType::Float64 => {
            let v = arr
                .as_any()
                .downcast_ref::<Float64Array>()
                .unwrap()
                .value(i);
            serde_json::Number::from_f64(v)
                .map(Value::Number)
                .unwrap_or(Value::Null)
        }
        DataType::Utf8 => str_val(arr.as_any().downcast_ref::<StringArray>().unwrap().value(i)),
        DataType::LargeUtf8 => str_val(
            arr.as_any()
                .downcast_ref::<LargeStringArray>()
                .unwrap()
                .value(i),
        ),
        // Date/Timestamp/Binary/… → cast về chuỗi hiển thị.
        _ => match cast(arr, &DataType::Utf8) {
            Ok(s) => match s.as_any().downcast_ref::<StringArray>() {
                Some(s) if !s.is_null(i) => str_val(s.value(i)),
                _ => Value::Null,
            },
            Err(_) => Value::Null,
        },
    }
}

// ---------------------------------------------------------------------------
// path helpers
// ---------------------------------------------------------------------------

/// Vệ sinh slug tên file: chỉ [a-z0-9_-], còn lại → '_'; rỗng → "export".
fn slugify(s: &str) -> String {
    let out: String = s
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "export".to_string()
    } else {
        trimmed
    }
}

/// Đọc một file export để download (GET /api/exports/:file). CHẶN path traversal:
/// `name` phải là tên file trần (không có '/'/'\\'/'..'); resolve rồi kiểm prefix
/// exports_dir trên đường dẫn ĐÃ canonicalize. Trả (bytes, tên file an toàn).
pub fn read_export_file(name: &str) -> Result<Vec<u8>> {
    read_export_file_at(&config::exports_dir(), name)
}

pub(crate) fn read_export_file_at(exports_root: &Path, name: &str) -> Result<Vec<u8>> {
    // Từ chối ngay tên chứa thành phần đường dẫn (defense-in-depth trước cả canonicalize).
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || Path::new(name).components().count() != 1
    {
        return Err(anyhow!(
            "tên file export không hợp lệ (chỉ tên file trần): '{name}'"
        ));
    }
    let root = exports_root
        .canonicalize()
        .map_err(|e| anyhow!("thư mục exports không mở được: {e}"))?;
    let target = root.join(name);
    let target = target
        .canonicalize()
        .map_err(|_| anyhow!("không có file export '{name}'"))?;
    if !target.starts_with(&root) {
        return Err(anyhow!("path '{name}' ngoài thư mục exports — bị chặn"));
    }
    std::fs::read(&target).map_err(|e| anyhow!("đọc file export '{name}' thất bại: {e}"))
}

/// Content-type gợi ý theo đuôi file export (download).
pub fn content_type_for(name: &str) -> &'static str {
    match Path::new(name).extension().and_then(|s| s.to_str()) {
        Some("csv") => "text/csv; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("parquet") => "application/vnd.apache.parquet",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::IngestedTable;
    use datafusion::arrow::datatypes::{Field, Schema};
    use std::sync::Arc;

    fn schema3() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("city", DataType::Utf8, true),
            Field::new("amount", DataType::Float64, true),
        ]))
    }

    fn batch(
        ids: Vec<Option<i64>>,
        cities: Vec<Option<&str>>,
        amts: Vec<Option<f64>>,
    ) -> RecordBatch {
        let cities: Vec<Option<String>> = cities.into_iter().map(|x| x.map(String::from)).collect();
        RecordBatch::try_new(
            schema3(),
            vec![
                Arc::new(Int64Array::from(ids)),
                Arc::new(StringArray::from(cities)),
                Arc::new(Float64Array::from(amts)),
            ],
        )
        .unwrap()
    }

    fn import(lake_root: &Path, db: &Db, ns: &str, name: &str, batches: Vec<RecordBatch>) {
        let rows = batches.iter().map(|b| b.num_rows()).sum();
        let t = IngestedTable {
            name: name.into(),
            schema: schema3(),
            batches,
            origin: "csv",
            note: "t".into(),
            rows,
        };
        lake::create_dataset_from_ingested_at(lake_root, db, ns, name, &t, "run-1").unwrap();
    }

    #[tokio::test]
    async fn export_csv_writes_full_file() {
        let exp = tempfile::tempdir().unwrap();
        let lk = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        import(
            lk.path(),
            &db,
            "raw",
            "orders",
            vec![batch(
                vec![Some(1), Some(2), Some(3)],
                vec![Some("hanoi"), Some("hue"), Some("hcm")],
                vec![Some(1.5), Some(2.0), None],
            )],
        );
        let rep = export_dataset_at(
            exp.path(),
            lk.path(),
            &db,
            "raw",
            "orders",
            ExportFormat::Csv,
            None,
        )
        .await
        .unwrap();
        assert_eq!(rep.rows, 3);
        assert!(rep.bytes > 0);
        let abs = exp.path().join(&rep.file);
        assert!(abs.exists());
        let text = std::fs::read_to_string(&abs).unwrap();
        // Header + 3 dòng dữ liệu = 4 dòng (crate csv kết thúc bằng '\n').
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 4, "csv: {text}");
        assert_eq!(lines[0], "id,city,amount");
        assert!(lines[1].starts_with("1,hanoi"));
        // null amount → cell rỗng ở dòng 3.
        assert!(lines[3].ends_with(","), "null → rỗng: {}", lines[3]);
    }

    #[tokio::test]
    async fn export_json_is_array_of_objects() {
        let exp = tempfile::tempdir().unwrap();
        let lk = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        import(
            lk.path(),
            &db,
            "raw",
            "orders",
            vec![batch(
                vec![Some(1), Some(2)],
                vec![Some("a"), None],
                vec![Some(9.0), Some(8.0)],
            )],
        );
        let rep = export_dataset_at(
            exp.path(),
            lk.path(),
            &db,
            "raw",
            "orders",
            ExportFormat::Json,
            None,
        )
        .await
        .unwrap();
        assert_eq!(rep.rows, 2);
        let text = std::fs::read_to_string(exp.path().join(&rep.file)).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["id"], json_i(1));
        assert_eq!(arr[0]["city"], Value::String("a".into()));
        assert_eq!(arr[1]["city"], Value::Null, "null giữ null trong json");
    }

    #[tokio::test]
    async fn export_parquet_roundtrips() {
        let exp = tempfile::tempdir().unwrap();
        let lk = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        import(
            lk.path(),
            &db,
            "raw",
            "orders",
            vec![batch(
                vec![Some(1), Some(2), Some(3)],
                vec![Some("a"), Some("b"), Some("c")],
                vec![Some(1.0), Some(2.0), Some(3.0)],
            )],
        );
        let rep = export_dataset_at(
            exp.path(),
            lk.path(),
            &db,
            "raw",
            "orders",
            ExportFormat::Parquet,
            None,
        )
        .await
        .unwrap();
        assert_eq!(rep.rows, 3);
        // Đọc lại file parquet → đúng 3 dòng.
        let back = lake::read_parquet_file(&exp.path().join(&rep.file)).unwrap();
        let n: usize = back.iter().map(|b| b.num_rows()).sum();
        assert_eq!(n, 3);
    }

    #[tokio::test]
    async fn export_with_custom_sql_filters() {
        let exp = tempfile::tempdir().unwrap();
        let lk = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        import(
            lk.path(),
            &db,
            "raw",
            "orders",
            vec![batch(
                vec![Some(1), Some(2), Some(3)],
                vec![Some("a"), Some("b"), Some("c")],
                vec![Some(10.0), Some(20.0), Some(30.0)],
            )],
        );
        let rep = export_dataset_at(
            exp.path(),
            lk.path(),
            &db,
            "raw",
            "orders",
            ExportFormat::Csv,
            Some("SELECT id FROM raw.orders WHERE amount >= 20 ORDER BY id"),
        )
        .await
        .unwrap();
        assert_eq!(rep.rows, 2, "filter amount>=20 → 2 dòng");
        assert_eq!(rep.columns, vec!["id"]);
    }

    #[tokio::test]
    async fn export_missing_dataset_errors() {
        let exp = tempfile::tempdir().unwrap();
        let lk = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        let err = export_dataset_at(
            exp.path(),
            lk.path(),
            &db,
            "raw",
            "nope",
            ExportFormat::Csv,
            None,
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("không có dataset"), "{err}");
    }

    #[test]
    fn download_rejects_path_traversal() {
        let exp = tempfile::tempdir().unwrap();
        // File hợp lệ trong exports.
        std::fs::write(exp.path().join("ok.csv"), b"a,b\n1,2\n").unwrap();
        assert!(read_export_file_at(exp.path(), "ok.csv").is_ok());
        // Traversal + tên có đường dẫn → chặn.
        assert!(read_export_file_at(exp.path(), "../secret").is_err());
        assert!(read_export_file_at(exp.path(), "sub/ok.csv").is_err());
        assert!(read_export_file_at(exp.path(), "..").is_err());
        assert!(read_export_file_at(exp.path(), "").is_err());
    }

    #[test]
    fn format_parse_guards() {
        assert_eq!(ExportFormat::parse("CSV").unwrap(), ExportFormat::Csv);
        assert_eq!(ExportFormat::parse(" json ").unwrap(), ExportFormat::Json);
        assert!(ExportFormat::parse("xlsx").is_err());
    }

    fn json_i(n: i64) -> Value {
        Value::from(n)
    }
}
