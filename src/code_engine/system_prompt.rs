//! Code-session system prompt builder — wraps the standard zen_core
//! `SYSTEM_PROMPT` with code-specific context (workspace, git state, refs,
//! command) so the agent has the right framing for every turn.
//!
//! Mirrors `claude-code`'s `fetchSystemPromptParts()` → custom prompt + user
//! context + memory + appended block, with one important difference: file
//! references are injected as content blocks (resolved via [`CodeSession`]),
//! while system metadata (cwd, git, edited files) live in the system prompt.

use std::path::Path;
use std::process::Command;

use crate::code_engine::session::CodeSession;
use crate::code_engine::prompt::PromptParseResult;

/// Coding-specific preamble injected after the standard `SYSTEM_PROMPT`. Tells
/// the agent it's in a sandboxed git-backed workspace and how to behave for
/// code tasks. Mirrors the spirit of claude-code's project-level guidance.
pub const CODE_SYSTEM_PROMPT: &str = "## Coding Workspace

You are working inside a **sandboxed code session**. All file operations are constrained to the session workspace below. Bash commands run with the workspace as cwd. Git is enabled — every user turn produces a checkpoint commit so the user can `/rollback` if something goes wrong.

**Tool preferences (in this session):**
- Use `Read` / `Write` / `Edit` for file changes — they respect the workspace boundary.
- Use `Glob` and `Grep` for discovery. They search only inside the workspace.
- Use `Bash` for running build/test/lint, **never** for navigation (`cd` is disabled — paths must be absolute or workspace-relative).
- Prefer `mcp__senclaw-code__*` for code-graph-aware operations (skeleton, structured search) when scale demands it.

**Conduct in code sessions:**
- Read before editing — never blind-edit a file.
- Keep edits minimal; the diff is what the user reviews.
- After non-trivial changes, run the relevant test/build command and report results.
- When uncertain about an API, run `Grep` for usage first, not guess.

## Plan-first for new projects / multi-file work

**Triggers** — when the user asks to:
- *Phát triển ứng dụng / build app / create project / scaffold / develop X*
- Create 3+ files in one task
- Set up a new framework (React, Vue, Next.js, FastAPI, Express, Cargo crate, …)
- Refactor across modules

→ You MUST enter Plan mode **before writing any code**:

1. Call `Skill { skill: \"code\" }` if a code skill exists, otherwise use the workflow here.
2. Write a plan file at `<workspace>/plans/<kebab-name>.md` listing:
   - Goal in one sentence
   - File tree (absolute paths) you will create / modify
   - Key design decisions + trade-offs
   - Verification commands (`npm test`, `cargo build`, `curl …`)
3. Call `ExitPlanMode { plan: \"<plan markdown>\", planFilePath: \"<abs path>\" }` to request user approval.
4. Wait for approval — do NOT touch source files before this.
5. After approval, execute the plan. End with the verification step.

## Verification — never claim completion without proof

After multi-file writes / non-trivial changes you MUST verify with shell:

```
Bash { command: \"ls -la <workspace>/...\" }     # confirm files exist
Bash { command: \"cat <file>\" }                  # confirm content (key files)
Bash { command: \"<build cmd>\" }                # confirm it builds (when applicable)
```

**Forbidden**: writing \"Done\" / \"Đã hoàn thành\" / \"Tôi đã …\" without first running a `Bash` verification. The user trusts your completion message; a lie wastes hours.

If a tool call returned an error or you never actually ran it, say so explicitly:
\"I wasn't able to complete X because Y — here's what's actually in the workspace: <ls output>\".
";

/// Build the code-session-specific system prompt block. Returns a string that
/// should be **appended** to the base `SYSTEM_PROMPT` already produced by
/// `ZenEngine::assemble_system_prompt`. The engine receives this via
/// [`crate::zen_core::ZenCoreOptions::system_prompt`].
pub fn build_code_system_prompt(
    session: &CodeSession,
    session_name: &str,
) -> String {
    let mut out = String::new();
    out.push_str(CODE_SYSTEM_PROMPT);
    out.push_str("\n\n## Session\n");
    out.push_str(&format!(
        "- Session: `{}` (id={})\n",
        session_name, session.session_id
    ));
    out.push_str(&format!(
        "- Workspace: `{}`\n",
        session.workspace.display()
    ));
    out.push_str(&format!(
        "- Git: {}\n",
        if session.git_enabled {
            "enabled (auto-checkpoint per user turn)"
        } else {
            "disabled (no rollback support)"
        }
    ));
    if let Some(branch) = current_branch(&session.workspace) {
        out.push_str(&format!("- Branch: `{branch}`\n"));
    }

    // Tracked file edits this session — gives the agent recent-edit recall.
    let edited = session.tracker.list();
    if !edited.is_empty() {
        out.push_str(&format!(
            "- Recent file edits ({}): {}\n",
            edited.len(),
            edited
                .iter()
                .take(8)
                .map(|p| format!("`{}`", p.display()))
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }

    out
}

/// Build the per-turn **user-input** preamble (a `<context>` block prepended
/// to the user's text). Mirrors claude-code's `processUserInput` which
/// resolves `@refs` and `/cmd` into structured context.
///
/// Returns the wrapped prompt. When the parsed result has nothing to inject,
/// returns the original plain text unchanged.
pub fn build_user_prompt(
    parsed: &PromptParseResult,
    resolved_refs: &[String],
) -> String {
    if parsed.command.is_none() && resolved_refs.is_empty() && parsed.skills.is_empty() {
        return parsed.plain_text.clone();
    }

    let mut out = String::new();
    out.push_str("<context>\n");
    if let Some(cmd) = &parsed.command {
        out.push_str(&format!("  <command>{cmd}</command>\n"));
    }
    if !resolved_refs.is_empty() {
        out.push_str("  <refs>\n");
        for r in resolved_refs {
            out.push_str(&format!("    <ref path=\"{r}\"/>\n"));
        }
        out.push_str("  </refs>\n");
    }
    if !parsed.skills.is_empty() {
        out.push_str("  <skills>\n");
        for s in &parsed.skills {
            out.push_str(&format!("    <skill name=\"{s}\"/>\n"));
        }
        out.push_str("  </skills>\n");
    }
    out.push_str("</context>\n\n");
    out.push_str(&parsed.plain_text);
    out
}

/// Best-effort `git rev-parse --abbrev-ref HEAD` to surface current branch.
fn current_branch(workspace: &Path) -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workspace)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mk_session(workspace: PathBuf, git_enabled: bool) -> CodeSession {
        CodeSession {
            session_id: "test-session".to_string(),
            workspace,
            git_enabled,
            tracker: Default::default(),
        }
    }

    #[test]
    fn build_code_system_prompt_contains_workspace_and_id() {
        let s = mk_session(PathBuf::from("/tmp/test-ws"), true);
        let out = build_code_system_prompt(&s, "My Project");
        assert!(out.contains("Coding Workspace"));
        assert!(out.contains("My Project"));
        assert!(out.contains("test-session"));
        assert!(out.contains("/tmp/test-ws"));
        assert!(out.contains("Git: enabled"));
    }

    #[test]
    fn build_code_system_prompt_marks_git_disabled() {
        let s = mk_session(PathBuf::from("/tmp/test-no-git"), false);
        let out = build_code_system_prompt(&s, "No Git");
        assert!(out.contains("Git: disabled"));
    }

    #[test]
    fn build_code_system_prompt_lists_edited_files() {
        let s = mk_session(PathBuf::from("/tmp/test-edit"), false);
        s.tracker.record(Path::new("/tmp/test-edit/src/main.rs"));
        s.tracker.record(Path::new("/tmp/test-edit/Cargo.toml"));
        let out = build_code_system_prompt(&s, "Edit");
        assert!(out.contains("Recent file edits (2)"));
        assert!(out.contains("main.rs"));
        assert!(out.contains("Cargo.toml"));
    }

    #[test]
    fn build_user_prompt_returns_plain_when_no_context() {
        let parsed = PromptParseResult {
            command: None,
            refs: vec![],
            skills: vec![],
            plain_text: "fix the bug".into(),
            normalized_prompt: "fix the bug".into(),
        };
        let out = build_user_prompt(&parsed, &[]);
        assert_eq!(out, "fix the bug");
    }

    #[test]
    fn build_user_prompt_wraps_when_command_or_refs() {
        let parsed = PromptParseResult {
            command: Some("plan".into()),
            refs: vec!["src/foo.rs".into()],
            skills: vec!["pdf".into()],
            plain_text: "refactor the parser".into(),
            normalized_prompt: String::new(),
        };
        let out = build_user_prompt(&parsed, &["/tmp/ws/src/foo.rs".into()]);
        assert!(out.contains("<command>plan</command>"));
        assert!(out.contains("<ref path=\"/tmp/ws/src/foo.rs\"/>"));
        assert!(out.contains("<skill name=\"pdf\"/>"));
        assert!(out.contains("refactor the parser"));
    }
}
