//! Cowork team persistence — minimal CRUD for the multi-agent team table.
//!
//! A team is just a named manager + member list + workspace folder. It
//! becomes a regular chat group on first open (jid = "cowork:<id>"), so
//! all message routing, history, and engine code stays unchanged.

use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::Db;

/// Rich member metadata mirroring the legacy `cowork_members` row:
/// what the member does, when it triggers, when it hands off, and what
/// shape its output must take. All fields optional — a minimal member is
/// just `{ folder }`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamMember {
    /// Agent.folder (or persona name) — the dispatched profile.
    pub folder: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responsibilities: Option<String>,
    /// When this member auto-activates (free-form: cron, keyword, status, etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triggers: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handoff_rules: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance_criteria: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sla: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<String>,
}

impl TeamMember {
    /// Build a minimal member from just a folder slug.
    pub fn from_folder(folder: impl Into<String>) -> Self {
        Self {
            folder: folder.into(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoworkTeam {
    pub id: String,
    pub name: String,
    /// Agent.folder of the manager (lead) profile.
    pub manager_folder: String,
    /// Members the manager can dispatch to. Stored as JSON in
    /// `members_json`. Backward compat: legacy string-array entries
    /// `["folder1"]` are coerced into `[{folder: "folder1"}]` on load.
    pub members: Vec<TeamMember>,
    /// Optional absolute path of the shared workspace.
    pub workspace_dir: Option<String>,
    pub created_at: String,
}

/// Parse the `members_json` column, accepting either the legacy
/// `["folder1", "folder2"]` shape or the new array-of-objects shape.
fn parse_members(raw: &str) -> Vec<TeamMember> {
    // Try the new shape first.
    if let Ok(list) = serde_json::from_str::<Vec<TeamMember>>(raw) {
        return list;
    }
    // Fall back to legacy string array.
    if let Ok(strs) = serde_json::from_str::<Vec<String>>(raw) {
        return strs.into_iter().map(TeamMember::from_folder).collect();
    }
    Vec::new()
}

impl Db {
    pub fn insert_cowork_team(&self, t: &CoworkTeam) -> Result<()> {
        let members_json = serde_json::to_string(&t.members).unwrap_or_else(|_| "[]".into());
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO cowork_teams (id, name, manager_folder, members_json, workspace_dir, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![t.id, t.name, t.manager_folder, members_json, t.workspace_dir, t.created_at],
            )?;
            Ok(())
        })
    }

    pub fn list_cowork_teams(&self) -> Result<Vec<CoworkTeam>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT id, name, manager_folder, members_json, workspace_dir, created_at \
                 FROM cowork_teams ORDER BY created_at DESC",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    let members_json: String = r.get(3)?;
                    let members = parse_members(&members_json);
                    Ok(CoworkTeam {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        manager_folder: r.get(2)?,
                        members,
                        workspace_dir: r.get(4)?,
                        created_at: r.get(5)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn get_cowork_team(&self, id: &str) -> Result<Option<CoworkTeam>> {
        self.with_conn(|c| {
            let row = c
                .query_row(
                    "SELECT id, name, manager_folder, members_json, workspace_dir, created_at \
                     FROM cowork_teams WHERE id = ?1",
                    params![id],
                    |r| {
                        let members_json: String = r.get(3)?;
                        let members = parse_members(&members_json);
                        Ok(CoworkTeam {
                            id: r.get(0)?,
                            name: r.get(1)?,
                            manager_folder: r.get(2)?,
                            members,
                            workspace_dir: r.get(4)?,
                            created_at: r.get(5)?,
                        })
                    },
                )
                .ok();
            Ok(row)
        })
    }

    /// Overwrite the team's member list. Used by member-edit endpoints
    /// (update one member, add new, remove existing) — caller does the
    /// list mutation and passes the full new state.
    pub fn update_cowork_team_members(&self, id: &str, members: &[TeamMember]) -> Result<()> {
        let members_json = serde_json::to_string(members).unwrap_or_else(|_| "[]".into());
        self.with_conn(|c| {
            c.execute(
                "UPDATE cowork_teams SET members_json = ?1 WHERE id = ?2",
                params![members_json, id],
            )?;
            Ok(())
        })
    }

    pub fn delete_cowork_team(&self, id: &str) -> Result<()> {
        self.with_conn(|c| {
            c.execute("DELETE FROM cowork_teams WHERE id = ?1", params![id])?;
            Ok(())
        })
    }
}
