//! Query path (warehouse) — SessionContext + manifest→bảng + SQLOptions chặn ghi.
//! Thiết kế: docs/data-lake-app-design.md §6.4 (đường đọc), §7 (query path).
//!
//! Quy tắc load-bearing:
//!   * **Đăng ký bảng từ MANIFEST + arrow_schema catalog** — không quét thư mục,
//!     KHÔNG schema inference (§6.4). Mỗi file active đọc rồi ép về schema catalog:
//!     cột thiếu → NULL, kiểu khác → cast lossless, cast fail → NULL. File cũ
//!     thiếu cột mới đọc ra NULL đúng như §6.4.
//!   * **Chặn ghi bằng `SQLOptions`** (§7) — `verify_plan` soi cả LogicalPlan,
//!     bắt được DML lồng trong `EXPLAIN ANALYZE INSERT` (variant-filter tay bị lách).
//!   * Parse-first chỉ để chặn multi-statement + báo lỗi thân thiện.
//!   * Kết quả LLM-safe: default 100 row, clamp 1..1000, cắt cell chuỗi 500 ký tự
//!     TRÊN char boundary (trap UTF-8 tiếng Việt), kèm has_more/total_estimate.

#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

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
use datafusion::catalog::MemorySchemaProvider;
use datafusion::common::TableReference;
use datafusion::datasource::MemTable;
use datafusion::execution::memory_pool::GreedyMemoryPool;
use datafusion::execution::runtime_env::RuntimeEnvBuilder;
use datafusion::prelude::{SQLOptions, SessionConfig, SessionContext};

use crate::config;
use crate::db::Db;
use crate::lake;

/// Trần cắt cell chuỗi trước khi trả client (§7).
const CELL_MAX_CHARS: usize = 500;

/// Một trang kết quả query (LLM-safe).
#[derive(Debug, Clone, Serialize)]
pub struct QueryPage {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub returned: usize,
    pub has_more: bool,
    /// Tổng số dòng CHÍNH XÁC khi đã đọc hết (không has_more); None khi còn trang sau.
    pub total_estimate: Option<i64>,
}

// ---------------------------------------------------------------------------
// session + đăng ký bảng
// ---------------------------------------------------------------------------

/// SessionContext theo settings: GreedyMemoryPool (memory_limit_mb) +
/// target_partitions + information_schema (embed phải tự bật — §7).
pub fn session(db: &Db) -> SessionContext {
    let mem_mb = db.setting_i64("memory_limit_mb", 2048).clamp(64, 65_536) as usize;
    let target = db.setting_i64("target_partitions", 4).clamp(1, 64) as usize;

    let pool = Arc::new(GreedyMemoryPool::new(mem_mb * 1024 * 1024));
    let rt = RuntimeEnvBuilder::new()
        .with_memory_pool(pool)
        .build_arc()
        .expect("dựng RuntimeEnv thất bại");
    let cfg = SessionConfig::new()
        .with_target_partitions(target)
        .with_information_schema(true);
    SessionContext::new_with_config_rt(cfg, rt)
}

/// Đăng ký mọi dataset active dưới tên `<namespace>.<dataset>` từ MANIFEST —
/// file active tuyệt đối + schema catalog, KHÔNG infer (§6.4/§7).
pub fn register_datasets(ctx: &SessionContext, db: &Db) -> Result<()> {
    register_datasets_at(ctx, db, &config::lake_dir())
}

pub(crate) fn register_datasets_at(ctx: &SessionContext, db: &Db, root: &Path) -> Result<()> {
    for d in db.dataset_list(None, 500, 0)? {
        if let Some((schema, batches)) = load_dataset_batches(db, root, &d)? {
            register_one(ctx, &d.namespace, &d.name, schema, batches)?;
        }
    }
    Ok(())
}

/// Đọc mọi file active của một dataset, ép về schema catalog hiện hành (§6.4). Trả
/// `None` khi dataset chưa có schema_version (chưa land gì) — không dựng bảng vô kiểu.
pub(crate) fn load_dataset_batches(
    db: &Db,
    root: &Path,
    d: &crate::db::Dataset,
) -> Result<Option<(SchemaRef, Vec<RecordBatch>)>> {
    let schema = match db.schema_version_current(d.id)? {
        Some(sv) => lake::schema_from_json(&sv.arrow_schema)?,
        None => return Ok(None),
    };
    let files = db.manifest_active_files(d.id)?;
    let mut batches: Vec<RecordBatch> = Vec::new();
    for f in &files {
        let abs = root.join(&f.path);
        for b in lake::read_parquet_file(&abs)? {
            batches.push(conform_batch(&b, &schema)?);
        }
    }
    Ok(Some((schema, batches)))
}

/// Đăng ký input cho một transform (§6.2): mọi dataset dưới `<ns>.<dataset>` cộng
/// một alias TRẦN theo `id` của mỗi source/transform step (SQL transform tham chiếu
/// bảng bằng step id — khớp `derive_dag`). Alias chỉ đăng ký khi dataset đã có dữ
/// liệu (đã land); step chưa chạy → bỏ qua (transform sẽ lỗi "table not found" rõ).
pub(crate) fn register_flow_inputs_at(
    ctx: &SessionContext,
    db: &Db,
    def: &crate::flow::FlowDef,
    root: &Path,
) -> Result<()> {
    register_datasets_at(ctx, db, root)?;
    // (alias trần, (ns, dataset)) của từng step.
    let mut aliases: Vec<(String, (String, String))> = Vec::new();
    for s in &def.sources {
        aliases.push((s.id.clone(), crate::flow::source_target(s)));
    }
    for t in &def.transforms {
        aliases.push((t.id.clone(), crate::flow::transform_target(t)));
    }
    for (alias, (ns, name)) in aliases {
        let Some(d) = db.dataset_get(&ns, &name)? else {
            continue;
        };
        if let Some((schema, batches)) = load_dataset_batches(db, root, &d)? {
            // Đăng ký dưới tên trần `alias` (bảng cùng catalog mặc định, schema mặc định).
            let mem = MemTable::try_new(schema, vec![batches])
                .map_err(|e| anyhow!("dựng MemTable alias '{alias}' thất bại: {e}"))?;
            // register_table dưới alias trần đè nếu trùng — idempotent.
            ctx.register_table(TableReference::bare(alias.clone()), Arc::new(mem))
                .map_err(|e| anyhow!("đăng ký alias '{alias}' thất bại: {e}"))?;
        }
    }
    Ok(())
}

/// Chạy một câu SELECT của transform trên input flow (register_flow_inputs) và thu
/// RecordBatches. SELECT-only (SQLOptions chặn DDL/DML/statement — §7). Trả cả schema
/// kết quả (để land) lẫn batches.
pub(crate) async fn transform_select_at(
    root: &Path,
    db: &Db,
    def: &crate::flow::FlowDef,
    sql: &str,
) -> Result<(SchemaRef, Vec<RecordBatch>)> {
    ensure_single_statement(sql)?;
    let ctx = session(db);
    register_flow_inputs_at(&ctx, db, def, root)?;
    let df = ctx.sql_with_options(sql, select_only()).await?;
    let schema: SchemaRef = Arc::new(df.schema().as_arrow().clone());
    let batches = df.collect().await?;
    Ok((schema, batches))
}

/// Ép một batch về schema catalog: theo TÊN cột (§6.4). Cột catalog thiếu trong
/// file → NULL; kiểu khác → cast lossless, cast fail → NULL (không vỡ đọc). Cột
/// dư trong file bị bỏ.
pub(crate) fn conform_batch(batch: &RecordBatch, target: &SchemaRef) -> Result<RecordBatch> {
    let n = batch.num_rows();
    let mut cols: Vec<ArrayRef> = Vec::with_capacity(target.fields().len());
    for field in target.fields() {
        let col = match batch.schema().index_of(field.name()) {
            Ok(i) => {
                let c = batch.column(i);
                if c.data_type() == field.data_type() {
                    c.clone()
                } else {
                    cast(c, field.data_type()).unwrap_or_else(|_| new_null(field.data_type(), n))
                }
            }
            Err(_) => new_null(field.data_type(), n),
        };
        cols.push(col);
    }
    RecordBatch::try_new(target.clone(), cols)
        .map_err(|e| anyhow!("ép batch về schema catalog thất bại: {e}"))
}

fn new_null(dt: &DataType, n: usize) -> ArrayRef {
    datafusion::arrow::array::new_null_array(dt, n)
}

/// Đăng ký MemTable dưới `<ns>.<ds>`. Tạo schema `ns` trong catalog nếu chưa có
/// (register_table đòi schema tồn tại trước).
fn register_one(
    ctx: &SessionContext,
    ns: &str,
    ds: &str,
    schema: SchemaRef,
    batches: Vec<RecordBatch>,
) -> Result<()> {
    let catalog = ctx
        .catalog("datafusion")
        .ok_or_else(|| anyhow!("không có catalog mặc định 'datafusion'"))?;
    if catalog.schema(ns).is_none() {
        catalog.register_schema(ns, Arc::new(MemorySchemaProvider::new()))?;
    }
    let mem = MemTable::try_new(schema, vec![batches])
        .map_err(|e| anyhow!("dựng MemTable '{ns}.{ds}' thất bại: {e}"))?;
    ctx.register_table(
        TableReference::partial(ns.to_string(), ds.to_string()),
        Arc::new(mem),
    )
    .map_err(|e| anyhow!("đăng ký bảng '{ns}.{ds}' thất bại: {e}"))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// query + explain
// ---------------------------------------------------------------------------

/// SELECT-only: chặn DDL/DML/statement tận gốc plan (§7).
fn select_only() -> SQLOptions {
    SQLOptions::new()
        .with_allow_ddl(false)
        .with_allow_dml(false)
        .with_allow_statements(false)
}

/// Chặn multi-statement TRƯỚC khi plan (parse-first, §7) + báo lỗi thân thiện.
fn ensure_single_statement(sql: &str) -> Result<()> {
    use datafusion::sql::parser::DFParser;
    let stmts = DFParser::parse_sql(sql).map_err(|e| anyhow!("SQL không hợp lệ: {e}"))?;
    match stmts.len() {
        1 => Ok(()),
        0 => Err(anyhow!("không có câu lệnh SQL")),
        n => Err(anyhow!("chỉ cho phép một câu lệnh SQL, nhận {n}")),
    }
}

pub async fn query_page(
    db: &Db,
    sql: &str,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<QueryPage> {
    query_page_at(&config::lake_dir(), db, sql, limit, offset).await
}

pub(crate) async fn query_page_at(
    root: &Path,
    db: &Db,
    sql: &str,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<QueryPage> {
    let limit = limit.unwrap_or(100).clamp(1, 1000) as usize;
    let offset = offset.unwrap_or(0).max(0) as usize;
    ensure_single_statement(sql)?;

    let ctx = session(db);
    register_datasets_at(&ctx, db, root)?;
    let opts = select_only();
    let secs = db.setting_i64("query_max_seconds", 600).max(1) as u64;
    let sql_owned = sql.to_string();

    // Lấy limit+1 để biết còn trang sau. sql_with_options verify_plan trên SQL gốc
    // (bắt DML lồng); limit áp qua DataFrame, không nối chuỗi SQL.
    let fut = async {
        let df = ctx.sql_with_options(&sql_owned, opts).await?;
        let df = df.limit(offset, Some(limit + 1))?;
        let schema: SchemaRef = Arc::new(df.schema().as_arrow().clone());
        let batches = df.collect().await?;
        anyhow::Ok((schema, batches))
    };
    let (schema, batches) = tokio::time::timeout(Duration::from_secs(secs), fut)
        .await
        .map_err(|_| anyhow!("query vượt {secs}s"))??;

    let columns: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    let total: usize = batches.iter().map(|b| b.num_rows()).sum();
    let has_more = total > limit;
    let returned = total.min(limit);

    let mut rows: Vec<Vec<Value>> = Vec::with_capacity(returned);
    let mut emitted = 0usize;
    'outer: for b in &batches {
        for r in 0..b.num_rows() {
            if emitted >= returned {
                break 'outer;
            }
            let mut row = Vec::with_capacity(b.num_columns());
            for c in 0..b.num_columns() {
                row.push(cell_json(b.column(c), r));
            }
            rows.push(row);
            emitted += 1;
        }
    }

    let total_estimate = if has_more {
        None
    } else {
        Some((offset + returned) as i64)
    };

    Ok(QueryPage {
        columns,
        rows,
        returned,
        has_more,
        total_estimate,
    })
}

/// Chạy một câu SELECT và thu **TOÀN BỘ** kết quả (không clamp limit) — đường
/// export (§8 /query/export). SELECT-only (SQLOptions chặn DDL/DML). Timeout theo
/// `query_max_seconds`. Trả cả schema kết quả (để ghi file) lẫn batches đầy đủ.
/// CẢNH BÁO: gọi trên dataset lớn tốn RAM — chỉ dùng cho export ra đĩa, không trả
/// thẳng client (khác query_page_at có phân trang).
pub(crate) async fn collect_all_at(
    root: &Path,
    db: &Db,
    sql: &str,
) -> Result<(SchemaRef, Vec<RecordBatch>)> {
    ensure_single_statement(sql)?;
    let ctx = session(db);
    register_datasets_at(&ctx, db, root)?;
    let secs = db.setting_i64("query_max_seconds", 600).max(1) as u64;
    let sql_owned = sql.to_string();
    let opts = select_only();
    let fut = async {
        let df = ctx.sql_with_options(&sql_owned, opts).await?;
        let schema: SchemaRef = Arc::new(df.schema().as_arrow().clone());
        let batches = df.collect().await?;
        anyhow::Ok((schema, batches))
    };
    tokio::time::timeout(Duration::from_secs(secs), fut)
        .await
        .map_err(|_| anyhow!("query vượt {secs}s"))?
}

pub async fn explain(db: &Db, sql: &str) -> Result<String> {
    explain_at(&config::lake_dir(), db, sql).await
}

pub(crate) async fn explain_at(root: &Path, db: &Db, sql: &str) -> Result<String> {
    ensure_single_statement(sql)?;
    let ctx = session(db);
    register_datasets_at(&ctx, db, root)?;
    // EXPLAIN gói SELECT là LogicalPlan::Explain (không phải Statement) — SQLOptions
    // vẫn chặn DML lồng (EXPLAIN ANALYZE INSERT) qua verify_plan.
    let df = ctx
        .sql_with_options(&format!("EXPLAIN {sql}"), select_only())
        .await?;
    let batches = df.collect().await?;
    // EXPLAIN trả 2 cột (plan_type, plan) — nối cột 'plan' (cuối) thành text.
    let mut lines = Vec::new();
    for b in &batches {
        if b.num_columns() == 0 {
            continue;
        }
        let plan_idx = b.num_columns() - 1;
        let plan = cast(b.column(plan_idx), &DataType::Utf8)
            .map_err(|e| anyhow!("format EXPLAIN thất bại: {e}"))?;
        let plan = plan
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| anyhow!("cột plan không phải chuỗi"))?;
        for i in 0..plan.len() {
            if !plan.is_null(i) {
                lines.push(plan.value(i).to_string());
            }
        }
    }
    Ok(lines.join("\n"))
}

// ---------------------------------------------------------------------------
// cell → JSON
// ---------------------------------------------------------------------------

/// Cắt chuỗi tối đa `CELL_MAX_CHARS` ký tự TRÊN char boundary (đếm char, không byte).
fn truncate_cell(s: &str) -> Value {
    if s.chars().count() <= CELL_MAX_CHARS {
        Value::String(s.to_string())
    } else {
        Value::String(s.chars().take(CELL_MAX_CHARS).collect())
    }
}

/// Ô Arrow → serde_json::Value. Số/bool giữ kiểu JSON; chuỗi cắt 500 ký tự; kiểu
/// khác cast-về-chuỗi rồi cắt (an toàn cho mọi kiểu).
fn cell_json(arr: &ArrayRef, i: usize) -> Value {
    if arr.is_null(i) {
        return Value::Null;
    }
    macro_rules! num {
        ($ty:ty) => {
            Value::from(arr.as_any().downcast_ref::<$ty>().unwrap().value(i))
        };
    }
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
        DataType::Utf8 => {
            truncate_cell(arr.as_any().downcast_ref::<StringArray>().unwrap().value(i))
        }
        DataType::LargeUtf8 => truncate_cell(
            arr.as_any()
                .downcast_ref::<LargeStringArray>()
                .unwrap()
                .value(i),
        ),
        // Date/Timestamp/Binary/… → cast về chuỗi hiển thị rồi cắt.
        _ => match cast(arr, &DataType::Utf8) {
            Ok(s) => match s.as_any().downcast_ref::<StringArray>() {
                Some(s) if !s.is_null(i) => truncate_cell(s.value(i)),
                _ => Value::Null,
            },
            Err(_) => Value::Null,
        },
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

    fn schema3() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("name", DataType::Utf8, true),
            Field::new("note", DataType::Utf8, true),
        ]))
    }

    fn batch3(
        ids: Vec<Option<i64>>,
        names: Vec<Option<&str>>,
        notes: Vec<Option<&str>>,
    ) -> RecordBatch {
        let names: Vec<Option<String>> = names.into_iter().map(|x| x.map(String::from)).collect();
        let notes: Vec<Option<String>> = notes.into_iter().map(|x| x.map(String::from)).collect();
        RecordBatch::try_new(
            schema3(),
            vec![
                Arc::new(Int64Array::from(ids)),
                Arc::new(StringArray::from(names)),
                Arc::new(StringArray::from(notes)),
            ],
        )
        .unwrap()
    }

    /// Import trực tiếp một bảng (schema + batches) vào lake dưới `root`.
    fn import(
        root: &Path,
        db: &Db,
        ns: &str,
        name: &str,
        schema: SchemaRef,
        batches: Vec<RecordBatch>,
        run_id: &str,
    ) {
        let rows = batches.iter().map(|b| b.num_rows()).sum();
        let t = IngestedTable {
            name: name.into(),
            schema,
            batches,
            origin: "csv",
            note: "t".into(),
            rows,
        };
        lake::create_dataset_from_ingested_at(root, db, ns, name, &t, run_id).unwrap();
    }

    #[tokio::test]
    async fn landed_then_committed_is_queryable() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        import(
            dir.path(),
            &db,
            "raw",
            "orders",
            schema3(),
            vec![batch3(
                vec![Some(1), Some(2)],
                vec![Some("a"), Some("b")],
                vec![None, Some("x")],
            )],
            "run-1",
        );
        let page = query_page_at(
            dir.path(),
            &db,
            "SELECT id, name FROM raw.orders ORDER BY id",
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(page.columns, vec!["id", "name"]);
        assert_eq!(page.returned, 2);
        assert!(!page.has_more);
        assert_eq!(page.total_estimate, Some(2));
        assert_eq!(page.rows[0][0], serde_json::json!(1));
        assert_eq!(page.rows[0][1], serde_json::json!("a"));
    }

    #[tokio::test]
    async fn landed_but_not_committed_is_invisible() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        // Tạo dataset + schema_version nhưng KHÔNG commit file vào manifest.
        let ds = db
            .dataset_upsert("raw", "orders", None, None, None)
            .unwrap();
        db.schema_version_add(ds, &lake::schema_to_json(&schema3()), Some("init"))
            .unwrap();
        // Land file xuống đĩa (vô hình) — không manifest_add_files.
        lake::land_batches_at(
            dir.path(),
            "raw",
            "orders",
            "run-x",
            &[batch3(vec![Some(9)], vec![Some("z")], vec![None])],
            None,
        )
        .unwrap();

        let page = query_page_at(dir.path(), &db, "SELECT * FROM raw.orders", None, None)
            .await
            .unwrap();
        // File có trên đĩa nhưng chưa vào manifest → 0 dòng.
        assert_eq!(page.returned, 0);
        assert!(!page.has_more);
    }

    #[tokio::test]
    async fn mixed_schema_missing_column_reads_null() {
        // §6.4: catalog schema có cột 'note'; file cũ thiếu cột đó → đọc NULL.
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        let ds = db
            .dataset_upsert("raw", "orders", None, None, None)
            .unwrap();
        db.schema_version_add(ds, &lake::schema_to_json(&schema3()), Some("init"))
            .unwrap();
        // File cũ chỉ có (id, name) — thiếu 'note'.
        let old_schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("name", DataType::Utf8, true),
        ]));
        let old = RecordBatch::try_new(
            old_schema,
            vec![
                Arc::new(Int64Array::from(vec![Some(1)])),
                Arc::new(StringArray::from(vec![Some("a".to_string())])),
            ],
        )
        .unwrap();
        let files =
            lake::land_batches_at(dir.path(), "raw", "orders", "run-old", &[old], None).unwrap();
        db.manifest_add_files(ds, "run-old", &files).unwrap();

        let page = query_page_at(
            dir.path(),
            &db,
            "SELECT id, note FROM raw.orders",
            None,
            None,
        )
        .await
        .unwrap();
        assert_eq!(page.returned, 1);
        assert_eq!(page.rows[0][0], serde_json::json!(1));
        assert_eq!(page.rows[0][1], Value::Null, "cột thiếu ở file → NULL");
    }

    #[tokio::test]
    async fn sql_options_block_writes() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        import(
            dir.path(),
            &db,
            "raw",
            "orders",
            schema3(),
            vec![batch3(vec![Some(1)], vec![Some("a")], vec![None])],
            "run-1",
        );
        // INSERT — DML chặn.
        assert!(query_page_at(
            dir.path(),
            &db,
            "INSERT INTO raw.orders VALUES (2,'b','c')",
            None,
            None
        )
        .await
        .is_err());
        // CREATE TABLE — DDL chặn.
        assert!(
            query_page_at(dir.path(), &db, "CREATE TABLE x AS SELECT 1", None, None)
                .await
                .is_err()
        );
        // EXPLAIN ANALYZE INSERT — variant-filter tay bị lách; verify_plan bắt DML lồng.
        assert!(query_page_at(
            dir.path(),
            &db,
            "EXPLAIN ANALYZE INSERT INTO raw.orders VALUES (2,'b','c')",
            None,
            None
        )
        .await
        .is_err());
        // Multi-statement — parse-first chặn.
        assert!(
            query_page_at(dir.path(), &db, "SELECT 1; SELECT 2", None, None)
                .await
                .is_err()
        );
        // SELECT hợp lệ vẫn chạy.
        assert!(
            query_page_at(dir.path(), &db, "SELECT * FROM raw.orders", None, None)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn page_clamps_and_reports_has_more_and_truncates_cell() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        // 5 dòng; cột note dòng đầu là chuỗi tiếng Việt dài >500 ký tự.
        let long = "Xin chào ".repeat(120); // ~1080 ký tự, có dấu tiếng Việt
        assert!(long.chars().count() > CELL_MAX_CHARS);
        let notes: Vec<Option<String>> = vec![Some(long.clone()), None, None, None, None];
        let batch = RecordBatch::try_new(
            schema3(),
            vec![
                Arc::new(Int64Array::from(vec![
                    Some(1),
                    Some(2),
                    Some(3),
                    Some(4),
                    Some(5),
                ])),
                Arc::new(StringArray::from(vec![
                    Some("a".to_string()),
                    Some("b".to_string()),
                    Some("c".to_string()),
                    Some("d".to_string()),
                    Some("e".to_string()),
                ])),
                Arc::new(StringArray::from(notes)),
            ],
        )
        .unwrap();
        import(
            dir.path(),
            &db,
            "raw",
            "orders",
            schema3(),
            vec![batch],
            "run-1",
        );

        // limit 2 → returned 2, has_more true, total_estimate None.
        let page = query_page_at(
            dir.path(),
            &db,
            "SELECT id, note FROM raw.orders ORDER BY id",
            Some(2),
            Some(0),
        )
        .await
        .unwrap();
        assert_eq!(page.returned, 2);
        assert!(page.has_more);
        assert_eq!(page.total_estimate, None);
        // Cell note cắt đúng 500 ký tự, trên char boundary (String hợp lệ, không panic).
        let cell = page.rows[0][1].as_str().unwrap();
        assert_eq!(cell.chars().count(), CELL_MAX_CHARS);
        assert!(cell.starts_with("Xin chào"));

        // limit vượt trần 1000 → clamp; limit 0 → clamp lên 1.
        let all = query_page_at(
            dir.path(),
            &db,
            "SELECT id FROM raw.orders",
            Some(9999),
            None,
        )
        .await
        .unwrap();
        assert_eq!(all.returned, 5);
        assert!(!all.has_more);
        let one = query_page_at(dir.path(), &db, "SELECT id FROM raw.orders", Some(0), None)
            .await
            .unwrap();
        assert_eq!(one.returned, 1);
        assert!(one.has_more);
    }

    #[tokio::test]
    async fn explain_returns_plan_text() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_memory().unwrap();
        import(
            dir.path(),
            &db,
            "raw",
            "orders",
            schema3(),
            vec![batch3(vec![Some(1)], vec![Some("a")], vec![None])],
            "run-1",
        );
        let plan = explain_at(dir.path(), &db, "SELECT id FROM raw.orders WHERE id > 0")
            .await
            .unwrap();
        assert!(!plan.is_empty());
        // EXPLAIN ANALYZE INSERT vẫn bị chặn ở explain.
        assert!(explain_at(
            dir.path(),
            &db,
            "EXPLAIN ANALYZE INSERT INTO raw.orders VALUES (1,'a','b')"
        )
        .await
        .is_err());
    }

    #[test]
    fn truncate_cell_is_char_boundary_safe() {
        let s = "Xin chào".repeat(100);
        if let Value::String(out) = truncate_cell(&s) {
            assert_eq!(out.chars().count(), CELL_MAX_CHARS);
            assert!(out.starts_with("Xin chào"));
        } else {
            panic!("kỳ vọng String");
        }
    }
}
