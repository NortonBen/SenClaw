//! `webhook` — start a run from an inbound HTTP POST.
//!
//! `start` only registers the node's `Emitter` under its webhook id; the axum
//! handler in `api.rs` looks the id up and emits. Nothing here blocks, and
//! `stop` really unregisters — the Go engine never called `Stop()`, so a
//! redeployed chain kept its old listeners alive alongside the new ones.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::engine::spec::{Category, RuleSpec, SourceCtx, SourceRule};
use crate::engine::types::ChainId;
use crate::rules::RouteMap;

/// webhook id → the emitters listening on it. Read by the HTTP handler.
pub fn routes() -> &'static RouteMap {
    static R: std::sync::OnceLock<RouteMap> = std::sync::OnceLock::new();
    R.get_or_init(RouteMap::new)
}

/// The shared secret the HTTP handler must check, if the node set one.
pub fn secret_of(config: &Value) -> Option<String> {
    config
        .get("secret")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Whether the HTTP handler should attach the request headers to `meta.headers`.
///
/// Off by default. The actual attaching (and stripping of secret headers) happens
/// in the axum ingress handler in `api.rs`; this is the single place that reads
/// the node's `includeHeaders` flag so the schema and the handler agree.
pub fn include_headers(config: &Value) -> bool {
    config
        .get("includeHeaders")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn id_is_safe(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub struct WebhookSource {
    spec: RuleSpec,
}

pub fn source() -> Arc<dyn SourceRule> {
    Arc::new(WebhookSource::new())
}

impl WebhookSource {
    fn new() -> Self {
        let spec = RuleSpec::builder("webhook", "Webhook", Category::Source)
            .desc("Nhận HTTP POST từ hệ thống ngoài và bắt đầu một lần chạy.")
            .icon("📥")
            .color("#fa8c16")
            .schema(json!({
                "type": "object",
                "required": ["webhookId"],
                "properties": {
                    "webhookId": {
                        "type": "string",
                        "title": "Webhook ID",
                        "placeholder": "canh-bao-kho-a",
                        "description": "Chuỗi duy nhất, chỉ gồm chữ, số, `_` và `-`. Nằm trong URL."
                    },
                    "secret": {
                        "type": "string",
                        "title": "Secret",
                        "ui": "password",
                        "description": "Bỏ trống là không kiểm. Có giá trị thì request phải gửi kèm header `X-Webhook-Secret`."
                    },
                    "includeHeaders": {
                        "type": "boolean",
                        "title": "Kèm headers",
                        "default": false,
                        "description": "Bật thì headers của request được đưa vào `meta.headers`."
                    }
                }
            }))
            .doc(
                "URL: `POST /api/hooks/<webhookId>`\n\n\
                 - Body JSON của request đi thẳng vào `data` (không có lớp bọc nhánh).\n\
                 - **Kèm headers** mặc định TẮT: headers KHÔNG vào `meta`. Bật lên thì \
                   headers nằm ở `meta.headers`, nhưng các header bí mật \
                   (`x-webhook-secret`, `authorization`, `cookie`…) vẫn bị lọc bỏ để \
                   không rò ra downstream.\n\
                 - Có **Secret** thì request phải gửi header `X-Webhook-Secret` khớp, \
                   nếu không sẽ bị từ chối 401 và không có lần chạy nào.\n\
                 - Mỗi request là một lần chạy mới.\n\n\
                 Nhiều node ở nhiều chain có thể dùng chung một `webhookId`: tất cả \
                 cùng được kích hoạt.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl SourceRule for WebhookSource {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("webhookId").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => {
                if !id_is_safe(s.trim()) {
                    out.push(
                        "Webhook ID chỉ được chứa chữ cái, chữ số, `_` và `-` (nó nằm trong URL)."
                            .to_string(),
                    );
                }
            }
            _ => out.push("Thiếu Webhook ID.".to_string()),
        }
        out
    }

    async fn start(&self, ctx: SourceCtx) -> Result<(), String> {
        let Some(id) = ctx.cfg_str("webhookId") else {
            return Err("Thiếu Webhook ID.".to_string());
        };
        let id = id.trim().to_string();
        if !id_is_safe(&id) {
            return Err(
                "Webhook ID chỉ được chứa chữ cái, chữ số, `_` và `-` (nó nằm trong URL)."
                    .to_string(),
            );
        }
        routes().add(&id, ctx.emitter.clone());
        ctx.log("info", format!("webhook sẵn sàng tại POST /api/hooks/{id}"));
        Ok(())
    }

    async fn stop(&self, chain_id: ChainId, node: &str) {
        routes().remove(chain_id, node);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::engine::services::{EventBus, Services};
    use crate::engine::spec::{Emitter, Ingress};
    use crate::engine::types::PORT_OUT;

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
    async fn start_registers_a_route_and_stop_removes_it() {
        let s = WebhookSource::new();
        let (c, _rx) = source_ctx(11, "n1", json!({ "webhookId": "hook-a" }));
        s.start(c).await.unwrap();
        assert_eq!(routes().get("hook-a").len(), 1);

        s.stop(11, "n1").await;
        assert!(routes().get("hook-a").is_empty());
    }

    #[tokio::test]
    async fn the_registered_emitter_starts_a_run_on_the_out_port() {
        let s = WebhookSource::new();
        let (c, mut rx) = source_ctx(12, "n1", json!({ "webhookId": "hook-b" }));
        s.start(c).await.unwrap();

        let emitter = routes().get("hook-b").remove(0);
        emitter
            .emit(
                PORT_OUT,
                json!({ "temp": 31 }),
                json!({ "headers": { "x": "1" } }),
            )
            .await;
        let ing = rx.recv().await.unwrap();
        assert_eq!(ing.chain_id, 12);
        assert_eq!(ing.node, "n1");
        assert_eq!(ing.port, PORT_OUT);
        assert_eq!(ing.data["temp"], 31);
        assert_eq!(ing.meta["headers"]["x"], "1");

        s.stop(12, "n1").await;
    }

    /// Two chains may share one webhook id without shadowing each other.
    #[tokio::test]
    async fn two_nodes_can_share_one_webhook_id() {
        let s = WebhookSource::new();
        let (a, _ra) = source_ctx(13, "n1", json!({ "webhookId": "hook-c" }));
        let (b, _rb) = source_ctx(14, "n1", json!({ "webhookId": "hook-c" }));
        s.start(a).await.unwrap();
        s.start(b).await.unwrap();
        assert_eq!(routes().get("hook-c").len(), 2);

        s.stop(13, "n1").await;
        assert_eq!(routes().get("hook-c").len(), 1);
        s.stop(14, "n1").await;
        assert!(routes().get("hook-c").is_empty());
    }

    #[tokio::test]
    async fn start_refuses_an_unsafe_id() {
        let s = WebhookSource::new();
        let (c, _rx) = source_ctx(15, "n1", json!({ "webhookId": "a/../b" }));
        let err = s.start(c).await.unwrap_err();
        assert!(err.contains("chỉ được chứa"), "{err}");
        assert!(routes().get("a/../b").is_empty());
    }

    #[test]
    fn validate_checks_presence_and_charset() {
        let s = WebhookSource::new();
        assert!(!s.validate(&json!({})).is_empty());
        assert!(!s.validate(&json!({ "webhookId": "có dấu" })).is_empty());
        assert!(!s.validate(&json!({ "webhookId": "a b" })).is_empty());
        assert!(s.validate(&json!({ "webhookId": "kho_A-1" })).is_empty());
    }

    #[test]
    fn include_headers_defaults_to_false() {
        assert!(!include_headers(&json!({})));
        assert!(!include_headers(&json!({ "includeHeaders": false })));
        assert!(include_headers(&json!({ "includeHeaders": true })));
    }

    #[test]
    fn secret_of_ignores_blank_secrets() {
        assert_eq!(
            secret_of(&json!({ "secret": "s3cr3t" })).as_deref(),
            Some("s3cr3t")
        );
        assert_eq!(secret_of(&json!({ "secret": "   " })), None);
        assert_eq!(secret_of(&json!({})), None);
    }
}
