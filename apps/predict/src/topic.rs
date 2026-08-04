//! Generic, user-configurable prediction topics — the "form chung" layer.
//! A topic defines a small field schema; records are JSON rows entered by
//! form, imported (CSV/JSON) or searched; the AI derives rules from the
//! accumulated history and answers "will X happen?" with a ledgered
//! probability. Pure helpers here (validation, CSV, loose JSON extraction) —
//! storage lives in `db.rs`, LLM calls in `llm.rs`.

use serde_json::{json, Map, Value};

/// One schema field of a topic. `kind`: text | number | date | bool.
#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub kind: String,
}

pub fn parse_fields(v: &Value) -> Vec<Field> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| {
                    let name = f["name"].as_str()?.trim().to_string();
                    if name.is_empty() {
                        return None;
                    }
                    let kind = match f["kind"]
                        .as_str()
                        .or_else(|| f["type"].as_str())
                        .unwrap_or("text")
                    {
                        "number" => "number",
                        "date" => "date",
                        "bool" => "bool",
                        _ => "text",
                    };
                    Some(Field {
                        name,
                        kind: kind.into(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn fields_json(fields: &[Field]) -> Value {
    Value::Array(
        fields
            .iter()
            .map(|f| json!({ "name": f.name, "kind": f.kind }))
            .collect(),
    )
}

/// Chuẩn hoá **cấu hình tĩnh** của chủ đề: map `{tên: giá trị}` cố định (vị trí,
/// thành phố, thông số không đổi theo thời gian). Nhận cả dạng mảng
/// `[{name, value}]` từ UI. Bỏ khoá rỗng; giá trị ép về chuỗi.
pub fn parse_static(v: &Value) -> Value {
    let mut out = Map::new();
    let mut put = |k: &str, val: &Value| {
        let k = k.trim();
        if k.is_empty() {
            return;
        }
        let s = match val {
            Value::String(s) => s.trim().to_string(),
            Value::Null => String::new(),
            other => other.to_string(),
        };
        if !s.is_empty() {
            out.insert(k.to_string(), json!(s));
        }
    };
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                put(k, val);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                if let Some(name) = item["name"].as_str() {
                    put(name, &item["value"]);
                }
            }
        }
        _ => {}
    }
    Value::Object(out)
}

/// Validate + coerce one record against the topic schema. Unknown keys are
/// kept (schema is a guide, not a cage); typed fields must parse.
pub fn validate_record(fields: &[Field], data: &Value) -> Result<Value, String> {
    let Some(obj) = data.as_object() else {
        return Err("bản ghi phải là một object {trường: giá trị}".into());
    };
    let mut out = Map::new();
    for (k, v) in obj {
        let kind = fields
            .iter()
            .find(|f| &f.name == k)
            .map(|f| f.kind.as_str())
            .unwrap_or("text");
        let coerced = match kind {
            "number" => match v {
                Value::Number(_) => v.clone(),
                Value::String(s) => s
                    .trim()
                    .replace(',', ".")
                    .parse::<f64>()
                    .map(|n| json!(n))
                    .map_err(|_| format!("trường '{k}' phải là số (nhận '{s}')"))?,
                _ => return Err(format!("trường '{k}' phải là số")),
            },
            "bool" => match v {
                Value::Bool(_) => v.clone(),
                Value::String(s) => {
                    let t = s.trim().to_lowercase();
                    json!(matches!(
                        t.as_str(),
                        "true" | "1" | "yes" | "có" | "co" | "x"
                    ))
                }
                Value::Number(n) => json!(n.as_f64().unwrap_or(0.0) != 0.0),
                _ => return Err(format!("trường '{k}' phải là bool")),
            },
            "date" => match v.as_str() {
                Some(s) if crate::timeutil::parse_date_days(s.trim()).is_some() => json!(s.trim()),
                _ => return Err(format!("trường '{k}' phải là ngày YYYY-MM-DD")),
            },
            _ => v.clone(),
        };
        out.insert(k.clone(), coerced);
    }
    if out.is_empty() {
        return Err("bản ghi rỗng".into());
    }
    Ok(Value::Object(out))
}

/// Minimal quote-aware CSV split for one line.
fn split_csv_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                out.push(cur.trim().to_string());
                cur = String::new();
            }
            _ => cur.push(c),
        }
    }
    out.push(cur.trim().to_string());
    out
}

/// Parse a CSV blob (first line = header mapped to field names) into records.
/// Returns (records, per-line errors).
pub fn parse_csv_records(fields: &[Field], csv: &str) -> (Vec<Value>, Vec<String>) {
    let mut lines = csv.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return (vec![], vec!["CSV rỗng".into()]);
    };
    let cols = split_csv_line(header);
    let mut records = Vec::new();
    let mut errors = Vec::new();
    for (i, line) in lines.enumerate() {
        let vals = split_csv_line(line);
        let mut obj = Map::new();
        for (c, v) in cols.iter().zip(vals.iter()) {
            if !c.is_empty() {
                obj.insert(c.clone(), json!(v));
            }
        }
        match validate_record(fields, &Value::Object(obj)) {
            Ok(r) => records.push(r),
            Err(e) => errors.push(format!("dòng {}: {e}", i + 2)),
        }
    }
    (records, errors)
}

/// Extract the first JSON object/array from an LLM reply (handles ```json
/// fences and surrounding prose). None when nothing parses.
pub fn extract_json(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return Some(v);
    }
    // Fenced block first.
    if let Some(start) = t.find("```") {
        let inner = &t[start + 3..];
        let inner = inner.strip_prefix("json").unwrap_or(inner);
        if let Some(end) = inner.find("```") {
            if let Ok(v) = serde_json::from_str::<Value>(inner[..end].trim()) {
                return Some(v);
            }
        }
    }
    // First balanced {...} or [...] scan — whichever bracket appears first
    // (an array of objects must yield the array, not its first element).
    let mut opens = ['{', '['];
    if t.find('[').unwrap_or(usize::MAX) < t.find('{').unwrap_or(usize::MAX) {
        opens = ['[', '{'];
    }
    for open in opens {
        let close = if open == '{' { '}' } else { ']' };
        if let Some(s) = t.find(open) {
            let mut depth = 0i32;
            let mut in_str = false;
            let mut prev = '\0';
            for (i, c) in t[s..].char_indices() {
                match c {
                    '"' if prev != '\\' => in_str = !in_str,
                    c if c == open && !in_str => depth += 1,
                    c if c == close && !in_str => {
                        depth -= 1;
                        if depth == 0 {
                            if let Ok(v) = serde_json::from_str::<Value>(&t[s..s + i + 1]) {
                                return Some(v);
                            }
                            break;
                        }
                    }
                    _ => {}
                }
                prev = c;
            }
        }
    }
    None
}

/// Per-numeric-field summary over records (nền tảng dữ liệu → outside view):
/// count/min/max/mean/latest for every `number` field, plus true-share for
/// every `bool` field. `records` are `{data: {...}}` rows, newest first.
pub fn numeric_summary(fields: &[Field], records: &[serde_json::Value]) -> serde_json::Value {
    let mut out = Map::new();
    for f in fields {
        match f.kind.as_str() {
            "number" => {
                let vals: Vec<f64> = records
                    .iter()
                    .filter_map(|r| r["data"][&f.name].as_f64())
                    .collect();
                if vals.is_empty() {
                    continue;
                }
                let sum: f64 = vals.iter().sum();
                let (mut lo, mut hi) = (vals[0], vals[0]);
                for v in &vals {
                    lo = lo.min(*v);
                    hi = hi.max(*v);
                }
                let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
                out.insert(
                    f.name.clone(),
                    json!({
                        "count": vals.len(), "min": r3(lo), "max": r3(hi),
                        "mean": r3(sum / vals.len() as f64), "latest": r3(vals[0]),
                    }),
                );
            }
            "bool" => {
                let vals: Vec<bool> = records
                    .iter()
                    .filter_map(|r| r["data"][&f.name].as_bool())
                    .collect();
                if vals.is_empty() {
                    continue;
                }
                let t = vals.iter().filter(|b| **b).count();
                out.insert(
                    f.name.clone(),
                    json!({
                        "count": vals.len(),
                        "true_share": ((t as f64 / vals.len() as f64) * 1000.0).round() / 1000.0,
                    }),
                );
            }
            _ => {}
        }
    }
    serde_json::Value::Object(out)
}

/// Time series per numeric field, keyed by the topic's first `date` field:
/// `{field: [[date, value], …]}` ascending by date. Records are `{data}` rows
/// (any order); rows missing either side are skipped.
pub fn series_by_date(fields: &[Field], records: &[serde_json::Value]) -> serde_json::Value {
    let Some(date_field) = fields
        .iter()
        .find(|f| f.kind == "date")
        .map(|f| f.name.clone())
    else {
        return json!({});
    };
    let mut out = Map::new();
    for f in fields.iter().filter(|f| f.kind == "number") {
        let mut pts: Vec<(String, f64)> = records
            .iter()
            .filter_map(|r| {
                let d = r["data"][&date_field].as_str()?.to_string();
                let v = r["data"][&f.name].as_f64()?;
                Some((d, v))
            })
            .collect();
        pts.sort_by(|a, b| a.0.cmp(&b.0));
        pts.dedup_by(|a, b| a.0 == b.0);
        if !pts.is_empty() {
            out.insert(
                f.name.clone(),
                serde_json::Value::Array(pts.into_iter().map(|(d, v)| json!([d, v])).collect()),
            );
        }
    }
    serde_json::Value::Object(out)
}

/// Slug for ledger domain: "topic:<lowercase-alnum-dashes>".
pub fn ledger_domain(name: &str) -> String {
    let slug: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    format!(
        "topic:{}",
        if slug.is_empty() {
            "chung".into()
        } else {
            slug
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Vec<Field> {
        parse_fields(&json!([
            { "name": "ngày", "kind": "date" },
            { "name": "giá", "kind": "number" },
            { "name": "tăng", "kind": "bool" },
            { "name": "ghi chú" }
        ]))
    }

    #[test]
    fn parse_fields_kinds() {
        let f = fields();
        assert_eq!(f.len(), 4);
        assert_eq!(f[0].kind, "date");
        assert_eq!(f[3].kind, "text");
        // roundtrip
        assert_eq!(parse_fields(&fields_json(&f)).len(), 4);
    }

    #[test]
    fn static_config_parsing() {
        // Dạng object
        let m =
            parse_static(&json!({ "vị trí": " Đà Lạt ", "độ cao": 1500, "rỗng": "", " ": "x" }));
        assert_eq!(m["vị trí"], "Đà Lạt");
        assert_eq!(m["độ cao"], "1500");
        assert!(m.get("rỗng").is_none() && m.get(" ").is_none());
        // Dạng mảng {name, value} từ UI
        let a = parse_static(&json!([
            { "name": "thành phố", "value": "Nha Trang" },
            { "name": "", "value": "bỏ" },
            { "name": "ghi chú", "value": "vùng biển" },
        ]));
        assert_eq!(a.as_object().unwrap().len(), 2);
        assert_eq!(a["thành phố"], "Nha Trang");
        assert_eq!(parse_static(&json!("rác")), json!({}));
    }

    #[test]
    fn validate_coerces_types() {
        let f = fields();
        let ok = validate_record(
            &f,
            &json!({ "ngày": "2026-07-27", "giá": "12,5", "tăng": "có", "ghi chú": "test" }),
        )
        .unwrap();
        assert_eq!(ok["giá"], 12.5);
        assert_eq!(ok["tăng"], true);
        assert!(validate_record(&f, &json!({ "giá": "abc" })).is_err());
        assert!(validate_record(&f, &json!({ "ngày": "27/07/2026" })).is_err());
        assert!(validate_record(&f, &json!({})).is_err());
        // Unknown key kept as text.
        let extra = validate_record(&f, &json!({ "khác": 5 })).unwrap();
        assert_eq!(extra["khác"], 5);
    }

    #[test]
    fn csv_import() {
        let f = fields();
        let csv = "ngày,giá,tăng,ghi chú\n2026-07-26,100,1,\"mở cửa, tốt\"\n2026-07-27,xx,0,hỏng\n2026-07-28,101.5,có,ok\n";
        let (records, errors) = parse_csv_records(&f, csv);
        assert_eq!(records.len(), 2);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("dòng 3"));
        assert_eq!(records[0]["ghi chú"], "mở cửa, tốt");
        assert_eq!(records[1]["giá"], 101.5);
    }

    #[test]
    fn extract_json_variants() {
        assert_eq!(extract_json("{\"p\": 0.7}").unwrap()["p"], 0.7);
        assert_eq!(
            extract_json("Kết quả:\n```json\n{\"p\": 0.6}\n```\nhết").unwrap()["p"],
            0.6
        );
        assert_eq!(
            extract_json("nói dài dòng {\"p\": 0.5, \"lý do\": \"a {b}\"} xong").unwrap()["p"],
            0.5
        );
        let arr = extract_json("rules: [{\"rule\": \"x\", \"confidence\": 0.8}] done").unwrap();
        assert_eq!(arr[0]["confidence"], 0.8);
        assert!(extract_json("không có json").is_none());
    }

    #[test]
    fn series_by_date_sorted_and_deduped() {
        let f = fields();
        let records = vec![
            json!({ "data": { "ngày": "2026-07-27", "giá": 123.0 } }),
            json!({ "data": { "ngày": "2026-07-25", "giá": 122.0 } }),
            json!({ "data": { "ngày": "2026-07-27", "giá": 999.0 } }), // dup date dropped
            json!({ "data": { "giá": 5.0 } }),                         // no date skipped
        ];
        let s = series_by_date(&f, &records);
        let pts = s["giá"].as_array().unwrap();
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0][0], "2026-07-25");
        assert_eq!(pts[1][0], "2026-07-27");
        // No date field in schema → empty.
        let no_date = parse_fields(&json!([{ "name": "x", "kind": "number" }]));
        assert_eq!(series_by_date(&no_date, &records), json!({}));
    }

    #[test]
    fn numeric_summary_stats() {
        let f = fields();
        let records = vec![
            json!({ "data": { "giá": 123.0, "tăng": false } }),
            json!({ "data": { "giá": 124.0, "tăng": true } }),
            json!({ "data": { "giá": 118.0, "tăng": true } }),
            json!({ "data": { "ghi chú": "no numbers" } }),
        ];
        let s = numeric_summary(&f, &records);
        assert_eq!(s["giá"]["count"], 3);
        assert_eq!(s["giá"]["min"], 118.0);
        assert_eq!(s["giá"]["max"], 124.0);
        assert_eq!(s["giá"]["latest"], 123.0);
        assert!((s["giá"]["mean"].as_f64().unwrap() - 121.667).abs() < 0.001);
        assert_eq!(s["tăng"]["true_share"], 0.667);
        assert!(s.get("ngày").is_none());
    }

    #[test]
    fn ledger_domain_slug() {
        assert_eq!(ledger_domain("Giá Vàng SJC"), "topic:giá-vàng-sjc");
        assert_eq!(ledger_domain("  "), "topic:chung");
    }
}
