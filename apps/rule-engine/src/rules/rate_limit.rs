//! `rate-limit` — điều tiết tần suất bằng token-bucket.
//!
//! Mỗi node giữ một xô token trong state (theo chain + node). Cứ `perMs` mili-giây
//! xô được nạp thêm `rate` token, trần bằng `rate`. Mỗi message tiêu 1 token; hết
//! token thì message đi ra `dropped` thay vì `out`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{now_ms, Message, Outcome};

const SCOPE_BUCKET: &str = "bucket";
const PORT_OUT: &str = "out";
const PORT_DROPPED: &str = "dropped";
const DEFAULT_RATE: f64 = 5.0;
const DEFAULT_PER_MS: f64 = 1000.0;

pub struct RateLimitRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(RateLimitRule::new())
}

impl RateLimitRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("rate-limit", "Giới hạn tần suất", Category::Filter)
            .desc("Điều tiết tần suất bằng token-bucket; vượt hạn mức thì đẩy ra nhánh riêng.")
            .icon("⏱️")
            .color("#722ed1")
            .outputs(vec![
                PortSpec::new(PORT_OUT, "out")
                    .color("#52c41a")
                    .desc("Còn token — message được cho qua."),
                PortSpec::new(PORT_DROPPED, "dropped")
                    .color("#fa541c")
                    .desc("Hết token — vượt hạn mức, message đi ra đây."),
            ])
            .schema(json!({
                "type": "object",
                "properties": {
                    "rate": {
                        "type": "number",
                        "title": "Số token mỗi cửa sổ",
                        "default": DEFAULT_RATE,
                        "minimum": 0,
                        "exclusiveMinimum": 0,
                        "description": "Số message tối đa cho qua trong mỗi cửa sổ. Cũng là dung tích tối đa của xô."
                    },
                    "perMs": {
                        "type": "number",
                        "title": "Độ dài cửa sổ (ms)",
                        "default": DEFAULT_PER_MS,
                        "minimum": 0,
                        "exclusiveMinimum": 0,
                        "description": "Khoảng thời gian (mili-giây) để nạp đủ `rate` token."
                    }
                }
            }))
            .doc(
                "Xô token dung tích `rate`, nạp `rate` token mỗi `perMs` ms (nạp liên \
                 tục theo thời gian thực chứ không giật cục).\n\n\
                 - `out`: còn ≥ 1 token, trừ đi 1 và cho qua.\n\
                 - `dropped`: hết token — vượt hạn mức.\n\n\
                 Xô khởi đầu đầy, nên đợt đầu `rate` message luôn được cho qua.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for RateLimitRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        let as_num = |v: &Value| -> Option<f64> {
            match v {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse().ok(),
                _ => None,
            }
        };
        if let Some(v) = config.get("rate").filter(|v| !v.is_null()) {
            match as_num(v) {
                Some(n) if n > 0.0 => {}
                _ => out.push("Số token mỗi cửa sổ phải là số > 0.".to_string()),
            }
        }
        if let Some(v) = config.get("perMs").filter(|v| !v.is_null()) {
            match as_num(v) {
                Some(n) if n > 0.0 => {}
                _ => out.push("Độ dài cửa sổ (ms) phải là số > 0.".to_string()),
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let rate = ctx.cfg_f64_or("rate", DEFAULT_RATE).max(f64::MIN_POSITIVE);
        let per_ms = ctx
            .cfg_f64_or("perMs", DEFAULT_PER_MS)
            .max(f64::MIN_POSITIVE);
        let now = now_ms();

        // Xô khởi đầu đầy tại thời điểm message đầu tiên.
        let (mut tokens, last) = ctx
            .state_get(SCOPE_BUCKET)
            .and_then(|v| {
                let t = v.get("tokens")?.as_f64()?;
                let l = v.get("last")?.as_i64()?;
                Some((t, l))
            })
            .unwrap_or((rate, now));

        // Nạp token theo thời gian đã trôi, trần bằng dung tích `rate`.
        let elapsed = (now - last) as f64;
        tokens = (tokens + elapsed / per_ms * rate).min(rate);

        let allowed = tokens >= 1.0;
        if allowed {
            tokens -= 1.0;
        }
        ctx.state_set(SCOPE_BUCKET, &json!({ "tokens": tokens, "last": now }));

        Outcome::port(if allowed { PORT_OUT } else { PORT_DROPPED }, msg.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, msg, one};

    #[tokio::test]
    async fn the_first_message_passes_and_the_second_is_dropped() {
        let r = RateLimitRule::new();
        // rate=1, perMs rất lớn → gần như không nạp lại trong lúc test.
        let c = ctx("rate-limit", json!({ "rate": 1, "perMs": 100000 }));
        assert_eq!(one(r.handle(&c, msg(json!({ "x": 1 }))).await).0, PORT_OUT);
        let (port, _) = one(r.handle(&c, msg(json!({ "x": 2 }))).await);
        assert_eq!(port, PORT_DROPPED);
    }

    #[tokio::test]
    async fn a_burst_up_to_rate_all_pass() {
        let r = RateLimitRule::new();
        let c = ctx("rate-limit", json!({ "rate": 3, "perMs": 100000 }));
        for _ in 0..3 {
            assert_eq!(one(r.handle(&c, msg(json!({}))).await).0, PORT_OUT);
        }
        assert_eq!(one(r.handle(&c, msg(json!({}))).await).0, PORT_DROPPED);
    }

    #[test]
    fn validate_rejects_non_positive_settings() {
        let r = RateLimitRule::new();
        assert!(r.validate(&json!({})).is_empty());
        assert!(r.validate(&json!({ "rate": 5, "perMs": 1000 })).is_empty());
        assert!(!r.validate(&json!({ "rate": 0 })).is_empty());
        assert!(!r.validate(&json!({ "perMs": -1 })).is_empty());
    }
}
