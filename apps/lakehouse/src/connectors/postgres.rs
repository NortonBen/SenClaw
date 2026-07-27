//! Postgres connector qua `sqlx 0.9` (§5.2 tier 1).
//!
//! Extract-only cho Phase 2 (load = Phase 4). Không có Postgres sống trong CI nên
//! file này CHỈ cần COMPILE; logic build SQL/map kiểu được test ở `mod.rs`.

use anyhow::{Context, Result};
use async_trait::async_trait;
use datafusion::arrow::array::{
    ArrayRef, BooleanBuilder, Float64Builder, Int64Builder, StringBuilder,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{Column, Row, TypeInfo};
use std::sync::Arc;

use super::{
    build_create_table, build_insert_sql, build_select, chunk_rows, column_cells, quote_qualified,
    Cell, ColumnInfo, Connector, Dialect, ExtractSpec, LoadFlavor, LoadMode, LoadSpec, TableInfo,
    PG_MAX_PARAMS,
};

pub struct PostgresConnector {
    dsn: String,
}

impl PostgresConnector {
    pub fn new(dsn: String) -> Self {
        Self { dsn }
    }

    async fn pool(&self) -> Result<sqlx::PgPool> {
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&self.dsn)
            .await
            .context("kết nối Postgres")
    }
}

#[async_trait]
impl Connector for PostgresConnector {
    async fn test(&self) -> Result<()> {
        let pool = self.pool().await?;
        sqlx::query("SELECT 1").execute(&pool).await.context("SELECT 1")?;
        Ok(())
    }

    async fn introspect(&self) -> Result<Vec<TableInfo>> {
        let pool = self.pool().await?;
        // Cột từ information_schema (bỏ schema hệ thống).
        let rows = sqlx::query(
            "SELECT table_schema, table_name, column_name, data_type, is_nullable \
             FROM information_schema.columns \
             WHERE table_schema NOT IN ('pg_catalog','information_schema') \
             ORDER BY table_schema, table_name, ordinal_position",
        )
        .fetch_all(&pool)
        .await
        .context("đọc information_schema.columns")?;

        let mut out: Vec<TableInfo> = Vec::new();
        for r in rows {
            let schema: String = r.try_get("table_schema").unwrap_or_default();
            let name: String = r.try_get("table_name").unwrap_or_default();
            let col: String = r.try_get("column_name").unwrap_or_default();
            let dtype: String = r.try_get("data_type").unwrap_or_default();
            let is_null: String = r.try_get("is_nullable").unwrap_or_else(|_| "YES".into());

            let ci = ColumnInfo {
                name: col,
                data_type: dtype,
                nullable: is_null.eq_ignore_ascii_case("YES"),
            };
            match out
                .last_mut()
                .filter(|t| t.schema.as_deref() == Some(schema.as_str()) && t.name == name)
            {
                Some(t) => t.columns.push(ci),
                None => out.push(TableInfo {
                    schema: Some(schema),
                    name,
                    columns: vec![ci],
                    row_estimate: None,
                }),
            }
        }
        Ok(out)
    }

    async fn extract(&self, spec: ExtractSpec) -> Result<BoxStream<'static, Result<RecordBatch>>> {
        let pool = self.pool().await?;
        let (sql, params) = build_select(&spec, Dialect::POSTGRES);

        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
        for p in &params {
            q = bind_json(q, p);
        }
        let rows = q.fetch_all(&pool).await.context("extract Postgres")?;

        let batches = rows_to_batches(&rows, spec.batch_rows.max(1))?;
        let s = futures_util::stream::iter(batches.into_iter().map(Ok));
        Ok(s.boxed())
    }

    async fn load(&self, spec: LoadSpec, batches: Vec<RecordBatch>) -> Result<u64> {
        let schema = match batches.first() {
            Some(b) => b.schema(),
            None => return Ok(0),
        };
        let flavor = LoadFlavor::Postgres;
        let pool = self.pool().await?;

        if spec.create_if_missing {
            let ddl = build_create_table(flavor, &spec.table, &schema);
            sqlx::query(sqlx::AssertSqlSafe(ddl))
                .execute(&pool)
                .await
                .context("CREATE TABLE Postgres")?;
        }

        let cols: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
        let col_types: Vec<DataType> =
            schema.fields().iter().map(|f| f.data_type().clone()).collect();
        let upsert_keys: Option<Vec<String>> = match &spec.mode {
            LoadMode::Upsert { keys } => Some(keys.clone()),
            _ => None,
        };
        let per_chunk = chunk_rows(cols.len(), PG_MAX_PARAMS);

        let mut tx = pool.begin().await.context("mở transaction Postgres")?;

        // FullRefresh: TRUNCATE trong cùng txn (không swap staging — fallback §5.2).
        if spec.mode == LoadMode::FullRefresh {
            let table = quote_qualified(&spec.table, Dialect::POSTGRES.quote);
            sqlx::query(sqlx::AssertSqlSafe(format!("TRUNCATE TABLE {table}")))
                .execute(&mut *tx)
                .await
                .with_context(|| format!("TRUNCATE {table}"))?;
        }

        let mut written: u64 = 0;
        for batch in &batches {
            let col_cells: Vec<Vec<Cell>> = batch.columns().iter().map(column_cells).collect();
            let nrows = batch.num_rows();
            let mut start = 0;
            while start < nrows {
                let end = (start + per_chunk).min(nrows);
                let n = end - start;
                let sql =
                    build_insert_sql(flavor, &spec.table, &cols, n, upsert_keys.as_deref());
                let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
                for r in start..end {
                    for (ci, cell) in col_cells.iter().enumerate() {
                        q = bind_cell(q, &col_types[ci], &cell[r]);
                    }
                }
                q.execute(&mut *tx).await.context("INSERT chunk Postgres")?;
                written += n as u64;
                start = end;
            }
        }

        tx.commit().await.context("commit load Postgres")?;
        Ok(written)
    }
}

/// Bind một `Cell` vào query Postgres theo KIỂU CỘT (để NULL cũng đúng kiểu — sqlx cần
/// kiểu Rust tường minh khi bind None).
fn bind_cell<'q>(
    q: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    dt: &DataType,
    cell: &Cell,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match dt {
        DataType::Boolean => q.bind(cell.as_bool()),
        DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64 => q.bind(cell.as_int()),
        DataType::Float16 | DataType::Float32 | DataType::Float64 => q.bind(cell.as_float()),
        DataType::Binary | DataType::LargeBinary | DataType::FixedSizeBinary(_) => {
            q.bind(cell.as_bytes())
        }
        // Utf8/Date/Timestamp/Decimal/nested… → chuỗi (column_cells đã cast về text).
        _ => q.bind(cell.as_text()),
    }
}

/// Bind một JSON scalar vào query Postgres theo kiểu thực.
fn bind_json<'q>(
    q: sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments>,
    v: &'q serde_json::Value,
) -> sqlx::query::Query<'q, sqlx::Postgres, sqlx::postgres::PgArguments> {
    match v {
        serde_json::Value::Null => q.bind(None::<String>),
        serde_json::Value::Bool(b) => q.bind(*b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                q.bind(i)
            } else if let Some(f) = n.as_f64() {
                q.bind(f)
            } else {
                q.bind(n.to_string())
            }
        }
        serde_json::Value::String(s) => q.bind(s.clone()),
        other => q.bind(other.to_string()),
    }
}

/// Map tên kiểu Postgres → Arrow DataType (cơ bản, §5.3).
fn map_pg_type(name: &str) -> DataType {
    match name.to_ascii_uppercase().as_str() {
        "BOOL" | "BOOLEAN" => DataType::Boolean,
        "INT2" | "INT4" | "INT8" | "SMALLINT" | "INTEGER" | "BIGINT" | "SERIAL" | "BIGSERIAL" => {
            DataType::Int64
        }
        "FLOAT4" | "FLOAT8" | "REAL" | "DOUBLE PRECISION" | "NUMERIC" | "DECIMAL" => {
            DataType::Float64
        }
        _ => DataType::Utf8,
    }
}

/// Chuyển các PgRow thành RecordBatch (schema từ metadata cột của row đầu).
fn rows_to_batches(rows: &[PgRow], batch_rows: usize) -> Result<Vec<RecordBatch>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let cols = rows[0].columns();
    let ncol = cols.len();
    let names: Vec<String> = cols.iter().map(|c| c.name().to_string()).collect();
    let types: Vec<DataType> = cols.iter().map(|c| map_pg_type(c.type_info().name())).collect();

    let fields: Vec<Field> = names
        .iter()
        .zip(&types)
        .map(|(n, t)| Field::new(n, t.clone(), true))
        .collect();
    let schema: SchemaRef = Arc::new(Schema::new(fields));

    let mut batches = Vec::new();
    let mut start = 0;
    while start < rows.len() {
        let end = (start + batch_rows).min(rows.len());
        let slice = &rows[start..end];
        let mut arrays: Vec<ArrayRef> = Vec::with_capacity(ncol);
        for (i, t) in types.iter().enumerate() {
            arrays.push(build_pg_column(slice, i, t));
        }
        batches.push(
            RecordBatch::try_new(schema.clone(), arrays).context("dựng RecordBatch Postgres")?,
        );
        start = end;
    }
    Ok(batches)
}

fn build_pg_column(rows: &[PgRow], idx: usize, dt: &DataType) -> ArrayRef {
    match dt {
        DataType::Boolean => {
            let mut b = BooleanBuilder::new();
            for r in rows {
                match r.try_get::<Option<bool>, _>(idx) {
                    Ok(Some(v)) => b.append_value(v),
                    _ => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
        DataType::Int64 => {
            let mut b = Int64Builder::new();
            for r in rows {
                match r.try_get::<Option<i64>, _>(idx) {
                    Ok(Some(v)) => b.append_value(v),
                    _ => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
        DataType::Float64 => {
            let mut b = Float64Builder::new();
            for r in rows {
                match r.try_get::<Option<f64>, _>(idx) {
                    Ok(Some(v)) => b.append_value(v),
                    _ => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
        _ => {
            let mut b = StringBuilder::new();
            for r in rows {
                match r.try_get::<Option<String>, _>(idx) {
                    Ok(Some(v)) => b.append_value(v),
                    _ => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pg_type_mapping() {
        assert_eq!(map_pg_type("int8"), DataType::Int64);
        assert_eq!(map_pg_type("BIGINT"), DataType::Int64);
        assert_eq!(map_pg_type("float8"), DataType::Float64);
        assert_eq!(map_pg_type("numeric"), DataType::Float64);
        assert_eq!(map_pg_type("bool"), DataType::Boolean);
        assert_eq!(map_pg_type("text"), DataType::Utf8);
        assert_eq!(map_pg_type("jsonb"), DataType::Utf8);
    }

    #[test]
    fn empty_rows_no_batches() {
        let out = rows_to_batches(&[], 100).unwrap();
        assert!(out.is_empty());
    }
}
