//! Format conversions shared by the REST API, the Ant Design UI and the MCP
//! server: JSON ↔ YAML / CSV / TSV / XML, JSON Pointer queries and structural
//! diff. Deterministic, offline, no runtime dependencies.
//!
//! The browser calls these over `/api/*` rather than reimplementing them in JS,
//! so what a person sees in the UI is exactly what an agent gets from MCP.
//! Data encodings live in [`crate::codec`], analysis in [`crate::analyze`].

use quick_xml::events::Event;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

// ---------------------------------------------------------------- YAML

pub fn json_to_yaml(src: &str) -> Result<String, String> {
    let v = crate::fmt::validate(src).map_err(|e| e.to_string())?;
    serde_yaml::to_string(&v).map_err(|e| e.to_string())
}

pub fn yaml_to_json(src: &str) -> Result<Value, String> {
    serde_yaml::from_str::<Value>(src).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- CSV / TSV

/// Rows for tabular output: a JSON array of objects, or a single object
/// (treated as one row). Nested values are re-serialised as compact JSON.
fn rows_of(v: &Value) -> Result<Vec<&Map<String, Value>>, String> {
    match v {
        Value::Array(items) => items
            .iter()
            .map(|it| {
                it.as_object()
                    .ok_or_else(|| "CSV/TSV needs an array of objects".to_string())
            })
            .collect(),
        Value::Object(o) => Ok(vec![o]),
        _ => Err("CSV/TSV needs a JSON object or an array of objects".into()),
    }
}

fn cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `columns`: explicit column order; when empty the union of all keys is used,
/// sorted (object key order is not preserved by `serde_json::Value`).
pub fn json_to_delimited(src: &str, delim: u8, columns: &[String]) -> Result<String, String> {
    let v = crate::fmt::validate(src).map_err(|e| e.to_string())?;
    let rows = rows_of(&v)?;

    let headers: Vec<String> = if !columns.is_empty() {
        columns.to_vec()
    } else {
        rows.iter()
            .flat_map(|r| r.keys().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };

    let mut wtr = csv::WriterBuilder::new()
        .delimiter(delim)
        .from_writer(vec![]);
    wtr.write_record(&headers).map_err(|e| e.to_string())?;
    for row in rows {
        let record: Vec<String> = headers
            .iter()
            .map(|h| row.get(h).map(cell).unwrap_or_default())
            .collect();
        wtr.write_record(&record).map_err(|e| e.to_string())?;
    }
    let bytes = wtr.into_inner().map_err(|e| e.to_string())?;
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

/// Parse delimited text into an array of objects. Numeric and boolean cells
/// are inferred; everything else stays a string. Empty cells become null.
pub fn delimited_to_json(src: &str, delim: u8) -> Result<Value, String> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        .flexible(true)
        .from_reader(src.as_bytes());
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let mut out = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| e.to_string())?;
        let mut obj = Map::new();
        for (i, h) in headers.iter().enumerate() {
            obj.insert(h.clone(), infer(rec.get(i).unwrap_or("")));
        }
        out.push(Value::Object(obj));
    }
    Ok(Value::Array(out))
}

fn infer(raw: &str) -> Value {
    let s = raw.trim();
    if s.is_empty() {
        return Value::Null;
    }
    match s {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" => return Value::Null,
        _ => {}
    }
    // Zero-padded runs (phone numbers, zip codes, IDs) must stay text.
    if !is_zero_padded(s) {
        if let Ok(i) = s.parse::<i64>() {
            return json!(i);
        }
        // `f.is_finite()` rejects things like "1e5000", which parse to infinity
        // and cannot be represented in JSON.
        if let Ok(f) = s.parse::<f64>() {
            if f.is_finite() {
                return json!(f);
            }
        }
    }
    Value::String(raw.to_string())
}

/// "0912345678" / "-007" → true; "0", "0.5", "-0.5" → false.
fn is_zero_padded(s: &str) -> bool {
    let digits = s.strip_prefix(['-', '+']).unwrap_or(s);
    digits.starts_with('0') && digits.len() > 1 && !digits.starts_with("0.")
}

// ---------------------------------------------------------------- XML

/// JSON → XML. Keys prefixed with `@` become attributes, the key `#text`
/// becomes element text, arrays repeat the parent tag.
pub fn json_to_xml(src: &str, root: &str, indent: usize) -> Result<String, String> {
    let v = crate::fmt::validate(src).map_err(|e| e.to_string())?;
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    write_node(&mut out, root, &v, 0, indent);
    Ok(out)
}

fn write_node(out: &mut String, tag: &str, v: &Value, depth: usize, indent: usize) {
    let pad = " ".repeat(depth * indent);
    match v {
        Value::Array(items) => {
            for it in items {
                write_node(out, tag, it, depth, indent);
            }
        }
        Value::Object(obj) => {
            let mut attrs = String::new();
            for (k, val) in obj {
                if let Some(name) = k.strip_prefix('@') {
                    attrs.push_str(&format!(" {}=\"{}\"", name, escape_xml(&cell(val))));
                }
            }
            let children: Vec<(&String, &Value)> = obj
                .iter()
                .filter(|(k, _)| !k.starts_with('@') && k.as_str() != "#text")
                .collect();
            let text = obj.get("#text").map(cell);

            if children.is_empty() {
                match text {
                    Some(t) => {
                        out.push_str(&format!("{pad}<{tag}{attrs}>{}</{tag}>\n", escape_xml(&t)))
                    }
                    None => out.push_str(&format!("{pad}<{tag}{attrs}/>\n")),
                }
                return;
            }
            out.push_str(&format!("{pad}<{tag}{attrs}>\n"));
            if let Some(t) = text {
                out.push_str(&format!(
                    "{}{}\n",
                    " ".repeat((depth + 1) * indent),
                    escape_xml(&t)
                ));
            }
            for (k, val) in children {
                write_node(out, &sanitize_tag(k), val, depth + 1, indent);
            }
            out.push_str(&format!("{pad}</{tag}>\n"));
        }
        Value::Null => out.push_str(&format!("{pad}<{tag}/>\n")),
        other => out.push_str(&format!(
            "{pad}<{tag}>{}</{tag}>\n",
            escape_xml(&cell(other))
        )),
    }
}

/// XML tag names cannot start with a digit or contain spaces.
fn sanitize_tag(k: &str) -> String {
    let mut s: String = k
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == ':' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s.push('_');
    }
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// XML → JSON, in the widely used "compact" shape: attributes under `@name`,
/// text under `#text` (or the bare string when an element has nothing else),
/// repeated sibling tags collapsed into arrays.
pub fn xml_to_json(src: &str) -> Result<Value, String> {
    let mut reader = quick_xml::Reader::from_str(src);
    reader.config_mut().trim_text(true);

    // Stack of (tag, children map, text buffer).
    let mut stack: Vec<(String, Map<String, Value>, String)> =
        vec![(String::from("#document"), Map::new(), String::new())];

    loop {
        match reader.read_event() {
            Err(e) => return Err(format!("XML parse error: {e}")),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut obj = Map::new();
                push_attrs(&mut obj, &e)?;
                stack.push((name, obj, String::new()));
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut obj = Map::new();
                push_attrs(&mut obj, &e)?;
                let value = if obj.is_empty() {
                    Value::Null
                } else {
                    Value::Object(obj)
                };
                let parent = stack.last_mut().expect("root frame is never popped");
                insert_child(&mut parent.1, &name, value);
            }
            Ok(Event::End(_)) => {
                if stack.len() <= 1 {
                    return Err("XML has an unmatched closing tag".into());
                }
                let (name, obj, text) = stack.pop().expect("checked len > 1");
                let value = finish_element(obj, text);
                let parent = stack.last_mut().expect("root frame is never popped");
                insert_child(&mut parent.1, &name, value);
            }
            Ok(Event::Text(e)) => {
                let t = e.unescape().map_err(|e| e.to_string())?.to_string();
                if !t.trim().is_empty() {
                    stack
                        .last_mut()
                        .expect("root frame is never popped")
                        .2
                        .push_str(&t);
                }
            }
            Ok(Event::CData(e)) => {
                let t = String::from_utf8_lossy(&e).to_string();
                stack
                    .last_mut()
                    .expect("root frame is never popped")
                    .2
                    .push_str(&t);
            }
            _ => {}
        }
    }

    if stack.len() != 1 {
        return Err("XML has an unclosed tag".into());
    }
    Ok(Value::Object(stack.pop().expect("checked len == 1").1))
}

fn push_attrs(
    obj: &mut Map<String, Value>,
    e: &quick_xml::events::BytesStart,
) -> Result<(), String> {
    for attr in e.attributes() {
        let attr = attr.map_err(|e| e.to_string())?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        let val = attr
            .unescape_value()
            .map_err(|e| e.to_string())?
            .to_string();
        obj.insert(format!("@{key}"), Value::String(val));
    }
    Ok(())
}

fn finish_element(mut obj: Map<String, Value>, text: String) -> Value {
    let text = text.trim().to_string();
    if obj.is_empty() {
        return if text.is_empty() {
            Value::Null
        } else {
            Value::String(text)
        };
    }
    if !text.is_empty() {
        obj.insert("#text".into(), Value::String(text));
    }
    Value::Object(obj)
}

/// Second and later siblings with the same tag turn the slot into an array.
fn insert_child(parent: &mut Map<String, Value>, name: &str, value: Value) {
    match parent.get_mut(name) {
        Some(Value::Array(arr)) => arr.push(value),
        Some(existing) => {
            let prev = existing.take();
            *existing = Value::Array(vec![prev, value]);
        }
        None => {
            parent.insert(name.to_string(), value);
        }
    }
}

/// Re-indent XML in place. Unlike `xml → json → xml` this keeps the document
/// exactly as authored (attribute order, empty-element syntax, text nodes) and
/// only fixes whitespace.
pub fn xml_pretty(src: &str, indent: usize) -> Result<String, String> {
    let mut reader = quick_xml::Reader::from_str(src);
    reader.config_mut().trim_text(true);
    let mut writer = quick_xml::Writer::new_with_indent(Vec::new(), b' ', indent.max(1));
    let mut depth: i32 = 0;

    loop {
        match reader.read_event() {
            Err(e) => return Err(format!("XML parse error: {e}")),
            Ok(Event::Eof) => break,
            Ok(event) => {
                match &event {
                    Event::Start(_) => depth += 1,
                    Event::End(_) => depth -= 1,
                    _ => {}
                }
                writer
                    .write_event(event)
                    .map_err(|e| format!("XML write error: {e}"))?;
            }
        }
    }
    if depth != 0 {
        return Err("XML có thẻ chưa được đóng".into());
    }
    String::from_utf8(writer.into_inner()).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- query

/// Accepts an RFC-6901 JSON Pointer (`/a/b/0`) or a dotted path
/// (`a.b[0]`, `$.a.b[0]`) and returns the pointer form.
pub fn to_pointer(path: &str) -> String {
    let p = path.trim();
    if p.is_empty() || p == "$" {
        return String::new();
    }
    if p.starts_with('/') {
        return p.to_string();
    }
    let p = p
        .strip_prefix("$.")
        .or_else(|| p.strip_prefix('$'))
        .unwrap_or(p);
    let mut out = String::new();
    for seg in p.replace('[', ".").replace(']', "").split('.') {
        if seg.is_empty() {
            continue;
        }
        out.push('/');
        out.push_str(&seg.replace('~', "~0").replace('/', "~1"));
    }
    out
}

pub fn query(src: &str, path: &str) -> Result<Value, String> {
    let v = crate::fmt::validate(src).map_err(|e| e.to_string())?;
    let ptr = to_pointer(path);
    if ptr.is_empty() {
        return Ok(v);
    }
    v.pointer(&ptr)
        .cloned()
        .ok_or_else(|| format!("no value at path `{path}` (pointer `{ptr}`)"))
}

// ---------------------------------------------------------------- dispatch

pub const FORMATS: [&str; 5] = ["json", "yaml", "csv", "tsv", "xml"];

/// Convert `input` between any two of [`FORMATS`], always via JSON.
/// `root` names the XML root element, `columns` pins CSV/TSV column order.
pub fn convert(
    from: &str,
    to: &str,
    input: &str,
    root: &str,
    columns: &[String],
    indent: usize,
) -> Result<String, String> {
    let from = from.trim().to_lowercase();
    let to = to.trim().to_lowercase();
    for f in [&from, &to] {
        if !FORMATS.contains(&f.as_str()) {
            return Err(format!(
                "unsupported format `{f}` — expected one of {}",
                FORMATS.join(", ")
            ));
        }
    }

    // Same-format requests are re-indentations, not conversions: routing them
    // through JSON would sort object keys / drop XML authoring details.
    if from == "xml" && to == "xml" {
        return xml_pretty(input, indent);
    }

    // json → json keeps the original text so key order survives re-indenting.
    let as_json: String = match from.as_str() {
        "json" => {
            crate::fmt::validate(input).map_err(|e| e.to_string())?;
            input.to_string()
        }
        "yaml" => yaml_to_json(input)?.to_string(),
        "csv" => delimited_to_json(input, b',')?.to_string(),
        "tsv" => delimited_to_json(input, b'\t')?.to_string(),
        "xml" => xml_to_json(input)?.to_string(),
        _ => unreachable!("format was validated above"),
    };

    match to.as_str() {
        "json" => crate::fmt::pretty(&as_json, indent).map_err(|e| e.to_string()),
        "yaml" => json_to_yaml(&as_json),
        "csv" => json_to_delimited(&as_json, b',', columns),
        "tsv" => json_to_delimited(&as_json, b'\t', columns),
        "xml" => json_to_xml(&as_json, root, indent),
        _ => unreachable!("format was validated above"),
    }
}

// ---------------------------------------------------------------- diff

/// Structural diff: a flat list of `{path, op, left, right}` entries, where
/// `op` is one of `added` / `removed` / `changed`.
pub fn diff(left: &str, right: &str) -> Result<Value, String> {
    let a = crate::fmt::validate(left).map_err(|e| format!("left: {e}"))?;
    let b = crate::fmt::validate(right).map_err(|e| format!("right: {e}"))?;
    let mut changes = Vec::new();
    walk_diff("", &a, &b, &mut changes);
    Ok(json!({
        "equal": changes.is_empty(),
        "count": changes.len(),
        "changes": changes,
    }))
}

fn walk_diff(path: &str, a: &Value, b: &Value, out: &mut Vec<Value>) {
    let here = if path.is_empty() { "/" } else { path };
    match (a, b) {
        (Value::Object(ao), Value::Object(bo)) => {
            let keys: BTreeSet<&String> = ao.keys().chain(bo.keys()).collect();
            for k in keys {
                let child = format!("{}/{}", path, k);
                match (ao.get(k), bo.get(k)) {
                    (Some(x), Some(y)) => walk_diff(&child, x, y, out),
                    (Some(x), None) => {
                        out.push(json!({ "path": child, "op": "removed", "left": x }))
                    }
                    (None, Some(y)) => {
                        out.push(json!({ "path": child, "op": "added", "right": y }))
                    }
                    (None, None) => {}
                }
            }
        }
        (Value::Array(aa), Value::Array(ba)) => {
            for i in 0..aa.len().max(ba.len()) {
                let child = format!("{}/{}", path, i);
                match (aa.get(i), ba.get(i)) {
                    (Some(x), Some(y)) => walk_diff(&child, x, y, out),
                    (Some(x), None) => {
                        out.push(json!({ "path": child, "op": "removed", "left": x }))
                    }
                    (None, Some(y)) => {
                        out.push(json!({ "path": child, "op": "added", "right": y }))
                    }
                    (None, None) => {}
                }
            }
        }
        (x, y) if x != y => {
            out.push(json!({ "path": here, "op": "changed", "left": x, "right": y }))
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_round_trip() {
        let y = json_to_yaml(r#"{"a":1,"b":["x","y"]}"#).unwrap();
        assert!(y.contains("a: 1"));
        let back = yaml_to_json(&y).unwrap();
        assert_eq!(back, json!({"a": 1, "b": ["x", "y"]}));
    }

    #[test]
    fn csv_round_trip_with_ragged_rows() {
        let csv = json_to_delimited(r#"[{"a":1,"b":"x"},{"a":2}]"#, b',', &[]).unwrap();
        assert_eq!(csv, "a,b\n1,x\n2,\n");
        let back = delimited_to_json(&csv, b',').unwrap();
        assert_eq!(back, json!([{"a": 1, "b": "x"}, {"a": 2, "b": null}]));
    }

    #[test]
    fn csv_quotes_embedded_delimiters() {
        let csv = json_to_delimited(r#"[{"a":"x,y","b":"say \"hi\""}]"#, b',', &[]).unwrap();
        assert_eq!(csv, "a,b\n\"x,y\",\"say \"\"hi\"\"\"\n");
    }

    #[test]
    fn tsv_uses_tabs_and_explicit_columns() {
        let tsv = json_to_delimited(
            r#"[{"a":1,"b":2}]"#,
            b'\t',
            &["b".to_string(), "a".to_string()],
        )
        .unwrap();
        assert_eq!(tsv, "b\ta\n2\t1\n");
    }

    #[test]
    fn csv_keeps_leading_zeros_as_text() {
        let v = delimited_to_json("phone,n\n0912345678,42\n", b',').unwrap();
        assert_eq!(v[0]["phone"], json!("0912345678"));
        assert_eq!(v[0]["n"], json!(42));
    }

    #[test]
    fn xml_round_trip() {
        // r##…##: the payload contains `"#text`, which would close an r#…# string.
        let xml = json_to_xml(
            r##"{"item":[{"@id":"1","#text":"one"},{"@id":"2"}]}"##,
            "root",
            2,
        )
        .unwrap();
        assert!(xml.contains("<item id=\"1\">one</item>"));
        let back = xml_to_json(&xml).unwrap();
        assert_eq!(back["root"]["item"][0]["@id"], json!("1"));
        assert_eq!(back["root"]["item"][0]["#text"], json!("one"));
        assert_eq!(back["root"]["item"][1]["@id"], json!("2"));
    }

    #[test]
    fn xml_escapes_and_sanitizes() {
        let xml = json_to_xml(r#"{"2bad key":"a & b < c"}"#, "root", 2).unwrap();
        assert!(
            xml.contains("<_2bad_key>a &amp; b &lt; c</_2bad_key>"),
            "{xml}"
        );
        let back = xml_to_json(&xml).unwrap();
        assert_eq!(back["root"]["_2bad_key"], json!("a & b < c"));
    }

    #[test]
    fn xml_rejects_malformed_input() {
        assert!(xml_to_json("<a><b></a>").is_err());
    }

    #[test]
    fn xml_to_xml_reindents_in_place() {
        let out = convert("xml", "xml", "<a><b x=\"1\">t</b><b/></a>", "root", &[], 2).unwrap();
        assert_eq!(out, "<a>\n  <b x=\"1\">t</b>\n  <b/>\n</a>");
        assert!(convert("xml", "xml", "<a><b></a>", "root", &[], 2).is_err());
    }

    #[test]
    fn pointer_forms() {
        assert_eq!(to_pointer("a.b[0].c"), "/a/b/0/c");
        assert_eq!(to_pointer("$.a.b"), "/a/b");
        assert_eq!(to_pointer("/a/b"), "/a/b");
        assert_eq!(to_pointer("$"), "");
        let doc = r#"{"a":{"b":[10,20]}}"#;
        assert_eq!(query(doc, "a.b[1]").unwrap(), json!(20));
        assert!(query(doc, "a.z").is_err());
    }

    #[test]
    fn diff_reports_each_op() {
        let d = diff(r#"{"a":1,"b":2,"c":[1,2]}"#, r#"{"a":9,"c":[1,2,3],"d":4}"#).unwrap();
        assert_eq!(d["equal"], json!(false));
        let ops: Vec<(String, String)> = d["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                (
                    c["path"].as_str().unwrap().to_string(),
                    c["op"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        assert!(ops.contains(&("/a".into(), "changed".into())));
        assert!(ops.contains(&("/b".into(), "removed".into())));
        assert!(ops.contains(&("/c/2".into(), "added".into())));
        assert!(ops.contains(&("/d".into(), "added".into())));
        assert_eq!(ops.len(), 4);
    }

    #[test]
    fn convert_dispatches_across_formats() {
        let csv = convert("json", "csv", r#"[{"a":1,"b":2}]"#, "root", &[], 2).unwrap();
        assert_eq!(csv, "a,b\n1,2\n");
        let yaml = convert("csv", "yaml", &csv, "root", &[], 2).unwrap();
        assert!(yaml.contains("a: 1"), "{yaml}");
        let xml = convert("yaml", "xml", &yaml, "rows", &[], 2).unwrap();
        assert!(xml.contains("<rows>"), "{xml}");
        assert!(convert("json", "avro", "{}", "root", &[], 2).is_err());
    }

    #[test]
    fn convert_json_to_json_reindents_without_reordering() {
        let out = convert("json", "json", r#"{"z":1,"a":2}"#, "root", &[], 4).unwrap();
        assert_eq!(out, "{\n    \"z\": 1,\n    \"a\": 2\n}");
    }

    #[test]
    fn diff_equal_documents() {
        let d = diff(r#"{"a":[1,{"b":2}]}"#, r#"{ "a" : [1, {"b":2}] }"#).unwrap();
        assert_eq!(d["equal"], json!(true));
        assert_eq!(d["count"], json!(0));
    }
}
