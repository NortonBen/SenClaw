//! `dedup` — chặn message trùng khoá trong một khoảng thời gian.
//!
//! Lần đầu thấy một khoá thì message đi qua `out`; các lần lặp lại trong cửa sổ
//! `windowMs` đi ra `dropped` để một chain có thể đếm hoặc rẽ nhánh. Bảng khoá
//! đã thấy lưu trong state của node (theo chain + node), giống các key Redis mà
//! bản Go dùng, nhưng tự dọn để không phình vô hạn.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use crate::daq;
use crate::engine::spec::{Category, PortSpec, Rule, RuleSpec, RunCtx};
use crate::engine::types::{now_ms, Message, Outcome};

const SCOPE_SEEN: &str = "seen";
const PORT_OUT: &str = "out";
const PORT_DROPPED: &str = "dropped";
const DEFAULT_WINDOW_MS: f64 = 60_000.0;

pub struct DedupRule {
    spec: RuleSpec,
}

pub fn rule() -> Arc<dyn Rule> {
    Arc::new(DedupRule::new())
}

impl DedupRule {
    fn new() -> Self {
        let spec = RuleSpec::builder("dedup", "Chặn trùng", Category::Filter)
            .desc("Chặn message trùng khoá trong một cửa sổ thời gian; lần đầu cho qua.")
            .icon("🚫")
            .color("#eb2f96")
            .outputs(vec![
                PortSpec::new(PORT_OUT, "out")
                    .color("#52c41a")
                    .desc("Lần đầu thấy khoá — message đi qua nguyên vẹn."),
                PortSpec::new(PORT_DROPPED, "dropped")
                    .color("#fa8c16")
                    .desc("Khoá đã thấy trong cửa sổ — message trùng đi ra đây."),
            ])
            .schema(json!({
                "type": "object",
                "properties": {
                    "key": {
                        "type": "string",
                        "title": "Field làm khoá",
                        "placeholder": "device_id",
                        "description": "Đường dẫn trong dữ liệu dùng làm khoá, ví dụ `order.id`. Bỏ trống thì lấy toàn bộ payload làm khoá."
                    },
                    "windowMs": {
                        "type": "number",
                        "title": "Cửa sổ (ms)",
                        "default": DEFAULT_WINDOW_MS,
                        "minimum": 0,
                        "description": "Khoá được coi là “đã thấy” trong bấy nhiêu mili-giây kể từ lần cuối gặp."
                    }
                }
            }))
            .doc(
                "Giữ bảng `khoá → thời điểm thấy gần nhất` trong state của node.\n\n\
                 - `out`: lần đầu gặp khoá (hoặc khoá cũ đã hết hạn cửa sổ).\n\
                 - `dropped`: gặp lại khoá trong vòng `windowMs` ms.\n\n\
                 Bỏ trống `key` sẽ băm toàn bộ payload làm khoá, hợp cho việc chặn \
                 lặp y hệt. Mỗi lần cho qua, các khoá cũ hơn cửa sổ được dọn để bảng \
                 không phình vô hạn.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl Rule for DedupRule {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(w) = config.get("windowMs").filter(|v| !v.is_null()) {
            let n = match w {
                Value::Number(n) => n.as_f64(),
                Value::String(s) => s.trim().parse().ok(),
                _ => None,
            };
            match n {
                Some(v) if v >= 0.0 => {}
                _ => out.push("Cửa sổ (ms) phải là số ≥ 0.".to_string()),
            }
        }
        out
    }

    async fn handle(&self, ctx: &RunCtx, msg: Message) -> Outcome {
        let window = ctx.cfg_f64_or("windowMs", DEFAULT_WINDOW_MS) as i64;

        // Khoá: một field trong payload, hoặc toàn bộ payload nếu bỏ trống.
        let key_string = match ctx.cfg_str("key") {
            Some(path) => match daq::get(&msg.data, &path) {
                Some(v) => v.to_string(),
                None => "null".to_string(),
            },
            None => msg.data.to_string(),
        };

        let now = now_ms();
        let mut seen: Map<String, Value> = ctx
            .state_get(SCOPE_SEEN)
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();

        if let Some(ts) = seen.get(&key_string).and_then(|v| v.as_i64()) {
            if now - ts < window {
                // Trùng: giữ nguyên thời điểm cũ, không gia hạn cửa sổ.
                return Outcome::port(PORT_DROPPED, msg.data);
            }
        }

        // Lần đầu (hoặc đã hết hạn): ghi lại và dọn khoá cũ để chặn phình.
        seen.insert(key_string, json!(now));
        seen.retain(|_, v| now - v.as_i64().unwrap_or(0) < window);
        ctx.state_set(SCOPE_SEEN, &json!(seen));

        Outcome::port(PORT_OUT, msg.data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{ctx, msg, one};

    #[tokio::test]
    async fn the_first_occurrence_passes_through() {
        let r = DedupRule::new();
        let c = ctx("dedup", json!({ "key": "id", "windowMs": 60000 }));
        let (port, data) = one(r.handle(&c, msg(json!({ "id": "a" }))).await);
        assert_eq!(port, PORT_OUT);
        assert_eq!(data["id"], "a");
    }

    #[tokio::test]
    async fn an_immediate_duplicate_is_dropped() {
        let r = DedupRule::new();
        let c = ctx("dedup", json!({ "key": "id", "windowMs": 60000 }));
        assert_eq!(
            one(r.handle(&c, msg(json!({ "id": "a" }))).await).0,
            PORT_OUT
        );
        let (port, _) = one(r.handle(&c, msg(json!({ "id": "a" }))).await);
        assert_eq!(port, PORT_DROPPED);
    }

    #[tokio::test]
    async fn a_different_key_passes_through() {
        let r = DedupRule::new();
        let c = ctx("dedup", json!({ "key": "id", "windowMs": 60000 }));
        assert_eq!(
            one(r.handle(&c, msg(json!({ "id": "a" }))).await).0,
            PORT_OUT
        );
        let (port, _) = one(r.handle(&c, msg(json!({ "id": "b" }))).await);
        assert_eq!(port, PORT_OUT);
    }

    #[tokio::test]
    async fn an_empty_key_dedups_on_the_whole_payload() {
        let r = DedupRule::new();
        let c = ctx("dedup", json!({ "windowMs": 60000 }));
        assert_eq!(one(r.handle(&c, msg(json!({ "x": 1 }))).await).0, PORT_OUT);
        assert_eq!(
            one(r.handle(&c, msg(json!({ "x": 1 }))).await).0,
            PORT_DROPPED
        );
        assert_eq!(one(r.handle(&c, msg(json!({ "x": 2 }))).await).0, PORT_OUT);
    }

    #[test]
    fn validate_rejects_a_negative_window() {
        let r = DedupRule::new();
        assert!(r.validate(&json!({})).is_empty());
        assert!(r.validate(&json!({ "windowMs": 1000 })).is_empty());
        assert!(!r.validate(&json!({ "windowMs": -1 })).is_empty());
    }
}
