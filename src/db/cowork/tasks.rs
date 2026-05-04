use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::types::{CoworkTask, CoworkTaskComment};

use super::super::rows::{row_to_cowork_task, row_to_cowork_task_comment};

impl super::super::Db {
    // ============================================================
    // Cowork — Tasks
    // ============================================================

    pub fn insert_cowork_task(&self, t: &CoworkTask) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO cowork_tasks (id,workspace_id,title,description,status,assignee,reviewer,priority,depends_on,attachments,created_by,created_at,updated_at,due_at,completed_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                params![t.id, t.workspace_id, t.title, t.description, t.status, t.assignee, t.reviewer, t.priority, t.depends_on, t.attachments, t.created_by, t.created_at, t.updated_at, t.due_at, t.completed_at],
            )?;
            Ok(())
        })
    }

    pub fn get_cowork_task(&self, id: &str) -> Result<Option<CoworkTask>> {
        self.with_conn(|c| {
            c.query_row("SELECT * FROM cowork_tasks WHERE id=?1", params![id], |r| {
                Ok(row_to_cowork_task(r))
            })
            .optional()?
            .transpose()
        })
    }

    pub fn list_cowork_tasks(
        &self,
        workspace_id: &str,
        status_filter: Option<&str>,
    ) -> Result<Vec<CoworkTask>> {
        self.with_conn(|c| {
            if let Some(s) = status_filter {
                let mut stmt = c.prepare("SELECT * FROM cowork_tasks WHERE workspace_id=?1 AND status=?2 ORDER BY priority DESC, created_at ASC")?;
                let rows: Vec<_> = stmt.query_map(params![workspace_id, s], |r| Ok(row_to_cowork_task(r)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.into_iter().collect()
            } else {
                let mut stmt = c.prepare("SELECT * FROM cowork_tasks WHERE workspace_id=?1 ORDER BY priority DESC, created_at ASC")?;
                let rows: Vec<_> = stmt.query_map(params![workspace_id], |r| Ok(row_to_cowork_task(r)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.into_iter().collect()
            }
        })
    }

    pub fn update_cowork_task(
        &self,
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
        self.with_conn(|c| {
            let tx = c.unchecked_transaction()?;
            if let Some(t) = title {
                tx.execute(
                    "UPDATE cowork_tasks SET title=?1,updated_at=?2 WHERE id=?3",
                    params![t, now, id],
                )?;
            }
            if let Some(d) = description {
                tx.execute(
                    "UPDATE cowork_tasks SET description=?1,updated_at=?2 WHERE id=?3",
                    params![d, now, id],
                )?;
            }
            if let Some(s) = status {
                let completed = if s == "done" {
                    Some(now.to_string())
                } else {
                    None
                };
                tx.execute(
                    "UPDATE cowork_tasks SET status=?1,completed_at=?2,updated_at=?3 WHERE id=?4",
                    params![s, completed, now, id],
                )?;
            }
            if let Some(a) = assignee {
                tx.execute(
                    "UPDATE cowork_tasks SET assignee=?1,updated_at=?2 WHERE id=?3",
                    params![a, now, id],
                )?;
            }
            if let Some(r) = reviewer {
                tx.execute(
                    "UPDATE cowork_tasks SET reviewer=?1,updated_at=?2 WHERE id=?3",
                    params![r, now, id],
                )?;
            }
            if let Some(p) = priority {
                tx.execute(
                    "UPDATE cowork_tasks SET priority=?1,updated_at=?2 WHERE id=?3",
                    params![p, now, id],
                )?;
            }
            if let Some(d) = depends_on {
                tx.execute(
                    "UPDATE cowork_tasks SET depends_on=?1,updated_at=?2 WHERE id=?3",
                    params![d, now, id],
                )?;
            }
            if let Some(a) = attachments {
                tx.execute(
                    "UPDATE cowork_tasks SET attachments=?1,updated_at=?2 WHERE id=?3",
                    params![a, now, id],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    pub fn delete_cowork_task(&self, id: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM cowork_tasks WHERE id=?1", params![id])?;
            Ok(())
        })
    }

    // ============================================================
    // Cowork — Task comments
    // ============================================================

    pub fn insert_cowork_task_comment(
        &self,
        task_id: &str,
        author: &str,
        content: &str,
        created_at: &str,
    ) -> Result<i64> {
        self.with_conn(|c| {
            c.execute("INSERT INTO cowork_task_comments (task_id,author,content,created_at) VALUES (?1,?2,?3,?4)", params![task_id, author, content, created_at])?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn list_cowork_task_comments(&self, task_id: &str) -> Result<Vec<CoworkTaskComment>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM cowork_task_comments WHERE task_id=?1 ORDER BY created_at ASC",
            )?;
            let rows: Vec<_> = stmt
                .query_map(params![task_id], |r| Ok(row_to_cowork_task_comment(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        })
    }
}
