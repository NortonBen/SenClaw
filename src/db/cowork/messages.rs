use anyhow::Result;
use rusqlite::params;

use crate::types::CoworkMessage;

use super::super::rows::row_to_cowork_message;

impl super::super::Db {
    // ============================================================
    // Cowork — Messages
    // ============================================================

    pub fn insert_cowork_message(&self, msg: &CoworkMessage) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO cowork_messages (id,workspace_id,from_member,to_member,message_type,content,attachments,task_id,is_read,created_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![msg.id, msg.workspace_id, msg.from_member, msg.to_member, msg.message_type, msg.content, msg.attachments, msg.task_id, msg.is_read as i64, msg.created_at],
            )?;
            Ok(())
        })
    }

    pub fn list_cowork_messages(
        &self,
        workspace_id: &str,
        limit: u32,
        since: Option<&str>,
    ) -> Result<Vec<CoworkMessage>> {
        self.with_conn(|c| {
            if let Some(s) = since {
                let mut stmt = c.prepare("SELECT * FROM cowork_messages WHERE workspace_id=?1 AND created_at>?2 ORDER BY created_at ASC LIMIT ?3")?;
                let rows: Vec<_> = stmt
                    .query_map(params![workspace_id, s, limit as i64], |r| {
                        Ok(row_to_cowork_message(r))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.into_iter().collect::<Result<Vec<_>, _>>()
            } else {
                // Newest-first query so LIMIT returns the latest window; reverse to chronological
                // for chat UI (older at top, newest near input).
                let mut stmt = c.prepare("SELECT * FROM cowork_messages WHERE workspace_id=?1 ORDER BY created_at DESC LIMIT ?2")?;
                let rows: Vec<_> = stmt
                    .query_map(params![workspace_id, limit as i64], |r| {
                        Ok(row_to_cowork_message(r))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let mut rows: Vec<CoworkMessage> = rows.into_iter().collect::<Result<_, _>>()?;
                rows.reverse();
                Ok(rows)
            }
        })
    }

    pub fn mark_cowork_message_read(&self, id: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE cowork_messages SET is_read=1 WHERE id=?1",
                params![id],
            )?;
            Ok(())
        })
    }
}
