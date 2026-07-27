//! Dashboard WebSocket hub — port of `internal/bridge/dashws.go`. A broadcast
//! channel fans events out to every connected dashboard; slow clients drop
//! messages rather than block the emitter.

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct DashHub {
    tx: broadcast::Sender<String>,
}

impl DashHub {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        DashHub { tx }
    }

    /// Broadcast `{type, data, timestamp}` to all dashboards. Never blocks.
    pub fn emit(&self, event_type: &str, data: Value) {
        let msg = json!({
            "type": event_type,
            "data": data,
            "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true),
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
                        if sink.send(Message::Text(text)).await.is_err() {
                            break;
                        }
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
