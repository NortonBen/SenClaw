//! Cowork-aware prompt builder. Constructs rich agent prompts with workspace
//! context, board knowledge, member persona, responsibilities, acceptance
//! criteria, and dependency results.

use std::fs;
use std::path::PathBuf;

use crate::types::{CoworkBoardEntry, CoworkMember, CoworkMessage, CoworkTask, CoworkWorkspace};

/// Collapse newlines + tabs into single spaces and trim, so a chat-style
/// message renders cleanly on a single XML line in the prompt. Prevents the
/// `<message>X</message>` tag from being broken by stray `\n` and keeps the
/// prompt parseable by the receiving model.
pub(crate) fn sanitize_one_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = false;
    for ch in s.chars() {
        let mapped = if ch == '\n' || ch == '\r' || ch == '\t' {
            ' '
        } else {
            ch
        };
        if mapped == ' ' {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(mapped);
            last_space = false;
        }
    }
    out.trim().to_string()
}

/// Truncate to at most `max_chars` *characters* (not bytes), appending `…`
/// when shortened. Counts by Unicode scalar, so Vietnamese diacritics stay
/// intact and don't blow the per-message cap.
pub(crate) fn clip_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Cap on the number of cowork chat messages embedded as `<recent_activity>`
/// context. Picked to give the agent enough continuity ("what did the user
/// and other members say last") without ballooning the prompt past the
/// LLM's effective attention window — observed empty-completions when the
/// surrounding prompt got over ~12K chars.
pub const RECENT_MESSAGES_CAP: usize = 8;
/// Per-message character cap so a single rambling reply doesn't dominate.
pub const RECENT_MESSAGE_CHARS: usize = 400;
/// Cap on the number of recently-completed cross-member tasks shown so the
/// agent understands the project state, not just its own assigned slice.
pub const RECENT_TASKS_CAP: usize = 5;
/// Per-task description char cap for the recent-tasks section.
pub const RECENT_TASK_DESC_CHARS: usize = 200;

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

/// Backwards-compatible thin wrapper — preserves the original 5-arg signature
/// for callers (and tests) that don't have workspace chat history handy.
pub fn build_cowork_task_prompt(
    task: &CoworkTask,
    member: &CoworkMember,
    workspace: &CoworkWorkspace,
    board_entries: &[CoworkBoardEntry],
    dependent_results: &[CoworkTask],
) -> String {
    build_cowork_task_prompt_with_history(
        task,
        member,
        workspace,
        board_entries,
        dependent_results,
        &[],
        &[],
    )
}

/// Build a prompt for a cowork agent to execute a task, including recent
/// workspace chat + cross-member completed tasks so each turn feels like a
/// continuation rather than a cold start.
///
/// The prompt wraps the task in XML-style context tags that give the agent a
/// clear picture of its role, the workspace state, and what's expected.
pub fn build_cowork_task_prompt_with_history(
    task: &CoworkTask,
    member: &CoworkMember,
    workspace: &CoworkWorkspace,
    board_entries: &[CoworkBoardEntry],
    dependent_results: &[CoworkTask],
    recent_messages: &[CoworkMessage],
    recent_completed_tasks: &[CoworkTask],
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

    // Recent workspace activity — chat messages + cross-member completed
    // tasks. Gives the agent continuity across the cowork DAG so consecutive
    // members understand what already happened without having to be told.
    // Capped (RECENT_*_CAP / *_CHARS constants) to keep the prompt under
    // the LLM's effective attention window.
    let has_history = !recent_messages.is_empty() || !recent_completed_tasks.is_empty();
    if has_history {
        p.push_str("<recent_activity>\n");

        if !recent_completed_tasks.is_empty() {
            p.push_str("  <completed_tasks>\n");
            let tasks: Vec<&CoworkTask> = recent_completed_tasks
                .iter()
                .rev()
                .take(RECENT_TASKS_CAP)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            for t in tasks {
                p.push_str(&format!(
                    "    <task assignee=\"{}\" status=\"{}\">\n",
                    t.assignee.as_deref().unwrap_or("unknown"),
                    t.status,
                ));
                p.push_str(&format!(
                    "      <title>{}</title>\n",
                    sanitize_one_line(&t.title)
                ));
                if let Some(desc) = t.description.as_deref() {
                    let body = clip_chars(desc, RECENT_TASK_DESC_CHARS);
                    p.push_str(&format!(
                        "      <summary>{}</summary>\n",
                        sanitize_one_line(&body)
                    ));
                }
                p.push_str("    </task>\n");
            }
            p.push_str("  </completed_tasks>\n");
        }

        if !recent_messages.is_empty() {
            p.push_str("  <chat_messages>\n");
            let msgs: Vec<&CoworkMessage> = recent_messages
                .iter()
                .rev()
                .take(RECENT_MESSAGES_CAP)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            for m in msgs {
                let to = m
                    .to_member
                    .as_deref()
                    .map(|t| format!(" to=\"{t}\""))
                    .unwrap_or_default();
                let body = clip_chars(&m.content, RECENT_MESSAGE_CHARS);
                p.push_str(&format!(
                    "    <message from=\"{}\"{} type=\"{}\">{}</message>\n",
                    m.from_member,
                    to,
                    m.message_type,
                    sanitize_one_line(&body),
                ));
            }
            p.push_str("  </chat_messages>\n");
        }

        p.push_str("</recent_activity>\n\n");
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
            input_summary: None,
            result_output: None,
            references: None,
            artifacts: None,
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
            input_summary: None,
            result_output: None,
            references: None,
            artifacts: None,
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

    fn sample_message(from: &str, msg_type: &str, content: &str) -> CoworkMessage {
        CoworkMessage {
            id: format!("msg-{from}-{}", content.len()),
            workspace_id: "ws-test".into(),
            from_member: from.into(),
            to_member: None,
            message_type: msg_type.into(),
            content: content.into(),
            attachments: None,
            task_id: None,
            is_read: false,
            created_at: "2026-05-23T04:00:00Z".into(),
        }
    }

    #[test]
    fn history_block_omitted_when_empty() {
        let p = build_cowork_task_prompt_with_history(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
            &[],
            &[],
        );
        assert!(!p.contains("<recent_activity>"));
    }

    #[test]
    fn history_block_includes_chat_and_tasks() {
        let msgs = vec![
            sample_message("user", "status", "Bắt đầu nghiên cứu"),
            sample_message("research-lead", "handoff", "Giao cho researcher"),
            sample_message("researcher", "result", "Đã tổng hợp 3 nguồn"),
        ];
        let done_tasks = vec![CoworkTask {
            id: "done-1".into(),
            workspace_id: "ws-test".into(),
            title: "Khảo sát thị trường".into(),
            description: Some("Đã xong báo cáo sơ bộ về 4 tài sản".into()),
            status: "done".into(),
            assignee: Some("researcher".into()),
            reviewer: None,
            priority: "medium".into(),
            depends_on: None,
            attachments: None,
            created_by: "user".into(),
            created_at: "2026-05-23T03:00:00Z".into(),
            updated_at: "2026-05-23T03:30:00Z".into(),
            due_at: None,
            completed_at: Some("2026-05-23T03:30:00Z".into()),
            input_summary: None,
            result_output: None,
            references: None,
            artifacts: None,
        }];
        let p = build_cowork_task_prompt_with_history(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
            &msgs,
            &done_tasks,
        );
        assert!(p.contains("<recent_activity>"));
        assert!(p.contains("<chat_messages>"));
        assert!(p.contains("Bắt đầu nghiên cứu"));
        assert!(p.contains("Đã tổng hợp 3 nguồn"));
        assert!(p.contains("<completed_tasks>"));
        assert!(p.contains("Khảo sát thị trường"));
        assert!(p.contains("Đã xong báo cáo sơ bộ"));
    }

    #[test]
    fn history_clips_long_message_and_keeps_diacritics() {
        let long = "Đây là một tin nhắn rất dài ".repeat(60);
        let msgs = vec![sample_message("researcher", "result", &long)];
        let p = build_cowork_task_prompt_with_history(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
            &msgs,
            &[],
        );
        // Clipped — ellipsis present, and the content stays UTF-8 safe.
        assert!(p.contains('…'));
        // Diacritics survived the clip.
        assert!(p.contains("Đây"));
    }

    #[test]
    fn history_collapses_newlines_in_chat() {
        let msgs = vec![sample_message("user", "status", "Dòng 1\n\nDòng 2\tDòng 3")];
        let p = build_cowork_task_prompt_with_history(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
            &msgs,
            &[],
        );
        // sanitize_one_line drops \n and \t — message renders inline.
        assert!(p.contains("Dòng 1 Dòng 2 Dòng 3"));
        // And the <message> tag isn't broken by stray newlines.
        let line = p
            .lines()
            .find(|l| l.contains("Dòng 1"))
            .expect("message line");
        assert!(line.contains("<message"));
        assert!(line.contains("</message>"));
    }

    #[test]
    fn history_caps_message_count() {
        // Build more messages than the cap; only the latest RECENT_MESSAGES_CAP
        // should land in the prompt (oldest dropped). Tag each message with a
        // bracketed marker so substring tests aren't fooled by "msg-1" being a
        // prefix of "msg-10".
        let total = RECENT_MESSAGES_CAP + 5;
        let mut msgs = Vec::new();
        for i in 0..total {
            msgs.push(sample_message("user", "status", &format!("[m{i:03}]")));
        }
        let p = build_cowork_task_prompt_with_history(
            &sample_task(),
            &sample_member(),
            &sample_workspace(),
            &[],
            &[],
            &msgs,
            &[],
        );
        let n_in_prompt = (0..total)
            .filter(|i| p.contains(&format!("[m{i:03}]")))
            .count();
        assert_eq!(n_in_prompt, RECENT_MESSAGES_CAP);
        // The oldest messages got dropped — only the tail survives.
        assert!(!p.contains("[m000]"));
        assert!(p.contains(&format!("[m{:03}]", total - 1)));
    }
}
