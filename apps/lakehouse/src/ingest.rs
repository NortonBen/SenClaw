//! Stage File — sniff + normalize bất kỳ file nào thành Arrow RecordBatches
//! (docs/data-lake-app-design.md §5.2 dòng "File" + §9 `lake_import_file`).
//!
//! Đây là bản THU HẸP của sniffer `apps/ontology/src/ingest.rs`: ontology
//! normalize về *text* (CSV/JSON) để LLM lift; ở đây đích đến là **Arrow trực
//! tiếp** — vào thẳng lake dưới dạng Parquet, không có tầng LLM ở giữa. Vì vậy
//! chỉ nhận các định dạng *có cấu trúc bảng*:
//!
//!   * CSV / TSV / PSV (delimiter tự dò, quote-aware);
//!   * JSON (array-of-objects, hoặc object bọc một array — chọn `best_array`);
//!   * NDJSON / JSON Lines;
//!   * Excel (.xlsx/.xls) — mỗi sheet một nguồn, date serial → ISO;
//!   * Parquet passthrough (đọc lại RecordBatch nguyên si).
//!
//! CỐ Ý KHÔNG nhận PDF/HTML/YAML/Markdown/docx: chúng là "văn bản tự do" —
//! địa hạt của app ontology, không phải data lake.
//!
//! Quy tắc load-bearing:
//!   * **Magic bytes trước, phần mở rộng sau** — một `.txt` chứa JSON vẫn phải
//!     ra bảng; một `.csv` thực chất là Parquet (đổi tên) vẫn phải đọc đúng.
//!   * **Suy kiểu per cột theo thứ tự ưu tiên** bool → int64 → float64 → date32 →
//!     timestamp(µs) → utf8; cột rỗng = utf8 nullable. Ô thiếu/trống = null.
//!   * **Cắt chuỗi trên char boundary** — không `&s[..n]` (đa byte panic).

// Ingest được engine/runner gọi ở stage sau; giữ allow tới khi Phase 2 wire.
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};
use std::sync::Arc;

use datafusion::arrow::array::{
    ArrayRef, BooleanArray, Date32Array, Float64Array, Int64Array, StringArray,
    TimestampMicrosecondArray,
};
use datafusion::arrow::datatypes::{DataType, Field, Schema, SchemaRef, TimeUnit};
use datafusion::arrow::record_batch::RecordBatch;

/// Một nguồn đã normalize, sẵn sàng ghi vào lake. Một file có thể sinh nhiều
/// (workbook Excel → mỗi sheet một cái).
pub struct IngestedTable {
    /// Tên logic gợi ý (stem file, kèm tên sheet nếu có).
    pub name: String,
    pub schema: SchemaRef,
    pub batches: Vec<RecordBatch>,
    /// Định dạng thực dò được, cho provenance: "csv"|"tsv"|"psv"|"json"|
    /// "ndjson"|"xlsx"|"xls"|"parquet".
    pub origin: &'static str,
    /// Sniffer đã làm gì, một câu người đọc được.
    pub note: String,
    pub rows: usize,
}

// ---------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------

/// Sniff `bytes` và normalize. `filename` chỉ là gợi ý (để đặt tên) — magic
/// bytes và cấu trúc mới quyết định định dạng.
pub fn ingest(filename: &str, bytes: &[u8]) -> Result<Vec<IngestedTable>> {
    if bytes.is_empty() {
        return Err(anyhow!("file rỗng — không có gì để nạp"));
    }
    let stem = file_stem(filename);

    // --- container nhị phân, nhận diện bằng magic bytes ---
    // ZIP (xlsx/xlsm) và OLE2 (xls) đều do calamine đọc qua auto-detect.
    if bytes.starts_with(b"PK\x03\x04") {
        return from_spreadsheet(&stem, bytes, "xlsx");
    }
    if bytes.starts_with(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1") {
        return from_spreadsheet(&stem, bytes, "xls");
    }
    if bytes.starts_with(b"PAR1") {
        return Ok(vec![from_parquet(&stem, bytes)?]);
    }

    // --- còn lại là text; *cấu trúc* quyết định ---
    let text = decode_text(bytes);
    let head = text.trim_start();
    if head.is_empty() {
        return Err(anyhow!("file rỗng — không có gì để nạp"));
    }

    // JSON nguyên khối (array-of-objects hoặc object bọc array) trước.
    if head.starts_with('{') || head.starts_with('[') {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            return Ok(vec![from_json_value(&stem, "json", v)?]);
        }
        // Không parse nguyên khối được → thử NDJSON (nhiều value mỗi dòng).
        if let Some(t) = from_ndjson(&stem, &text)? {
            return Ok(vec![t]);
        }
    }

    // Delimited text (, \t ; |), tự dò.
    if let Some((delim, origin, _cols)) = sniff_delimiter(&text) {
        return Ok(vec![from_delimited(&stem, &text, delim, origin)?]);
    }

    // Không dò ra delimiter nào: có thể là bảng một cột (header + list giá trị).
    // Chỉ nhận khi có ≥2 dòng không rỗng (header + ≥1 dữ liệu) — một dòng đơn là
    // văn bản tự do, thuộc app khác.
    if let Some(t) = from_single_column(&stem, &text) {
        return Ok(vec![t]);
    }

    Err(anyhow!(
        "không nhận ra cấu trúc bảng — hỗ trợ CSV/TSV/JSON/NDJSON/Excel/Parquet"
    ))
}

// ---------------------------------------------------------------------------
// JSON / NDJSON
// ---------------------------------------------------------------------------

/// JSON value bất kỳ → bảng: tìm array "giống danh sách bản ghi" nhất bên trong,
/// flatten từng phần tử. Object trần → một dòng.
fn from_json_value(name: &str, origin: &'static str, v: Value) -> Result<IngestedTable> {
    let (rows, note) = match best_array(&v, 0) {
        Some((path, arr)) => {
            let rows: Vec<Map<String, Value>> = arr.iter().map(flatten_row).collect();
            let note = if path.is_empty() {
                format!("{origin}: array {} bản ghi, đã flatten", rows.len())
            } else {
                format!("{origin}: {} bản ghi từ '{path}', đã flatten", rows.len())
            };
            (rows, note)
        }
        None => (
            vec![flatten_row(&v)],
            format!("{origin}: một bản ghi, đã flatten"),
        ),
    };
    table_from_json_rows(name, origin, rows, note)
}

/// JSON Lines / NDJSON: một JSON value mỗi dòng. Trả `None` nếu không phải
/// NDJSON hợp lệ (để caller thử đường khác).
fn from_ndjson(name: &str, text: &str) -> Result<Option<IngestedTable>> {
    let mut rows = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(l) {
            Ok(v) => rows.push(flatten_row(&v)),
            // Một dòng không phải JSON → đây không phải NDJSON.
            Err(_) => return Ok(None),
        }
    }
    if rows.is_empty() {
        return Ok(None);
    }
    let note = format!("ndjson: {} bản ghi, đã flatten", rows.len());
    Ok(Some(table_from_json_rows(name, "ndjson", rows, note)?))
}

/// Depth-first tìm array giống "danh sách bản ghi" nhất: nhiều phần tử object
/// nhất, path nông hơn khi hòa (port từ ontology).
fn best_array(v: &Value, depth: usize) -> Option<(String, &Vec<Value>)> {
    if depth > 6 {
        return None;
    }
    let mut best: Option<(String, &Vec<Value>, usize, usize)> = None;
    let mut candidates: Vec<(String, &Vec<Value>, usize)> = Vec::new();
    match v {
        Value::Array(arr) => candidates.push((String::new(), arr, depth)),
        Value::Object(o) => {
            for (k, child) in o {
                match child {
                    Value::Array(arr) => candidates.push((k.clone(), arr, depth)),
                    Value::Object(_) => {
                        if let Some((p, arr)) = best_array(child, depth + 1) {
                            let path = if p.is_empty() {
                                k.clone()
                            } else {
                                format!("{k}.{p}")
                            };
                            candidates.push((path, arr, depth + 1));
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    for (path, arr, d) in candidates {
        let objects = arr.iter().filter(|x| x.is_object()).count();
        if objects == 0 {
            continue;
        }
        let better = match &best {
            None => true,
            Some((_, _, bs, bd)) => objects > *bs || (objects == *bs && d < *bd),
        };
        if better {
            best = Some((path, arr, objects, d));
        }
    }
    best.map(|(p, a, _, _)| (p, a))
}

/// Số phần tử của array-of-object được cấp cột riêng. Quá ngưỡng chỉ giữ
/// `.count` — một danh sách con lặp lại thực chất là thực thể thứ hai (§ontology).
const ARRAY_FANOUT: usize = 3;
/// Trần cột sau flatten (chống blow-up từ JSON bệnh lý).
const MAX_COLS: usize = 300;

/// Flatten một bản ghi thành `col -> scalar`. Object lồng → dotted path; array
/// scalar → join "; "; array-of-object → fanout 3 phần tử + cột `.count`.
fn flatten_row(v: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    flatten_into("", v, &mut out, 0);
    if out.is_empty() {
        out.insert("value".into(), scalar_string(v));
    }
    out
}

fn flatten_into(prefix: &str, v: &Value, out: &mut Map<String, Value>, depth: usize) {
    if out.len() >= MAX_COLS {
        return;
    }
    let key = |k: &str| {
        if prefix.is_empty() {
            k.to_string()
        } else {
            format!("{prefix}.{k}")
        }
    };
    match v {
        Value::Object(o) => {
            if depth > 6 {
                out.insert(prefix.to_string(), Value::String(v.to_string()));
                return;
            }
            for (k, child) in o {
                flatten_into(&key(k), child, out, depth + 1);
            }
        }
        Value::Array(arr) => {
            if arr.iter().all(|x| !x.is_object() && !x.is_array()) {
                let joined = arr.iter().map(scalar_str).collect::<Vec<_>>().join("; ");
                out.insert(prefix.to_string(), Value::String(joined));
            } else {
                // Array-of-object indexed cố định: một phần tử phải sinh CÙNG tên
                // cột với năm phần tử, nếu không mapping trỏ vào cột lúc có lúc không.
                out.insert(format!("{prefix}.count"), Value::from(arr.len()));
                for (i, item) in arr.iter().take(ARRAY_FANOUT).enumerate() {
                    flatten_into(&format!("{prefix}.{i}"), item, out, depth + 1);
                }
            }
        }
        _ => {
            if !prefix.is_empty() {
                out.insert(prefix.to_string(), scalar_string(v));
            }
        }
    }
}

fn scalar_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn scalar_string(v: &Value) -> Value {
    match v {
        Value::Null => Value::String(String::new()),
        Value::String(_) | Value::Number(_) | Value::Bool(_) => v.clone(),
        other => Value::String(other.to_string()),
    }
}

/// Rows đã flatten → IngestedTable. Thứ tự cột = thứ tự gặp lần đầu qua các dòng.
fn table_from_json_rows(
    name: &str,
    origin: &'static str,
    rows: Vec<Map<String, Value>>,
    note: String,
) -> Result<IngestedTable> {
    // Union cột theo thứ tự xuất hiện.
    let mut col_order: Vec<String> = Vec::new();
    for r in &rows {
        for k in r.keys() {
            if !col_order.iter().any(|c| c == k) {
                col_order.push(k.clone());
                if col_order.len() >= MAX_COLS {
                    break;
                }
            }
        }
    }
    // Mỗi cột → Vec<Option<String>>; Null / thiếu / chuỗi rỗng = None.
    let columns: Vec<Vec<Option<String>>> = col_order
        .iter()
        .map(|c| rows.iter().map(|r| json_cell(r.get(c))).collect())
        .collect();
    let names = disambiguate(&col_order);
    build_table(name, origin, note, rows.len(), names, columns)
}

/// Value một ô JSON → Option<String>. Null / thiếu / chuỗi rỗng-sau-trim = None
/// (đồng nhất với ô CSV trống → null).
fn json_cell(v: Option<&Value>) -> Option<String> {
    match v {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => {
            if s.trim().is_empty() {
                None
            } else {
                Some(s.clone())
            }
        }
        Some(Value::Bool(b)) => Some(b.to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(other) => Some(other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// delimited text (CSV/TSV/PSV)
// ---------------------------------------------------------------------------

/// Chọn delimiter có số cột >1 và đồng thuận nhất qua các dòng đầu. Quote-aware
/// nên dấu phẩy trong `"a,b"` không đánh lừa. Trả (delim, origin, cols).
fn sniff_delimiter(text: &str) -> Option<(u8, &'static str, usize)> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(25)
        .collect();
    if lines.is_empty() {
        return None;
    }
    let total = lines.len() as f64;
    let mut best: Option<(u8, &'static str, usize, f64)> = None;
    for (d, origin) in [(b',', "csv"), (b'\t', "tsv"), (b';', "csv"), (b'|', "psv")] {
        let counts: Vec<usize> = lines.iter().map(|l| count_fields(l, d)).collect();
        let first = counts[0];
        if first < 2 {
            continue;
        }
        // Coverage: bao nhiêu dòng thực sự có ≥2 field (delimiter "phủ" file).
        // CSV ragged (dòng thiếu cột cuối) vẫn phủ tốt — dùng coverage thay vì
        // đòi số cột khớp tuyệt đối, chỉ dùng consistency làm tiebreak.
        let cover = counts.iter().filter(|c| **c >= 2).count() as f64 / total;
        if cover < 0.60 {
            continue;
        }
        let consist = counts.iter().filter(|c| **c == first).count() as f64 / total;
        let score = cover * 100.0 + consist * 10.0 + first as f64 * 0.1;
        if best.as_ref().is_none_or(|(_, _, _, bs)| score > *bs) {
            best = Some((d, origin, first, score));
        }
    }
    best.map(|(d, o, c, _)| (d, o, c))
}

fn count_fields(line: &str, delim: u8) -> usize {
    let mut n = 1;
    let mut in_quotes = false;
    for b in line.bytes() {
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if b == delim && !in_quotes {
            n += 1;
        }
    }
    n
}

/// Parse delimited text bằng crate `csv` (quote/escape chuẩn). Dòng đầu = header;
/// dòng ngắn → cột thiếu = null; dòng dài → cột dư bỏ.
fn from_delimited(
    name: &str,
    text: &str,
    delim: u8,
    origin: &'static str,
) -> Result<IngestedTable> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());

    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| anyhow!("đọc header thất bại: {e}"))?
        .iter()
        .map(|s| s.to_string())
        .collect();
    let names = disambiguate(&headers);
    let ncols = names.len();

    let mut columns: Vec<Vec<Option<String>>> = vec![Vec::new(); ncols];
    let mut nrows = 0usize;
    for rec in rdr.records() {
        let rec = rec.map_err(|e| anyhow!("đọc dòng thất bại: {e}"))?;
        for (j, col) in columns.iter_mut().enumerate() {
            let cell = rec.get(j).map(str::to_string).and_then(non_empty);
            col.push(cell);
        }
        nrows += 1;
    }
    let note = format!("{origin}: {nrows} dòng, {ncols} cột");
    build_table(name, origin, note, nrows, names, columns)
}

/// Bảng một cột: dòng đầu = header, các dòng sau = giá trị. Trả `None` nếu
/// không đủ ≥2 dòng không rỗng.
fn from_single_column(name: &str, text: &str) -> Option<IngestedTable> {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.len() < 2 {
        return None;
    }
    let header = lines[0].trim().to_string();
    let names = disambiguate(&[header]);
    let column: Vec<Option<String>> = lines[1..]
        .iter()
        .map(|l| non_empty(l.trim().to_string()))
        .collect();
    let nrows = column.len();
    let note = format!("csv: {nrows} dòng, 1 cột (một cột)");
    build_table(name, "csv", note, nrows, names, vec![column]).ok()
}

fn non_empty(s: String) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}

// ---------------------------------------------------------------------------
// Excel (calamine) — mỗi sheet một nguồn
// ---------------------------------------------------------------------------

fn from_spreadsheet(stem: &str, bytes: &[u8], origin: &'static str) -> Result<Vec<IngestedTable>> {
    use calamine::Reader;
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut wb = calamine::open_workbook_auto_from_rs(cursor)
        .map_err(|e| anyhow!("đọc workbook thất bại: {e}"))?;
    let sheets = wb.sheet_names().to_vec();
    let multi = sheets.len() > 1;

    let mut out = Vec::new();
    for sheet in sheets {
        let Ok(range) = wb.worksheet_range(&sheet) else {
            continue;
        };
        if range.is_empty() {
            continue;
        }
        // Bỏ các dòng đầu rỗng hoàn toàn, dòng còn lại đầu tiên = header.
        let mut rows_iter = range
            .rows()
            .skip_while(|r| r.iter().all(|c| calamine_cell(c).is_none()));
        let Some(header_row) = rows_iter.next() else {
            continue;
        };
        let raw_headers: Vec<String> = header_row
            .iter()
            .enumerate()
            .map(|(i, c)| calamine_cell(c).unwrap_or_else(|| format!("col{}", i + 1)))
            .collect();
        let names = disambiguate(&raw_headers);
        let ncols = names.len();

        let mut columns: Vec<Vec<Option<String>>> = vec![Vec::new(); ncols];
        let mut nrows = 0usize;
        for r in rows_iter {
            let cells: Vec<Option<String>> = (0..ncols)
                .map(|i| r.get(i).and_then(calamine_cell))
                .collect();
            if cells.iter().all(|c| c.is_none()) {
                continue; // dòng rỗng giữa bảng
            }
            for (j, c) in cells.into_iter().enumerate() {
                columns[j].push(c);
            }
            nrows += 1;
        }
        let name = if multi {
            format!("{stem}__{}", slug(&sheet))
        } else {
            stem.to_string()
        };
        let note = format!("{origin} sheet '{sheet}': {nrows} dòng, {ncols} cột");
        out.push(build_table(&name, origin, note, nrows, names, columns)?);
    }
    if out.is_empty() {
        return Err(anyhow!("workbook không có sheet nào chứa dữ liệu"));
    }
    Ok(out)
}

/// Ô calamine → Option<String>. DateTime serial → ISO (Excel lưu ngày dạng số
/// serial; Display sẽ ra "45292" nơi ta cần thấy ngày).
fn calamine_cell(c: &calamine::Data) -> Option<String> {
    use calamine::Data;
    match c {
        Data::Empty => None,
        Data::Error(_) => None,
        Data::String(s) => non_empty(s.clone()),
        Data::Int(i) => Some(i.to_string()),
        Data::Float(f) => Some(fmt_float(*f)),
        Data::Bool(b) => Some(b.to_string()),
        Data::DateTime(dt) => dt
            .as_datetime()
            .map(|d| {
                let s = d.format("%Y-%m-%dT%H:%M:%S").to_string();
                // Nửa đêm → chỉ giữ phần ngày để suy được Date32.
                s.strip_suffix("T00:00:00").map(str::to_string).unwrap_or(s)
            })
            .or_else(|| non_empty(c.to_string())),
        Data::DateTimeIso(s) | Data::DurationIso(s) => non_empty(s.clone()),
    }
}

/// f64 → chuỗi không đuôi ".0" thừa cho số nguyên (để "45292.0" suy được int64).
fn fmt_float(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 9.007_199_254_740_992e15 {
        format!("{}", f as i64)
    } else {
        format!("{f}")
    }
}

// ---------------------------------------------------------------------------
// Parquet passthrough
// ---------------------------------------------------------------------------

fn from_parquet(stem: &str, bytes: &[u8]) -> Result<IngestedTable> {
    use datafusion::parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
    // ChunkReader chỉ impl cho `bytes::Bytes` (RAM) và `File`. Ingest là hàm
    // thuần (không đụng đĩa) nên đi đường Bytes; `axum::body::Bytes` chính là
    // `bytes::Bytes` được re-export (cùng một crate version trong lockfile) —
    // tránh phải khai `bytes` làm dependency trực tiếp.
    let data = axum::body::Bytes::from(bytes.to_vec());
    let builder = ParquetRecordBatchReaderBuilder::try_new(data)
        .map_err(|e| anyhow!("mở parquet thất bại: {e}"))?;
    let schema = builder.schema().clone();
    let reader = builder
        .build()
        .map_err(|e| anyhow!("dựng parquet reader thất bại: {e}"))?;
    let mut batches = Vec::new();
    let mut rows = 0usize;
    for b in reader {
        let b = b.map_err(|e| anyhow!("đọc parquet batch thất bại: {e}"))?;
        rows += b.num_rows();
        batches.push(b);
    }
    let ncols = schema.fields().len();
    Ok(IngestedTable {
        name: stem.to_string(),
        schema,
        batches,
        origin: "parquet",
        note: format!("parquet: {rows} dòng, {ncols} cột"),
        rows,
    })
}

// ---------------------------------------------------------------------------
// type inference + array building
// ---------------------------------------------------------------------------

/// Xây IngestedTable từ cột dạng chuỗi: suy kiểu per cột, dựng Arrow array.
/// `columns[j].len()` phải bằng `nrows` cho mọi j.
fn build_table(
    name: &str,
    origin: &'static str,
    note: String,
    nrows: usize,
    names: Vec<String>,
    columns: Vec<Vec<Option<String>>>,
) -> Result<IngestedTable> {
    let mut fields = Vec::with_capacity(names.len());
    let mut arrays: Vec<ArrayRef> = Vec::with_capacity(names.len());
    for (col_name, col) in names.iter().zip(columns.iter()) {
        let dt = infer_type(col);
        let arr = build_array(&dt, col);
        // Mọi cột nullable: file thật luôn có ô thiếu, cột toàn null cũng hợp lệ.
        fields.push(Field::new(col_name, dt, true));
        arrays.push(arr);
    }
    let schema: SchemaRef = Arc::new(Schema::new(fields));
    // Bảng không cột → không batch (chỉ giữ schema rỗng); còn lại một batch.
    let batches = if arrays.is_empty() {
        Vec::new()
    } else {
        vec![RecordBatch::try_new(schema.clone(), arrays)
            .map_err(|e| anyhow!("dựng RecordBatch thất bại: {e}"))?]
    };
    Ok(IngestedTable {
        name: name.to_string(),
        schema,
        batches,
        origin,
        note,
        rows: nrows,
    })
}

const TS_FORMATS: &[&str] = &[
    "%Y-%m-%dT%H:%M:%S%.f",
    "%Y-%m-%d %H:%M:%S%.f",
    "%Y-%m-%dT%H:%M:%S",
    "%Y-%m-%d %H:%M:%S",
];

/// Suy kiểu một cột theo ưu tiên bool → int64 → float64 → date32 → timestamp(µs)
/// → utf8. Cột không có giá trị nào (toàn null) = utf8.
fn infer_type(col: &[Option<String>]) -> DataType {
    let mut saw = false;
    let (mut all_bool, mut all_int, mut all_float, mut all_date, mut all_ts) =
        (true, true, true, true, true);
    for v in col.iter().flatten() {
        saw = true;
        let s = v.trim();
        if all_bool && parse_bool(s).is_none() {
            all_bool = false;
        }
        if all_int && s.parse::<i64>().is_err() {
            all_int = false;
        }
        if all_float && s.parse::<f64>().is_err() {
            all_float = false;
        }
        if all_date && parse_date32(s).is_none() {
            all_date = false;
        }
        if all_ts && parse_ts_micros(s).is_none() {
            all_ts = false;
        }
        if !(all_bool || all_int || all_float || all_date || all_ts) {
            break;
        }
    }
    if !saw {
        return DataType::Utf8;
    }
    if all_bool {
        DataType::Boolean
    } else if all_int {
        DataType::Int64
    } else if all_float {
        DataType::Float64
    } else if all_date {
        DataType::Date32
    } else if all_ts {
        DataType::Timestamp(TimeUnit::Microsecond, None)
    } else {
        DataType::Utf8
    }
}

fn build_array(dt: &DataType, col: &[Option<String>]) -> ArrayRef {
    let trimmed = |o: &Option<String>| o.as_deref().map(|s| s.trim().to_string());
    match dt {
        DataType::Boolean => {
            let v: Vec<Option<bool>> = col
                .iter()
                .map(|o| trimmed(o).as_deref().and_then(parse_bool))
                .collect();
            Arc::new(BooleanArray::from(v))
        }
        DataType::Int64 => {
            let v: Vec<Option<i64>> = col
                .iter()
                .map(|o| trimmed(o).and_then(|s| s.parse::<i64>().ok()))
                .collect();
            Arc::new(Int64Array::from(v))
        }
        DataType::Float64 => {
            let v: Vec<Option<f64>> = col
                .iter()
                .map(|o| trimmed(o).and_then(|s| s.parse::<f64>().ok()))
                .collect();
            Arc::new(Float64Array::from(v))
        }
        DataType::Date32 => {
            let v: Vec<Option<i32>> = col
                .iter()
                .map(|o| trimmed(o).as_deref().and_then(parse_date32))
                .collect();
            Arc::new(Date32Array::from(v))
        }
        DataType::Timestamp(TimeUnit::Microsecond, None) => {
            let v: Vec<Option<i64>> = col
                .iter()
                .map(|o| trimmed(o).as_deref().and_then(parse_ts_micros))
                .collect();
            Arc::new(TimestampMicrosecondArray::from(v))
        }
        // Utf8 và mọi trường hợp khác: giữ nguyên chuỗi (không trim để bảo toàn).
        _ => {
            let v: Vec<Option<String>> = col.to_vec();
            Arc::new(StringArray::from(v))
        }
    }
}

fn parse_bool(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// "YYYY-MM-DD" → số ngày từ epoch (Date32).
fn parse_date32(s: &str) -> Option<i32> {
    let d = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()?;
    let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)?;
    Some(d.signed_duration_since(epoch).num_days() as i32)
}

/// Datetime (ISO hoặc "YYYY-MM-DD HH:MM:SS") → micros từ epoch (UTC).
fn parse_ts_micros(s: &str) -> Option<i64> {
    for f in TS_FORMATS {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, f) {
            return Some(dt.and_utc().timestamp_micros());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// tên cột / file
// ---------------------------------------------------------------------------

/// Tên cột trùng sau flatten/header → thêm hậu tố `_2`, `_3`… để unique. Cột
/// tên rỗng → `col{i+1}`.
fn disambiguate(raw: &[String]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::with_capacity(raw.len());
    for (i, name) in raw.iter().enumerate() {
        let base = if name.trim().is_empty() {
            format!("col{}", i + 1)
        } else {
            name.trim().to_string()
        };
        if !seen.iter().any(|s| s == &base) {
            seen.push(base);
            continue;
        }
        let mut n = 2;
        loop {
            let cand = format!("{base}_{n}");
            if !seen.iter().any(|s| s == &cand) {
                seen.push(cand);
                break;
            }
            n += 1;
        }
    }
    seen
}

fn decode_text(bytes: &[u8]) -> String {
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(body).to_string()
}

/// `sales report 2024.final.csv` → `sales_report_2024_final`.
fn file_stem(filename: &str) -> String {
    let base = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
    let s = slug(stem);
    if s.is_empty() {
        "source".into()
    } else {
        s
    }
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    let mut prev_us = false;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            out.push(ch);
            prev_us = false;
        } else if !prev_us {
            out.push('_');
            prev_us = true;
        }
    }
    out.trim_matches('_').to_string()
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use datafusion::arrow::array::{Array, Int64Array as I64};

    fn one(filename: &str, bytes: &[u8]) -> IngestedTable {
        let mut v = ingest(filename, bytes).unwrap();
        assert_eq!(v.len(), 1, "kỳ vọng đúng một bảng");
        v.pop().unwrap()
    }

    fn col_type(t: &IngestedTable, name: &str) -> DataType {
        t.schema.field_with_name(name).unwrap().data_type().clone()
    }

    #[test]
    fn csv_infers_types_and_nulls() {
        let csv = "id,price,when,active,label\n\
                   1,9.5,2024-01-02,true,alpha\n\
                   2,,2024-02-03,false,\n\
                   3,7,2024-03-04,true,gamma\n";
        let t = one("orders.csv", csv.as_bytes());
        assert_eq!(t.origin, "csv");
        assert_eq!(t.rows, 3);
        assert_eq!(col_type(&t, "id"), DataType::Int64);
        assert_eq!(col_type(&t, "price"), DataType::Float64);
        assert_eq!(col_type(&t, "when"), DataType::Date32);
        assert_eq!(col_type(&t, "active"), DataType::Boolean);
        assert_eq!(col_type(&t, "label"), DataType::Utf8);

        let batch = &t.batches[0];
        // price có ô thiếu ở dòng 2 → null.
        let price = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        assert!(price.is_null(1));
        assert_eq!(price.value(0), 9.5);
        // label dòng 2 trống → null.
        let label = batch
            .column(4)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(label.is_null(1));
    }

    #[test]
    fn csv_short_row_pads_null() {
        // Dòng 2 thiếu cột cuối hoàn toàn.
        let csv = "a,b,c\n1,2,3\n4,5\n";
        let t = one("x.csv", csv.as_bytes());
        assert_eq!(t.rows, 2);
        let c = t.batches[0]
            .column(2)
            .as_any()
            .downcast_ref::<I64>()
            .unwrap();
        assert_eq!(c.value(0), 3);
        assert!(c.is_null(1), "cột thiếu → null");
    }

    #[test]
    fn tsv_sniffed_by_tabs() {
        let tsv = "name\tage\nAn\t30\nBình\t25\n";
        let t = one("people.tsv", tsv.as_bytes());
        assert_eq!(t.origin, "tsv");
        assert_eq!(col_type(&t, "age"), DataType::Int64);
        assert_eq!(t.rows, 2);
    }

    #[test]
    fn ndjson_one_object_per_line() {
        let nd = "{\"id\":1,\"name\":\"a\"}\n{\"id\":2,\"name\":\"b\"}\n";
        let t = one("stream.ndjson", nd.as_bytes());
        assert_eq!(t.origin, "ndjson");
        assert_eq!(t.rows, 2);
        assert_eq!(col_type(&t, "id"), DataType::Int64);
        assert_eq!(col_type(&t, "name"), DataType::Utf8);
    }

    #[test]
    fn json_nested_flatten_fanout_and_count() {
        let json = r#"[
          {"id": 1, "addr": {"city": "Hà Nội"}, "tags": ["x","y"],
           "items": [{"sku":"A"},{"sku":"B"},{"sku":"C"},{"sku":"D"}]},
          {"id": 2, "addr": {"city": "Huế"}, "tags": ["z"],
           "items": [{"sku":"E"}]}
        ]"#;
        let t = one("nested.json", json.as_bytes());
        assert_eq!(t.origin, "json");
        assert_eq!(t.rows, 2);
        // dotted path cho object lồng.
        assert_eq!(col_type(&t, "addr.city"), DataType::Utf8);
        // array scalar join "; ".
        let tags = t.batches[0]
            .column(t.schema.index_of("tags").unwrap())
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert_eq!(tags.value(0), "x; y");
        // array-of-object: fanout 3 + .count (4 phần tử nhưng chỉ 3 cột index).
        assert!(t.schema.field_with_name("items.count").is_ok());
        assert!(t.schema.field_with_name("items.0.sku").is_ok());
        assert!(t.schema.field_with_name("items.2.sku").is_ok());
        assert!(
            t.schema.field_with_name("items.3.sku").is_err(),
            "quá fanout không có cột index"
        );
        let cnt = t.batches[0]
            .column(t.schema.index_of("items.count").unwrap())
            .as_any()
            .downcast_ref::<I64>()
            .unwrap();
        assert_eq!(cnt.value(0), 4);
        assert_eq!(cnt.value(1), 1);
    }

    #[test]
    fn json_object_wrapping_best_array() {
        // Object bọc nhiều array — chọn cái nhiều object nhất (data, 2 phần tử)
        // chứ không phải meta.
        let json = r#"{
          "meta": {"generated": "x"},
          "data": [{"k": 1}, {"k": 2}],
          "notes": ["a", "b", "c"]
        }"#;
        let t = one("wrapped.json", json.as_bytes());
        assert_eq!(t.rows, 2);
        assert_eq!(col_type(&t, "k"), DataType::Int64);
        assert!(t.note.contains("'data'"), "note nêu path: {}", t.note);
    }

    #[test]
    fn parquet_roundtrip() {
        use datafusion::parquet::arrow::ArrowWriter;
        // Dựng một batch rồi ghi parquet vào RAM.
        let schema: SchemaRef = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, true),
            Field::new("name", DataType::Utf8, true),
        ]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(Int64Array::from(vec![Some(10), Some(20), None])),
                Arc::new(StringArray::from(vec![
                    Some("a".to_string()),
                    None,
                    Some("c".to_string()),
                ])),
            ],
        )
        .unwrap();
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut w = ArrowWriter::try_new(&mut buf, schema.clone(), None).unwrap();
            w.write(&batch).unwrap();
            w.close().unwrap();
        }
        assert!(buf.starts_with(b"PAR1"), "magic bytes parquet");

        let t = one("dump.parquet", &buf);
        assert_eq!(t.origin, "parquet");
        assert_eq!(t.rows, 3);
        assert_eq!(col_type(&t, "id"), DataType::Int64);
        let ids = t.batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<I64>()
            .unwrap();
        assert_eq!(ids.value(0), 10);
        assert!(ids.is_null(2));
    }

    #[test]
    fn duplicate_columns_disambiguated() {
        // Header trùng tên (hợp lệ trong CSV thô).
        let csv = "id,id,id\n1,2,3\n";
        let t = one("dup.csv", csv.as_bytes());
        let names: Vec<String> = t.schema.fields().iter().map(|f| f.name().clone()).collect();
        assert_eq!(names, vec!["id", "id_2", "id_3"]);
    }

    #[test]
    fn json_duplicate_after_flatten_disambiguated() {
        // "a.b" và "a" chứa "b" không đụng nhau ở đây; kiểm nhậu tố khi trùng
        // thật: hai cột cùng tên sau flatten (đường .0 và key gốc).
        let json = r#"[{"x": 1, "y": {"z": 2}}]"#;
        let t = one("f.json", json.as_bytes());
        assert!(t.schema.field_with_name("x").is_ok());
        assert!(t.schema.field_with_name("y.z").is_ok());
    }

    #[test]
    fn timestamp_column_inferred() {
        let csv = "ts\n2024-01-02T03:04:05\n2024-01-02 06:07:08\n";
        let t = one("t.csv", csv.as_bytes());
        assert_eq!(
            col_type(&t, "ts"),
            DataType::Timestamp(TimeUnit::Microsecond, None)
        );
        let arr = t.batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<TimestampMicrosecondArray>()
            .unwrap();
        // 2024-01-02T03:04:05 UTC = 1704164645 s.
        assert_eq!(arr.value(0), 1_704_164_645_000_000);
    }

    #[test]
    fn empty_file_errors() {
        assert!(ingest("empty.csv", b"").is_err());
        assert!(ingest("blank.csv", b"   \n  \n").is_err());
    }

    #[test]
    fn empty_column_is_nullable_utf8() {
        let csv = "a,b\n1,\n2,\n";
        let t = one("e.csv", csv.as_bytes());
        // b toàn trống → utf8, mọi ô null.
        assert_eq!(col_type(&t, "b"), DataType::Utf8);
        let b = t.batches[0]
            .column(1)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(b.is_null(0) && b.is_null(1));
    }
}
