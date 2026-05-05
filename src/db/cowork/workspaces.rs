use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::types::CoworkWorkspace;

use super::super::rows::row_to_cowork_workspace;

impl super::super::Db {
    // ============================================================
    // Cowork — Workspaces
    // ============================================================

    pub fn insert_cowork_workspace(&self, ws: &CoworkWorkspace) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO cowork_workspaces (id, name, description, status, root_dir, working_dir, owner, created_at, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,'admin',?7,?8)",
                params![ws.id, ws.name, ws.description, ws.status, ws.root_dir, ws.working_dir, ws.created_at, ws.updated_at],
            )?;
            Ok(())
        })
    }

    pub fn get_cowork_workspace(&self, id: &str) -> Result<Option<CoworkWorkspace>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT * FROM cowork_workspaces WHERE id=?1",
                params![id],
                |r| Ok(row_to_cowork_workspace(r)),
            )
            .optional()?
            .transpose()
        })
    }

    pub fn list_cowork_workspaces(&self) -> Result<Vec<CoworkWorkspace>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM cowork_workspaces ORDER BY created_at DESC")?;
            let rows: Vec<_> = stmt
                .query_map([], |r| Ok(row_to_cowork_workspace(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        })
    }

    pub fn update_cowork_workspace(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
        status: Option<&str>,
        working_dir: Option<&str>,
        now: &str,
    ) -> Result<()> {
        self.with_conn(|c| {
            if let Some(n) = name {
                c.execute(
                    "UPDATE cowork_workspaces SET name=?1,updated_at=?2 WHERE id=?3",
                    params![n, now, id],
                )?;
            }
            if let Some(d) = description {
                c.execute(
                    "UPDATE cowork_workspaces SET description=?1,updated_at=?2 WHERE id=?3",
                    params![d, now, id],
                )?;
            }
            if let Some(s) = status {
                c.execute(
                    "UPDATE cowork_workspaces SET status=?1,updated_at=?2 WHERE id=?3",
                    params![s, now, id],
                )?;
            }
            if let Some(w) = working_dir {
                c.execute(
                    "UPDATE cowork_workspaces SET working_dir=?1,updated_at=?2 WHERE id=?3",
                    params![w, now, id],
                )?;
            }
            Ok(())
        })
    }

    pub fn delete_cowork_workspace(&self, id: &str) -> Result<()> {
        self.with_conn(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM cowork_task_comments WHERE task_id IN \
                 (SELECT id FROM cowork_tasks WHERE workspace_id=?1)",
                params![id],
            )?;
            tx.execute("DELETE FROM cowork_tasks WHERE workspace_id=?1", params![id])?;
            tx.execute("DELETE FROM cowork_messages WHERE workspace_id=?1", params![id])?;
            tx.execute(
                "DELETE FROM cowork_board_entries WHERE workspace_id=?1",
                params![id],
            )?;
            tx.execute(
                "DELETE FROM cowork_recording_sessions WHERE workspace_id=?1",
                params![id],
            )?;
            tx.execute("DELETE FROM cowork_members WHERE workspace_id=?1", params![id])?;
            tx.execute("DELETE FROM cowork_workspaces WHERE id=?1", params![id])?;
            tx.commit()?;
            Ok(())
        })
    }
}
