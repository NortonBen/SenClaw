//! ClickHouse connector qua HTTP interface (reqwest THUẦN) — §5.2 tier mở rộng.
//!
//! LỆCH DESIGN CÓ CHỦ Ý (§5.2 dòng ClickHouse gợi ý crate `clickhouse 0.15.1` +
//! `FORMAT ArrowStream` sau feature-gate): ở đây KHÔNG thêm dependency mới nào (tránh
//! rủi ro pin arrow của crate `clickhouse`, giữ build sạch). Ta chỉ dùng `reqwest`
//! (đã có) gọi HTTP interface cổng 8123, đọc/ghi bằng `FORMAT JSONEachRow` (1 object
//! JSON / dòng) — cùng interface Connector như postgres/sqlite.
//!
//! GIỚI HẠN E2E: các test co-located CHỈ kiểm các hàm THUẦN (parse DSN, build SQL,
//! inline literal, dựng DDL/INSERT body, type map). Round-trip thực (extract/load) cần
//! một ClickHouse server sống — CHƯA e2e trong CI (giống postgres/mysql hiện tại).

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use datafusion::arrow::array::{
    ArrayRef, BooleanBuilder, Float64Builder, Int64Builder, StringBuilder,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef, TimeUnit};
use datafusion::arrow::record_batch::RecordBatch;
use futures_util::stream::BoxStream;
use futures_util::StreamExt;
use serde_json::{Map, Value};
use std::sync::Arc;

use super::{
    build_select, column_cells, quote_ident, quote_qualified, Cell, ColumnInfo, Connector, Dialect,
    ExtractSpec, LoadMode, LoadSpec, TableInfo,
};

/// Số dòng tối đa mỗi HTTP INSERT (chặn body quá lớn). ClickHouse không có trần
/// placeholder như PG nên đây thuần là ngưỡng kích thước body.
const CH_INSERT_ROWS: usize = 50_000;

// ---------------------------------------------------------------------------
// Cấu hình kết nối rút từ DSN
// ---------------------------------------------------------------------------

/// Cấu hình HTTP đã parse từ DSN ClickHouse.
#[derive(Debug, Clone, PartialEq)]
struct ChConfig {
    /// Endpoint POST, luôn kết thúc `/` (vd "http://host:8123/").
    endpoint: String,
    user: Option<String>,
    password: Option<String>,
    database: Option<String>,
}

/// Parse DSN → `ChConfig`. Chấp nhận:
/// - `clickhouse://user:pass@host:8123/db` (scheme clickhouse/clickhouses → http/https)
/// - `http://host:8123/?database=db` (user/password/database cũng nhận ở query)
fn parse_ch_dsn(dsn: &str) -> Result<ChConfig> {
    let url =
        url::Url::parse(dsn).with_context(|| format!("DSN ClickHouse không hợp lệ: {dsn}"))?;

    let https = matches!(
        url.scheme(),
        "https" | "clickhouses" | "clickhouse+https" | "clickhouse+tls"
    );
    let host = url.host_str().context("DSN ClickHouse thiếu host")?;
    let port = url.port().unwrap_or(8123);
    let proto = if https { "https" } else { "http" };
    let endpoint = format!("{proto}://{host}:{port}/");

    // userinfo có ưu tiên; nếu trống thì lấy từ query param.
    let mut user = if url.username().is_empty() {
        None
    } else {
        Some(url.username().to_string())
    };
    let mut password = url.password().map(|p| p.to_string());

    // database: path (bỏ '/' đầu) nếu có, ngược lại query 'database'.
    let mut database = {
        let p = url.path().trim_start_matches('/');
        if p.is_empty() {
            None
        } else {
            Some(p.to_string())
        }
    };

    for (k, v) in url.query_pairs() {
        match k.as_ref() {
            "database" | "db" => database = Some(v.into_owned()),
            "user" | "username" => {
                if user.is_none() {
                    user = Some(v.into_owned());
                }
            }
            "password" => {
                if password.is_none() {
                    password = Some(v.into_owned());
                }
            }
            _ => {}
        }
    }

    Ok(ChConfig {
        endpoint,
        user,
        password,
        database,
    })
}

// ---------------------------------------------------------------------------
// Connector
// ---------------------------------------------------------------------------

pub struct ClickHouseConnector {
    cfg: ChConfig,
    client: reqwest::Client,
}

impl ClickHouseConnector {
    pub fn new(dsn: String) -> Result<Self> {
        let cfg = parse_ch_dsn(&dsn)?;
        Ok(Self {
            cfg,
            client: reqwest::Client::new(),
        })
    }

    /// Gửi một câu SQL (body POST) tới HTTP interface, trả body phản hồi dạng text.
    /// auth qua header `X-ClickHouse-User`/`X-ClickHouse-Key`; database qua `?database=`.
    async fn run_sql(&self, sql: String) -> Result<String> {
        let mut req = self.client.post(&self.cfg.endpoint);
        if let Some(db) = &self.cfg.database {
            req = req.query(&[("database", db.as_str())]);
        }
        if let Some(u) = &self.cfg.user {
            req = req.header("X-ClickHouse-User", u);
        }
        if let Some(p) = &self.cfg.password {
            req = req.header("X-ClickHouse-Key", p);
        }
        let resp = req
            .body(sql)
            .send()
            .await
            .context("gửi HTTP tới ClickHouse")?;
        let status = resp.status();
        let text = resp.text().await.context("đọc phản hồi ClickHouse")?;
        if !status.is_success() {
            bail!("ClickHouse trả {status}: {}", text.trim());
        }
        Ok(text)
    }
}

#[async_trait]
impl Connector for ClickHouseConnector {
    async fn test(&self) -> Result<()> {
        self.run_sql("SELECT 1".into()).await.map(|_| ())
    }

    async fn introspect(&self) -> Result<Vec<TableInfo>> {
        // Danh sách bảng của database hiện tại.
        let tbl_out = self
            .run_sql(
                "SELECT name FROM system.tables WHERE database = currentDatabase() \
                 ORDER BY name FORMAT JSONEachRow"
                    .into(),
            )
            .await?;

        let mut out = Vec::new();
        for line in json_lines(&tbl_out) {
            let v: Value = serde_json::from_str(line).context("parse dòng system.tables")?;
            let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
            if name.is_empty() {
                continue;
            }

            // Cột: name + native type (giữ nguyên chuỗi như postgres; nullable suy từ Nullable()).
            let col_sql = format!(
                "SELECT name, type FROM system.columns \
                 WHERE database = currentDatabase() AND table = {} \
                 ORDER BY position FORMAT JSONEachRow",
                ch_string_literal(&name)
            );
            let col_out = self.run_sql(col_sql).await?;
            let mut columns = Vec::new();
            for cl in json_lines(&col_out) {
                let cv: Value = serde_json::from_str(cl).context("parse dòng system.columns")?;
                let cname = cv.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
                let ctype = cv.get("type").and_then(|x| x.as_str()).unwrap_or("").to_string();
                columns.push(ColumnInfo {
                    nullable: is_ch_nullable(&ctype),
                    name: cname,
                    data_type: ctype,
                });
            }

            // Ước lượng dòng — best-effort (bỏ qua lỗi).
            let row_estimate = self
                .run_sql(format!(
                    "SELECT count() AS c FROM {} FORMAT JSONEachRow",
                    quote_qualified(&name, '`')
                ))
                .await
                .ok()
                .and_then(|s| {
                    json_lines(&s)
                        .next()
                        .and_then(|l| serde_json::from_str::<Value>(l).ok())
                        .and_then(|v| v.get("c").cloned())
                        .and_then(|c| json_count_to_i64(&c))
                });

            out.push(TableInfo {
                schema: None,
                name,
                columns,
                row_estimate,
            });
        }
        Ok(out)
    }

    async fn extract(&self, spec: ExtractSpec) -> Result<BoxStream<'static, Result<RecordBatch>>> {
        // Câu SELECT (tái dùng build_select) — CH không bind params → inline literal.
        let (base_sql, params) = build_select(&spec, Dialect::CLICKHOUSE);
        let inlined = inline_params(&base_sql, &params);

        // DESCRIBE cho tên + kiểu + THỨ TỰ cột (đúng cả Table lẫn Query source; có schema
        // ngay cả khi 0 dòng).
        let desc_out = self
            .run_sql(format!("DESCRIBE ({inlined}) FORMAT JSONEachRow"))
            .await?;
        let mut names: Vec<String> = Vec::new();
        let mut logical: Vec<DataType> = Vec::new();
        for line in json_lines(&desc_out) {
            let v: Value = serde_json::from_str(line).context("parse DESCRIBE")?;
            let n = v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
            names.push(n);
            logical.push(map_ch_type(t));
        }

        // Dữ liệu: mỗi dòng là 1 object JSON.
        let data_out = self
            .run_sql(format!("{inlined} FORMAT JSONEachRow"))
            .await?;
        let mut rows: Vec<Map<String, Value>> = Vec::new();
        for line in json_lines(&data_out) {
            let obj: Map<String, Value> =
                serde_json::from_str(line).context("parse dòng dữ liệu JSONEachRow")?;
            rows.push(obj);
        }

        let batches = json_rows_to_batches(&names, &logical, &rows, spec.batch_rows.max(1))?;
        let s = futures_util::stream::iter(batches.into_iter().map(Ok));
        Ok(s.boxed())
    }

    async fn load(&self, spec: LoadSpec, batches: Vec<RecordBatch>) -> Result<u64> {
        let schema = match batches.first() {
            Some(b) => b.schema(),
            None => return Ok(0),
        };

        // Upsert: ClickHouse không có ON CONFLICT chuẩn.
        if let LoadMode::Upsert { .. } = spec.mode {
            bail!(
                "ClickHouse dùng ReplacingMergeTree cho upsert, chưa hỗ trợ ở đây; \
                 dùng append hoặc full_refresh"
            );
        }

        let table_q = quote_qualified(&spec.table, '`');

        if spec.create_if_missing {
            let ddl = build_create_table_ch(&spec.table, &schema);
            self.run_sql(ddl).await?;
        }

        // FullRefresh: TRUNCATE rồi INSERT — GIỚI HẠN: ClickHouse KHÔNG có transaction
        // đa-câu như PG, nên hai bước này KHÔNG nguyên tử (nếu INSERT lỗi giữa chừng, bảng
        // đã bị xoá). Chấp nhận theo fallback §5.2.
        if spec.mode == LoadMode::FullRefresh {
            self.run_sql(format!("TRUNCATE TABLE {table_q}")).await?;
        }

        let cols: Vec<String> = schema.fields().iter().map(|f| f.name().clone()).collect();

        let mut written: u64 = 0;
        for batch in &batches {
            let col_cells: Vec<Vec<Cell>> = batch.columns().iter().map(column_cells).collect();
            let nrows = batch.num_rows();
            let mut start = 0;
            while start < nrows {
                let end = (start + CH_INSERT_ROWS).min(nrows);
                let body = build_insert_jsoneachrow(&table_q, &cols, &col_cells, start, end);
                self.run_sql(body).await?;
                written += (end - start) as u64;
                start = end;
            }
        }
        Ok(written)
    }
}

// ---------------------------------------------------------------------------
// Hàm thuần — build SQL / inline literal / type map (test được không cần server)
// ---------------------------------------------------------------------------

/// Lặp qua các dòng phi-rỗng của một body JSONEachRow.
fn json_lines(body: &str) -> impl Iterator<Item = &str> {
    body.lines().map(|l| l.trim()).filter(|l| !l.is_empty())
}

/// JSON scalar count() → i64 (JSONEachRow có thể render UInt64 dạng số HOẶC chuỗi).
fn json_count_to_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.parse::<i64>().ok(),
        _ => None,
    }
}

/// Bọc chuỗi thành literal ClickHouse an toàn: `'...'` với escape `\` và `'`.
fn ch_string_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            _ => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Một JSON scalar (từ CursorPred) → literal ClickHouse.
fn ch_literal(v: &Value) -> String {
    match v {
        Value::Null => "NULL".to_string(),
        Value::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::String(s) => ch_string_literal(s),
        other => ch_string_literal(&other.to_string()),
    }
}

/// Thay lần lượt mỗi `?` trong `sql` bằng literal đã escape của `params` (theo thứ tự).
/// CH HTTP không bind params như sqlx nên phải nội suy — literal đã được escape an toàn.
fn inline_params(sql: &str, params: &[Value]) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut idx = 0usize;
    for c in sql.chars() {
        if c == '?' && idx < params.len() {
            out.push_str(&ch_literal(&params[idx]));
            idx += 1;
        } else {
            out.push(c);
        }
    }
    out
}

/// Bỏ các wrapper `Nullable(...)` / `LowCardinality(...)` để lấy kiểu lõi.
fn strip_ch_wrappers(raw: &str) -> &str {
    let t = raw.trim();
    for w in ["Nullable(", "LowCardinality("] {
        if let Some(inner) = t.strip_prefix(w) {
            if let Some(core) = inner.strip_suffix(')') {
                return strip_ch_wrappers(core);
            }
        }
    }
    t
}

/// Cột ClickHouse có nullable không (native type bọc `Nullable(...)`).
fn is_ch_nullable(raw: &str) -> bool {
    raw.trim().starts_with("Nullable(")
}

/// Map tên kiểu ClickHouse → Arrow DataType (§5.3).
/// String/FixedString→Utf8, Int*/UInt*→Int64, Float*→Float64, Bool→Boolean,
/// DateTime*→Timestamp(µs), Date*→Date32, Decimal(p,s)→Decimal128(p,s), khác→Utf8.
fn map_ch_type(raw: &str) -> DataType {
    let t = strip_ch_wrappers(raw);
    // DateTime PHẢI kiểm trước Date ("DateTime".starts_with("Date") == true).
    if t.starts_with("DateTime") {
        DataType::Timestamp(TimeUnit::Microsecond, None)
    } else if t.starts_with("Date") {
        DataType::Date32
    } else if t.starts_with("String") || t.starts_with("FixedString") {
        DataType::Utf8
    } else if t.starts_with("Bool") {
        DataType::Boolean
    } else if t.starts_with("Int") || t.starts_with("UInt") {
        DataType::Int64
    } else if t.starts_with("Float") {
        DataType::Float64
    } else if t.starts_with("Decimal") {
        let (p, s) = parse_decimal_params(t).unwrap_or((38, 0));
        DataType::Decimal128(p, s)
    } else {
        DataType::Utf8
    }
}

/// Rút (precision, scale) từ "Decimal(p, s)" / "Decimal64(s)" — best-effort.
fn parse_decimal_params(t: &str) -> Option<(u8, i8)> {
    let inner = t.split_once('(')?.1.strip_suffix(')')?;
    let nums: Vec<i64> = inner
        .split(',')
        .filter_map(|x| x.trim().parse::<i64>().ok())
        .collect();
    match nums.as_slice() {
        [p, s] => Some((*p as u8, *s as i8)),
        [s] => Some((38, *s as i8)),
        _ => None,
    }
}

/// Kiểu Arrow "dựng được" từ JSONEachRow: gộp mọi kiểu phi-primitive (Timestamp/Date/
/// Decimal/Binary/nested) về Utf8 — JSONEachRow render chúng dạng chuỗi, giữ lossless.
fn build_type(dt: &DataType) -> DataType {
    match dt {
        DataType::Boolean | DataType::Int64 | DataType::Float64 | DataType::Utf8 => dt.clone(),
        _ => DataType::Utf8,
    }
}

/// Dựng RecordBatch từ các object JSON (JSONEachRow). Thứ tự/tên/kiểu cột lấy từ DESCRIBE.
fn json_rows_to_batches(
    names: &[String],
    logical: &[DataType],
    rows: &[Map<String, Value>],
    batch_rows: usize,
) -> Result<Vec<RecordBatch>> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let build_types: Vec<DataType> = logical.iter().map(build_type).collect();
    let fields: Vec<Field> = names
        .iter()
        .zip(&build_types)
        .map(|(n, t)| Field::new(n, t.clone(), true))
        .collect();
    let schema: SchemaRef = Arc::new(Schema::new(fields));

    // Không có dòng → trả một batch rỗng để giữ schema (đồng nhất với sqlite).
    if rows.is_empty() {
        let cols: Vec<ArrayRef> = build_types
            .iter()
            .zip(names)
            .map(|(t, n)| build_column(t, n, &[]))
            .collect();
        return Ok(vec![
            RecordBatch::try_new(schema, cols).context("RecordBatch ClickHouse rỗng")?,
        ]);
    }

    let mut batches = Vec::new();
    let mut start = 0;
    while start < rows.len() {
        let end = (start + batch_rows).min(rows.len());
        let slice = &rows[start..end];
        let cols: Vec<ArrayRef> = build_types
            .iter()
            .zip(names)
            .map(|(t, n)| build_column(t, n, slice))
            .collect();
        batches.push(
            RecordBatch::try_new(schema.clone(), cols)
                .context("dựng RecordBatch từ ClickHouse")?,
        );
        start = end;
    }
    Ok(batches)
}

/// Dựng một cột Arrow (theo `build_type`) đọc key `name` từ mỗi object JSON.
fn build_column(dt: &DataType, name: &str, rows: &[Map<String, Value>]) -> ArrayRef {
    let get = |r: &Map<String, Value>| r.get(name).unwrap_or(&Value::Null).clone();
    match dt {
        DataType::Boolean => {
            let mut b = BooleanBuilder::new();
            for r in rows {
                match json_as_bool(&get(r)) {
                    Some(v) => b.append_value(v),
                    None => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
        DataType::Int64 => {
            let mut b = Int64Builder::new();
            for r in rows {
                match json_as_i64(&get(r)) {
                    Some(v) => b.append_value(v),
                    None => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
        DataType::Float64 => {
            let mut b = Float64Builder::new();
            for r in rows {
                match json_as_f64(&get(r)) {
                    Some(v) => b.append_value(v),
                    None => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
        // Utf8 (mặc định): String giữ nguyên, số/bool serialize, null → null.
        _ => {
            let mut b = StringBuilder::new();
            for r in rows {
                match json_as_text(&get(r)) {
                    Some(v) => b.append_value(v),
                    None => b.append_null(),
                }
            }
            Arc::new(b.finish())
        }
    }
}

fn json_as_bool(v: &Value) -> Option<bool> {
    match v {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|i| i != 0),
        Value::String(s) => match s.as_str() {
            "true" | "1" => Some(true),
            "false" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn json_as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.parse::<i64>().ok(),
        Value::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

fn json_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn json_as_text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Load — Arrow→ClickHouse DDL + INSERT ... FORMAT JSONEachRow
// ---------------------------------------------------------------------------

/// Arrow DataType → kiểu cột ClickHouse (LÕI, chưa bọc Nullable).
fn ch_ddl_type(dt: &DataType) -> String {
    match dt {
        DataType::Boolean => "Bool".into(),
        DataType::Int8 => "Int8".into(),
        DataType::Int16 => "Int16".into(),
        DataType::Int32 => "Int32".into(),
        DataType::Int64 => "Int64".into(),
        DataType::UInt8 => "UInt8".into(),
        DataType::UInt16 => "UInt16".into(),
        DataType::UInt32 => "UInt32".into(),
        DataType::UInt64 => "UInt64".into(),
        DataType::Float16 | DataType::Float32 => "Float32".into(),
        DataType::Float64 => "Float64".into(),
        DataType::Utf8 | DataType::LargeUtf8 => "String".into(),
        DataType::Binary | DataType::LargeBinary | DataType::FixedSizeBinary(_) => "String".into(),
        DataType::Date32 | DataType::Date64 => "Date".into(),
        DataType::Timestamp(_, _) => "DateTime64(6)".into(),
        DataType::Decimal128(p, s) | DataType::Decimal256(p, s) => format!("Decimal({p},{s})"),
        // nested/khác → String (serialize JSON khi ghi qua Cell).
        _ => "String".into(),
    }
}

/// `CREATE TABLE IF NOT EXISTS <table> (...) ENGINE = MergeTree ORDER BY tuple()`.
/// Cột nullable (Arrow field.is_nullable()) được bọc `Nullable(T)`.
fn build_create_table_ch(table: &str, schema: &SchemaRef) -> String {
    let cols = schema
        .fields()
        .iter()
        .map(|f| {
            let inner = ch_ddl_type(f.data_type());
            let ty = if f.is_nullable() {
                format!("Nullable({inner})")
            } else {
                inner
            };
            format!("{} {}", quote_ident(f.name(), '`'), ty)
        })
        .collect::<Vec<_>>()
        .join(",\n  ");
    format!(
        "CREATE TABLE IF NOT EXISTS {} (\n  {}\n) ENGINE = MergeTree ORDER BY tuple()",
        quote_qualified(table, '`'),
        cols
    )
}

/// Cell → serde_json::Value cho một ô JSONEachRow.
fn cell_to_json(c: &Cell) -> Value {
    match c {
        Cell::Null => Value::Null,
        Cell::Int(i) => Value::Number((*i).into()),
        Cell::Float(f) => serde_json::Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Cell::Bool(b) => Value::Bool(*b),
        Cell::Text(s) => Value::String(s.clone()),
        // Bytes → chuỗi (ClickHouse String nhận UTF-8 text từ JSONEachRow).
        Cell::Bytes(b) => Value::String(String::from_utf8_lossy(b).into_owned()),
    }
}

/// Dựng body `INSERT INTO t (cols) FORMAT JSONEachRow\n{...}\n...` cho dải [start,end).
fn build_insert_jsoneachrow(
    table_q: &str,
    cols: &[String],
    col_cells: &[Vec<Cell>],
    start: usize,
    end: usize,
) -> String {
    let cols_sql = cols
        .iter()
        .map(|c| quote_ident(c, '`'))
        .collect::<Vec<_>>()
        .join(", ");
    let mut body = format!("INSERT INTO {table_q} ({cols_sql}) FORMAT JSONEachRow\n");
    for r in start..end {
        let mut obj = Map::new();
        for (ci, name) in cols.iter().enumerate() {
            obj.insert(name.clone(), cell_to_json(&col_cells[ci][r]));
        }
        // serde_json::to_string trên một Map — không cần preserve_order (nạp theo tên).
        body.push_str(&serde_json::to_string(&Value::Object(obj)).unwrap_or_default());
        body.push('\n');
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- DSN parse ----

    #[test]
    fn parse_clickhouse_scheme_userpass_db() {
        let c = parse_ch_dsn("clickhouse://alice:secret@ch.example:8123/analytics").unwrap();
        assert_eq!(c.endpoint, "http://ch.example:8123/");
        assert_eq!(c.user.as_deref(), Some("alice"));
        assert_eq!(c.password.as_deref(), Some("secret"));
        assert_eq!(c.database.as_deref(), Some("analytics"));
    }

    #[test]
    fn parse_http_scheme_database_query() {
        let c = parse_ch_dsn("http://ch.example:8123/?database=metrics").unwrap();
        assert_eq!(c.endpoint, "http://ch.example:8123/");
        assert_eq!(c.database.as_deref(), Some("metrics"));
        assert!(c.user.is_none());
        assert!(c.password.is_none());
    }

    #[test]
    fn parse_default_port_and_https_and_query_creds() {
        // Không port → 8123; scheme clickhouses → https; creds ở query.
        let c = parse_ch_dsn("clickhouses://host/db?user=bob&password=pw").unwrap();
        assert_eq!(c.endpoint, "https://host:8123/");
        assert_eq!(c.user.as_deref(), Some("bob"));
        assert_eq!(c.password.as_deref(), Some("pw"));
        assert_eq!(c.database.as_deref(), Some("db"));
    }

    // ---- SELECT + FORMAT JSONEachRow (tái dùng build_select + inline) ----

    fn ch_select(spec: &ExtractSpec) -> String {
        let (sql, params) = build_select(spec, Dialect::CLICKHOUSE);
        format!("{} FORMAT JSONEachRow", inline_params(&sql, &params))
    }

    #[test]
    fn select_table_all_cols_format() {
        let spec = ExtractSpec {
            source: super::super::SourceRel::Table {
                schema: None,
                name: "orders".into(),
            },
            columns: None,
            cursor: None,
            batch_rows: 100,
        };
        assert_eq!(ch_select(&spec), "SELECT * FROM `orders` FORMAT JSONEachRow");
    }

    #[test]
    fn select_cursor_inlines_literals() {
        // Cursor Ge số + biên trên → hai literal nội suy (không placeholder `?`).
        let spec = ExtractSpec {
            source: super::super::SourceRel::Table {
                schema: None,
                name: "events".into(),
            },
            columns: Some(vec!["id".into()]),
            cursor: Some(super::super::CursorPred {
                column: "id".into(),
                op: super::super::CursorOp::Gt,
                from: json!(100),
                to: Some(json!(200)),
            }),
            batch_rows: 100,
        };
        assert_eq!(
            ch_select(&spec),
            "SELECT `id` FROM `events` WHERE `id` > 100 AND `id` < 200 \
             ORDER BY `id` ASC FORMAT JSONEachRow"
        );
    }

    #[test]
    fn select_cursor_string_literal_escaped() {
        let spec = ExtractSpec {
            source: super::super::SourceRel::Table {
                schema: None,
                name: "t".into(),
            },
            columns: None,
            cursor: Some(super::super::CursorPred {
                column: "d".into(),
                op: super::super::CursorOp::Ge,
                from: json!("2024-01-01"),
                to: None,
            }),
            batch_rows: 100,
        };
        assert_eq!(
            ch_select(&spec),
            "SELECT * FROM `t` WHERE `d` >= '2024-01-01' ORDER BY `d` ASC FORMAT JSONEachRow"
        );
    }

    #[test]
    fn ch_string_literal_escapes_quote_and_backslash() {
        assert_eq!(ch_string_literal("O'Brien"), r"'O\'Brien'");
        assert_eq!(ch_string_literal(r"a\b"), r"'a\\b'");
    }

    #[test]
    fn inline_params_stops_at_param_count() {
        // `?` dư (không có param) giữ nguyên.
        assert_eq!(inline_params("a = ? b = ?", &[json!(1)]), "a = 1 b = ?");
    }

    // ---- type map CH → Arrow (introspect) ----

    #[test]
    fn map_ch_type_covers_families() {
        assert_eq!(map_ch_type("String"), DataType::Utf8);
        assert_eq!(map_ch_type("FixedString(8)"), DataType::Utf8);
        assert_eq!(map_ch_type("Int32"), DataType::Int64);
        assert_eq!(map_ch_type("UInt64"), DataType::Int64);
        assert_eq!(map_ch_type("Float32"), DataType::Float64);
        assert_eq!(map_ch_type("Float64"), DataType::Float64);
        assert_eq!(map_ch_type("Bool"), DataType::Boolean);
        assert_eq!(map_ch_type("Date"), DataType::Date32);
        assert_eq!(map_ch_type("Date32"), DataType::Date32);
        assert_eq!(
            map_ch_type("DateTime"),
            DataType::Timestamp(TimeUnit::Microsecond, None)
        );
        assert_eq!(
            map_ch_type("DateTime64(3)"),
            DataType::Timestamp(TimeUnit::Microsecond, None)
        );
        assert_eq!(map_ch_type("Decimal(10, 2)"), DataType::Decimal128(10, 2));
        // Nullable/LowCardinality wrapper bị bóc.
        assert_eq!(map_ch_type("Nullable(Int64)"), DataType::Int64);
        assert_eq!(
            map_ch_type("LowCardinality(Nullable(String))"),
            DataType::Utf8
        );
    }

    #[test]
    fn is_ch_nullable_detects_wrapper() {
        assert!(is_ch_nullable("Nullable(Int64)"));
        assert!(!is_ch_nullable("Int64"));
        assert!(!is_ch_nullable("LowCardinality(String)"));
    }

    // ---- CREATE TABLE ClickHouse DDL ----

    #[test]
    fn create_table_ch_shape_and_types() {
        use datafusion::arrow::datatypes::{Field, Schema};
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("price", DataType::Float64, true),
            Field::new("label", DataType::Utf8, true),
            Field::new("ts", DataType::Timestamp(TimeUnit::Microsecond, None), true),
            Field::new("amount", DataType::Decimal128(12, 4), true),
            Field::new("flag", DataType::Boolean, true),
        ]));
        let ddl = build_create_table_ch("analytics.dest", &schema);
        assert!(
            ddl.starts_with("CREATE TABLE IF NOT EXISTS `analytics`.`dest` ("),
            "{ddl}"
        );
        // non-nullable → không bọc Nullable.
        assert!(ddl.contains("`id` Int64"), "{ddl}");
        assert!(ddl.contains("`price` Nullable(Float64)"), "{ddl}");
        assert!(ddl.contains("`label` Nullable(String)"), "{ddl}");
        assert!(ddl.contains("`ts` Nullable(DateTime64(6))"), "{ddl}");
        assert!(ddl.contains("`amount` Nullable(Decimal(12,4))"), "{ddl}");
        assert!(ddl.contains("`flag` Nullable(Bool)"), "{ddl}");
        assert!(ddl.ends_with(") ENGINE = MergeTree ORDER BY tuple()"), "{ddl}");
    }

    #[test]
    fn ch_ddl_type_binary_and_widths() {
        assert_eq!(ch_ddl_type(&DataType::Binary), "String");
        assert_eq!(ch_ddl_type(&DataType::Int8), "Int8");
        assert_eq!(ch_ddl_type(&DataType::UInt32), "UInt32");
        assert_eq!(ch_ddl_type(&DataType::Float32), "Float32");
        assert_eq!(ch_ddl_type(&DataType::Date32), "Date");
    }

    // ---- INSERT ... FORMAT JSONEachRow body ----

    #[test]
    fn insert_jsoneachrow_encodes_values_and_nulls() {
        let cols = vec![
            "id".to_string(),
            "amount".to_string(),
            "label".to_string(),
            "ok".to_string(),
        ];
        // 2 dòng: dòng 2 có null ở amount + label.
        let col_cells: Vec<Vec<Cell>> = vec![
            vec![Cell::Int(1), Cell::Int(2)],
            vec![Cell::Float(9.5), Cell::Null],
            vec![Cell::Text("a".into()), Cell::Null],
            vec![Cell::Bool(true), Cell::Bool(false)],
        ];
        let body = build_insert_jsoneachrow("`db`.`t`", &cols, &col_cells, 0, 2);
        let mut lines = body.lines();
        assert_eq!(
            lines.next().unwrap(),
            "INSERT INTO `db`.`t` (`id`, `amount`, `label`, `ok`) FORMAT JSONEachRow"
        );
        // Parse lại từng dòng JSON để kiểm giá trị/kiểu (không phụ thuộc thứ tự khoá).
        let r1: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(r1["id"], json!(1));
        assert_eq!(r1["amount"], json!(9.5));
        assert_eq!(r1["label"], json!("a"));
        assert_eq!(r1["ok"], json!(true));
        let r2: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(r2["id"], json!(2));
        assert_eq!(r2["amount"], Value::Null);
        assert_eq!(r2["label"], Value::Null);
        assert_eq!(r2["ok"], json!(false));
        assert!(lines.next().is_none());
    }

    // ---- extract: dựng RecordBatch từ JSON rows (không cần server) ----

    #[test]
    fn json_rows_build_typed_batch() {
        use datafusion::arrow::array::{Array, Float64Array, Int64Array, StringArray};
        let names = vec!["id".to_string(), "price".to_string(), "label".to_string()];
        let logical = vec![DataType::Int64, DataType::Float64, DataType::Utf8];
        let rows: Vec<Map<String, Value>> = vec![
            serde_json::from_str(r#"{"id":1,"price":9.5,"label":"a"}"#).unwrap(),
            serde_json::from_str(r#"{"id":2,"price":null,"label":null}"#).unwrap(),
        ];
        let batches = json_rows_to_batches(&names, &logical, &rows, 8192).unwrap();
        assert_eq!(batches.len(), 1);
        let b = &batches[0];
        assert_eq!(b.num_rows(), 2);
        let id = b.column(0).as_any().downcast_ref::<Int64Array>().unwrap();
        assert_eq!(id.value(0), 1);
        assert_eq!(id.value(1), 2);
        let price = b.column(1).as_any().downcast_ref::<Float64Array>().unwrap();
        assert!((price.value(0) - 9.5).abs() < 1e-9);
        assert!(price.is_null(1));
        let label = b.column(2).as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(label.value(0), "a");
        assert!(label.is_null(1));
    }

    #[test]
    fn json_rows_empty_keeps_schema() {
        let names = vec!["id".to_string()];
        let logical = vec![DataType::Int64];
        let batches = json_rows_to_batches(&names, &logical, &[], 100).unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].num_rows(), 0);
        assert_eq!(batches[0].schema().field(0).name(), "id");
    }

    #[test]
    fn timestamp_logical_builds_as_utf8() {
        use datafusion::arrow::array::StringArray;
        // DateTime → logical Timestamp, nhưng build_type gộp về Utf8 (JSONEachRow trả chuỗi).
        let names = vec!["ts".to_string()];
        let logical = vec![DataType::Timestamp(TimeUnit::Microsecond, None)];
        let rows: Vec<Map<String, Value>> =
            vec![serde_json::from_str(r#"{"ts":"2024-01-01 00:00:00"}"#).unwrap()];
        let batches = json_rows_to_batches(&names, &logical, &rows, 100).unwrap();
        let col = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(col.value(0), "2024-01-01 00:00:00");
    }

    // ---- redact_dsn: clickhouse:// DSN che password (structural, dùng chung) ----

    #[test]
    fn redact_clickhouse_dsn_hides_password() {
        assert_eq!(
            super::super::redact_dsn("clickhouse://alice:secret@host:8123/db"),
            "clickhouse://alice:•••@host:8123/db"
        );
        // Không có password → giữ nguyên.
        assert_eq!(
            super::super::redact_dsn("clickhouse://alice@host:8123/db"),
            "clickhouse://alice@host:8123/db"
        );
    }

    // ---- upsert bị từ chối rõ ràng ----

    #[tokio::test]
    async fn upsert_returns_clear_error() {
        use datafusion::arrow::array::Int64Array;
        use datafusion::arrow::datatypes::{Field, Schema};
        let c = ClickHouseConnector::new("clickhouse://host:8123/db".into()).unwrap();
        let schema: SchemaRef =
            Arc::new(Schema::new(vec![Field::new("id", DataType::Int64, true)]));
        let batch =
            RecordBatch::try_new(schema, vec![Arc::new(Int64Array::from(vec![1i64]))]).unwrap();
        let err = c
            .load(
                LoadSpec {
                    table: "t".into(),
                    mode: LoadMode::Upsert { keys: vec!["id".into()] },
                    create_if_missing: false,
                },
                vec![batch],
            )
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ReplacingMergeTree"), "{msg}");
        assert!(msg.contains("append") && msg.contains("full_refresh"), "{msg}");
    }
}
