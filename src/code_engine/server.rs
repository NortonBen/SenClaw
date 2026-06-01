//! Code MCP server — 8 tools for agent-driven code editing.
//!
//! Tools: read_file, write_file, edit_file, bash, search_code,
//!        glob, get_skeleton, list_files.
//!
//! Env vars: SENCLAW_CODE_WORKSPACE, SENCLAW_CODE_PROJECT_ID.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rmcp::ServiceExt;
use similar::{ChangeTag, TextDiff};

use super::session::CodeSession;

// ─── Server ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub(super) struct McpCodeServer {
    pub workspace: PathBuf,
    pub project_id: String,
}

impl McpCodeServer {
    fn session(&self) -> CodeSession {
        CodeSession::open(&self.project_id, &self.workspace, true).unwrap_or_else(|_| CodeSession {
            session_id: self.project_id.clone(),
            workspace: self.workspace.clone(),
            git_enabled: false,
            tracker: Default::default(),
        })
    }

    fn resolve(&self, path: &str) -> Result<PathBuf, String> {
        self.session()
            .resolve_path(path)
            .map_err(|e| format!("❌ {e}"))
    }
}

// ─── Param types ─────────────────────────────────────────────────────────────

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct ReadFileParams {
    /// Path relative to workspace root
    path: String,
    /// First line (1-indexed, default 1)
    start_line: Option<u32>,
    /// Last line inclusive (default = end of file)
    end_line: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct WriteFileParams {
    /// Path relative to workspace root
    path: String,
    content: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct EditFileParams {
    /// Path relative to workspace root
    path: String,
    old_str: String,
    new_str: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct BashParams {
    cmd: String,
    /// Timeout in seconds (default 30, max 300)
    timeout_secs: Option<u64>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct SearchCodeParams {
    /// ast-grep pattern string
    pattern: String,
    /// Language hint: rust, typescript, python, go, …
    language: Option<String>,
    /// Sub-path to search (default ".")
    path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct GlobParams {
    pattern: String,
    /// Root directory for glob (default ".")
    dir: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct GetSkeletonParams {
    /// File or directory path (default ".")
    path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub(super) struct ListFilesParams {
    /// Directory to list (default ".")
    dir: Option<String>,
    /// Max depth (default 3)
    depth: Option<u32>,
}

// ─── Tools ───────────────────────────────────────────────────────────────────

#[rmcp::tool_router(server_handler)]
impl McpCodeServer {
    #[rmcp::tool(
        description = "Read a file from the workspace. Returns content with line numbers. Use start_line/end_line for a range."
    )]
    fn read_file(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ReadFileParams,
        >,
    ) -> String {
        let abs = match self.resolve(&p.path) {
            Ok(v) => v,
            Err(e) => return e,
        };
        match std::fs::read_to_string(&abs) {
            Err(e) => format!("❌ read {}: {e}", abs.display()),
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let total = lines.len();
                let start = p
                    .start_line
                    .map(|n| (n as usize).saturating_sub(1))
                    .unwrap_or(0);
                let end = p.end_line.map(|n| (n as usize).min(total)).unwrap_or(total);
                let slice = if start < total {
                    &lines[start..end]
                } else {
                    &lines[0..0]
                };
                let out = slice
                    .iter()
                    .enumerate()
                    .map(|(i, l)| format!("{:>4}\t{l}", start + i + 1))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{out}\n\n[{} of {total} lines shown]", slice.len())
            }
        }
    }

    #[rmcp::tool(
        description = "Write (create or overwrite) a file in the workspace. Returns a unified diff."
    )]
    fn write_file(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            WriteFileParams,
        >,
    ) -> String {
        let abs = match self.resolve(&p.path) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if let Some(parent) = abs.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("❌ create dirs: {e}");
            }
        }
        let old = std::fs::read_to_string(&abs).unwrap_or_default();
        if let Err(e) = std::fs::write(&abs, &p.content) {
            return format!("❌ write: {e}");
        }
        let sess = self.session();
        sess.tracker.record(&abs);
        let _ = sess.checkpoint(&format!("write_file {}", p.path));
        let diff = make_unified_diff(&old, &p.content, &p.path);
        format!("✅ Wrote {} ({} bytes)\n{diff}", p.path, p.content.len())
    }

    #[rmcp::tool(
        description = "Edit a file by exact string replacement. old_str must appear exactly once. Returns unified diff."
    )]
    fn edit_file(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            EditFileParams,
        >,
    ) -> String {
        let abs = match self.resolve(&p.path) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let content = match std::fs::read_to_string(&abs) {
            Ok(c) => c,
            Err(e) => return format!("❌ read {}: {e}", p.path),
        };
        let n = content.matches(p.old_str.as_str()).count();
        match n {
            0 => return format!(
                "❌ old_str not found in {}. Verify the exact text including whitespace.",
                p.path
            ),
            1 => {}
            _ => return format!(
                "❌ old_str appears {n} times in {}. Provide more surrounding context to make it unique.",
                p.path
            ),
        }
        let new_content = content.replacen(p.old_str.as_str(), p.new_str.as_str(), 1);
        if let Err(e) = std::fs::write(&abs, &new_content) {
            return format!("❌ write: {e}");
        }
        let sess = self.session();
        sess.tracker.record(&abs);
        let _ = sess.checkpoint(&format!("edit_file {}", p.path));
        let diff = make_unified_diff(&content, &new_content, &p.path);
        format!("✅ Edited {}\n{diff}", p.path)
    }

    #[rmcp::tool(
        description = "Run a shell command inside the workspace. Returns stdout + stderr + exit code."
    )]
    fn bash(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            BashParams,
        >,
    ) -> String {
        let timeout = Duration::from_secs(p.timeout_secs.unwrap_or(30).min(300));
        let start = Instant::now();
        let result = std::process::Command::new("sh")
            .arg("-c")
            .arg(&p.cmd)
            .current_dir(&self.workspace)
            .output();
        let elapsed = start.elapsed();
        match result {
            Err(e) => format!("❌ spawn: {e}"),
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let code = out.status.code().unwrap_or(-1);
                format!(
                    "exit_code: {code}{}\n--- stdout ---\n{stdout}--- stderr ---\n{stderr}",
                    if elapsed >= timeout { " [TIMEOUT]" } else { "" }
                )
            }
        }
    }

    #[rmcp::tool(
        description = "Search code with ast-grep AST pattern. Falls back to grep if ast-grep is not installed."
    )]
    fn search_code(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            SearchCodeParams,
        >,
    ) -> String {
        let search_path = p.path.as_deref().unwrap_or(".");
        let abs_path = match self.resolve(search_path) {
            Ok(v) => v,
            Err(e) => return e,
        };

        let mut args = vec!["run", "--pattern", p.pattern.as_str()];
        let lang_owned;
        if let Some(ref lang) = p.language {
            lang_owned = lang.clone();
            args.push("--lang");
            args.push(&lang_owned);
        }
        args.push("--json");
        args.push(abs_path.to_str().unwrap_or("."));

        match std::process::Command::new("ast-grep")
            .args(&args)
            .current_dir(&self.workspace)
            .output()
        {
            Err(_) => grep_fallback(&self.workspace, &p.pattern, search_path),
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.trim().is_empty() {
                    format!("No matches for pattern: {}", p.pattern)
                } else {
                    stdout.into_owned()
                }
            }
        }
    }

    #[rmcp::tool(description = "Find files matching a glob pattern inside the workspace.")]
    fn glob(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            GlobParams,
        >,
    ) -> String {
        let base = p.dir.as_deref().unwrap_or(".");
        let abs_base = match self.resolve(base) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let full_pattern = format!("{}/{}", abs_base.display(), p.pattern);
        match glob::glob(&full_pattern) {
            Err(e) => format!("❌ invalid glob: {e}"),
            Ok(paths) => {
                let mut out: Vec<String> = paths
                    .flatten()
                    .map(|pb| {
                        pb.strip_prefix(&self.workspace)
                            .map(|r| r.to_string_lossy().into_owned())
                            .unwrap_or_else(|_| pb.to_string_lossy().into_owned())
                    })
                    .collect();
                out.sort();
                if out.is_empty() {
                    format!("No files match: {}", p.pattern)
                } else {
                    out.join("\n")
                }
            }
        }
    }

    #[rmcp::tool(
        description = "Token-efficient code skeleton: function signatures, structs, imports — no bodies. Pass a file or directory path."
    )]
    fn get_skeleton(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            GetSkeletonParams,
        >,
    ) -> String {
        let target = p.path.as_deref().unwrap_or(".");
        let abs = match self.resolve(target) {
            Ok(v) => v,
            Err(e) => return e,
        };
        build_skeleton(&self.workspace, &abs)
    }

    #[rmcp::tool(description = "List files in a workspace directory as an indented tree.")]
    fn list_files(
        &self,
        rmcp::handler::server::wrapper::Parameters(p): rmcp::handler::server::wrapper::Parameters<
            ListFilesParams,
        >,
    ) -> String {
        let dir = p.dir.as_deref().unwrap_or(".");
        let abs = match self.resolve(dir) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let mut lines: Vec<String> = Vec::new();
        walk_tree(&abs, &self.workspace, 0, p.depth.unwrap_or(3), &mut lines);
        if lines.is_empty() {
            format!("{dir} is empty")
        } else {
            lines.join("\n")
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

pub(super) fn make_unified_diff(old: &str, new: &str, file_path: &str) -> String {
    let diff = TextDiff::from_lines(old, new);
    let mut out = format!("--- a/{file_path}\n+++ b/{file_path}\n");
    for group in diff.grouped_ops(3) {
        for op in &group {
            for change in diff.iter_changes(op) {
                let prefix = match change.tag() {
                    ChangeTag::Delete => "-",
                    ChangeTag::Insert => "+",
                    ChangeTag::Equal => " ",
                };
                out.push_str(prefix);
                out.push_str(change.value());
                if !change.value().ends_with('\n') {
                    out.push('\n');
                }
            }
        }
    }
    out
}

fn grep_fallback(workspace: &Path, pattern: &str, search_path: &str) -> String {
    let output = std::process::Command::new("grep")
        .args([
            "-rn",
            "--include=*.rs",
            "--include=*.ts",
            "--include=*.py",
            "--include=*.go",
            "--include=*.js",
            pattern,
            search_path,
        ])
        .current_dir(workspace)
        .output();
    match output {
        Err(e) => format!("❌ grep failed: {e}"),
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if stdout.trim().is_empty() {
                format!("No matches for: {pattern}")
            } else {
                stdout.into_owned()
            }
        }
    }
}

fn build_skeleton(workspace: &Path, target: &Path) -> String {
    // Try ast-grep first
    let probe = std::process::Command::new("ast-grep")
        .args([
            "run",
            "--pattern",
            "fn $NAME($$$) $$$",
            "--lang",
            "rust",
            ".",
        ])
        .current_dir(workspace)
        .output();

    if probe.is_ok_and(|o| o.status.success()) {
        let langs = [
            ("rust", "fn $NAME($$$) $$$"),
            ("typescript", "function $NAME($$$) $$$"),
            ("python", "def $NAME($$$): $$$"),
        ];
        let mut out = String::new();
        for (lang, pat) in &langs {
            if let Ok(o) = std::process::Command::new("ast-grep")
                .args([
                    "run",
                    "--pattern",
                    pat,
                    "--lang",
                    lang,
                    target.to_str().unwrap_or("."),
                ])
                .current_dir(workspace)
                .output()
            {
                let text = String::from_utf8_lossy(&o.stdout);
                if !text.trim().is_empty() {
                    out.push_str(&format!("=== {lang} ===\n{text}\n"));
                }
            }
        }
        if !out.is_empty() {
            return out;
        }
    }

    // Fallback: grep for declaration lines
    let output = std::process::Command::new("grep")
        .args([
            "-rn",
            "-E",
            r"^(pub )?(async )?(fn |def |function |class |struct |interface |type |const )",
            target.to_str().unwrap_or("."),
        ])
        .current_dir(workspace)
        .output();
    match output {
        Err(e) => format!("❌ skeleton failed: {e}"),
        Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
    }
}

fn walk_tree(dir: &Path, root: &Path, depth: u32, max_depth: u32, out: &mut Vec<String>) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let indent = "  ".repeat(depth as usize);
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.starts_with('.')
            || matches!(
                name.as_str(),
                "node_modules" | "target" | "dist" | "build" | "__pycache__"
            )
        {
            continue;
        }
        if path.is_dir() {
            out.push(format!("{indent}{name}/"));
            walk_tree(&path, root, depth + 1, max_depth, out);
        } else {
            out.push(format!("{indent}{name}"));
        }
    }
}

// ─── Entrypoint ──────────────────────────────────────────────────────────────

pub async fn run_code_server() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();

    let workspace =
        std::env::var("SENCLAW_CODE_WORKSPACE").context("SENCLAW_CODE_WORKSPACE not set")?;
    let project_id = std::env::var("SENCLAW_CODE_PROJECT_ID").unwrap_or_else(|_| "default".into());

    let server = McpCodeServer {
        workspace: PathBuf::from(workspace),
        project_id,
    };

    let service = server.serve(rmcp::transport::io::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
