//! `delay` — hold a message for a while, then let it continue unchanged.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

/// Anything longer than this is a scheduled task, not a delay — and it would pin
/// a worker for the whole time.
const MAX_MS: f64 = 300_000.0;
const DEFAULT_MS: f64 = 1000.0;

pub struct DelayRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(DelayRule::new())
}

/// Clamp instead of reject: a negative or absurd number in the form should not
/// break a running chain.
fn clamp_ms(raw: f64) -> u64 {
    if !raw.is_finite() {
        return DEFAULT_MS as u64;
    }
    raw.clamp(0.0, MAX_MS) as u64
}

impl DelayRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("delay", "Chờ", Category::Transform)
            .desc("Giữ message lại một khoảng thời gian rồi cho đi tiếp nguyên vẹn.")
            .icon("⏳")
            .color("#faad14")
            .schema(json!({
                "type": "object",
                "properties": {
                    "ms": {
                        "type": "number",
                        "title": "Thời gian chờ (mili giây)",
                        "default": 1000,
                        "minimum": 0,
                        "maximum": 300000,
                        "description": "Tự cắt về khoảng 0–300000 (tối đa 5 phút)."
                    }
                }
            }))
            .doc(
                "Chèn một khoảng nghỉ giữa hai node.\n\n\
                 ```json\n{ \"ms\": 1500 }\n```\n\n\
                 - Dữ liệu đi ra **y hệt** dữ liệu đi vào.\n\
                 - Giá trị âm hoặc quá lớn được tự cắt về khoảng 0–300000 ms thay vì \
                   báo lỗi.\n\n\
                 ⚠️ **Node này chiếm một worker của chính nó trong suốt thời gian chờ.** \
                 Với `concurrency = 1` (mặc định) thì các message xếp hàng: 10 message \
                 chờ 1 giây sẽ mất 10 giây chứ không phải 1. Cần thông lượng thì tăng \
                 `opts.concurrency` của node lên.\n\n\
                 Cần chờ hàng phút, hàng giờ hoặc chờ tới một mốc cụ thể thì dùng node \
                 nguồn `schedule`, đừng dùng `delay`.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for DelayRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("ms") {
            None | Some(Value::Null) => {}
            Some(Value::Number(_)) => {}
            Some(Value::String(s)) if s.trim().parse::<f64>().is_ok() => {}
            Some(other) => out.push(format!("Thời gian chờ `{other}` không phải số.")),
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let ms = clamp_ms(ctx.cfg_f64_or("ms", DEFAULT_MS));
        tokio::time::sleep(Duration::from_millis(ms)).await;
        Outcome::out(msg.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, msg, one};

    #[tokio::test]
    async fn zero_returns_immediately_with_the_payload_intact() {
        let r = DelayRule::new();
        let c = ctx("delay", json!({ "ms": 0 }));
        let input = json!({ "a": 1, "l": [1, 2, 3] });
        let (port, data) = one(r.handle(&c, msg(input.clone())).await);
        assert_eq!(port, "out");
        assert_eq!(data, input);
    }

    /// A negative value must not hang the chain waiting for the clamp to fail.
    #[tokio::test]
    async fn a_negative_value_is_clamped_to_zero_and_still_emits() {
        let r = DelayRule::new();
        let c = ctx("delay", json!({ "ms": -5000 }));
        let (port, data) = one(r.handle(&c, msg(json!({ "x": 1 }))).await);
        assert_eq!(port, "out");
        assert_eq!(data["x"], 1);
    }

    #[tokio::test]
    async fn a_small_delay_actually_waits() {
        let r = DelayRule::new();
        let c = ctx("delay", json!({ "ms": 20 }));
        let start = std::time::Instant::now();
        let (port, _) = one(r.handle(&c, msg(json!({}))).await);
        assert_eq!(port, "out");
        assert!(
            start.elapsed() >= Duration::from_millis(15),
            "phải có chờ thật"
        );
    }

    #[test]
    fn the_configured_value_is_clamped_into_range() {
        assert_eq!(clamp_ms(-5.0), 0);
        assert_eq!(clamp_ms(0.0), 0);
        assert_eq!(clamp_ms(1500.0), 1500);
        assert_eq!(clamp_ms(999_999.0), MAX_MS as u64);
        assert_eq!(clamp_ms(f64::NAN), DEFAULT_MS as u64);
        assert_eq!(clamp_ms(f64::INFINITY), DEFAULT_MS as u64);
    }

    #[test]
    fn validate_only_complains_about_non_numbers() {
        let r = DelayRule::new();
        assert!(r.validate(&json!({})).is_empty());
        assert!(r.validate(&json!({ "ms": 500 })).is_empty());
        assert!(r.validate(&json!({ "ms": "500" })).is_empty());
        assert!(!r.validate(&json!({ "ms": "một lát" })).is_empty());
    }
}
