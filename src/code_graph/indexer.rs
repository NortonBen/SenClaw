//! CodeGraphIndexer — orchestrates: discover → parse → store → resolve.
//!
//! Bước 3 & 4 của GitNexus pipeline:
//!   3. Resolution  — cross-file edge to_sym_id linking
//!   4. Storage     — persist nodes + edges vào SQLite

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tracing::{debug, info, warn};

use crate::db::Db;

use super::parser::CodeParser;
use super::schema::apply_code_graph_schema;
use super::types::{IndexStats, Language};

// Extensions we index
const INDEXED_EXTENSIONS: &[&str] = &[
    // Rust / C family
    "rs", "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx", "cs",
    // JVM
    "java", "scala", "sc",
    // Go
    "go",
    // Web / scripting
    "ts", "tsx", "js", "jsx", "mjs", "cjs",
    "py", "pyi", "rb", "rake", "gemspec", "php",
    "sh", "bash", "zsh",
    // Functional
    "hs", "lhs", "ml", "mli", "agda",
    // Other
    "jl", "v", "sv", "svh",
];

// Directories we always skip
const SKIP_DIRS: &[&str] = &[
    "node_modules", ".git", "target", "dist", "build",
    "__pycache__", ".venv", "venv", ".senclaw-code",
];

// Max file size to parse (512 KB)
const MAX_FILE_BYTES: u64 = 512 * 1024;

pub struct CodeGraphIndexer {
    db: Arc<Db>,
    workspace_root: PathBuf,
}

impl CodeGraphIndexer {
    pub fn new(db: Arc<Db>, workspace_root: impl Into<PathBuf>) -> Result<Self> {
        let idx = Self { db, workspace_root: workspace_root.into() };
        // Ensure schema is applied
        idx.db.with_conn(|conn| apply_code_graph_schema(conn))?;
        Ok(idx)
    }

    /// Index full workspace. With `incremental=true` skips files whose mtime hasn't changed.
    pub fn index_workspace(&self, project_id: &str, incremental: bool) -> Result<IndexStats> {
        let files = self.discover_files()?;
        info!("[CodeGraph] discovered {} candidate files in {}", files.len(), self.workspace_root.display());

        let mut stats = IndexStats {
            files_indexed: 0,
            files_skipped: 0,
            symbols: 0,
            edges: 0,
        };

        for file_path in &files {
            let rel = file_path.strip_prefix(&self.workspace_root)
                .unwrap_or(file_path)
                .to_string_lossy()
                .to_string();

            let mtime = file_mtime(file_path);

            if incremental {
                let known_mtime = self.db.with_conn(|conn| {
                    Ok(conn.query_row(
                        "SELECT mtime_secs FROM cg_index_state WHERE project_id=?1 AND file_path=?2",
                        rusqlite::params![project_id, rel],
                        |row| row.get::<_, i64>(0),
                    ).ok())
                })?;

                if let Some(km) = known_mtime {
                    if km == mtime as i64 {
                        stats.files_skipped += 1;
                        continue;
                    }
                }
            }

            match self.index_file(project_id, file_path, &rel) {
                Ok((syms, edges)) => {
                    stats.files_indexed += 1;
                    stats.symbols += syms;
                    stats.edges += edges;
                    self.db.with_conn(|conn| {
                        conn.execute(
                            "INSERT OR REPLACE INTO cg_index_state \
                             (project_id, file_path, mtime_secs, symbol_count, edge_count) \
                             VALUES (?1,?2,?3,?4,?5)",
                            rusqlite::params![project_id, rel, mtime as i64, syms as i64, edges as i64],
                        )?;
                        Ok(())
                    })?;
                }
                Err(e) => {
                    warn!("[CodeGraph] skip {rel}: {e}");
                    stats.files_skipped += 1;
                }
            }
        }

        // Phase 3: resolve cross-file edges (fill to_sym_id where possible)
        self.resolve_edges(project_id)?;

        info!("[CodeGraph] indexed {} files ({} skipped), {} symbols, {} edges",
            stats.files_indexed, stats.files_skipped, stats.symbols, stats.edges);
        Ok(stats)
    }

    /// Index a single file, replacing its symbols/edges.
    fn index_file(&self, project_id: &str, abs_path: &Path, rel_path: &str) -> Result<(usize, usize)> {
        let ext = abs_path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let lang = Language::from_extension(ext);
        if lang == Language::Unknown {
            return Ok((0, 0));
        }

        let meta = std::fs::metadata(abs_path)?;
        if meta.len() > MAX_FILE_BYTES {
            debug!("[CodeGraph] skip large file {rel_path} ({} KB)", meta.len() / 1024);
            return Ok((0, 0));
        }

        let source = std::fs::read_to_string(abs_path)?;
        let parsed = CodeParser::parse_file(rel_path, &source, lang)?;

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        let sym_count = parsed.nodes.len();
        let edge_count = parsed.edges.len();

        self.db.with_conn(|conn| {
            // Remove stale data for this file
            conn.execute(
                "DELETE FROM cg_symbols WHERE project_id=?1 AND file_path=?2",
                rusqlite::params![project_id, rel_path],
            )?;
            conn.execute(
                "DELETE FROM cg_edges WHERE project_id=?1 AND from_file=?2",
                rusqlite::params![project_id, rel_path],
            )?;

            // Insert symbols
            let mut sym_ids: HashMap<String, i64> = HashMap::new();
            for node in &parsed.nodes {
                conn.execute(
                    "INSERT INTO cg_symbols \
                     (project_id, file_path, name, kind, signature, start_line, end_line, language, indexed_at) \
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    rusqlite::params![
                        project_id, rel_path, node.name, node.kind.as_str(),
                        node.signature, node.start_line, node.end_line,
                        lang.as_str(), now_ms,
                    ],
                )?;
                let id = conn.last_insert_rowid();
                sym_ids.insert(node.name.clone(), id);
            }

            // Insert edges (from_sym_id resolved within same file; to_sym_id deferred)
            for edge in &parsed.edges {
                let from_id = sym_ids.get(&edge.from_name).copied();
                conn.execute(
                    "INSERT INTO cg_edges \
                     (project_id, from_sym_id, from_name, from_file, to_sym_id, to_name, to_file, kind, at_line) \
                     VALUES (?1,?2,?3,?4,NULL,?5,?6,?7,?8)",
                    rusqlite::params![
                        project_id, from_id, edge.from_name, rel_path,
                        edge.to_name, edge.to_file, edge.kind.as_str(), edge.at_line,
                    ],
                )?;
            }
            Ok(())
        })?;

        Ok((sym_count, edge_count))
    }

    /// Phase 3: fill to_sym_id for edges where target symbol is now known.
    fn resolve_edges(&self, project_id: &str) -> Result<()> {
        self.db.with_conn(|conn| {
            // For each unresolved edge, try to find matching symbol by name
            conn.execute(
                "UPDATE cg_edges SET to_sym_id = (
                    SELECT id FROM cg_symbols
                    WHERE project_id = cg_edges.project_id
                      AND name = cg_edges.to_name
                    LIMIT 1
                )
                WHERE project_id = ?1 AND to_sym_id IS NULL",
                rusqlite::params![project_id],
            )?;
            Ok(())
        })
    }

    // ─── File discovery ───────────────────────────────────────────────────────

    fn discover_files(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        self.walk_dir(&self.workspace_root, &mut files);
        Ok(files)
    }

    fn walk_dir(&self, dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if path.is_dir() {
                if !SKIP_DIRS.contains(&name) {
                    self.walk_dir(&path, out);
                }
            } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if INDEXED_EXTENSIONS.contains(&ext) {
                    out.push(path);
                }
            }
        }
    }
}

fn file_mtime(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .and_then(|t| Ok(t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()))
        .unwrap_or(0)
}
