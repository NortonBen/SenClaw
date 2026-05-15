use anyhow::Result;
use rusqlite::params;

use crate::types::{
    RunStatus, ScheduledTask, TaskRunLog, TaskRunLogInsert, TaskStatus,
};

use super::rows::row_to_task;

impl super::Db {
    // ============================================================
    // Scheduled tasks
    // ============================================================

    pub fn insert_task(&self, task: &ScheduledTask) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                r#"
                INSERT INTO scheduled_tasks
                  (id, group_folder, chat_jid, prompt, schedule_type, schedule_value,
                   context_mode, script_path, next_run, last_run, last_result, status, created_at)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                "#,
                params![
                    task.id,
                    task.group_folder,
                    task.chat_jid,
                    task.prompt,
                    task.schedule_type.as_str(),
                    task.schedule_value,
                    task.context_mode.as_str(),
                    task.script_command,
                    task.next_run,
                    task.last_run,
                    task.last_result,
                    task.status.as_str(),
                    task.created_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_due_tasks(&self, now: &str) -> Result<Vec<ScheduledTask>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM scheduled_tasks
                 WHERE status = 'active' AND next_run IS NOT NULL AND next_run <= ?1
                 ORDER BY next_run ASC",
            )?;
            let rows = stmt
                .query_map(params![now], |r| Ok(row_to_task(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        })
    }

    pub fn get_tasks_by_group(&self, group_folder: &str) -> Result<Vec<ScheduledTask>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM scheduled_tasks WHERE group_folder = ?1 ORDER BY created_at DESC",
            )?;
            let rows = stmt
                .query_map(params![group_folder], |r| Ok(row_to_task(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        })
    }

    pub fn list_all_tasks(&self) -> Result<Vec<ScheduledTask>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM scheduled_tasks ORDER BY created_at DESC")?;
            let rows = stmt
                .query_map([], |r| Ok(row_to_task(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        })
    }

    pub fn update_task_run(
        &self,
        id: &str,
        next_run: Option<&str>,
        last_run: &str,
        last_result: Option<&str>,
        status: TaskStatus,
    ) -> Result<()> {
        let truncated: Option<String> = last_result.map(|s| {
            if s.chars().count() > 500 {
                s.chars().take(500).collect()
            } else {
                s.to_owned()
            }
        });
        self.with_conn(|c| {
            c.execute(
                "UPDATE scheduled_tasks
                 SET next_run = ?1, last_run = ?2, last_result = ?3, status = ?4
                 WHERE id = ?5",
                params![next_run, last_run, truncated, status.as_str(), id],
            )?;
            Ok(())
        })
    }

    pub fn advance_task_next_run(
        &self,
        id: &str,
        next_run: Option<&str>,
        status: TaskStatus,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE scheduled_tasks SET next_run = ?1, status = ?2 WHERE id = ?3",
                params![next_run, status.as_str(), id],
            )?;
            Ok(())
        })
    }

    pub fn update_task_status(&self, id: &str, status: TaskStatus) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE scheduled_tasks SET status = ?1 WHERE id = ?2",
                params![status.as_str(), id],
            )?;
            Ok(())
        })
    }

    pub fn delete_task(&self, id: &str) -> Result<bool> {
        self.with_conn(|c| {
            let n = c.execute("DELETE FROM scheduled_tasks WHERE id = ?1", params![id])?;
            Ok(n > 0)
        })
    }

    // ============================================================
    // Task run logs
    // ============================================================

    pub fn insert_task_run_log(&self, e: &TaskRunLogInsert) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO task_run_logs (task_id, run_at, duration_ms, status, result, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    e.task_id,
                    e.run_at,
                    e.duration_ms,
                    e.status.as_str(),
                    e.result,
                    e.error,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_task_run_logs(&self, task_id: &str, limit: u32) -> Result<Vec<TaskRunLog>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT id, task_id, run_at, duration_ms, status, result, error
                 FROM task_run_logs WHERE task_id = ?1 ORDER BY run_at DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![task_id, limit as i64], |r| {
                    Ok(TaskRunLog {
                        id: r.get(0)?,
                        task_id: r.get(1)?,
                        run_at: r.get(2)?,
                        duration_ms: r.get(3)?,
                        status: RunStatus::parse(&r.get::<_, String>(4)?),
                        result: r.get(5)?,
                        error: r.get(6)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }
}
