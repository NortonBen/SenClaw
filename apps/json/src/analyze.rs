//! Document analysis: cheap structural stats and JSON Schema inference.
//! Both exist so an agent can size up a large document (or hand a schema to a
//! downstream tool) without paging the whole thing into its context.

use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

/// Structural summary of a JSON document.
pub fn stats(src: &str) -> Result<Value, String> {
    let v = crate::fmt::validate(src).map_err(|e| e.to_string())?;
    let mut c = Counts::default();
    c.walk(&v, 1);

    let top_level = match &v {
        Value::Object(o) => json!({ "kind": "object", "keys": o.keys().collect::<Vec<_>>() }),
        Value::Array(a) => json!({ "kind": "array", "length": a.len() }),
        other => json!({ "kind": crate::api::type_name(other) }),
    };

    Ok(json!({
        "bytes": src.len(),
        "lines": src.lines().count().max(1),
        "root_type": crate::api::type_name(&v),
        "max_depth": c.depth,
        "nodes": c.nodes,
        "counts": {
            "objects": c.objects,
            "arrays": c.arrays,
            "strings": c.strings,
            "numbers": c.numbers,
            "booleans": c.booleans,
            "nulls": c.nulls,
        },
        "object_keys": { "total": c.keys_total, "unique": c.keys_unique.len() },
        "largest_array": c.largest_array,
        "longest_string": c.longest_string,
        "top_level": top_level,
    }))
}

#[derive(Default)]
struct Counts {
    depth: usize,
    nodes: usize,
    objects: usize,
    arrays: usize,
    strings: usize,
    numbers: usize,
    booleans: usize,
    nulls: usize,
    keys_total: usize,
    keys_unique: BTreeSet<String>,
    largest_array: usize,
    longest_string: usize,
}

impl Counts {
    fn walk(&mut self, v: &Value, depth: usize) {
        self.nodes += 1;
        self.depth = self.depth.max(depth);
        match v {
            Value::Object(o) => {
                self.objects += 1;
                self.keys_total += o.len();
                for (k, val) in o {
                    self.keys_unique.insert(k.clone());
                    self.walk(val, depth + 1);
                }
            }
            Value::Array(a) => {
                self.arrays += 1;
                self.largest_array = self.largest_array.max(a.len());
                for item in a {
                    self.walk(item, depth + 1);
                }
            }
            Value::String(s) => {
                self.strings += 1;
                self.longest_string = self.longest_string.max(s.chars().count());
            }
            Value::Number(_) => self.numbers += 1,
            Value::Bool(_) => self.booleans += 1,
            Value::Null => self.nulls += 1,
        }
    }
}

/// Infer a JSON Schema (draft-07) from a sample document. Array items are
/// merged across every element, so `required` only lists keys present in all
/// of them — the useful, conservative reading of a sample.
pub fn infer_schema(src: &str) -> Result<Value, String> {
    let v = crate::fmt::validate(src).map_err(|e| e.to_string())?;
    let mut schema = infer(&v);
    if let Some(obj) = schema.as_object_mut() {
        obj.insert(
            "$schema".into(),
            json!("http://json-schema.org/draft-07/schema#"),
        );
    }
    Ok(schema)
}

fn infer(v: &Value) -> Value {
    match v {
        Value::Null => json!({ "type": "null" }),
        Value::Bool(_) => json!({ "type": "boolean" }),
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                json!({ "type": "integer" })
            } else {
                json!({ "type": "number" })
            }
        }
        Value::String(s) => match string_format(s) {
            Some(f) => json!({ "type": "string", "format": f }),
            None => json!({ "type": "string" }),
        },
        Value::Array(items) => {
            let merged = items.iter().map(infer).reduce(merge);
            match merged {
                Some(item_schema) => json!({ "type": "array", "items": item_schema }),
                None => json!({ "type": "array" }),
            }
        }
        Value::Object(o) => {
            let mut props = Map::new();
            for (k, val) in o {
                props.insert(k.clone(), infer(val));
            }
            json!({
                "type": "object",
                "properties": props,
                "required": o.keys().collect::<Vec<_>>(),
            })
        }
    }
}

/// Only formats that can be recognised without false positives.
fn string_format(s: &str) -> Option<&'static str> {
    let bytes = s.as_bytes();
    let is_date = s.len() == 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && s.chars().filter(|c| c.is_ascii_digit()).count() == 8;
    if is_date {
        return Some("date");
    }
    if s.len() >= 20 && bytes.get(10) == Some(&b'T') && (s.ends_with('Z') || s.contains('+')) {
        return Some("date-time");
    }
    if s.starts_with("http://") || s.starts_with("https://") {
        return Some("uri");
    }
    // A single @ with non-empty sides and a dotted domain.
    let mut at = s.split('@');
    if let (Some(local), Some(domain), None) = (at.next(), at.next(), at.next()) {
        if !local.is_empty() && domain.contains('.') && !domain.ends_with('.') {
            return Some("email");
        }
    }
    None
}

/// Combine two inferred schemas into one that accepts both shapes.
fn merge(a: Value, b: Value) -> Value {
    if a == b {
        return a;
    }
    let (ao, bo) = match (a.as_object(), b.as_object()) {
        (Some(x), Some(y)) => (x.clone(), y.clone()),
        _ => return a,
    };

    let mut types: BTreeSet<String> = BTreeSet::new();
    for o in [&ao, &bo] {
        match o.get("type") {
            Some(Value::String(t)) => {
                types.insert(t.clone());
            }
            Some(Value::Array(list)) => types.extend(
                list.iter()
                    .filter_map(|t| t.as_str().map(|s| s.to_string())),
            ),
            _ => {}
        }
    }
    // integer + number → number: every integer is a valid number.
    if types.contains("number") {
        types.remove("integer");
    }

    let mut out = Map::new();
    out.insert(
        "type".into(),
        if types.len() == 1 {
            json!(types.iter().next().expect("len == 1"))
        } else {
            json!(types.iter().collect::<Vec<_>>())
        },
    );

    // Objects: union of properties, intersection of `required`.
    if types.contains("object") {
        let empty = Map::new();
        let ap = ao
            .get("properties")
            .and_then(|p| p.as_object())
            .unwrap_or(&empty)
            .clone();
        let bp = bo
            .get("properties")
            .and_then(|p| p.as_object())
            .unwrap_or(&empty)
            .clone();
        let mut props = Map::new();
        for key in ap.keys().chain(bp.keys()).cloned().collect::<BTreeSet<_>>() {
            let merged = match (ap.get(&key), bp.get(&key)) {
                (Some(x), Some(y)) => merge(x.clone(), y.clone()),
                (Some(x), None) | (None, Some(x)) => x.clone(),
                (None, None) => continue,
            };
            props.insert(key, merged);
        }
        if !props.is_empty() {
            out.insert("properties".into(), Value::Object(props));
        }
        let req_a = required_set(&ao);
        let req_b = required_set(&bo);
        let both: Vec<&String> = req_a.intersection(&req_b).collect();
        if !both.is_empty() {
            out.insert("required".into(), json!(both));
        }
    }

    // Arrays: merge the item schemas.
    if types.contains("array") {
        let merged_items = match (ao.get("items"), bo.get("items")) {
            (Some(x), Some(y)) => Some(merge(x.clone(), y.clone())),
            (Some(x), None) | (None, Some(x)) => Some(x.clone()),
            (None, None) => None,
        };
        if let Some(items) = merged_items {
            out.insert("items".into(), items);
        }
    }

    // A `format` only survives if both sides agree on it.
    if let (Some(fa), Some(fb)) = (ao.get("format"), bo.get("format")) {
        if fa == fb {
            out.insert("format".into(), fa.clone());
        }
    }

    Value::Object(out)
}

fn required_set(o: &Map<String, Value>) -> BTreeSet<String> {
    o.get("required")
        .and_then(|r| r.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|k| k.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_counts_every_node_type() {
        let s = stats(r#"{"a":[1,2,3],"b":{"c":"xyz"},"d":true,"e":null,"f":1.5}"#).unwrap();
        assert_eq!(s["root_type"], json!("object"));
        assert_eq!(s["counts"]["arrays"], json!(1));
        assert_eq!(s["counts"]["objects"], json!(2));
        assert_eq!(s["counts"]["numbers"], json!(4)); // 1,2,3 and 1.5
        assert_eq!(s["counts"]["booleans"], json!(1));
        assert_eq!(s["counts"]["nulls"], json!(1));
        assert_eq!(s["max_depth"], json!(3));
        assert_eq!(s["largest_array"], json!(3));
        assert_eq!(s["longest_string"], json!(3));
        assert_eq!(s["object_keys"]["total"], json!(6)); // a,b,d,e,f + c
        assert_eq!(s["top_level"]["kind"], json!("object"));
    }

    #[test]
    fn stats_rejects_invalid_json() {
        assert!(stats("{oops").is_err());
    }

    #[test]
    fn schema_of_scalars_and_formats() {
        let s = infer_schema(
            r#"{"id":1,"ratio":0.5,"when":"2026-07-21","site":"https://x.dev","mail":"a@b.dev"}"#,
        )
        .unwrap();
        assert_eq!(
            s["$schema"],
            json!("http://json-schema.org/draft-07/schema#")
        );
        assert_eq!(s["properties"]["id"]["type"], json!("integer"));
        assert_eq!(s["properties"]["ratio"]["type"], json!("number"));
        assert_eq!(s["properties"]["when"]["format"], json!("date"));
        assert_eq!(s["properties"]["site"]["format"], json!("uri"));
        assert_eq!(s["properties"]["mail"]["format"], json!("email"));
        assert_eq!(
            s["required"],
            json!(["id", "mail", "ratio", "site", "when"])
        );
    }

    #[test]
    fn array_items_merge_conservatively() {
        // `b` is missing from the second element → optional; `a` changes type.
        let s = infer_schema(r#"[{"a":1,"b":"x"},{"a":"one"}]"#).unwrap();
        assert_eq!(s["type"], json!("array"));
        assert_eq!(
            s["items"]["properties"]["a"]["type"],
            json!(["integer", "string"])
        );
        assert_eq!(s["items"]["properties"]["b"]["type"], json!("string"));
        assert_eq!(s["items"]["required"], json!(["a"]));
    }

    #[test]
    fn integer_and_float_collapse_to_number() {
        let s = infer_schema(r#"[1, 2.5]"#).unwrap();
        assert_eq!(s["items"]["type"], json!("number"));
    }

    #[test]
    fn nullable_field_keeps_both_types() {
        let s = infer_schema(r#"[{"a":"x"},{"a":null}]"#).unwrap();
        assert_eq!(
            s["items"]["properties"]["a"]["type"],
            json!(["null", "string"])
        );
    }

    #[test]
    fn empty_array_has_no_items() {
        let s = infer_schema("[]").unwrap();
        assert_eq!(s["type"], json!("array"));
        assert!(s.get("items").is_none());
    }
}
