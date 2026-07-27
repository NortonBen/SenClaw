//! `trigger-time` — compare one part of two instants (same hour? same day?).
//!
//! The Go rule accepted a `timezone` field and then dropped it on the floor,
//! comparing everything in the server's local zone. Here the zone is applied
//! before the component is read, which is the only way `day`/`weekday` mean
//! anything: 2026-01-01T20:00Z is already January 2nd in Asia/Ho_Chi_Minh.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Datelike, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const UNITS: [&str; 6] = ["minute", "hour", "day", "weekday", "month", "year"];
const DEFAULT_TZ: &str = "Asia/Ho_Chi_Minh";

pub struct TriggerTimeRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(TriggerTimeRule::new())
}

impl TriggerTimeRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("trigger-time", "So thời gian", Category::Logic)
            .desc("So một thành phần thời gian (giờ, ngày, thứ, tháng, năm) của hai mốc rồi rẽ nhánh.")
            .icon("⏰")
            .color("#faad14")
            .outputs(vec![
                PortSpec::new("true", "true")
                    .one()
                    .color("#52c41a")
                    .desc("Hai mốc trùng nhau ở đơn vị đã chọn"),
                PortSpec::new("false", "false")
                    .one()
                    .color("#f5222d")
                    .desc("Hai mốc khác nhau"),
            ])
            .schema(json!({
                "type": "object",
                "required": ["left", "right", "unit"],
                "properties": {
                    "left": {
                        "type": "string",
                        "title": "Mốc trái",
                        "default": "now()",
                        "placeholder": "now()",
                        "description": "`now()` = hiện tại, hoặc đường dẫn tới field chứa thời gian."
                    },
                    "right": {
                        "type": "string",
                        "title": "Mốc phải",
                        "placeholder": "hen_gio",
                        "description": "`now()` hoặc đường dẫn tới field chứa thời gian."
                    },
                    "unit": {
                        "type": "string",
                        "title": "Đơn vị so sánh",
                        "ui": "select",
                        "enum": ["minute", "hour", "day", "weekday", "month", "year"],
                        "default": "hour",
                        "description": "`day` = ngày trong tháng, `weekday` = thứ trong tuần."
                    },
                    "timezone": {
                        "type": "string",
                        "title": "Múi giờ",
                        "default": "Asia/Ho_Chi_Minh",
                        "placeholder": "Asia/Ho_Chi_Minh",
                        "description": "Tên IANA. Quyết định thật sự kết quả: cùng một mốc có thể là hai ngày khác nhau ở hai múi giờ."
                    }
                }
            }))
            .doc(
                "Hỏi \"đã tới giờ chưa?\" mà không cần viết biểu thức thời gian.\n\n\
                 ```json\n\
                 {\n  \"left\": \"now()\",\n  \"right\": \"hen_gio\",\n  \
                 \"unit\": \"hour\",\n  \"timezone\": \"Asia/Ho_Chi_Minh\"\n}\n\
                 ```\n\n\
                 Mỗi mốc nhận một trong ba dạng:\n\n\
                 - `now()` — thời điểm hiện tại.\n\
                 - đường dẫn tới một **số** — hiểu là unix giây.\n\
                 - đường dẫn tới một **chuỗi** — phải đúng RFC3339 (`2026-07-20T14:30:00Z`).\n\n\
                 Đơn vị: `minute`, `hour`, `day` (ngày trong tháng), `weekday` (thứ trong \
                 tuần), `month`, `year`.\n\n\
                 - **Múi giờ được áp dụng trước khi đọc thành phần**, nên `day` và \
                   `weekday` mới đúng theo giờ Việt Nam chứ không theo giờ máy chủ.\n\
                 - Chỉ so **một** thành phần: cùng `hour` nghĩa là cùng giờ trong ngày, \
                   không phải cùng thời điểm. Muốn chặt hơn thì nối thêm một node nữa \
                   so `day`.\n\
                 - Múi giờ sai, thiếu field, chuỗi không parse được → cổng `error`.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for TriggerTimeRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        for side in ["left", "right"] {
            let v = config
                .get(side)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim();
            if v.is_empty() {
                out.push(format!("Thiếu mốc `{side}`."));
            }
        }
        let unit = config
            .get("unit")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if unit.is_empty() {
            out.push("Thiếu đơn vị so sánh (`unit`).".to_string());
        } else if !UNITS.contains(&unit) {
            out.push(format!(
                "Đơn vị `{unit}` không hợp lệ (chỉ nhận {}).",
                UNITS.join(", ")
            ));
        }
        if let Some(tz) = config.get("timezone").and_then(|v| v.as_str()) {
            if !tz.trim().is_empty() && tz.trim().parse::<Tz>().is_err() {
                out.push(format!("Múi giờ `{tz}` không tồn tại (cần tên IANA)."));
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let (Some(left), Some(right)) = (ctx.cfg_str("left"), ctx.cfg_str("right")) else {
            return ctx.fail_config("Cần cả hai mốc `left` và `right`.");
        };
        let unit = ctx.cfg_str_or("unit", "hour");
        let tz_name = ctx.cfg_str_or("timezone", DEFAULT_TZ);
        let Ok(tz) = tz_name.trim().parse::<Tz>() else {
            return ctx.fail_config(format!("Múi giờ `{tz_name}` không tồn tại (cần tên IANA)."));
        };

        let l = match instant(&left, &msg.data, tz) {
            Ok(v) => v,
            Err(e) => return ctx.fail_runtime(format!("Mốc trái: {e}")),
        };
        let r = match instant(&right, &msg.data, tz) {
            Ok(v) => v,
            Err(e) => return ctx.fail_runtime(format!("Mốc phải: {e}")),
        };
        let (a, b) = match (part(&l, &unit), part(&r, &unit)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return ctx.fail_config(e),
        };

        Outcome::port(if a == b { "true" } else { "false" }, msg.data)
    }
}

/// `now()` or a path into the payload.
fn instant(source: &str, data: &Value, tz: Tz) -> Result<DateTime<Tz>, String> {
    let source = source.trim();
    if source.eq_ignore_ascii_case("now()") || source.eq_ignore_ascii_case("now") {
        return Ok(Utc::now().with_timezone(&tz));
    }
    let Some(v) = daq::get(data, source) else {
        return Err(format!("không tìm thấy `{source}` trong dữ liệu"));
    };
    match v {
        Value::Number(n) => {
            let secs = n
                .as_f64()
                .ok_or_else(|| format!("`{source}` không đọc được"))?;
            DateTime::<Utc>::from_timestamp(secs.trunc() as i64, 0)
                .map(|d| d.with_timezone(&tz))
                .ok_or_else(|| format!("`{source}` = {secs} không phải unix giây hợp lệ"))
        }
        Value::String(s) => DateTime::parse_from_rfc3339(s.trim())
            .map(|d| d.with_timezone(&tz))
            .map_err(|e| format!("`{source}` = `{s}` không đúng RFC3339 ({e})")),
        other => Err(format!("`{source}` = `{other}` không phải thời gian")),
    }
}

fn part(dt: &DateTime<Tz>, unit: &str) -> Result<i64, String> {
    Ok(match unit.trim() {
        "minute" => dt.minute() as i64,
        "hour" => dt.hour() as i64,
        "day" => dt.day() as i64,
        "weekday" => dt.weekday().num_days_from_monday() as i64,
        "month" => dt.month() as i64,
        "year" => dt.year() as i64,
        other => {
            return Err(format!(
                "Đơn vị `{other}` không hợp lệ (chỉ nhận {}).",
                UNITS.join(", ")
            ))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, one};

    #[tokio::test]
    async fn now_matches_a_unix_timestamp_taken_at_the_same_moment() {
        let r = TriggerTimeRule::new();
        let c = ctx(
            "trigger-time",
            json!({ "left": "now()", "right": "ts", "unit": "hour", "timezone": "UTC" }),
        );
        let now = Utc::now().timestamp();
        let (port, _) = one(r.handle(&c, msg(json!({ "ts": now }))).await);
        assert_eq!(port, "true");
    }

    #[tokio::test]
    async fn different_days_route_to_false() {
        let r = TriggerTimeRule::new();
        let c = ctx(
            "trigger-time",
            json!({ "left": "a", "right": "b", "unit": "day", "timezone": "UTC" }),
        );
        let (port, data) = one(r
            .handle(
                &c,
                msg(json!({ "a": "2026-07-20T10:00:00Z", "b": "2026-07-21T10:00:00Z" })),
            )
            .await);
        assert_eq!(port, "false");
        assert_eq!(data["a"], "2026-07-20T10:00:00Z", "dữ liệu không bị sửa");
    }

    /// The whole reason `timezone` exists: the same two instants are different
    /// days in UTC but the same day in Vietnam.
    #[tokio::test]
    async fn the_timezone_actually_changes_the_answer() {
        let r = TriggerTimeRule::new();
        let payload = json!({ "a": "2026-01-01T20:00:00Z", "b": "2026-01-02T02:00:00Z" });

        let utc = ctx(
            "trigger-time",
            json!({ "left": "a", "right": "b", "unit": "day", "timezone": "UTC" }),
        );
        assert_eq!(one(r.handle(&utc, msg(payload.clone())).await).0, "false");

        let vn = ctx(
            "trigger-time",
            json!({ "left": "a", "right": "b", "unit": "day", "timezone": "Asia/Ho_Chi_Minh" }),
        );
        assert_eq!(one(r.handle(&vn, msg(payload)).await).0, "true");
    }

    #[tokio::test]
    async fn a_number_is_read_as_unix_seconds() {
        let r = TriggerTimeRule::new();
        let c = ctx(
            "trigger-time",
            json!({ "left": "a", "right": "b", "unit": "year", "timezone": "UTC" }),
        );
        // 0 = 1970-01-01, 1_000_000_000 = 2001-09-09.
        let (port, _) = one(r.handle(&c, msg(json!({ "a": 0, "b": 1000000000 }))).await);
        assert_eq!(port, "false");
    }

    #[tokio::test]
    async fn an_unknown_timezone_fails() {
        let r = TriggerTimeRule::new();
        let c = ctx(
            "trigger-time",
            json!({ "left": "now()", "right": "now()", "unit": "hour", "timezone": "Mars/Olympus" }),
        );
        let err = failure(r.handle(&c, msg(json!({}))).await);
        assert!(err.contains("Mars/Olympus"), "{err}");
    }

    #[tokio::test]
    async fn a_missing_field_fails_with_the_side_that_broke() {
        let r = TriggerTimeRule::new();
        let c = ctx(
            "trigger-time",
            json!({ "left": "now()", "right": "hen", "unit": "hour" }),
        );
        let err = failure(r.handle(&c, msg(json!({}))).await);
        assert!(err.contains("Mốc phải") && err.contains("hen"), "{err}");
    }

    #[tokio::test]
    async fn an_unparseable_string_fails() {
        let r = TriggerTimeRule::new();
        let c = ctx(
            "trigger-time",
            json!({ "left": "now()", "right": "t", "unit": "hour" }),
        );
        let err = failure(r.handle(&c, msg(json!({ "t": "20/07/2026" }))).await);
        assert!(err.contains("RFC3339"), "{err}");
    }

    #[test]
    fn validate_checks_units_and_the_timezone_name() {
        let r = TriggerTimeRule::new();
        let ok = json!({ "left": "now()", "right": "t", "unit": "hour", "timezone": "Asia/Ho_Chi_Minh" });
        assert!(r.validate(&ok).is_empty());
        assert!(!r
            .validate(&json!({ "left": "now()", "right": "t", "unit": "fortnight" }))
            .is_empty());
        assert!(!r
            .validate(&json!({ "left": "now()", "unit": "hour" }))
            .is_empty());
        assert!(!r
            .validate(
                &json!({ "left": "now()", "right": "t", "unit": "hour", "timezone": "Nowhere" })
            )
            .is_empty());
    }

    #[test]
    fn the_branches_are_exclusive() {
        let r = TriggerTimeRule::new();
        assert_eq!(
            r.spec().output("true").unwrap().arity,
            crate::engine::spec::PortArity::One
        );
        assert!(r.spec().has_output("error"));
    }
}
