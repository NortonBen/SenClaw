//! Dashboard WebSocket broadcast hub.

use serde_json::{json, Value};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct DashHub {
    tx: broadcast::Sender<String>,
}

impl Default for DashHub {
    fn default() -> Self {
        Self::new()
    }
}

impl DashHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        DashHub { tx }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    /// The envelope key is `type`, not `event`.
    ///
    /// video-flow's client reads `event` while its server sends `type`, so its
    /// live updates never fire. Both ends here agree on `type`.
    pub fn emit(&self, event_type: &str, data: Value) {
        let msg = json!({
            "type": event_type,
            "data": data,
            "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        });
        let _ = self.tx.send(msg.to_string());
    }
}
