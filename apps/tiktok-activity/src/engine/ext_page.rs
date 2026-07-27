//! `ExtPage` — a `PageOps` whose primitives are RPC calls to the browser
//! extension over the ext-WS bridge. The extension runs each primitive against
//! the single controlled TikTok tab (eval via scripting, mouse/keyboard via the
//! debugger protocol so events are trusted), then replies `{id, result}` or
//! `{id, error}`.

use super::page::PageOps;
use crate::extbridge::ExtBridge;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

pub struct ExtPage {
    bridge: ExtBridge,
}

impl ExtPage {
    pub fn new(bridge: ExtBridge) -> Self {
        Self { bridge }
    }

    /// Call an extension method, unwrapping `{result}` / `{error}`.
    async fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let v = self.bridge.call(method, params, timeout).await.map_err(|e| anyhow!(e))?;
        if let Some(err) = v.get("error").and_then(Value::as_str) {
            if !err.is_empty() {
                return Err(anyhow!("ext {method}: {err}"));
            }
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }
}

#[async_trait]
impl PageOps for ExtPage {
    async fn url(&self) -> String {
        match self.call("url", json!({}), Duration::from_secs(10)).await {
            Ok(Value::String(s)) => s,
            Ok(v) => v.get("url").and_then(Value::as_str).unwrap_or("").to_string(),
            Err(_) => String::new(),
        }
    }

    async fn navigate(&self, url: &str, timeout_ms: u64) -> Result<()> {
        self.call(
            "navigate",
            json!({ "url": url, "timeout_ms": timeout_ms }),
            Duration::from_millis(timeout_ms.max(5000) + 5000),
        )
        .await
        .map(|_| ())
    }

    async fn eval(&self, js: &str) -> Result<Value> {
        self.call("eval", json!({ "js": js }), Duration::from_secs(30)).await
    }

    async fn mouse_click(&self, x: f64, y: f64) -> Result<()> {
        self.call("mouse_click", json!({ "x": x, "y": y }), Duration::from_secs(15)).await.map(|_| ())
    }

    async fn type_chars(&self, text: &str) -> Result<()> {
        self.call("type_text", json!({ "text": text }), Duration::from_secs(60)).await.map(|_| ())
    }

    async fn press_named(&self, key: &str) -> Result<()> {
        self.call("press_key", json!({ "key": key }), Duration::from_secs(15)).await.map(|_| ())
    }

    async fn wheel(&self, x: f64, y: f64, dx: f64, dy: f64) -> Result<()> {
        self.call("wheel", json!({ "x": x, "y": y, "dx": dx, "dy": dy }), Duration::from_secs(15)).await.map(|_| ())
    }
}
