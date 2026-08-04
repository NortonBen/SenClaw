//! Stage [0] **Universal ingest** — "drop any file, get something the pipeline
//! can chew on".
//!
//! Everything upstream of the profiler used to be `CSV | JSON-array`, which
//! meant a human had to convert their spreadsheet / export / report by hand
//! before the ontology app could see it. This module removes that step: it
//! sniffs the real format (magic bytes first, then structure, never the file
//! extension alone) and **normalizes** it into exactly one of two shapes:
//!
//! * **tabular** → a JSON array of *flat* objects (`kind = "json"`), or the raw
//!   CSV text when the input already was CSV (`kind = "csv"` — keeping the
//!   original bytes makes the lift auditable against the file the user has).
//! * **unstructured** → plain text (`kind = "text"`), which the LLM extraction
//!   path turns into triples.
//!
//! Normalizing here (rather than teaching every downstream stage about every
//! format) is what keeps `profile` / `mapping` / `lift` unchanged: they still
//! only ever see a `Table`.
//!
//! Nested structures are flattened with dotted paths (`address.city`), because
//! the mapping DSL addresses columns by name — a nested value the DSL cannot
//! name is a value the ontology can never reach.

use anyhow::{anyhow, Result};
use serde_json::{Map, Value};

/// One normalized source ready to be stored. A single upload can yield several
/// (an Excel workbook → one per sheet, an XML export → one per record type).
#[derive(Debug, Clone, serde::Serialize)]
pub struct Ingested {
    /// Suggested logical source name (file stem, plus the sheet name if any).
    pub name: String,
    /// Storage kind the rest of the pipeline understands: `csv | json | text`.
    pub kind: String,
    /// The format actually detected, for display/provenance (`xlsx`, `pdf`, …).
    pub origin: String,
    /// Normalized content.
    pub content: String,
    /// What the sniffer did, in one human sentence.
    pub note: String,
    pub rows: usize,
    pub columns: usize,
}

impl Ingested {
    fn text(name: &str, origin: &str, body: String, note: String) -> Self {
        let rows = body.lines().filter(|l| !l.trim().is_empty()).count();
        Self {
            name: name.to_string(),
            kind: "text".into(),
            origin: origin.into(),
            content: body,
            note,
            rows,
            columns: 0,
        }
    }

    fn rows_json(name: &str, origin: &str, rows: Vec<Map<String, Value>>, note: String) -> Self {
        let mut cols: Vec<String> = Vec::new();
        for r in &rows {
            for k in r.keys() {
                if !cols.iter().any(|c| c == k) {
                    cols.push(k.clone());
                }
            }
        }
        let n = rows.len();
        Self {
            name: name.to_string(),
            kind: "json".into(),
            origin: origin.into(),
            content: serde_json::to_string(&Value::Array(
                rows.into_iter().map(Value::Object).collect(),
            ))
            .unwrap_or_else(|_| "[]".into()),
            note,
            rows: n,
            columns: cols.len(),
        }
    }
}

/// Formats we can turn into something useful, for the UI's "drop a file" hint.
pub const SUPPORTED: &[&str] = &[
    "csv", "tsv", "psv", "json", "jsonl", "ndjson", "yaml", "yml", "xml", "xlsx", "xlsm", "xls",
    "ods", "docx", "pdf", "html", "htm", "md", "markdown", "txt", "log", "rtf",
];

// ---------------------------------------------------------------------------
// entry point
// ---------------------------------------------------------------------------

/// Sniff `bytes` and normalize. `filename` is only a hint — magic bytes and
/// structure win, so a `.txt` holding JSON is still ingested as a table.
pub fn ingest(filename: &str, bytes: &[u8]) -> Result<Vec<Ingested>> {
    let stem = file_stem(filename);
    if bytes.is_empty() {
        return Err(anyhow!("empty file"));
    }
    // --- binary containers, detected by magic bytes ---
    if bytes.starts_with(b"%PDF") {
        return Ok(vec![from_pdf(&stem, bytes)?]);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return from_zip_container(&stem, bytes);
    }
    // Legacy OLE2 compound file: .xls / .doc — calamine reads the spreadsheet form.
    if bytes.starts_with(b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1") {
        return from_spreadsheet(&stem, bytes).map_err(|e| {
            anyhow!("legacy Office file: only .xls spreadsheets can be read here ({e}). Save as .docx/.xlsx and retry.")
        });
    }
    // --- everything else is text; the *structure* decides ---
    let text = decode_text(bytes);
    ingest_text(&stem, &text)
}

/// Same as [`ingest`] but for content that is already a `String` (the MCP path,
/// where an agent pastes the file body).
pub fn ingest_text(name: &str, text: &str) -> Result<Vec<Ingested>> {
    let t = text.trim_start_matches('\u{feff}');
    let head = t.trim_start();
    if head.is_empty() {
        return Err(anyhow!("empty content"));
    }

    // JSON / JSONL
    if head.starts_with('{') || head.starts_with('[') {
        if let Ok(v) = serde_json::from_str::<Value>(t) {
            return Ok(vec![from_json_value(name, "json", v)]);
        }
        if let Some(ing) = from_jsonl(name, t) {
            return Ok(vec![ing]);
        }
    }

    // XML / HTML — `<?xml`, `<!DOCTYPE html>`, or any leading tag.
    if head.starts_with('<') {
        let lower = head.to_ascii_lowercase();
        if lower.starts_with("<!doctype html")
            || lower.starts_with("<html")
            || lower.contains("<body")
        {
            return from_html(name, t);
        }
        if let Ok(tables) = from_html(name, t) {
            // An `<?xml`-headed doc containing only tables is really HTML-ish.
            if !tables.is_empty() && tables[0].kind == "json" {
                return Ok(tables);
            }
        }
        return from_xml(name, t);
    }

    // Markdown pipe tables (before the delimiter sniff — `|` is also a CSV
    // delimiter, and a markdown table would sniff as a very ragged PSV).
    if let Some(ing) = from_markdown_tables(name, t) {
        return Ok(vec![ing]);
    }

    // Delimited text (comma / semicolon / tab / pipe), auto-detected.
    if let Some((delim, cols)) = sniff_delimiter(t) {
        let content = if delim == ',' {
            t.to_string()
        } else {
            retab(t, delim)
        };
        let rows = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count()
            .saturating_sub(1);
        return Ok(vec![Ingested {
            name: name.to_string(),
            kind: "csv".into(),
            origin: match delim {
                ',' => "csv",
                '\t' => "tsv",
                ';' => "csv-semicolon",
                _ => "psv",
            }
            .into(),
            content,
            note: format!(
                "delimited text: '{}' separator, {cols} columns",
                show_delim(delim)
            ),
            rows,
            columns: cols,
        }]);
    }

    // YAML — only after the structured sniffs, since valid JSON is valid YAML.
    if looks_like_yaml(t) {
        if let Ok(y) = serde_yaml::from_str::<serde_yaml::Value>(t) {
            if let Ok(v) = serde_json::to_value(y) {
                if !matches!(v, Value::String(_) | Value::Null) {
                    return Ok(vec![from_json_value(name, "yaml", v)]);
                }
            }
        }
    }

    // Nothing structured: unstructured text for the LLM extraction path.
    Ok(vec![Ingested::text(
        name,
        "text",
        t.to_string(),
        "free text — the AI will extract triples from it".into(),
    )])
}

// ---------------------------------------------------------------------------
// JSON / YAML
// ---------------------------------------------------------------------------

/// Turn any JSON value into a table: find the most table-like array inside it,
/// flatten each element. A bare object becomes a single row.
fn from_json_value(name: &str, origin: &str, v: Value) -> Ingested {
    match best_array(&v, 0) {
        Some((path, arr)) => {
            let rows: Vec<Map<String, Value>> = arr.iter().map(flatten_row).collect();
            let note = if path.is_empty() {
                format!("{origin}: array of {} records, flattened", rows.len())
            } else {
                format!("{origin}: {} records from '{path}', flattened", rows.len())
            };
            Ingested::rows_json(name, origin, rows, note)
        }
        None => {
            let rows = vec![flatten_row(&v)];
            Ingested::rows_json(
                name,
                origin,
                rows,
                format!("{origin}: single record, flattened"),
            )
        }
    }
}

/// JSON Lines / NDJSON: one JSON value per line.
fn from_jsonl(name: &str, text: &str) -> Option<Ingested> {
    let mut rows = Vec::new();
    let mut lines = 0usize;
    for line in text.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        lines += 1;
        let v: Value = serde_json::from_str(l).ok()?;
        rows.push(flatten_row(&v));
    }
    if lines < 2 {
        return None;
    }
    Some(Ingested::rows_json(
        name,
        "jsonl",
        rows,
        format!("JSON Lines: {lines} records, flattened"),
    ))
}

/// Depth-first search for the array that most looks like a record list: the one
/// with the most object elements, preferring shallower paths on a tie.
fn best_array(v: &Value, depth: usize) -> Option<(String, &Vec<Value>)> {
    if depth > 6 {
        return None;
    }
    // (path, array, object-count, depth) — the best candidate seen so far.
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

/// Flatten one record into `col -> scalar`. Nested objects become dotted paths;
/// scalar arrays are joined; object arrays are kept as compact JSON so nothing
/// is silently lost (the AI can still read them when drafting the mapping).
fn flatten_row(v: &Value) -> Map<String, Value> {
    let mut out = Map::new();
    flatten_into("", v, &mut out, 0);
    if out.is_empty() {
        out.insert("value".into(), scalar_string(v));
    }
    out
}

/// How many elements of a nested object array get their own columns. Past this,
/// only `.count` survives — a repeating child list is really a second entity and
/// belongs in its own source, not in 200 columns of this one.
const ARRAY_FANOUT: usize = 3;

fn flatten_into(prefix: &str, v: &Value, out: &mut Map<String, Value>, depth: usize) {
    if out.len() >= 300 {
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
                // Arrays of objects are indexed, never length-dependent: a
                // one-element array must produce the SAME column names as a
                // five-element one, or the mapping would address columns that
                // exist on some rows and not others.
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

fn looks_like_yaml(t: &str) -> bool {
    let mut kv = 0;
    for line in t.lines().take(40) {
        let l = line.trim_end();
        if l.trim().is_empty() || l.trim_start().starts_with('#') {
            continue;
        }
        if l == "---" {
            return true;
        }
        let body = l.trim_start_matches(['-', ' ']);
        match body.split_once(':') {
            Some((k, _)) if !k.is_empty() && !k.contains(' ') => kv += 1,
            _ => {}
        }
    }
    kv >= 2
}

// ---------------------------------------------------------------------------
// delimited text
// ---------------------------------------------------------------------------

fn show_delim(d: char) -> &'static str {
    match d {
        '\t' => "tab",
        ';' => ";",
        '|' => "|",
        _ => ",",
    }
}

/// Pick the delimiter whose field count is >1 and most consistent across the
/// first lines. Quote-aware, so commas inside `"a,b"` don't fool the count.
fn sniff_delimiter(text: &str) -> Option<(char, usize)> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(25)
        .collect();
    if lines.len() < 2 {
        return None;
    }
    let mut best: Option<(char, usize, f64)> = None;
    for d in [',', '\t', ';', '|'] {
        let counts: Vec<usize> = lines.iter().map(|l| count_fields(l, d)).collect();
        let first = counts[0];
        if first < 2 {
            continue;
        }
        let agree = counts.iter().filter(|c| **c == first).count() as f64 / counts.len() as f64;
        if agree < 0.7 {
            continue;
        }
        // Prefer the delimiter that agrees most; break ties on more columns.
        let score = agree * 100.0 + first as f64;
        if best.as_ref().is_none_or(|(_, _, bs)| score > *bs) {
            best = Some((d, first, score));
        }
    }
    best.map(|(d, c, _)| (d, c))
}

fn count_fields(line: &str, delim: char) -> usize {
    let mut n = 1;
    let mut in_quotes = false;
    for c in line.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == delim && !in_quotes {
            n += 1;
        }
    }
    n
}

/// Re-emit a non-comma-delimited file as proper CSV so the stored source and
/// the profiler agree on one dialect.
fn retab(text: &str, delim: char) -> String {
    let mut out = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_delimited(line, delim);
        let row: Vec<String> = fields.iter().map(|f| csv_field(f)).collect();
        out.push_str(&row.join(","));
        out.push('\n');
    }
    out
}

fn split_delimited(line: &str, delim: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for c in line.chars() {
        if c == '"' {
            in_quotes = !in_quotes;
        } else if c == delim && !in_quotes {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(c);
        }
    }
    out.push(cur);
    out
}

fn csv_field(s: &str) -> String {
    let t = s.trim();
    if t.contains(',') || t.contains('"') || t.contains('\n') {
        format!("\"{}\"", t.replace('"', "\"\""))
    } else {
        t.to_string()
    }
}

// ---------------------------------------------------------------------------
// Markdown
// ---------------------------------------------------------------------------

/// Extract the first (largest) GitHub-style pipe table from a Markdown doc.
fn from_markdown_tables(name: &str, text: &str) -> Option<Ingested> {
    let lines: Vec<&str> = text.lines().collect();
    let mut best: Option<(usize, usize)> = None; // (start, end) of the widest table
    let mut i = 0;
    while i + 1 < lines.len() {
        let header = lines[i].trim();
        let sep = lines[i + 1].trim();
        let is_sep = sep.contains('-')
            && sep.chars().all(|c| matches!(c, '-' | ':' | '|' | ' '))
            && sep.contains('|');
        if header.contains('|') && is_sep {
            let mut end = i + 2;
            while end < lines.len()
                && lines[end].trim().contains('|')
                && !lines[end].trim().is_empty()
            {
                end += 1;
            }
            if best.is_none_or(|(bs, be)| end - i > be - bs) {
                best = Some((i, end));
            }
            i = end;
        } else {
            i += 1;
        }
    }
    let (start, end) = best?;
    let cells = |l: &str| -> Vec<String> {
        l.trim()
            .trim_matches('|')
            .split('|')
            .map(|c| c.trim().to_string())
            .collect()
    };
    let headers = cells(lines[start]);
    let mut out = String::new();
    out.push_str(
        &headers
            .iter()
            .map(|h| csv_field(h))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push('\n');
    let mut rows = 0;
    for l in &lines[start + 2..end] {
        let r = cells(l);
        if r.iter().all(|c| c.is_empty()) {
            continue;
        }
        out.push_str(&r.iter().map(|c| csv_field(c)).collect::<Vec<_>>().join(","));
        out.push('\n');
        rows += 1;
    }
    if rows == 0 {
        return None;
    }
    Some(Ingested {
        name: name.to_string(),
        kind: "csv".into(),
        origin: "markdown-table".into(),
        content: out,
        note: format!("Markdown table: {rows} rows × {} columns", headers.len()),
        rows,
        columns: headers.len(),
    })
}

// ---------------------------------------------------------------------------
// HTML
// ---------------------------------------------------------------------------

/// HTML: every `<table>` becomes its own tabular source; a page with no table
/// degrades to its stripped text.
fn from_html(name: &str, text: &str) -> Result<Vec<Ingested>> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut pos = 0usize;
    let mut idx = 0usize;
    while let Some(rel) = lower[pos..].find("<table") {
        let start = pos + rel;
        let Some(rel_end) = lower[start..].find("</table") else {
            break;
        };
        let end = start + rel_end;
        let block = &text[start..end];
        if let Some(ing) = html_table(name, block, idx) {
            out.push(ing);
            idx += 1;
        }
        pos = end + 7;
    }
    if !out.is_empty() {
        return Ok(out);
    }
    let stripped = strip_tags(text);
    if stripped.trim().is_empty() {
        return Err(anyhow!("HTML document has no table and no readable text"));
    }
    Ok(vec![Ingested::text(
        name,
        "html",
        stripped,
        "HTML page with no table — text extracted for AI extraction".into(),
    )])
}

fn html_table(name: &str, block: &str, idx: usize) -> Option<Ingested> {
    let lower = block.to_ascii_lowercase();
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut pos = 0usize;
    while let Some(rel) = lower[pos..].find("<tr") {
        let start = pos + rel;
        let end = lower[start..]
            .find("</tr")
            .map(|r| start + r)
            .unwrap_or(block.len());
        rows.push(html_cells(&block[start..end]));
        pos = end + 4;
        if pos >= block.len() {
            break;
        }
    }
    rows.retain(|r| !r.is_empty());
    if rows.len() < 2 {
        return None;
    }
    let width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let headers: Vec<String> = rows[0]
        .iter()
        .enumerate()
        .map(|(i, h)| {
            if h.trim().is_empty() {
                format!("col{}", i + 1)
            } else {
                h.clone()
            }
        })
        .collect();
    let mut csv_out = headers
        .iter()
        .map(|h| csv_field(h))
        .collect::<Vec<_>>()
        .join(",");
    csv_out.push('\n');
    for r in &rows[1..] {
        let mut cells: Vec<String> = r.clone();
        cells.resize(width, String::new());
        csv_out.push_str(
            &cells
                .iter()
                .map(|c| csv_field(c))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv_out.push('\n');
    }
    let n = rows.len() - 1;
    Some(Ingested {
        name: if idx == 0 {
            name.to_string()
        } else {
            format!("{name}_table{}", idx + 1)
        },
        kind: "csv".into(),
        origin: "html-table".into(),
        content: csv_out,
        note: format!("HTML table #{}: {n} rows × {width} columns", idx + 1),
        rows: n,
        columns: width,
    })
}

fn html_cells(row: &str) -> Vec<String> {
    let lower = row.to_ascii_lowercase();
    let mut cells = Vec::new();
    let mut pos = 0usize;
    loop {
        let td = lower[pos..].find("<td").map(|r| pos + r);
        let th = lower[pos..].find("<th").map(|r| pos + r);
        let Some(start) = [td, th].into_iter().flatten().min() else {
            break;
        };
        let Some(open_end) = lower[start..].find('>').map(|r| start + r + 1) else {
            break;
        };
        let end = lower[open_end..]
            .find("</t")
            .map(|r| open_end + r)
            .unwrap_or(row.len());
        cells.push(strip_tags(&row[open_end..end]).trim().to_string());
        pos = end + 3;
        if pos >= row.len() {
            break;
        }
    }
    cells
}

/// Remove tags and decode the handful of entities that actually matter.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut skip_until: Option<&str> = None;
    let lower = html.to_ascii_lowercase();
    let bytes: Vec<char> = html.chars().collect();
    let lower_chars: Vec<char> = lower.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(close) = skip_until {
            let rest: String = lower_chars[i..].iter().take(close.len()).collect();
            if rest == close {
                skip_until = None;
                i += close.len();
                continue;
            }
            i += 1;
            continue;
        }
        let c = bytes[i];
        if c == '<' {
            let tag: String = lower_chars[i..].iter().take(8).collect();
            if tag.starts_with("<script") {
                skip_until = Some("</script");
            } else if tag.starts_with("<style") {
                skip_until = Some("</style");
            } else if tag.starts_with("<br")
                || tag.starts_with("</p")
                || tag.starts_with("</div")
                || tag.starts_with("</tr")
                || tag.starts_with("</h")
            {
                out.push('\n');
            }
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            out.push(c);
        }
        i += 1;
    }
    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    // Collapse the blank-line storm that tag stripping leaves behind.
    let mut lines: Vec<&str> = Vec::new();
    for l in out.lines() {
        let t = l.trim();
        if t.is_empty() {
            if matches!(lines.last(), Some(&"") | None) {
                continue;
            }
            lines.push("");
        } else {
            lines.push(t);
        }
    }
    lines.join("\n")
}

// ---------------------------------------------------------------------------
// XML
// ---------------------------------------------------------------------------

/// XML → JSON tree → the same "find the record array" logic as JSON. Attributes
/// become `@name` columns; leaf text becomes the value.
fn from_xml(name: &str, text: &str) -> Result<Vec<Ingested>> {
    let v = xml_to_json(text)?;
    let ing = from_json_value(name, "xml", v);
    if ing.rows == 0 {
        return Err(anyhow!("XML document had no repeating records"));
    }
    Ok(vec![ing])
}

fn xml_to_json(text: &str) -> Result<Value> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(text);
    reader.config_mut().trim_text(true);
    // Stack of (element name, child map, accumulated text).
    let mut stack: Vec<(String, Map<String, Value>, String)> =
        vec![("#root".into(), Map::new(), String::new())];
    let mut buf_depth = 0usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                buf_depth += 1;
                if buf_depth > 64 {
                    return Err(anyhow!("XML nested too deeply"));
                }
                let tag = local_name(e.name().0);
                let mut attrs = Map::new();
                for a in e.attributes().flatten() {
                    let k = local_name(a.key.0);
                    let val = a
                        .unescape_value()
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    attrs.insert(format!("@{k}"), Value::String(val));
                }
                stack.push((tag, attrs, String::new()));
            }
            Ok(Event::Empty(e)) => {
                let tag = local_name(e.name().0);
                let mut attrs = Map::new();
                for a in e.attributes().flatten() {
                    let k = local_name(a.key.0);
                    let val = a
                        .unescape_value()
                        .map(|c| c.to_string())
                        .unwrap_or_default();
                    attrs.insert(format!("@{k}"), Value::String(val));
                }
                let node = if attrs.is_empty() {
                    Value::String(String::new())
                } else {
                    Value::Object(attrs)
                };
                push_child(&mut stack, &tag, node);
            }
            Ok(Event::Text(t)) => {
                let s = t.unescape().map(|c| c.to_string()).unwrap_or_default();
                if let Some(top) = stack.last_mut() {
                    top.2.push_str(&s);
                }
            }
            Ok(Event::CData(t)) => {
                let s = String::from_utf8_lossy(&t).to_string();
                if let Some(top) = stack.last_mut() {
                    top.2.push_str(&s);
                }
            }
            Ok(Event::End(_)) => {
                buf_depth = buf_depth.saturating_sub(1);
                if stack.len() <= 1 {
                    continue;
                }
                let (tag, children, txt) = stack.pop().unwrap();
                let node = if children.is_empty() {
                    Value::String(txt.trim().to_string())
                } else {
                    let mut o = children;
                    if !txt.trim().is_empty() {
                        o.insert("#text".into(), Value::String(txt.trim().to_string()));
                    }
                    Value::Object(o)
                };
                push_child(&mut stack, &tag, node);
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("xml: {e}")),
            _ => {}
        }
    }
    let (_, root, _) = stack.pop().unwrap();
    Ok(Value::Object(root))
}

/// Insert a finished element under its parent, promoting a repeated tag name to
/// an array — that array is exactly what `best_array` later picks up as the
/// record list.
fn push_child(stack: &mut [(String, Map<String, Value>, String)], tag: &str, node: Value) {
    let Some(parent) = stack.last_mut() else {
        return;
    };
    match parent.1.get_mut(tag) {
        Some(Value::Array(arr)) => arr.push(node),
        Some(existing) => {
            let prev = existing.take();
            *existing = Value::Array(vec![prev, node]);
        }
        None => {
            parent.1.insert(tag.to_string(), node);
        }
    }
}

fn local_name(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    s.rsplit(':').next().unwrap_or(&s).to_string()
}

// ---------------------------------------------------------------------------
// binary containers
// ---------------------------------------------------------------------------

/// A ZIP is an OOXML/ODF container — decide by what's inside, not by extension.
fn from_zip_container(stem: &str, bytes: &[u8]) -> Result<Vec<Ingested>> {
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| anyhow!("zip: {e}"))?;
    let names: Vec<String> = zip.file_names().map(|s| s.to_string()).collect();
    let has = |p: &str| names.iter().any(|n| n == p);

    if has("xl/workbook.xml") || names.iter().any(|n| n.starts_with("xl/worksheets/")) {
        return from_spreadsheet(stem, bytes);
    }
    if has("content.xml") && names.iter().any(|n| n == "mimetype") {
        // OpenDocument: calamine reads .ods; .odt falls back to its text.
        if let Ok(v) = from_spreadsheet(stem, bytes) {
            return Ok(v);
        }
        let content = {
            use std::io::Read;
            let mut f = zip
                .by_name("content.xml")
                .map_err(|e| anyhow!("odf: {e}"))?;
            let mut raw = Vec::new();
            f.read_to_end(&mut raw).map_err(|e| anyhow!("odf: {e}"))?;
            String::from_utf8_lossy(&raw).to_string()
        };
        return Ok(vec![Ingested::text(
            stem,
            "odt",
            strip_tags(&content),
            "OpenDocument text — text extracted for AI extraction".into(),
        )]);
    }
    if has("word/document.xml") {
        use std::io::Read;
        let mut f = zip
            .by_name("word/document.xml")
            .map_err(|e| anyhow!("docx: {e}"))?;
        let mut raw = Vec::new();
        f.read_to_end(&mut raw).map_err(|e| anyhow!("docx: {e}"))?;
        let text = docx_text(&raw)?;
        // A .docx that is mostly one big table should still become a table.
        if let Some(ing) = sniff_delimiter(&text)
            .filter(|(d, cols)| *d == '\t' && *cols >= 2)
            .map(|(d, cols)| Ingested {
                name: stem.to_string(),
                kind: "csv".into(),
                origin: "docx-table".into(),
                content: retab(&text, d),
                note: format!("Word table: {cols} columns"),
                rows: text
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .count()
                    .saturating_sub(1),
                columns: cols,
            })
        {
            return Ok(vec![ing]);
        }
        return Ok(vec![Ingested::text(
            stem,
            "docx",
            text,
            "Word document — text extracted for AI extraction".into(),
        )]);
    }
    Err(anyhow!(
        "unrecognized ZIP container (not xlsx/ods/docx). Export the data as CSV, JSON or XLSX."
    ))
}

/// Excel / OpenDocument workbook → one tabular source per non-empty sheet.
fn from_spreadsheet(stem: &str, bytes: &[u8]) -> Result<Vec<Ingested>> {
    use calamine::{Data, Reader};
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut wb =
        calamine::open_workbook_auto_from_rs(cursor).map_err(|e| anyhow!("spreadsheet: {e}"))?;
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
        let cell = |c: &Data| -> String {
            match c {
                Data::Empty => String::new(),
                // Excel stores dates as serial numbers; Display would emit
                // "45292" where the profiler needs to see a date.
                Data::DateTime(dt) => dt
                    .as_datetime()
                    .map(|d| {
                        let s = d.format("%Y-%m-%dT%H:%M:%S").to_string();
                        s.strip_suffix("T00:00:00").map(str::to_string).unwrap_or(s)
                    })
                    .unwrap_or_else(|| c.to_string()),
                other => other.to_string(),
            }
        };
        let mut rows_iter = range
            .rows()
            .skip_while(|r| r.iter().all(|c| cell(c).trim().is_empty()));
        let Some(header_row) = rows_iter.next() else {
            continue;
        };
        let headers: Vec<String> = header_row
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let h = cell(c).trim().to_string();
                if h.is_empty() {
                    format!("col{}", i + 1)
                } else {
                    h
                }
            })
            .collect();
        let mut csv_out = headers
            .iter()
            .map(|h| csv_field(h))
            .collect::<Vec<_>>()
            .join(",");
        csv_out.push('\n');
        let mut n = 0usize;
        for r in rows_iter {
            let values: Vec<String> = (0..headers.len())
                .map(|i| r.get(i).map(&cell).unwrap_or_default())
                .collect();
            if values.iter().all(|v| v.trim().is_empty()) {
                continue;
            }
            csv_out.push_str(
                &values
                    .iter()
                    .map(|v| csv_field(v))
                    .collect::<Vec<_>>()
                    .join(","),
            );
            csv_out.push('\n');
            n += 1;
        }
        if n == 0 {
            continue;
        }
        out.push(Ingested {
            name: if multi {
                format!("{stem}_{}", slug(&sheet))
            } else {
                stem.to_string()
            },
            kind: "csv".into(),
            origin: "xlsx".into(),
            content: csv_out,
            note: format!("sheet '{sheet}': {n} rows × {} columns", headers.len()),
            rows: n,
            columns: headers.len(),
        });
    }
    if out.is_empty() {
        return Err(anyhow!("workbook has no non-empty sheet"));
    }
    Ok(out)
}

/// `word/document.xml` → readable text: `<w:t>` runs concatenated, paragraphs
/// on `</w:p>`, table cells tab-separated so a Word table can still be sniffed
/// as a table.
fn docx_text(xml: &[u8]) -> Result<String> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    let mut out = String::new();
    let mut buf = Vec::new();
    let mut in_text = false;
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                if local_name(e.name().0) == "t" {
                    in_text = true;
                }
            }
            Ok(Event::Text(t)) => {
                if in_text {
                    out.push_str(&t.unescape().map(|c| c.to_string()).unwrap_or_default());
                }
            }
            Ok(Event::End(e)) => match local_name(e.name().0).as_str() {
                "t" => in_text = false,
                "p" => out.push('\n'),
                "tc" => out.push('\t'),
                "tr" => {
                    while out.ends_with('\t') || out.ends_with('\n') {
                        out.pop();
                    }
                    out.push('\n');
                }
                "tab" => out.push('\t'),
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => return Err(anyhow!("docx xml: {e}")),
            _ => {}
        }
        buf.clear();
    }
    // Squash the blank lines empty paragraphs leave behind.
    let lines: Vec<&str> = out.lines().map(|l| l.trim_end()).collect();
    let mut cleaned: Vec<&str> = Vec::new();
    for l in lines {
        if l.trim().is_empty() && matches!(cleaned.last(), Some(x) if x.trim().is_empty()) {
            continue;
        }
        cleaned.push(l);
    }
    Ok(cleaned.join("\n").trim().to_string())
}

/// PDF text layer. Scanned PDFs have none — say so instead of storing an empty
/// source (the fix is OCR, which is a different SenClaw server).
fn from_pdf(stem: &str, bytes: &[u8]) -> Result<Ingested> {
    let text = pdf_extract::extract_text_from_mem(bytes).map_err(|e| anyhow!("pdf: {e}"))?;
    let trimmed = text.trim();
    if trimmed.len() < 20 {
        return Err(anyhow!(
            "this PDF has no text layer (likely a scan) — run it through OCR first, then upload the text"
        ));
    }
    // A PDF whose text is really a table still deserves the tabular path.
    if let Some((d, cols)) = sniff_delimiter(trimmed).filter(|(_, c)| *c >= 3) {
        return Ok(Ingested {
            name: stem.to_string(),
            kind: "csv".into(),
            origin: "pdf-table".into(),
            content: retab(trimmed, d),
            note: format!("PDF text layer looked tabular: {cols} columns"),
            rows: trimmed
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count()
                .saturating_sub(1),
            columns: cols,
        });
    }
    Ok(Ingested::text(
        stem,
        "pdf",
        trimmed.to_string(),
        "PDF text layer extracted for AI extraction".into(),
    ))
}

// ---------------------------------------------------------------------------
// small helpers
// ---------------------------------------------------------------------------

/// Bytes → String, tolerating a BOM and non-UTF8 bytes. UTF-8 (with or without
/// a BOM) covers virtually every export we see; a lossy decode beats refusing
/// the file, and beats pulling in full charset detection.
fn decode_text(bytes: &[u8]) -> String {
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    String::from_utf8_lossy(body).to_string()
}

/// `sales report 2024.final.xlsx` → `sales_report_2024_final`.
pub fn file_stem(filename: &str) -> String {
    let base = filename.rsplit(['/', '\\']).next().unwrap_or(filename);
    let stem = match base.rsplit_once('.') {
        Some((s, ext)) if SUPPORTED.contains(&ext.to_ascii_lowercase().as_str()) => s,
        _ => base,
    };
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
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            prev_us = false;
        } else if !prev_us && !out.is_empty() {
            out.push('_');
            prev_us = true;
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(name: &str, body: &str) -> Ingested {
        ingest(name, body.as_bytes()).unwrap().remove(0)
    }

    #[test]
    fn sniffs_semicolon_csv() {
        let i = one("a.csv", "sku;price;name\nA1;10;Widget\nA2;20;Gadget\n");
        assert_eq!(i.kind, "csv");
        assert_eq!(i.columns, 3);
        assert!(i.content.starts_with("sku,price,name"), "{}", i.content);
    }

    #[test]
    fn sniffs_tsv_regardless_of_extension() {
        let i = one("weird.txt", "a\tb\tc\n1\t2\t3\n4\t5\t6\n");
        assert_eq!(i.origin, "tsv");
        assert_eq!(i.columns, 3);
    }

    #[test]
    fn flattens_nested_json_and_finds_the_record_array() {
        let body = r#"{"meta":{"page":1},"data":[
            {"id":"1","customer":{"name":"An","city":"HCM"},"tags":["vip","new"]},
            {"id":"2","customer":{"name":"Binh","city":"HN"},"tags":[]}]}"#;
        let i = one("api.json", body);
        assert_eq!(i.kind, "json");
        assert_eq!(i.rows, 2);
        let v: Value = serde_json::from_str(&i.content).unwrap();
        assert_eq!(v[0]["customer.name"], "An");
        assert_eq!(v[0]["tags"], "vip; new");
        assert!(i.note.contains("data"), "{}", i.note);
    }

    #[test]
    fn object_arrays_flatten_by_index_regardless_of_length() {
        // The one-item row must produce the same column *names* as the two-item
        // row, otherwise a mapping cannot address them.
        let body =
            r#"[{"id":"o1","items":[{"sku":"A"},{"sku":"B"}]},{"id":"o2","items":[{"sku":"C"}]}]"#;
        let i = one("orders.json", body);
        let v: Value = serde_json::from_str(&i.content).unwrap();
        assert_eq!(v[0]["items.0.sku"], "A");
        assert_eq!(v[0]["items.1.sku"], "B");
        assert_eq!(v[0]["items.count"], 2);
        assert_eq!(v[1]["items.0.sku"], "C");
        assert_eq!(v[1]["items.count"], 1);
        assert!(
            v[1].get("items").is_none(),
            "no length-dependent blob column"
        );
    }

    #[test]
    fn reads_jsonl() {
        let i = one("events.jsonl", "{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n");
        assert_eq!(i.origin, "jsonl");
        assert_eq!(i.rows, 3);
    }

    #[test]
    fn reads_xml_records() {
        let body = r#"<?xml version="1.0"?><orders><order id="1"><total>10</total></order>
            <order id="2"><total>20</total></order></orders>"#;
        let i = one("orders.xml", body);
        assert_eq!(i.origin, "xml");
        assert_eq!(i.rows, 2);
        let v: Value = serde_json::from_str(&i.content).unwrap();
        assert_eq!(v[0]["@id"], "1");
        assert_eq!(v[1]["total"], "20");
    }

    #[test]
    fn reads_yaml() {
        let i = one(
            "cfg.yaml",
            "items:\n  - name: a\n    qty: 1\n  - name: b\n    qty: 2\n",
        );
        assert_eq!(i.origin, "yaml");
        assert_eq!(i.rows, 2);
    }

    #[test]
    fn reads_markdown_table() {
        let body = "# Report\n\nsome prose\n\n| Name | Qty |\n|---|---|\n| a | 1 |\n| b | 2 |\n";
        let i = one("r.md", body);
        assert_eq!(i.origin, "markdown-table");
        assert_eq!(i.rows, 2);
        assert!(i.content.starts_with("Name,Qty"));
    }

    #[test]
    fn reads_html_table() {
        let body = "<html><body><table><tr><th>a</th><th>b</th></tr><tr><td>1</td><td>2</td></tr></table></body></html>";
        let i = one("p.html", body);
        assert_eq!(i.origin, "html-table");
        assert_eq!(i.rows, 1);
        assert_eq!(i.columns, 2);
    }

    #[test]
    fn falls_back_to_text_for_prose() {
        let i = one(
            "note.txt",
            "Công ty ABC ký hợp đồng với Công ty XYZ ngày 3 tháng 7.\nGiá trị 2 tỷ đồng.",
        );
        assert_eq!(i.kind, "text");
        assert_eq!(i.origin, "text");
    }

    #[test]
    fn html_page_without_table_becomes_text() {
        let i = one("a.html", "<html><body><script>var x=1</script><h1>Tiêu đề</h1><p>Nội dung dài hơn.</p></body></html>");
        assert_eq!(i.kind, "text");
        assert!(!i.content.contains("var x"), "{}", i.content);
        assert!(i.content.contains("Tiêu đề"));
    }

    #[test]
    fn file_stem_is_slugged() {
        assert_eq!(file_stem("/tmp/Sales Report 2024.CSV"), "sales_report_2024");
        assert_eq!(file_stem("data.tar.gz"), "data_tar_gz");
    }

    #[test]
    fn rejects_empty() {
        assert!(ingest("x.csv", b"").is_err());
    }
}
