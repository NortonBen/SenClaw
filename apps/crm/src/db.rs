//! SQLite store for the CRM app: one row per customer, one row per interaction.
//! Avatars are stored as a URL (external or `data:image/...;base64,...` inline)
//! so the whole customer record is portable and self-contained. Tags are stored
//! as a JSON array on the customer row — simple full-text search over tags is
//! enough for the "who is my design-agency customer" use case.

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS customers (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  name        TEXT NOT NULL,
  email       TEXT NOT NULL DEFAULT '',
  phone       TEXT NOT NULL DEFAULT '',
  company     TEXT NOT NULL DEFAULT '',
  title       TEXT NOT NULL DEFAULT '',
  avatar_url  TEXT NOT NULL DEFAULT '',
  notes       TEXT NOT NULL DEFAULT '',
  tags_json   TEXT NOT NULL DEFAULT '[]',
  role        TEXT NOT NULL DEFAULT 'lead',
  source      TEXT NOT NULL DEFAULT '',
  address     TEXT NOT NULL DEFAULT '',
  birthday    TEXT NOT NULL DEFAULT '',
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_customers_email   ON customers(email);
CREATE INDEX IF NOT EXISTS idx_customers_phone   ON customers(phone);
CREATE INDEX IF NOT EXISTS idx_customers_role    ON customers(role);
CREATE INDEX IF NOT EXISTS idx_customers_updated ON customers(updated_at DESC);

-- Relationships: how one customer relates to another. Directional:
--   from_id --(kind)--> to_id
--   e.g.  Anna --(referred_by)--> Tuấn Anh   ("Anna được Tuấn Anh giới thiệu")
-- `source` = 'user' (manually added) or 'ai' (auto-extracted).
CREATE TABLE IF NOT EXISTS relationships (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  from_id     INTEGER NOT NULL,
  to_id       INTEGER NOT NULL,
  kind        TEXT NOT NULL,
  note        TEXT NOT NULL DEFAULT '',
  confidence  REAL NOT NULL DEFAULT 1.0,
  source      TEXT NOT NULL DEFAULT 'user',
  created_at  INTEGER NOT NULL,
  UNIQUE(from_id, to_id, kind)
);
CREATE INDEX IF NOT EXISTS idx_rel_from ON relationships(from_id);
CREATE INDEX IF NOT EXISTS idx_rel_to   ON relationships(to_id);

-- People mentioned in notes/interactions that AI extracted but haven't been
-- resolved to an existing customer yet. Once resolved, materialize as a
-- relationships row and mark `resolved_customer_id`.
CREATE TABLE IF NOT EXISTS extracted_mentions (
  id                   INTEGER PRIMARY KEY AUTOINCREMENT,
  source_customer_id   INTEGER NOT NULL,
  name                 TEXT NOT NULL,
  role_guess           TEXT NOT NULL DEFAULT 'contact',
  kind_guess           TEXT NOT NULL DEFAULT 'contact_of',
  context              TEXT NOT NULL DEFAULT '',
  confidence           REAL NOT NULL DEFAULT 0.5,
  resolved_customer_id INTEGER,
  created_at           INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mentions_source ON extracted_mentions(source_customer_id);
CREATE INDEX IF NOT EXISTS idx_mentions_unresolved ON extracted_mentions(resolved_customer_id) WHERE resolved_customer_id IS NULL;

-- FTS5 index across every text surface in the CRM: name/email/company/notes of
-- customers, interaction summaries+details, relationship notes, extracted
-- mentions. Rows are inserted/updated by triggers below (WHERE possible) plus
-- explicit calls from the write paths. Fallback tokenizer is 'unicode61' so
-- Vietnamese diacritics match with or without tone marks.
CREATE VIRTUAL TABLE IF NOT EXISTS search_index USING fts5(
  entity_type,
  entity_id UNINDEXED,
  customer_id UNINDEXED,
  title,
  body,
  tokenize='unicode61 remove_diacritics 2'
);

-- UI-side view state persisted per key. Currently we only use one key
-- ("network") — the Network view filters + focus + AI-common highlight so
-- switching tabs or reloading the app keeps everything in place. The value
-- is a client-authored JSON blob; the server does not interpret it.
CREATE TABLE IF NOT EXISTS crm_state (
  key        TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

-- Extra contact channels per customer: additional phone numbers, emails,
-- social handles (Zalo/Facebook/LinkedIn/Instagram/X/Telegram/WhatsApp/…).
-- `kind` is a free-form slug validated by the UI (see CHANNEL_META in the
-- frontend). `value` is the raw handle/phone/URL. `label` is optional user
-- shorthand ("Công việc", "Cá nhân", "Vợ"). URL construction happens at the
-- render side so the server doesn't need to know every social schema.
CREATE TABLE IF NOT EXISTS customer_channels (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  customer_id  INTEGER NOT NULL,
  kind         TEXT NOT NULL,
  value        TEXT NOT NULL,
  label        TEXT NOT NULL DEFAULT '',
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_channels_customer ON customer_channels(customer_id);
CREATE INDEX IF NOT EXISTS idx_channels_kind     ON customer_channels(kind);

CREATE TABLE IF NOT EXISTS interactions (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  customer_id  INTEGER NOT NULL,
  kind         TEXT NOT NULL,
  summary      TEXT NOT NULL,
  details      TEXT NOT NULL DEFAULT '',
  occurred_at  INTEGER NOT NULL,
  created_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_interactions_customer ON interactions(customer_id, occurred_at DESC);

CREATE TABLE IF NOT EXISTS deals (
  id                 INTEGER PRIMARY KEY AUTOINCREMENT,
  customer_id        INTEGER NOT NULL,
  title              TEXT NOT NULL,
  amount             REAL NOT NULL DEFAULT 0,
  currency           TEXT NOT NULL DEFAULT 'VND',
  stage              TEXT NOT NULL DEFAULT 'qualifying',
  probability        INTEGER NOT NULL DEFAULT 50,
  expected_close_at  INTEGER,
  closed_at          INTEGER,
  notes              TEXT NOT NULL DEFAULT '',
  created_at         INTEGER NOT NULL,
  updated_at         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_deals_customer ON deals(customer_id);
CREATE INDEX IF NOT EXISTS idx_deals_stage    ON deals(stage);
CREATE INDEX IF NOT EXISTS idx_deals_close    ON deals(expected_close_at);

CREATE TABLE IF NOT EXISTS tasks (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  customer_id  INTEGER,
  title        TEXT NOT NULL,
  details      TEXT NOT NULL DEFAULT '',
  due_at       INTEGER,
  done         INTEGER NOT NULL DEFAULT 0,
  done_at      INTEGER,
  created_at   INTEGER NOT NULL,
  updated_at   INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_tasks_customer ON tasks(customer_id);
CREATE INDEX IF NOT EXISTS idx_tasks_due      ON tasks(due_at) WHERE done = 0;
"#;

/// Row shape returned by the list + get APIs. `tags` is materialized from
/// `tags_json` so the wire format stays a first-class array.
#[derive(Serialize, Clone)]
pub struct Customer {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub company: String,
    pub title: String,
    pub avatar_url: String,
    pub notes: String,
    pub tags: Vec<String>,
    pub role: String,
    pub source: String,
    pub address: String,
    pub birthday: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub interaction_count: i64,
    pub last_interaction_at: Option<i64>,
}

/// A sales opportunity linked to one customer. `stage` is one of
/// `qualifying|proposal|negotiation|won|lost` (or any user-defined string —
/// the Kanban column falls back to a catch-all).
#[derive(Serialize, Clone)]
pub struct Deal {
    pub id: i64,
    pub customer_id: i64,
    pub customer_name: String,
    pub title: String,
    pub amount: f64,
    pub currency: String,
    pub stage: String,
    pub probability: i64,
    pub expected_close_at: Option<i64>,
    pub closed_at: Option<i64>,
    pub notes: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// A directional relationship between two customers.
///   from_id --(kind)--> to_id
/// Example: Anna --(referred_by)--> Tuấn Anh → "Anna was referred by Tuấn Anh".
/// `source` = "user" | "ai" (auto-extracted). `confidence` in 0..1.
#[derive(Serialize, Clone)]
pub struct Relationship {
    pub id: i64,
    pub from_id: i64,
    pub from_name: String,
    pub to_id: i64,
    pub to_name: String,
    pub kind: String,
    pub note: String,
    pub confidence: f64,
    pub source: String,
    pub created_at: i64,
}

/// An AI-extracted person mentioned in a customer's notes/interactions who
/// isn't (yet) a customer themselves. Once matched to a real customer id,
/// materialize as a `relationships` row.
#[derive(Serialize, Clone)]
pub struct ExtractedMention {
    pub id: i64,
    pub source_customer_id: i64,
    pub source_customer_name: String,
    pub name: String,
    pub role_guess: String,
    pub kind_guess: String,
    pub context: String,
    pub confidence: f64,
    pub resolved_customer_id: Option<i64>,
    pub created_at: i64,
}

#[derive(Deserialize, Default)]
pub struct RelationshipCreate {
    pub from_id: i64,
    pub to_id: i64,
    pub kind: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub source: String,
}

/// Extra contact channel: phone/email/social/website. `kind` is a slug like
/// `phone|zalo|facebook|linkedin|x|instagram|tiktok|youtube|github|telegram|
/// whatsapp|signal|line|wechat|skype|viber|discord|messenger|website|email`.
#[derive(Serialize, Clone)]
pub struct CustomerChannel {
    pub id: i64,
    pub customer_id: i64,
    pub kind: String,
    pub value: String,
    pub label: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Deserialize, Default)]
pub struct ChannelCreate {
    pub kind: String,
    pub value: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Deserialize, Default)]
pub struct ChannelPatch {
    pub kind: Option<String>,
    pub value: Option<String>,
    pub label: Option<String>,
}

/// One search hit — used by the FTS5 search endpoint.
#[derive(Serialize, Clone)]
pub struct SearchHit {
    pub entity_type: String,
    pub entity_id: i64,
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
    pub title: String,
    pub snippet: String,
}

/// A follow-up task, optionally linked to a customer.
#[derive(Serialize, Clone)]
pub struct Task {
    pub id: i64,
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
    pub title: String,
    pub details: String,
    pub due_at: Option<i64>,
    pub done: bool,
    pub done_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One logged touchpoint (call, email, meeting, note).
#[derive(Serialize, Clone)]
pub struct Interaction {
    pub id: i64,
    pub customer_id: i64,
    pub kind: String,
    pub summary: String,
    pub details: String,
    pub occurred_at: i64,
    pub created_at: i64,
}

#[derive(Deserialize, Default)]
pub struct DealCreate {
    pub customer_id: i64,
    pub title: String,
    #[serde(default)]
    pub amount: f64,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub stage: String,
    #[serde(default)]
    pub probability: Option<i64>,
    #[serde(default)]
    pub expected_close_at: Option<i64>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Deserialize, Default)]
pub struct DealPatch {
    pub title: Option<String>,
    pub amount: Option<f64>,
    pub currency: Option<String>,
    pub stage: Option<String>,
    pub probability: Option<i64>,
    /// `Some(None)` clears the field, `Some(Some(x))` sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_close_at: Option<Option<i64>>,
    pub notes: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct TaskCreate {
    #[serde(default)]
    pub customer_id: Option<i64>,
    pub title: String,
    #[serde(default)]
    pub details: String,
    #[serde(default)]
    pub due_at: Option<i64>,
}

/// Fields the caller may PATCH on a customer. `None` = leave unchanged; for
/// scalar TEXT columns an inner empty string clears the field.
#[derive(Deserialize, Default)]
pub struct CustomerPatch {
    pub name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub company: Option<String>,
    pub title: Option<String>,
    pub avatar_url: Option<String>,
    pub notes: Option<String>,
    pub tags: Option<Vec<String>>,
    pub role: Option<String>,
    pub source: Option<String>,
    pub address: Option<String>,
    pub birthday: Option<String>,
}

/// All fields for a fresh customer. `name` is the only requirement.
#[derive(Deserialize, Default)]
pub struct CustomerCreate {
    pub name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub company: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub avatar_url: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub birthday: String,
}

/// Migrations applied to pre-existing DBs. Each ALTER is wrapped so a
/// "duplicate column" on a fresh DB is silently ignored.
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE customers ADD COLUMN role TEXT NOT NULL DEFAULT 'lead'",
    // Historic column name — copy its values over on first upgrade, then leave
    // it alone. We do NOT drop the column (SQLite ALTER DROP COLUMN needs 3.35+
    // and this is a one-shot copy, not a hot-path perf concern).
    "UPDATE customers SET role = status WHERE role = 'lead' AND status <> '' AND status IS NOT NULL",
];

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        for m in MIGRATIONS {
            let _ = conn.execute(m, []); // ignore "duplicate column" / column missing
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap();
        f(&conn)
    }

    fn row_to_customer(r: &rusqlite::Row) -> rusqlite::Result<Customer> {
        let tags_json: String = r.get("tags_json")?;
        let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
        Ok(Customer {
            id: r.get("id")?,
            name: r.get("name")?,
            email: r.get("email")?,
            phone: r.get("phone")?,
            company: r.get("company")?,
            title: r.get("title")?,
            avatar_url: r.get("avatar_url")?,
            notes: r.get("notes")?,
            tags,
            role: r.get("role")?,
            source: r.get("source")?,
            address: r.get("address")?,
            birthday: r.get("birthday")?,
            created_at: r.get("created_at")?,
            updated_at: r.get("updated_at")?,
            interaction_count: r.get::<_, i64>("interaction_count").unwrap_or(0),
            last_interaction_at: r.get::<_, Option<i64>>("last_interaction_at").unwrap_or(None),
        })
    }

    /// List / search customers. `q` searches name/email/phone/company/tags with
    /// simple LIKE, `tag` narrows to customers carrying that tag verbatim,
    /// `role` narrows by relationship role (customer / lead / partner / …).
    pub fn list_customers(
        &self,
        q: Option<&str>,
        tag: Option<&str>,
        role: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Customer>> {
        self.with(|c| {
            let mut sql = String::from(
                "SELECT c.*,
                        (SELECT COUNT(*) FROM interactions i WHERE i.customer_id = c.id) AS interaction_count,
                        (SELECT MAX(i.occurred_at) FROM interactions i WHERE i.customer_id = c.id) AS last_interaction_at
                 FROM customers c WHERE 1=1",
            );
            let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            if let Some(q) = q.filter(|s| !s.trim().is_empty()) {
                let like = format!("%{}%", q.trim());
                sql.push_str(" AND (c.name LIKE ?1 OR c.email LIKE ?1 OR c.phone LIKE ?1 OR c.company LIKE ?1 OR c.tags_json LIKE ?1 OR c.notes LIKE ?1)");
                args.push(Box::new(like));
            }
            if let Some(tag) = tag.filter(|s| !s.trim().is_empty()) {
                // Match `"<tag>"` inside the JSON array. Case-sensitive; the UI
                // normalizes tag casing when writing.
                sql.push_str(&format!(" AND c.tags_json LIKE ?{}", args.len() + 1));
                args.push(Box::new(format!("%\"{}\"%", tag.trim())));
            }
            if let Some(rl) = role.filter(|s| !s.trim().is_empty()) {
                sql.push_str(&format!(" AND c.role = ?{}", args.len() + 1));
                args.push(Box::new(rl.to_string()));
            }
            sql.push_str(" ORDER BY c.updated_at DESC LIMIT ");
            sql.push_str(&limit.max(1).min(500).to_string());
            let mut stmt = c.prepare(&sql)?;
            let params_ref: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
            let rows = stmt
                .query_map(params_ref.as_slice(), Self::row_to_customer)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn get_customer(&self, id: i64) -> Result<Option<Customer>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT c.*,
                            (SELECT COUNT(*) FROM interactions i WHERE i.customer_id = c.id) AS interaction_count,
                            (SELECT MAX(i.occurred_at) FROM interactions i WHERE i.customer_id = c.id) AS last_interaction_at
                     FROM customers c WHERE c.id = ?1",
                    params![id],
                    Self::row_to_customer,
                )
                .optional()?;
            Ok(row)
        })
    }

    pub fn find_by_email(&self, email: &str) -> Result<Option<Customer>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT c.*,
                            (SELECT COUNT(*) FROM interactions i WHERE i.customer_id = c.id) AS interaction_count,
                            (SELECT MAX(i.occurred_at) FROM interactions i WHERE i.customer_id = c.id) AS last_interaction_at
                     FROM customers c WHERE lower(c.email) = lower(?1) LIMIT 1",
                    params![email.trim()],
                    Self::row_to_customer,
                )
                .optional()?;
            Ok(row)
        })
    }

    pub fn create_customer(&self, c: &CustomerCreate, now: i64) -> Result<i64> {
        if c.name.trim().is_empty() {
            return Err(anyhow!("name is required"));
        }
        let tags_json = serde_json::to_string(&normalize_tags(&c.tags)).unwrap_or_else(|_| "[]".into());
        let role = if c.role.trim().is_empty() { "lead" } else { c.role.trim() };
        let id = self.with(|conn| {
            conn.execute(
                "INSERT INTO customers(name, email, phone, company, title, avatar_url, notes, tags_json, role, source, address, birthday, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?13)",
                params![
                    c.name.trim(),
                    c.email.trim(),
                    c.phone.trim(),
                    c.company.trim(),
                    c.title.trim(),
                    c.avatar_url.trim(),
                    c.notes,
                    tags_json,
                    role,
                    c.source.trim(),
                    c.address.trim(),
                    c.birthday.trim(),
                    now,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })?;
        // FTS index for the new row.
        let _ = self.reindex_customer(id);
        Ok(id)
    }

    pub fn update_customer(&self, id: i64, patch: &CustomerPatch, now: i64) -> Result<()> {
        self.with(|c| {
            if c.query_row("SELECT 1 FROM customers WHERE id=?1", params![id], |_| Ok(()))
                .optional()?
                .is_none()
            {
                return Err(anyhow!("customer {id} not found"));
            }
            if let Some(v) = patch.name.as_deref().map(str::trim) {
                if v.is_empty() {
                    return Err(anyhow!("name cannot be empty"));
                }
                c.execute("UPDATE customers SET name=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = patch.email.as_deref() {
                c.execute("UPDATE customers SET email=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = patch.phone.as_deref() {
                c.execute("UPDATE customers SET phone=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = patch.company.as_deref() {
                c.execute("UPDATE customers SET company=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = patch.title.as_deref() {
                c.execute("UPDATE customers SET title=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = patch.avatar_url.as_deref() {
                c.execute("UPDATE customers SET avatar_url=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = patch.notes.as_deref() {
                c.execute("UPDATE customers SET notes=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(tags) = &patch.tags {
                let tags_json = serde_json::to_string(&normalize_tags(tags)).unwrap_or_else(|_| "[]".into());
                c.execute("UPDATE customers SET tags_json=?2 WHERE id=?1", params![id, tags_json])?;
            }
            if let Some(v) = patch.role.as_deref() {
                let st = if v.trim().is_empty() { "lead" } else { v.trim() };
                c.execute("UPDATE customers SET role=?2 WHERE id=?1", params![id, st])?;
            }
            if let Some(v) = patch.source.as_deref() {
                c.execute("UPDATE customers SET source=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = patch.address.as_deref() {
                c.execute("UPDATE customers SET address=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            if let Some(v) = patch.birthday.as_deref() {
                c.execute("UPDATE customers SET birthday=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            c.execute("UPDATE customers SET updated_at=?2 WHERE id=?1", params![id, now])?;
            Ok(())
        })?;
        let _ = self.reindex_customer(id);
        Ok(())
    }

    pub fn delete_customer(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM interactions WHERE customer_id=?1", params![id])?;
            c.execute("DELETE FROM relationships WHERE from_id=?1 OR to_id=?1", params![id])?;
            c.execute("DELETE FROM extracted_mentions WHERE source_customer_id=?1 OR resolved_customer_id=?1", params![id])?;
            c.execute("DELETE FROM customer_channels WHERE customer_id=?1", params![id])?;
            c.execute("DELETE FROM search_index WHERE customer_id=?1", params![id])?;
            let n = c.execute("DELETE FROM customers WHERE id=?1", params![id])?;
            if n == 0 {
                return Err(anyhow!("customer {id} not found"));
            }
            Ok(())
        })
    }

    /// Every tag currently in use, sorted, unique. Powers the tag-filter chip row.
    pub fn all_tags(&self) -> Result<Vec<String>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT tags_json FROM customers")?;
            let rows: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect();
            let mut set = std::collections::BTreeSet::new();
            for j in rows {
                if let Ok(v) = serde_json::from_str::<Vec<String>>(&j) {
                    for t in v {
                        let t = t.trim();
                        if !t.is_empty() {
                            set.insert(t.to_string());
                        }
                    }
                }
            }
            Ok(set.into_iter().collect())
        })
    }

    // ---- interactions ----

    pub fn add_interaction(
        &self,
        customer_id: i64,
        kind: &str,
        summary: &str,
        details: &str,
        occurred_at: i64,
        now: i64,
    ) -> Result<i64> {
        if summary.trim().is_empty() {
            return Err(anyhow!("summary is required"));
        }
        let id = self.with(|c| {
            if c.query_row("SELECT 1 FROM customers WHERE id=?1", params![customer_id], |_| Ok(()))
                .optional()?
                .is_none()
            {
                return Err(anyhow!("customer {customer_id} not found"));
            }
            c.execute(
                "INSERT INTO interactions(customer_id, kind, summary, details, occurred_at, created_at)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![customer_id, kind, summary.trim(), details, occurred_at, now],
            )?;
            let id = c.last_insert_rowid();
            c.execute("UPDATE customers SET updated_at=?2 WHERE id=?1", params![customer_id, now])?;
            Ok(id)
        })?;
        let _ = self.reindex_customer(customer_id);
        Ok(id)
    }

    pub fn list_interactions(&self, customer_id: i64, limit: i64) -> Result<Vec<Interaction>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, customer_id, kind, summary, details, occurred_at, created_at
                 FROM interactions WHERE customer_id=?1 ORDER BY occurred_at DESC, id DESC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![customer_id, limit.max(1).min(500)], |r| {
                    Ok(Interaction {
                        id: r.get(0)?,
                        customer_id: r.get(1)?,
                        kind: r.get(2)?,
                        summary: r.get(3)?,
                        details: r.get(4)?,
                        occurred_at: r.get(5)?,
                        created_at: r.get(6)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn delete_interaction(&self, id: i64, now: i64) -> Result<()> {
        self.with(|c| {
            let customer_id: Option<i64> = c
                .query_row("SELECT customer_id FROM interactions WHERE id=?1", params![id], |r| r.get(0))
                .optional()?;
            let Some(cid) = customer_id else {
                return Err(anyhow!("interaction {id} not found"));
            };
            c.execute("DELETE FROM interactions WHERE id=?1", params![id])?;
            c.execute("UPDATE customers SET updated_at=?2 WHERE id=?1", params![cid, now])?;
            Ok(())
        })
    }

    /// Total customers / interactions / open-tasks / pipeline value plus per-role
    /// customer counts and per-stage deal counts+values for the dashboard.
    pub fn stats(&self) -> Result<serde_json::Value> {
        self.with(|c| {
            let total: i64 = c.query_row("SELECT COUNT(*) FROM customers", [], |r| r.get(0))?;
            let interactions: i64 = c.query_row("SELECT COUNT(*) FROM interactions", [], |r| r.get(0))?;
            let open_tasks: i64 = c.query_row("SELECT COUNT(*) FROM tasks WHERE done=0", [], |r| r.get(0))?;
            let overdue_tasks: i64 = c.query_row(
                "SELECT COUNT(*) FROM tasks WHERE done=0 AND due_at IS NOT NULL AND due_at < strftime('%s','now')",
                [],
                |r| r.get(0),
            )?;
            let open_deals: i64 = c.query_row(
                "SELECT COUNT(*) FROM deals WHERE stage NOT IN ('won','lost')",
                [],
                |r| r.get(0),
            )?;
            let pipeline_value: f64 = c
                .query_row(
                    "SELECT COALESCE(SUM(amount), 0) FROM deals WHERE stage NOT IN ('won','lost')",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0.0);
            let won_value: f64 = c
                .query_row(
                    "SELECT COALESCE(SUM(amount), 0) FROM deals WHERE stage='won'",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0.0);
            let mut stmt = c.prepare("SELECT role, COUNT(*) FROM customers GROUP BY role")?;
            let by_role: serde_json::Map<String, serde_json::Value> = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
                .filter_map(|r| r.ok())
                .map(|(k, v)| (k, serde_json::Value::from(v)))
                .collect();
            let mut stmt = c.prepare(
                "SELECT stage, COUNT(*), COALESCE(SUM(amount), 0) FROM deals GROUP BY stage",
            )?;
            let by_stage: serde_json::Map<String, serde_json::Value> = stmt
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, f64>(2)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .map(|(stage, n, sum)| {
                    (stage, serde_json::json!({ "count": n, "value": sum }))
                })
                .collect();
            Ok(serde_json::json!({
                "customers": total,
                "interactions": interactions,
                "open_tasks": open_tasks,
                "overdue_tasks": overdue_tasks,
                "open_deals": open_deals,
                "pipeline_value": pipeline_value,
                "won_value": won_value,
                "by_role": by_role,
                "by_stage": by_stage,
            }))
        })
    }

    // ---- deals ----

    fn row_to_deal(r: &rusqlite::Row) -> rusqlite::Result<Deal> {
        Ok(Deal {
            id: r.get("id")?,
            customer_id: r.get("customer_id")?,
            customer_name: r.get::<_, String>("customer_name").unwrap_or_default(),
            title: r.get("title")?,
            amount: r.get("amount")?,
            currency: r.get("currency")?,
            stage: r.get("stage")?,
            probability: r.get("probability")?,
            expected_close_at: r.get("expected_close_at")?,
            closed_at: r.get("closed_at")?,
            notes: r.get("notes")?,
            created_at: r.get("created_at")?,
            updated_at: r.get("updated_at")?,
        })
    }

    pub fn create_deal(&self, d: &DealCreate, now: i64) -> Result<i64> {
        if d.title.trim().is_empty() {
            return Err(anyhow!("title is required"));
        }
        let stage = if d.stage.trim().is_empty() { "qualifying" } else { d.stage.trim() };
        let currency = if d.currency.trim().is_empty() { "VND" } else { d.currency.trim() };
        let prob = d.probability.unwrap_or(50).clamp(0, 100);
        self.with(|c| {
            if c.query_row("SELECT 1 FROM customers WHERE id=?1", params![d.customer_id], |_| Ok(()))
                .optional()?
                .is_none()
            {
                return Err(anyhow!("customer {} not found", d.customer_id));
            }
            let closed_at = if stage == "won" || stage == "lost" { Some(now) } else { None };
            c.execute(
                "INSERT INTO deals(customer_id, title, amount, currency, stage, probability, expected_close_at, closed_at, notes, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)",
                params![
                    d.customer_id, d.title.trim(), d.amount, currency, stage, prob,
                    d.expected_close_at, closed_at, d.notes, now,
                ],
            )?;
            c.execute("UPDATE customers SET updated_at=?2 WHERE id=?1", params![d.customer_id, now])?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn update_deal(&self, id: i64, patch: &DealPatch, now: i64) -> Result<()> {
        self.with(|c| {
            let cid: i64 = c
                .query_row("SELECT customer_id FROM deals WHERE id=?1", params![id], |r| r.get(0))
                .optional()?
                .ok_or_else(|| anyhow!("deal {id} not found"))?;
            if let Some(v) = patch.title.as_deref().map(str::trim) {
                if v.is_empty() {
                    return Err(anyhow!("title cannot be empty"));
                }
                c.execute("UPDATE deals SET title=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = patch.amount {
                c.execute("UPDATE deals SET amount=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = patch.currency.as_deref().map(str::trim) {
                if !v.is_empty() {
                    c.execute("UPDATE deals SET currency=?2 WHERE id=?1", params![id, v])?;
                }
            }
            if let Some(v) = patch.stage.as_deref().map(str::trim) {
                if !v.is_empty() {
                    let closed_at = if v == "won" || v == "lost" { Some(now) } else { None };
                    c.execute(
                        "UPDATE deals SET stage=?2, closed_at=?3 WHERE id=?1",
                        params![id, v, closed_at],
                    )?;
                }
            }
            if let Some(v) = patch.probability {
                c.execute("UPDATE deals SET probability=?2 WHERE id=?1", params![id, v.clamp(0, 100)])?;
            }
            if let Some(v) = patch.expected_close_at {
                c.execute("UPDATE deals SET expected_close_at=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = patch.notes.as_deref() {
                c.execute("UPDATE deals SET notes=?2 WHERE id=?1", params![id, v])?;
            }
            c.execute("UPDATE deals SET updated_at=?2 WHERE id=?1", params![id, now])?;
            c.execute("UPDATE customers SET updated_at=?2 WHERE id=?1", params![cid, now])?;
            Ok(())
        })
    }

    pub fn delete_deal(&self, id: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute("DELETE FROM deals WHERE id=?1", params![id])?;
            if n == 0 {
                return Err(anyhow!("deal {id} not found"));
            }
            Ok(())
        })
    }

    /// All deals for one customer, newest first.
    pub fn deals_of_customer(&self, customer_id: i64) -> Result<Vec<Deal>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT d.*, c.name AS customer_name FROM deals d
                 JOIN customers c ON c.id = d.customer_id
                 WHERE d.customer_id = ?1
                 ORDER BY d.updated_at DESC",
            )?;
            let rows = stmt
                .query_map(params![customer_id], Self::row_to_deal)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// All deals grouped by stage (for the Kanban board). Optional stage filter.
    pub fn list_deals(&self, stage: Option<&str>) -> Result<Vec<Deal>> {
        self.with(|c| {
            let (sql, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = match stage {
                Some(s) if !s.is_empty() => (
                    "SELECT d.*, c.name AS customer_name FROM deals d
                     JOIN customers c ON c.id = d.customer_id
                     WHERE d.stage = ?1 ORDER BY d.updated_at DESC"
                        .into(),
                    vec![Box::new(s.to_string())],
                ),
                _ => (
                    "SELECT d.*, c.name AS customer_name FROM deals d
                     JOIN customers c ON c.id = d.customer_id
                     ORDER BY d.updated_at DESC"
                        .into(),
                    vec![],
                ),
            };
            let mut stmt = c.prepare(&sql)?;
            let params_ref: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
            let rows = stmt
                .query_map(params_ref.as_slice(), Self::row_to_deal)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    // ---- tasks ----

    fn row_to_task(r: &rusqlite::Row) -> rusqlite::Result<Task> {
        Ok(Task {
            id: r.get("id")?,
            customer_id: r.get("customer_id")?,
            customer_name: r.get::<_, Option<String>>("customer_name")?,
            title: r.get("title")?,
            details: r.get("details")?,
            due_at: r.get("due_at")?,
            done: r.get::<_, i64>("done")? != 0,
            done_at: r.get("done_at")?,
            created_at: r.get("created_at")?,
            updated_at: r.get("updated_at")?,
        })
    }

    pub fn create_task(&self, t: &TaskCreate, now: i64) -> Result<i64> {
        if t.title.trim().is_empty() {
            return Err(anyhow!("title is required"));
        }
        self.with(|c| {
            if let Some(cid) = t.customer_id {
                if c.query_row("SELECT 1 FROM customers WHERE id=?1", params![cid], |_| Ok(()))
                    .optional()?
                    .is_none()
                {
                    return Err(anyhow!("customer {cid} not found"));
                }
            }
            c.execute(
                "INSERT INTO tasks(customer_id, title, details, due_at, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,?5)",
                params![t.customer_id, t.title.trim(), t.details, t.due_at, now],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Toggle a task's done state. `done_at` is set on transition to done, cleared
    /// on transition back to open.
    pub fn set_task_done(&self, id: i64, done: bool, now: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute(
                "UPDATE tasks SET done=?2, done_at=?3, updated_at=?4 WHERE id=?1",
                params![id, done as i64, if done { Some(now) } else { None }, now],
            )?;
            if n == 0 {
                return Err(anyhow!("task {id} not found"));
            }
            Ok(())
        })
    }

    /// Reverse-sync task update: apply title/due_at changes coming from an
    /// external calendar. Kept separate from the normal task-CRUD path so we
    /// can log it distinctly and never touch fields we don't understand.
    pub fn reverse_update_task(
        &self,
        id: i64,
        new_title: Option<&str>,
        new_due: Option<i64>,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            if let Some(t) = new_title {
                c.execute("UPDATE tasks SET title=?2 WHERE id=?1", params![id, t])?;
            }
            if let Some(d) = new_due {
                c.execute("UPDATE tasks SET due_at=?2 WHERE id=?1", params![id, d])?;
            }
            c.execute("UPDATE tasks SET updated_at=?2 WHERE id=?1", params![id, now])?;
            Ok(())
        })
    }

    pub fn delete_task(&self, id: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute("DELETE FROM tasks WHERE id=?1", params![id])?;
            if n == 0 {
                return Err(anyhow!("task {id} not found"));
            }
            Ok(())
        })
    }

    /// Tasks for one customer (newest first, open first).
    pub fn tasks_of_customer(&self, customer_id: i64) -> Result<Vec<Task>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT t.*, c.name AS customer_name FROM tasks t
                 LEFT JOIN customers c ON c.id = t.customer_id
                 WHERE t.customer_id = ?1
                 ORDER BY t.done ASC, COALESCE(t.due_at, 9999999999) ASC, t.id DESC",
            )?;
            let rows = stmt
                .query_map(params![customer_id], Self::row_to_task)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// List tasks. `open_only` filters to unfinished, `limit` caps the result.
    pub fn list_tasks(&self, open_only: bool, limit: i64) -> Result<Vec<Task>> {
        self.with(|c| {
            let sql = if open_only {
                "SELECT t.*, c.name AS customer_name FROM tasks t
                 LEFT JOIN customers c ON c.id = t.customer_id
                 WHERE t.done = 0
                 ORDER BY COALESCE(t.due_at, 9999999999) ASC, t.id DESC LIMIT ?1"
            } else {
                "SELECT t.*, c.name AS customer_name FROM tasks t
                 LEFT JOIN customers c ON c.id = t.customer_id
                 ORDER BY t.done ASC, COALESCE(t.due_at, 9999999999) ASC, t.id DESC LIMIT ?1"
            };
            let mut stmt = c.prepare(sql)?;
            let rows = stmt
                .query_map(params![limit.max(1).min(500)], Self::row_to_task)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    // ---- cross-cutting feeds ----

    /// Global timeline of every interaction, newest first.
    pub fn recent_activity(&self, limit: i64) -> Result<Vec<serde_json::Value>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT i.id, i.customer_id, c.name AS customer_name, i.kind, i.summary,
                        i.details, i.occurred_at
                 FROM interactions i
                 JOIN customers c ON c.id = i.customer_id
                 ORDER BY i.occurred_at DESC, i.id DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![limit.max(1).min(500)], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, i64>(0)?,
                        "customer_id": r.get::<_, i64>(1)?,
                        "customer_name": r.get::<_, String>(2)?,
                        "kind": r.get::<_, String>(3)?,
                        "summary": r.get::<_, String>(4)?,
                        "details": r.get::<_, String>(5)?,
                        "occurred_at": r.get::<_, i64>(6)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Everything coming up in the next `window_days`: open tasks with a due_at
    /// in range + birthdays whose next occurrence lands in the window.
    pub fn upcoming(&self, now: i64, window_days: i64) -> Result<serde_json::Value> {
        self.with(|c| {
            let horizon = now + window_days.max(1) * 86400;
            let mut stmt = c.prepare(
                "SELECT t.id, t.title, t.due_at, t.customer_id, c.name AS customer_name
                 FROM tasks t
                 LEFT JOIN customers c ON c.id = t.customer_id
                 WHERE t.done = 0 AND t.due_at IS NOT NULL AND t.due_at <= ?1
                 ORDER BY t.due_at ASC",
            )?;
            let tasks: Vec<serde_json::Value> = stmt
                .query_map(params![horizon], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, i64>(0)?,
                        "title": r.get::<_, String>(1)?,
                        "due_at": r.get::<_, i64>(2)?,
                        "customer_id": r.get::<_, Option<i64>>(3)?,
                        "customer_name": r.get::<_, Option<String>>(4)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            // Birthdays: customers with a `birthday` string of form YYYY-MM-DD or
            // MM-DD. Compute the next occurrence and keep it if within window.
            let mut stmt = c.prepare(
                "SELECT id, name, birthday FROM customers WHERE birthday <> ''",
            )?;
            let raw: Vec<(i64, String, String)> = stmt
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
                .filter_map(|r| r.ok())
                .collect();
            let birthdays: Vec<serde_json::Value> = raw
                .into_iter()
                .filter_map(|(id, name, b)| {
                    let (mm, dd) = parse_month_day(&b)?;
                    let next = next_occurrence(now, mm, dd)?;
                    if next <= horizon {
                        Some(serde_json::json!({
                            "customer_id": id,
                            "customer_name": name,
                            "birthday": b,
                            "next_at": next,
                        }))
                    } else {
                        None
                    }
                })
                .collect();
            Ok(serde_json::json!({
                "now": now,
                "window_days": window_days,
                "tasks": tasks,
                "birthdays": birthdays,
            }))
        })
    }

    /// Customers ordered by their interaction count DESC, limit N. Used to
    /// ground the aggregate AI report.
    pub fn top_active_customers(&self, limit: i64) -> Result<Vec<(Customer, i64)>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT c.*,
                        (SELECT COUNT(*) FROM interactions i WHERE i.customer_id = c.id) AS interaction_count,
                        (SELECT MAX(i.occurred_at) FROM interactions i WHERE i.customer_id = c.id) AS last_interaction_at
                 FROM customers c
                 ORDER BY interaction_count DESC, c.updated_at DESC
                 LIMIT ?1",
            )?;
            let rows: Vec<(Customer, i64)> = stmt
                .query_map(params![limit.max(1).min(100)], |r| {
                    let n: i64 = r.get::<_, i64>("interaction_count")?;
                    let cust = Self::row_to_customer(r)?;
                    Ok((cust, n))
                })?
                .filter_map(|r| r.ok())
                .filter(|(_, n)| *n > 0)
                .collect();
            Ok(rows)
        })
    }

    /// Top open deals by amount DESC (excludes won/lost).
    pub fn top_open_deals(&self, limit: i64) -> Result<Vec<Deal>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT d.*, c.name AS customer_name FROM deals d
                 JOIN customers c ON c.id = d.customer_id
                 WHERE d.stage NOT IN ('won','lost')
                 ORDER BY d.amount DESC LIMIT ?1",
            )?;
            let rows = stmt
                .query_map(params![limit.max(1).min(50)], Self::row_to_deal)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Open tasks with due_at in the past (order by how overdue they are).
    pub fn overdue_tasks(&self, now: i64, limit: i64) -> Result<Vec<Task>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT t.*, c.name AS customer_name FROM tasks t
                 LEFT JOIN customers c ON c.id = t.customer_id
                 WHERE t.done = 0 AND t.due_at IS NOT NULL AND t.due_at < ?1
                 ORDER BY t.due_at ASC LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![now, limit.max(1).min(50)], Self::row_to_task)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    // ---- relationships ----

    fn row_to_relationship(r: &rusqlite::Row) -> rusqlite::Result<Relationship> {
        Ok(Relationship {
            id: r.get("id")?,
            from_id: r.get("from_id")?,
            from_name: r.get::<_, String>("from_name").unwrap_or_default(),
            to_id: r.get("to_id")?,
            to_name: r.get::<_, String>("to_name").unwrap_or_default(),
            kind: r.get("kind")?,
            note: r.get("note")?,
            confidence: r.get("confidence")?,
            source: r.get("source")?,
            created_at: r.get("created_at")?,
        })
    }

    pub fn add_relationship(&self, r: &RelationshipCreate, now: i64) -> Result<i64> {
        if r.kind.trim().is_empty() {
            return Err(anyhow!("kind is required"));
        }
        if r.from_id == r.to_id {
            return Err(anyhow!("from_id and to_id must differ"));
        }
        let source = if r.source.trim().is_empty() { "user" } else { r.source.trim() };
        let conf = r.confidence.unwrap_or(1.0).clamp(0.0, 1.0);
        self.with(|c| {
            for id in [r.from_id, r.to_id] {
                if c.query_row("SELECT 1 FROM customers WHERE id=?1", params![id], |_| Ok(()))
                    .optional()?
                    .is_none()
                {
                    return Err(anyhow!("customer {id} not found"));
                }
            }
            c.execute(
                "INSERT INTO relationships(from_id, to_id, kind, note, confidence, source, created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)
                 ON CONFLICT(from_id, to_id, kind) DO UPDATE SET note=excluded.note, confidence=excluded.confidence, source=excluded.source",
                params![r.from_id, r.to_id, r.kind.trim(), r.note.trim(), conf, source, now],
            )?;
            let id: i64 = c.query_row(
                "SELECT id FROM relationships WHERE from_id=?1 AND to_id=?2 AND kind=?3",
                params![r.from_id, r.to_id, r.kind.trim()],
                |row| row.get(0),
            )?;
            Ok(id)
        })
    }

    pub fn delete_relationship(&self, id: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute("DELETE FROM relationships WHERE id=?1", params![id])?;
            if n == 0 {
                return Err(anyhow!("relationship {id} not found"));
            }
            Ok(())
        })
    }

    /// All relationships that involve `customer_id` (as either endpoint).
    pub fn relationships_of(&self, customer_id: i64) -> Result<Vec<Relationship>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT r.*, cf.name AS from_name, ct.name AS to_name
                 FROM relationships r
                 JOIN customers cf ON cf.id = r.from_id
                 JOIN customers ct ON ct.id = r.to_id
                 WHERE r.from_id = ?1 OR r.to_id = ?1
                 ORDER BY r.created_at DESC",
            )?;
            let rows = stmt
                .query_map(params![customer_id], Self::row_to_relationship)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Every relationship in the CRM (for the graph view).
    pub fn all_relationships(&self) -> Result<Vec<Relationship>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT r.*, cf.name AS from_name, ct.name AS to_name
                 FROM relationships r
                 JOIN customers cf ON cf.id = r.from_id
                 JOIN customers ct ON ct.id = r.to_id",
            )?;
            let rows = stmt
                .query_map([], Self::row_to_relationship)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// A slim customer projection for the network graph — everything needed to
    /// render a node (name, role for colour, avatar for the tooltip).
    pub fn graph_nodes(&self) -> Result<Vec<serde_json::Value>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT c.id, c.name, c.role, c.company, c.avatar_url,
                        (SELECT COUNT(*) FROM interactions i WHERE i.customer_id = c.id) AS interaction_count
                 FROM customers c ORDER BY c.id",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(serde_json::json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "role": r.get::<_, String>(2)?,
                        "company": r.get::<_, String>(3)?,
                        "avatar_url": r.get::<_, String>(4)?,
                        "interaction_count": r.get::<_, i64>(5)?,
                    }))
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    // ---- FTS5 index ----

    /// Rebuild every FTS row for one customer. Idempotent — deletes previous
    /// entries for the customer first. Called on customer/interaction writes.
    fn reindex_customer(&self, customer_id: i64) -> Result<()> {
        self.with(|c| {
            // Wipe every row keyed to this customer (customer + interactions +
            // relationships + mentions). `customer_id` is UNINDEXED so we filter
            // through the base rowid table by using `MATCH` on entity_id — but
            // FTS5 stores entity_id as UNINDEXED, so we use plain =.
            c.execute(
                "DELETE FROM search_index WHERE customer_id = ?1",
                params![customer_id],
            )?;
            // Customer row.
            let cust = c
                .query_row(
                    "SELECT name, email, phone, company, title, role, notes, tags_json, address FROM customers WHERE id=?1",
                    params![customer_id],
                    |r| {
                        let name: String = r.get(0)?;
                        let body = format!(
                            "{} {} {} {} {} role:{} {} {} {}",
                            name,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, String>(4)?,
                            r.get::<_, String>(5)?,
                            r.get::<_, String>(6)?,
                            r.get::<_, String>(7)?,
                            r.get::<_, String>(8)?,
                        );
                        Ok((name, body))
                    },
                )
                .optional()?;
            if let Some((name, body)) = cust {
                c.execute(
                    "INSERT INTO search_index(entity_type, entity_id, customer_id, title, body)
                     VALUES('customer', ?1, ?1, ?2, ?3)",
                    params![customer_id, name, body],
                )?;
            }
            // Interactions.
            let mut stmt = c.prepare(
                "SELECT id, kind, summary, details FROM interactions WHERE customer_id=?1",
            )?;
            let rows: Vec<(i64, String, String, String)> = stmt
                .query_map(params![customer_id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })?
                .filter_map(|r| r.ok())
                .collect();
            for (id, kind, summary, details) in rows {
                let title = format!("[{kind}] {summary}");
                let body = format!("{summary} {details}");
                c.execute(
                    "INSERT INTO search_index(entity_type, entity_id, customer_id, title, body)
                     VALUES('interaction', ?1, ?2, ?3, ?4)",
                    params![id, customer_id, title, body],
                )?;
            }
            // Extra channels (multi-phone + social handles) — index them so
            // "zalo Anna" or "0912345" hit the customer via FTS.
            let mut stmt = c.prepare(
                "SELECT id, kind, value, label FROM customer_channels WHERE customer_id=?1",
            )?;
            let rows: Vec<(i64, String, String, String)> = stmt
                .query_map(params![customer_id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })?
                .filter_map(|r| r.ok())
                .collect();
            for (id, kind, value, label) in rows {
                let title = format!("{kind}: {value}");
                let body = format!("{value} {label} {kind}");
                c.execute(
                    "INSERT INTO search_index(entity_type, entity_id, customer_id, title, body)
                     VALUES('channel', ?1, ?2, ?3, ?4)",
                    params![id, customer_id, title, body],
                )?;
            }
            // Extracted mentions.
            let mut stmt = c.prepare(
                "SELECT id, name, role_guess, context FROM extracted_mentions WHERE source_customer_id=?1",
            )?;
            let rows: Vec<(i64, String, String, String)> = stmt
                .query_map(params![customer_id], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
                })?
                .filter_map(|r| r.ok())
                .collect();
            for (id, name, role_guess, context) in rows {
                let title = format!("mention: {name} ({role_guess})");
                c.execute(
                    "INSERT INTO search_index(entity_type, entity_id, customer_id, title, body)
                     VALUES('mention', ?1, ?2, ?3, ?4)",
                    params![id, customer_id, title, context],
                )?;
            }
            Ok(())
        })
    }

    /// Rebuild the FTS index from scratch. Called if the index is empty on
    /// startup (fresh install after upgrading from a build that didn't have FTS)
    /// or from an admin `reindex` MCP call.
    pub fn reindex_all(&self) -> Result<usize> {
        let ids: Vec<i64> = self.with(|c| {
            let mut stmt = c.prepare("SELECT id FROM customers")?;
            let ids = stmt
                .query_map([], |r| r.get::<_, i64>(0))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(ids)
        })?;
        let n = ids.len();
        for id in ids {
            self.reindex_customer(id)?;
        }
        Ok(n)
    }

    /// True if the search index is empty. Used on startup to lazily rebuild.
    pub fn search_index_empty(&self) -> Result<bool> {
        self.with(|c| {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM search_index", [], |r| r.get(0))?;
            Ok(n == 0)
        })
    }

    /// FTS5 search. Returns hits with a 60-char snippet. Query goes through the
    /// unicode61 tokenizer with diacritic-folding, so "khach" matches "khách".
    pub fn search(&self, q: &str, limit: i64) -> Result<Vec<SearchHit>> {
        let q = q.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        // Escape the query so a user's `-` or `(` doesn't become an FTS operator
        // — wrap the whole thing in quotes and per-token combine with OR to give
        // partial matches ("ann" matches "anna"). Actually simplest: quoted-phrase +
        // prefix on each token.
        let terms = q
            .split_whitespace()
            .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" OR ");
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT s.entity_type, s.entity_id, s.customer_id,
                        s.title,
                        snippet(search_index, 4, '', '', '…', 12) AS snippet,
                        cu.name AS customer_name,
                        bm25(search_index) AS rank
                 FROM search_index s
                 LEFT JOIN customers cu ON cu.id = s.customer_id
                 WHERE search_index MATCH ?1
                 ORDER BY rank
                 LIMIT ?2",
            )?;
            let rows = stmt
                .query_map(params![terms, limit.max(1).min(100)], |r| {
                    Ok(SearchHit {
                        entity_type: r.get("entity_type")?,
                        entity_id: r.get("entity_id")?,
                        customer_id: r.get("customer_id")?,
                        customer_name: r.get::<_, Option<String>>("customer_name")?,
                        title: r.get("title")?,
                        snippet: r.get("snippet")?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    // ---- extracted mentions ----

    /// Save an AI-extracted mention. If `resolved_customer_id` is set and both
    /// endpoints exist, ALSO writes a `relationships` row (source='ai').
    pub fn add_mention(
        &self,
        source_customer_id: i64,
        name: &str,
        role_guess: &str,
        kind_guess: &str,
        context: &str,
        confidence: f64,
        resolved_customer_id: Option<i64>,
        now: i64,
    ) -> Result<i64> {
        if name.trim().is_empty() {
            return Err(anyhow!("name is required"));
        }
        self.with(|c| {
            c.execute(
                "INSERT INTO extracted_mentions(source_customer_id, name, role_guess, kind_guess, context, confidence, resolved_customer_id, created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    source_customer_id, name.trim(), role_guess, kind_guess, context,
                    confidence.clamp(0.0, 1.0), resolved_customer_id, now,
                ],
            )?;
            Ok(c.last_insert_rowid())
        })?;
        // Re-index the source customer so the mention hits FTS.
        let _ = self.reindex_customer(source_customer_id);
        // Materialize as a relationship if we already know both parties.
        if let Some(resolved) = resolved_customer_id {
            if resolved != source_customer_id {
                self.add_relationship(
                    &RelationshipCreate {
                        from_id: source_customer_id,
                        to_id: resolved,
                        kind: kind_guess.to_string(),
                        note: context.to_string(),
                        confidence: Some(confidence),
                        source: "ai".into(),
                    },
                    now,
                )?;
            }
        }
        // Return the id of the mention.
        self.with(|c| Ok(c.last_insert_rowid()))
    }

    /// Every extracted mention, resolved or not. `unresolved_only` skips ones
    /// already turned into relationships.
    pub fn list_mentions(&self, unresolved_only: bool, limit: i64) -> Result<Vec<ExtractedMention>> {
        self.with(|c| {
            let sql = if unresolved_only {
                "SELECT m.*, c.name AS source_customer_name
                 FROM extracted_mentions m JOIN customers c ON c.id = m.source_customer_id
                 WHERE m.resolved_customer_id IS NULL
                 ORDER BY m.confidence DESC, m.id DESC LIMIT ?1"
            } else {
                "SELECT m.*, c.name AS source_customer_name
                 FROM extracted_mentions m JOIN customers c ON c.id = m.source_customer_id
                 ORDER BY m.id DESC LIMIT ?1"
            };
            let mut stmt = c.prepare(sql)?;
            let rows = stmt
                .query_map(params![limit.max(1).min(500)], |r| {
                    Ok(ExtractedMention {
                        id: r.get("id")?,
                        source_customer_id: r.get("source_customer_id")?,
                        source_customer_name: r.get::<_, String>("source_customer_name").unwrap_or_default(),
                        name: r.get("name")?,
                        role_guess: r.get("role_guess")?,
                        kind_guess: r.get("kind_guess")?,
                        context: r.get("context")?,
                        confidence: r.get("confidence")?,
                        resolved_customer_id: r.get("resolved_customer_id")?,
                        created_at: r.get("created_at")?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    // ---- similarity + path + subgraph ----

    /// Score every OTHER customer against `id`, ranked by a deterministic
    /// similarity blend. Signals combined via unweighted sum:
    ///   1. Jaccard over tags
    ///   2. Same company (boolean)
    ///   3. Jaccard over 1-hop neighbours in the relationship graph
    ///   4. Extracted-mention overlap: names each side has mentioned
    /// Reasons are human-readable strings so the UI can render "vì cùng công ty
    /// Shop Co, cùng biết Tuấn Anh" without further work.
    pub fn similar_customers(&self, id: i64, limit: i64) -> Result<Vec<(Customer, f64, Vec<String>)>> {
        let focus = self.get_customer(id)?.ok_or_else(|| anyhow!("customer {id} not found"))?;
        let candidates = self.list_customers(None, None, None, 5000)?;
        let focus_neighbours = self.neighbour_ids(id)?;
        let focus_mentions = self.mention_names(id)?;
        let focus_tags: std::collections::BTreeSet<String> = focus.tags.iter().map(|t| t.to_lowercase()).collect();

        let mut scored: Vec<(Customer, f64, Vec<String>)> = Vec::new();
        for c in candidates {
            if c.id == id {
                continue;
            }
            let mut score = 0.0f64;
            let mut reasons: Vec<String> = Vec::new();

            let c_tags: std::collections::BTreeSet<String> = c.tags.iter().map(|t| t.to_lowercase()).collect();
            let tag_j = jaccard(&focus_tags, &c_tags);
            if tag_j > 0.0 {
                let shared: Vec<&String> = focus_tags.intersection(&c_tags).collect();
                score += tag_j * 1.5;
                reasons.push(format!(
                    "chung tag: {}",
                    shared.iter().take(3).map(|s| format!("#{}", s)).collect::<Vec<_>>().join(", ")
                ));
            }

            if !focus.company.is_empty() && focus.company.eq_ignore_ascii_case(&c.company) {
                score += 0.8;
                reasons.push(format!("cùng công ty \"{}\"", focus.company));
            }

            let c_neighbours = self.neighbour_ids(c.id).unwrap_or_default();
            let n_j = jaccard(&focus_neighbours, &c_neighbours);
            if n_j > 0.0 {
                let shared_ids: Vec<i64> = focus_neighbours.intersection(&c_neighbours).copied().collect();
                let shared_names = self.customer_names(&shared_ids).unwrap_or_default();
                score += n_j * 1.2;
                reasons.push(format!("cùng biết: {}", shared_names.iter().take(3).cloned().collect::<Vec<_>>().join(", ")));
            }

            let c_mentions = self.mention_names(c.id).unwrap_or_default();
            let m_shared: Vec<&String> = focus_mentions.intersection(&c_mentions).collect();
            if !m_shared.is_empty() {
                let m_j = m_shared.len() as f64 / focus_mentions.union(&c_mentions).count() as f64;
                score += m_j * 1.0;
                reasons.push(format!(
                    "trong ghi chú/tương tác cả 2 có nhắc: {}",
                    m_shared.iter().take(3).map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }

            // Direct relationship: if there's already an edge, that's a strong signal.
            if focus_neighbours.contains(&c.id) {
                score += 0.5;
                reasons.push("có quan hệ trực tiếp".to_string());
            }

            if score > 0.0 {
                scored.push((c, score, reasons));
            }
        }
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit.max(1).min(50) as usize);
        Ok(scored)
    }

    /// Set of every direct neighbour of `customer_id` in the relationship graph
    /// (undirected — collapses from/to distinction).
    fn neighbour_ids(&self, customer_id: i64) -> Result<std::collections::BTreeSet<i64>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT CASE WHEN from_id = ?1 THEN to_id ELSE from_id END
                 FROM relationships WHERE from_id = ?1 OR to_id = ?1",
            )?;
            let ids: std::collections::BTreeSet<i64> = stmt
                .query_map(params![customer_id], |r| r.get::<_, i64>(0))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(ids)
        })
    }

    /// Every extracted-mention name that customer has recorded (lowercased for
    /// case-insensitive intersection).
    fn mention_names(&self, customer_id: i64) -> Result<std::collections::BTreeSet<String>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT DISTINCT name FROM extracted_mentions WHERE source_customer_id = ?1")?;
            let names: std::collections::BTreeSet<String> = stmt
                .query_map(params![customer_id], |r| r.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .map(|n| n.to_lowercase())
                .collect();
            Ok(names)
        })
    }

    fn customer_names(&self, ids: &[i64]) -> Result<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with(|c| {
            let placeholders = ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
            let sql = format!("SELECT id, name FROM customers WHERE id IN ({placeholders})");
            let mut stmt = c.prepare(&sql)?;
            let params_vec: Vec<Box<dyn rusqlite::ToSql>> = ids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>).collect();
            let params_ref: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
            let mut by_id: std::collections::BTreeMap<i64, String> = stmt
                .query_map(params_ref.as_slice(), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .filter_map(|r| r.ok())
                .collect();
            // Preserve caller order.
            let mut out = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(n) = by_id.remove(id) {
                    out.push(n);
                }
            }
            Ok(out)
        })
    }

    /// BFS shortest path between two customers through the (undirected)
    /// relationship graph. Returns the customer-id path inclusive of both
    /// endpoints, or None when no route exists.
    pub fn find_path(&self, from: i64, to: i64) -> Result<Option<Vec<i64>>> {
        if from == to {
            return Ok(Some(vec![from]));
        }
        let all = self.all_relationships()?;
        let mut adj: std::collections::HashMap<i64, Vec<i64>> = std::collections::HashMap::new();
        for r in &all {
            adj.entry(r.from_id).or_default().push(r.to_id);
            adj.entry(r.to_id).or_default().push(r.from_id);
        }
        // Standard BFS.
        let mut parent: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
        let mut queue: std::collections::VecDeque<i64> = std::collections::VecDeque::new();
        queue.push_back(from);
        parent.insert(from, from);
        while let Some(cur) = queue.pop_front() {
            if cur == to {
                let mut path = vec![to];
                let mut c = to;
                while parent[&c] != c {
                    c = parent[&c];
                    path.push(c);
                }
                path.reverse();
                return Ok(Some(path));
            }
            if let Some(neigh) = adj.get(&cur) {
                for &n in neigh {
                    if !parent.contains_key(&n) {
                        parent.insert(n, cur);
                        queue.push_back(n);
                    }
                }
            }
        }
        Ok(None)
    }

    /// Every node reachable from `focus` within `hops` edges, plus every edge
    /// among those nodes. Used to build the "expand from focus" subgraph view.
    pub fn subgraph_within(&self, focus: i64, hops: i64) -> Result<(Vec<serde_json::Value>, Vec<Relationship>)> {
        let all = self.all_relationships()?;
        let mut adj: std::collections::HashMap<i64, Vec<i64>> = std::collections::HashMap::new();
        for r in &all {
            adj.entry(r.from_id).or_default().push(r.to_id);
            adj.entry(r.to_id).or_default().push(r.from_id);
        }
        let mut visited: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
        let mut frontier: std::collections::BTreeSet<i64> = std::collections::BTreeSet::from([focus]);
        visited.insert(focus);
        for _ in 0..hops.max(0) {
            let mut next: std::collections::BTreeSet<i64> = std::collections::BTreeSet::new();
            for v in &frontier {
                if let Some(neigh) = adj.get(v) {
                    for &n in neigh {
                        if !visited.contains(&n) {
                            next.insert(n);
                            visited.insert(n);
                        }
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            frontier = next;
        }
        let all_nodes = self.graph_nodes()?;
        let nodes: Vec<serde_json::Value> = all_nodes
            .into_iter()
            .filter(|n| n.get("id").and_then(|v| v.as_i64()).map(|id| visited.contains(&id)).unwrap_or(false))
            .collect();
        let edges: Vec<Relationship> = all
            .into_iter()
            .filter(|r| visited.contains(&r.from_id) && visited.contains(&r.to_id))
            .collect();
        Ok((nodes, edges))
    }

    // ---- ui state singleton ----

    // ---- customer channels (multi-phone + social) ----

    pub fn list_channels(&self, customer_id: i64) -> Result<Vec<CustomerChannel>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, customer_id, kind, value, label, created_at, updated_at
                 FROM customer_channels WHERE customer_id=?1 ORDER BY kind, id",
            )?;
            let rows = stmt
                .query_map(params![customer_id], |r| {
                    Ok(CustomerChannel {
                        id: r.get(0)?,
                        customer_id: r.get(1)?,
                        kind: r.get(2)?,
                        value: r.get(3)?,
                        label: r.get(4)?,
                        created_at: r.get(5)?,
                        updated_at: r.get(6)?,
                    })
                })?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn add_channel(&self, customer_id: i64, c: &ChannelCreate, now: i64) -> Result<i64> {
        if c.kind.trim().is_empty() {
            return Err(anyhow!("kind is required"));
        }
        if c.value.trim().is_empty() {
            return Err(anyhow!("value is required"));
        }
        let id = self.with(|conn| {
            if conn.query_row("SELECT 1 FROM customers WHERE id=?1", params![customer_id], |_| Ok(())).optional()?.is_none() {
                return Err(anyhow!("customer {customer_id} not found"));
            }
            conn.execute(
                "INSERT INTO customer_channels(customer_id, kind, value, label, created_at, updated_at)
                 VALUES(?1,?2,?3,?4,?5,?5)",
                params![customer_id, c.kind.trim(), c.value.trim(), c.label.trim(), now],
            )?;
            conn.execute("UPDATE customers SET updated_at=?2 WHERE id=?1", params![customer_id, now])?;
            Ok(conn.last_insert_rowid())
        })?;
        let _ = self.reindex_customer(customer_id);
        Ok(id)
    }

    pub fn update_channel(&self, id: i64, patch: &ChannelPatch, now: i64) -> Result<()> {
        let cid = self.with(|c| {
            let cid: Option<i64> = c
                .query_row("SELECT customer_id FROM customer_channels WHERE id=?1", params![id], |r| r.get(0))
                .optional()?;
            let Some(cid) = cid else {
                return Err(anyhow!("channel {id} not found"));
            };
            if let Some(v) = patch.kind.as_deref().map(str::trim) {
                if v.is_empty() { return Err(anyhow!("kind cannot be empty")); }
                c.execute("UPDATE customer_channels SET kind=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = patch.value.as_deref().map(str::trim) {
                if v.is_empty() { return Err(anyhow!("value cannot be empty")); }
                c.execute("UPDATE customer_channels SET value=?2 WHERE id=?1", params![id, v])?;
            }
            if let Some(v) = patch.label.as_deref() {
                c.execute("UPDATE customer_channels SET label=?2 WHERE id=?1", params![id, v.trim()])?;
            }
            c.execute("UPDATE customer_channels SET updated_at=?2 WHERE id=?1", params![id, now])?;
            c.execute("UPDATE customers SET updated_at=?2 WHERE id=?1", params![cid, now])?;
            Ok(cid)
        })?;
        let _ = self.reindex_customer(cid);
        Ok(())
    }

    pub fn delete_channel(&self, id: i64) -> Result<i64> {
        let cid = self.with(|c| {
            let cid: Option<i64> = c
                .query_row("SELECT customer_id FROM customer_channels WHERE id=?1", params![id], |r| r.get(0))
                .optional()?;
            let Some(cid) = cid else {
                return Err(anyhow!("channel {id} not found"));
            };
            c.execute("DELETE FROM customer_channels WHERE id=?1", params![id])?;
            Ok(cid)
        })?;
        let _ = self.reindex_customer(cid);
        Ok(cid)
    }

    /// Read a persisted view-state blob by key. Returns `null`-shaped Value
    /// when nothing has been stored yet so the client can hydrate to defaults.
    pub fn get_state(&self, key: &str) -> Result<Option<serde_json::Value>> {
        self.with(|c| {
            let row: Option<String> = c
                .query_row("SELECT value_json FROM crm_state WHERE key=?1", params![key], |r| r.get(0))
                .optional()?;
            match row {
                None => Ok(None),
                Some(s) => Ok(Some(serde_json::from_str(&s).unwrap_or(serde_json::Value::Null))),
            }
        })
    }

    pub fn set_state(&self, key: &str, value: &serde_json::Value, now: i64) -> Result<()> {
        let s = serde_json::to_string(value).unwrap_or_else(|_| "null".into());
        self.with(|c| {
            c.execute(
                "INSERT INTO crm_state(key, value_json, updated_at) VALUES(?1,?2,?3)
                 ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json, updated_at=excluded.updated_at",
                params![key, s, now],
            )?;
            Ok(())
        })
    }

    pub fn delete_state(&self, key: &str) -> Result<()> {
        self.with(|c| {
            c.execute("DELETE FROM crm_state WHERE key=?1", params![key])?;
            Ok(())
        })
    }

    /// Compact multi-line snapshot for LLM grounding: name / role / company /
    /// tags / notes / last few interaction summaries + last few extracted-mention
    /// names. Kept short so many customers fit in one prompt.
    pub fn compact_context(&self, id: i64) -> Result<String> {
        let c = self.get_customer(id)?.ok_or_else(|| anyhow!("customer {id} not found"))?;
        let interactions = self.list_interactions(id, 6)?;
        let mentions = self.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT name FROM extracted_mentions WHERE source_customer_id = ?1 ORDER BY confidence DESC, id DESC LIMIT 8",
            )?;
            let rows: Vec<String> = stmt
                .query_map(params![id], |r| r.get::<_, String>(0))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        }).unwrap_or_default();
        let mut lines = Vec::new();
        lines.push(format!("Name: {}", c.name));
        if !c.role.is_empty() {
            lines.push(format!("Role: {}", c.role));
        }
        if !c.company.is_empty() {
            lines.push(format!("Company: {}", c.company));
        }
        if !c.title.is_empty() {
            lines.push(format!("Title: {}", c.title));
        }
        if !c.address.is_empty() {
            lines.push(format!("Address: {}", c.address));
        }
        if !c.tags.is_empty() {
            lines.push(format!("Tags: {}", c.tags.join(", ")));
        }
        if !c.notes.trim().is_empty() {
            let n = c.notes.trim();
            lines.push(format!(
                "Notes: {}",
                if n.chars().count() > 300 { n.chars().take(300).collect::<String>() + "…" } else { n.to_string() }
            ));
        }
        if !interactions.is_empty() {
            let mut acts = Vec::new();
            for i in interactions.iter().take(4) {
                let d = if i.details.trim().is_empty() {
                    String::new()
                } else {
                    let dt = i.details.trim();
                    let dt = if dt.chars().count() > 120 { dt.chars().take(120).collect::<String>() + "…" } else { dt.to_string() };
                    format!(" — {dt}")
                };
                acts.push(format!("[{}] {}{}", i.kind, i.summary, d));
            }
            lines.push(format!("Recent activity: {}", acts.join(" | ")));
        }
        if !mentions.is_empty() {
            lines.push(format!("Mentioned in context: {}", mentions.join(", ")));
        }
        // Include extra contact channels — LLM can use these to reason about
        // "which social platforms is X active on".
        if let Ok(channels) = self.list_channels(id) {
            if !channels.is_empty() {
                let parts: Vec<String> = channels.iter()
                    .map(|ch| {
                        if ch.label.is_empty() {
                            format!("{}={}", ch.kind, ch.value)
                        } else {
                            format!("{}={} ({})", ch.kind, ch.value, ch.label)
                        }
                    })
                    .collect();
                lines.push(format!("Channels: {}", parts.join(", ")));
            }
        }
        Ok(lines.join("\n"))
    }

    /// Every customer as a CSV row (for the export button). Uses RFC 4180 quoting.
    pub fn export_customers_csv(&self) -> Result<String> {
        let rows = self.list_customers(None, None, None, 5000)?;
        let mut out = String::new();
        out.push_str("id,name,email,phone,company,title,role,tags,source,address,birthday,notes,created_at,updated_at\n");
        for r in rows {
            let tags = r.tags.join(";");
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                r.id,
                csv_field(&r.name),
                csv_field(&r.email),
                csv_field(&r.phone),
                csv_field(&r.company),
                csv_field(&r.title),
                csv_field(&r.role),
                csv_field(&tags),
                csv_field(&r.source),
                csv_field(&r.address),
                csv_field(&r.birthday),
                csv_field(&r.notes),
                r.created_at,
                r.updated_at,
            ));
        }
        Ok(out)
    }
}

/// Jaccard similarity |A ∩ B| / |A ∪ B|. 0 when both sets empty.
fn jaccard<T: Ord>(a: &std::collections::BTreeSet<T>, b: &std::collections::BTreeSet<T>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let inter = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        inter as f64 / union as f64
    }
}

/// CSV-quote a field per RFC 4180: wrap in `"…"` if it contains a comma, quote,
/// CR, or LF; embedded quotes are doubled.
fn csv_field(s: &str) -> String {
    if s.chars().any(|c| c == ',' || c == '"' || c == '\n' || c == '\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Parse a birthday into (month, day). Accepts `YYYY-MM-DD`, `MM-DD`, `M/D`,
/// `DD/MM/YYYY`. Returns None on anything else — the field is a free-form
/// string so we just skip what we can't interpret.
fn parse_month_day(s: &str) -> Option<(u32, u32)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let parts: Vec<&str> = if s.contains('-') {
        s.split('-').collect()
    } else if s.contains('/') {
        s.split('/').collect()
    } else {
        return None;
    };
    let nums: Vec<u32> = parts.iter().filter_map(|p| p.parse::<u32>().ok()).collect();
    let (mm, dd) = match nums.len() {
        2 => (nums[0], nums[1]),
        3 => {
            // YYYY-MM-DD or DD/MM/YYYY. Whichever component looks like a year
            // (> 1900) tells us the order.
            if nums[0] > 1900 {
                (nums[1], nums[2])
            } else if nums[2] > 1900 {
                (nums[1], nums[0])
            } else {
                (nums[0], nums[1])
            }
        }
        _ => return None,
    };
    if (1..=12).contains(&mm) && (1..=31).contains(&dd) {
        Some((mm, dd))
    } else {
        None
    }
}

/// Next Unix-seconds occurrence of (month, day) at or after `now`. Uses UTC
/// noon so DST/timezone shifts never cross the date boundary.
fn next_occurrence(now: i64, mm: u32, dd: u32) -> Option<i64> {
    let today = jd_to_ymd(now.div_euclid(86400) + 2440588);
    for year_off in 0..2 {
        let y = today.0 + year_off;
        let jd = ymd_to_jd(y, mm as i64, dd as i64)?;
        let ts = (jd - 2440588) * 86400 + 43200; // noon UTC
        if ts >= now {
            return Some(ts);
        }
    }
    None
}

/// (year, month, day) from a Julian Day Number.
fn jd_to_ymd(jd: i64) -> (i64, i64, i64) {
    let a = jd + 32044;
    let b = (4 * a + 3) / 146097;
    let c = a - (146097 * b) / 4;
    let d = (4 * c + 3) / 1461;
    let e = c - (1461 * d) / 4;
    let m = (5 * e + 2) / 153;
    let day = e - (153 * m + 2) / 5 + 1;
    let month = m + 3 - 12 * (m / 10);
    let year = 100 * b + d - 4800 + m / 10;
    (year, month, day)
}

/// Julian Day Number from (year, month, day). None on invalid dates.
fn ymd_to_jd(y: i64, m: i64, d: i64) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let a = (14 - m) / 12;
    let y2 = y + 4800 - a;
    let m2 = m + 12 * a - 3;
    Some(d + (153 * m2 + 2) / 5 + 365 * y2 + y2 / 4 - y2 / 100 + y2 / 400 - 32045)
}

/// Trim, drop empties, deduplicate (case-insensitive) — writers pass raw user
/// input and get back the canonical tag list stored on the row.
fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for t in tags {
        let t = t.trim();
        if t.is_empty() {
            continue;
        }
        let key = t.to_lowercase();
        if seen.insert(key) {
            out.push(t.to_string());
        }
    }
    out
}

/// Per-app data dir, e.g. `~/.senclaw/space-apps/crm/`.
pub fn default_data_dir(app: &str) -> PathBuf {
    let base = std::env::var("SENCLAW_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".senclaw")
        });
    base.join("space-apps").join(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now() -> i64 {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
    }

    fn tmp_db() -> Db {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crm.db");
        let db = Db::open(&path).unwrap();
        // Leak the tempdir so the file lives for the test.
        std::mem::forget(dir);
        db
    }

    #[test]
    fn create_get_update_delete() {
        let db = tmp_db();
        let id = db
            .create_customer(
                &CustomerCreate {
                    name: "Nguyễn Văn A".into(),
                    email: "a@example.com".into(),
                    phone: "0900".into(),
                    company: "Foo Co".into(),
                    tags: vec!["vip".into(), "  vip ".into(), "hà nội".into()],
                    ..Default::default()
                },
                now(),
            )
            .unwrap();
        assert!(id > 0);

        let c = db.get_customer(id).unwrap().unwrap();
        assert_eq!(c.name, "Nguyễn Văn A");
        assert_eq!(c.tags, vec!["vip".to_string(), "hà nội".to_string()]);
        assert_eq!(c.role, "lead");

        db.update_customer(
            id,
            &CustomerPatch {
                role: Some("customer".into()),
                notes: Some("prefers zalo".into()),
                tags: Some(vec!["vip".into(), "designer".into()]),
                ..Default::default()
            },
            now(),
        )
        .unwrap();
        let c = db.get_customer(id).unwrap().unwrap();
        assert_eq!(c.role, "customer");
        assert_eq!(c.notes, "prefers zalo");
        assert_eq!(c.tags, vec!["vip".to_string(), "designer".to_string()]);

        db.delete_customer(id).unwrap();
        assert!(db.get_customer(id).unwrap().is_none());
    }

    #[test]
    fn search_and_tag_filter() {
        let db = tmp_db();
        db.create_customer(
            &CustomerCreate {
                name: "Anna".into(),
                email: "anna@shop.vn".into(),
                tags: vec!["vip".into()],
                ..Default::default()
            },
            now(),
        )
        .unwrap();
        db.create_customer(
            &CustomerCreate {
                name: "Bob".into(),
                company: "Shop Co".into(),
                tags: vec!["lead".into()],
                ..Default::default()
            },
            now(),
        )
        .unwrap();
        let all = db.list_customers(None, None, None, 50).unwrap();
        assert_eq!(all.len(), 2);
        let shop = db.list_customers(Some("shop"), None, None, 50).unwrap();
        assert_eq!(shop.len(), 2);
        let vip = db.list_customers(None, Some("vip"), None, 50).unwrap();
        assert_eq!(vip.len(), 1);
        assert_eq!(vip[0].name, "Anna");
    }

    #[test]
    fn deals_lifecycle_and_stats() {
        let db = tmp_db();
        let cid = db
            .create_customer(&CustomerCreate { name: "Acme".into(), ..Default::default() }, now())
            .unwrap();
        let did = db
            .create_deal(
                &DealCreate {
                    customer_id: cid,
                    title: "Yearly plan".into(),
                    amount: 12_000_000.0,
                    currency: "VND".into(),
                    stage: "proposal".into(),
                    probability: Some(60),
                    ..Default::default()
                },
                now(),
            )
            .unwrap();
        let deals = db.list_deals(None).unwrap();
        assert_eq!(deals.len(), 1);
        assert_eq!(deals[0].customer_name, "Acme");
        assert_eq!(deals[0].probability, 60);

        // Move to won → stats reflect closed value.
        db.update_deal(
            did,
            &DealPatch { stage: Some("won".into()), ..Default::default() },
            now(),
        )
        .unwrap();
        let deals = db.list_deals(Some("won")).unwrap();
        assert_eq!(deals.len(), 1);
        assert!(deals[0].closed_at.is_some());
        let s = db.stats().unwrap();
        assert_eq!(s["won_value"].as_f64().unwrap(), 12_000_000.0);
        assert_eq!(s["open_deals"].as_i64().unwrap(), 0);
    }

    #[test]
    fn tasks_open_and_done() {
        let db = tmp_db();
        let cid = db.create_customer(&CustomerCreate { name: "Z".into(), ..Default::default() }, 1).unwrap();
        let t1 = db.create_task(&TaskCreate { customer_id: Some(cid), title: "Gọi khách".into(), due_at: Some(100), ..Default::default() }, 1).unwrap();
        db.create_task(&TaskCreate { title: "Việc chung không gắn khách".into(), ..Default::default() }, 1).unwrap();
        let open = db.list_tasks(true, 20).unwrap();
        assert_eq!(open.len(), 2);
        assert!(open[0].due_at.is_some());
        db.set_task_done(t1, true, 200).unwrap();
        assert_eq!(db.list_tasks(true, 20).unwrap().len(), 1);
        assert_eq!(db.list_tasks(false, 20).unwrap().len(), 2);
        let s = db.stats().unwrap();
        assert_eq!(s["open_tasks"].as_i64().unwrap(), 1);
    }

    #[test]
    fn upcoming_finds_birthdays() {
        let db = tmp_db();
        let (yy, mm, dd) = jd_to_ymd(now().div_euclid(86400) + 2440588);
        // A birthday exactly 3 days out (in the same or next year — both fine).
        let target = ymd_to_jd(yy, mm, dd + 3).unwrap();
        let target_ymd = jd_to_ymd(target);
        let bday = format!("1990-{:02}-{:02}", target_ymd.1, target_ymd.2);
        db.create_customer(
            &CustomerCreate { name: "Bday".into(), birthday: bday.clone(), ..Default::default() },
            now(),
        )
        .unwrap();
        let up = db.upcoming(now(), 14).unwrap();
        let list = up["birthdays"].as_array().unwrap();
        assert_eq!(list.len(), 1, "birthday {bday} should surface: {up}");
        assert_eq!(list[0]["birthday"].as_str().unwrap(), bday);
    }

    #[test]
    fn csv_export_quotes_commas() {
        let db = tmp_db();
        db.create_customer(
            &CustomerCreate {
                name: "Nguyen, A".into(),
                email: "a@x".into(),
                notes: "line1\nline2".into(),
                tags: vec!["a".into(), "b".into()],
                ..Default::default()
            },
            1,
        )
        .unwrap();
        let csv = db.export_customers_csv().unwrap();
        assert!(csv.starts_with("id,name,email"));
        assert!(csv.contains("\"Nguyen, A\""));
        // notes column contains an embedded newline → must be quoted
        assert!(csv.contains("\"line1\nline2\""));
        // tags joined by semicolons (safe, no quoting needed)
        assert!(csv.contains(",a;b,"));
    }

    #[test]
    fn relationships_directional_and_upsert() {
        let db = tmp_db();
        let a = db.create_customer(&CustomerCreate { name: "Anna".into(), ..Default::default() }, 1).unwrap();
        let b = db.create_customer(&CustomerCreate { name: "Bob".into(), ..Default::default() }, 1).unwrap();
        db.add_relationship(
            &RelationshipCreate {
                from_id: a,
                to_id: b,
                kind: "referred_by".into(),
                note: "Anna được Bob giới thiệu".into(),
                confidence: Some(0.9),
                source: "user".into(),
            },
            1,
        )
        .unwrap();
        let rels = db.relationships_of(a).unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].from_name, "Anna");
        assert_eq!(rels[0].to_name, "Bob");
        assert_eq!(rels[0].kind, "referred_by");
        // Upsert: same triple → updates confidence/note.
        db.add_relationship(
            &RelationshipCreate {
                from_id: a,
                to_id: b,
                kind: "referred_by".into(),
                note: "updated".into(),
                confidence: Some(0.5),
                source: "user".into(),
            },
            2,
        )
        .unwrap();
        let rels = db.relationships_of(a).unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].note, "updated");
        assert_eq!(rels[0].confidence, 0.5);
    }

    #[test]
    fn fts5_search_finds_across_customer_and_interactions() {
        let db = tmp_db();
        let id = db
            .create_customer(
                &CustomerCreate {
                    name: "Nguyễn Anna".into(),
                    company: "Shop Co".into(),
                    notes: "Thích cà phê arabica đậm".into(),
                    ..Default::default()
                },
                1,
            )
            .unwrap();
        db.add_interaction(id, "note", "Gửi mẫu arabica cho khách", "", 100, 100).unwrap();

        let hits = db.search("arabica", 20).unwrap();
        // Should find at least 2 entries — the customer notes and the interaction.
        assert!(hits.len() >= 2, "got {} hits: {:?}", hits.len(), hits.iter().map(|h| &h.title).collect::<Vec<_>>());
        assert!(hits.iter().any(|h| h.entity_type == "customer"));
        assert!(hits.iter().any(|h| h.entity_type == "interaction"));

        // Diacritic-fold: "khach" without tone marks still hits "khách".
        let hits = db.search("khach", 10).unwrap();
        assert!(hits.iter().any(|h| h.entity_type == "interaction"));
    }

    #[test]
    fn add_mention_materializes_relationship_when_resolved() {
        let db = tmp_db();
        let anna = db.create_customer(&CustomerCreate { name: "Anna".into(), ..Default::default() }, 1).unwrap();
        let tuan = db.create_customer(&CustomerCreate { name: "Tuấn Anh".into(), ..Default::default() }, 1).unwrap();
        db.add_mention(anna, "Tuấn Anh", "referrer", "referred_by", "Anna nói do anh Tuấn giới thiệu", 0.85, Some(tuan), 1)
            .unwrap();
        let rels = db.relationships_of(anna).unwrap();
        assert_eq!(rels.len(), 1);
        assert_eq!(rels[0].to_name, "Tuấn Anh");
        assert_eq!(rels[0].source, "ai");
    }

    #[test]
    fn similar_customers_scoring() {
        let db = tmp_db();
        // Anna & Bob share company "Shop Co" and tag "vip", plus both know Tuấn.
        let anna = db.create_customer(&CustomerCreate { name: "Anna".into(), company: "Shop Co".into(), tags: vec!["vip".into(), "hà nội".into()], ..Default::default() }, 1).unwrap();
        let bob = db.create_customer(&CustomerCreate { name: "Bob".into(), company: "Shop Co".into(), tags: vec!["vip".into()], ..Default::default() }, 1).unwrap();
        let tuan = db.create_customer(&CustomerCreate { name: "Tuấn".into(), ..Default::default() }, 1).unwrap();
        let alien = db.create_customer(&CustomerCreate { name: "Alien".into(), company: "Other".into(), ..Default::default() }, 1).unwrap();

        db.add_relationship(&RelationshipCreate { from_id: anna, to_id: tuan, kind: "referred_by".into(), ..Default::default() }, 1).unwrap();
        db.add_relationship(&RelationshipCreate { from_id: bob, to_id: tuan, kind: "referred_by".into(), ..Default::default() }, 1).unwrap();

        let ranked = db.similar_customers(anna, 5).unwrap();
        assert!(!ranked.is_empty());
        // Bob must rank first (shared company + tag + neighbour).
        assert_eq!(ranked[0].0.id, bob);
        assert!(ranked[0].1 > 0.5);
        assert!(ranked[0].2.iter().any(|r| r.contains("Shop Co")));
        assert!(ranked[0].2.iter().any(|r| r.contains("Tuấn")));
        // Alien should either be absent or score much lower than Bob.
        let alien_score = ranked.iter().find(|(c, _, _)| c.id == alien).map(|(_, s, _)| *s).unwrap_or(0.0);
        assert!(alien_score < ranked[0].1);
    }

    #[test]
    fn find_path_bfs_shortest() {
        let db = tmp_db();
        let a = db.create_customer(&CustomerCreate { name: "A".into(), ..Default::default() }, 1).unwrap();
        let b = db.create_customer(&CustomerCreate { name: "B".into(), ..Default::default() }, 1).unwrap();
        let c = db.create_customer(&CustomerCreate { name: "C".into(), ..Default::default() }, 1).unwrap();
        let d = db.create_customer(&CustomerCreate { name: "D".into(), ..Default::default() }, 1).unwrap();

        db.add_relationship(&RelationshipCreate { from_id: a, to_id: b, kind: "colleague_of".into(), ..Default::default() }, 1).unwrap();
        db.add_relationship(&RelationshipCreate { from_id: b, to_id: c, kind: "colleague_of".into(), ..Default::default() }, 1).unwrap();
        db.add_relationship(&RelationshipCreate { from_id: c, to_id: d, kind: "colleague_of".into(), ..Default::default() }, 1).unwrap();

        let path = db.find_path(a, d).unwrap().unwrap();
        assert_eq!(path, vec![a, b, c, d]);
        // Isolated node -> no path.
        let e = db.create_customer(&CustomerCreate { name: "E".into(), ..Default::default() }, 1).unwrap();
        assert!(db.find_path(a, e).unwrap().is_none());
    }

    #[test]
    fn subgraph_within_expands_hops() {
        let db = tmp_db();
        let a = db.create_customer(&CustomerCreate { name: "A".into(), ..Default::default() }, 1).unwrap();
        let b = db.create_customer(&CustomerCreate { name: "B".into(), ..Default::default() }, 1).unwrap();
        let c = db.create_customer(&CustomerCreate { name: "C".into(), ..Default::default() }, 1).unwrap();
        let d = db.create_customer(&CustomerCreate { name: "D".into(), ..Default::default() }, 1).unwrap();
        db.add_relationship(&RelationshipCreate { from_id: a, to_id: b, kind: "colleague_of".into(), ..Default::default() }, 1).unwrap();
        db.add_relationship(&RelationshipCreate { from_id: b, to_id: c, kind: "colleague_of".into(), ..Default::default() }, 1).unwrap();
        db.add_relationship(&RelationshipCreate { from_id: c, to_id: d, kind: "colleague_of".into(), ..Default::default() }, 1).unwrap();

        let (nodes1, _edges1) = db.subgraph_within(a, 1).unwrap();
        assert_eq!(nodes1.len(), 2); // A + B
        let (nodes2, edges2) = db.subgraph_within(a, 2).unwrap();
        assert_eq!(nodes2.len(), 3); // A + B + C
        // Only edges among visited nodes are returned.
        assert!(edges2.len() >= 2);
        let (nodes3, _) = db.subgraph_within(a, 3).unwrap();
        assert_eq!(nodes3.len(), 4); // all
    }

    #[test]
    fn interactions_roundtrip_and_touch() {
        let db = tmp_db();
        let id = db
            .create_customer(&CustomerCreate { name: "C".into(), ..Default::default() }, 1)
            .unwrap();
        db.add_interaction(id, "call", "Alo hỏi thăm", "", 100, 100).unwrap();
        db.add_interaction(id, "email", "Gửi báo giá", "chi tiết…", 200, 200).unwrap();
        let list = db.list_interactions(id, 10).unwrap();
        assert_eq!(list.len(), 2);
        // Ordered by occurred_at DESC.
        assert_eq!(list[0].kind, "email");
        assert_eq!(list[1].kind, "call");
        let c = db.get_customer(id).unwrap().unwrap();
        assert_eq!(c.interaction_count, 2);
        assert_eq!(c.last_interaction_at, Some(200));
    }
}
