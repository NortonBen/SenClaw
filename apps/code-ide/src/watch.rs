use crate::api::AppState;
use notify::{EventKind, RecursiveMode, Watcher};
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

/// Install a recursive filesystem watcher on `root`. External changes (edits
/// made outside the editor, git checkouts, etc.) are broadcast on
/// `state.events_tx` as JSON `{ kind, paths: [relative…] }` and streamed to the
/// UI over `/api/events`. Replaces any previously installed watcher.
pub fn install_watcher(state: &Arc<AppState>, root: &Path) {
    let root = root.to_path_buf();
    let tx = state.events_tx.clone();
    let root_for_cb = root.clone();

    let mut watcher = match notify::recommended_watcher(
        move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            let kind = match event.kind {
                EventKind::Create(_) => "create",
                EventKind::Modify(_) => "modify",
                EventKind::Remove(_) => "remove",
                _ => return,
            };
            let paths: Vec<String> = event
            .paths
            .iter()
            .filter(|p| !p.components().any(|c| {
                matches!(c, std::path::Component::Normal(n)
                    if n == std::ffi::OsStr::new(".git") || n == std::ffi::OsStr::new("node_modules"))
            }))
            .map(|p| {
                p.strip_prefix(&root_for_cb)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
            if paths.is_empty() {
                return;
            }
            let _ = tx.send(json!({ "kind": kind, "paths": paths }).to_string());
        },
    ) {
        Ok(w) => w,
        Err(_) => return,
    };

    if watcher.watch(&root, RecursiveMode::Recursive).is_ok() {
        *state.watcher.lock().unwrap() = Some(watcher);
    }
}
