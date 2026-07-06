use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// SQLite store for the Mindmap app: `maps` (one row per mindmap) and `nodes`
/// (a normalized adjacency-list tree — one row per node, `parent_id` NULL = root).
pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS maps (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  title       TEXT NOT NULL,
  description TEXT NOT NULL DEFAULT '',
  layout      TEXT NOT NULL DEFAULT 'mindmap',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS nodes (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  map_id     INTEGER NOT NULL,
  parent_id  INTEGER,
  text       TEXT NOT NULL,
  note       TEXT NOT NULL DEFAULT '',
  color      TEXT,
  shape      TEXT,
  fill       INTEGER NOT NULL DEFAULT 0,
  icon       TEXT,
  pos_x      REAL,
  pos_y      REAL,
  collapsed  INTEGER NOT NULL DEFAULT 0,
  ord        INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_nodes_map    ON nodes(map_id);
CREATE INDEX IF NOT EXISTS idx_nodes_parent ON nodes(parent_id);

CREATE TABLE IF NOT EXISTS chat_sessions (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  map_id     INTEGER NOT NULL,
  title      TEXT NOT NULL DEFAULT 'Hội thoại',
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS chat_messages (
  id         INTEGER PRIMARY KEY AUTOINCREMENT,
  session_id INTEGER NOT NULL,
  role       TEXT NOT NULL,
  content    TEXT NOT NULL,
  model      TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_sessions_map  ON chat_sessions(map_id);
CREATE INDEX IF NOT EXISTS idx_messages_sess ON chat_messages(session_id);
"#;

/// Columns added after v1 — applied to pre-existing DBs (errors on already-present
/// columns are ignored).
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE maps  ADD COLUMN layout TEXT NOT NULL DEFAULT 'mindmap'",
    "ALTER TABLE nodes ADD COLUMN shape TEXT",
    "ALTER TABLE nodes ADD COLUMN fill  INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE nodes ADD COLUMN icon  TEXT",
    "ALTER TABLE nodes ADD COLUMN pos_x REAL",
    "ALTER TABLE nodes ADD COLUMN pos_y REAL",
];

/// A mindmap's metadata (list view).
#[derive(Serialize)]
pub struct MapMeta {
    pub id: i64,
    pub title: String,
    pub description: String,
    /// Layout style: `mindmap` | `org` | `outline` | `right`.
    pub layout: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub node_count: i64,
}

/// A node as stored (flat).
#[derive(Serialize, Clone)]
pub struct Node {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub text: String,
    pub note: String,
    pub color: Option<String>,
    /// Box shape: `rounded` (default) | `rect` | `pill` | `ellipse` | `line`.
    pub shape: Option<String>,
    /// Filled with `color` (vs. outlined with a colored accent).
    pub fill: bool,
    /// Optional leading emoji/icon.
    pub icon: Option<String>,
    /// Custom position (free-drag mode); None = use auto-layout.
    pub pos_x: Option<f64>,
    pub pos_y: Option<f64>,
    pub collapsed: bool,
    pub ord: i64,
}

/// A chat session bound to a map.
#[derive(Serialize)]
pub struct ChatSession {
    pub id: i64,
    pub map_id: i64,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
}

/// One stored chat message.
#[derive(Serialize)]
pub struct ChatMessageRow {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub model: Option<String>,
    pub created_at: i64,
}

/// A node rendered as a nested tree (used by the get-map API + MCP).
#[derive(Serialize)]
pub struct TreeNode {
    pub id: i64,
    pub text: String,
    pub note: String,
    pub color: Option<String>,
    pub shape: Option<String>,
    pub fill: bool,
    pub icon: Option<String>,
    pub pos_x: Option<f64>,
    pub pos_y: Option<f64>,
    pub collapsed: bool,
    pub children: Vec<TreeNode>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        for m in MIGRATIONS {
            let _ = conn.execute(m, []); // ignore "duplicate column" on existing DBs
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }

    // ---- maps ----

    pub fn list_maps(&self) -> Result<Vec<MapMeta>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT m.id, m.title, m.description, m.layout, m.created_at, m.updated_at,
                        (SELECT COUNT(*) FROM nodes n WHERE n.map_id = m.id)
                 FROM maps m ORDER BY m.updated_at DESC",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(MapMeta {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        description: r.get(2)?,
                        layout: r.get(3)?,
                        created_at: r.get(4)?,
                        updated_at: r.get(5)?,
                        node_count: r.get(6)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Create a map plus its root node (text = title). Returns (map_id, root_id).
    pub fn create_map(&self, title: &str, description: &str, layout: &str, now: i64) -> Result<(i64, i64)> {
        self.with(|c| {
            c.execute(
                "INSERT INTO maps(title, description, layout, created_at, updated_at) VALUES(?1,?2,?3,?4,?4)",
                params![title, description, layout, now],
            )?;
            let map_id = c.last_insert_rowid();
            c.execute(
                "INSERT INTO nodes(map_id, parent_id, text, note, color, fill, collapsed, ord, created_at)
                 VALUES(?1, NULL, ?2, '', NULL, 1, 0, 0, ?3)",
                params![map_id, title, now],
            )?;
            let root_id = c.last_insert_rowid();
            Ok((map_id, root_id))
        })
    }

    /// Change a map's layout style.
    pub fn set_layout(&self, map_id: i64, layout: &str, now: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE maps SET layout=?2, updated_at=?3 WHERE id=?1",
                params![map_id, layout, now],
            )?;
            Ok(())
        })
    }

    pub fn rename_map(&self, map_id: i64, title: &str, description: &str, now: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE maps SET title=?2, description=?3, updated_at=?4 WHERE id=?1",
                params![map_id, title, description, now],
            )?;
            Ok(())
        })
    }

    pub fn delete_map(&self, map_id: i64) -> Result<()> {
        self.with(|c| {
            c.execute(
                "DELETE FROM chat_messages WHERE session_id IN (SELECT id FROM chat_sessions WHERE map_id=?1)",
                params![map_id],
            )?;
            c.execute("DELETE FROM chat_sessions WHERE map_id=?1", params![map_id])?;
            c.execute("DELETE FROM nodes WHERE map_id=?1", params![map_id])?;
            c.execute("DELETE FROM maps WHERE id=?1", params![map_id])?;
            Ok(())
        })
    }

    pub fn map_meta(&self, map_id: i64) -> Result<Option<MapMeta>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT id, title, description, layout, created_at, updated_at,
                            (SELECT COUNT(*) FROM nodes n WHERE n.map_id = maps.id)
                     FROM maps WHERE id=?1",
                    params![map_id],
                    |r| {
                        Ok(MapMeta {
                            id: r.get(0)?,
                            title: r.get(1)?,
                            description: r.get(2)?,
                            layout: r.get(3)?,
                            created_at: r.get(4)?,
                            updated_at: r.get(5)?,
                            node_count: r.get(6)?,
                        })
                    },
                )
                .optional()?;
            Ok(row)
        })
    }

    fn touch(c: &Connection, map_id: i64, now: i64) -> Result<()> {
        c.execute("UPDATE maps SET updated_at=?2 WHERE id=?1", params![map_id, now])?;
        Ok(())
    }

    // ---- nodes ----

    pub fn nodes_of(&self, map_id: i64) -> Result<Vec<Node>> {
        self.with(|c| Self::nodes_of_conn(c, map_id))
    }

    fn nodes_of_conn(c: &Connection, map_id: i64) -> Result<Vec<Node>> {
        let mut stmt = c.prepare(
            "SELECT id, parent_id, text, note, color, shape, fill, icon, pos_x, pos_y, collapsed, ord
             FROM nodes WHERE map_id=?1 ORDER BY ord, id",
        )?;
        let rows = stmt
            .query_map(params![map_id], |r| {
                Ok(Node {
                    id: r.get(0)?,
                    parent_id: r.get(1)?,
                    text: r.get(2)?,
                    note: r.get(3)?,
                    color: r.get(4)?,
                    shape: r.get(5)?,
                    fill: r.get::<_, i64>(6)? != 0,
                    icon: r.get(7)?,
                    pos_x: r.get(8)?,
                    pos_y: r.get(9)?,
                    collapsed: r.get::<_, i64>(10)? != 0,
                    ord: r.get(11)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// The full node tree of a map, as a single nested root (or None if empty).
    pub fn tree_of(&self, map_id: i64) -> Result<Option<TreeNode>> {
        let nodes = self.nodes_of(map_id)?;
        Ok(build_tree(nodes))
    }

    fn map_of_node(c: &Connection, node_id: i64) -> Result<i64> {
        c.query_row("SELECT map_id FROM nodes WHERE id=?1", params![node_id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| anyhow!("node {node_id} not found"))
    }

    fn next_ord(c: &Connection, parent_id: i64) -> Result<i64> {
        let ord: i64 = c
            .query_row(
                "SELECT COALESCE(MAX(ord), -1) + 1 FROM nodes WHERE parent_id=?1",
                params![parent_id],
                |r| r.get(0),
            )
            .unwrap_or(0);
        Ok(ord)
    }

    /// Add a child under `parent_id`. Returns the new node id.
    pub fn add_node(&self, parent_id: i64, text: &str, note: &str, color: Option<&str>, now: i64) -> Result<i64> {
        self.with(|c| {
            let map_id = Self::map_of_node(c, parent_id)?;
            let ord = Self::next_ord(c, parent_id)?;
            c.execute(
                "INSERT INTO nodes(map_id, parent_id, text, note, color, collapsed, ord, created_at)
                 VALUES(?1, ?2, ?3, ?4, ?5, 0, ?6, ?7)",
                params![map_id, parent_id, text, note, color, ord, now],
            )?;
            let id = c.last_insert_rowid();
            Self::touch(c, map_id, now)?;
            Ok(id)
        })
    }

    /// Update any subset of a node's fields (None = leave unchanged; an inner
    /// `None` on the double-option fields clears the column).
    #[allow(clippy::too_many_arguments)]
    pub fn update_node(
        &self,
        node_id: i64,
        text: Option<&str>,
        note: Option<&str>,
        color: Option<Option<&str>>,
        shape: Option<Option<&str>>,
        fill: Option<bool>,
        icon: Option<Option<&str>>,
        collapsed: Option<bool>,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            let map_id = Self::map_of_node(c, node_id)?;
            if let Some(t) = text {
                c.execute("UPDATE nodes SET text=?2 WHERE id=?1", params![node_id, t])?;
            }
            if let Some(n) = note {
                c.execute("UPDATE nodes SET note=?2 WHERE id=?1", params![node_id, n])?;
            }
            if let Some(col) = color {
                c.execute("UPDATE nodes SET color=?2 WHERE id=?1", params![node_id, col])?;
            }
            if let Some(sh) = shape {
                c.execute("UPDATE nodes SET shape=?2 WHERE id=?1", params![node_id, sh])?;
            }
            if let Some(f) = fill {
                c.execute("UPDATE nodes SET fill=?2 WHERE id=?1", params![node_id, f as i64])?;
            }
            if let Some(ic) = icon {
                c.execute("UPDATE nodes SET icon=?2 WHERE id=?1", params![node_id, ic])?;
            }
            if let Some(cl) = collapsed {
                c.execute("UPDATE nodes SET collapsed=?2 WHERE id=?1", params![node_id, cl as i64])?;
            }
            Self::touch(c, map_id, now)?;
            Ok(())
        })
    }

    /// Delete a node and its whole subtree. Refuses to delete a root node.
    pub fn delete_node(&self, node_id: i64, now: i64) -> Result<()> {
        self.with(|c| {
            let map_id = Self::map_of_node(c, node_id)?;
            let parent: Option<i64> =
                c.query_row("SELECT parent_id FROM nodes WHERE id=?1", params![node_id], |r| r.get(0))?;
            if parent.is_none() {
                return Err(anyhow!("cannot delete the root node of a map"));
            }
            // Collect the subtree ids (adjacency-list walk).
            let all = Self::nodes_of_conn(c, map_id)?;
            let ids = subtree_ids(&all, node_id);
            for id in &ids {
                c.execute("DELETE FROM nodes WHERE id=?1", params![id])?;
            }
            Self::touch(c, map_id, now)?;
            Ok(())
        })
    }

    /// Re-parent a node under `new_parent` (both in the same map; no cycles).
    pub fn move_node(&self, node_id: i64, new_parent: i64, now: i64) -> Result<()> {
        self.with(|c| {
            let map_id = Self::map_of_node(c, node_id)?;
            let parent_map = Self::map_of_node(c, new_parent)?;
            if map_id != parent_map {
                return Err(anyhow!("cannot move a node across maps"));
            }
            if node_id == new_parent {
                return Err(anyhow!("cannot parent a node to itself"));
            }
            let all = Self::nodes_of_conn(c, map_id)?;
            if subtree_ids(&all, node_id).contains(&new_parent) {
                return Err(anyhow!("cannot move a node into its own subtree"));
            }
            let ord = Self::next_ord(c, new_parent)?;
            c.execute(
                "UPDATE nodes SET parent_id=?2, ord=?3 WHERE id=?1",
                params![node_id, new_parent, ord],
            )?;
            Self::touch(c, map_id, now)?;
            Ok(())
        })
    }

    /// Bulk-insert a nested subtree of children under `parent_id` (used by AI
    /// generation). Each child is `{text, note?, children?}`. Returns count added.
    pub fn insert_subtree(&self, parent_id: i64, children: &[GenNode], now: i64) -> Result<usize> {
        self.with(|c| {
            let map_id = Self::map_of_node(c, parent_id)?;
            let mut count = 0usize;
            insert_children(c, map_id, parent_id, children, now, &mut count)?;
            Self::touch(c, map_id, now)?;
            Ok(count)
        })
    }

    /// Replace a node's children entirely with `children` (delete existing subtrees
    /// first). Used when regenerating. Returns count added.
    pub fn replace_children(&self, parent_id: i64, children: &[GenNode], now: i64) -> Result<usize> {
        self.with(|c| {
            let map_id = Self::map_of_node(c, parent_id)?;
            let all = Self::nodes_of_conn(c, map_id)?;
            for kid in all.iter().filter(|n| n.parent_id == Some(parent_id)) {
                for id in subtree_ids(&all, kid.id) {
                    c.execute("DELETE FROM nodes WHERE id=?1", params![id])?;
                }
            }
            let mut count = 0usize;
            insert_children(c, map_id, parent_id, children, now, &mut count)?;
            Self::touch(c, map_id, now)?;
            Ok(count)
        })
    }

    /// Set custom (free-drag) positions for a batch of nodes.
    pub fn set_positions(&self, items: &[(i64, f64, f64)], now: i64) -> Result<()> {
        self.with(|c| {
            let mut map_id: Option<i64> = None;
            for (id, x, y) in items {
                c.execute(
                    "UPDATE nodes SET pos_x=?2, pos_y=?3 WHERE id=?1",
                    params![id, x, y],
                )?;
                if map_id.is_none() {
                    map_id = Self::map_of_node(c, *id).ok();
                }
            }
            if let Some(m) = map_id {
                Self::touch(c, m, now)?;
            }
            Ok(())
        })
    }

    /// Clear all custom positions in a map (back to auto-layout).
    pub fn clear_positions(&self, map_id: i64, now: i64) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE nodes SET pos_x=NULL, pos_y=NULL WHERE map_id=?1", params![map_id])?;
            Self::touch(c, map_id, now)?;
            Ok(())
        })
    }

    /// Replace a map's entire node set with `nodes` (preserving ids) and set its
    /// layout. Used by undo/redo to restore a snapshot atomically.
    pub fn restore_map(&self, map_id: i64, nodes: &[RestoreNode], layout: &str, now: i64) -> Result<()> {
        if nodes.is_empty() {
            return Err(anyhow!("refusing to restore an empty map"));
        }
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute("DELETE FROM nodes WHERE map_id=?1", params![map_id])?;
            for n in nodes {
                tx.execute(
                    "INSERT INTO nodes(id, map_id, parent_id, text, note, color, shape, fill, icon, pos_x, pos_y, collapsed, ord, created_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    params![
                        n.id,
                        map_id,
                        n.parent_id,
                        n.text,
                        n.note,
                        n.color,
                        n.shape,
                        n.fill as i64,
                        n.icon,
                        n.pos_x,
                        n.pos_y,
                        n.collapsed as i64,
                        n.ord,
                        now
                    ],
                )?;
            }
            tx.execute(
                "UPDATE maps SET layout=?2, updated_at=?3 WHERE id=?1",
                params![map_id, layout, now],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    /// Whether a map has any custom-positioned node (free-drag has been used).
    pub fn has_positions(&self, map_id: i64) -> Result<bool> {
        self.with(|c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM nodes WHERE map_id=?1 AND pos_x IS NOT NULL",
                params![map_id],
                |r| r.get(0),
            )?;
            Ok(n > 0)
        })
    }

    pub fn node_text(&self, node_id: i64) -> Result<String> {
        self.with(|c| {
            c.query_row("SELECT text FROM nodes WHERE id=?1", params![node_id], |r| r.get(0))
                .map_err(|e| anyhow!(e))
        })
    }

    /// Labels from the root down to `node_id` (inclusive), for generation context.
    pub fn ancestor_path(&self, node_id: i64) -> Result<Vec<String>> {
        self.with(|c| {
            let map_id = Self::map_of_node(c, node_id)?;
            let nodes = Self::nodes_of_conn(c, map_id)?;
            let mut chain = Vec::new();
            let mut cur = Some(node_id);
            while let Some(id) = cur {
                match nodes.iter().find(|n| n.id == id) {
                    Some(n) => {
                        chain.push(n.text.clone());
                        cur = n.parent_id;
                    }
                    None => break,
                }
            }
            chain.reverse();
            Ok(chain)
        })
    }

    // ---- chat sessions ----

    pub fn list_sessions(&self, map_id: i64) -> Result<Vec<ChatSession>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT s.id, s.map_id, s.title, s.created_at, s.updated_at,
                        (SELECT COUNT(*) FROM chat_messages m WHERE m.session_id = s.id)
                 FROM chat_sessions s WHERE s.map_id=?1 ORDER BY s.updated_at DESC",
            )?;
            let rows = stmt
                .query_map(params![map_id], |r| {
                    Ok(ChatSession {
                        id: r.get(0)?,
                        map_id: r.get(1)?,
                        title: r.get(2)?,
                        created_at: r.get(3)?,
                        updated_at: r.get(4)?,
                        message_count: r.get(5)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn create_session(&self, map_id: i64, title: &str, now: i64) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO chat_sessions(map_id, title, created_at, updated_at) VALUES(?1,?2,?3,?3)",
                params![map_id, title, now],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn rename_session(&self, id: i64, title: &str) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE chat_sessions SET title=?2 WHERE id=?1", params![id, title])?;
            Ok(())
        })
    }

    pub fn delete_session(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM chat_messages WHERE session_id=?1", params![id])?;
            c.execute("DELETE FROM chat_sessions WHERE id=?1", params![id])?;
            Ok(())
        })
    }

    pub fn session_messages(&self, session_id: i64) -> Result<Vec<ChatMessageRow>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, role, content, model, created_at FROM chat_messages
                 WHERE session_id=?1 ORDER BY id",
            )?;
            let rows = stmt
                .query_map(params![session_id], |r| {
                    Ok(ChatMessageRow {
                        id: r.get(0)?,
                        role: r.get(1)?,
                        content: r.get(2)?,
                        model: r.get(3)?,
                        created_at: r.get(4)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn add_message(&self, session_id: i64, role: &str, content: &str, model: Option<&str>, now: i64) -> Result<i64> {
        self.with(|c| {
            c.execute(
                "INSERT INTO chat_messages(session_id, role, content, model, created_at) VALUES(?1,?2,?3,?4,?5)",
                params![session_id, role, content, model, now],
            )?;
            c.execute("UPDATE chat_sessions SET updated_at=?2 WHERE id=?1", params![session_id, now])?;
            Ok(c.last_insert_rowid())
        })
    }

    /// The map a session belongs to (for outline grounding / validation).
    pub fn session_map(&self, session_id: i64) -> Result<i64> {
        self.with(|c| {
            c.query_row("SELECT map_id FROM chat_sessions WHERE id=?1", params![session_id], |r| r.get(0))
                .map_err(|e| anyhow!(e))
        })
    }
}

/// A flat node used to restore a whole map (undo/redo). Ids are explicit so a
/// restore preserves node identity across the round-trip.
#[derive(serde::Deserialize)]
pub struct RestoreNode {
    pub id: i64,
    #[serde(default)]
    pub parent_id: Option<i64>,
    pub text: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub fill: bool,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub pos_x: Option<f64>,
    #[serde(default)]
    pub pos_y: Option<f64>,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default)]
    pub ord: i64,
}

/// A generated node from the LLM or a template (nested). Styling fields are
/// optional so LLM output (text/note/children only) and rich templates share one type.
#[derive(serde::Deserialize, Clone)]
pub struct GenNode {
    pub text: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub shape: Option<String>,
    #[serde(default)]
    pub fill: bool,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub children: Vec<GenNode>,
}

fn insert_children(
    c: &Connection,
    map_id: i64,
    parent_id: i64,
    children: &[GenNode],
    now: i64,
    count: &mut usize,
) -> Result<()> {
    let mut ord = Db::next_ord(c, parent_id)?;
    for kid in children {
        let text = kid.text.trim();
        if text.is_empty() {
            continue;
        }
        c.execute(
            "INSERT INTO nodes(map_id, parent_id, text, note, color, shape, fill, icon, collapsed, ord, created_at)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, ?9, ?10)",
            params![
                map_id,
                parent_id,
                text,
                kid.note.trim(),
                kid.color,
                kid.shape,
                kid.fill as i64,
                kid.icon,
                ord,
                now
            ],
        )?;
        let id = c.last_insert_rowid();
        *count += 1;
        ord += 1;
        if !kid.children.is_empty() {
            insert_children(c, map_id, id, &kid.children, now, count)?;
        }
    }
    Ok(())
}

/// Assemble a flat node list into a nested tree rooted at the parent-less node.
pub fn build_tree(nodes: Vec<Node>) -> Option<TreeNode> {
    let root = nodes.iter().find(|n| n.parent_id.is_none())?.clone();
    Some(build_subtree(root.id, &nodes))
}

fn build_subtree(id: i64, all: &[Node]) -> TreeNode {
    let n = all.iter().find(|x| x.id == id).unwrap();
    let mut children: Vec<&Node> = all.iter().filter(|x| x.parent_id == Some(id)).collect();
    children.sort_by_key(|x| (x.ord, x.id));
    TreeNode {
        id: n.id,
        text: n.text.clone(),
        note: n.note.clone(),
        color: n.color.clone(),
        shape: n.shape.clone(),
        fill: n.fill,
        icon: n.icon.clone(),
        pos_x: n.pos_x,
        pos_y: n.pos_y,
        collapsed: n.collapsed,
        children: children.into_iter().map(|c| build_subtree(c.id, all)).collect(),
    }
}

/// All ids in the subtree rooted at `root` (inclusive), via adjacency-list BFS.
fn subtree_ids(all: &[Node], root: i64) -> Vec<i64> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let cur = out[i];
        for n in all.iter().filter(|n| n.parent_id == Some(cur)) {
            out.push(n.id);
        }
        i += 1;
    }
    out
}

/// Per-app data dir, e.g. `~/.senclaw/space-apps/mindmap/`.
pub fn default_data_dir(app: &str) -> PathBuf {
    let base = std::env::var("SENCLAW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".senclaw")
        });
    base.join("space-apps").join(app)
}
