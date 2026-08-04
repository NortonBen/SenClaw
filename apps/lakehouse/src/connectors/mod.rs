//! Connectors — extract từ database nguồn (design §5).
//!
//! Phase 2: chỉ `extract` (test/introspect/extract). `load` = Phase 4.
//! Tier 1: Postgres/MySQL qua `sqlx 0.9`, SQLite qua `rusqlite` sẵn có.
//!
//! `build_select` và `redact_dsn` là hàm thuần — test được KHÔNG cần DB sống.

use anyhow::Result;
use async_trait::async_trait;
use datafusion::arrow::array::{
    Array, ArrayRef, BinaryArray, BooleanArray, Float32Array, Float64Array, Int16Array, Int32Array,
    Int64Array, Int8Array, LargeBinaryArray, LargeStringArray, StringArray, UInt16Array,
    UInt32Array, UInt64Array, UInt8Array,
};
use datafusion::arrow::compute::cast;
use datafusion::arrow::datatypes::{DataType, SchemaRef};
use datafusion::arrow::record_batch::RecordBatch;
use futures_util::stream::BoxStream;
use serde::{Deserialize, Serialize};

pub mod clickhouse;
pub mod mysql;
pub mod postgres;
pub mod sqlite;

use crate::db::ConnectionInfo;

// ---------------------------------------------------------------------------
// Kiểu dữ liệu (§5.1)
// ---------------------------------------------------------------------------

/// Một cột trong bảng nguồn (introspect).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnInfo {
    pub name: String,
    /// Tên kiểu native của DB nguồn (vd "integer", "varchar", "REAL").
    pub data_type: String,
    pub nullable: bool,
}

/// Một bảng nguồn (introspect).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableInfo {
    pub schema: Option<String>,
    pub name: String,
    pub columns: Vec<ColumnInfo>,
    /// Ước lượng số dòng (rẻ, có thể sai) — None nếu không lấy được.
    pub row_estimate: Option<i64>,
}

/// Nguồn của một lần extract: bảng đã định danh, HOẶC câu SQL tuỳ ý.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SourceRel {
    Table {
        schema: Option<String>,
        name: String,
    },
    Query {
        sql: String,
    },
}

/// Toán tử so sánh cursor (§6.2). `Ge` = closed-range mặc định, `Gt` = strict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorOp {
    Ge,
    Gt,
}

impl CursorOp {
    fn sql(self) -> &'static str {
        match self {
            CursorOp::Ge => ">=",
            CursorOp::Gt => ">",
        }
    }
}

/// Vị từ cursor cho incremental extract: `WHERE column op from [AND column < to]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPred {
    pub column: String,
    pub op: CursorOp,
    /// Biên dưới (watermark). Đưa vào params, không nội suy chuỗi.
    pub from: serde_json::Value,
    /// Biên trên (nửa mở, `< to`) — Some khi backfill một chunk.
    pub to: Option<serde_json::Value>,
}

/// Đặc tả một lần extract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractSpec {
    pub source: SourceRel,
    /// Projection cột; None = `*`.
    pub columns: Option<Vec<String>>,
    pub cursor: Option<CursorPred>,
    /// Kích thước batch (flush RecordBatch mỗi ngần này dòng).
    pub batch_rows: usize,
}

impl ExtractSpec {
    /// Batch mặc định (§5.1). Dùng bởi runner/DSL Phase 2 (agent khác).
    #[allow(dead_code)]
    pub const DEFAULT_BATCH_ROWS: usize = 8192;
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait Connector: Send + Sync {
    /// Mở kết nối thử — trả Ok nếu nguồn sống.
    async fn test(&self) -> Result<()>;
    /// Liệt kê bảng + cột + ước lượng dòng.
    async fn introspect(&self) -> Result<Vec<TableInfo>>;
    /// Stream RecordBatch theo `spec`.
    async fn extract(&self, spec: ExtractSpec) -> Result<BoxStream<'static, Result<RecordBatch>>>;
    /// Ghi (load) các RecordBatch xuống bảng đích theo `spec`. Trả số dòng đã ghi.
    /// DB-load (§5.1/§5.2): fallback batched multi-row INSERT (KHÔNG pgpq/COPY — §2.3).
    async fn load(&self, spec: LoadSpec, batches: Vec<RecordBatch>) -> Result<u64>;
}

/// Dispatch connector theo `kind` của connection.
pub fn connector_for(info: ConnectionInfo) -> Result<Box<dyn Connector>> {
    match info.kind.as_str() {
        "postgres" | "postgresql" => Ok(Box::new(postgres::PostgresConnector::new(info.dsn))),
        "mysql" | "mariadb" => Ok(Box::new(mysql::MysqlConnector::new(info.dsn))),
        "sqlite" => Ok(Box::new(sqlite::SqliteConnector::new(info.dsn))),
        "clickhouse" => Ok(Box::new(clickhouse::ClickHouseConnector::new(info.dsn)?)),
        other => Err(anyhow::anyhow!("kind kết nối chưa hỗ trợ: {other}")),
    }
}

// ---------------------------------------------------------------------------
// Quote identifier (chống injection — §5.2 allowlist)
// ---------------------------------------------------------------------------

/// Bọc một identifier bằng dấu nháy `q`, escape bằng cách nhân đôi.
/// (Postgres/SQLite dùng `"`, MySQL dùng `` ` ``.)
pub(crate) fn quote_ident(s: &str, q: char) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push(q);
    for c in s.chars() {
        if c == q {
            out.push(q);
        }
        out.push(c);
    }
    out.push(q);
    out
}

/// Placeholder tham số theo phương ngữ. Postgres = `$1..`, MySQL/SQLite = `?`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placeholder {
    /// `$1`, `$2`, … (Postgres)
    Numbered,
    /// `?` (MySQL, SQLite)
    Question,
}

impl Placeholder {
    fn render(self, idx: usize) -> String {
        match self {
            Placeholder::Numbered => format!("${idx}"),
            Placeholder::Question => "?".to_string(),
        }
    }
}

/// Phương ngữ SQL cho việc build câu SELECT.
#[derive(Debug, Clone, Copy)]
pub struct Dialect {
    pub quote: char,
    pub placeholder: Placeholder,
}

impl Dialect {
    pub const POSTGRES: Dialect = Dialect {
        quote: '"',
        placeholder: Placeholder::Numbered,
    };
    pub const MYSQL: Dialect = Dialect {
        quote: '`',
        placeholder: Placeholder::Question,
    };
    pub const SQLITE: Dialect = Dialect {
        quote: '"',
        placeholder: Placeholder::Question,
    };
    /// ClickHouse: identifier bọc backtick; placeholder `?` (connector CH tự thay bằng
    /// literal đã escape — HTTP interface không dùng bind params như sqlx).
    pub const CLICKHOUSE: Dialect = Dialect {
        quote: '`',
        placeholder: Placeholder::Question,
    };
}

// ---------------------------------------------------------------------------
// build_select — hàm thuần, test được không cần DB
// ---------------------------------------------------------------------------

/// Dựng câu `SELECT [cols] FROM src [WHERE cursor …] [ORDER BY cursor]`.
///
/// Trả `(sql, params)` — `params` theo thứ tự bind (from, rồi to nếu có).
/// Cursor luôn kéo theo `ORDER BY <cursor> ASC` để watermark tiến đơn điệu (§6.2).
pub fn build_select(spec: &ExtractSpec, d: Dialect) -> (String, Vec<serde_json::Value>) {
    let q = d.quote;

    // Danh sách cột.
    let cols = match &spec.columns {
        Some(cs) if !cs.is_empty() => cs
            .iter()
            .map(|c| quote_ident(c, q))
            .collect::<Vec<_>>()
            .join(", "),
        _ => "*".to_string(),
    };

    // Nguồn FROM.
    let from = match &spec.source {
        SourceRel::Table { schema, name } => match schema {
            Some(s) if !s.is_empty() => {
                format!("{}.{}", quote_ident(s, q), quote_ident(name, q))
            }
            _ => quote_ident(name, q),
        },
        // Query tuỳ ý: bọc subquery để cursor/projection áp lên trên.
        SourceRel::Query { sql } => format!("({}) AS {}", sql.trim(), quote_ident("_src", q)),
    };

    let mut sql = format!("SELECT {cols} FROM {from}");
    let mut params: Vec<serde_json::Value> = Vec::new();

    if let Some(cur) = &spec.cursor {
        let col = quote_ident(&cur.column, q);
        let mut clauses = Vec::new();

        params.push(cur.from.clone());
        clauses.push(format!(
            "{col} {} {}",
            cur.op.sql(),
            d.placeholder.render(params.len())
        ));

        if let Some(to) = &cur.to {
            params.push(to.clone());
            clauses.push(format!("{col} < {}", d.placeholder.render(params.len())));
        }

        sql.push_str(" WHERE ");
        sql.push_str(&clauses.join(" AND "));
        sql.push_str(&format!(" ORDER BY {col} ASC"));
    }

    (sql, params)
}

// ---------------------------------------------------------------------------
// redact_dsn — che mật khẩu trước khi trả về client (§11)
// ---------------------------------------------------------------------------

/// Token trung gian: thay password bằng token ASCII (url KHÔNG percent-encode), sau đó
/// đổi lại thành `•••` ở output — tránh việc url mã hoá ký tự bullet.
const REDACT_TOKEN: &str = "SENCLAWREDACTEDPW";

/// Một query key có phải chứa mật khẩu không (password, sslpassword...).
fn is_password_key(k: &str) -> bool {
    k.to_ascii_lowercase().contains("password")
}

/// Che mật khẩu trong DSN — CÓ CẤU TRÚC (parse bằng `url`): che password trong userinfo
/// (`user:pass@`) LẪN mọi query param chứa "password" (vd `?password=`, `?sslpassword=`;
/// sqlx-postgres chấp nhận cả hai). Split tay theo '@'/':' đầu tiên rò password chứa '@'
/// hoặc password ở query — nên dùng parser. DSN không có gì để che → trả NGUYÊN VĂN input
/// (không đụng chuỗi). DSN không parse được như URL (vd keyword DSN) → che token password.
pub fn redact_dsn(dsn: &str) -> String {
    match url::Url::parse(dsn) {
        Ok(url) => {
            let has_pw = url.password().map(|p| !p.is_empty()).unwrap_or(false);
            let has_pw_query = url.query_pairs().any(|(k, _)| is_password_key(&k));
            if !has_pw && !has_pw_query {
                // Không có bí mật → giữ nguyên byte-for-byte (không để url normalize).
                return dsn.to_string();
            }
            let mut redacted = url.clone();
            if has_pw {
                let _ = redacted.set_password(Some(REDACT_TOKEN));
            }
            if has_pw_query {
                let pairs: Vec<(String, String)> = url
                    .query_pairs()
                    .map(|(k, v)| {
                        if is_password_key(&k) {
                            (k.into_owned(), REDACT_TOKEN.to_string())
                        } else {
                            (k.into_owned(), v.into_owned())
                        }
                    })
                    .collect();
                redacted
                    .query_pairs_mut()
                    .clear()
                    .extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
            }
            redacted.to_string().replace(REDACT_TOKEN, "•••")
        }
        // Không phải URL DSN (keyword DSN `host=.. password=..`, đường dẫn) → che token
        // password nếu có, còn lại giữ nguyên.
        Err(_) => redact_keyword_dsn(dsn),
    }
}

/// Che value của token `password`/`sslpassword` trong DSN dạng keyword (`k=v` phân tách
/// bởi khoảng trắng/`;`/`&`). Không có token mật khẩu → trả nguyên văn.
fn redact_keyword_dsn(dsn: &str) -> String {
    if !dsn.to_ascii_lowercase().contains("password") {
        return dsn.to_string();
    }
    dsn.split_inclusive([' ', ';', '&'])
        .map(|tok| {
            let (body, sep) = match tok.chars().last() {
                Some(c @ (' ' | ';' | '&')) => (
                    &tok[..tok.len() - c.len_utf8()],
                    &tok[tok.len() - c.len_utf8()..],
                ),
                _ => (tok, ""),
            };
            match body.split_once('=') {
                Some((k, _v)) if is_password_key(k.trim()) => format!("{k}=•••{sep}"),
                _ => tok.to_string(),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Load (§5.1) — LoadSpec/LoadMode + Arrow→DDL + INSERT builder + Cell
// ---------------------------------------------------------------------------

/// Chế độ ghi bảng đích (§5.1). Parse từ `ExportStep.mode` + `keys`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadMode {
    /// Xoá sạch bảng rồi ghi lại (TRUNCATE/DELETE + INSERT trong 1 txn).
    FullRefresh,
    /// Chỉ chèn thêm.
    Append,
    /// Chèn hoặc cập nhật theo khoá (PG/SQLite: ON CONFLICT; MySQL: ON DUPLICATE KEY).
    Upsert { keys: Vec<String> },
}

impl LoadMode {
    /// Parse từ DSL: "full_refresh" | "append" | "upsert" (upsert đòi keys không rỗng).
    pub fn from_export(mode: &str, keys: Vec<String>) -> Result<LoadMode> {
        match mode {
            "full_refresh" => Ok(LoadMode::FullRefresh),
            "append" => Ok(LoadMode::Append),
            "upsert" => {
                if keys.is_empty() {
                    Err(anyhow::anyhow!("mode 'upsert' cần 'keys' không rỗng"))
                } else {
                    Ok(LoadMode::Upsert { keys })
                }
            }
            other => Err(anyhow::anyhow!("mode export không hỗ trợ: {other}")),
        }
    }
}

/// Đặc tả một lần load (§5.1). `table` có thể là "schema.name".
#[derive(Debug, Clone)]
pub struct LoadSpec {
    pub table: String,
    pub mode: LoadMode,
    pub create_if_missing: bool,
}

/// Phương ngữ DB đích cho việc dựng DDL/INSERT (khác `Dialect` ở chỗ mang type-map).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadFlavor {
    Postgres,
    Mysql,
    Sqlite,
}

impl LoadFlavor {
    pub fn dialect(self) -> Dialect {
        match self {
            LoadFlavor::Postgres => Dialect::POSTGRES,
            LoadFlavor::Mysql => Dialect::MYSQL,
            LoadFlavor::Sqlite => Dialect::SQLITE,
        }
    }

    /// Arrow DataType → kiểu cột DDL đích (bảng §5.1, chiều ngược của extract).
    pub fn ddl_type(self, dt: &DataType) -> String {
        use LoadFlavor::*;
        match dt {
            DataType::Boolean => "BOOLEAN".into(),
            // 32-bit trở xuống → INTEGER; 64-bit/unsigned lớn → BIGINT (an toàn phạm vi).
            DataType::Int8
            | DataType::Int16
            | DataType::Int32
            | DataType::UInt8
            | DataType::UInt16 => "INTEGER".into(),
            DataType::Int64 | DataType::UInt32 | DataType::UInt64 => "BIGINT".into(),
            DataType::Float16 | DataType::Float32 | DataType::Float64 => match self {
                Postgres => "DOUBLE PRECISION".into(),
                Mysql => "DOUBLE".into(),
                Sqlite => "REAL".into(),
            },
            DataType::Utf8 | DataType::LargeUtf8 => "TEXT".into(),
            DataType::Binary | DataType::LargeBinary | DataType::FixedSizeBinary(_) => match self {
                Postgres => "BYTEA".into(),
                _ => "BLOB".into(),
            },
            DataType::Date32 | DataType::Date64 => "DATE".into(),
            DataType::Timestamp(_, _) => match self {
                Postgres | Sqlite => "TIMESTAMP".into(),
                Mysql => "DATETIME".into(),
            },
            DataType::Decimal128(p, s) | DataType::Decimal256(p, s) => match self {
                Mysql => format!("DECIMAL({p},{s})"),
                _ => format!("NUMERIC({p},{s})"),
            },
            // Nested (List/Struct/Map) → JSON/TEXT (serialize khi ghi).
            DataType::List(_)
            | DataType::LargeList(_)
            | DataType::Struct(_)
            | DataType::Map(_, _) => match self {
                Postgres => "JSONB".into(),
                Mysql => "JSON".into(),
                Sqlite => "TEXT".into(),
            },
            // Còn lại → TEXT (ghi dạng chuỗi hiển thị).
            _ => "TEXT".into(),
        }
    }
}

/// Quote một tên bảng có thể chứa schema ("schema.name" → "schema"."name").
pub fn quote_qualified(table: &str, q: char) -> String {
    table
        .split('.')
        .map(|p| quote_ident(p, q))
        .collect::<Vec<_>>()
        .join(".")
}

/// `CREATE TABLE IF NOT EXISTS <table> (<col> <ddl-type>, …)` — hàm thuần, test được.
pub fn build_create_table(flavor: LoadFlavor, table: &str, schema: &SchemaRef) -> String {
    let q = flavor.dialect().quote;
    let cols = schema
        .fields()
        .iter()
        .map(|f| {
            format!(
                "{} {}",
                quote_ident(f.name(), q),
                flavor.ddl_type(f.data_type())
            )
        })
        .collect::<Vec<_>>()
        .join(",\n  ");
    format!(
        "CREATE TABLE IF NOT EXISTS {} (\n  {}\n)",
        quote_qualified(table, q),
        cols
    )
}

/// Dựng một câu INSERT nhiều dòng (`VALUES (...),(...)`). `upsert` = Some(keys) → thêm
/// mệnh đề ON CONFLICT (PG/SQLite) hoặc ON DUPLICATE KEY (MySQL). Hàm thuần, test được.
///
/// Placeholder: PG đánh số `$1..` tuần tự trên cả câu; MySQL/SQLite dùng `?`.
pub fn build_insert_sql(
    flavor: LoadFlavor,
    table: &str,
    cols: &[String],
    nrows: usize,
    upsert: Option<&[String]>,
) -> String {
    let d = flavor.dialect();
    let q = d.quote;
    let cols_sql = cols
        .iter()
        .map(|c| quote_ident(c, q))
        .collect::<Vec<_>>()
        .join(", ");

    // Tuple placeholder theo từng dòng.
    let mut idx = 0usize;
    let mut tuples = Vec::with_capacity(nrows);
    for _ in 0..nrows {
        let mut row = Vec::with_capacity(cols.len());
        for _ in cols {
            idx += 1;
            row.push(d.placeholder.render(idx));
        }
        tuples.push(format!("({})", row.join(", ")));
    }

    let mut sql = format!(
        "INSERT INTO {} ({}) VALUES {}",
        quote_qualified(table, q),
        cols_sql,
        tuples.join(", ")
    );

    if let Some(keys) = upsert {
        let non_key: Vec<&String> = cols.iter().filter(|c| !keys.contains(c)).collect();
        match flavor {
            LoadFlavor::Postgres | LoadFlavor::Sqlite => {
                let key_sql = keys
                    .iter()
                    .map(|c| quote_ident(c, q))
                    .collect::<Vec<_>>()
                    .join(", ");
                if non_key.is_empty() {
                    // Toàn bộ cột là khoá → không có gì để cập nhật.
                    sql.push_str(&format!(" ON CONFLICT ({key_sql}) DO NOTHING"));
                } else {
                    let sets = non_key
                        .iter()
                        .map(|c| {
                            let qc = quote_ident(c, q);
                            format!("{qc} = EXCLUDED.{qc}")
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    sql.push_str(&format!(" ON CONFLICT ({key_sql}) DO UPDATE SET {sets}"));
                }
            }
            LoadFlavor::Mysql => {
                // MySQL không nêu tên khoá trong ON DUPLICATE KEY (dùng PK/unique có sẵn).
                if non_key.is_empty() {
                    // No-op update để câu vẫn hợp lệ (cột khoá gán chính nó).
                    let k = quote_ident(&keys[0], q);
                    sql.push_str(&format!(" ON DUPLICATE KEY UPDATE {k} = {k}"));
                } else {
                    let sets = non_key
                        .iter()
                        .map(|c| {
                            let qc = quote_ident(c, q);
                            format!("{qc} = VALUES({qc})")
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    sql.push_str(&format!(" ON DUPLICATE KEY UPDATE {sets}"));
                }
            }
        }
    }

    sql
}

/// Trần số placeholder / câu INSERT (PG cứng 65535 params; chọn 60000 an toàn). SQLite
/// mặc định thấp hơn (999/32766 tuỳ bản) → connector SQLite tự hạ trần riêng.
pub const PG_MAX_PARAMS: usize = 60000;

/// Số dòng mỗi chunk INSERT sao cho `nrows*ncols ≤ max_params`.
pub fn chunk_rows(ncols: usize, max_params: usize) -> usize {
    (max_params / ncols.max(1)).max(1)
}

/// Giá trị một ô đã chuẩn hoá (trung gian giữa Arrow và driver). Bind vào rusqlite/sqlx.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Null,
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
    Bytes(Vec<u8>),
}

impl Cell {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Cell::Int(i) => Some(*i),
            Cell::Bool(b) => Some(*b as i64),
            Cell::Float(f) => Some(*f as i64),
            _ => None,
        }
    }
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Cell::Float(f) => Some(*f),
            Cell::Int(i) => Some(*i as f64),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Cell::Bool(b) => Some(*b),
            Cell::Int(i) => Some(*i != 0),
            _ => None,
        }
    }
    pub fn as_text(&self) -> Option<String> {
        match self {
            Cell::Null => None,
            Cell::Text(s) => Some(s.clone()),
            Cell::Int(i) => Some(i.to_string()),
            Cell::Float(f) => Some(f.to_string()),
            Cell::Bool(b) => Some(b.to_string()),
            Cell::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
        }
    }
    pub fn as_bytes(&self) -> Option<Vec<u8>> {
        match self {
            Cell::Bytes(b) => Some(b.clone()),
            Cell::Text(s) => Some(s.clone().into_bytes()),
            _ => None,
        }
    }
}

/// Trích toàn bộ một cột Arrow thành `Vec<Cell>` (theo đúng kiểu cột). Kiểu không phải
/// primitive (Date/Timestamp/Decimal/nested…) → cast về Utf8, ghi dạng chuỗi.
pub fn column_cells(arr: &ArrayRef) -> Vec<Cell> {
    let n = arr.len();
    macro_rules! ints {
        ($ty:ty) => {{
            let a = arr.as_any().downcast_ref::<$ty>().unwrap();
            (0..n)
                .map(|i| {
                    if a.is_null(i) {
                        Cell::Null
                    } else {
                        Cell::Int(a.value(i) as i64)
                    }
                })
                .collect()
        }};
    }
    match arr.data_type() {
        DataType::Boolean => {
            let a = arr.as_any().downcast_ref::<BooleanArray>().unwrap();
            (0..n)
                .map(|i| {
                    if a.is_null(i) {
                        Cell::Null
                    } else {
                        Cell::Bool(a.value(i))
                    }
                })
                .collect()
        }
        DataType::Int8 => ints!(Int8Array),
        DataType::Int16 => ints!(Int16Array),
        DataType::Int32 => ints!(Int32Array),
        DataType::Int64 => ints!(Int64Array),
        DataType::UInt8 => ints!(UInt8Array),
        DataType::UInt16 => ints!(UInt16Array),
        DataType::UInt32 => ints!(UInt32Array),
        DataType::UInt64 => ints!(UInt64Array),
        DataType::Float32 => {
            let a = arr.as_any().downcast_ref::<Float32Array>().unwrap();
            (0..n)
                .map(|i| {
                    if a.is_null(i) {
                        Cell::Null
                    } else {
                        Cell::Float(a.value(i) as f64)
                    }
                })
                .collect()
        }
        DataType::Float64 => {
            let a = arr.as_any().downcast_ref::<Float64Array>().unwrap();
            (0..n)
                .map(|i| {
                    if a.is_null(i) {
                        Cell::Null
                    } else {
                        Cell::Float(a.value(i))
                    }
                })
                .collect()
        }
        DataType::Utf8 => {
            let a = arr.as_any().downcast_ref::<StringArray>().unwrap();
            (0..n)
                .map(|i| {
                    if a.is_null(i) {
                        Cell::Null
                    } else {
                        Cell::Text(a.value(i).to_string())
                    }
                })
                .collect()
        }
        DataType::LargeUtf8 => {
            let a = arr.as_any().downcast_ref::<LargeStringArray>().unwrap();
            (0..n)
                .map(|i| {
                    if a.is_null(i) {
                        Cell::Null
                    } else {
                        Cell::Text(a.value(i).to_string())
                    }
                })
                .collect()
        }
        DataType::Binary => {
            let a = arr.as_any().downcast_ref::<BinaryArray>().unwrap();
            (0..n)
                .map(|i| {
                    if a.is_null(i) {
                        Cell::Null
                    } else {
                        Cell::Bytes(a.value(i).to_vec())
                    }
                })
                .collect()
        }
        DataType::LargeBinary => {
            let a = arr.as_any().downcast_ref::<LargeBinaryArray>().unwrap();
            (0..n)
                .map(|i| {
                    if a.is_null(i) {
                        Cell::Null
                    } else {
                        Cell::Bytes(a.value(i).to_vec())
                    }
                })
                .collect()
        }
        // Date/Timestamp/Decimal/nested… → cast Utf8, ghi chuỗi.
        _ => match cast(arr, &DataType::Utf8) {
            Ok(s) => {
                let a = s.as_any().downcast_ref::<StringArray>();
                (0..n)
                    .map(|i| match a {
                        Some(a) if !a.is_null(i) => Cell::Text(a.value(i).to_string()),
                        _ => Cell::Null,
                    })
                    .collect()
            }
            Err(_) => (0..n).map(|_| Cell::Null).collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn table_spec() -> ExtractSpec {
        ExtractSpec {
            source: SourceRel::Table {
                schema: Some("public".into()),
                name: "orders".into(),
            },
            columns: None,
            cursor: None,
            batch_rows: 100,
        }
    }

    #[test]
    fn select_table_all_cols_pg() {
        let (sql, params) = build_select(&table_spec(), Dialect::POSTGRES);
        assert_eq!(sql, r#"SELECT * FROM "public"."orders""#);
        assert!(params.is_empty());
    }

    #[test]
    fn select_table_no_schema_mysql() {
        let spec = ExtractSpec {
            source: SourceRel::Table {
                schema: None,
                name: "orders".into(),
            },
            columns: Some(vec!["id".into(), "amount".into()]),
            cursor: None,
            batch_rows: 100,
        };
        let (sql, params) = build_select(&spec, Dialect::MYSQL);
        assert_eq!(sql, "SELECT `id`, `amount` FROM `orders`");
        assert!(params.is_empty());
    }

    #[test]
    fn select_cursor_ge_no_to() {
        let mut spec = table_spec();
        spec.cursor = Some(CursorPred {
            column: "updated_at".into(),
            op: CursorOp::Ge,
            from: json!("2024-01-01"),
            to: None,
        });
        let (sql, params) = build_select(&spec, Dialect::POSTGRES);
        assert_eq!(
            sql,
            r#"SELECT * FROM "public"."orders" WHERE "updated_at" >= $1 ORDER BY "updated_at" ASC"#
        );
        assert_eq!(params, vec![json!("2024-01-01")]);
    }

    #[test]
    fn select_cursor_gt_with_to_closed_range() {
        let mut spec = table_spec();
        spec.cursor = Some(CursorPred {
            column: "id".into(),
            op: CursorOp::Gt,
            from: json!(100),
            to: Some(json!(200)),
        });
        let (sql, params) = build_select(&spec, Dialect::POSTGRES);
        assert_eq!(
            sql,
            r#"SELECT * FROM "public"."orders" WHERE "id" > $1 AND "id" < $2 ORDER BY "id" ASC"#
        );
        assert_eq!(params, vec![json!(100), json!(200)]);
    }

    #[test]
    fn select_cursor_mysql_uses_question_marks() {
        let spec = ExtractSpec {
            source: SourceRel::Table {
                schema: None,
                name: "t".into(),
            },
            columns: None,
            cursor: Some(CursorPred {
                column: "ts".into(),
                op: CursorOp::Ge,
                from: json!(1),
                to: Some(json!(2)),
            }),
            batch_rows: 10,
        };
        let (sql, params) = build_select(&spec, Dialect::MYSQL);
        assert_eq!(
            sql,
            "SELECT * FROM `t` WHERE `ts` >= ? AND `ts` < ? ORDER BY `ts` ASC"
        );
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn select_query_source_wrapped() {
        let spec = ExtractSpec {
            source: SourceRel::Query {
                sql: "SELECT a, b FROM foo".into(),
            },
            columns: Some(vec!["a".into()]),
            cursor: None,
            batch_rows: 10,
        };
        let (sql, _) = build_select(&spec, Dialect::SQLITE);
        assert_eq!(sql, r#"SELECT "a" FROM (SELECT a, b FROM foo) AS "_src""#);
    }

    #[test]
    fn quote_ident_escapes_embedded_quote() {
        assert_eq!(quote_ident(r#"we"ird"#, '"'), r#""we""ird""#);
        assert_eq!(quote_ident("a`b", '`'), "`a``b`");
    }

    #[test]
    fn redact_hides_password() {
        assert_eq!(
            redact_dsn("postgres://user:secret@host:5432/db"),
            "postgres://user:•••@host:5432/db"
        );
        assert_eq!(
            redact_dsn("mysql://root:p%40ss@127.0.0.1/app"),
            "mysql://root:•••@127.0.0.1/app"
        );
    }

    #[test]
    fn redact_no_password_untouched() {
        assert_eq!(
            redact_dsn("postgres://user@host/db"),
            "postgres://user@host/db"
        );
        assert_eq!(
            redact_dsn("sqlite:///tmp/x.sqlite"),
            "sqlite:///tmp/x.sqlite"
        );
        assert_eq!(redact_dsn("/tmp/plain.sqlite"), "/tmp/plain.sqlite");
    }

    #[test]
    fn redact_query_password_param() {
        // BUG: password ở query-param bị trả nguyên văn (sqlx-postgres chấp nhận).
        let r = redact_dsn("postgres://user@host:5432/db?password=secret&sslmode=require");
        assert!(!r.contains("secret"), "password query phải bị che: {r}");
        assert!(r.contains("•••"), "có token che: {r}");
        assert!(r.contains("sslmode=require"), "param khác giữ nguyên: {r}");

        // sslpassword cũng là bí mật.
        let r2 = redact_dsn("postgres://user@host/db?sslpassword=x&application_name=app");
        assert!(
            !r2.contains("sslpassword=x"),
            "sslpassword phải bị che: {r2}"
        );
        assert!(r2.contains("application_name=app"), "param khác giữ: {r2}");
    }

    #[test]
    fn redact_password_with_at_sign_no_leak() {
        // BUG: split '@' đầu tiên rò đuôi password (`ss` từ `p@ss`).
        let r = redact_dsn("postgres://user:p@ss@host/db");
        assert!(!r.contains("ss"), "không rò đuôi password chứa '@': {r}");
        assert!(r.contains("•••"), "password bị che: {r}");
        assert!(r.contains("@host/db"), "host giữ nguyên: {r}");
    }

    #[test]
    fn redact_keyword_dsn_password() {
        // DSN keyword không parse được như URL → vẫn phải che token password.
        let r = redact_dsn("host=localhost port=5432 password=secret dbname=app");
        assert!(!r.contains("secret"), "keyword password phải bị che: {r}");
        assert!(r.contains("host=localhost"), "token khác giữ nguyên: {r}");
        assert!(r.contains("dbname=app"), "token cuối giữ nguyên: {r}");
    }

    // ---- Load: Arrow→DDL type map (§5.1) ----

    #[test]
    fn ddl_type_map_covers_core_types() {
        use datafusion::arrow::datatypes::TimeUnit;
        let pg = LoadFlavor::Postgres;
        let my = LoadFlavor::Mysql;
        let lt = LoadFlavor::Sqlite;

        assert_eq!(pg.ddl_type(&DataType::Utf8), "TEXT");
        assert_eq!(pg.ddl_type(&DataType::Int32), "INTEGER");
        assert_eq!(pg.ddl_type(&DataType::Int64), "BIGINT");
        assert_eq!(pg.ddl_type(&DataType::Float64), "DOUBLE PRECISION");
        assert_eq!(my.ddl_type(&DataType::Float64), "DOUBLE");
        assert_eq!(lt.ddl_type(&DataType::Float64), "REAL");
        assert_eq!(pg.ddl_type(&DataType::Boolean), "BOOLEAN");
        assert_eq!(pg.ddl_type(&DataType::Date32), "DATE");
        assert_eq!(
            pg.ddl_type(&DataType::Timestamp(TimeUnit::Microsecond, None)),
            "TIMESTAMP"
        );
        assert_eq!(
            my.ddl_type(&DataType::Timestamp(TimeUnit::Microsecond, None)),
            "DATETIME"
        );
        assert_eq!(pg.ddl_type(&DataType::Decimal128(10, 2)), "NUMERIC(10,2)");
        assert_eq!(my.ddl_type(&DataType::Decimal128(10, 2)), "DECIMAL(10,2)");
        assert_eq!(pg.ddl_type(&DataType::Binary), "BYTEA");
        assert_eq!(my.ddl_type(&DataType::Binary), "BLOB");
        assert_eq!(lt.ddl_type(&DataType::Binary), "BLOB");
    }

    #[test]
    fn create_table_ddl_shape() {
        use datafusion::arrow::datatypes::{Field, Schema};
        use std::sync::Arc;
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("name", DataType::Utf8, true),
        ]));
        let ddl = build_create_table(LoadFlavor::Postgres, "public.dest", &schema);
        assert!(
            ddl.starts_with("CREATE TABLE IF NOT EXISTS \"public\".\"dest\" ("),
            "{ddl}"
        );
        assert!(ddl.contains("\"id\" BIGINT"), "{ddl}");
        assert!(ddl.contains("\"name\" TEXT"), "{ddl}");
    }

    // ---- Load: INSERT builder (§5.2 fallback batched multi-row) ----

    #[test]
    fn insert_pg_numbered_placeholders() {
        let cols = vec!["id".to_string(), "v".to_string()];
        let sql = build_insert_sql(LoadFlavor::Postgres, "t", &cols, 2, None);
        assert_eq!(
            sql,
            r#"INSERT INTO "t" ("id", "v") VALUES ($1, $2), ($3, $4)"#
        );
    }

    #[test]
    fn insert_mysql_question_placeholders() {
        let cols = vec!["id".to_string(), "v".to_string()];
        let sql = build_insert_sql(LoadFlavor::Mysql, "t", &cols, 2, None);
        assert_eq!(sql, "INSERT INTO `t` (`id`, `v`) VALUES (?, ?), (?, ?)");
    }

    #[test]
    fn insert_pg_upsert_on_conflict() {
        let cols = vec!["id".to_string(), "v".to_string(), "w".to_string()];
        let keys = vec!["id".to_string()];
        let sql = build_insert_sql(LoadFlavor::Postgres, "t", &cols, 1, Some(&keys));
        assert!(
            sql.ends_with(
                r#"ON CONFLICT ("id") DO UPDATE SET "v" = EXCLUDED."v", "w" = EXCLUDED."w""#
            ),
            "{sql}"
        );
    }

    #[test]
    fn insert_sqlite_upsert_on_conflict() {
        let cols = vec!["id".to_string(), "v".to_string()];
        let keys = vec!["id".to_string()];
        let sql = build_insert_sql(LoadFlavor::Sqlite, "t", &cols, 1, Some(&keys));
        assert!(
            sql.ends_with(r#"ON CONFLICT ("id") DO UPDATE SET "v" = EXCLUDED."v""#),
            "{sql}"
        );
    }

    #[test]
    fn insert_mysql_upsert_on_duplicate() {
        let cols = vec!["id".to_string(), "v".to_string()];
        let keys = vec!["id".to_string()];
        let sql = build_insert_sql(LoadFlavor::Mysql, "t", &cols, 1, Some(&keys));
        assert!(
            sql.ends_with("ON DUPLICATE KEY UPDATE `v` = VALUES(`v`)"),
            "{sql}"
        );
    }

    #[test]
    fn insert_upsert_all_keys_no_update() {
        let cols = vec!["id".to_string(), "k".to_string()];
        let keys = vec!["id".to_string(), "k".to_string()];
        let pg = build_insert_sql(LoadFlavor::Postgres, "t", &cols, 1, Some(&keys));
        assert!(
            pg.ends_with(r#"ON CONFLICT ("id", "k") DO NOTHING"#),
            "{pg}"
        );
        let my = build_insert_sql(LoadFlavor::Mysql, "t", &cols, 1, Some(&keys));
        assert!(my.ends_with("ON DUPLICATE KEY UPDATE `id` = `id`"), "{my}");
    }

    #[test]
    fn chunk_rows_respects_param_ceiling() {
        // 3 cột, trần 60000 → 20000 dòng/chunk.
        assert_eq!(chunk_rows(3, PG_MAX_PARAMS), 20000);
        // Không bao giờ 0 dòng.
        assert_eq!(chunk_rows(100000, 10), 1);
    }

    #[test]
    fn load_mode_parse() {
        assert_eq!(
            LoadMode::from_export("full_refresh", vec![]).unwrap(),
            LoadMode::FullRefresh
        );
        assert_eq!(
            LoadMode::from_export("append", vec![]).unwrap(),
            LoadMode::Append
        );
        assert_eq!(
            LoadMode::from_export("upsert", vec!["id".into()]).unwrap(),
            LoadMode::Upsert {
                keys: vec!["id".into()]
            }
        );
        // upsert thiếu keys → lỗi.
        assert!(LoadMode::from_export("upsert", vec![]).is_err());
        assert!(LoadMode::from_export("bogus", vec![]).is_err());
    }

    #[test]
    fn column_cells_extract_types() {
        use datafusion::arrow::array::{Float64Array, Int64Array, StringArray};
        use std::sync::Arc;
        let ints: ArrayRef = Arc::new(Int64Array::from(vec![Some(1), None, Some(3)]));
        assert_eq!(
            column_cells(&ints),
            vec![Cell::Int(1), Cell::Null, Cell::Int(3)]
        );
        let fs: ArrayRef = Arc::new(Float64Array::from(vec![Some(1.5)]));
        assert_eq!(column_cells(&fs), vec![Cell::Float(1.5)]);
        let ss: ArrayRef = Arc::new(StringArray::from(vec![Some("x"), None]));
        assert_eq!(column_cells(&ss), vec![Cell::Text("x".into()), Cell::Null]);
    }
}
