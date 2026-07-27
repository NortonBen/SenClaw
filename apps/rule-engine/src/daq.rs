//! Path access into `serde_json::Value`, plus the view an expression sees.
//!
//! Rewritten from the Go `core/daq` package, whose reflection-based typing had
//! several inverted conditions (every array read returned an error, `float64`
//! was classified as a string, `Number()` cached wrongly on `0`). Behaviour
//! here is the intent, not the bug.

use serde_json::{Map, Value};

/// One step of a path: `a` or `a[2]`.
#[derive(Debug, PartialEq)]
enum Step {
    Key(String),
    Index(usize),
}

fn parse_path(path: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    for raw in path.split('.') {
        if raw.is_empty() {
            continue;
        }
        let mut name = raw;
        // Split trailing `[i]` groups off the key.
        let mut indices = Vec::new();
        while let Some(open) = name.rfind('[') {
            if !name.ends_with(']') {
                break;
            }
            let idx = &name[open + 1..name.len() - 1];
            match idx.trim().parse::<usize>() {
                Ok(i) => {
                    indices.push(i);
                    name = &name[..open];
                }
                Err(_) => break,
            }
        }
        if !name.is_empty() {
            steps.push(Step::Key(name.to_string()));
        }
        for i in indices.into_iter().rev() {
            steps.push(Step::Index(i));
        }
    }
    steps
}

/// Read `path` out of `root`. Missing or type-mismatched steps give `None`.
pub fn get(root: &Value, path: &str) -> Option<Value> {
    let mut cur = root;
    for step in parse_path(path) {
        cur = match step {
            Step::Key(k) => cur.get(&k)?,
            Step::Index(i) => cur.get(i)?,
        };
    }
    Some(cur.clone())
}

pub fn get_str(root: &Value, path: &str) -> Option<String> {
    get(root, path).map(|v| match v {
        Value::String(s) => s,
        other => other.to_string(),
    })
}

pub fn get_f64(root: &Value, path: &str) -> Option<f64> {
    match get(root, path)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        Value::Bool(b) => Some(if b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

/// Write `value` at `path`, creating intermediate objects as needed.
/// Array indices must already exist; a missing index appends when it is the
/// next slot, otherwise the write is skipped.
pub fn set(root: &mut Value, path: &str, value: Value) {
    let steps = parse_path(path);
    if steps.is_empty() {
        // `""`, `"."`, `".."` all parse to zero steps. Replacing the entire
        // payload from such a path is almost always an accident (the node-level
        // `trim().is_empty()` guard doesn't catch all-dots), so skip the write
        // rather than wipe every downstream field.
        return;
    }
    if !root.is_object() && !root.is_array() {
        *root = Value::Object(Map::new());
    }
    let mut cur = root;
    let last = steps.len() - 1;
    for (i, step) in steps.iter().enumerate() {
        let is_last = i == last;
        match step {
            Step::Key(k) => {
                if !cur.is_object() {
                    *cur = Value::Object(Map::new());
                }
                let obj = cur.as_object_mut().expect("just made it an object");
                if is_last {
                    obj.insert(k.clone(), value);
                    return;
                }
                cur = obj
                    .entry(k.clone())
                    .or_insert_with(|| Value::Object(Map::new()));
            }
            Step::Index(idx) => {
                if !cur.is_array() {
                    *cur = Value::Array(Vec::new());
                }
                let arr = cur.as_array_mut().expect("just made it an array");
                if *idx == arr.len() {
                    arr.push(Value::Null);
                }
                if *idx >= arr.len() {
                    return; // sparse write: refuse rather than pad with nulls
                }
                if is_last {
                    arr[*idx] = value;
                    return;
                }
                cur = &mut arr[*idx];
            }
        }
    }
}

pub fn remove(root: &mut Value, path: &str) {
    let steps = parse_path(path);
    if steps.is_empty() {
        return;
    }
    let mut cur = root;
    let last = steps.len() - 1;
    for (i, step) in steps.iter().enumerate() {
        let is_last = i == last;
        match step {
            Step::Key(k) => {
                let Some(obj) = cur.as_object_mut() else {
                    return;
                };
                if is_last {
                    obj.remove(k);
                    return;
                }
                let Some(next) = obj.get_mut(k) else { return };
                cur = next;
            }
            Step::Index(idx) => {
                let Some(arr) = cur.as_array_mut() else {
                    return;
                };
                if *idx >= arr.len() {
                    return;
                }
                if is_last {
                    arr.remove(*idx);
                    return;
                }
                cur = &mut arr[*idx];
            }
        }
    }
}

/// The object an expression sees: the payload's own fields at the top level,
/// plus the message metadata under `meta_data`.
///
/// The Go engine flattened `data["default"]` and stashed the whole object under
/// `meta_data`; with the branch wrapper gone the payload *is* the top level,
/// which is what people expect when they type `temperature > 30`.
pub fn view(data: &Value, meta: &Value) -> Value {
    let mut out = match data {
        Value::Object(m) => m.clone(),
        other => {
            let mut m = Map::new();
            m.insert("value".to_string(), other.clone());
            m
        }
    };
    // Only expose the message metadata under `meta_data` if the payload doesn't
    // already own that key. IoT gateways sometimes send a real `meta_data`
    // field; clobbering it would silently make every expression read the
    // message meta instead of the user's data.
    out.entry("meta_data".to_string())
        .or_insert_with(|| meta.clone());
    Value::Object(out)
}

/// Interpolate `${key}` / `${a.b.c}` from the payload, falling back to meta.
///
/// The Go version resolved `${x}` against top-level keys of `data`, which after
/// the branch wrapper meant branch names — so `${temperature}` never resolved.
pub fn interpolate(template: &str, data: &Value, meta: &Value) -> String {
    let mut out = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '$' && i + 1 < chars.len() && chars[i + 1] == '{' {
            if let Some(end) = (i + 2..chars.len()).find(|&j| chars[j] == '}') {
                let key: String = chars[i + 2..end].iter().collect();
                out.push_str(&lookup(key.trim(), data, meta));
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn lookup(key: &str, data: &Value, meta: &Value) -> String {
    // An empty path (`${}`, `${ }`) or an all-dots path (`${.}`, `${..}`) yields
    // no steps, and `get` would then return the ENTIRE root — leaking the whole
    // payload into a URL / message on a typo. Resolve those to "" instead.
    if parse_path(key).is_empty() {
        return String::new();
    }
    let candidates = [get(data, key), get(meta, key)];
    for c in candidates.into_iter().flatten() {
        return match c {
            Value::String(s) => s,
            Value::Null => String::new(),
            other => other.to_string(),
        };
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_walks_objects_and_arrays() {
        let v = json!({ "a": { "b": [ {"c": 42}, {"c": 7} ] } });
        assert_eq!(get(&v, "a.b[1].c"), Some(json!(7)));
        assert_eq!(get(&v, "a.b[0].c"), Some(json!(42)));
        assert_eq!(get(&v, "a.missing"), None);
        assert_eq!(get(&v, "a.b[9]"), None);
    }

    #[test]
    fn arrays_are_readable_at_all() {
        // The Go implementation returned NotConvertTypeArray for every array.
        let v = json!({ "list": [1, 2, 3] });
        assert_eq!(get(&v, "list"), Some(json!([1, 2, 3])));
        assert_eq!(get_f64(&v, "list[2]"), Some(3.0));
    }

    #[test]
    fn get_f64_handles_zero_and_numeric_strings() {
        let v = json!({ "z": 0, "s": "12.5", "b": true });
        assert_eq!(get_f64(&v, "z"), Some(0.0));
        assert_eq!(get_f64(&v, "s"), Some(12.5));
        assert_eq!(get_f64(&v, "b"), Some(1.0));
    }

    #[test]
    fn set_creates_intermediate_objects() {
        let mut v = json!({});
        set(&mut v, "a.b.c", json!(1));
        assert_eq!(v, json!({"a":{"b":{"c":1}}}));
        set(&mut v, "a.b.c", json!(2));
        assert_eq!(get(&v, "a.b.c"), Some(json!(2)));
    }

    #[test]
    fn set_appends_at_the_next_array_slot_only() {
        let mut v = json!({ "l": [1] });
        set(&mut v, "l[1]", json!(2));
        assert_eq!(v["l"], json!([1, 2]));
        set(&mut v, "l[9]", json!(9));
        assert_eq!(v["l"], json!([1, 2]), "sparse write must be refused");
    }

    #[test]
    fn view_exposes_payload_fields_at_the_top_level() {
        let data = json!({ "a": 1, "b": 2 });
        let meta = json!({ "device_id": "d-1" });
        let v = view(&data, &meta);
        assert_eq!(v["a"], 1);
        assert_eq!(v["meta_data"]["device_id"], "d-1");
    }

    #[test]
    fn view_wraps_a_scalar_payload_as_value() {
        let v = view(&json!(42), &json!({}));
        assert_eq!(v["value"], 42);
    }

    #[test]
    fn interpolate_resolves_payload_fields_then_meta() {
        let data = json!({ "temp": 31.5, "name": "kho A" });
        let meta = json!({ "device_id": "d-1" });
        let s = interpolate(
            "Cảnh báo ${name}: ${temp} độ (thiết bị ${device_id})",
            &data,
            &meta,
        );
        assert_eq!(s, "Cảnh báo kho A: 31.5 độ (thiết bị d-1)");
    }

    #[test]
    fn interpolate_walks_nested_paths() {
        let data = json!({ "user": { "name": "Lan" }, "l": [1, 2] });
        let s = interpolate("${user.name}-${l[1]}", &data, &json!({}));
        assert_eq!(s, "Lan-2");
    }

    #[test]
    fn interpolate_leaves_unknown_keys_empty_and_keeps_stray_dollars() {
        let s = interpolate("a=${nope} $5 ${", &json!({}), &json!({}));
        assert_eq!(s, "a= $5 ${");
    }

    #[test]
    fn interpolate_empty_key_does_not_dump_the_whole_payload() {
        let data = json!({ "secret": "s3cr3t", "t": 1 });
        // `${}`, `${ }`, `${.}` must resolve to "" — never the entire payload.
        assert_eq!(interpolate("x=${}", &data, &json!({})), "x=");
        assert_eq!(interpolate("x=${ }", &data, &json!({})), "x=");
        assert_eq!(interpolate("x=${.}", &data, &json!({})), "x=");
        assert_eq!(interpolate("x=${..}", &data, &json!({})), "x=");
    }

    #[test]
    fn set_ignores_root_replacing_paths() {
        // An all-dots (or empty) path must be a no-op, not a wipe of the payload.
        let original = json!({ "a": 1, "b": 2 });
        for path in ["", ".", "..", "..."] {
            let mut d = original.clone();
            set(&mut d, path, json!("wiped"));
            assert_eq!(d, original, "path `{path}` phải là no-op");
        }
    }

    #[test]
    fn view_does_not_clobber_an_existing_meta_data_field() {
        let data = json!({ "meta_data": "mine", "x": 1 });
        let v = view(&data, &json!({ "device_id": "d1" }));
        // The user's `meta_data` wins; the message meta is not injected over it.
        assert_eq!(v["meta_data"], json!("mine"));
        assert_eq!(v["x"], 1);
    }

    #[test]
    fn remove_deletes_keys_and_elements() {
        let mut v = json!({ "a": { "b": 1, "c": 2 }, "l": [1,2,3] });
        remove(&mut v, "a.b");
        assert_eq!(v["a"], json!({"c":2}));
        remove(&mut v, "l[1]");
        assert_eq!(v["l"], json!([1, 3]));
    }
}
