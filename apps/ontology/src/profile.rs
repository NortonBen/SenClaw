//! Stage [1] Profiling. Read a CSV or JSON-array source and infer, per column:
//! inferred datatype, null ratio, distinct count, whether it is a candidate key
//! (unique) or an enum (few distinct values), and a heuristic ontology *role*
//! (identifier / relation / attribute / enum). The LLM can refine these roles
//! later, but the deterministic pass gives a solid default.

use anyhow::{anyhow, Result};
use serde::Serialize;
use std::collections::HashSet;

/// One source parsed into headers + string rows.
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// Row as a `col -> value` map (for the mapping interpreter).
    pub fn row_map(&self, row: &[String]) -> std::collections::HashMap<String, String> {
        self.headers
            .iter()
            .cloned()
            .zip(row.iter().cloned())
            .collect()
    }
}

/// Parse CSV text into a `Table`.
pub fn parse_csv(content: &str) -> Result<Table> {
    let mut rdr = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(content.as_bytes());
    let headers = rdr
        .headers()
        .map_err(|e| anyhow!("csv header: {e}"))?
        .iter()
        .map(|s| s.trim().to_string())
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for rec in rdr.records() {
        let rec = rec.map_err(|e| anyhow!("csv row: {e}"))?;
        rows.push(rec.iter().map(|s| s.to_string()).collect());
    }
    Ok(Table { headers, rows })
}

/// Parse a JSON array of flat objects into a `Table` (union of keys as headers).
pub fn parse_json(content: &str) -> Result<Table> {
    let v: serde_json::Value = serde_json::from_str(content).map_err(|e| anyhow!("json: {e}"))?;
    let arr = v.as_array().ok_or_else(|| anyhow!("expected a JSON array of objects"))?;
    let mut headers: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for item in arr {
        if let Some(obj) = item.as_object() {
            for k in obj.keys() {
                if seen.insert(k.clone()) {
                    headers.push(k.clone());
                }
            }
        }
    }
    let mut rows = Vec::new();
    for item in arr {
        let obj = item.as_object();
        let row = headers
            .iter()
            .map(|h| match obj.and_then(|o| o.get(h)) {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Null) | None => String::new(),
                Some(other) => other.to_string(),
            })
            .collect();
        rows.push(row);
    }
    Ok(Table { headers, rows })
}

pub fn parse(kind: &str, content: &str) -> Result<Table> {
    match kind {
        "json" => parse_json(content),
        _ => parse_csv(content),
    }
}

#[derive(Serialize, Clone)]
pub struct ColumnProfile {
    pub name: String,
    /// integer | decimal | boolean | date | string
    pub datatype: String,
    /// full XSD IRI suggestion for a data property
    #[serde(rename = "xsdDatatype")]
    pub xsd_datatype: String,
    #[serde(rename = "nullRatio")]
    pub null_ratio: f64,
    #[serde(rename = "distinctCount")]
    pub distinct_count: usize,
    #[serde(rename = "isUnique")]
    pub is_unique: bool,
    #[serde(rename = "isEnum")]
    pub is_enum: bool,
    /// identifier | relation | attribute | enum
    pub role: String,
    pub samples: Vec<String>,
}

fn classify_value(v: &str) -> &'static str {
    let t = v.trim();
    if t.is_empty() {
        return "empty";
    }
    if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false") {
        return "boolean";
    }
    if t.parse::<i64>().is_ok() {
        return "integer";
    }
    if t.parse::<f64>().is_ok() {
        return "decimal";
    }
    // yyyy-mm-dd (very light date sniff)
    let b = t.as_bytes();
    if t.len() >= 8
        && b.iter().take(4).all(|c| c.is_ascii_digit())
        && (t.contains('-') || t.contains('/'))
        && t.chars().filter(|c| c.is_ascii_digit()).count() >= 6
    {
        return "date";
    }
    "string"
}

fn xsd_for(dt: &str) -> String {
    let local = match dt {
        "integer" => "integer",
        "decimal" => "decimal",
        "boolean" => "boolean",
        "date" => "date",
        _ => "string",
    };
    format!("{}{}", crate::vocab::XSD, local)
}

/// Profile every column of a table.
pub fn profile(table: &Table) -> Vec<ColumnProfile> {
    let n = table.rows.len().max(1);
    let mut out = Vec::new();
    for (ci, name) in table.headers.iter().enumerate() {
        let mut nulls = 0usize;
        let mut distinct: HashSet<String> = HashSet::new();
        let mut samples: Vec<String> = Vec::new();
        let mut counts = std::collections::HashMap::<&'static str, usize>::new();
        for row in &table.rows {
            let val = row.get(ci).map(|s| s.as_str()).unwrap_or("");
            if val.trim().is_empty() {
                nulls += 1;
                continue;
            }
            distinct.insert(val.to_string());
            if samples.len() < 5 && !samples.iter().any(|s| s == val) {
                samples.push(val.to_string());
            }
            *counts.entry(classify_value(val)).or_insert(0) += 1;
        }
        let non_null = table.rows.len() - nulls;
        // Dominant non-empty datatype.
        let datatype = counts
            .iter()
            .max_by_key(|(_, c)| **c)
            .map(|(k, _)| *k)
            .unwrap_or("string")
            .to_string();
        let distinct_count = distinct.len();
        let is_unique = non_null > 1 && distinct_count == non_null;
        let is_enum = !is_unique
            && distinct_count > 0
            && distinct_count <= 20
            && (distinct_count as f64) / (non_null.max(1) as f64) <= 0.2
            && datatype == "string";
        let lname = name.to_lowercase();
        let looks_id = lname == "id"
            || lname.ends_with("_id")
            || lname.ends_with("id")
            || lname.contains("sku")
            || lname.contains("code")
            || lname.contains("mst")
            || lname.contains("uuid");
        let looks_fk = (lname.ends_with("_id") || lname.ends_with("id")) && !is_unique && lname != "id";
        let role = if is_unique && looks_id {
            "identifier"
        } else if looks_fk {
            "relation"
        } else if is_enum {
            "enum"
        } else if is_unique {
            "identifier"
        } else {
            "attribute"
        }
        .to_string();
        out.push(ColumnProfile {
            name: name.clone(),
            datatype: datatype.clone(),
            xsd_datatype: xsd_for(&datatype),
            null_ratio: (nulls as f64 / n as f64 * 1000.0).round() / 1000.0,
            distinct_count,
            is_unique,
            is_enum,
            role,
            samples,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_csv() {
        // 10 rows so the enum heuristic (few distinct relative to row count) fires.
        let mut csv = String::from("sku,price,name,status\n");
        for i in 0..10 {
            let st = if i % 5 == 0 { "retired" } else { "active" };
            csv.push_str(&format!("A{i},{},Item{i},{st}\n", 100000 - i * 1000));
        }
        let t = parse_csv(&csv).unwrap();
        let p = profile(&t);
        let sku = p.iter().find(|c| c.name == "sku").unwrap();
        assert!(sku.is_unique);
        assert_eq!(sku.role, "identifier");
        let price = p.iter().find(|c| c.name == "price").unwrap();
        assert_eq!(price.datatype, "integer");
        let status = p.iter().find(|c| c.name == "status").unwrap();
        assert!(status.is_enum);
        assert_eq!(status.role, "enum");
    }

    #[test]
    fn parses_json() {
        let j = r#"[{"a":"1","b":"x"},{"a":"2"}]"#;
        let t = parse_json(j).unwrap();
        assert_eq!(t.headers, vec!["a", "b"]);
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[1][1], "");
    }
}
