use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

/// Directory/file entries that never show up in the explorer or search.
const HARD_IGNORE: &[&str] = &[".git", "node_modules", "target", "dist", ".DS_Store"];

const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024; // 2 MB — refuse to open larger as text
const MAX_SEARCH_MATCHES: usize = 300;

#[derive(Serialize)]
pub struct TreeEntry {
    pub name: String,
    /// Workspace-relative path (forward slashes).
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

#[derive(Serialize)]
pub struct FileContent {
    pub path: String,
    pub content: String,
    pub lang: String,
    pub too_large: bool,
    pub binary: bool,
    pub size: u64,
}

#[derive(Serialize)]
pub struct SearchHit {
    pub path: String,
    pub line: u32,
    pub text: String,
}

/// Join `rel` onto `root`, rejecting any component that would escape the
/// workspace (`..`, absolute prefixes, Windows roots). Purely lexical — no
/// filesystem access — so it works for paths that don't exist yet (create/save).
pub fn safe_join(root: &Path, rel: &str) -> Result<PathBuf> {
    let rel = rel.trim().trim_start_matches('/');
    let mut out = root.to_path_buf();
    for comp in Path::new(rel).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            Component::ParentDir => bail!("path escapes workspace: {rel}"),
            Component::RootDir | Component::Prefix(_) => bail!("absolute path not allowed: {rel}"),
        }
    }
    Ok(out)
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_hidden_or_ignored(name: &str) -> bool {
    HARD_IGNORE.contains(&name)
}

/// Immediate children of a directory (lazy tree expansion). Dirs first, then
/// files, both alphabetical. `rel` is workspace-relative ("" = root).
pub fn list_dir(root: &Path, rel: &str) -> Result<Vec<TreeEntry>> {
    let dir = safe_join(root, rel)?;
    if !dir.is_dir() {
        bail!("not a directory: {rel}");
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_hidden_or_ignored(&name) {
            continue;
        }
        let meta = entry.metadata()?;
        let is_dir = meta.is_dir();
        out.push(TreeEntry {
            name,
            path: rel_of(root, &entry.path()),
            is_dir,
            size: if is_dir { 0 } else { meta.len() },
        });
    }
    out.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    Ok(out)
}

/// Read a file as UTF-8 text, guarding against binary blobs and huge files.
pub fn read_file(root: &Path, rel: &str) -> Result<FileContent> {
    let path = safe_join(root, rel)?;
    let meta = std::fs::metadata(&path).map_err(|e| anyhow!("stat {rel}: {e}"))?;
    if !meta.is_file() {
        bail!("not a file: {rel}");
    }
    let size = meta.len();
    let lang = lang_from_path(rel);
    if size > MAX_TEXT_BYTES {
        return Ok(FileContent {
            path: rel.to_string(),
            content: String::new(),
            lang,
            too_large: true,
            binary: false,
            size,
        });
    }
    let bytes = std::fs::read(&path)?;
    // Cheap binary sniff: a NUL byte in the first 8 KB.
    let binary = bytes.iter().take(8192).any(|&b| b == 0);
    if binary {
        return Ok(FileContent {
            path: rel.to_string(),
            content: String::new(),
            lang,
            too_large: false,
            binary: true,
            size,
        });
    }
    let content = String::from_utf8_lossy(&bytes).into_owned();
    Ok(FileContent { path: rel.to_string(), content, lang, too_large: false, binary: false, size })
}

/// Write (creating parent dirs) a text file.
pub fn write_file(root: &Path, rel: &str, content: &str) -> Result<()> {
    let path = safe_join(root, rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content)?;
    Ok(())
}

pub fn create_path(root: &Path, rel: &str, is_dir: bool) -> Result<()> {
    let path = safe_join(root, rel)?;
    if path.exists() {
        bail!("already exists: {rel}");
    }
    if is_dir {
        std::fs::create_dir_all(&path)?;
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, "")?;
    }
    Ok(())
}

pub fn rename_path(root: &Path, from: &str, to: &str) -> Result<()> {
    let src = safe_join(root, from)?;
    let dst = safe_join(root, to)?;
    if !src.exists() {
        bail!("no such path: {from}");
    }
    if dst.exists() {
        bail!("target exists: {to}");
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&src, &dst)?;
    Ok(())
}

pub fn delete_path(root: &Path, rel: &str) -> Result<()> {
    let path = safe_join(root, rel)?;
    if !path.exists() {
        bail!("no such path: {rel}");
    }
    if path.is_dir() {
        std::fs::remove_dir_all(&path)?;
    } else {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Plain-text substring search across the workspace, respecting `.gitignore`
/// and the hard-ignore set. Case-insensitive; capped at `MAX_SEARCH_MATCHES`.
pub fn search_text(root: &Path, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let needle = query.to_lowercase();
    let cap = limit.clamp(1, MAX_SEARCH_MATCHES);
    let mut hits = Vec::new();

    let mut builder = ignore::WalkBuilder::new(root);
    builder.hidden(false).git_ignore(true).git_global(false).parents(false);
    for name in HARD_IGNORE {
        builder.filter_entry(move |e| e.file_name().to_string_lossy() != *name);
    }
    for result in builder.build() {
        if hits.len() >= cap {
            break;
        }
        let entry = match result {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let meta = match path.metadata() {
            Ok(m) if m.len() <= MAX_TEXT_BYTES => m,
            _ => continue,
        };
        let _ = meta;
        let bytes = match std::fs::read(path) {
            Ok(b) if !b.iter().take(8192).any(|&c| c == 0) => b,
            _ => continue,
        };
        let text = String::from_utf8_lossy(&bytes);
        let rel = rel_of(root, path);
        for (i, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&needle) {
                hits.push(SearchHit {
                    path: rel.clone(),
                    line: (i + 1) as u32,
                    text: line.trim().chars().take(240).collect(),
                });
                if hits.len() >= cap {
                    break;
                }
            }
        }
    }
    Ok(hits)
}

/// Flat list of all workspace files (respecting `.gitignore` + hard-ignores),
/// for the chat `@`-mention file picker. Capped at `limit`.
pub fn list_all_files(root: &Path, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut builder = ignore::WalkBuilder::new(root);
    builder.hidden(false).git_ignore(true).git_global(false).parents(false);
    for name in HARD_IGNORE {
        builder.filter_entry(move |e| e.file_name().to_string_lossy() != *name);
    }
    for result in builder.build() {
        if out.len() >= limit {
            break;
        }
        if let Ok(entry) = result {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                out.push(rel_of(root, entry.path()));
            }
        }
    }
    out.sort();
    out
}

/// Map a path's extension to a Monaco language id.
pub fn lang_from_path(path: &str) -> String {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    let name = path.rsplit('/').next().unwrap_or(path);
    let by_name = match name {
        "Dockerfile" => Some("dockerfile"),
        "Makefile" => Some("makefile"),
        "Cargo.toml" | "Cargo.lock" => Some("toml"),
        _ => None,
    };
    if let Some(l) = by_name {
        return l.to_string();
    }
    let l = match ext.as_str() {
        "rs" => "rust",
        "ts" => "typescript",
        "tsx" => "typescript",
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascript",
        "py" | "pyi" => "python",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" => "cpp",
        "cs" => "csharp",
        "rb" => "ruby",
        "php" => "php",
        "swift" => "swift",
        "kt" | "kts" => "kotlin",
        "scala" => "scala",
        "sh" | "bash" | "zsh" => "shell",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" | "markdown" => "markdown",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" => "scss",
        "sql" => "sql",
        "xml" => "xml",
        "dart" => "dart",
        _ => "plaintext",
    };
    l.to_string()
}
