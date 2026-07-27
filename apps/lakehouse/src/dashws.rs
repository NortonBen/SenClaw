//! WS dashboard hub (design §6.7): broadcast envelope `{type, data, timestamp}`.
//!
//! Load-bearing:
//!   * Key envelope là **`type`** (không `event`) — video-flow từng chết vì đọc sai key.
//!   * `broadcast::channel(256)` — subscriber chậm rớt message cũ (lag), KHÔNG chặn
//!     runner. Emit không bao giờ được block đường chạy ETL.
//!   * Hub sống trong `AppState`; runner phát `run:status` + `dataset:updated`; route
//!     `GET /api/ws/dashboard` mỗi client subscribe rồi forward.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use crate::api::AppState;

/// Sức chứa ring buffer broadcast. Subscriber chậm rớt bản cũ (lag) thay vì chặn.
const CHANNEL_CAP: usize = 256;

/// Hub phát sự kiện dashboard. Clone rẻ (Arc bên trong broadcast::Sender).
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
        let (tx, _rx) = broadcast::channel(CHANNEL_CAP);
        Self { tx }
    }

    /// Đăng ký một receiver mới (mỗi WS client một cái).
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }

    /// Phát một sự kiện. Envelope `{type, data, timestamp}` serialize sẵn thành text
    /// để mọi subscriber chia sẻ một chuỗi. Không subscriber (send Err) → bỏ qua im lặng.
    pub fn emit(&self, ev_type: &str, data: Value) {
        let envelope = json!({
            "type": ev_type,
            "data": data,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        // to_string không fail với Value hợp lệ; nếu có, bỏ qua thay vì panic runner.
        if let Ok(text) = serde_json::to_string(&envelope) {
            let _ = self.tx.send(text);
        }
    }

    /// Tiện ích: `run:status` (§6.7) — UI invalidate list runs + flow.
    pub fn emit_run_status(&self, run_id: &str, flow_id: &str, status: &str) {
        self.emit(
            "run:status",
            json!({ "run_id": run_id, "flow_id": flow_id, "status": status }),
        );
    }

    /// Tiện ích: `dataset:updated` (§6.7) — UI invalidate datasets.
    pub fn emit_dataset_updated(
        &self,
        namespace: &str,
        name: &str,
        schema_version: Option<i64>,
        row_count: i64,
    ) {
        self.emit(
            "dataset:updated",
            json!({
                "namespace": namespace,
                "name": name,
                "schema_version": schema_version,
                "row_count": row_count,
            }),
        );
    }
}

/// `GET /api/ws/dashboard` — upgrade rồi forward mọi sự kiện hub tới client.
pub async fn ws_dashboard(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    let hub = state.hub.clone();
    ws.on_upgrade(move |socket| client_loop(socket, hub))
}

/// Vòng đời một client: forward broadcast → socket. Ping của client được nuốt; lag
/// (subscriber chậm) chỉ log-skip, không đóng kết nối vì một bản trễ.
async fn client_loop(mut socket: WebSocket, hub: DashHub) {
    let mut rx = hub.subscribe();
    // Gửi một hello để client biết đã kết nối (đồng bộ với các app khác).
    let hello = json!({
        "type": "hello",
        "data": { "ok": true },
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    if socket.send(Message::Text(hello.to_string())).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            // Sự kiện từ hub → đẩy xuống client.
            msg = rx.recv() => match msg {
                Ok(text) => {
                    if socket.send(Message::Text(text)).await.is_err() {
                        break; // client đóng
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    // Subscriber chậm — bỏ qua các bản đã rớt, tiếp tục.
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            // Message từ client: chỉ để phát hiện đóng kết nối (nuốt ping/text).
            inbound = socket.recv() => match inbound {
                Some(Ok(_)) => {}
                _ => break, // None (đóng) hoặc lỗi
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emit_reaches_subscriber_with_type_key() {
        let hub = DashHub::new();
        let mut rx = hub.subscribe();
        hub.emit_run_status("r1", "f1", "success");
        let text = rx.recv().await.unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        // Key PHẢI là `type` (không `event`).
        assert_eq!(v["type"], json!("run:status"));
        assert_eq!(v["data"]["run_id"], json!("r1"));
        assert_eq!(v["data"]["flow_id"], json!("f1"));
        assert_eq!(v["data"]["status"], json!("success"));
        assert!(v["timestamp"].is_string());
    }

    #[tokio::test]
    async fn dataset_updated_shape() {
        let hub = DashHub::new();
        let mut rx = hub.subscribe();
        hub.emit_dataset_updated("raw", "orders", Some(3), 42);
        let v: Value = serde_json::from_str(&rx.recv().await.unwrap()).unwrap();
        assert_eq!(v["type"], json!("dataset:updated"));
        assert_eq!(v["data"]["namespace"], json!("raw"));
        assert_eq!(v["data"]["name"], json!("orders"));
        assert_eq!(v["data"]["schema_version"], json!(3));
        assert_eq!(v["data"]["row_count"], json!(42));
    }

    #[tokio::test]
    async fn emit_with_no_subscriber_is_silent() {
        let hub = DashHub::new();
        // Không subscriber — send trả Err, emit nuốt, không panic.
        hub.emit("run:status", json!({ "run_id": "x" }));
    }
}
