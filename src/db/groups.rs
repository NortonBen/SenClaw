use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::types::GroupBinding;

use super::helpers::json_or_null;
use super::rows::row_to_group;

impl super::Db {
    // ============================================================
    // Groups
    // ============================================================

    pub fn upsert_group(&self, g: &GroupBinding) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                r#"
                INSERT INTO groups
                  (jid, folder, name, channel, group_type, is_admin, requires_trigger,
                   allowed_tools, allowed_paths, allowed_work_dirs,
                   bot_token, max_messages, last_active, added_at)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
                ON CONFLICT(jid) DO UPDATE SET
                  folder            = excluded.folder,
                  name              = excluded.name,
                  channel           = excluded.channel,
                  group_type        = excluded.group_type,
                  is_admin          = excluded.is_admin,
                  requires_trigger  = excluded.requires_trigger,
                  allowed_tools     = excluded.allowed_tools,
                  allowed_paths     = excluded.allowed_paths,
                  allowed_work_dirs = excluded.allowed_work_dirs,
                  bot_token         = excluded.bot_token,
                  max_messages      = excluded.max_messages,
                  last_active       = excluded.last_active
                "#,
                params![
                    g.jid,
                    g.folder,
                    g.name,
                    g.channel,
                    g.group_type,
                    g.is_admin as i64,
                    g.requires_trigger as i64,
                    json_or_null(&g.allowed_tools)?,
                    json_or_null(&g.allowed_paths)?,
                    json_or_null(&g.allowed_work_dirs)?,
                    g.bot_token,
                    g.max_messages,
                    g.last_active,
                    g.added_at,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_group(&self, jid: &str) -> Result<Option<GroupBinding>> {
        self.with_conn(|c| {
            let row = c
                .query_row("SELECT * FROM groups WHERE jid = ?1", params![jid], |r| {
                    Ok(row_to_group(r))
                })
                .optional()?;
            row.transpose()
        })
    }

    pub fn list_groups(&self) -> Result<Vec<GroupBinding>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT * FROM groups ORDER BY added_at")?;
            let rows = stmt
                .query_map([], |r| Ok(row_to_group(r)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().collect::<Result<Vec<_>>>()
        })
    }

    pub fn delete_group(&self, jid: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM groups WHERE jid = ?1", params![jid])?;
            Ok(())
        })
    }

    pub fn delete_group_by_folder(&self, folder: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM groups WHERE folder = ?1", params![folder])?;
            Ok(())
        })
    }

    pub fn rename_group_jid(&self, old_jid: &str, new_jid: &str) -> Result<Option<GroupBinding>> {
        self.with_conn_mut(|c| {
            let existing: Option<GroupBinding> = c
                .query_row("SELECT * FROM groups WHERE jid = ?1", params![old_jid], |r| {
                    Ok(row_to_group(r))
                })
                .optional()?
                .transpose()?;
            let Some(mut binding) = existing else { return Ok(None) };
            binding.jid = new_jid.to_owned();

            let tx = c.transaction()?;
            tx.execute("DELETE FROM groups WHERE jid = ?1", params![old_jid])?;
            tx.execute(
                r#"
                INSERT INTO groups
                  (jid, folder, name, channel, is_admin, requires_trigger,
                   allowed_tools, allowed_paths, allowed_work_dirs,
                   bot_token, max_messages, last_active, added_at)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                "#,
                params![
                    binding.jid,
                    binding.folder,
                    binding.name,
                    binding.channel,
                    binding.is_admin as i64,
                    binding.requires_trigger as i64,
                    json_or_null(&binding.allowed_tools)?,
                    json_or_null(&binding.allowed_paths)?,
                    json_or_null(&binding.allowed_work_dirs)?,
                    binding.bot_token,
                    binding.max_messages,
                    binding.last_active,
                    binding.added_at,
                ],
            )?;
            tx.commit()?;
            Ok(Some(binding))
        })
    }

    pub fn touch_group_active(&self, jid: &str, timestamp: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE groups SET last_active = ?1 WHERE jid = ?2",
                params![timestamp, jid],
            )?;
            Ok(())
        })
    }
}
