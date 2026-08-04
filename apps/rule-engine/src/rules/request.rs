//! `request` — a named synchronous entry point.
//!
//! Like `manual` it owns no timer or socket; it exists so a caller can invoke
//! *this specific* flow and wait for a result. The `rule_call` MCP tool pushes
//! the request payload straight into this node's `out` port, then blocks until
//! the run reaches a `respond` node. Pair one `request` with one `respond`.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::engine::spec::{Category, RuleSpec, SourceCtx, SourceRule};
use crate::engine::types::ChainId;

pub struct RequestSource {
    spec: RuleSpec,
}

pub fn source() -> Arc<dyn SourceRule> {
    Arc::new(RequestSource::new())
}

impl RequestSource {
    fn new() -> Self {
        let spec = RuleSpec::builder("request", "Yêu cầu (đồng bộ)", Category::Source)
            .desc("Điểm vào cho lời gọi đồng bộ: bên gọi `rule_call` bơm dữ liệu vào đây rồi chờ node `respond` trả kết quả.")
            .icon("🎯")
            .color("#722ed1")
            .schema(json!({ "type": "object", "properties": {} }))
            .doc(
                "Không có cấu hình. Dùng cặp với node `respond`.\n\n\
                 - Qua MCP: `rule_call` với `{ \"chainId\": <id>, \"node\": \"<id node request>\", \
                   \"data\": { ... }, \"timeoutMs\": 10000 }`. Trả về `{ status, result, error }` \
                   trong đó `result` là dữ liệu tới node `respond`.\n\
                 - Nếu luồng không có `respond`, lời gọi vẫn trả `status` nhưng `result` rỗng.\n\n\
                 Dữ liệu bơm vào đi thẳng ra cổng `out`.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl SourceRule for RequestSource {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    async fn start(&self, ctx: SourceCtx) -> Result<(), String> {
        ctx.log(
            "info",
            "node `request` sẵn sàng — gọi đồng bộ bằng `rule_call`.",
        );
        Ok(())
    }

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
    async fn start_is_passive() {
        let s = RequestSource::new();
        let (c, mut rx) = source_ctx(1, "n1", json!({}));
        s.start(c).await.unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn spec_is_a_source_with_out_port() {
        let s = RequestSource::new();
        assert!(s.spec().inputs.is_empty());
        assert!(s.spec().has_output("out"));
    }
}
