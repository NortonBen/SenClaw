//! Extension bridge — a dedicated WS server (default :9223) that the YouTube
//! Chrome extension dials. Request/response RPC over the socket: outbound commands
//! carry an `id`, the extension answers with a message bearing the same `id` (over
//! WS or via `POST /api/ext/callback`), which resolves a pending oneshot.
//!
//! Adapted verbatim from `apps/video-flow/src/extbridge.rs` — the transport is
//! product-agnostic; only the methods the extension understands differ.

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

type EventHandler = Arc<dyn Fn(Value) + Send + Sync>;

#[derive(Clone)]
pub struct ExtBridge {
    inner: Arc<Inner>,
}

struct Inner {
    /// Sender half of the single active connection's outbound queue.
    conn_tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    connected: AtomicBool,
    connects: AtomicU64,
    disconnects: AtomicU64,
    connected_since: Mutex<Option<Instant>>,
    on_event: Mutex<Option<EventHandler>>,
    secret: String,
}

impl ExtBridge {
    pub fn new() -> Self {
        ExtBridge {
            inner: Arc::new(Inner {
                conn_tx: Mutex::new(None),
                pending: Mutex::new(HashMap::new()),
                connected: AtomicBool::new(false),
                connects: AtomicU64::new(0),
                disconnects: AtomicU64::new(0),
                connected_since: Mutex::new(None),
                on_event: Mutex::new(None),
                secret: crate::db::new_id(),
            }),
        }
    }

    pub fn set_event_handler(&self, h: impl Fn(Value) + Send + Sync + 'static) {
        *self.inner.on_event.lock().unwrap() = Some(Arc::new(h));
    }

    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::Relaxed)
    }

    /// The callback secret the extension must present on `POST /api/ext/callback`.
    pub fn secret(&self) -> String {
        self.inner.secret.clone()
    }

    pub fn stats(&self) -> Value {
        let uptime = self
            .inner
            .connected_since
            .lock()
            .unwrap()
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        json!({
            "connected": self.is_connected(),
            "connects": self.inner.connects.load(Ordering::Relaxed),
            "disconnects": self.inner.disconnects.load(Ordering::Relaxed),
            "uptime_s": uptime,
        })
    }

    pub fn register_pending(&self, id: &str) -> oneshot::Receiver<Value> {
        let (tx, rx) = oneshot::channel();
        self.inner.pending.lock().unwrap().insert(id.to_string(), tx);
        rx
    }

    pub fn cancel_pending(&self, id: &str) {
        self.inner.pending.lock().unwrap().remove(id);
    }

    /// Deliver a callback message to whoever is waiting on `id`. True if a waiter existed.
    pub fn complete_callback(&self, id: &str, msg: Value) -> bool {
        if let Some(tx) = self.inner.pending.lock().unwrap().remove(id) {
            let _ = tx.send(msg);
            true
        } else {
            false
        }
    }

    /// Send `{id, method, params}` to the extension. Errors when disconnected.
    pub fn send(&self, id: &str, method: &str, params: Value) -> Result<(), String> {
        let msg = json!({ "id": id, "method": method, "params": params }).to_string();
        let guard = self.inner.conn_tx.lock().unwrap();
        match guard.as_ref() {
            Some(tx) if tx.send(msg).is_ok() => Ok(()),
            _ => Err("Chrome extension chưa kết nối".to_string()),
        }
    }

    /// send + await the correlated callback with a timeout.
    pub async fn call(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = crate::db::new_id();
        let rx = self.register_pending(&id);
        if let Err(e) = self.send(&id, method, params) {
            self.cancel_pending(&id);
            return Err(e);
        }
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(_)) => Err("Extension ngắt kết nối giữa chừng".to_string()),
            Err(_) => {
                self.cancel_pending(&id);
                Err(format!("gọi extension `{method}` quá hạn sau {}s", timeout.as_secs()))
            }
        }
    }

    fn clear_pending(&self) {
        let mut p = self.inner.pending.lock().unwrap();
        for (_, tx) in p.drain() {
            let _ = tx.send(json!({ "error": "Extension disconnected" }));
        }
    }

    /// Test-only: inject a fake connection so `send`/`call` deliver to the returned
    /// receiver (standing in for a real extension WS). The test reads the outbound
    /// `{id,method,params}` and resolves it with `complete_callback`.
    #[cfg(test)]
    pub fn test_connect(&self) -> mpsc::UnboundedReceiver<String> {
        let (tx, rx) = mpsc::unbounded_channel();
        *self.inner.conn_tx.lock().unwrap() = Some(tx);
        self.inner.connected.store(true, Ordering::Relaxed);
        rx
    }

    /// Handle one upgraded extension socket. Last connection wins.
    pub async fn serve(&self, socket: WebSocket) {
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        {
            let mut guard = self.inner.conn_tx.lock().unwrap();
            *guard = Some(out_tx.clone());
        }
        self.inner.connected.store(true, Ordering::Relaxed);
        self.inner.connects.fetch_add(1, Ordering::Relaxed);
        *self.inner.connected_since.lock().unwrap() = Some(Instant::now());

        // Handshake: hand the extension the callback secret.
        let _ = out_tx.send(json!({ "type": "callback_secret", "secret": self.inner.secret }).to_string());

        let (mut sink, mut stream) = socket.split();
        let writer = tokio::spawn(async move {
            while let Some(text) = out_rx.recv().await {
                if sink.send(Message::Text(text)).await.is_err() {
                    break;
                }
            }
        });

        while let Some(Ok(msg)) = stream.next().await {
            let text = match msg {
                Message::Text(t) => t,
                Message::Close(_) => break,
                _ => continue,
            };
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            self.dispatch_inbound(v, &out_tx);
        }

        writer.abort();
        {
            let mut guard = self.inner.conn_tx.lock().unwrap();
            if guard.as_ref().map(|tx| tx.same_channel(&out_tx)).unwrap_or(false) {
                *guard = None;
                self.inner.connected.store(false, Ordering::Relaxed);
                *self.inner.connected_since.lock().unwrap() = None;
            }
        }
        self.inner.disconnects.fetch_add(1, Ordering::Relaxed);
        self.clear_pending();
    }

    fn dispatch_inbound(&self, v: Value, out_tx: &mpsc::UnboundedSender<String>) {
        if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
            if self.complete_callback(id, v.clone()) {
                return;
            }
        }
        match v.get("type").and_then(|x| x.as_str()) {
            Some("ping") => {
                let _ = out_tx.send(json!({ "type": "pong" }).to_string());
            }
            Some(_) => {
                let handler = self.inner.on_event.lock().unwrap().clone();
                if let Some(h) = handler {
                    tokio::spawn(async move {
                        h(v);
                    });
                }
            }
            None => {}
        }
    }
}

/// Run the dedicated extension WS server (route "/").
pub async fn serve_ws(bridge: ExtBridge, port: u16) {
    use axum::routing::get;
    let app = axum::Router::new().route(
        "/",
        get(move |ws: axum::extract::WebSocketUpgrade| {
            let bridge = bridge.clone();
            async move { ws.on_upgrade(move |socket| async move { bridge.serve(socket).await }) }
        }),
    );
    match tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await {
        Ok(listener) => {
            println!("youtube extension bridge on ws://0.0.0.0:{port}");
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("extension WS server error: {e}");
            }
        }
        Err(e) => eprintln!("cannot bind extension WS port {port}: {e}"),
    }
}
