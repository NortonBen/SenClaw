//! Cowork workspace lifecycle management.
//! Workspaces are multi-agent collaborative environments with task boards,
//! shared knowledge boards, inter-agent messaging, and recording.

pub mod prompt;

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use uuid::Uuid;

use crate::config::Config;
use crate::db::Db;
use crate::types::{
    AgentApi, CoworkBoardEntry, CoworkMember, CoworkMessage, CoworkTask, CoworkWorkspace,
    CoworkTemplate, GroupBinding, TemplateMember, TemplateBoard, TemplateBoardSection,
};

pub struct CoworkManager {
    on_changed: Mutex<Option<Box<dyn Fn() + Send + 'static>>>,
}

impl CoworkManager {
    pub fn new() -> Self {
        Self {
            on_changed: Mutex::new(None),
        }
    }

    pub fn set_on_changed(&self, cb: Box<dyn Fn() + Send + 'static>) {
        if let Ok(mut guard) = self.on_changed.lock() {
            *guard = Some(cb);
        }
    }

    fn fire_changed(&self) {
        if let Ok(guard) = self.on_changed.lock() {
            if let Some(ref cb) = *guard {
                cb();
            }
        }
    }

    // ============================================================
    // Orchestration — message → task decomposition
    // ============================================================

    /// Dispatch a cowork task to a workspace member agent for execution.
    /// Spawns a background task that calls process_and_wait and updates status.
    pub fn send_to_cowork_agent(
        &self,
        db: Arc<Db>,
        agent_api: Arc<dyn AgentApi>,
        workspace_id: &str,
        task: &CoworkTask,
        member: &CoworkMember,
        manager: Arc<CoworkManager>,
    ) {
        let task_id = task.id.clone();
        let member_id = member.member_id.clone();
        let workspace_id_owned = workspace_id.to_string();
        let jid = member.jid.clone().unwrap_or_else(|| {
            format!("cowork:{}:{}", workspace_id, member.member_id)
        });
        let folder = member.member_id.clone();
        let allowed_dirs = member.subdir.clone().map(|d| vec![d]);

        // Resolve context for prompt
        let workspace = match db.get_cowork_workspace(&workspace_id_owned) {
            Ok(Some(ws)) => ws,
            _ => return,
        };
        let board = db.get_cowork_board_entries(&workspace_id_owned, None).unwrap_or_default();
        let all_tasks = db.list_cowork_tasks(&workspace_id_owned, None).unwrap_or_default();

        // Find completed dependency tasks
        let dependent_results: Vec<CoworkTask> = if let Some(ref deps_json) = task.depends_on {
            if let Ok(dep_ids) = serde_json::from_str::<Vec<String>>(deps_json) {
                all_tasks
                    .into_iter()
                    .filter(|t| dep_ids.contains(&t.id) && t.status == "done")
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let prompt = self::prompt::build_cowork_task_prompt(
            task,
            member,
            &workspace,
            &board,
            &dependent_results,
        );

        let group = GroupBinding {
            jid: jid.clone(),
            folder: folder.clone(),
            name: format!("cowork-{member_id}"),
            channel: "web".to_string(),
            group_type: "cowork".to_string(),
            is_admin: false,
            requires_trigger: false,
            allowed_tools: None,
            allowed_paths: None,
            allowed_work_dirs: allowed_dirs,
            bot_token: None,
            max_messages: None,
            last_active: None,
            added_at: chrono::Utc::now().to_rfc3339(),
        };

        let db_clone = Arc::clone(&db);

        tokio::spawn(async move {
            // Mark in_progress
            let now = chrono::Utc::now().to_rfc3339();
            let _ = db_clone.update_cowork_task(
                &task_id,
                None,   // title
                None,   // description
                Some("in_progress"),
                None,   // assignee
                None,   // reviewer
                None,   // priority
                None,   // depends_on
                None,   // attachments
                &now,
            );

            let result = agent_api.process_and_wait(&jid, &group, &prompt).await;

            // Update status based on result
            let now2 = chrono::Utc::now().to_rfc3339();
            let new_status = if result.is_ok() { "done" } else { "blocked" };
            let _ = db_clone.update_cowork_task(
                &task_id,
                None, None,
                Some(new_status),
                None, None, None, None, None,
                &now2,
            );

            manager.fire_changed();
        });
    }

    /// Process a user message in the workspace: save it, create tasks for agents,
    /// and optionally dispatch tasks to agents for execution.
    /// Returns (message, created_tasks).
    pub fn process_user_message(
        &self,
        db: &Db,
        workspace_id: &str,
        from_user: &str,
        content: &str,
        now: &str,
        agent_api: Option<(Arc<dyn AgentApi>, Arc<Db>)>,
        self_arc: Arc<CoworkManager>,
    ) -> Result<(CoworkMessage, Vec<CoworkTask>)> {
        // 1. Save the message
        let msg_id = format!("cwmsg-{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("0000"));
        let msg = CoworkMessage {
            id: msg_id,
            workspace_id: workspace_id.to_string(),
            from_member: from_user.to_string(),
            to_member: None,
            message_type: if from_user == "user" { "status".to_string() } else { "handoff".to_string() },
            content: content.to_string(),
            attachments: None,
            task_id: None,
            is_read: false,
            created_at: now.to_string(),
        };
        db.insert_cowork_message(&msg)?;

        // 2. Get workspace members (agents)
        let members = db.list_cowork_members(workspace_id)?;
        let mut created_tasks = Vec::new();

        if members.is_empty() {
            self.fire_changed();
            return Ok((msg, created_tasks));
        }

        // 3. Find the lead/orchestrator agent (first worker or lead role)
        let lead = members.iter()
            .find(|m| m.role == "lead" || m.role == "worker")
            .or_else(|| members.first());

        if let Some(agent) = lead {
            // Create a planning/execution task for the lead agent
            let task_title = if content.len() > 80 {
                format!("{}...", &content[..80])
            } else {
                content.to_string()
            };
            let task = self.create_task(
                db, workspace_id, &task_title,
                Some(content),                          // full message as description
                Some(&agent.member_id),                  // assignee
                None,                                    // reviewer
                Some("high"),
                None,                                    // depends_on
                from_user,
                None,                                    // attachments
                now,
            )?;
            created_tasks.push(task);
        }

        // 4. Check handoff rules — if other agents have triggers matching this message,
        //    create standby tasks for them too
        for member in &members {
            if let Some(ref triggers_json) = member.triggers {
                if let Ok(triggers) = serde_json::from_str::<Vec<serde_json::Value>>(triggers_json) {
                    for trigger in &triggers {
                        let trigger_type = trigger["type"].as_str().unwrap_or("");
                        let should_fire = match trigger_type {
                            "message_received" => {
                                // Check if from matches
                                let from_filter = trigger["from"].as_str();
                                from_filter.map_or(true, |f| f == from_user)
                            }
                            "on_mention" => {
                                let from_filter = trigger["from"].as_str();
                                from_filter.map_or(false, |f| f == from_user)
                            }
                            _ => false,
                        };
                        if should_fire && !created_tasks.iter().any(|t| t.assignee.as_deref() == Some(&member.member_id)) {
                            let task = self.create_task(
                                db, workspace_id,
                                &format!("[from {}] {}", from_user, if content.len() > 60 { &content[..60] } else { content }),
                                Some(content),
                                Some(&member.member_id),
                                None,
                                Some("medium"),
                                None,
                                from_user,
                                None,
                                now,
                            )?;
                            created_tasks.push(task);
                        }
                    }
                }
            }
        }

        // 5. Dispatch tasks to agents if an agent API is available
        if let Some((ref api, ref db_arc)) = agent_api {
            for task in &created_tasks {
                let assignee_id = task.assignee.as_deref().unwrap_or("");
                if let Some(member) = members.iter().find(|m| m.member_id == assignee_id) {
                    self.send_to_cowork_agent(
                        Arc::clone(db_arc),
                        Arc::clone(api),
                        workspace_id,
                        task,
                        member,
                        Arc::clone(&self_arc),
                    );
                }
            }
        }

        self.fire_changed();
        Ok((msg, created_tasks))
    }

    // ============================================================
    // Workspaces
    // ============================================================

    pub fn create_workspace(
        &self,
        db: &Db,
        config: &Config,
        name: &str,
        description: Option<&str>,
        working_dir: Option<&str>,
        now: &str,
    ) -> Result<CoworkWorkspace> {
        let id = format!("ws-{}", &name.to_lowercase().replace(|c: char| !c.is_alphanumeric() && c != '-', "-"));
        let root_dir = config
            .paths
            .workspace_dir
            .join("cowork")
            .join(&id);

        // Create workspace directory structure
        fs::create_dir_all(root_dir.join("board")).ok();
        fs::create_dir_all(root_dir.join("tasks")).ok();
        fs::create_dir_all(root_dir.join("memory")).ok();
        fs::create_dir_all(root_dir.join("shared")).ok();
        fs::create_dir_all(root_dir.join("agents")).ok();
        fs::create_dir_all(root_dir.join("recordings")).ok();

        let ws = CoworkWorkspace {
            id: id.clone(),
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            status: "active".to_string(),
            root_dir: root_dir.to_string_lossy().into_owned(),
            working_dir: working_dir.map(|s| s.to_string()),
            created_at: now.to_string(),
            updated_at: now.to_string(),
        };

        db.insert_cowork_workspace(&ws)?;

        self.fire_changed();
        Ok(ws)
    }

    pub fn get_workspace(&self, db: &Db, id: &str) -> Result<Option<CoworkWorkspace>> {
        db.get_cowork_workspace(id)
    }

    pub fn list_workspaces(&self, db: &Db) -> Result<Vec<CoworkWorkspace>> {
        db.list_cowork_workspaces()
    }

    pub fn update_workspace(
        &self,
        db: &Db,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        working_dir: Option<&str>,
        now: &str,
    ) -> Result<()> {
        db.update_cowork_workspace(id, name, description, status, working_dir, now)?;
        self.fire_changed();
        Ok(())
    }

    pub fn delete_workspace(&self, db: &Db, id: &str) -> Result<()> {
        db.delete_cowork_workspace(id)?;
        self.fire_changed();
        Ok(())
    }

    // ============================================================
    // Members
    // ============================================================

    pub fn add_member(
        &self,
        db: &Db,
        config: &Config,
        workspace_id: &str,
        member_id: &str,
        role: &str,
        jid: Option<&str>,
        subdir: Option<&str>,
        now: &str,
    ) -> Result<CoworkMember> {
        let ws = db
            .get_cowork_workspace(workspace_id)?
            .ok_or_else(|| anyhow::anyhow!("Workspace {workspace_id} not found"))?;

        // Create agent subdir
        let agent_dir = PathBuf::from(&ws.root_dir).join("agents").join(member_id);
        fs::create_dir_all(&agent_dir).ok();
        let workspace_link = config.paths.workspace_dir.join(member_id);
        if !workspace_link.exists() {
            // Point agent's workspace to cowork shared dir
            std::os::unix::fs::symlink(&ws.root_dir, &workspace_link).ok();
        }

        let member = CoworkMember {
            workspace_id: workspace_id.to_string(),
            member_id: member_id.to_string(),
            role: role.to_string(),
            jid: jid.map(|s| s.to_string()),
            subdir: subdir.map(|s| s.to_string()),
            persona: None,
            responsibilities: None,
            triggers: None,
            handoff_rules: None,
            acceptance_criteria: None,
            output_format: None,
            sla: None,
            limits: None,
            joined_at: now.to_string(),
            updated_at: now.to_string(),
        };

        db.insert_cowork_member(&member)?;
        self.fire_changed();
        Ok(member)
    }

    pub fn get_member(
        &self,
        db: &Db,
        workspace_id: &str,
        member_id: &str,
    ) -> Result<Option<CoworkMember>> {
        db.get_cowork_member(workspace_id, member_id)
    }

    pub fn list_members(&self, db: &Db, workspace_id: &str) -> Result<Vec<CoworkMember>> {
        db.list_cowork_members(workspace_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_member_spec(
        &self,
        db: &Db,
        workspace_id: &str,
        member_id: &str,
        role: Option<&str>,
        persona: Option<&str>,
        responsibilities: Option<&str>,
        triggers: Option<&str>,
        handoff_rules: Option<&str>,
        acceptance_criteria: Option<&str>,
        output_format: Option<&str>,
        sla: Option<&str>,
        limits: Option<&str>,
        now: &str,
    ) -> Result<()> {
        db.update_cowork_member(
            workspace_id, member_id, role, persona, responsibilities, triggers,
            handoff_rules, acceptance_criteria, output_format, sla, limits, now,
        )?;
        self.fire_changed();
        Ok(())
    }

    pub fn remove_member(
        &self,
        db: &Db,
        workspace_id: &str,
        member_id: &str,
    ) -> Result<()> {
        db.delete_cowork_member(workspace_id, member_id)?;
        self.fire_changed();
        Ok(())
    }

    // ============================================================
    // Board entries
    // ============================================================

    pub fn upsert_board_entry(
        &self,
        db: &Db,
        workspace_id: &str,
        section: &str,
        title: Option<&str>,
        content: &str,
        author: &str,
        now: &str,
    ) -> Result<CoworkBoardEntry> {
        // Check if a section entry already exists for this workspace+section
        let existing = db.get_cowork_board_entries(workspace_id, Some(section))?;
        let entry = if let Some(e) = existing.first() {
            // Update existing
            let id = e.id.clone();
            db.update_cowork_board_entry(&id, title, Some(content), None, None, now)?;
            db.get_cowork_board_entries(workspace_id, Some(section))?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("Board entry disappeared after update"))?
        } else {
            let id = format!("be-{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("0000"));
            let e = CoworkBoardEntry {
                id,
                workspace_id: workspace_id.to_string(),
                section: section.to_string(),
                title: title.map(|s| s.to_string()),
                content: content.to_string(),
                author: author.to_string(),
                pinned: false,
                tags: None,
                created_at: now.to_string(),
                updated_at: now.to_string(),
            };
            db.insert_cowork_board_entry(&e)?;
            e
        };
        self.fire_changed();
        Ok(entry)
    }

    pub fn get_board(
        &self,
        db: &Db,
        workspace_id: &str,
        section: Option<&str>,
    ) -> Result<Vec<CoworkBoardEntry>> {
        db.get_cowork_board_entries(workspace_id, section)
    }

    pub fn update_board_entry(
        &self,
        db: &Db,
        id: &str,
        title: Option<&str>,
        content: Option<&str>,
        pinned: Option<bool>,
        tags: Option<&str>,
        now: &str,
    ) -> Result<()> {
        db.update_cowork_board_entry(id, title, content, pinned, tags, now)?;
        self.fire_changed();
        Ok(())
    }

    pub fn delete_board_entry(&self, db: &Db, id: &str) -> Result<()> {
        db.delete_cowork_board_entry(id)?;
        self.fire_changed();
        Ok(())
    }

    // ============================================================
    // Tasks
    // ============================================================

    pub fn create_task(
        &self,
        db: &Db,
        workspace_id: &str,
        title: &str,
        description: Option<&str>,
        assignee: Option<&str>,
        reviewer: Option<&str>,
        priority: Option<&str>,
        depends_on: Option<&str>,
        created_by: &str,
        attachments: Option<&str>,
        now: &str,
    ) -> Result<CoworkTask> {
        let id = format!("cwt-{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("0000"));
        let task = CoworkTask {
            id,
            workspace_id: workspace_id.to_string(),
            title: title.to_string(),
            description: description.map(|s| s.to_string()),
            status: "todo".to_string(),
            assignee: assignee.map(|s| s.to_string()),
            reviewer: reviewer.map(|s| s.to_string()),
            priority: priority.unwrap_or("medium").to_string(),
            depends_on: depends_on.map(|s| s.to_string()),
            attachments: attachments.map(|s| s.to_string()),
            created_by: created_by.to_string(),
            created_at: now.to_string(),
            updated_at: now.to_string(),
            due_at: None,
            completed_at: None,
        };
        db.insert_cowork_task(&task)?;
        self.fire_changed();
        Ok(task)
    }

    pub fn get_task(&self, db: &Db, id: &str) -> Result<Option<CoworkTask>> {
        db.get_cowork_task(id)
    }

    pub fn list_tasks(
        &self,
        db: &Db,
        workspace_id: &str,
        status: Option<&str>,
    ) -> Result<Vec<CoworkTask>> {
        db.list_cowork_tasks(workspace_id, status)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_task(
        &self,
        db: &Db,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        assignee: Option<&str>,
        reviewer: Option<&str>,
        priority: Option<&str>,
        depends_on: Option<&str>,
        attachments: Option<&str>,
        now: &str,
    ) -> Result<()> {
        db.update_cowork_task(
            id, title, description, status, assignee, reviewer, priority,
            depends_on, attachments, now,
        )?;
        self.fire_changed();
        Ok(())
    }

    pub fn delete_task(&self, db: &Db, id: &str) -> Result<()> {
        db.delete_cowork_task(id)?;
        self.fire_changed();
        Ok(())
    }

    // ============================================================
    // Task comments
    // ============================================================

    pub fn add_task_comment(
        &self,
        db: &Db,
        task_id: &str,
        author: &str,
        content: &str,
        now: &str,
    ) -> Result<i64> {
        let id = db.insert_cowork_task_comment(task_id, author, content, now)?;
        self.fire_changed();
        Ok(id)
    }

    pub fn list_task_comments(
        &self,
        db: &Db,
        task_id: &str,
    ) -> Result<Vec<crate::types::CoworkTaskComment>> {
        db.list_cowork_task_comments(task_id)
    }

    // ============================================================
    // Messages
    // ============================================================

    pub fn send_message(
        &self,
        db: &Db,
        workspace_id: &str,
        from_member: &str,
        to_member: Option<&str>,
        message_type: &str,
        content: &str,
        task_id: Option<&str>,
        attachments: Option<&str>,
        now: &str,
    ) -> Result<CoworkMessage> {
        let id = format!("cwm-{}", Uuid::new_v4().to_string().split('-').next().unwrap_or("0000"));
        let msg = CoworkMessage {
            id,
            workspace_id: workspace_id.to_string(),
            from_member: from_member.to_string(),
            to_member: to_member.map(|s| s.to_string()),
            message_type: message_type.to_string(),
            content: content.to_string(),
            attachments: attachments.map(|s| s.to_string()),
            task_id: task_id.map(|s| s.to_string()),
            is_read: false,
            created_at: now.to_string(),
        };
        db.insert_cowork_message(&msg)?;
        self.fire_changed();
        Ok(msg)
    }

    pub fn list_messages(
        &self,
        db: &Db,
        workspace_id: &str,
        limit: u32,
        since: Option<&str>,
    ) -> Result<Vec<CoworkMessage>> {
        db.list_cowork_messages(workspace_id, limit, since)
    }

    pub fn mark_message_read(&self, db: &Db, id: &str) -> Result<()> {
        db.mark_cowork_message_read(id)
    }

    // ============================================================
    // Templates
    // ============================================================

    /// Ensure built-in templates exist in the templates directory
    pub fn ensure_builtin_templates(&self, config: &Config) {
        let dir = &config.paths.workspace_templates_dir;
        fs::create_dir_all(dir).ok();

        // Don't overwrite existing templates
        if dir.join("software-dev.json").exists() {
            return;
        }

        let builtins: Vec<CoworkTemplate> = vec![
            CoworkTemplate {
                name: "Software Development".into(),
                description: "Multi-agent software development workflow with code, review, and test agents".into(),
                icon: Some("CodeOutlined".into()),
                members: vec![
                    TemplateMember {
                        agent_folder: "code-agent".into(),
                        role: "worker".into(),
                        subdir: Some("impl".into()),
                        persona: Some("Senior backend engineer. Prioritize correctness and performance. Always write unit tests for public functions.".into()),
                        responsibilities: Some(vec![
                            "Implement tasks tagged \"backend\" or \"feature\"".into(),
                            "Write unit tests and integration tests for your code".into(),
                            "Fix bugs assigned after review cycle".into(),
                            "Update Board progress section after each feature".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"review-agent","type":"review_request"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "cargo test passes".into(),
                            "cargo clippy has no warnings".into(),
                            "No new unwrap() in production paths".into(),
                        ]),
                        output: Some(serde_json::from_str(r#"{"format":"markdown","requiredSections":["Summary","Files Changed","Test Results","Notes"],"attachDiff":true}"#).unwrap()),
                        sla: Some(serde_json::from_str(r#"{"maxDurationPerTaskMinutes":60,"maxTokenPerTask":50000}"#).unwrap()),
                        limits: Some(serde_json::from_str(r#"{"allowedBashCommands":["cargo build","cargo test","cargo clippy","git diff"]}"#).unwrap()),
                    },
                    TemplateMember {
                        agent_folder: "review-agent".into(),
                        role: "reviewer".into(),
                        subdir: Some("review".into()),
                        persona: Some("Senior engineer specialized in code review. Focus on correctness, security, and performance. Provide specific, actionable feedback.".into()),
                        responsibilities: Some(vec![
                            "Review code from code-agent when receiving handoff".into(),
                            "Record important review decisions to Board \"decisions\"".into(),
                            "Approve or reject with clear reasoning".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"code-agent","messageType":"review_request"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"code-agent","type":"result"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Full diff has been read".into(),
                            "Test coverage verified".into(),
                            "Key decisions recorded to Board".into(),
                        ]),
                        output: None,
                        sla: None,
                        limits: Some(serde_json::from_str(r#"{"deniedTools":["Write","Edit","Bash"]}"#).unwrap()),
                    },
                    TemplateMember {
                        agent_folder: "test-agent".into(),
                        role: "worker".into(),
                        subdir: Some("tests".into()),
                        persona: Some("QA engineer specialized in thorough testing. Focus on edge cases, error paths, and concurrent scenarios.".into()),
                        responsibilities: Some(vec![
                            "Write integration tests after review-agent approves".into(),
                            "Run full test suite when requested".into(),
                            "Report coverage and failed tests".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_status_changed","status":"done","assignee":"code-agent"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"review-agent","type":"status"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Test coverage >= 80%".into(),
                            "No flaky tests".into(),
                        ]),
                        output: None,
                        sla: None,
                        limits: Some(serde_json::from_str(r#"{"allowedBashCommands":["cargo test","cargo tarpaulin"]}"#).unwrap()),
                    },
                ],
                board: Some(TemplateBoard {
                    sections: vec![
                        TemplateBoardSection { section_type: "brief".into(), title: "Project Brief".into(), template: Some("Describe the project and its goals...".into()) },
                        TemplateBoardSection { section_type: "guidelines".into(), title: "Development Guidelines".into(), template: Some("- Language/framework: ...\n- Coding conventions: ...\n- Testing requirements: ...".into()) },
                        TemplateBoardSection { section_type: "decisions".into(), title: "Architecture Decisions".into(), template: Some("(decisions will be auto-recorded by review-agent)".into()) },
                    ],
                }),
            },
            CoworkTemplate {
                name: "Research".into(),
                description: "Research workflow with researcher, synthesizer, and critic agents".into(),
                icon: Some("SearchOutlined".into()),
                members: vec![
                    TemplateMember {
                        agent_folder: "researcher".into(),
                        role: "worker".into(),
                        subdir: Some("research".into()),
                        persona: Some("Research analyst. Gather information thoroughly from multiple sources. Cite all sources clearly.".into()),
                        responsibilities: Some(vec![
                            "Research topics assigned via tasks".into(),
                            "Compile findings with full citations".into(),
                            "Update Board reference section with key sources".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"synthesizer","type":"handoff"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "All claims have citations".into(),
                            "Multiple sources cross-referenced".into(),
                        ]),
                        output: Some(serde_json::from_str(r#"{"format":"markdown","requiredSections":["Summary","Findings","Sources","Further Reading"]}"#).unwrap()),
                        sla: Some(serde_json::from_str(r#"{"maxDurationPerTaskMinutes":120,"maxTokenPerTask":100000}"#).unwrap()),
                        limits: None,
                    },
                    TemplateMember {
                        agent_folder: "synthesizer".into(),
                        role: "worker".into(),
                        subdir: Some("synthesis".into()),
                        persona: Some("Synthesis specialist. Combine research findings into coherent, well-structured documents. Identify patterns and connections.".into()),
                        responsibilities: Some(vec![
                            "Synthesize research findings into structured documents".into(),
                            "Identify gaps in research and request clarification".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"researcher","messageType":"handoff"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"critic","type":"review_request"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Document is well-structured".into(),
                            "All researcher findings are incorporated".into(),
                        ]),
                        output: None, sla: None, limits: None,
                    },
                    TemplateMember {
                        agent_folder: "critic".into(),
                        role: "reviewer".into(),
                        subdir: Some("critique".into()),
                        persona: Some("Critical thinker. Challenge assumptions, identify logical fallacies, and verify factual claims. Be constructive, not destructive.".into()),
                        responsibilities: Some(vec![
                            "Critique synthesized documents for logic and accuracy".into(),
                            "Fact-check key claims".into(),
                            "Suggest improvements".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"synthesizer","messageType":"review_request"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"synthesizer","type":"result"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "All key claims verified".into(),
                            "Actionable feedback provided".into(),
                        ]),
                        output: None, sla: None, limits: None,
                    },
                ],
                board: Some(TemplateBoard {
                    sections: vec![
                        TemplateBoardSection { section_type: "brief".into(), title: "Research Brief".into(), template: Some("Research topic and scope...".into()) },
                        TemplateBoardSection { section_type: "guidelines".into(), title: "Research Guidelines".into(), template: Some("- Use primary sources when possible\n- Cross-reference claims\n- Cite in APA format".into()) },
                        TemplateBoardSection { section_type: "reference".into(), title: "Key References".into(), template: Some("(references will be added by researcher)".into()) },
                    ],
                }),
            },
            CoworkTemplate {
                name: "Content Pipeline".into(),
                description: "Content creation workflow with writer, editor, and fact-checker agents".into(),
                icon: Some("EditOutlined".into()),
                members: vec![
                    TemplateMember {
                        agent_folder: "writer".into(),
                        role: "worker".into(),
                        subdir: Some("drafts".into()),
                        persona: Some("Professional content writer. Write clear, engaging, and well-structured content for the target audience.".into()),
                        responsibilities: Some(vec![
                            "Write content based on brief and guidelines".into(),
                            "Incorporate editor feedback".into(),
                            "Track word count and tone requirements".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"editor","type":"review_request"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Meets word count target".into(),
                            "Matches tone requirements".into(),
                            "Grammar and spelling correct".into(),
                        ]),
                        output: Some(serde_json::from_str(r#"{"format":"markdown","attachDiff":false}"#).unwrap()),
                        sla: Some(serde_json::from_str(r#"{"maxDurationPerTaskMinutes":90,"maxTokenPerTask":80000}"#).unwrap()),
                        limits: None,
                    },
                    TemplateMember {
                        agent_folder: "editor".into(),
                        role: "reviewer".into(),
                        subdir: Some("edits".into()),
                        persona: Some("Senior editor. Improve clarity, flow, and impact. Focus on structure and audience engagement, not just grammar.".into()),
                        responsibilities: Some(vec![
                            "Review content for structure and flow".into(),
                            "Check tone and audience alignment".into(),
                            "Return with specific revision notes".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"writer","messageType":"review_request"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"fact-checker","type":"review_request"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec!["Structural issues addressed".into(), "Tone is consistent".into()]),
                        output: None, sla: None, limits: None,
                    },
                    TemplateMember {
                        agent_folder: "fact-checker".into(),
                        role: "reviewer".into(),
                        subdir: Some("facts".into()),
                        persona: Some("Fact-checker. Verify every factual claim. Flag any unverified statements. Accuracy over speed.".into()),
                        responsibilities: Some(vec!["Verify all factual claims".into(), "Flag unverified or dubious claims".into()]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"editor","messageType":"review_request"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"writer","type":"result"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec!["Every claim verified or flagged".into()]),
                        output: None, sla: None, limits: None,
                    },
                ],
                board: Some(TemplateBoard {
                    sections: vec![
                        TemplateBoardSection { section_type: "brief".into(), title: "Content Brief".into(), template: Some("Topic, target audience, word count, tone...".into()) },
                        TemplateBoardSection { section_type: "guidelines".into(), title: "Style Guide".into(), template: Some("- Tone: ...\n- Word count: ...\n- Format: ...".into()) },
                    ],
                }),
            },
            CoworkTemplate {
                name: "Data Analysis".into(),
                description: "Analyst + Visualizer agents for data processing, statistics, and chart generation".into(),
                icon: Some("BarChartOutlined".into()),
                members: vec![
                    TemplateMember {
                        agent_folder: "analyst".into(),
                        role: "worker".into(),
                        subdir: Some("analysis".into()),
                        persona: Some("Data analyst. Process large datasets, compute statistics, identify trends. Write clean analysis code.".into()),
                        responsibilities: Some(vec![
                            "Load and clean data from CSV, JSON, or Parquet files".into(),
                            "Compute descriptive and inferential statistics".into(),
                            "Identify patterns, anomalies, and trends".into(),
                            "Document methodology and assumptions".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"visualizer","type":"handoff"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "All statistics are correctly computed".into(),
                            "Edge cases and nulls are handled".into(),
                            "Methodology is documented".into(),
                        ]),
                        output: Some(serde_json::from_str(r#"{"format":"markdown","requiredSections":["Summary","Methodology","Results","Caveats"]}"#).unwrap()),
                        sla: Some(serde_json::from_str(r#"{"maxDurationPerTaskMinutes":120,"maxTokenPerTask":150000}"#).unwrap()),
                        limits: Some(serde_json::from_str(r#"{"allowedBashCommands":["python","python3","node","jq","csvkit","pandas"]}"#).unwrap()),
                    },
                    TemplateMember {
                        agent_folder: "visualizer".into(),
                        role: "worker".into(),
                        subdir: Some("charts".into()),
                        persona: Some("Data visualization specialist. Create clear, informative charts and graphs. Use matplotlib, plotly, or eCharts.".into()),
                        responsibilities: Some(vec![
                            "Create charts from analyst's results".into(),
                            "Generate interactive visualizations when useful".into(),
                            "Export charts as PNG/SVG for reports".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"analyst","messageType":"handoff"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"analyst","type":"result"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Charts are correctly labeled".into(),
                            "Colors are accessible".into(),
                            "Data matches analyst output".into(),
                        ]),
                        output: None, sla: None, limits: None,
                    },
                ],
                board: Some(TemplateBoard {
                    sections: vec![
                        TemplateBoardSection { section_type: "brief".into(), title: "Analysis Brief".into(), template: Some("Data source, questions to answer, output format...".into()) },
                        TemplateBoardSection { section_type: "guidelines".into(), title: "Analysis Guidelines".into(), template: Some("- Handle missing values explicitly\n- Report confidence intervals\n- Use appropriate statistical tests".into()) },
                        TemplateBoardSection { section_type: "reference".into(), title: "Data Sources".into(), template: Some("(links and descriptions of data files)".into()) },
                    ],
                }),
            },
            CoworkTemplate {
                name: "API Backend".into(),
                description: "Backend engineer + Test agent for REST API development with OpenAPI specs".into(),
                icon: Some("ApiOutlined".into()),
                members: vec![
                    TemplateMember {
                        agent_folder: "backend-dev".into(),
                        role: "worker".into(),
                        subdir: Some("src".into()),
                        persona: Some("Backend engineer. Build robust REST APIs. Follow OpenAPI spec, handle errors properly, write integration tests.".into()),
                        responsibilities: Some(vec![
                            "Implement API endpoints per spec".into(),
                            "Write OpenAPI/Swagger documentation".into(),
                            "Handle error cases with proper status codes".into(),
                            "Write integration tests for all endpoints".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"task_assigned","condition":"assignee == me"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"api-tester","type":"review_request"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "All endpoints return correct status codes".into(),
                            "OpenAPI spec is valid".into(),
                            "Error responses follow RFC 7807".into(),
                        ]),
                        output: Some(serde_json::from_str(r#"{"format":"markdown","requiredSections":["Summary","Endpoints","Error Handling","Test Results"]}"#).unwrap()),
                        sla: Some(serde_json::from_str(r#"{"maxDurationPerTaskMinutes":90,"maxTokenPerTask":80000}"#).unwrap()),
                        limits: None,
                    },
                    TemplateMember {
                        agent_folder: "api-tester".into(),
                        role: "reviewer".into(),
                        subdir: Some("tests".into()),
                        persona: Some("API test engineer. Test every endpoint thoroughly — happy path, edge cases, error conditions, and load characteristics.".into()),
                        responsibilities: Some(vec![
                            "Test all API endpoints from OpenAPI spec".into(),
                            "Test edge cases: nulls, empty arrays, large payloads".into(),
                            "Report performance and error rates".into(),
                        ]),
                        triggers: Some(vec![
                            serde_json::from_str(r#"{"type":"message_received","from":"backend-dev","messageType":"review_request"}"#).unwrap(),
                        ]),
                        handoff: Some(vec![
                            serde_json::from_str(r#"{"when":"task_complete","to":"backend-dev","type":"result"}"#).unwrap(),
                        ]),
                        acceptance_criteria: Some(vec![
                            "Every endpoint tested".into(),
                            "Edge cases covered".into(),
                            "Performance within SLA".into(),
                        ]),
                        output: None, sla: None, limits: None,
                    },
                ],
                board: Some(TemplateBoard {
                    sections: vec![
                        TemplateBoardSection { section_type: "brief".into(), title: "API Brief".into(), template: Some("API purpose, base URL, auth method...".into()) },
                        TemplateBoardSection { section_type: "guidelines".into(), title: "API Guidelines".into(), template: Some("- OpenAPI 3.0+\n- RESTful conventions\n- Error format: RFC 7807\n- Versioning: URL path".into()) },
                        TemplateBoardSection { section_type: "reference".into(), title: "Endpoints Reference".into(), template: Some("(generated OpenAPI spec)".into()) },
                    ],
                }),
            },
            CoworkTemplate {
                name: "Blank".into(),
                description: "Empty workspace — start from scratch and configure everything manually".into(),
                icon: Some("FileOutlined".into()),
                members: vec![],
                board: None,
            },
        ];

        for tmpl in &builtins {
            let filename = tmpl.name.to_lowercase().replace(' ', "-") + ".json";
            let path = dir.join(&filename);
            if let Ok(json) = serde_json::to_string_pretty(tmpl) {
                fs::write(&path, json).ok();
            }
        }
    }

    /// List all available templates
    pub fn list_templates(&self, config: &Config) -> Result<Vec<CoworkTemplate>> {
        let dir = &config.paths.workspace_templates_dir;
        if !dir.exists() {
            return Ok(vec![]);
        }

        let mut templates = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "json") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(tmpl) = serde_json::from_str::<CoworkTemplate>(&content) {
                            templates.push(tmpl);
                        }
                    }
                }
            }
        }
        templates.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(templates)
    }

    /// Load a specific template by name
    pub fn get_template(&self, config: &Config, name: &str) -> Result<Option<CoworkTemplate>> {
        let filename = name.to_lowercase().replace(' ', "-") + ".json";
        let path = config.paths.workspace_templates_dir.join(&filename);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&content).ok())
    }

    /// Create a workspace and optionally apply a template
    pub fn create_workspace_with_template(
        &self,
        db: &Db,
        config: &Config,
        name: &str,
        description: Option<&str>,
        working_dir: Option<&str>,
        template_name: Option<&str>,
        now: &str,
    ) -> Result<CoworkWorkspace> {
        let ws = self.create_workspace(db, config, name, description, working_dir, now)?;

        if let Some(tmpl_name) = template_name {
            if let Some(tmpl) = self.get_template(config, tmpl_name)? {
                // Apply template members
                for m in &tmpl.members {
                    let member = CoworkMember {
                        workspace_id: ws.id.clone(),
                        member_id: m.agent_folder.clone(),
                        role: m.role.clone(),
                        jid: None,
                        subdir: m.subdir.clone(),
                        persona: m.persona.clone(),
                        responsibilities: m.responsibilities.as_ref().map(|r| serde_json::to_string(r).unwrap_or_default()),
                        triggers: m.triggers.as_ref().map(|t| serde_json::to_string(t).unwrap_or_default()),
                        handoff_rules: m.handoff.as_ref().map(|h| serde_json::to_string(h).unwrap_or_default()),
                        acceptance_criteria: m.acceptance_criteria.as_ref().map(|a| serde_json::to_string(a).unwrap_or_default()),
                        output_format: m.output.as_ref().map(|o| serde_json::to_string(o).unwrap_or_default()),
                        sla: m.sla.as_ref().map(|s| serde_json::to_string(s).unwrap_or_default()),
                        limits: m.limits.as_ref().map(|l| serde_json::to_string(l).unwrap_or_default()),
                        joined_at: now.to_string(),
                        updated_at: now.to_string(),
                    };

                    // Create agent subdir
                    let agent_dir = PathBuf::from(&ws.root_dir).join("agents").join(&m.agent_folder);
                    fs::create_dir_all(&agent_dir).ok();

                    db.insert_cowork_member(&member).ok();
                }

                // Apply template board sections
                if let Some(ref board) = tmpl.board {
                    for section in &board.sections {
                        let content = section.template.as_deref().unwrap_or("");
                        self.upsert_board_entry(
                            db, &ws.id, &section.section_type, Some(&section.title),
                            content, "system", now,
                        ).ok();
                    }
                }
            }
        }

        self.fire_changed();
        Ok(ws)
    }
}

impl Default for CoworkManager {
    fn default() -> Self {
        Self::new()
    }
}
