//! Dashboard WebSocket hub. A broadcast channel fans events out to every
//! connected browser; slow clients drop messages rather than block the emitter.

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::broadcast;

/// Event names pushed to the UI. The rewrite worker is the only emitter.
pub mod event {
    pub const PROCESS_UPDATE: &str = "process:update";
    pub const PROCESS_DELTA: &str = "process:delta";
    pub const PROCESS_COMPLETE: &str = "process:complete";
    pub const PROCESS_FAILED: &str = "process:failed";
    pub const PROCESS_CANCELLED: &str = "process:cancelled";
}

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

    /// Broadcast `{type, data, timestamp}` to all clients. Never blocks.
    ///
    /// The envelope key is `type`. video-flow's web client reads `event`
    /// instead, so its live updates silently never fire and the UI falls back to
    /// polling — don't repeat that mismatch here.
    pub fn emit(&self, event_type: &str, data: Value) {
        let msg = json!({
            "type": event_type,
            "data": data,
            "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        });
        let _ = self.tx.send(msg.to_string());
    }

    pub async fn serve(&self, socket: WebSocket) {
        let mut rx = self.tx.subscribe();
        let (mut sink, mut stream) = socket.split();
        loop {
            tokio::select! {
                m = rx.recv() => match m {
                    Ok(text) => {
                        if sink.send(Message::Text(text)).await.is_err() { break; }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                },
                // Drain reads purely to detect disconnect.
                m = stream.next() => match m {
                    Some(Ok(_)) => continue,
                    _ => break,
                },
            }
        }
    }
}
