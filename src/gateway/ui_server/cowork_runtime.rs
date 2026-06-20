//! Cowork runtime — minimal trigger evaluator + task creator.
//!
//! Wired into `handle_message_send`: every user message in a cowork group
//! lands here. We persist:
//!
//!   1. A "primary" task carrying the message (assigned to the manager).
//!   2. One follow-up task per member whose triggers match this message.
//!
//! Trigger schema mirrors the legacy CoworkManager (`b307fa8:src/cowork/mod.rs::
//! collect_triggered_tasks`): a JSON array of typed objects. We support
//! `message_received` (from?, messageType?) and `on_mention` (from).
//! Status/cron triggers are no-ops here — they fire from elsewhere.
//!
//! This is intentionally lightweight: no agent dispatch yet, no handoff
//! chain, no output validation. The point is: when you chat into a cowork
//! group, the team task list grows. The UI shows you what triggered.

use std::sync::Arc;

use crate::db::cowork_tasks::CoworkTeamTask;
use crate::db::cowork_teams::TeamMember;
use crate::db::Db;
use crate::util::local_time::local_iso_string_now;

/// Truncate `s` to at most `max_chars` chars without splitting UTF-8.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

/// Match a single trigger object against this user message.
///
/// Only message-time triggers are evaluated. Returns true if the rule's
/// `from`/`messageType` constraints are satisfied (or absent).
fn trigger_fires_on_user_message(
    rule: &serde_json::Value,
    sender: &str,
    msg_type: &str,
) -> bool {
    let ttype = rule.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match ttype {
        "message_received" => {
            let from_ok = rule
                .get("from")
                .and_then(|v| v.as_str())
                .map_or(true, |f| f == sender);
            let mt_ok = rule
                .get("messageType")
                .and_then(|v| v.as_str())
                .map_or(true, |m| m == msg_type);
            from_ok && mt_ok
        }
        "on_mention" => {
            // Only fire when the rule specifies who's mentioning. We
            // treat the sender as the "mentioner" — UI will tighten this
            // once a real @mention parser is wired.
            rule.get("from")
                .and_then(|v| v.as_str())
                .is_some_and(|f| f == sender)
        }
        _ => false,
    }
}

fn count_matching_rules(member: &TeamMember, sender: &str, msg_type: &str) -> usize {
    let Some(raw) = member.triggers.as_deref() else { return 0 };
    let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(raw) else { return 0 };
    list.iter()
        .filter(|r| trigger_fires_on_user_message(r, sender, msg_type))
        .count()
}

/// Entry point: persist tasks for a user message in a cowork team.
///
/// Fire-and-forget — failures are logged but never propagated up to the
/// chat send path. The user gets their message routed to the manager even
/// if the cowork bookkeeping breaks.
/// Find the latest non-terminal manager task for a team. Used by the
/// state-transition hooks below to upgrade the right row when an agent
/// state event fires for a cowork chat.
pub fn latest_manager_task_id(db: &Arc<Db>, team_id: &str) -> Option<String> {
    let team = db.get_cowork_team(team_id).ok().flatten()?;
    let tasks = db.list_cowork_team_tasks(team_id).ok()?;
    tasks
        .into_iter()
        .filter(|t| t.assignee.as_deref() == Some(team.manager_folder.as_str()))
        .filter(|t| matches!(t.status.as_str(), "todo" | "in_progress"))
        .max_by(|a, b| a.created_at.cmp(&b.created_at))
        .map(|t| t.id)
}

/// Transition the manager's pending task to `in_progress` when the agent
/// starts working. No-op if no pending task or already in_progress.
pub fn on_agent_processing(db: &Arc<Db>, team_id: &str) {
    let Some(task_id) = latest_manager_task_id(db, team_id) else { return };
    let now = local_iso_string_now();
    let _ = db.update_cowork_team_task(
        &task_id,
        None,
        None,
        Some("in_progress"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        &now,
    );
    tracing::info!(
        "[cowork_runtime] team={team_id} task={task_id} → in_progress (agent started)"
    );
}

/// Mark the manager's pending task as `done` and persist the agent's reply
/// as `result_output` when the agent finishes a turn. Mirrors the legacy
/// "task complete on message_complete" flow in `b307fa8:src/cowork/mod.rs`.
pub fn on_agent_reply(db: &Arc<Db>, team_id: &str, reply: &str) {
    let Some(task_id) = latest_manager_task_id(db, team_id) else { return };
    let now = local_iso_string_now();
    let _ = db.update_cowork_team_task(
        &task_id,
        None,
        None,
        Some("done"),
        None,
        None,
        None,
        None,
        Some(reply),
        None,
        Some(&now),
        &now,
    );
    tracing::info!(
        "[cowork_runtime] team={team_id} task={task_id} → done (result_len={})",
        reply.len()
    );
}

/// Build the team-context preamble that we prepend to a user message in a
/// cowork chat. The manager sees this every turn so it knows it's in DAG
/// mode, which members exist, and that its job is to PLAN → DELEGATE →
/// SYNTHESIZE rather than execute the underlying work itself.
///
/// Returns `None` for unknown teams or solo teams (no members) — caller
/// should fall back to the raw message in that case.
pub fn team_context_preamble(db: &Arc<Db>, team_id: &str) -> Option<String> {
    let team = db.get_cowork_team(team_id).ok().flatten()?;
    if team.members.is_empty() {
        return None;
    }
    let mut s = String::new();
    s.push_str("[Cowork DAG context — you are the LEAD of this team]\n");
    s.push_str(&format!("Team: {}\n", team.name));
    s.push_str(&format!("Your role: lead (folder: {})\n", team.manager_folder));
    if let Some(ref ws) = team.workspace_dir {
        s.push_str(&format!("Shared workspace: {ws}\n"));
    }
    s.push_str("\nMembers you can delegate to (use the `Task` tool — `subagent_type` = member folder):\n");
    for m in team.members.iter() {
        let role = m.role.as_deref().unwrap_or("specialist");
        let resp = m.responsibilities.as_deref().unwrap_or("—");
        s.push_str(&format!(" • `{}` ({role}) — {resp}\n", m.folder));
    }
    s.push_str(
        "\nMANDATORY workflow for THIS turn (NO other tools available):\n\
         You are restricted to TWO tools: `Task` (delegation) and `TodoWrite` (planning).\n\
         You CANNOT browse the web, run shell commands, or edit files yourself.\n\
         The only way to make progress is to delegate via `Task`.\n\
         \n\
         How to delegate (subagent_type must be `general-purpose` — Task does NOT route by member folder yet):\n\
         \n\
         For each member you want to engage, call `Task` like this:\n\
         \n\
         Task(\n\
            subagent_type: \"general-purpose\",\n\
            description: \"<3-7 word imperative>\",\n\
            prompt: \"You are acting as the team's <MEMBER FOLDER> (role: <ROLE>). \\n\\\n\
                     Responsibilities: <RESPONSIBILITIES>. \\n\\\n\
                     User request: <VERBATIM USER REQUEST>. \\n\\\n\
                     Stay in scope of your role. Return a concise report.\"\n\
         )\n\
         \n\
         Steps:\n\
         1. PLAN — use `TodoWrite` to outline 1-N subtasks (one per member you'll engage).\n\
         2. DELEGATE — call `Task` once per subtask. Independent calls IN PARALLEL (same turn).\n\
         3. SYNTHESIZE — after Task results return, merge them into ONE final answer for the user, \
            attributing which member contributed what.\n\
         \n\
         If the request is purely conversational (greeting, small talk), say so explicitly: \
         \"No specialist needed — answering directly.\" and answer. Otherwise delegation is REQUIRED.\n",
    );
    s.push_str("\n---\n\nUser request (delegate per workflow above):\n");
    Some(s)
}

pub fn on_user_message(db: &Arc<Db>, team_id: &str, content: &str) {
    let Ok(Some(team)) = db.get_cowork_team(team_id) else {
        tracing::debug!("[cowork_runtime] no team for id={team_id}");
        return;
    };

    let now = local_iso_string_now();
    let title = truncate_chars(content.trim(), 60);
    if title.is_empty() {
        return;
    }
    let sender = "user";
    let msg_type = "status"; // mirrors legacy: user messages default to "status"

    // 1. Primary task — always created, assigned to the manager.
    let primary = CoworkTeamTask {
        id: uuid::Uuid::new_v4().to_string(),
        team_id: team_id.to_string(),
        title: title.clone(),
        description: Some(content.to_string()),
        status: "todo".to_string(),
        assignee: Some(team.manager_folder.clone()),
        reviewer: None,
        priority: "high".to_string(),
        depends_on: Vec::new(),
        result_output: None,
        created_at: now.clone(),
        updated_at: now.clone(),
        due_at: None,
        completed_at: None,
    };
    if let Err(e) = db.insert_cowork_team_task(&primary) {
        tracing::warn!("[cowork_runtime] primary insert failed: {e}");
    } else {
        tracing::info!(
            "[cowork_runtime] team={team_id} primary task created for manager={}",
            team.manager_folder
        );
    }

    // 2. Triggered follow-ups — one task per matching member rule. Each
    //    rule that fires creates a task assigned to that member, with the
    //    trigger ref in the title prefix for visibility.
    for member in team.members.iter() {
        let matches = count_matching_rules(member, sender, msg_type);
        if matches == 0 {
            continue;
        }
        let trig_title = format!("[triggered:{}] {}", member.folder, title);
        let task = CoworkTeamTask {
            id: uuid::Uuid::new_v4().to_string(),
            team_id: team_id.to_string(),
            title: trig_title,
            description: Some(content.to_string()),
            status: "todo".to_string(),
            assignee: Some(member.folder.clone()),
            reviewer: None,
            priority: "medium".to_string(),
            depends_on: Vec::new(),
            result_output: None,
            created_at: now.clone(),
            updated_at: now.clone(),
            due_at: None,
            completed_at: None,
        };
        if let Err(e) = db.insert_cowork_team_task(&task) {
            tracing::warn!(
                "[cowork_runtime] trigger task insert failed for {}: {e}",
                member.folder
            );
        } else {
            tracing::info!(
                "[cowork_runtime] team={team_id} triggered {} rule(s) → task for member={}",
                matches,
                member.folder
            );
        }
    }
}
