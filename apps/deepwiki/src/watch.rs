use crate::api::AppState;
use crate::db::Db;
use notify::{RecursiveMode, Watcher};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// Background task: coalesces filesystem-change pings and incrementally
/// re-indexes the current repo root, with a short debounce window.
pub fn spawn_reindexer(db: Arc<Db>, mut rx: tokio::sync::mpsc::Receiver<()>) {
    tokio::spawn(async move {
        while rx.recv().await.is_some() {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(800)) => break,
                    msg = rx.recv() => { if msg.is_none() { return; } }
                }
            }
            let Some(root) = db.get_meta("root").ok().flatten() else { continue };
            let db2 = db.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = crate::index::index_repo(&db2, Path::new(&root));
            })
            .await;
        }
    });
}

/// (Re)install a recursive watcher on `root`, replacing any previous one.
pub fn install_watcher(state: &Arc<AppState>, root: &Path) {
    let tx = state.watch_tx.clone();
    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            use notify::EventKind;
            if matches!(ev.kind, EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)) {
                let _ = tx.try_send(());
            }
        }
    }) {
        Ok(w) => w,
        Err(_) => return,
    };
    if watcher.watch(root, RecursiveMode::Recursive).is_ok() {
        *state.watcher.lock().unwrap() = Some(watcher);
    }
}
