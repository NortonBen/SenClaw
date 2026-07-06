use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::Response;
use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::api::AppState;

/// WebSocket endpoint backing the integrated terminal. Spawns the user's login
/// shell in a PTY rooted at the open workspace and bridges it to xterm.js.
///
/// Client → server frames are JSON text: `{"d":"keystrokes"}` for input or
/// `{"r":[cols,rows]}` to resize. Server → client frames are raw PTY bytes
/// (binary), written straight into the terminal.
pub async fn terminal_ws(State(s): State<Arc<AppState>>, ws: WebSocketUpgrade) -> Response {
    let root = s.root.lock().unwrap().clone();
    ws.on_upgrade(move |socket| handle(socket, root))
}

async fn handle(socket: WebSocket, root: Option<PathBuf>) {
    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }) {
        Ok(p) => p,
        Err(_) => return,
    };

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
    let mut cmd = CommandBuilder::new(shell);
    if let Some(r) = &root {
        cmd.cwd(r);
    } else if let Ok(home) = std::env::var("HOME") {
        cmd.cwd(home);
    }
    cmd.env("TERM", "xterm-256color");

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(_) => return,
    };
    drop(pair.slave);

    let mut reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut writer = match pair.master.take_writer() {
        Ok(w) => w,
        Err(_) => return,
    };
    let master = Arc::new(Mutex::new(pair.master));

    // PTY output → async task (tokio unbounded; sender is sync-callable).
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if out_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Client input → PTY writer thread (std channel; async side sends without blocking).
    let (in_tx, in_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        while let Ok(bytes) = in_rx.recv() {
            if writer.write_all(&bytes).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    });

    let (mut sink, mut stream) = socket.split();
    let out_task = tokio::spawn(async move {
        while let Some(chunk) = out_rx.recv().await {
            if sink.send(Message::Binary(chunk)).await.is_err() {
                break;
            }
        }
        let _ = sink.send(Message::Close(None)).await;
    });

    while let Some(Ok(msg)) = stream.next().await {
        match msg {
            Message::Text(t) => {
                if let Ok(v) = serde_json::from_str::<Value>(&t) {
                    if let Some(d) = v.get("d").and_then(|x| x.as_str()) {
                        let _ = in_tx.send(d.as_bytes().to_vec());
                    } else if let Some(r) = v.get("r").and_then(|x| x.as_array()) {
                        let cols = r.first().and_then(|x| x.as_u64()).unwrap_or(80) as u16;
                        let rows = r.get(1).and_then(|x| x.as_u64()).unwrap_or(24) as u16;
                        let _ = master.lock().unwrap().resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                }
            }
            Message::Binary(b) => {
                let _ = in_tx.send(b);
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    let _ = child.kill();
    out_task.abort();
}
