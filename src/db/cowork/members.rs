use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::types::CoworkMember;

use super::super::rows::row_to_cowork_member;

impl super::super::Db {
    // ============================================================
    // Cowork — Members
    // ============================================================

    pub fn insert_cowork_member(&self, m: &CoworkMember) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO cowork_members (workspace_id,member_id,member_type,role,jid,subdir,persona,responsibilities,triggers,handoff_rules,acceptance_criteria,output_format,sla,limits,joined_at,updated_at)
                 VALUES (?1,?2,'agent',?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                params![m.workspace_id, m.member_id, m.role, m.jid, m.subdir, m.persona, m.responsibilities, m.triggers, m.handoff_rules, m.acceptance_criteria, m.output_format, m.sla, m.limits, m.joined_at, m.updated_at],
            )?;
            Ok(())
        })
    }

    pub fn get_cowork_member(&self, workspace_id: &str, member_id: &str) -> Result<Option<CoworkMember>> {
        self.with_conn(|c| {
            c.query_row("SELECT * FROM cowork_members WHERE workspace_id=?1 AND member_id=?2", params![workspace_id, member_id], |r| Ok(row_to_cowork_member(r)))
                .optional()?.transpose()
        })
    }

    pub fn list_cowork_members(&self, workspace_id: &str) -> Result<Vec<CoworkMember>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM cowork_members WHERE workspace_id=?1 ORDER BY joined_at")?;
            let rows: Vec<_> = stmt.query_map(params![workspace_id], |r| Ok(row_to_cowork_member(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect()
        })
    }

    pub fn update_cowork_member(&self, workspace_id: &str, member_id: &str, role: Option<&str>, persona: Option<&str>, responsibilities: Option<&str>, triggers: Option<&str>, handoff_rules: Option<&str>, acceptance_criteria: Option<&str>, output_format: Option<&str>, sla: Option<&str>, limits: Option<&str>, now: &str) -> Result<()> {
        self.with_conn(|c| {
            if let Some(r) = role { c.execute("UPDATE cowork_members SET role=?1,updated_at=?2 WHERE workspace_id=?3 AND member_id=?4", params![r, now, workspace_id, member_id])?; }
            if let Some(p) = persona { c.execute("UPDATE cowork_members SET persona=?1,updated_at=?2 WHERE workspace_id=?3 AND member_id=?4", params![p, now, workspace_id, member_id])?; }
            if let Some(r) = responsibilities { c.execute("UPDATE cowork_members SET responsibilities=?1,updated_at=?2 WHERE workspace_id=?3 AND member_id=?4", params![r, now, workspace_id, member_id])?; }
            if let Some(t) = triggers { c.execute("UPDATE cowork_members SET triggers=?1,updated_at=?2 WHERE workspace_id=?3 AND member_id=?4", params![t, now, workspace_id, member_id])?; }
            if let Some(h) = handoff_rules { c.execute("UPDATE cowork_members SET handoff_rules=?1,updated_at=?2 WHERE workspace_id=?3 AND member_id=?4", params![h, now, workspace_id, member_id])?; }
            if let Some(a) = acceptance_criteria { c.execute("UPDATE cowork_members SET acceptance_criteria=?1,updated_at=?2 WHERE workspace_id=?3 AND member_id=?4", params![a, now, workspace_id, member_id])?; }
            if let Some(o) = output_format { c.execute("UPDATE cowork_members SET output_format=?1,updated_at=?2 WHERE workspace_id=?3 AND member_id=?4", params![o, now, workspace_id, member_id])?; }
            if let Some(s) = sla { c.execute("UPDATE cowork_members SET sla=?1,updated_at=?2 WHERE workspace_id=?3 AND member_id=?4", params![s, now, workspace_id, member_id])?; }
            if let Some(l) = limits { c.execute("UPDATE cowork_members SET limits=?1,updated_at=?2 WHERE workspace_id=?3 AND member_id=?4", params![l, now, workspace_id, member_id])?; }
            Ok(())
        })
    }

    pub fn delete_cowork_member(&self, workspace_id: &str, member_id: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM cowork_members WHERE workspace_id=?1 AND member_id=?2", params![workspace_id, member_id])?;
            Ok(())
        })
    }
}
