//! Engine đánh giá assertion. Một assertion là một object JSON:
//!
//! ```json
//! { "type": "status",        "op": "eq",  "value": 200 }
//! { "type": "json",          "path": "data.token", "op": "exists" }
//! { "type": "body_contains", "value": "ok" }
//! { "type": "header",        "name": "content-type", "op": "contains", "value": "json" }
//! { "type": "duration_max_ms", "value": 1500 }
//! { "type": "exit_code",     "value": 0 }
//! { "type": "stdout_contains", "value": "PASS" }
//! { "type": "stdout_matches", "value": "\\d+ passed" }
//! { "type": "text_contains", "value": "Đăng xuất" }
//! { "type": "url_contains",  "value": "/dashboard" }
//! ```
//!
//! `op` mặc định: `eq` (status/exit_code/json), `contains` (header).
//! So sánh số khi cả hai vế parse được thành f64, ngược lại so sánh chuỗi.
//! Kết quả từng assertion được trả về đầy đủ (desc/pass/actual/expected) để
//! UI và agent thấy CHÍNH XÁC cái gì lệch — không chỉ pass/fail tổng.

use crate::tmpl;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Kết quả thô sau khi thực thi một case — nguồn dữ liệu cho mọi assertion.
#[derive(Debug, Default, Clone)]
pub struct Outcome {
    /// HTTP status (case http).
    pub status: Option<u16>,
    /// Header response, tên đã lowercase (case http).
    pub headers: BTreeMap<String, String>,
    /// Body response (case http).
    pub body: String,
    /// Exit code (case script).
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Text trang cuối cùng (case web).
    pub page_text: String,
    /// URL cuối cùng (case web).
    pub final_url: String,
    pub duration_ms: u64,
}

impl Outcome {
    pub fn body_json(&self) -> Option<Value> {
        serde_json::from_str(self.body.trim()).ok()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}…")
}

/// So sánh hai vế theo `op`; số nếu cả hai là số.
fn compare(op: &str, actual: &str, expected: &str) -> bool {
    let nums = (actual.trim().parse::<f64>(), expected.trim().parse::<f64>());
    match op {
        "eq" => match nums {
            (Ok(a), Ok(b)) => a == b,
            _ => actual == expected,
        },
        "ne" => match nums {
            (Ok(a), Ok(b)) => a != b,
            _ => actual != expected,
        },
        "lt" | "lte" | "gt" | "gte" => match nums {
            (Ok(a), Ok(b)) => match op {
                "lt" => a < b,
                "lte" => a <= b,
                "gt" => a > b,
                _ => a >= b,
            },
            _ => false,
        },
        "contains" => actual.contains(expected),
        "not_contains" => !actual.contains(expected),
        "matches" => regex::Regex::new(expected)
            .map(|re| re.is_match(actual))
            .unwrap_or(false),
        _ => false,
    }
}

/// Đánh giá một assertion trên `outcome`. Trả về object kết quả cho UI/agent.
pub fn evaluate(spec: &Value, outcome: &Outcome) -> Value {
    let typ = spec.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let op_default = match typ {
        "header" => "contains",
        "json" => "eq",
        _ => "eq",
    };
    let op = spec
        .get("op")
        .and_then(|v| v.as_str())
        .unwrap_or(op_default);
    let expected = spec
        .get("value")
        .map(tmpl::value_to_string)
        .unwrap_or_default();

    let (desc, pass, actual): (String, bool, String) = match typ {
        "status" => {
            let actual = outcome.status.map(|s| s.to_string()).unwrap_or_default();
            (
                format!("HTTP status {op} {expected}"),
                compare(op, &actual, &expected),
                actual,
            )
        }
        "json" => {
            let path = spec.get("path").and_then(|v| v.as_str()).unwrap_or("");
            match outcome.body_json() {
                None => (
                    format!("json {path} {op} {expected}"),
                    false,
                    "(body không phải JSON)".into(),
                ),
                Some(root) => match tmpl::json_path(&root, path) {
                    None => {
                        let pass = op == "not_exists";
                        (
                            format!("json {path} {op} {expected}"),
                            pass,
                            "(không có path)".into(),
                        )
                    }
                    Some(v) => {
                        let actual = tmpl::value_to_string(v);
                        let pass = match op {
                            "exists" => true,
                            "not_exists" => false,
                            _ => compare(op, &actual, &expected),
                        };
                        (
                            format!("json {path} {op} {expected}"),
                            pass,
                            truncate(&actual, 200),
                        )
                    }
                },
            }
        }
        "body_contains" => (
            format!("body chứa \"{}\"", truncate(&expected, 80)),
            outcome.body.contains(&expected),
            truncate(&outcome.body, 200),
        ),
        "body_not_contains" => (
            format!("body KHÔNG chứa \"{}\"", truncate(&expected, 80)),
            !outcome.body.contains(&expected),
            truncate(&outcome.body, 200),
        ),
        "header" => {
            let name = spec
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_lowercase();
            let actual = outcome.headers.get(&name).cloned().unwrap_or_default();
            (
                format!("header {name} {op} {expected}"),
                compare(op, &actual, &expected),
                actual,
            )
        }
        "duration_max_ms" => {
            let actual = outcome.duration_ms.to_string();
            (
                format!("thời gian ≤ {expected}ms"),
                compare("lte", &actual, &expected),
                format!("{}ms", actual),
            )
        }
        "exit_code" => {
            let actual = outcome.exit_code.map(|c| c.to_string()).unwrap_or_default();
            (
                format!("exit code {op} {expected}"),
                compare(op, &actual, &expected),
                actual,
            )
        }
        "stdout_contains" => (
            format!("stdout chứa \"{}\"", truncate(&expected, 80)),
            outcome.stdout.contains(&expected),
            truncate(&outcome.stdout, 200),
        ),
        "stderr_contains" => (
            format!("stderr chứa \"{}\"", truncate(&expected, 80)),
            outcome.stderr.contains(&expected),
            truncate(&outcome.stderr, 200),
        ),
        "stdout_matches" => (
            format!("stdout khớp regex /{}/", truncate(&expected, 80)),
            compare("matches", &outcome.stdout, &expected),
            truncate(&outcome.stdout, 200),
        ),
        "text_contains" => (
            format!("trang chứa \"{}\"", truncate(&expected, 80)),
            outcome.page_text.contains(&expected),
            truncate(&outcome.page_text, 200),
        ),
        "text_not_contains" => (
            format!("trang KHÔNG chứa \"{}\"", truncate(&expected, 80)),
            !outcome.page_text.contains(&expected),
            truncate(&outcome.page_text, 200),
        ),
        "url_contains" => (
            format!("URL chứa \"{}\"", truncate(&expected, 80)),
            outcome.final_url.contains(&expected),
            outcome.final_url.clone(),
        ),
        other => (
            format!("assertion không hỗ trợ: \"{other}\""),
            false,
            String::new(),
        ),
    };

    json!({ "desc": desc, "pass": pass, "actual": actual, "expected": expected, "type": typ })
}

/// Đánh giá cả mảng assertion; trả `(kết quả từng cái, tất cả pass?)`.
/// Mảng RỖNG coi là fail-safe pass=true nhưng kèm ghi chú — một case không có
/// assertion vẫn "pass" nếu thực thi không lỗi (smoke test).
pub fn evaluate_all(specs: &[Value], outcome: &Outcome) -> (Vec<Value>, bool) {
    let results: Vec<Value> = specs.iter().map(|s| evaluate(s, outcome)).collect();
    let all_pass = results.iter().all(|r| r["pass"].as_bool().unwrap_or(false));
    (results, all_pass)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http_outcome() -> Outcome {
        let mut headers = BTreeMap::new();
        headers.insert(
            "content-type".to_string(),
            "application/json; charset=utf-8".to_string(),
        );
        Outcome {
            status: Some(200),
            headers,
            body: r#"{"ok":true,"data":{"token":"abc123","count":5}}"#.to_string(),
            duration_ms: 120,
            ..Default::default()
        }
    }

    #[test]
    fn status_eq_and_ne() {
        let o = http_outcome();
        assert!(evaluate(&json!({"type":"status","value":200}), &o)["pass"]
            .as_bool()
            .unwrap());
        assert!(!evaluate(&json!({"type":"status","value":404}), &o)["pass"]
            .as_bool()
            .unwrap());
        assert!(
            evaluate(&json!({"type":"status","op":"lt","value":400}), &o)["pass"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn json_path_ops() {
        let o = http_outcome();
        assert!(evaluate(
            &json!({"type":"json","path":"data.token","op":"exists"}),
            &o
        )["pass"]
            .as_bool()
            .unwrap());
        assert!(evaluate(
            &json!({"type":"json","path":"data.count","op":"gte","value":5}),
            &o
        )["pass"]
            .as_bool()
            .unwrap());
        assert!(evaluate(
            &json!({"type":"json","path":"data.token","value":"abc123"}),
            &o
        )["pass"]
            .as_bool()
            .unwrap());
        assert!(!evaluate(
            &json!({"type":"json","path":"data.missing","op":"exists"}),
            &o
        )["pass"]
            .as_bool()
            .unwrap());
        assert!(evaluate(
            &json!({"type":"json","path":"data.missing","op":"not_exists"}),
            &o
        )["pass"]
            .as_bool()
            .unwrap());
    }

    #[test]
    fn body_and_header_and_duration() {
        let o = http_outcome();
        assert!(
            evaluate(&json!({"type":"body_contains","value":"\"ok\":true"}), &o)["pass"]
                .as_bool()
                .unwrap()
        );
        assert!(
            evaluate(&json!({"type":"body_not_contains","value":"error"}), &o)["pass"]
                .as_bool()
                .unwrap()
        );
        assert!(evaluate(
            &json!({"type":"header","name":"Content-Type","value":"json"}),
            &o
        )["pass"]
            .as_bool()
            .unwrap());
        assert!(
            evaluate(&json!({"type":"duration_max_ms","value":500}), &o)["pass"]
                .as_bool()
                .unwrap()
        );
        assert!(
            !evaluate(&json!({"type":"duration_max_ms","value":50}), &o)["pass"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn script_assertions() {
        let o = Outcome {
            exit_code: Some(0),
            stdout: "12 passed, 0 failed\n".to_string(),
            stderr: String::new(),
            ..Default::default()
        };
        assert!(evaluate(&json!({"type":"exit_code","value":0}), &o)["pass"]
            .as_bool()
            .unwrap());
        assert!(
            evaluate(&json!({"type":"stdout_contains","value":"12 passed"}), &o)["pass"]
                .as_bool()
                .unwrap()
        );
        assert!(
            evaluate(&json!({"type":"stdout_matches","value":"\\d+ passed"}), &o)["pass"]
                .as_bool()
                .unwrap()
        );
        assert!(
            !evaluate(&json!({"type":"stdout_matches","value":"["}), &o)["pass"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn web_assertions() {
        let o = Outcome {
            page_text: "Chào mừng — Đăng xuất".to_string(),
            final_url: "http://x/dashboard?tab=1".to_string(),
            ..Default::default()
        };
        assert!(
            evaluate(&json!({"type":"text_contains","value":"Đăng xuất"}), &o)["pass"]
                .as_bool()
                .unwrap()
        );
        assert!(
            evaluate(&json!({"type":"url_contains","value":"/dashboard"}), &o)["pass"]
                .as_bool()
                .unwrap()
        );
    }

    #[test]
    fn evaluate_all_empty_is_pass() {
        let (results, ok) = evaluate_all(&[], &http_outcome());
        assert!(results.is_empty());
        assert!(ok);
    }

    #[test]
    fn unknown_type_fails() {
        assert!(!evaluate(&json!({"type":"wat"}), &http_outcome())["pass"]
            .as_bool()
            .unwrap());
    }
}
