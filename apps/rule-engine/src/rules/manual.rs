//! `manual` — the node you fire by hand.
//!
//! It owns no timer, no socket and no state: `start` only announces itself and
//! returns. Runs come from outside — the "Chạy thử" button on the canvas
//! (`POST /api/chains/:id/trigger`) or the `rule_trigger` MCP tool — both of
//! which push straight into the engine's ingress for this node.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::engine::spec::{Category, RuleSpec, SourceCtx, SourceRule};
use crate::engine::types::ChainId;

pub struct ManualSource {
    spec: RuleSpec,
}

pub fn source() -> Arc<dyn SourceRule> {
    Arc::new(ManualSource::new())
}

impl ManualSource {
    fn new() -> Self {
        let spec = RuleSpec::builder("manual", "Chạy thủ công", Category::Source)
            .desc("Điểm bắt đầu chạy tay: bấm \"Chạy thử\" trên canvas hoặc gọi qua MCP.")
            .icon("▶️")
            .color("#52c41a")
            .schema(json!({ "type": "object", "properties": {} }))
            .doc(
                "Không có cấu hình. Mỗi lần kích hoạt là một lần chạy mới.\n\n\
                 - Trên canvas: nút **Chạy thử** của node.\n\
                 - Qua API: `POST /api/chains/{id}/trigger` với `{ \"node\": \"<id node>\", \
                   \"data\": { ... } }`.\n\
                 - Qua MCP: `rule_trigger`.\n\n\
                 Dữ liệu gửi kèm đi thẳng ra cổng `out`; không gửi gì thì là `{}`.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl SourceRule for ManualSource {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    async fn start(&self, ctx: SourceCtx) -> Result<(), String> {
        ctx.log(
            "info",
            "node `manual` sẵn sàng — kích hoạt bằng nút Chạy thử hoặc `rule_trigger`.",
        );
        Ok(())
    }

    /// Nothing was allocated, so nothing has to be released.
    async fn stop(&self, _chain_id: ChainId, _node: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::engine::services::{EventBus, Services};
    use crate::engine::spec::{Emitter, Ingress};
    use serde_json::Value;

    fn source_ctx(
        chain_id: ChainId,
        node: &str,
        config: Value,
    ) -> (SourceCtx, tokio::sync::mpsc::Receiver<Ingress>) {
        let db = Arc::new(Db::open(":memory:").expect("in-memory db"));
        let _ = db.create_chain(chain_id, "test", "");
        let svc = Arc::new(Services::new(db, EventBus::new()));
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let emitter = Emitter {
            tx,
            chain_id,
            node: node.to_string(),
        };
        (
            SourceCtx {
                chain_id,
                node: node.to_string(),
                config,
                svc,
                emitter,
            },
            rx,
        )
    }

    #[tokio::test]
    async fn start_registers_nothing_and_returns_immediately() {
        let s = ManualSource::new();
        let (c, mut rx) = source_ctx(1, "n1", json!({}));
        let db = c.svc.db.clone();
        s.start(c).await.unwrap();
        // No run is started by `start` itself.
        assert!(rx.try_recv().is_err());
        let logs = db.list_logs(1, 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].message.contains("manual"));
    }

    #[tokio::test]
    async fn stop_is_a_no_op() {
        let s = ManualSource::new();
        s.stop(1, "n1").await;
    }

    #[test]
    fn the_spec_is_a_source_with_a_single_out_port() {
        let s = ManualSource::new();
        assert!(s.spec().inputs.is_empty());
        assert!(s.spec().has_output("out"));
        assert!(s.validate(&json!({})).is_empty());
    }
}
