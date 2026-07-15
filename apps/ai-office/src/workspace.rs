//! The office's shared workspace folder: Sếp drops reference documents in
//! (Finder/Explorer or any tool), staff read them as task context and write
//! their deliverables back (`task-<id>/…`). Text-first: only plain-text-ish
//! files are read into context; everything else is just listed.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const TEXT_EXTS: &[&str] = &[
    "md", "markdown", "txt", "csv", "tsv", "json", "yaml", "yml", "toml", "html", "htm", "xml",
    "log", "rs", "ts", "tsx", "js", "py", "sql",
];

fn is_text(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| TEXT_EXTS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub struct WsFile {
    pub rel: String,
    pub size: u64,
    pub modified: i64,
    pub text: bool,
}

/// Recursive listing (depth ≤ 2, max 200 entries), newest first.
pub fn list_files(dir: &Path) -> Vec<WsFile> {
    let mut out = Vec::new();
    collect(dir, dir, 0, &mut out);
    out.sort_by_key(|f| -f.modified);
    out.truncate(200);
    out
}

fn collect(root: &Path, dir: &Path, depth: usize, out: &mut Vec<WsFile>) {
    if depth > 2 || out.len() >= 200 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, depth + 1, out);
        } else if let Ok(meta) = entry.metadata() {
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            out.push(WsFile {
                rel: path.strip_prefix(root).unwrap_or(&path).to_string_lossy().to_string(),
                size: meta.len(),
                modified,
                text: is_text(&path),
            });
        }
    }
}

pub fn files_json(dir: &Path) -> Value {
    let files: Vec<Value> = list_files(dir)
        .into_iter()
        .map(|f| json!({ "rel": f.rel, "size": f.size, "modified": f.modified, "text": f.text }))
        .collect();
    json!({ "dir": dir.to_string_lossy(), "files": files })
}

fn clip(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max_chars).collect::<String>())
    }
}

/// Context block for the working agents: full listing (capped) plus excerpts
/// of the most relevant text files — filename-vs-task-keyword match first,
/// then recency. Returns ("", 0) when the folder is empty/missing.
pub fn read_context(dir: &Path, task_title: &str) -> (String, usize) {
    let files = list_files(dir);
    if files.is_empty() {
        return (String::new(), 0);
    }
    let mut ctx = String::from("Danh sách tệp trong workspace:\n");
    for f in files.iter().take(20) {
        ctx.push_str(&format!("- {} ({} bytes)\n", f.rel, f.size));
    }
    if files.len() > 20 {
        ctx.push_str(&format!("… và {} tệp khác\n", files.len() - 20));
    }

    // Score text files: keyword hits in the filename beat recency.
    let keywords: Vec<String> = task_title
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.chars().count() > 3)
        .map(|w| w.to_string())
        .collect();
    let mut candidates: Vec<&WsFile> = files.iter().filter(|f| f.text && f.size < 512 * 1024).collect();
    candidates.sort_by_key(|f| {
        let name = f.rel.to_lowercase();
        let hits = keywords.iter().filter(|k| name.contains(*k)).count() as i64;
        (-hits, -f.modified)
    });

    let mut budget = 3200usize; // chars across all excerpts
    let mut used = 0;
    for f in candidates.iter().take(3) {
        if budget < 400 {
            break;
        }
        let Ok(body) = std::fs::read_to_string(dir.join(&f.rel)) else { continue };
        let take = budget.min(1600);
        let excerpt = clip(body.trim(), take);
        budget = budget.saturating_sub(excerpt.chars().count());
        ctx.push_str(&format!("\nTrích tệp {}:\n{}\n", f.rel, excerpt));
        used += 1;
    }
    (clip(&ctx, 6000), files.len().max(used))
}

/// Write one deliverable under the workspace; parents are created. Returns
/// the path relative to the workspace root.
pub fn write_doc(dir: &Path, rel: &str, content: &str) -> Result<String, String> {
    let path = dir.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(rel.to_string())
}

/// Ensure the folder exists (called when saving the setting and on task run).
pub fn ensure_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())
}

pub fn resolve(dir_setting: &str) -> PathBuf {
    crate::db::expand_home(dir_setting)
}
