//! Cowork-aware prompt builder. Constructs rich agent prompts with workspace
//! context, board knowledge, member persona, responsibilities, acceptance
//! criteria, and dependency results.

use std::fs;
use std::path::PathBuf;

use crate::types::{CoworkBoardEntry, CoworkMember, CoworkTask, CoworkWorkspace};

/// Explain where shared uploads live and list `shared/` so all agents see the same files.
pub fn shared_workspace_files_context(workspace: &CoworkWorkspace) -> String {
    let shared = PathBuf::from(&workspace.root_dir).join("shared");
    let mut lines: Vec<String> = vec![
        format!(
            "Workspace root (agent cwd / symlink target): {}",
            workspace.root_dir
        ),
        format!("UI uploads and shared artifacts: {}", shared.display()),
    ];
    if let Some(ref wd) = workspace.working_dir {
        if !wd.is_empty() {
            lines.push(format!(
                "Project working directory (implementation tree): {wd}"
            ));
        }
    }
    if shared.is_dir() {
        if let Ok(rd) = fs::read_dir(&shared) {
            let mut names: Vec<String> = rd
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with('.') {
                        return None;
                    }
                    let p = e.path();
                    let suffix = if p.is_dir() { "/" } else { "" };
                    Some(format!("shared/{}{}", name, suffix))
                })
                .collect();
            names.sort();
            if names.is_empty() {
                lines.push(
                    "shared/ is empty — add files via Cowork UI (create workspace or Resources)."
                        .to_string(),
                );
            } else {
                lines.push(
                    "Contents of shared/ (read these paths relative to workspace root):"
                        .to_string(),
                );
                lines.extend(names.into_iter().map(|n| format!("  - {n}")));
            }
        }
    } else {
        lines.push(
            "shared/ does not exist yet — it is created when you upload reference documents."
                .to_string(),
        );
    }
    lines.join("\n")
}

/// Build a prompt for a cowork agent to execute a task.
///
/// The prompt wraps the task in XML-style context tags that give the agent a
/// clear picture of its role, the workspace state, and what's expected.
pub fn build_cowork_task_prompt(
    task: &CoworkTask,
    member: &CoworkMember,
    workspace: &CoworkWorkspace,
    board_entries: &[CoworkBoardEntry],
    dependent_results: &[CoworkTask],
) -> String {
    let mut p = String::new();

    // Workspace identity
    p.push_str("<workspace>\n");
    p.push_str(&format!("  <name>{}</name>\n", workspace.name));
    if let Some(ref wd) = workspace.working_dir {
        if !wd.is_empty() {
            p.push_str(&format!(
                "  <working_directory>{}</working_directory>\n",
                wd
            ));
        }
    }
    p.push_str(&format!(
        "  <root_directory>{}</root_directory>\n",
        workspace.root_dir
    ));
    p.push_str("</workspace>\n\n");

    p.push_str("<shared_files>\n");
    p.push_str(&shared_workspace_files_context(workspace));
    p.push_str("\n</shared_files>\n\n");

    // Task
    p.push_str(&format!("<task>\n  <title>{}</title>\n", task.title));
    if let Some(ref desc) = task.description {
        if !desc.is_empty() && desc != &task.title {
            p.push_str(&format!("  <description>{}</description>\n", desc));
        }
    }
    p.push_str("</task>\n\n");

    // Board context (relevant sections)
    if !board_entries.is_empty() {
        p.push_str("<board_context>\n");
        for entry in board_entries {
            p.push_str(&format!(
                "  <section name=\"{}\">\n    {}\n  </section>\n",
                entry.section,
                entry.content.trim(),
            ));
        }
        p.push_str("</board_context>\n\n");
    }

    // Role & persona
    if let Some(ref persona) = member.persona {
        if !persona.is_empty() {
            p.push_str(&format!("<persona>{persona}</persona>\n\n"));
        }
    }

    // Responsibilities
    if let Some(ref resp) = member.responsibilities {
        if let Ok(items) = serde_json::from_str::<Vec<String>>(resp) {
            if !items.is_empty() {
                p.push_str("<responsibilities>\n");
                for item in &items {
                    p.push_str(&format!("  - {item}\n"));
                }
                p.push_str("</responsibilities>\n\n");
            }
        }
    }

    // Acceptance criteria
    if let Some(ref ac) = member.acceptance_criteria {
        if let Ok(items) = serde_json::from_str::<Vec<String>>(ac) {
            if !items.is_empty() {
                p.push_str("<acceptance_criteria>\n");
                for item in &items {
                    p.push_str(&format!("  - {item}\n"));
                }
                p.push_str("</acceptance_criteria>\n\n");
            }
        }
    }

    // Output format
    if let Some(ref output) = member.output_format {
        if let Ok(fmt) = serde_json::from_str::<serde_json::Value>(output) {
            if let Some(desc) = fmt.get("description").and_then(|v| v.as_str()) {
                if !desc.is_empty() {
                    p.push_str(&format!("<output_format>{desc}</output_format>\n\n"));
                }
            }
        }
    }

    // SLA / Limits
    if let Some(ref sla_json) = member.sla {
        if let Ok(sla) = serde_json::from_str::<serde_json::Value>(sla_json) {
            let mut sla_parts: Vec<String> = Vec::new();
            if let Some(d) = sla.get("maxDuration").and_then(|v| v.as_str()) {
                sla_parts.push(format!("max duration: {d}"));
            }
            if let Some(t) = sla.get("maxTokens").and_then(|v| v.as_u64()) {
                sla_parts.push(format!("max tokens: {t}"));
            }
            if let Some(r) = sla.get("maxRetries").and_then(|v| v.as_u64()) {
                sla_parts.push(format!("max retries: {r}"));
            }
            if !sla_parts.is_empty() {
                p.push_str(&format!("<sla>{}</sla>\n\n", sla_parts.join(", ")));
            }
        }
    }

    // Tool limits
    if let Some(ref limits_json) = member.limits {
        if let Ok(limits) = serde_json::from_str::<serde_json::Value>(limits_json) {
            if let Some(allowed) = limits.get("allowedBashCommands").and_then(|v| v.as_array()) {
                if !allowed.is_empty() {
                    let cmds: Vec<&str> = allowed.iter().filter_map(|v| v.as_str()).collect();
                    p.push_str(&format!(
                        "<allowed_commands>{}</allowed_commands>\n",
                        cmds.join(", ")
                    ));
                }
            }
            if let Some(denied) = limits.get("deniedTools").and_then(|v| v.as_array()) {
                if !denied.is_empty() {
                    let tools: Vec<&str> = denied.iter().filter_map(|v| v.as_str()).collect();
                    p.push_str(&format!(
                        "<denied_tools>{}</denied_tools>\n",
                        tools.join(", ")
                    ));
                }
            }
        }
    }

    // Dependency results (completed prerequisite tasks)
    if !dependent_results.is_empty() {
        p.push_str("<dependency_results>\n");
        for dep in dependent_results {
            if dep.status == "done" {
                p.push_str(&format!(
                    "  <task id=\"{}\" title=\"{}\" assignee=\"{}\">\n",
                    dep.id,
                    dep.title,
                    dep.assignee.as_deref().unwrap_or("unknown"),
                ));
                if let Some(ref desc) = dep.description {
                    p.push_str(&format!("    <description>{desc}</description>\n"));
                }
                p.push_str("  </task>\n");
            }
        }
        p.push_str("</dependency_results>\n\n");
    }

    // Instructions
    p.push_str("Please complete this task. Follow your persona and responsibilities. ");
    p.push_str("After finishing, clearly report your results.");
    if let Some(ref ac) = member.acceptance_criteria {
        p.push_str(" Ensure all acceptance criteria are met before marking done.");
        let _ = ac;
    }

    p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_task() -> CoworkTask {
        CoworkTask {
            id: "task-1".into(),
            workspace_id: "ws-test".into(),
            title: "Implement login page".into(),
            description: Some("Create a login page with email and password fields".into()),
            status: "todo".into(),
            assignee: Some("code-agent".into()),
            reviewer: None,
            priority: "high".into(),
            depends_on: None,
            attachments: None,
            created_by: "user".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            due_at: None,
            completed_at: None,
        }
    }

    fn sample_member() -> CoworkMember {
        CoworkMember {
            workspace_id: "ws-test".into(),
            member_id: "code-agent".into(),
            role: "worker".into(),
            jid: None,
            subdir: None,
            persona: Some("You are a senior frontend engineer specializing in React and TypeScript.".into()),
            responsibilities: Some(r#"["Write clean, testable code","Follow the project style guide","Review your own code before marking done"]"#.into()),
            triggers: None,
            handoff_rules: None,
            acceptance_criteria: Some(r#"["All tests pass","Code is reviewed","UI matches design spec"]"#.into()),
            output_format: Some(r#"{"description":"Provide a summary of changes, list of files modified, and any follow-up items"}"#.into()),
            sla: Some(r#"{"maxDuration":"30m","maxTokens":8000,"maxRetries":3}"#.into()),
            limits: Some(r#"{"allowedBashCommands":["cargo","npm","git"],"deniedTools":["Write","Edit"]}"#.into()),
            joined_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn sample_workspace() -> CoworkWorkspace {
        CoworkWorkspace {
            id: "ws-test".into(),
            name: "Test Project".into(),
            description: Some("A test workspace".into()),
            status: "active".into(),
            root_dir: "/tmp/ws-test".into(),
            working_dir: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn sample_board() -> Vec<CoworkBoardEntry> {
        vec![
            CoworkBoardEntry {
                id: "b1".into(),
                workspace_id: "ws-test".into(),
                section: "brief".into(),
                title: Some("Project Brief".into()),
                content: "Build a user authentication system for the web app.".into(),
                author: "user".into(),
                pinned: true,
                tags: None,
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            },
            CoworkBoardEntry {
                id: "b2".into(),
                workspace_id: "ws-test".into(),
                section: "guidelines".into(),
                title: Some("Guidelines".into()),
                content: "Use React 18, TypeScript, and Tailwind CSS.".into(),
                author: "user".into(),
                pinned: false,
                tags: None,
                created_at: "2026-01-01T00:00:00Z".into(),
                updated_at: "2026-01-01T00:00:00Z".into(),
            },
        ]
    }

    #[test]
    fn prompt_contains_task_title() {
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &sample_board(),
            &[],
        );
        assert!(p.contains("Implement login page"));
    }

    #[test]
    fn prompt_contains_workspace_name() {
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &sample_board(),
            &[],
        );
        assert!(p.contains("Test Project"));
    }

    #[test]
    fn prompt_contains_board_context() {
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &sample_board(),
            &[],
        );
        assert!(p.contains("authentication system"));
        assert!(p.contains("Tailwind CSS"));
    }

    #[test]
    fn prompt_contains_persona() {
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
        );
        assert!(p.contains("senior frontend engineer"));
    }

    #[test]
    fn prompt_contains_responsibilities() {
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
        );
        assert!(p.contains("Write clean, testable code"));
    }

    #[test]
    fn prompt_contains_acceptance_criteria() {
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
        );
        assert!(p.contains("All tests pass"));
    }

    #[test]
    fn prompt_contains_sla() {
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
        );
        assert!(p.contains("max duration: 30m"));
        assert!(p.contains("max retries: 3"));
    }

    #[test]
    fn prompt_contains_allowed_and_denied() {
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
        );
        assert!(p.contains("cargo, npm, git"));
        assert!(p.contains("Write, Edit"));
    }

    #[test]
    fn prompt_contains_dependency_results() {
        let deps = vec![CoworkTask {
            id: "dep-1".into(),
            workspace_id: "ws-test".into(),
            title: "Setup project scaffold".into(),
            description: Some("Created the Vite + React + TS project".into()),
            status: "done".into(),
            assignee: Some("code-agent".into()),
            reviewer: None,
            priority: "high".into(),
            depends_on: None,
            attachments: None,
            created_by: "user".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            due_at: None,
            completed_at: Some("2026-01-01T01:00:00Z".into()),
        }];
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &deps,
        );
        assert!(p.contains("Setup project scaffold"));
        assert!(p.contains("Vite + React + TS"));
    }

    #[test]
    fn prompt_skips_empty_board() {
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
        );
        assert!(!p.contains("board_context"));
    }

    #[test]
    fn prompt_skips_empty_deps() {
        let p = build_cowork_task_prompt(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &sample_board(),
            &[],
        );
        assert!(!p.contains("dependency_results"));
    }

    #[test]
    fn minimal_member_produces_valid_prompt() {
        let minimal = CoworkMember {
            workspace_id: "ws-test".into(),
            member_id: "minimal-agent".into(),
            role: "worker".into(),
            jid: None,
            subdir: None,
            persona: None,
            responsibilities: None,
            triggers: None,
            handoff_rules: None,
            acceptance_criteria: None,
            output_format: None,
            sla: None,
            limits: None,
            joined_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        };
        let p = build_cowork_task_prompt(&sample_task(), &minimal, &sample_workspace(), &[], &[]);
        assert!(p.contains("Implement login page"));
        assert!(!p.contains("<persona>"));
        assert!(!p.contains("<responsibilities>"));
    }
}
