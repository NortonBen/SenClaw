//! `format` — convert field types: number ↔ string ↔ bool ↔ time.
//!
//! Time layouts use **chrono's strftime** syntax (`%Y-%m-%d`), not Go's
//! reference-date layout (`2006-01-02`) that the original rule expected. A Go
//! layout pasted in here would parse nothing, so the schema and the doc say so
//! explicitly.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const TYPES: [&str; 6] = ["string", "number", "double", "bool", "time", "timestamp"];

pub struct FormatRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(FormatRule::new())
}

impl FormatRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("format", "Chuyển kiểu", Category::Transform)
            .desc("Đổi kiểu dữ liệu của các field: số, chuỗi, boolean, thời gian, timestamp.")
            .icon("🔤")
            .color("#531dab")
            .schema(json!({
                "type": "object",
                "required": ["fields"],
                "properties": {
                    "fields": {
                        "type": "array",
                        "title": "Danh sách chuyển đổi",
                        "ui": "table",
                        "default": [],
                        "items": {
                            "type": "object",
                            "properties": {
                                "source": {
                                    "type": "string",
                                    "title": "Field nguồn",
                                    "description": "Đường dẫn tới giá trị cần đổi, vd `payload.temp`."
                                },
                                "target": {
                                    "type": "string",
                                    "title": "Field đích",
                                    "description": "Ghi đè chính nó cũng được (điền y hệt `source`)."
                                },
                                "type": {
                                    "type": "string",
                                    "title": "Kiểu đích",
                                    "ui": "select",
                                    "enum": ["string", "number", "double", "bool", "time", "timestamp"],
                                    "default": "string",
                                    "description": "`number` = số nguyên, `double` = số thực, `time` = chuỗi RFC3339, `timestamp` = unix giây."
                                },
                                "format": {
                                    "type": "string",
                                    "title": "Định dạng thời gian",
                                    "placeholder": "%Y-%m-%d %H:%M:%S",
                                    "description": "Chỉ dùng khi nguồn là CHUỖI thời gian. Cú pháp strftime của chrono (%Y %m %d %H %M %S), KHÔNG phải layout kiểu Go `2006-01-02`. Bỏ trống = đọc theo RFC3339."
                                }
                            }
                        }
                    }
                }
            }))
            .doc(
                "Ép kiểu trước khi so sánh, tính toán hay gửi đi.\n\n\
                 ```json\n\
                 {\n  \"fields\": [\n    \
                 { \"source\": \"temp\", \"target\": \"temp\", \"type\": \"double\" },\n    \
                 { \"source\": \"luc\", \"target\": \"luc_iso\", \"type\": \"time\", \
                 \"format\": \"%Y-%m-%d %H:%M:%S\" }\n  ]\n}\n\
                 ```\n\n\
                 | Kiểu | Kết quả |\n\
                 |---|---|\n\
                 | `string` | chuỗi (số 12.5 → `\"12.5\"`) |\n\
                 | `number` | số nguyên (cắt phần thập phân) |\n\
                 | `double` | số thực |\n\
                 | `bool` | số `> 0` → true; chuỗi `\"true\"` (không phân biệt hoa thường) → true |\n\
                 | `time` | chuỗi RFC3339, vd `2026-07-20T14:30:00+00:00` |\n\
                 | `timestamp` | unix giây (số nguyên) |\n\n\
                 - Nguồn là **số** và đích là `time` → hiểu số đó là unix giây.\n\
                 - Nguồn là **chuỗi** và đích là `time`/`timestamp` → đọc theo `format` \
                   (strftime của chrono), bỏ trống thì đọc theo RFC3339. Chuỗi không kèm \
                   múi giờ được hiểu là UTC.\n\
                 - **Thiếu field nguồn ≠ lỗi**: dòng đó bị bỏ qua và ghi log `warn`, các \
                   dòng còn lại vẫn chạy. Payload thưa là chuyện thường.\n\
                 - **Đổi kiểu thất bại = lỗi thật**: cả message ra cổng `error` kèm tên \
                   field, thay vì lặng lẽ ghi `null`.\n\
                 - Các dòng chạy lần lượt, dòng sau đọc được kết quả dòng trước.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for FormatRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        let Some(fields) = config.get("fields").and_then(|v| v.as_array()) else {
            out.push("Thiếu danh sách chuyển đổi (`fields`).".to_string());
            return out;
        };
        if fields.is_empty() {
            out.push("Chưa có dòng chuyển đổi nào.".to_string());
        }
        for (i, f) in fields.iter().enumerate() {
            let row = i + 1;
            let source = f
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let target = f
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if source.is_empty() {
                out.push(format!("Dòng {row}: thiếu field nguồn (`source`)."));
            }
            if target.is_empty() {
                out.push(format!("Dòng {row}: thiếu field đích (`target`)."));
            }
            let ty = f
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("string")
                .trim();
            if !TYPES.contains(&ty) {
                out.push(format!(
                    "Dòng {row}: kiểu `{ty}` không hợp lệ (chỉ nhận {}).",
                    TYPES.join(", ")
                ));
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let Some(fields) = ctx.cfg("fields").and_then(|v| v.as_array()) else {
            return ctx.fail_config("Thiếu danh sách chuyển đổi (`fields`).");
        };

        let mut data = msg.data;
        for f in fields {
            let source = f
                .get("source")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            let target = f
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if source.is_empty() || target.is_empty() {
                return ctx.fail_config("Mỗi dòng chuyển đổi cần cả `source` và `target`.");
            }
            let ty = f
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("string")
                .trim();
            let fmt = f
                .get("format")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();

            // Read from the accumulating payload, not the original, so a later
            // row can build on an earlier conversion.
            let Some(raw) = daq::get(&data, source) else {
                ctx.log(
                    "warn",
                    format!("Bỏ qua `{source}` → `{target}`: không có trong dữ liệu."),
                );
                continue;
            };
            match convert(&raw, ty, fmt) {
                Ok(v) => daq::set(&mut data, target, v),
                Err(e) => return ctx.fail_runtime(format!("Field `{source}` → `{target}`: {e}")),
            }
        }
        Outcome::out(data)
    }
}

fn convert(v: &Value, ty: &str, fmt: &str) -> Result<Value, String> {
    match ty {
        "string" => Ok(Value::String(text(v))),
        "bool" => Ok(Value::Bool(match v {
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64().unwrap_or(0.0) > 0.0,
            Value::String(s) => s.trim().eq_ignore_ascii_case("true"),
            other => return Err(format!("không đổi `{other}` thành bool")),
        })),
        "double" => Ok(json!(number(v)?)),
        "number" => Ok(json!(number(v)?.trunc() as i64)),
        "time" => Ok(Value::String(instant(v, fmt)?.to_rfc3339())),
        "timestamp" => Ok(json!(instant(v, fmt)?.timestamp())),
        other => Err(format!(
            "kiểu `{other}` không hợp lệ (chỉ nhận {})",
            TYPES.join(", ")
        )),
    }
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn number(v: &Value) -> Result<f64, String> {
    let n = match v {
        Value::Number(n) => n
            .as_f64()
            .ok_or_else(|| format!("số `{n}` không đọc được"))?,
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("`{s}` không phải số"))?,
        other => return Err(format!("không đổi `{other}` thành số")),
    };
    if !n.is_finite() {
        return Err("số không hữu hạn (NaN hoặc vô cực)".to_string());
    }
    Ok(n)
}

/// Everything time-shaped funnels through here, so `time` and `timestamp`
/// accept exactly the same inputs and differ only in what they print.
fn instant(v: &Value, fmt: &str) -> Result<DateTime<Utc>, String> {
    match v {
        Value::Number(n) => {
            let secs = n.as_f64().ok_or("số không đọc được")?;
            DateTime::<Utc>::from_timestamp(secs.trunc() as i64, 0)
                .ok_or_else(|| format!("unix giây `{secs}` nằm ngoài khoảng biểu diễn được"))
        }
        Value::String(s) => {
            let s = s.trim();
            if fmt.is_empty() {
                return DateTime::parse_from_rfc3339(s)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| format!("`{s}` không đúng RFC3339 ({e}); hãy điền `format`"));
            }
            // Try an offset-bearing layout first; fall back to a naive one read
            // as UTC, which is what a layout without %z means.
            if let Ok(d) = DateTime::parse_from_str(s, fmt) {
                return Ok(d.with_timezone(&Utc));
            }
            NaiveDateTime::parse_from_str(s, fmt)
                .map(|n| n.and_utc())
                .map_err(|e| format!("`{s}` không khớp định dạng `{fmt}` ({e})"))
        }
        other => Err(format!("không đổi `{other}` thành thời gian")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, one};

    fn run(fields: Value, data: Value) -> (crate::engine::spec::RunCtx, Value) {
        (ctx("format", json!({ "fields": fields })), data)
    }

    #[tokio::test]
    async fn number_to_string() {
        let r = FormatRule::new();
        let (c, d) = run(
            json!([{ "source": "n", "target": "s", "type": "string" }]),
            json!({ "n": 12.5 }),
        );
        assert_eq!(one(r.handle(&c, msg(d)).await).1["s"], "12.5");
    }

    #[tokio::test]
    async fn string_to_number_truncates_and_to_double_keeps_the_fraction() {
        let r = FormatRule::new();
        let (c, d) = run(
            json!([
                { "source": "s", "target": "i", "type": "number" },
                { "source": "s", "target": "f", "type": "double" }
            ]),
            json!({ "s": "42.9" }),
        );
        let out = one(r.handle(&c, msg(d)).await).1;
        assert_eq!(out["i"], 42);
        assert_eq!(out["f"], 42.9);
    }

    #[tokio::test]
    async fn to_bool_uses_positive_numbers_and_the_word_true() {
        let r = FormatRule::new();
        let (c, d) = run(
            json!([
                { "source": "a", "target": "a", "type": "bool" },
                { "source": "b", "target": "b", "type": "bool" },
                { "source": "c", "target": "c", "type": "bool" },
                { "source": "e", "target": "e", "type": "bool" }
            ]),
            json!({ "a": 3, "b": 0, "c": "TRUE", "e": "yes" }),
        );
        let out = one(r.handle(&c, msg(d)).await).1;
        assert_eq!(out["a"], true);
        assert_eq!(out["b"], false);
        assert_eq!(out["c"], true);
        assert_eq!(out["e"], false, "chỉ chấp nhận đúng chữ `true`");
    }

    #[tokio::test]
    async fn a_unix_number_becomes_an_rfc3339_string() {
        let r = FormatRule::new();
        let (c, d) = run(
            json!([{ "source": "ts", "target": "iso", "type": "time" }]),
            json!({ "ts": 0 }),
        );
        let out = one(r.handle(&c, msg(d)).await).1;
        assert_eq!(out["iso"].as_str().unwrap(), "1970-01-01T00:00:00+00:00");
    }

    #[tokio::test]
    async fn a_strftime_layout_parses_a_naive_string_as_utc() {
        let r = FormatRule::new();
        let (c, d) = run(
            json!([{
                "source": "luc", "target": "iso", "type": "time",
                "format": "%Y-%m-%d %H:%M:%S"
            }]),
            json!({ "luc": "2026-07-20 14:30:00" }),
        );
        let out = one(r.handle(&c, msg(d)).await).1;
        assert_eq!(out["iso"].as_str().unwrap(), "2026-07-20T14:30:00+00:00");
    }

    #[tokio::test]
    async fn an_rfc3339_string_becomes_unix_seconds() {
        let r = FormatRule::new();
        let (c, d) = run(
            json!([{ "source": "iso", "target": "ts", "type": "timestamp" }]),
            json!({ "iso": "1970-01-01T00:01:00Z" }),
        );
        assert_eq!(one(r.handle(&c, msg(d)).await).1["ts"], 60);
    }

    /// A sparse payload must not take the whole message down.
    #[tokio::test]
    async fn a_missing_source_is_skipped_with_a_warning() {
        let r = FormatRule::new();
        let (c, d) = run(
            json!([
                { "source": "vang_mat", "target": "x", "type": "double" },
                { "source": "co", "target": "y", "type": "double" }
            ]),
            json!({ "co": "1.5" }),
        );
        let out = one(r.handle(&c, msg(d)).await).1;
        assert!(out.get("x").is_none(), "field thiếu không được tạo ra");
        assert_eq!(out["y"], 1.5);

        let logs = c.svc.db.list_logs(1, 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].level, "warn");
        assert!(logs[0].message.contains("vang_mat"), "{}", logs[0].message);
    }

    #[tokio::test]
    async fn a_failed_conversion_fails_the_message_with_the_field_name() {
        let r = FormatRule::new();
        let (c, d) = run(
            json!([{ "source": "s", "target": "n", "type": "double" }]),
            json!({ "s": "abc" }),
        );
        let err = failure(r.handle(&c, msg(d)).await);
        assert!(
            err.contains("`s`") && err.contains("không phải số"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn a_bad_time_string_reports_the_layout() {
        let r = FormatRule::new();
        let (c, d) = run(
            json!([{ "source": "s", "target": "t", "type": "time", "format": "%Y-%m-%d" }]),
            json!({ "s": "20/07/2026" }),
        );
        let err = failure(r.handle(&c, msg(d)).await);
        assert!(err.contains("%Y-%m-%d"), "{err}");
    }

    #[tokio::test]
    async fn an_unknown_type_fails_instead_of_passing_through() {
        let r = FormatRule::new();
        let (c, d) = run(
            json!([{ "source": "s", "target": "s", "type": "duration" }]),
            json!({ "s": "1" }),
        );
        assert!(failure(r.handle(&c, msg(d)).await).contains("duration"));
    }

    #[test]
    fn validate_checks_the_type_name_and_both_paths() {
        let r = FormatRule::new();
        assert!(r
            .validate(&json!({ "fields": [{ "source": "a", "target": "b", "type": "double" }] }))
            .is_empty());
        assert!(!r
            .validate(&json!({ "fields": [{ "source": "a", "target": "b", "type": "duration" }] }))
            .is_empty());
        assert!(!r
            .validate(&json!({ "fields": [{ "target": "b", "type": "string" }] }))
            .is_empty());
        assert!(!r.validate(&json!({})).is_empty());
        assert!(!r.validate(&json!({ "fields": [] })).is_empty());
    }
}
