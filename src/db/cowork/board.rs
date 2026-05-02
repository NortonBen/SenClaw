use anyhow::Result;
use rusqlite::params;

use crate::types::CoworkBoardEntry;

use super::super::rows::row_to_cowork_board_entry;

impl super::super::Db {
    // ============================================================
    // Cowork — Board entries
    // ============================================================

    pub fn insert_cowork_board_entry(&self, e: &CoworkBoardEntry) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO cowork_board_entries (id,workspace_id,section,title,content,author,pinned,tags,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![e.id, e.workspace_id, e.section, e.title, e.content, e.author, e.pinned as i64, e.tags, e.created_at, e.updated_at],
            )?;
            Ok(())
        })
    }

    pub fn get_cowork_board_entries(&self, workspace_id: &str, section: Option<&str>) -> Result<Vec<CoworkBoardEntry>> {
        self.with_conn(|c| {
            if let Some(sec) = section {
                let mut stmt = c.prepare("SELECT * FROM cowork_board_entries WHERE workspace_id=?1 AND section=?2 ORDER BY pinned DESC, updated_at DESC")?;
                let rows: Vec<_> = stmt.query_map(params![workspace_id, sec], |r| Ok(row_to_cowork_board_entry(r)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.into_iter().collect()
            } else {
                let mut stmt = c.prepare("SELECT * FROM cowork_board_entries WHERE workspace_id=?1 ORDER BY section, pinned DESC, updated_at DESC")?;
                let rows: Vec<_> = stmt.query_map(params![workspace_id], |r| Ok(row_to_cowork_board_entry(r)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows.into_iter().collect()
            }
        })
    }

    pub fn update_cowork_board_entry(&self, id: &str, title: Option<&str>, content: Option<&str>, pinned: Option<bool>, tags: Option<&str>, now: &str) -> Result<()> {
        self.with_conn(|c| {
            if let Some(t) = title { c.execute("UPDATE cowork_board_entries SET title=?1,updated_at=?2 WHERE id=?3", params![t, now, id])?; }
            if let Some(ct) = content { c.execute("UPDATE cowork_board_entries SET content=?1,updated_at=?2 WHERE id=?3", params![ct, now, id])?; }
            if let Some(p) = pinned { c.execute("UPDATE cowork_board_entries SET pinned=?1,updated_at=?2 WHERE id=?3", params![p as i64, now, id])?; }
            if let Some(tg) = tags { c.execute("UPDATE cowork_board_entries SET tags=?1,updated_at=?2 WHERE id=?3", params![tg, now, id])?; }
            Ok(())
        })
    }

    pub fn delete_cowork_board_entry(&self, id: &str) -> Result<()> {
        self.with_conn(|c| { c.execute("DELETE FROM cowork_board_entries WHERE id=?1", params![id])?; Ok(()) })
    }
}
