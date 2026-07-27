//! `telegram-hook` — start a run from a Telegram webhook update.
//!
//! Same shape as `webhook`, but with its own `RouteMap` keyed by the secret URL
//! token, so a Telegram token can never collide with a hand-written webhook id.
//!
//! The Go adapter built its inbound message with `SessionId = 0`, which the
//! router treated as "no session" and dropped; here the inbound update goes
//! through the node's `Emitter`, which mints a fresh run id per update.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::engine::spec::{Category, RuleSpec, SourceCtx, SourceRule};
use crate::engine::types::ChainId;
use crate::rules::RouteMap;

/// URL token → the emitters listening on it. Read by the HTTP handler.
pub fn routes() -> &'static RouteMap {
    static R: std::sync::OnceLock<RouteMap> = std::sync::OnceLock::new();
    R.get_or_init(RouteMap::new)
}

/// The `meta` the HTTP handler attaches to an inbound update.
pub fn meta_for(token: &str) -> Value {
    json!({ "_event": "telegram", "token": token })
}

fn token_is_safe(token: &str) -> bool {
    !token.is_empty()
        && token
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

pub struct TelegramHookSource {
    spec: RuleSpec,
}

pub fn source() -> Arc<dyn SourceRule> {
    Arc::new(TelegramHookSource::new())
}

impl TelegramHookSource {
    fn new() -> Self {
        let spec = RuleSpec::builder("telegram-hook", "Telegram Webhook", Category::Source)
            .desc("Nhận update từ Telegram Bot API và bắt đầu một lần chạy.")
            .icon("🤖")
            .color("#13c2c2")
            .schema(json!({
                "type": "object",
                "required": ["token"],
                "properties": {
                    "token": {
                        "type": "string",
                        "title": "Token đường dẫn",
                        "ui": "password",
                        "placeholder": "chuoi-bi-mat-trong-url",
                        "description": "Chuỗi bí mật nằm trong URL webhook. Chỉ gồm chữ, số, `_` và `-`. Không phải bot token."
                    }
                }
            }))
            .doc(
                "URL: `POST /api/hooks/telegram/<token>`\n\n\
                 Khai báo URL này với Telegram bằng `setWebhook`. Vì token nằm ngay trong \
                 đường dẫn, hãy dùng một chuỗi ngẫu nhiên dài và **không** dùng lại bot token.\n\n\
                 - Toàn bộ Telegram update đi vào `data` (ví dụ `message.text`, \
                   `message.chat.id`).\n\
                 - `meta` gồm `{ \"_event\": \"telegram\", \"token\": ... }`.\n\
                 - Mỗi update là một lần chạy mới.",
            )
            .build();
        Self { spec }
    }
}

#[async_trait]
impl SourceRule for TelegramHookSource {
    fn spec(&self) -> &RuleSpec {
        &self.spec
    }

    fn validate(&self, config: &Value) -> Vec<String> {
        let mut out = Vec::new();
        match config.get("token").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => {
                if !token_is_safe(s.trim()) {
                    out.push(
                        "Token chỉ được chứa chữ cái, chữ số, `_` và `-` (nó nằm trong URL)."
                            .to_string(),
                    );
                }
            }
            _ => out.push("Thiếu token đường dẫn.".to_string()),
        }
        out
    }

    async fn start(&self, ctx: SourceCtx) -> Result<(), String> {
        let Some(token) = ctx.cfg_str("token") else {
            return Err("Thiếu token đường dẫn.".to_string());
        };
        let token = token.trim().to_string();
        if !token_is_safe(&token) {
            return Err(
                "Token chỉ được chứa chữ cái, chữ số, `_` và `-` (nó nằm trong URL).".to_string(),
            );
        }
        routes().add(&token, ctx.emitter.clone());
        // The token is a secret in the URL — never log it.
        ctx.log(
            "info",
            "telegram webhook sẵn sàng tại POST /api/hooks/telegram/<token>",
        );
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
    async fn start_registers_the_token_and_stop_removes_it() {
        let s = TelegramHookSource::new();
        let (c, _rx) = source_ctx(31, "n1", json!({ "token": "tok-a" }));
        s.start(c).await.unwrap();
        assert_eq!(routes().get("tok-a").len(), 1);
        s.stop(31, "n1").await;
        assert!(routes().get("tok-a").is_empty());
    }

    /// Every update becomes its own run — the Go adapter used `SessionId = 0`
    /// and the router threw the message away.
    #[tokio::test]
    async fn an_update_reaches_the_engine_with_telegram_meta() {
        let s = TelegramHookSource::new();
        let (c, mut rx) = source_ctx(32, "n1", json!({ "token": "tok-b" }));
        s.start(c).await.unwrap();

        let update =
            json!({ "update_id": 9, "message": { "text": "xin chào", "chat": { "id": 7 } } });
        let emitter = routes().get("tok-b").remove(0);
        emitter.emit(PORT_OUT, update, meta_for("tok-b")).await;

        let ing = rx.recv().await.unwrap();
        assert_eq!(ing.chain_id, 32);
        assert_eq!(ing.node, "n1");
        assert_eq!(ing.data["message"]["text"], "xin chào");
        assert_eq!(ing.meta["_event"], "telegram");
        assert_eq!(ing.meta["token"], "tok-b");

        s.stop(32, "n1").await;
    }

    #[tokio::test]
    async fn the_token_never_appears_in_the_log() {
        let s = TelegramHookSource::new();
        let (c, _rx) = source_ctx(33, "n1", json!({ "token": "sieu-bi-mat" }));
        let db = c.svc.db.clone();
        s.start(c).await.unwrap();
        let logs = db.list_logs(33, 10).unwrap();
        assert_eq!(logs.len(), 1);
        assert!(
            !logs[0].message.contains("sieu-bi-mat"),
            "{}",
            logs[0].message
        );
        s.stop(33, "n1").await;
    }

    #[tokio::test]
    async fn start_refuses_a_missing_or_unsafe_token() {
        let s = TelegramHookSource::new();
        let (c, _rx) = source_ctx(34, "n1", json!({}));
        assert!(s.start(c).await.is_err());

        let (c, _rx) = source_ctx(35, "n1", json!({ "token": "a/b" }));
        assert!(s.start(c).await.unwrap_err().contains("chỉ được chứa"));
    }

    #[test]
    fn validate_requires_a_url_safe_token() {
        let s = TelegramHookSource::new();
        assert!(!s.validate(&json!({})).is_empty());
        assert!(!s.validate(&json!({ "token": "  " })).is_empty());
        assert!(!s.validate(&json!({ "token": "a b" })).is_empty());
        assert!(s.validate(&json!({ "token": "Abc_123-xyz" })).is_empty());
    }

    #[test]
    fn meta_names_the_event_and_the_token() {
        assert_eq!(
            meta_for("t1"),
            json!({ "_event": "telegram", "token": "t1" })
        );
    }
}
