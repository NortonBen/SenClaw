use anyhow::Result;
use rusqlite::Row;

use crate::types::{
    Agent, Binding, BindingWithRelations, Channel, ContextMode, CoworkBoardEntry, CoworkMember,
    CoworkMessage, CoworkRecordingSession, CoworkTask, CoworkTaskComment, CoworkWorkspace,
    GroupBinding, RunStatus, ScheduleType, ScheduledTask, StoredMessage, TaskStatus,
};

use super::helpers::parse_json_array;

pub(crate) fn row_to_channel(row: &Row<'_>) -> Result<Channel> {
    Ok(Channel {
        id: row.get("id")?,
        platform_type: row.get("platform_type")?,
        name: row.get("name")?,
        credentials_json: row.get("credentials_json")?,
        connection_state: row.get("connection_state")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub(crate) fn row_to_agent(row: &Row<'_>) -> Result<Agent> {
    Ok(Agent {
        id: row.get("id")?,
        folder: row.get("folder")?,
        name: row.get("name")?,
        requires_trigger: row.get::<_, i64>("requires_trigger")? != 0,
        allowed_tools: parse_json_array(row.get("allowed_tools")?),
        allowed_paths: parse_json_array(row.get("allowed_paths")?),
        allowed_work_dirs: parse_json_array(row.get("allowed_work_dirs")?),
        core_prompt: row.get::<_, String>("core_prompt").unwrap_or_default(),
        model_id: row.get("model_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub(crate) fn row_to_binding(row: &Row<'_>) -> Result<Binding> {
    Ok(Binding {
        id: row.get("id")?,
        jid: row.get("jid")?,
        agent_id: row.get("agent_id")?,
        channel_id: row.get("channel_id")?,
        is_admin: row.get::<_, i64>("is_admin")? != 0,
        bot_token_override: row.get("bot_token_override")?,
        max_messages: row.get::<_, Option<i64>>("max_messages")?.map(|n| n as u32),
        last_active: row.get("last_active")?,
        created_at: row.get("created_at")?,
    })
}

pub(crate) fn row_to_binding_with_relations(row: &Row<'_>) -> Result<BindingWithRelations> {
    Ok(BindingWithRelations {
        binding: Binding {
            id: row.get(0)?,
            jid: row.get(1)?,
            agent_id: row.get(2)?,
            channel_id: row.get(3)?,
            is_admin: row.get::<_, i64>(4)? != 0,
            bot_token_override: row.get(5)?,
            max_messages: row.get::<_, Option<i64>>(6)?.map(|n| n as u32),
            last_active: row.get(7)?,
            created_at: row.get(8)?,
        },
        agent: Agent {
            id: row.get(9)?,
            folder: row.get(10)?,
            name: row.get(11)?,
            requires_trigger: row.get::<_, i64>(12)? != 0,
            allowed_tools: parse_json_array(row.get(13)?),
            allowed_paths: parse_json_array(row.get(14)?),
            allowed_work_dirs: parse_json_array(row.get(15)?),
            core_prompt: row.get::<_, String>(16).unwrap_or_default(),
            model_id: row.get(17)?,
            created_at: row.get(18)?,
            updated_at: row.get(19)?,
        },
        channel: Channel {
            id: row.get(20)?,
            platform_type: row.get(21)?,
            name: row.get(22)?,
            credentials_json: row.get(23)?,
            connection_state: row.get(24)?,
            created_at: row.get(25)?,
            updated_at: row.get(26)?,
        },
    })
}

pub(crate) fn row_to_group(row: &Row<'_>) -> Result<GroupBinding> {
    Ok(GroupBinding {
        jid: row.get("jid")?,
        folder: row.get("folder")?,
        name: row.get("name")?,
        channel: row.get::<_, Option<String>>("channel")?.unwrap_or_default(),
        group_type: row
            .get::<_, Option<String>>("group_type")?
            .unwrap_or_else(|| "chat".to_string()),
        is_admin: row.get::<_, i64>("is_admin")? != 0,
        requires_trigger: row.get::<_, i64>("requires_trigger")? != 0,
        allowed_tools: parse_json_array(row.get("allowed_tools")?),
        allowed_paths: parse_json_array(row.get("allowed_paths")?),
        allowed_work_dirs: parse_json_array(row.get("allowed_work_dirs")?),
        bot_token: row.get("bot_token")?,
        max_messages: row.get::<_, Option<i64>>("max_messages")?.map(|n| n as u32),
        last_active: row.get("last_active")?,
        added_at: row.get("added_at")?,
    })
}

pub(crate) fn row_to_message(row: &Row<'_>) -> Result<StoredMessage> {
    Ok(StoredMessage {
        message_id: row.get("message_id")?,
        chat_jid: row.get("chat_jid")?,
        sender_jid: row.get("sender_jid")?,
        sender_name: row.get("sender_name")?,
        content: row.get("content")?,
        timestamp: row.get("timestamp")?,
        is_from_me: row.get::<_, i64>("is_from_me")? != 0,
        is_bot_reply: row.get::<_, i64>("is_bot_reply")? != 0,
        reply_to_id: row.get("reply_to_id")?,
        media_type: row.get("media_type")?,
    })
}

pub(crate) fn row_to_task(row: &Row<'_>) -> Result<ScheduledTask> {
    Ok(ScheduledTask {
        id: row.get("id")?,
        group_folder: row.get("group_folder")?,
        chat_jid: row.get("chat_jid")?,
        prompt: row.get("prompt")?,
        schedule_type: ScheduleType::parse(&row.get::<_, String>("schedule_type")?),
        schedule_value: row.get("schedule_value")?,
        context_mode: ContextMode::parse(&row.get::<_, String>("context_mode")?),
        script_command: row.get("script_path")?,
        next_run: row.get("next_run")?,
        last_run: row.get("last_run")?,
        last_result: row.get("last_result")?,
        status: TaskStatus::parse(&row.get::<_, String>("status")?),
        created_at: row.get("created_at")?,
    })
}

pub(crate) fn row_to_cowork_workspace(row: &Row<'_>) -> Result<CoworkWorkspace> {
    Ok(CoworkWorkspace {
        id: row.get("id")?,
        name: row.get("name")?,
        description: row.get("description")?,
        status: row
            .get::<_, Option<String>>("status")?
            .unwrap_or_else(|| "active".into()),
        root_dir: row.get("root_dir")?,
        working_dir: row.get("working_dir")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub(crate) fn row_to_cowork_member(row: &Row<'_>) -> Result<CoworkMember> {
    let _member_type: String = row.get("member_type")?;
    Ok(CoworkMember {
        workspace_id: row.get("workspace_id")?,
        member_id: row.get("member_id")?,
        role: row.get("role")?,
        jid: row.get("jid")?,
        subdir: row.get("subdir")?,
        persona: row.get("persona")?,
        responsibilities: row.get("responsibilities")?,
        triggers: row.get("triggers")?,
        handoff_rules: row.get("handoff_rules")?,
        acceptance_criteria: row.get("acceptance_criteria")?,
        output_format: row.get("output_format")?,
        sla: row.get("sla")?,
        limits: row.get("limits")?,
        joined_at: row.get("joined_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub(crate) fn row_to_cowork_board_entry(row: &Row<'_>) -> Result<CoworkBoardEntry> {
    Ok(CoworkBoardEntry {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        section: row.get("section")?,
        title: row.get("title")?,
        content: row.get("content")?,
        author: row.get("author")?,
        pinned: row.get::<_, i64>("pinned")? != 0,
        tags: row.get("tags")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

pub(crate) fn row_to_cowork_task(row: &Row<'_>) -> Result<CoworkTask> {
    Ok(CoworkTask {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        title: row.get("title")?,
        description: row.get("description")?,
        status: row
            .get::<_, Option<String>>("status")?
            .unwrap_or_else(|| "todo".into()),
        assignee: row.get("assignee")?,
        reviewer: row.get("reviewer")?,
        priority: row
            .get::<_, Option<String>>("priority")?
            .unwrap_or_else(|| "medium".into()),
        depends_on: row.get("depends_on")?,
        attachments: row.get("attachments")?,
        created_by: row.get("created_by")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        due_at: row.get("due_at")?,
        completed_at: row.get("completed_at")?,
    })
}

pub(crate) fn row_to_cowork_task_comment(row: &Row<'_>) -> Result<CoworkTaskComment> {
    Ok(CoworkTaskComment {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        author: row.get("author")?,
        content: row.get("content")?,
        created_at: row.get("created_at")?,
    })
}

pub(crate) fn row_to_cowork_message(row: &Row<'_>) -> Result<CoworkMessage> {
    Ok(CoworkMessage {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        from_member: row.get("from_member")?,
        to_member: row.get("to_member")?,
        message_type: row.get("message_type")?,
        content: row.get("content")?,
        attachments: row.get("attachments")?,
        task_id: row.get("task_id")?,
        is_read: row.get::<_, i64>("is_read")? != 0,
        created_at: row.get("created_at")?,
    })
}

pub(crate) fn row_to_cowork_recording_session(row: &Row<'_>) -> Result<CoworkRecordingSession> {
    Ok(CoworkRecordingSession {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        event_count: row.get::<_, i64>("event_count")?,
        total_tokens: row.get::<_, i64>("total_tokens")?,
        agents: row.get("agents")?,
    })
}
