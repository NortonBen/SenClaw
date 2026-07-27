//! SQLite connector qua `rusqlite` sẵn có (§5.2 tier 1).
//!
//! Blocking driver → mọi truy cập bọc trong `spawn_blocking`. Extract đọc toàn bộ
//! kết quả trong một lượt (dataset SQLite thường nhỏ), quyết định kiểu Arrow theo cột
//! rồi cắt thành RecordBatch mỗi `batch_rows`, trả về dưới dạng stream.

use anyhow::{Context, Result};
use async_trait::async_trait;
use datafusion::arrow::array::{
    ArrayRef, BinaryBuilder, Float64Builder, Int64Builder, StringBuilder,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use rusqlite::types::{Value as SqlValue, ValueRef};
use rusqlite::Connection;
use std::sync::Arc;

use super::{
    build_create_table, build_insert_sql, build_select, chunk_rows, column_cells, Cell, ColumnInfo,
    Connector, Dialect, ExtractSpec, LoadFlavor, LoadMode, LoadSpec, TableInfo,
};
#[cfg(test)]
use super::SourceRel;

/// Connector đọc file SQLite. `dsn` chấp nhận `sqlite:///path`, `sqlite://path`,
/// `file:path` hoặc đường dẫn trần.
pub struct SqliteConnector {
    path: String,
}

impl SqliteConnector {
    pub fn new(dsn: String) -> Self {
        Self {
            path: dsn_to_path(&dsn),
        }
    }
}

/// Rút đường dẫn file từ DSN SQLite.
fn dsn_to_path(dsn: &str) -> String {
    if let Some(rest) = dsn.strip_prefix("sqlite://") {
        // sqlite:///abs → "/abs"; sqlite://rel → "rel"
        rest.to_string()
    } else if let Some(rest) = dsn.strip_prefix("file:") {
        rest.to_string()
    } else {
        dsn.to_string()
    }
}

#[async_trait]
impl Connector for SqliteConnector {
    async fn test(&self) -> Result<()> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let conn = Connection::open(&path).with_context(|| format!("mở SQLite {path}"))?;
            conn.query_row("SELECT 1", [], |_| Ok(()))
                .context("SELECT 1 thất bại")?;
            Ok(())
        })
        .await
        .context("spawn_blocking test")?
    }

    async fn introspect(&self) -> Result<Vec<TableInfo>> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || -> Result<Vec<TableInfo>> {
            let conn = Connection::open(&path).with_context(|| format!("mở SQLite {path}"))?;
            introspect_blocking(&conn)
        })
        .await
        .context("spawn_blocking introspect")?
    }

    async fn extract(&self, spec: ExtractSpec) -> Result<BoxStream<'static, Result<RecordBatch>>> {
        let path = self.path.clone();
        let batches = tokio::task::spawn_blocking(move || -> Result<Vec<RecordBatch>> {
            let conn = Connection::open(&path).with_context(|| format!("mở SQLite {path}"))?;
            extract_blocking(&conn, &spec)
        })
        .await
        .context("spawn_blocking extract")??;

        // Trả về stream tĩnh trên các batch đã dựng.
        let s = futures_util::stream::iter(batches.into_iter().map(Ok));
        Ok(s.boxed())
    }

    async fn load(&self, spec: LoadSpec, batches: Vec<RecordBatch>) -> Result<u64> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || -> Result<u64> {
            let mut conn = Connection::open(&path).with_context(|| format!("mở SQLite {path}"))?;
            load_blocking(&mut conn, &spec, &batches)
        })
        .await
        .context("spawn_blocking load")?
    }
}

// ---------------------------------------------------------------------------
// load (blocking) — create_if_missing + FullRefresh(DELETE)/Append/Upsert trong 1 txn
// ---------------------------------------------------------------------------

/// Trần params/statement của SQLite — mặc định compile-time là 999 (bản cũ). Chọn thấp
/// để chạy đúng trên mọi bản.
const SQLITE_MAX_PARAMS: usize = 900;

fn load_blocking(conn: &mut Connection, spec: &LoadSpec, batches: &[RecordBatch]) -> Result<u64> {
    // Không có batch (kể cả batch rỗng) → không suy được schema. FullRefresh vẫn nên
    // xoá bảng đích; nhưng thiếu schema thì không tạo được — chỉ DELETE nếu bảng có.
    let schema = match batches.first() {
        Some(b) => b.schema(),
        None => return Ok(0),
    };
    let flavor = LoadFlavor::Sqlite;

    if spec.create_if_missing {
        let ddl = build_create_table(flavor, &spec.table, &schema);
        conn.execute_batch(&ddl).with_context(|| format!("CREATE TABLE: {ddl}"))?;
    }

    let cols: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
    let upsert_keys: Option<Vec<String>> = match &spec.mode {
        LoadMode::Upsert { keys } => Some(keys.clone()),
        _ => None,
    };
    let per_chunk = chunk_rows(cols.len(), SQLITE_MAX_PARAMS);

    let tx = conn.transaction().context("mở transaction load")?;

    // FullRefresh: xoá sạch bảng trong cùng txn (KHÔNG DROP — giữ schema/quyền §step).
    if spec.mode == LoadMode::FullRefresh {
        let table = super::quote_qualified(&spec.table, Dialect::SQLITE.quote);
        tx.execute_batch(&format!("DELETE FROM {table}"))
            .with_context(|| format!("DELETE FROM {table}"))?;
    }

    // Gom toàn bộ dòng thành ma trận Cell rồi chèn theo chunk.
    let mut written: u64 = 0;
    for batch in batches {
        let col_cells: Vec<Vec<Cell>> = batch.columns().iter().map(column_cells).collect();
        let nrows = batch.num_rows();
        let mut start = 0;
        while start < nrows {
            let end = (start + per_chunk).min(nrows);
            let n = end - start;
            let sql = build_insert_sql(flavor, &spec.table, &cols, n, upsert_keys.as_deref());
            // Params row-major: cho mỗi dòng, mỗi cột.
            let mut values: Vec<rusqlite::types::Value> = Vec::with_capacity(n * cols.len());
            for r in start..end {
                for c in &col_cells {
                    values.push(cell_to_sqlvalue(&c[r]));
                }
            }
            tx.execute(&sql, rusqlite::params_from_iter(values.iter()))
                .with_context(|| format!("INSERT chunk ({n} dòng)"))?;
            written += n as u64;
            start = end;
        }
    }

    tx.commit().context("commit load")?;
    Ok(written)
}

/// Cell → rusqlite Value cho bind (theo đúng biến thể).
fn cell_to_sqlvalue(c: &Cell) -> rusqlite::types::Value {
    use rusqlite::types::Value as V;
    match c {
        Cell::Null => V::Null,
        Cell::Int(i) => V::Integer(*i),
        Cell::Float(f) => V::Real(*f),
        Cell::Bool(b) => V::Integer(*b as i64),
        Cell::Text(s) => V::Text(s.clone()),
        Cell::Bytes(b) => V::Blob(b.clone()),
    }
}

// ---------------------------------------------------------------------------
// introspect (blocking)
// ---------------------------------------------------------------------------

fn introspect_blocking(conn: &Connection) -> Result<Vec<TableInfo>> {
    // Bảng người dùng (bỏ sqlite_* nội bộ).
    let mut names: Vec<String> = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master \
             WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for r in rows {
            names.push(r?);
        }
    }

    let mut out = Vec::with_capacity(names.len());
    for name in names {
        // PRAGMA table_info(name) → cid, name, type, notnull, dflt_value, pk
        let mut cols = Vec::new();
        {
            let sql = format!("PRAGMA table_info({})", quote_pragma(&name));
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map([], |r| {
                let cname: String = r.get(1)?;
                let ctype: String = r.get(2)?;
                let notnull: i64 = r.get(3)?;
                Ok((cname, ctype, notnull))
            })?;
            for r in rows {
                let (cname, ctype, notnull) = r?;
                cols.push(ColumnInfo {
                    name: cname,
                    data_type: if ctype.is_empty() { "".into() } else { ctype },
                    nullable: notnull == 0,
                });
            }
        }

        // Ước lượng dòng (COUNT rẻ với SQLite nhỏ).
        let row_estimate: Option<i64> = conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {}", quote_pragma(&name)),
                [],
                |r| r.get::<_, i64>(0),
            )
            .ok();

        out.push(TableInfo {
            schema: None,
            name,
            columns: cols,
            row_estimate,
        });
    }
    Ok(out)
}

/// Quote tên bảng cho PRAGMA/COUNT — SQLite dùng `"`.
fn quote_pragma(name: &str) -> String {
    let mut s = String::with_capacity(name.len() + 2);
    s.push('"');
    for c in name.chars() {
        if c == '"' {
            s.push('"');
        }
        s.push(c);
    }
    s.push('"');
    s
}

// ---------------------------------------------------------------------------
// extract (blocking) → RecordBatches
// ---------------------------------------------------------------------------

fn extract_blocking(conn: &Connection, spec: &ExtractSpec) -> Result<Vec<RecordBatch>> {
    let (sql, params) = build_select(spec, Dialect::SQLITE);

    // Bind params (từ JSON → rusqlite Value).
    let bound: Vec<SqlValue> = params.iter().map(json_to_sqlvalue).collect();
    let param_refs: Vec<&dyn rusqlite::ToSql> =
        bound.iter().map(|v| v as &dyn rusqlite::ToSql).collect();

    let mut stmt = conn.prepare(&sql).with_context(|| format!("prepare: {sql}"))?;
    let ncol = stmt.column_count();
    let col_names: Vec<String> = (0..ncol)
        .map(|i| stmt.column_name(i).unwrap_or("").to_string())
        .collect();

    // Đọc toàn bộ dòng thành ma trận Value (SQLite nhỏ — chấp nhận in-memory).
    let mut all_rows: Vec<Vec<SqlValue>> = Vec::new();
    {
        let mut rows = stmt.query(param_refs.as_slice())?;
        while let Some(row) = rows.next()? {
            let mut cells = Vec::with_capacity(ncol);
            for i in 0..ncol {
                cells.push(valueref_to_owned(row.get_ref(i)?));
            }
            all_rows.push(cells);
        }
    }

    // Quyết định kiểu Arrow từng cột theo giá trị quan sát được.
    let col_types: Vec<DataType> = (0..ncol)
        .map(|i| infer_col_type(&all_rows, i))
        .collect();

    let fields: Vec<Field> = col_names
        .iter()
        .zip(&col_types)
        .map(|(n, t)| Field::new(n, t.clone(), true))
        .collect();
    let schema: SchemaRef = Arc::new(Schema::new(fields));

    // Cắt thành batch.
    let batch_rows = spec.batch_rows.max(1);
    let mut batches = Vec::new();

    if all_rows.is_empty() {
        // Batch rỗng vẫn trả một RecordBatch 0 dòng để giữ schema.
        let cols = build_empty_columns(&col_types);
        batches.push(
            RecordBatch::try_new(schema.clone(), cols).context("RecordBatch rỗng")?,
        );
        return Ok(batches);
    }

    let mut start = 0;
    while start < all_rows.len() {
        let end = (start + batch_rows).min(all_rows.len());
        let slice = &all_rows[start..end];
        let cols = build_columns(&col_types, slice);
        batches.push(
            RecordBatch::try_new(schema.clone(), cols)
                .context("dựng RecordBatch từ SQLite")?,
        );
        start = end;
    }

    Ok(batches)
}

/// Suy kiểu Arrow cho cột `idx` từ các giá trị quan sát (ưu tiên: Binary>Utf8>Float64>Int64).
fn infer_col_type(rows: &[Vec<SqlValue>], idx: usize) -> DataType {
    let mut has_blob = false;
    let mut has_text = false;
    let mut has_real = false;
    let mut has_int = false;
    for r in rows {
        match &r[idx] {
            SqlValue::Blob(_) => has_blob = true,
            SqlValue::Text(_) => has_text = true,
            SqlValue::Real(_) => has_real = true,
            SqlValue::Integer(_) => has_int = true,
            SqlValue::Null => {}
        }
    }
    if has_blob {
        DataType::Binary
    } else if has_text {
        DataType::Utf8
    } else if has_real {
        DataType::Float64
    } else if has_int {
        DataType::Int64
    } else {
        // Toàn NULL → Utf8 nullable.
        DataType::Utf8
    }
}

fn build_empty_columns(types: &[DataType]) -> Vec<ArrayRef> {
    types
        .iter()
        .map(|t| build_columns_one(t, std::iter::empty()))
        .collect()
}

fn build_columns(types: &[DataType], rows: &[Vec<SqlValue>]) -> Vec<ArrayRef> {
    types
        .iter()
        .enumerate()
        .map(|(i, t)| build_columns_one(t, rows.iter().map(move |r| &r[i])))
        .collect()
}

/// Dựng một cột Arrow từ iterator giá trị.
fn build_columns_one<'a, I>(dt: &DataType, vals: I) -> ArrayRef
where
    I: Iterator<Item = &'a SqlValue>,
{
    match dt {
        DataType::Int64 => {
            let mut b = Int64Builder::new();
            for v in vals {
                match v {
                    SqlValue::Integer(i) => b.append_value(*i),
                    SqlValue::Real(f) => b.append_value(*f as i64),
                    SqlValue::Null => b.append_null(),
                    _ => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
        DataType::Float64 => {
            let mut b = Float64Builder::new();
            for v in vals {
                match v {
                    SqlValue::Real(f) => b.append_value(*f),
                    SqlValue::Integer(i) => b.append_value(*i as f64),
                    SqlValue::Null => b.append_null(),
                    _ => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
        DataType::Binary => {
            let mut b = BinaryBuilder::new();
            for v in vals {
                match v {
                    SqlValue::Blob(bytes) => b.append_value(bytes),
                    SqlValue::Text(s) => b.append_value(s.as_bytes()),
                    SqlValue::Null => b.append_null(),
                    _ => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
        // Utf8 và mọi trường hợp khác: ép về chuỗi.
        _ => {
            let mut b = StringBuilder::new();
            for v in vals {
                match v {
                    SqlValue::Text(s) => b.append_value(s),
                    SqlValue::Integer(i) => b.append_value(i.to_string()),
                    SqlValue::Real(f) => b.append_value(f.to_string()),
                    SqlValue::Blob(bytes) => {
                        b.append_value(String::from_utf8_lossy(bytes).as_ref())
                    }
                    SqlValue::Null => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
    }
}

fn valueref_to_owned(v: ValueRef<'_>) -> SqlValue {
    match v {
        ValueRef::Null => SqlValue::Null,
        ValueRef::Integer(i) => SqlValue::Integer(i),
        ValueRef::Real(f) => SqlValue::Real(f),
        ValueRef::Text(t) => SqlValue::Text(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => SqlValue::Blob(b.to_vec()),
    }
}

/// JSON param → rusqlite Value cho bind.
fn json_to_sqlvalue(v: &serde_json::Value) -> SqlValue {
    match v {
        serde_json::Value::Null => SqlValue::Null,
        serde_json::Value::Bool(b) => SqlValue::Integer(if *b { 1 } else { 0 }),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                SqlValue::Integer(i)
            } else if let Some(f) = n.as_f64() {
                SqlValue::Real(f)
            } else {
                SqlValue::Text(n.to_string())
            }
        }
        serde_json::Value::String(s) => SqlValue::Text(s.clone()),
        // Mảng/object không phải giá trị scalar hợp lệ cho cursor — serialize làm text.
        other => SqlValue::Text(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::array::{Array, Float64Array, Int64Array, StringArray};

    fn seed(path: &str) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE items (id INTEGER, price REAL, label TEXT);
             INSERT INTO items VALUES (1, 9.5, 'a');
             INSERT INTO items VALUES (2, 3.0, 'b');
             INSERT INTO items VALUES (3, 7.25, 'c');",
        )
        .unwrap();
    }

    #[tokio::test]
    async fn introspect_lists_table_and_columns() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.sqlite");
        let ps = path.to_str().unwrap().to_string();
        seed(&ps);

        let c = SqliteConnector::new(ps);
        let tables = c.introspect().await.unwrap();
        assert_eq!(tables.len(), 1);
        let t = &tables[0];
        assert_eq!(t.name, "items");
        assert_eq!(t.row_estimate, Some(3));
        let names: Vec<_> = t.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["id", "price", "label"]);
    }

    #[tokio::test]
    async fn extract_full_table_types_and_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.sqlite");
        let ps = path.to_str().unwrap().to_string();
        seed(&ps);

        let c = SqliteConnector::new(ps);
        let spec = ExtractSpec {
            source: SourceRel::Table {
                schema: None,
                name: "items".into(),
            },
            columns: None,
            cursor: None,
            batch_rows: 8192,
        };
        let mut stream = c.extract(spec).await.unwrap();
        let mut total = 0usize;
        let mut checked = false;
        while let Some(b) = stream.next().await {
            let b = b.unwrap();
            total += b.num_rows();
            if b.num_rows() > 0 && !checked {
                checked = true;
                // id → Int64
                let id = b
                    .column(0)
                    .as_any()
                    .downcast_ref::<Int64Array>()
                    .expect("id là Int64");
                assert_eq!(id.value(0), 1);
                // price → Float64
                let price = b
                    .column(1)
                    .as_any()
                    .downcast_ref::<Float64Array>()
                    .expect("price là Float64");
                assert!((price.value(0) - 9.5).abs() < 1e-9);
                // label → Utf8
                let label = b
                    .column(2)
                    .as_any()
                    .downcast_ref::<StringArray>()
                    .expect("label là Utf8");
                assert_eq!(label.value(0), "a");
            }
        }
        assert_eq!(total, 3);
        assert!(checked);
    }

    #[tokio::test]
    async fn extract_cursor_ge_filters() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.sqlite");
        let ps = path.to_str().unwrap().to_string();
        seed(&ps);

        let c = SqliteConnector::new(ps);
        let spec = ExtractSpec {
            source: SourceRel::Table {
                schema: None,
                name: "items".into(),
            },
            columns: Some(vec!["id".into()]),
            cursor: Some(super::super::CursorPred {
                column: "id".into(),
                op: super::super::CursorOp::Ge,
                from: serde_json::json!(2),
                to: None,
            }),
            batch_rows: 8192,
        };
        let mut stream = c.extract(spec).await.unwrap();
        let mut ids = Vec::new();
        while let Some(b) = stream.next().await {
            let b = b.unwrap();
            let col = b.column(0).as_any().downcast_ref::<Int64Array>().unwrap();
            for i in 0..col.len() {
                ids.push(col.value(i));
            }
        }
        assert_eq!(ids, vec![2, 3]);
    }

    #[tokio::test]
    async fn extract_small_batches_split() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.sqlite");
        let ps = path.to_str().unwrap().to_string();
        seed(&ps);

        let c = SqliteConnector::new(ps);
        let spec = ExtractSpec {
            source: SourceRel::Table {
                schema: None,
                name: "items".into(),
            },
            columns: None,
            cursor: None,
            batch_rows: 2,
        };
        let mut stream = c.extract(spec).await.unwrap();
        let mut nbatch = 0;
        let mut total = 0;
        while let Some(b) = stream.next().await {
            let b = b.unwrap();
            nbatch += 1;
            total += b.num_rows();
        }
        assert_eq!(total, 3);
        assert_eq!(nbatch, 2); // 2 + 1
    }

    #[test]
    fn dsn_path_variants() {
        assert_eq!(dsn_to_path("sqlite:///tmp/x.sqlite"), "/tmp/x.sqlite");
        assert_eq!(dsn_to_path("sqlite://rel.sqlite"), "rel.sqlite");
        assert_eq!(dsn_to_path("file:foo.db"), "foo.db");
        assert_eq!(dsn_to_path("/abs/plain.db"), "/abs/plain.db");
    }

    // ---- load (§5 DB-load) END-TO-END với SQLite thật ----

    use datafusion::arrow::datatypes::{Field, Schema, SchemaRef};
    use datafusion::arrow::record_batch::RecordBatch as RB;

    fn batch(ids: &[i64], labels: &[&str]) -> RB {
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("label", DataType::Utf8, true),
        ]));
        RB::try_new(
            schema,
            vec![
                Arc::new(Int64Array::from(ids.to_vec())),
                Arc::new(StringArray::from(labels.to_vec())),
            ],
        )
        .unwrap()
    }

    /// Đọc lại (id,label) từ file sqlite đích, sắp theo id.
    fn read_back(path: &str) -> Vec<(i64, String)> {
        let conn = Connection::open(path).unwrap();
        let mut stmt = conn.prepare("SELECT id, label FROM dest ORDER BY id").unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    #[tokio::test]
    async fn load_create_if_missing_and_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dst.sqlite");
        let ps = path.to_str().unwrap().to_string();
        let c = SqliteConnector::new(ps.clone());

        // Bảng chưa có → create_if_missing tự tạo, Append 2 dòng.
        let n = c
            .load(
                LoadSpec { table: "dest".into(), mode: LoadMode::Append, create_if_missing: true },
                vec![batch(&[1, 2], &["a", "b"])],
            )
            .await
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(read_back(&ps), vec![(1, "a".into()), (2, "b".into())]);

        // Append thêm → cộng dồn.
        let n2 = c
            .load(
                LoadSpec { table: "dest".into(), mode: LoadMode::Append, create_if_missing: true },
                vec![batch(&[3], &["c"])],
            )
            .await
            .unwrap();
        assert_eq!(n2, 1);
        assert_eq!(
            read_back(&ps),
            vec![(1, "a".into()), (2, "b".into()), (3, "c".into())]
        );

        // Kiểu cột đúng: id INTEGER (affinity), label TEXT.
        let conn = Connection::open(&ps).unwrap();
        let ty: String = conn
            .query_row("SELECT type FROM pragma_table_info('dest') WHERE name='id'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(ty, "BIGINT");
    }

    #[tokio::test]
    async fn load_full_refresh_rerun_no_doubling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dst.sqlite");
        let ps = path.to_str().unwrap().to_string();
        let c = SqliteConnector::new(ps.clone());

        let spec = || LoadSpec {
            table: "dest".into(),
            mode: LoadMode::FullRefresh,
            create_if_missing: true,
        };
        c.load(spec(), vec![batch(&[1, 2, 3], &["a", "b", "c"])]).await.unwrap();
        // Chạy lần 2 (dữ liệu khác) → DELETE + INSERT, KHÔNG nhân đôi.
        let n = c.load(spec(), vec![batch(&[7, 8], &["x", "y"])]).await.unwrap();
        assert_eq!(n, 2);
        assert_eq!(read_back(&ps), vec![(7, "x".into()), (8, "y".into())]);
    }

    #[tokio::test]
    async fn load_upsert_updates_by_key_no_dup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dst.sqlite");
        let ps = path.to_str().unwrap().to_string();

        // ON CONFLICT cần UNIQUE/PK trên khoá → tạo bảng đích với PK trước.
        {
            let conn = Connection::open(&ps).unwrap();
            conn.execute_batch("CREATE TABLE dest (id INTEGER PRIMARY KEY, label TEXT);")
                .unwrap();
        }
        let c = SqliteConnector::new(ps.clone());
        let upsert = || LoadSpec {
            table: "dest".into(),
            mode: LoadMode::Upsert { keys: vec!["id".into()] },
            // Bảng đã tồn tại (có PK) → không tạo lại.
            create_if_missing: false,
        };

        c.load(upsert(), vec![batch(&[1, 2], &["a", "b"])]).await.unwrap();
        // id=2 cập nhật, id=3 chèn mới → không nhân dòng id=2.
        c.load(upsert(), vec![batch(&[2, 3], &["B", "c"])]).await.unwrap();
        assert_eq!(
            read_back(&ps),
            vec![(1, "a".into()), (2, "B".into()), (3, "c".into())]
        );
    }
}
