//! Biến template `{{var}}` và trích giá trị theo JSON path dạng chấm.
//!
//! * [`substitute`] — thay mọi `{{name}}` bằng giá trị trong map biến; biến
//!   không tồn tại được GIỮ NGUYÊN (để lộ ra trong log thay vì thành chuỗi
//!   rỗng khó lần), đồng thời trả về danh sách tên biến thiếu.
//! * [`json_path`] — `data.items[0].id` / tiền tố `$.` tuỳ chọn. Không phải
//!   JSONPath đầy đủ — đủ dùng cho assertion & extract, dễ đoán, dễ ghi log.

use serde_json::Value;
use std::collections::BTreeMap;

/// Map biến của một lần chạy: biến environment + biến đã extract.
pub type Vars = BTreeMap<String, String>;

/// Thay `{{name}}` trong `input` bằng `vars[name]`. Trả về `(kết quả, biến thiếu)`.
pub fn substitute(input: &str, vars: &Vars) -> (String, Vec<String>) {
    let mut out = String::with_capacity(input.len());
    let mut missing: Vec<String> = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = input[i + 2..].find("}}") {
                let name = input[i + 2..i + 2 + end].trim();
                if let Some(v) = vars.get(name) {
                    out.push_str(v);
                } else {
                    out.push_str(&input[i..i + 2 + end + 2]);
                    if !name.is_empty() && !missing.contains(&name.to_string()) {
                        missing.push(name.to_string());
                    }
                }
                i += 2 + end + 2;
                continue;
            }
        }
        // Advance one full UTF-8 character, not one byte.
        let ch_len = input[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1);
        out.push_str(&input[i..i + ch_len]);
        i += ch_len;
    }
    (out, missing)
}

/// Lấy giá trị tại `path` (vd `data.items[0].id`, chấp nhận tiền tố `$.`).
pub fn json_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let path = path.trim().trim_start_matches("$.").trim_start_matches('$');
    if path.is_empty() {
        return Some(root);
    }
    let mut cur = root;
    for seg in path.split('.') {
        if seg.is_empty() {
            continue;
        }
        // Tách `name[0][1]` thành name + các chỉ số.
        let (name, rest) = match seg.find('[') {
            Some(p) => (&seg[..p], &seg[p..]),
            None => (seg, ""),
        };
        if !name.is_empty() {
            cur = cur.get(name)?;
        }
        let mut rest = rest;
        while let Some(close) = rest.find(']') {
            let idx: usize = rest[1..close].trim().parse().ok()?;
            cur = cur.get(idx)?;
            rest = &rest[close + 1..];
        }
    }
    Some(cur)
}

/// Giá trị JSON → chuỗi để so sánh/ghi biến (string giữ nguyên, còn lại serialize).
pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn vars(pairs: &[(&str, &str)]) -> Vars {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn substitute_basic_and_missing() {
        let v = vars(&[("base_url", "http://x"), ("token", "abc")]);
        let (out, miss) = substitute("{{base_url}}/api?t={{token}}&u={{user}}", &v);
        assert_eq!(out, "http://x/api?t=abc&u={{user}}");
        assert_eq!(miss, vec!["user".to_string()]);
    }

    #[test]
    fn substitute_multibyte_text() {
        let v = vars(&[("tên", "Bảo")]);
        let (out, miss) = substitute("Xin chào {{tên}} — kiểm thử tiếng Việt", &v);
        assert_eq!(out, "Xin chào Bảo — kiểm thử tiếng Việt");
        assert!(miss.is_empty());
    }

    #[test]
    fn substitute_unclosed_brace_kept() {
        let v = vars(&[]);
        let (out, miss) = substitute("a {{oops", &v);
        assert_eq!(out, "a {{oops");
        assert!(miss.is_empty());
    }

    #[test]
    fn json_path_nested_and_index() {
        let j = json!({"data": {"items": [{"id": 7}, {"id": 8}], "name": "vn"}});
        assert_eq!(json_path(&j, "data.items[1].id"), Some(&json!(8)));
        assert_eq!(json_path(&j, "$.data.name"), Some(&json!("vn")));
        assert_eq!(json_path(&j, "data.items[9].id"), None);
        assert_eq!(json_path(&j, "nope"), None);
        assert_eq!(json_path(&j, ""), Some(&j));
    }

    #[test]
    fn value_to_string_forms() {
        assert_eq!(value_to_string(&json!("abc")), "abc");
        assert_eq!(value_to_string(&json!(12)), "12");
        assert_eq!(value_to_string(&json!({"a":1})), "{\"a\":1}");
    }
}
