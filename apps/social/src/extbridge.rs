//! Extension bridge — a dedicated WS server the shared Chrome extension dials
//! (ws://127.0.0.1:9224). Request/response RPC over the socket: outbound
//! commands carry an `id`, the extension answers with a message bearing the same
//! `id` (over WS or via `POST /api/ext/callback`), resolving a pending oneshot.
//!
//! Adapted from `apps/video-flow/src/extbridge.rs`. "Last connection wins."

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
    conn_tx: Mutex<Option<mpsc::UnboundedSender<String>>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Value>>>,
    connected: AtomicBool,
    /// Whether the extension has learned a Facebook composer template (so it can
    /// post via FB's internal GraphQL rather than the DOM fallback).
    fb_composer_ready: AtomicBool,
    connects: AtomicU64,
    disconnects: AtomicU64,
    connected_since: Mutex<Option<Instant>>,
    /// Which hosts the extension currently reports a live session for
    /// (facebook/tiktok/x/instagram/youtube). Updated from heartbeats.
    hosts_ready: Mutex<Vec<String>>,
    /// Identity of the extension currently driving us (name/version/id),
    /// learned from its `hello` / heartbeat. `None` when nothing is connected.
    ext_info: Mutex<Option<ExtInfo>>,
    on_event: Mutex<Option<EventHandler>>,
    /// Called with (came_online, went_offline) whenever hosts_ready changes.
    on_hosts_change: Mutex<Option<HostsChangeHandler>>,
    secret: String,
}

type HostsChangeHandler = Arc<dyn Fn(Vec<String>, Vec<String>) + Send + Sync>;

/// Identity of the connected Chrome extension, as it announces itself.
#[derive(Clone, Debug, Default)]
pub struct ExtInfo {
    pub name: String,
    pub version: String,
    pub ext_id: String,
}

impl ExtBridge {
    pub fn new() -> Self {
        ExtBridge {
            inner: Arc::new(Inner {
                conn_tx: Mutex::new(None),
                pending: Mutex::new(HashMap::new()),
                connected: AtomicBool::new(false),
                fb_composer_ready: AtomicBool::new(false),
                connects: AtomicU64::new(0),
                disconnects: AtomicU64::new(0),
                connected_since: Mutex::new(None),
                hosts_ready: Mutex::new(Vec::new()),
                ext_info: Mutex::new(None),
                on_event: Mutex::new(None),
                on_hosts_change: Mutex::new(None),
                secret: crate::db::new_id(),
            }),
        }
    }

    #[allow(dead_code)]
    pub fn set_event_handler(&self, h: impl Fn(Value) + Send + Sync + 'static) {
        *self.inner.on_event.lock().unwrap() = Some(Arc::new(h));
    }

    /// Register a handler fired on every hosts_ready change: `(came_online,
    /// went_offline)`. Used to persist a login/session history.
    pub fn set_hosts_change_handler(
        &self,
        h: impl Fn(Vec<String>, Vec<String>) + Send + Sync + 'static,
    ) {
        *self.inner.on_hosts_change.lock().unwrap() = Some(Arc::new(h));
    }

    /// Replace hosts_ready, computing the diff and notifying the change handler.
    fn set_hosts(&self, new: Vec<String>) {
        let (added, removed) = {
            let mut guard = self.inner.hosts_ready.lock().unwrap();
            if *guard == new {
                return;
            }
            let added: Vec<String> = new.iter().filter(|h| !guard.contains(h)).cloned().collect();
            let removed: Vec<String> = guard.iter().filter(|h| !new.contains(h)).cloned().collect();
            *guard = new;
            (added, removed)
        };
        if added.is_empty() && removed.is_empty() {
            return;
        }
        if let Some(h) = self.inner.on_hosts_change.lock().unwrap().clone() {
            h(added, removed);
        }
    }

    pub fn is_connected(&self) -> bool {
        self.inner.connected.load(Ordering::Relaxed)
    }

    pub fn fb_composer_ready(&self) -> bool {
        self.inner.fb_composer_ready.load(Ordering::Relaxed)
    }

    pub fn secret(&self) -> &str {
        &self.inner.secret
    }

    pub fn hosts_ready(&self) -> Vec<String> {
        self.inner.hosts_ready.lock().unwrap().clone()
    }

    /// Identity of the extension currently controlling us, if known.
    pub fn ext_info(&self) -> Option<ExtInfo> {
        self.inner.ext_info.lock().unwrap().clone()
    }

    /// A short human label for the connected extension (name + version), or a
    /// fallback string when nothing has identified itself yet.
    pub fn ext_label(&self) -> String {
        match self.ext_info() {
            Some(i) if !i.name.is_empty() && !i.version.is_empty() => {
                format!("{} v{}", i.name, i.version)
            }
            Some(i) if !i.name.is_empty() => i.name,
            _ if self.is_connected() => "extension (chưa định danh)".into(),
            _ => "—".into(),
        }
    }

    fn set_ext_info(&self, name: &str, version: &str, ext_id: &str) {
        if name.is_empty() && version.is_empty() && ext_id.is_empty() {
            return;
        }
        let mut guard = self.inner.ext_info.lock().unwrap();
        let cur = guard.get_or_insert_with(ExtInfo::default);
        if !name.is_empty() {
            cur.name = name.to_string();
        }
        if !version.is_empty() {
            cur.version = version.to_string();
        }
        if !ext_id.is_empty() {
            cur.ext_id = ext_id.to_string();
        }
    }

    pub fn stats(&self) -> Value {
        let uptime = self
            .inner
            .connected_since
            .lock()
            .unwrap()
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        let info = self.ext_info().unwrap_or_default();
        json!({
            "connected": self.is_connected(),
            "connects": self.inner.connects.load(Ordering::Relaxed),
            "disconnects": self.inner.disconnects.load(Ordering::Relaxed),
            "uptime_s": uptime,
            "hosts_ready": self.hosts_ready(),
            "name": info.name,
            "version": info.version,
            "ext_id": info.ext_id,
            "label": self.ext_label(),
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

    pub fn complete_callback(&self, id: &str, msg: Value) -> bool {
        if let Some(tx) = self.inner.pending.lock().unwrap().remove(id) {
            let _ = tx.send(msg);
            true
        } else {
            false
        }
    }

    pub fn send(&self, id: &str, method: &str, params: Value) -> Result<(), String> {
        let msg = json!({ "id": id, "method": method, "params": params }).to_string();
        let guard = self.inner.conn_tx.lock().unwrap();
        match guard.as_ref() {
            Some(tx) if tx.send(msg).is_ok() => Ok(()),
            _ => Err("Extension chưa kết nối".to_string()),
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
                Err(format!("gọi extension '{method}' quá {}s không phản hồi", timeout.as_secs()))
            }
        }
    }

    fn clear_pending(&self) {
        let mut p = self.inner.pending.lock().unwrap();
        for (_, tx) in p.drain() {
            let _ = tx.send(json!({ "error": "Extension disconnected" }));
        }
    }

    /// Test-only: inject a fake connection so `send`/`call` deliver to the
    /// returned receiver (standing in for a real extension WS). The test reads
    /// the outbound `{id,method,params}` and resolves it with `complete_callback`.
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
                *self.inner.ext_info.lock().unwrap() = None;
                drop(guard);
                // Everything goes offline on disconnect (records the transitions).
                self.set_hosts(Vec::new());
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
        // Any inbound frame may carry the extension's identity — capture it
        // wherever it appears (dedicated `hello`, or piggybacked on heartbeats).
        {
            let name = v.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let version = v.get("version").and_then(|x| x.as_str()).unwrap_or("");
            let ext_id = v.get("ext_id").and_then(|x| x.as_str()).unwrap_or("");
            self.set_ext_info(name, version, ext_id);
        }
        match v.get("type").and_then(|x| x.as_str()) {
            Some("hello") => {
                let _ = out_tx.send(json!({ "type": "pong" }).to_string());
            }
            Some("ping") | Some("heartbeat") => {
                if let Some(ready) = v.get("fb_composer_ready").and_then(|b| b.as_bool()) {
                    self.inner.fb_composer_ready.store(ready, Ordering::Relaxed);
                }
                // Heartbeats may carry the list of hosts with a live session.
                if let Some(hosts) = v.get("hosts_ready").and_then(|h| h.as_array()) {
                    let list: Vec<String> = hosts
                        .iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect();
                    self.set_hosts(list);
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    #[test]
    fn hosts_change_handler_fires_with_added_and_removed() {
        let ext = ExtBridge::new();
        let events: Arc<StdMutex<Vec<(Vec<String>, Vec<String>)>>> = Arc::new(StdMutex::new(Vec::new()));
        let sink = events.clone();
        ext.set_hosts_change_handler(move |added, removed| {
            sink.lock().unwrap().push((added, removed));
        });

        ext.set_hosts(vec!["tiktok".into(), "x".into()]); // both come online
        ext.set_hosts(vec!["x".into()]); // tiktok goes offline
        ext.set_hosts(vec!["x".into()]); // no change → no event

        let ev = events.lock().unwrap();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].0, vec!["tiktok".to_string(), "x".to_string()]);
        assert!(ev[0].1.is_empty());
        assert!(ev[1].0.is_empty());
        assert_eq!(ev[1].1, vec!["tiktok".to_string()]);
        assert_eq!(ext.hosts_ready(), vec!["x".to_string()]);
    }

    #[test]
    fn captures_extension_identity_from_hello_and_heartbeat() {
        let ext = ExtBridge::new();
        let (tx, _rx) = mpsc::unbounded_channel::<String>();

        // A `hello` frame announces name + version + id.
        ext.dispatch_inbound(
            json!({ "type": "hello", "name": "SenClaw Social", "version": "0.1.0", "ext_id": "abcdef123456" }),
            &tx,
        );
        let info = ext.ext_info().expect("identity captured");
        assert_eq!(info.name, "SenClaw Social");
        assert_eq!(info.version, "0.1.0");
        assert_eq!(info.ext_id, "abcdef123456");
        assert_eq!(ext.ext_label(), "SenClaw Social v0.1.0");

        // A later heartbeat carrying a bumped version updates it in place.
        ext.dispatch_inbound(
            json!({ "type": "heartbeat", "name": "SenClaw Social", "version": "0.2.0", "hosts_ready": [] }),
            &tx,
        );
        assert_eq!(ext.ext_info().unwrap().version, "0.2.0");
        assert_eq!(ext.ext_info().unwrap().ext_id, "abcdef123456"); // preserved
    }
}

/// Run the dedicated extension WS server on `port`, route "/".
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
            println!("social extension bridge on ws://0.0.0.0:{port}");
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("extension WS server error: {e}");
            }
        }
        Err(e) => eprintln!("cannot bind extension WS port {port}: {e}"),
    }
}
