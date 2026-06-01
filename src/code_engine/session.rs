//! CodeSession — git-backed workspace sandbox for code agents.
//!
//! Provides path-traversal protection, git checkpoint/rollback, and
//! a per-session file tracker so agents can report what they changed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};

// ─── Session file tracker ────────────────────────────────────────────────────

/// Tracks which files have been written/edited in a session (insertion-ordered, deduplicated).
#[derive(Debug, Default)]
pub struct SessionFileTracker {
    edited: Mutex<Vec<PathBuf>>,
}

impl SessionFileTracker {
    pub fn record(&self, path: &Path) {
        let mut v = self.edited.lock().unwrap();
        let pb = path.to_path_buf();
        if !v.contains(&pb) {
            v.push(pb);
        }
    }

    pub fn list(&self) -> Vec<PathBuf> {
        self.edited.lock().unwrap().clone()
    }

    pub fn summary(&self) -> String {
        let files = self.edited.lock().unwrap();
        if files.is_empty() {
            return "No files modified this session.".into();
        }
        format!(
            "Files modified this session ({}):\n{}",
            files.len(),
            files
                .iter()
                .map(|p| format!("  • {}", p.display()))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
}

// ─── CodeSession ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CodeSession {
    pub session_id: String,
    pub workspace: PathBuf,
    pub git_enabled: bool,
    pub tracker: Arc<SessionFileTracker>,
}

impl CodeSession {
    /// Open (or create) a code session rooted at `workspace`.
    ///
    /// If `init_git` is true and the directory is not already a git repo,
    /// `git init` is called so checkpoint/rollback work.
    pub fn open(
        session_id: impl Into<String>,
        workspace: impl Into<PathBuf>,
        init_git: bool,
    ) -> Result<Self> {
        let workspace = workspace.into();
        std::fs::create_dir_all(&workspace).context("create workspace")?;

        let git_enabled = if init_git {
            let git_dir = workspace.join(".git");
            if !git_dir.exists() {
                std::process::Command::new("git")
                    .args(["init", "-q"])
                    .current_dir(&workspace)
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
            } else {
                true
            }
        } else {
            false
        };

        Ok(Self {
            session_id: session_id.into(),
            workspace,
            git_enabled,
            tracker: Arc::new(SessionFileTracker::default()),
        })
    }

    // ─── Path validation ─────────────────────────────────────────────────────

    /// Resolve a workspace-relative (or absolute) path and enforce sandbox boundary.
    pub fn resolve_path(&self, path: &str) -> Result<PathBuf> {
        let joined = self.workspace.join(path);
        let canonical = if joined.exists() {
            joined.canonicalize().context("canonicalize path")?
        } else {
            normalize_path(&joined)
        };
        if !canonical.starts_with(&self.workspace) {
            return Err(anyhow!("Path traversal denied: {path}"));
        }
        Ok(canonical)
    }

    // ─── Git checkpoint / rollback ───────────────────────────────────────────

    pub fn checkpoint(&self, msg: &str) -> Result<()> {
        if !self.git_enabled {
            return Ok(());
        }
        std::process::Command::new("git")
            .args(["add", "-A"])
            .current_dir(&self.workspace)
            .status()
            .context("git add")?;
        std::process::Command::new("git")
            .args([
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                &format!("checkpoint: {msg}"),
            ])
            .current_dir(&self.workspace)
            .status()
            .context("git commit")?;
        Ok(())
    }

    pub fn rollback(&self, steps: u32) -> Result<()> {
        if !self.git_enabled {
            return Err(anyhow!("Git not enabled for this session"));
        }
        let ok = std::process::Command::new("git")
            .args(["reset", "--hard", &format!("HEAD~{steps}")])
            .current_dir(&self.workspace)
            .status()
            .context("git reset")?
            .success();
        if !ok {
            return Err(anyhow!("git reset --hard HEAD~{steps} failed"));
        }
        Ok(())
    }

    pub fn git_log(&self, n: u32) -> String {
        if !self.git_enabled {
            return "Git not enabled.".into();
        }
        std::process::Command::new("git")
            .args(["log", "--oneline", &format!("-{n}")])
            .current_dir(&self.workspace)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_else(|_| "git log failed".into())
    }
}

// ─── Path normalization ───────────────────────────────────────────────────────

/// Normalize path components without requiring the path to exist (no canonicalize).
pub(super) fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in p.components() {
        use std::path::Component::*;
        match component {
            ParentDir => {
                out.pop();
            }
            CurDir => {}
            c => out.push(c),
        }
    }
    out
}
