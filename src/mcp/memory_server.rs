//! Memory MCP server. Port target: src-old/mcp/memory-server.ts
//!
//! Tools: memory_search, memory_get.
//! Provides read-only memory retrieval via FTS5 + vector hybrid search.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::db::Db;
use crate::mcp::schedule_server::ToolResult;

use rmcp::ServiceExt;
use crate::memory::embedding::EmbeddingProvider;
use crate::memory::fts_search::{self, SearchOptions};

// ===== MCP stdio server =====

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct MemorySearchParams {
    query: String,
    #[serde(default)]
    #[serde(rename = "maxResults")]
    max_results: Option<usize>,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize, schemars::JsonSchema)]
struct MemoryGetParams {
    #[serde(rename = "relPath")]
    rel_path: String,
    #[serde(default)]
    #[serde(rename = "startLine")]
    start_line: Option<u32>,
    #[serde(default)]
    #[serde(rename = "endLine")]
    end_line: Option<u32>,
}

#[derive(Clone)]
struct McpMemoryServer {
    db_path: String,
    folder: String,
    agents_dir: PathBuf,
}

impl McpMemoryServer {
    fn open_db(&self) -> Result<Db> {
        let mut cfg = crate::config::Config::from_env();
        cfg.paths.db_path = PathBuf::from(&self.db_path);
        Db::open(&cfg).context("open memory DB")
    }
}

#[rmcp::tool_router(server_handler)]
impl McpMemoryServer {
    #[rmcp::tool(description = "Search memories using hybrid FTS5 + vector search")]
    async fn memory_search(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<MemorySearchParams>,
    ) -> String {
        let db = match self.open_db() {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let srv = MemoryServer::new(db, &self.folder, &self.agents_dir, None);
        srv.memory_search(&p.query, p.max_results, p.source.as_deref())
            .await
            .content
    }

    #[rmcp::tool(description = "Retrieve a specific memory file by path and line range")]
    fn memory_get(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<MemoryGetParams>,
    ) -> String {
        let db = match self.open_db() {
            Ok(db) => db,
            Err(e) => return format!("Error: {e}"),
        };
        let srv = MemoryServer::new(db, &self.folder, &self.agents_dir, None);
        srv.memory_get(&p.rel_path, p.start_line, p.end_line).content
    }
}

/// Start the memory MCP server over stdio.
pub async fn run_stdio_server() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .try_init();

    let db_path = std::env::var("SENCLAW_DB_PATH").context("SENCLAW_DB_PATH not set")?;
    let folder = std::env::var("SENCLAW_FOLDER").context("SENCLAW_FOLDER not set")?;
    let agents_dir = std::env::var("SENCLAW_AGENTS_DIR")
        .context("SENCLAW_AGENTS_DIR not set")?;

    let server = McpMemoryServer {
        db_path,
        folder,
        agents_dir: PathBuf::from(agents_dir),
    };

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

pub struct MemoryServer {
    db: Db,
    folder: String,
    agents_dir: PathBuf,
    embedding_provider: Option<Box<dyn EmbeddingProvider>>,
}

impl MemoryServer {
    pub fn new(
        db: Db,
        folder: &str,
        agents_dir: &Path,
        embedding_provider: Option<Box<dyn EmbeddingProvider>>,
    ) -> Self {
        Self {
            db,
            folder: folder.to_owned(),
            agents_dir: agents_dir.to_path_buf(),
            embedding_provider,
        }
    }

    // ===== memory_search =====

    pub async fn memory_search(
        &self,
        query: &str,
        max_results: Option<usize>,
        source: Option<&str>,
    ) -> ToolResult {
        let limit = max_results.unwrap_or(6);
        let opts = SearchOptions {
            max_results: limit + 3,
            min_score: 0.25,
            source: source.map(|s| s.to_owned()),
        };

        let provider_ref: Option<&dyn EmbeddingProvider> =
            self.embedding_provider.as_deref();

        let raw_results = match fts_search::hybrid_search(
            &self.db,
            &self.folder,
            query,
            provider_ref,
            opts,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::err(format!("Search error: {e}")),
        };

        // Exclude today's log file (actively written, unstable content)
        let today_file = format!("{}.md", chrono::Utc::now().format("%Y-%m-%d"));
        let results: Vec<_> = raw_results
            .into_iter()
            .filter(|r| !r.path.ends_with(&today_file))
            .take(limit)
            .collect();

        if results.is_empty() {
            return ToolResult::ok("No matching memories found.".into());
        }

        let mut out = format!("Found {} results:\n\n", results.len());
        for (i, r) in results.iter().enumerate() {
            let path_parts: Vec<&str> = r.path.split(&['/', '\\'][..]).collect();
            let display_path = if path_parts.len() >= 2 {
                format!("{}/{}", path_parts[path_parts.len() - 2], path_parts[path_parts.len() - 1])
            } else {
                r.path.clone()
            };
            out.push_str(&format!(
                "[{}] {}:{}-{} (score: {:.2})\n",
                i + 1,
                display_path,
                r.start_line,
                r.end_line,
                r.score
            ));
            let summary = if r.text.len() > 300 {
                format!("{}...", &r.text[..300])
            } else {
                r.text.clone()
            };
            out.push_str(&format!("{summary}\n\n"));
        }

        ToolResult::ok(out.trim().to_string())
    }

    // ===== memory_get =====

    pub fn memory_get(
        &self,
        rel_path: &str,
        start_line: Option<u32>,
        end_line: Option<u32>,
    ) -> ToolResult {
        let abs_path = match resolve_memory_path(&self.agents_dir, &self.folder, rel_path) {
            Some(p) => p,
            None => return ToolResult::err(format!("File not found (path traversal blocked): {rel_path}")),
        };

        if !abs_path.exists() {
            return ToolResult::err(format!("File not found: {rel_path}"));
        }

        let content = match fs::read_to_string(&abs_path) {
            Ok(c) => c,
            Err(e) => return ToolResult::err(format!("Error reading file: {e}")),
        };

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len() as u32;

        let start = (start_line.unwrap_or(1)).saturating_sub(1).min(total_lines);
        let end = end_line
            .unwrap_or(total_lines)
            .min(total_lines)
            .max(start);

        let slice = &lines[start as usize..end as usize];
        let header = format!(
            "{} (lines {}-{} of {}):\n\n",
            rel_path,
            start + 1,
            end,
            total_lines
        );

        ToolResult::ok(format!("{header}{}", slice.join("\n")))
    }
}

/// Resolve a relative memory path to an absolute path, with path-traversal protection.
fn resolve_memory_path(agents_dir: &Path, folder: &str, relative_path: &str) -> Option<PathBuf> {
    let agent_dir = agents_dir.join(folder);

    let safe_check = |p: &PathBuf| -> bool {
        p.starts_with(&agent_dir) || p.as_path() == agent_dir.as_path()
    };

    // Try direct join
    let c1 = agent_dir.join(relative_path);
    if safe_check(&c1) && c1.exists() {
        return Some(c1);
    }

    // Try memory/ subdirectory
    let c2 = agent_dir.join("memory").join(relative_path);
    if safe_check(&c2) && c2.exists() {
        return Some(c2);
    }

    // Return safe path even if file doesn't exist (caller decides handling)
    if safe_check(&c1) {
        return Some(c1);
    }

    None
}
