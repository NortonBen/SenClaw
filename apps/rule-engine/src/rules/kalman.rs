//! `kalman` — scalar Kalman filter over one numeric field.
//!
//! Same recurrence as the Go rule (`pPred = p + q`, `k = pPred / (pPred + r)`),
//! but the estimate lives in node state instead of a Redis key, and a zero
//! denominator is reported instead of producing NaN.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const DEFAULT_R: f64 = 1.0;
const DEFAULT_Q: f64 = 0.1;
const DEFAULT_P: f64 = 1.0;

pub struct KalmanRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(KalmanRule::new())
}

fn scope_of(field: &str) -> String {
    format!("kalman:{field}")
}

impl KalmanRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("kalman", "Lọc Kalman", Category::Filter)
            .desc("Làm mượt một chuỗi số bằng bộ lọc Kalman một chiều.")
            .icon("〰️")
            .color("#722ed1")
            .schema(json!({
                "type": "object",
                "required": ["field"],
                "properties": {
                    "field": {
                        "type": "string",
                        "title": "Field cần lọc",
                        "placeholder": "temperature"
                    },
                    "r": {
                        "type": "number",
                        "title": "Nhiễu đo (R)",
                        "default": DEFAULT_R,
                        "description": "Càng lớn càng tin ước lượng cũ, kết quả mượt và chậm."
                    },
                    "q": {
                        "type": "number",
                        "title": "Nhiễu tiến trình (Q)",
                        "default": DEFAULT_Q,
                        "description": "Càng lớn càng tin giá trị mới, kết quả bám nhanh."
                    },
                    "p": {
                        "type": "number",
                        "title": "Hiệp phương sai ban đầu (P)",
                        "default": DEFAULT_P
                    },
                    "initial": {
                        "type": "number",
                        "title": "Ước lượng ban đầu",
                        "description": "Bỏ trống thì lấy chính giá trị đầu tiên đọc được."
                    },
                    "outputField": {
                        "type": "string",
                        "title": "Ghi kết quả vào field",
                        "placeholder": "temperature_smooth",
                        "description": "Bỏ trống thì ghi đè lên chính field gốc."
                    }
                }
            }))
            .doc(
                "Bộ lọc Kalman vô hướng:\n\n\
                 ```\n\
                 pPred = p + q\n\
                 k     = pPred / (pPred + r)\n\
                 x     = x + k * (z - x)\n\
                 p     = (1 - k) * pPred\n\
                 ```\n\n\
                 `x` và `p` được lưu trong state của node nên bền qua từng message. \
                 Lần chạy đầu tiên `x` lấy `initial`, nếu không có thì lấy luôn giá trị đo. \
                 `r + pPred = 0` sẽ báo lỗi thay vì trả về NaN.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for KalmanRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("field").and_then(|v| v.as_str()) {
            None | Some("") => out.push("Thiếu field cần lọc.".to_string()),
            Some(_) => {}
        }
        for key in ["r", "q", "p"] {
            if let Some(v) = config.get(key).filter(|v| !v.is_null()) {
                let n = match v {
                    Value::Number(n) => n.as_f64(),
                    Value::String(s) => s.trim().parse().ok(),
                    _ => None,
                };
                match n {
                    Some(x) if x >= 0.0 && x.is_finite() => {}
                    _ => out.push(format!("Tham số `{key}` phải là số không âm.")),
                }
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let Some(field) = ctx.cfg_str("field") else {
            return ctx.fail_config("Thiếu field cần lọc.");
        };
        let r = ctx.cfg_f64_or("r", DEFAULT_R);
        let q = ctx.cfg_f64_or("q", DEFAULT_Q);
        let p0 = ctx.cfg_f64_or("p", DEFAULT_P);

        let Some(z) = daq::get_f64(&msg.data, &field) else {
            return ctx.fail_runtime(format!(
                "Không đọc được số ở field `{field}` (thiếu, null, hoặc không phải số)."
            ));
        };

        let scope = scope_of(&field);
        let state = ctx.state_get(&scope);
        let (mut x, p_prev) = match state.as_ref() {
            Some(s) => (
                s.get("x").and_then(|v| v.as_f64()).unwrap_or(z),
                s.get("p").and_then(|v| v.as_f64()).unwrap_or(p0),
            ),
            // First sample: seed the estimate so the filter does not spend the
            // first few messages crawling up from zero.
            None => (ctx.cfg_f64("initial").unwrap_or(z), p0),
        };

        let p_pred = p_prev + q;
        if p_pred + r == 0.0 {
            return ctx.fail_config(
                "R + (P + Q) = 0 nên không tính được hệ số Kalman. Đặt R hoặc Q khác 0.",
            );
        }
        let k = p_pred / (p_pred + r);
        x += k * (z - x);
        let p = (1.0 - k) * p_pred;

        if !x.is_finite() || !p.is_finite() {
            return ctx.fail_runtime("Kết quả lọc Kalman không hữu hạn (NaN/vô cực).");
        }
        ctx.state_set(&scope, &json!({ "x": x, "p": p }));

        let target = ctx.cfg_str("outputField").unwrap_or(field);
        let mut data = msg.data;
        daq::set(&mut data, &target, json!(x));
        Outcome::out(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, one};

    #[tokio::test]
    async fn a_constant_signal_converges_to_itself() {
        let r = KalmanRule::new();
        let c = ctx("kalman", json!({ "field": "temp" }));
        let mut last = 0.0;
        for _ in 0..5 {
            let (port, data) = one(r.handle(&c, msg(json!({ "temp": 10.0 }))).await);
            assert_eq!(port, "out");
            last = data["temp"].as_f64().unwrap();
        }
        assert!((last - 10.0).abs() < 1e-9, "{last}");
    }

    #[tokio::test]
    async fn a_jump_is_smoothed_towards_the_new_value() {
        let r = KalmanRule::new();
        let c = ctx("kalman", json!({ "field": "temp", "r": 5.0, "q": 0.1 }));
        for _ in 0..5 {
            one(r.handle(&c, msg(json!({ "temp": 10.0 }))).await);
        }
        let (_, data) = one(r.handle(&c, msg(json!({ "temp": 20.0 }))).await);
        let x = data["temp"].as_f64().unwrap();
        assert!(x > 10.0 && x < 20.0, "phải nằm giữa cũ và mới, nhận {x}");
    }

    #[tokio::test]
    async fn the_estimate_survives_between_messages() {
        let r = KalmanRule::new();
        let c = ctx("kalman", json!({ "field": "temp" }));
        one(r.handle(&c, msg(json!({ "temp": 4.0 }))).await);
        let first = c.state_get(&scope_of("temp")).unwrap();
        one(r.handle(&c, msg(json!({ "temp": 6.0 }))).await);
        let second = c.state_get(&scope_of("temp")).unwrap();
        assert_eq!(first["x"].as_f64().unwrap(), 4.0);
        assert!(second["x"].as_f64().unwrap() > 4.0);
        // Covariance shrinks as evidence accumulates.
        assert!(second["p"].as_f64().unwrap() < first["p"].as_f64().unwrap());
    }

    #[tokio::test]
    async fn initial_seeds_the_first_estimate() {
        let r = KalmanRule::new();
        let c = ctx(
            "kalman",
            json!({ "field": "temp", "initial": 0.0, "r": 100.0, "q": 0.0, "p": 1.0 }),
        );
        let (_, data) = one(r.handle(&c, msg(json!({ "temp": 100.0 }))).await);
        // k = 1/101, so the estimate barely leaves the seed.
        assert!(data["temp"].as_f64().unwrap() < 2.0);
    }

    #[tokio::test]
    async fn output_field_leaves_the_raw_value_alone() {
        let r = KalmanRule::new();
        let c = ctx(
            "kalman",
            json!({ "field": "temp", "outputField": "temp_smooth" }),
        );
        let (_, data) = one(r.handle(&c, msg(json!({ "temp": 7.0 }))).await);
        assert_eq!(data["temp"], 7.0);
        assert_eq!(data["temp_smooth"], 7.0);
    }

    #[tokio::test]
    async fn a_zero_denominator_fails_instead_of_producing_nan() {
        let r = KalmanRule::new();
        let c = ctx(
            "kalman",
            json!({ "field": "temp", "r": 0.0, "q": 0.0, "p": 0.0 }),
        );
        let err = failure(r.handle(&c, msg(json!({ "temp": 1.0 }))).await);
        assert!(err.contains("hệ số Kalman"), "{err}");
    }

    #[tokio::test]
    async fn an_unreadable_field_fails() {
        let r = KalmanRule::new();
        let c = ctx("kalman", json!({ "field": "temp" }));
        let err = failure(r.handle(&c, msg(json!({ "other": 1 }))).await);
        assert!(err.contains("Không đọc được số"), "{err}");
    }

    #[test]
    fn validate_rejects_a_missing_field_and_negative_noise() {
        let r = KalmanRule::new();
        assert!(!r.validate(&json!({})).is_empty());
        assert!(!r.validate(&json!({ "field": "t", "r": -1 })).is_empty());
        assert!(r
            .validate(&json!({ "field": "t", "r": 2, "q": 0.5 }))
            .is_empty());
    }
}
