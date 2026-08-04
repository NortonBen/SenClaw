//! MySQL/MariaDB connector qua `sqlx 0.9` (§5.2 tier 1).
//!
//! Extract-only cho Phase 2. Không có MySQL sống trong CI → file này CHỈ cần COMPILE;
//! logic build SQL/map kiểu được test ở `mod.rs`.

use anyhow::{Context, Result};
use async_trait::async_trait;
use datafusion::arrow::array::{ArrayRef, Float64Builder, Int64Builder, StringBuilder};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use sqlx::mysql::{MySqlPoolOptions, MySqlRow};
use sqlx::{Column, Row, TypeInfo};
use std::sync::Arc;

use super::{
    build_create_table, build_insert_sql, build_select, chunk_rows, column_cells, quote_qualified,
    Cell, ColumnInfo, Connector, Dialect, ExtractSpec, LoadFlavor, LoadMode, LoadSpec, TableInfo,
    PG_MAX_PARAMS,
};

pub struct MysqlConnector {
    dsn: String,
}

impl MysqlConnector {
    pub fn new(dsn: String) -> Self {
        Self { dsn }
    }

    async fn pool(&self) -> Result<sqlx::MySqlPool> {
        MySqlPoolOptions::new()
            .max_connections(2)
            .connect(&self.dsn)
            .await
            .context("kết nối MySQL")
    }
}

#[async_trait]
impl Connector for MysqlConnector {
    async fn test(&self) -> Result<()> {
        let pool = self.pool().await?;
        sqlx::query("SELECT 1")
            .execute(&pool)
            .await
            .context("SELECT 1")?;
        Ok(())
    }

    async fn introspect(&self) -> Result<Vec<TableInfo>> {
        let pool = self.pool().await?;
        // MySQL: dùng DATABASE() để giới hạn schema hiện tại.
        let rows = sqlx::query(
            "SELECT table_schema, table_name, column_name, data_type, is_nullable \
             FROM information_schema.columns \
             WHERE table_schema = DATABASE() \
             ORDER BY table_name, ordinal_position",
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
            match out.last_mut().filter(|t| t.name == name) {
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
        let (sql, params) = build_select(&spec, Dialect::MYSQL);

        let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
        for p in &params {
            q = bind_json(q, p);
        }
        let rows = q.fetch_all(&pool).await.context("extract MySQL")?;

        let batches = rows_to_batches(&rows, spec.batch_rows.max(1))?;
        let s = futures_util::stream::iter(batches.into_iter().map(Ok));
        Ok(s.boxed())
    }

    async fn load(&self, spec: LoadSpec, batches: Vec<RecordBatch>) -> Result<u64> {
        let schema = match batches.first() {
            Some(b) => b.schema(),
            None => return Ok(0),
        };
        let flavor = LoadFlavor::Mysql;
        let pool = self.pool().await?;

        if spec.create_if_missing {
            let ddl = build_create_table(flavor, &spec.table, &schema);
            sqlx::query(sqlx::AssertSqlSafe(ddl))
                .execute(&pool)
                .await
                .context("CREATE TABLE MySQL")?;
        }

        let cols: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();
        let col_types: Vec<DataType> = schema
            .fields()
            .iter()
            .map(|f| f.data_type().clone())
            .collect();
        let upsert_keys: Option<Vec<String>> = match &spec.mode {
            LoadMode::Upsert { keys } => Some(keys.clone()),
            _ => None,
        };
        // MySQL cũng giới hạn 65535 placeholder/statement → dùng chung trần.
        let per_chunk = chunk_rows(cols.len(), PG_MAX_PARAMS);

        let mut tx = pool.begin().await.context("mở transaction MySQL")?;

        // FullRefresh: TRUNCATE (nếu không đủ quyền, đổi sang DELETE thủ công ngoài phạm vi).
        if spec.mode == LoadMode::FullRefresh {
            let table = quote_qualified(&spec.table, Dialect::MYSQL.quote);
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
                let sql = build_insert_sql(flavor, &spec.table, &cols, n, upsert_keys.as_deref());
                let mut q = sqlx::query(sqlx::AssertSqlSafe(sql));
                for r in start..end {
                    for (ci, cell) in col_cells.iter().enumerate() {
                        q = bind_cell(q, &col_types[ci], &cell[r]);
                    }
                }
                q.execute(&mut *tx).await.context("INSERT chunk MySQL")?;
                written += n as u64;
                start = end;
            }
        }

        tx.commit().await.context("commit load MySQL")?;
        Ok(written)
    }
}

/// Bind một `Cell` vào query MySQL theo KIỂU CỘT (NULL cũng đúng kiểu Rust).
fn bind_cell<'q>(
    q: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    dt: &DataType,
    cell: &Cell,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
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
        _ => q.bind(cell.as_text()),
    }
}

fn bind_json<'q>(
    q: sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments>,
    v: &'q serde_json::Value,
) -> sqlx::query::Query<'q, sqlx::MySql, sqlx::mysql::MySqlArguments> {
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

/// Map tên kiểu MySQL → Arrow DataType (cơ bản, §5.3).
fn map_mysql_type(name: &str) -> DataType {
    match name.to_ascii_uppercase().as_str() {
        "TINYINT" | "SMALLINT" | "MEDIUMINT" | "INT" | "INTEGER" | "BIGINT" | "YEAR" => {
            DataType::Int64
        }
        "FLOAT" | "DOUBLE" | "DECIMAL" | "NEWDECIMAL" => DataType::Float64,
        _ => DataType::Utf8,
    }
}

fn rows_to_batches(rows: &[MySqlRow], batch_rows: usize) -> Result<Vec<RecordBatch>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let cols = rows[0].columns();
    let ncol = cols.len();
    let names: Vec<String> = cols.iter().map(|c| c.name().to_string()).collect();
    let types: Vec<DataType> = cols
        .iter()
        .map(|c| map_mysql_type(c.type_info().name()))
        .collect();

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
            arrays.push(build_my_column(slice, i, t));
        }
        batches
            .push(RecordBatch::try_new(schema.clone(), arrays).context("dựng RecordBatch MySQL")?);
        start = end;
    }
    Ok(batches)
}

fn build_my_column(rows: &[MySqlRow], idx: usize, dt: &DataType) -> ArrayRef {
    match dt {
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
    fn mysql_type_mapping() {
        assert_eq!(map_mysql_type("int"), DataType::Int64);
        assert_eq!(map_mysql_type("BIGINT"), DataType::Int64);
        assert_eq!(map_mysql_type("double"), DataType::Float64);
        assert_eq!(map_mysql_type("decimal"), DataType::Float64);
        assert_eq!(map_mysql_type("varchar"), DataType::Utf8);
        assert_eq!(map_mysql_type("datetime"), DataType::Utf8);
    }

    #[test]
    fn empty_rows_no_batches() {
        let out = rows_to_batches(&[], 100).unwrap();
        assert!(out.is_empty());
    }
}
