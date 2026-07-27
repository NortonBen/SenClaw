//! `moving-average` — sliding-window smoothing used as a noise gate.
//!
//! The Go filter kept the window in Redis under
//! `rule:ma:{chan}:{node}:{branch}:{field}`; `StateStore` is already scoped by
//! chain + node, so the scope here only has to name the field.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::daq;
use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{Message, Outcome};

const DEFAULT_WINDOW: u64 = 5;

pub struct MovingAverageRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(MovingAverageRule::new())
}

fn scope_of(field: &str) -> String {
    format!("ma:{field}")
}

impl MovingAverageRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("moving-average", "Trung bình trượt", Category::Filter)
            .desc("Tính trung bình trượt của một field và tách giá trị nhiễu ra nhánh riêng.")
            .icon("📉")
            .color("#2f54eb")
            .outputs(vec![
                PortSpec::new("pass", "pass")
                    .color("#52c41a")
                    .desc("Giá trị nằm trong ngưỡng so với trung bình"),
                PortSpec::new("noise", "noise")
                    .color("#fa8c16")
                    .desc("Lệch khỏi trung bình quá ngưỡng — nghi là nhiễu"),
            ])
            .schema(json!({
                "type": "object",
                "required": ["field"],
                "properties": {
                    "field": {
                        "type": "string",
                        "title": "Field cần lọc",
                        "placeholder": "temperature",
                        "description": "Đường dẫn trong dữ liệu, ví dụ `sensor.temp` hoặc `values[0]`."
                    },
                    "windowSize": {
                        "type": "integer",
                        "title": "Kích thước cửa sổ",
                        "default": DEFAULT_WINDOW,
                        "minimum": 1,
                        "description": "Số mẫu gần nhất dùng để tính trung bình (kể cả mẫu hiện tại)."
                    },
                    "threshold": {
                        "type": "number",
                        "title": "Ngưỡng lệch",
                        "default": 0,
                        "description": "|giá trị − trung bình| vượt ngưỡng này thì đi ra cổng `noise`."
                    },
                    "outputField": {
                        "type": "string",
                        "title": "Ghi trung bình vào field",
                        "placeholder": "temperature_avg",
                        "description": "Bỏ trống thì không ghi gì, dữ liệu đi tiếp giữ nguyên."
                    }
                }
            }))
            .doc(
                "Giữ cửa sổ trượt `windowSize` mẫu gần nhất của `field` và so mẫu hiện tại \
                 với trung bình cửa sổ.\n\n\
                 - `pass`: |giá trị − trung bình| ≤ ngưỡng\n\
                 - `noise`: vượt ngưỡng\n\
                 - `error`: không đọc được số ở `field`\n\n\
                 Cửa sổ lưu trong state của node (theo chain + node), nên hai chain \
                 dùng cùng một loại node không đè lên nhau.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for MovingAverageRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("field").and_then(|v| v.as_str()) {
            None | Some("") => out.push("Thiếu field cần lọc.".to_string()),
            Some(_) => {}
        }
        if let Some(w) = config.get("windowSize") {
            let n = match w {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse().ok(),
                _ => None,
            };
            match n {
                Some(v) if v >= 1.0 => {}
                _ => out.push("Kích thước cửa sổ phải là số nguyên ≥ 1.".to_string()),
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let Some(field) = ctx.cfg_str("field") else {
            return ctx.fail_config("Thiếu field cần lọc.");
        };
        let window = ctx.cfg_u64_or("windowSize", DEFAULT_WINDOW).max(1) as usize;
        let threshold = ctx.cfg_f64_or("threshold", 0.0);

        let Some(current) = daq::get_f64(&msg.data, &field) else {
            return ctx.fail_runtime(format!(
                "Không đọc được số ở field `{field}` (thiếu, null, hoặc không phải số)."
            ));
        };

        let scope = scope_of(&field);
        let mut window_values: Vec<f64> = ctx
            .state_get(&scope)
            .and_then(|v| serde_json::from_value::<Vec<f64>>(v).ok())
            .unwrap_or_default();
        window_values.push(current);
        if window_values.len() > window {
            let excess = window_values.len() - window;
            window_values.drain(..excess);
        }
        ctx.state_set(&scope, &json!(window_values));

        let avg = window_values.iter().sum::<f64>() / window_values.len() as f64;
        let is_noise = (current - avg).abs() > threshold;

        let mut data = msg.data;
        if let Some(out_field) = ctx.cfg_str("outputField") {
            daq::set(&mut data, &out_field, json!(avg));
        }
        Outcome::port(if is_noise { "noise" } else { "pass" }, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, failure, msg, one};

    #[tokio::test]
    async fn a_steady_series_always_passes() {
        let r = MovingAverageRule::new();
        let c = ctx(
            "moving-average",
            json!({ "field": "temp", "windowSize": 5, "threshold": 1.0 }),
        );
        for _ in 0..6 {
            let (port, _) = one(r.handle(&c, msg(json!({ "temp": 25.0 }))).await);
            assert_eq!(port, "pass");
        }
    }

    #[tokio::test]
    async fn a_spike_goes_out_the_noise_port() {
        let r = MovingAverageRule::new();
        let c = ctx(
            "moving-average",
            json!({ "field": "temp", "windowSize": 5, "threshold": 2.0 }),
        );
        for _ in 0..4 {
            assert_eq!(
                one(r.handle(&c, msg(json!({ "temp": 25 }))).await).0,
                "pass"
            );
        }
        let (port, _) = one(r.handle(&c, msg(json!({ "temp": 90 }))).await);
        assert_eq!(port, "noise");
    }

    #[tokio::test]
    async fn the_window_never_grows_past_its_size() {
        let r = MovingAverageRule::new();
        let c = ctx(
            "moving-average",
            json!({ "field": "temp", "windowSize": 3, "threshold": 1000.0 }),
        );
        for v in [1, 2, 3, 4, 5] {
            one(r.handle(&c, msg(json!({ "temp": v }))).await);
        }
        let stored = c
            .state_get(&scope_of("temp"))
            .expect("cửa sổ phải được lưu");
        assert_eq!(stored, json!([3.0, 4.0, 5.0]));
    }

    #[tokio::test]
    async fn output_field_receives_the_average() {
        let r = MovingAverageRule::new();
        let c = ctx(
            "moving-average",
            json!({
                "field": "temp",
                "windowSize": 2,
                "threshold": 100.0,
                "outputField": "stats.avg"
            }),
        );
        one(r.handle(&c, msg(json!({ "temp": 10 }))).await);
        let (_, data) = one(r.handle(&c, msg(json!({ "temp": 20 }))).await);
        assert_eq!(data["stats"]["avg"], 15.0);
        assert_eq!(data["temp"], 20);
    }

    #[tokio::test]
    async fn numeric_strings_are_accepted_and_junk_fails() {
        let r = MovingAverageRule::new();
        let c = ctx(
            "moving-average",
            json!({ "field": "temp", "threshold": 5.0 }),
        );
        assert_eq!(
            one(r.handle(&c, msg(json!({ "temp": "12.5" }))).await).0,
            "pass"
        );
        let err = failure(r.handle(&c, msg(json!({ "temp": "nóng" }))).await);
        assert!(err.contains("Không đọc được số"), "{err}");
    }

    #[test]
    fn validate_catches_a_missing_field_and_a_zero_window() {
        let r = MovingAverageRule::new();
        assert!(!r.validate(&json!({})).is_empty());
        assert!(!r
            .validate(&json!({ "field": "t", "windowSize": 0 }))
            .is_empty());
        assert!(r
            .validate(&json!({ "field": "t", "windowSize": 5 }))
            .is_empty());
    }
}
