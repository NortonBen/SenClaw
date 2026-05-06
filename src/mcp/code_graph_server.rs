//! Code Knowledge Graph MCP server — `senclaw-code-graph`.
//!
//! 7 tools cho AI agent query đồ thị tri thức code:
//!   graph_reindex        — build/update index
//!   graph_find_callers   — ai gọi hàm X?
//!   graph_find_callees   — hàm X gọi những gì?
//!   graph_impact         — blast radius: sửa X ảnh hưởng đâu?
//!   graph_symbol_context — full context (callers + callees + file skeleton)
//!   graph_trace_flow     — trace call tree từ entry point
//!   graph_search         — full-text search symbols
//!   graph_skeleton       — skeleton của file/project cho token-efficient context

use anyhow::{Context, Result};
use rmcp::ServiceExt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::code_graph::{CodeGraphIndexer, GraphQuery};
use crate::db::Db;

// ─── Param types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
struct ReindexParams {
    /// Chỉ reindex file có mtime thay đổi (default: true)
    #[serde(default = "default_true")]
    incremental: bool,
}
fn default_true() -> bool { true }

#[derive(Debug, Deserialize, JsonSchema)]
struct SymbolParams {
    /// Tên symbol cần tìm (function, class, struct, ...)
    name: String,
    /// Gợi ý file chứa symbol (để disambiguate)
    #[serde(default)]
    file_hint: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ImpactParams {
    /// Tên symbol cần phân tích blast radius
    name: String,
    /// Độ sâu BFS tối đa (default: 3)
    #[serde(default = "default_depth")]
    depth: u32,
}
fn default_depth() -> u32 { 3 }

#[derive(Debug, Deserialize, JsonSchema)]
struct TraceParams {
    /// Entry point (hàm, method, endpoint handler)
    entry: String,
    /// Độ sâu DFS tối đa (default: 5)
    #[serde(default = "default_trace_depth")]
    max_depth: u32,
}
fn default_trace_depth() -> u32 { 5 }

#[derive(Debug, Deserialize, JsonSchema)]
struct SearchParams {
    /// Từ khóa tìm kiếm (full-text trên tên + signature)
    query: String,
    /// Số kết quả tối đa (default: 20)
    #[serde(default = "default_limit")]
    limit: u32,
}
fn default_limit() -> u32 { 20 }

#[derive(Debug, Deserialize, JsonSchema)]
struct SkeletonParams {
    /// File path relative to workspace root. Nếu bỏ trống → skeleton toàn project.
    #[serde(default)]
    file_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct FileDepsParams {
    /// File path relative to workspace root
    file_path: String,
    /// "imports" | "imported_by" | "both" (default: "both")
    #[serde(default = "default_direction")]
    direction: String,
}
fn default_direction() -> String { "both".to_string() }

// ─── Server ──────────────────────────────────────────────────────────────────

struct McpCodeGraphServer {
    db:           Arc<Db>,
    project_id:   String,
    workspace:    String,
}

impl McpCodeGraphServer {
    fn indexer(&self) -> Result<CodeGraphIndexer> {
        CodeGraphIndexer::new(Arc::clone(&self.db), &self.workspace)
    }

    fn query(&self) -> GraphQuery {
        GraphQuery::new(Arc::clone(&self.db))
    }
}

use rmcp::handler::server::wrapper::Parameters;

#[rmcp::tool_router(server_handler)]
impl McpCodeGraphServer {
    #[rmcp::tool(description = "Index codebase thành knowledge graph. incremental=true chỉ reindex file thay đổi.")]
    fn graph_reindex(&self, Parameters(p): Parameters<ReindexParams>) -> String {
        match self.indexer().and_then(|idx| idx.index_workspace(&self.project_id, p.incremental)) {
            Ok(s) => format!(
                "✅ Knowledge graph updated\n\
                 • Files indexed: {}\n\
                 • Files skipped (unchanged): {}\n\
                 • Symbols: {}\n\
                 • Relationships: {}",
                s.files_indexed, s.files_skipped, s.symbols, s.edges
            ),
            Err(e) => format!("❌ Reindex failed: {e}"),
        }
    }

    #[rmcp::tool(description = "Tìm tất cả nơi gọi đến symbol_name (CALLS relationship).")]
    fn graph_find_callers(&self, Parameters(p): Parameters<SymbolParams>) -> String {
        let q = self.query();
        match q.find_callers(&self.project_id, &p.name) {
            Err(e) => format!("❌ {e}"),
            Ok(callers) if callers.is_empty() => format!("No callers found for `{}`.", p.name),
            Ok(callers) => {
                let mut out = format!("**{}** is called from {} location(s):\n\n", p.name, callers.len());
                for c in &callers {
                    out.push_str(&format!("  • `{}` ({}:{}) — {}\n", c.caller_name, c.caller_file, c.at_line, c.caller_kind));
                }
                out
            }
        }
    }

    #[rmcp::tool(description = "Tìm tất cả symbol được gọi bởi symbol_name (outgoing CALLS).")]
    fn graph_find_callees(&self, Parameters(p): Parameters<SymbolParams>) -> String {
        let q = self.query();
        match q.find_callees(&self.project_id, &p.name) {
            Err(e) => format!("❌ {e}"),
            Ok(callees) if callees.is_empty() => format!("`{}` does not call any tracked symbols.", p.name),
            Ok(callees) => {
                let mut out = format!("**{}** calls {} symbol(s):\n\n", p.name, callees.len());
                for c in &callees {
                    out.push_str(&format!("  • `{}` ({}:{})\n", c.caller_name, c.caller_file, c.at_line));
                }
                out
            }
        }
    }

    #[rmcp::tool(description = "Blast radius analysis: sửa symbol_name sẽ ảnh hưởng đến những symbol/file nào?")]
    fn graph_impact(&self, Parameters(p): Parameters<ImpactParams>) -> String {
        let q = self.query();
        match q.impact_analysis(&self.project_id, &p.name, p.depth) {
            Err(e) => format!("❌ {e}"),
            Ok(nodes) if nodes.is_empty() => format!("No dependents found for `{}` (depth={}). Safe to modify.", p.name, p.depth),
            Ok(nodes) => {
                let files: std::collections::HashSet<&str> = nodes.iter().map(|n| n.file.as_str()).collect();
                let mut out = format!(
                    "⚠️  Modifying **{}** may impact **{}** symbol(s) across **{}** file(s):\n\n",
                    p.name, nodes.len(), files.len()
                );
                let mut by_depth: std::collections::BTreeMap<u32, Vec<&crate::code_graph::ImpactNode>> = Default::default();
                for node in &nodes { by_depth.entry(node.depth).or_default().push(node); }
                for (depth, items) in &by_depth {
                    out.push_str(&format!("**Depth {depth}:**\n"));
                    for item in items {
                        out.push_str(&format!("  • `{}` ({}:{}) — {}\n", item.name, item.file, item.depth, item.via));
                    }
                }
                out
            }
        }
    }

    #[rmcp::tool(description = "Context đầy đủ cho symbol: signature, callers, callees, skeleton file.")]
    fn graph_symbol_context(&self, Parameters(p): Parameters<SymbolParams>) -> String {
        let q = self.query();
        let symbols = match q.find_symbol(&self.project_id, &p.name, p.file_hint.as_deref()) {
            Ok(s) => s,
            Err(e) => return format!("❌ {e}"),
        };
        if symbols.is_empty() {
            return format!("Symbol `{}` not found in index. Run graph_reindex first.", p.name);
        }
        let sym = &symbols[0];
        let callers = q.find_callers(&self.project_id, &p.name).unwrap_or_default();
        let callees = q.find_callees(&self.project_id, &p.name).unwrap_or_default();
        let skeleton = q.file_skeleton(&self.project_id, &sym.file_path).unwrap_or_default();

        let mut out = format!(
            "## `{}` ({})\n\n**File:** `{}` L{}-L{}\n**Kind:** {}\n**Signature:** `{}`\n\n",
            sym.name, sym.language.as_str(),
            sym.file_path, sym.start_line, sym.end_line,
            sym.kind.as_str(),
            sym.signature.as_deref().unwrap_or(&sym.name)
        );
        if !callers.is_empty() {
            out.push_str(&format!("**Called by ({}):**\n", callers.len()));
            for c in callers.iter().take(10) {
                out.push_str(&format!("  • `{}` @ {}:{}\n", c.caller_name, c.caller_file, c.at_line));
            }
            out.push('\n');
        }
        if !callees.is_empty() {
            out.push_str(&format!("**Calls ({}):**\n", callees.len()));
            for c in callees.iter().take(10) {
                out.push_str(&format!("  • `{}`\n", c.caller_name));
            }
            out.push('\n');
        }
        if !skeleton.is_empty() {
            out.push_str(&format!("**File skeleton ({}):**\n```\n", sym.file_path));
            for s in &skeleton { out.push_str(&format!("{s}\n")); }
            out.push_str("```\n");
        }
        out
    }

    #[rmcp::tool(description = "Trace execution flow từ entry point, DFS theo CALLS relationships.")]
    fn graph_trace_flow(&self, Parameters(p): Parameters<TraceParams>) -> String {
        let q = self.query();
        match q.trace_call_tree(&self.project_id, &p.entry, p.max_depth) {
            Err(e) => format!("❌ {e}"),
            Ok(nodes) if nodes.is_empty() => format!("`{}` makes no tracked function calls.", p.entry),
            Ok(nodes) => {
                let mut out = format!("**Call tree from `{}`** (depth ≤ {}):\n\n```\n{}\n", p.entry, p.max_depth, p.entry);
                for node in &nodes {
                    let indent = "  ".repeat(node.depth as usize);
                    out.push_str(&format!("{indent}└─ {} ({}) @ {}\n", node.name, node.kind, node.file));
                }
                out.push_str("```\n");
                out
            }
        }
    }

    #[rmcp::tool(description = "Full-text search symbols (functions, classes, structs) theo tên hoặc signature.")]
    fn graph_search(&self, Parameters(p): Parameters<SearchParams>) -> String {
        let q = self.query();
        match q.search_symbols(&self.project_id, &p.query, p.limit) {
            Err(e) => format!("❌ {e}"),
            Ok(results) if results.is_empty() => format!("No symbols matching `{}`.", p.query),
            Ok(results) => {
                let mut out = format!("Found **{}** symbol(s) matching `{}`:\n\n", results.len(), p.query);
                for sym in &results {
                    out.push_str(&format!("  • `{}` [{}] @ {}:L{}\n    {}\n",
                        sym.name, sym.kind.as_str(), sym.file_path, sym.start_line,
                        sym.signature.as_deref().unwrap_or("")));
                }
                out
            }
        }
    }

    #[rmcp::tool(description = "Skeleton của file hoặc project: chỉ signatures, không body. Token-efficient context.")]
    fn graph_skeleton(&self, Parameters(p): Parameters<SkeletonParams>) -> String {
        let q = self.query();
        if let Some(file) = &p.file_path {
            match q.file_skeleton(&self.project_id, file) {
                Err(e) => format!("❌ {e}"),
                Ok(s) if s.is_empty() => format!("No indexed symbols for `{file}`. Run graph_reindex first."),
                Ok(s) => {
                    let mut out = format!("**Skeleton: `{file}`**\n```\n");
                    for line in &s { out.push_str(&format!("{line}\n")); }
                    out.push_str("```");
                    out
                }
            }
        } else {
            match q.project_skeleton(&self.project_id) {
                Err(e) => format!("❌ {e}"),
                Ok(p) if p.is_empty() => "No index found. Run graph_reindex first.".to_string(),
                Ok(project) => {
                    let total: usize = project.iter().map(|(_, v)| v.len()).sum();
                    let mut out = format!("**Project skeleton** ({} files, {} symbols)\n\n", project.len(), total);
                    for (file, syms) in &project {
                        out.push_str(&format!("### `{file}`\n```\n"));
                        for s in syms { out.push_str(&format!("{s}\n")); }
                        out.push_str("```\n\n");
                    }
                    out
                }
            }
        }
    }

    #[rmcp::tool(description = "Tìm dependencies của file: file này import gì, ai import file này.")]
    fn graph_file_deps(&self, Parameters(p): Parameters<FileDepsParams>) -> String {
        let q = self.query();
        let mut out = format!("**Dependencies for `{}`:**\n\n", p.file_path);

        if p.direction == "imports" || p.direction == "both" {
            match q.file_dependencies(&self.project_id, &p.file_path) {
                Err(e) => return format!("❌ {e}"),
                Ok(deps) if deps.is_empty() => out.push_str("**Imports:** (none)\n"),
                Ok(deps) => {
                    out.push_str(&format!("**Imports ({}):**\n", deps.len()));
                    for d in &deps { out.push_str(&format!("  • `{d}`\n")); }
                }
            }
            out.push('\n');
        }

        if p.direction == "imported_by" || p.direction == "both" {
            match q.file_dependents(&self.project_id, &p.file_path) {
                Err(e) => return format!("❌ {e}"),
                Ok(rdeps) if rdeps.is_empty() => out.push_str("**Imported by:** (none)\n"),
                Ok(rdeps) => {
                    out.push_str(&format!("**Imported by ({}):**\n", rdeps.len()));
                    for d in &rdeps { out.push_str(&format!("  • `{d}`\n")); }
                }
            }
        }
        out
    }
}

// ─── Entry point (spawned as subprocess by McpManager) ───────────────────────

pub async fn run_code_graph_server() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init()
        .ok();

    let db_path    = std::env::var("SENCLAW_DB_PATH").context("SENCLAW_DB_PATH not set")?;
    let project_id = std::env::var("SENCLAW_PROJECT_ID").context("SENCLAW_PROJECT_ID not set")?;
    let workspace  = std::env::var("SENCLAW_WORKSPACE").context("SENCLAW_WORKSPACE not set")?;

    let mut config = crate::config::Config::from_env();
    config.paths.db_path = std::path::PathBuf::from(&db_path);
    let db = Arc::new(Db::open(&config).context("open code-graph DB")?);

    let server = McpCodeGraphServer { db, project_id, workspace };
    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
