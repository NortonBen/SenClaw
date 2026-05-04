use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::types::CoworkRecordingSession;

use super::super::rows::row_to_cowork_recording_session;

impl super::super::Db {
    // ============================================================
    // Cowork — Recording sessions
    // ============================================================

    pub fn insert_cowork_recording_session(&self, s: &CoworkRecordingSession) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO cowork_recording_sessions (id,workspace_id,started_at,ended_at,event_count,total_tokens,agents)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![s.id, s.workspace_id, s.started_at, s.ended_at, s.event_count, s.total_tokens, s.agents],
            )?;
            Ok(())
        })
    }

    pub fn get_cowork_recording_session(&self, id: &str) -> Result<Option<CoworkRecordingSession>> {
        self.with_conn(|c| {
            c.query_row(
                "SELECT * FROM cowork_recording_sessions WHERE id=?1",
                params![id],
                |r| Ok(row_to_cowork_recording_session(r)),
            )
            .optional()?
            .transpose()
        })
    }

    pub fn list_cowork_recording_sessions(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<CoworkRecordingSession>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM cowork_recording_sessions WHERE workspace_id=?1 ORDER BY started_at DESC")?;
            let rows: Vec<_> = stmt.query_map(params![workspace_id], |r| Ok(row_to_cowork_recording_session(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        })
    }

    pub fn update_cowork_recording_session(
        &self,
        id: &str,
        ended_at: Option<&str>,
        event_count: Option<i64>,
        total_tokens: Option<i64>,
    ) -> Result<()> {
        self.with_conn(|c| {
            if let Some(e) = ended_at {
                c.execute(
                    "UPDATE cowork_recording_sessions SET ended_at=?1 WHERE id=?2",
                    params![e, id],
                )?;
            }
            if let Some(ec) = event_count {
                c.execute(
                    "UPDATE cowork_recording_sessions SET event_count=?1 WHERE id=?2",
                    params![ec, id],
                )?;
            }
            if let Some(tt) = total_tokens {
                c.execute(
                    "UPDATE cowork_recording_sessions SET total_tokens=?1 WHERE id=?2",
                    params![tt, id],
                )?;
            }
            Ok(())
        })
    }
}
