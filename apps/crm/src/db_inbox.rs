//! The inbox: connected accounts, threads, and traffic.
//!
//! Two things called "channel" meet here, and keeping them apart matters:
//!   - `channels`          — OUR accounts. A Telegram bot token, a Zalo OA. We poll these.
//!   - `customer_channels` — THEIR identities. An email, a phone, a handle. Pre-existing.
//!
//! `resolve_customer` is the bridge: an inbound message arrives bearing a
//! platform-side `external_id`, and we look for a customer who has claimed that
//! identity. A hit links the thread to a person and every downstream feature
//! (guardrail rate limits, Customer 360, sales state) starts working. A miss
//! leaves `customer_id = 0` — an unlinked thread an operator can attach later,
//! rather than a silently-invented contact.

use anyhow::{anyhow, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::Db;

/// Placeholder the API swaps in for secrets on egress. A PATCH carrying it back
/// means "unchanged", which is what lets the UI re-save a form it never saw the
/// real token for.
pub const SECRET_MASK: &str = "••••••";

const SECRET_KEYS: &[&str] = &[
    "token",
    "access_token",
    "refresh_token",
    "app_secret",
    "api_key",
];

#[derive(Serialize, Clone)]
pub struct Channel {
    pub id: i64,
    pub kind: String,
    pub name: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub cursor: String,
    pub last_sync_at: Option<i64>,
    pub last_status: String,
    pub last_error: String,
    pub created_at: i64,
}

#[derive(Deserialize)]
pub struct ChannelInput {
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub config: serde_json::Value,
}

#[derive(Deserialize, Default)]
pub struct ChannelPatch {
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
    pub enabled: Option<bool>,
}

#[derive(Serialize, Clone)]
pub struct Conversation {
    pub id: i64,
    pub channel_id: i64,
    pub channel_kind: String,
    pub external_id: String,
    pub customer_id: i64,
    pub customer_name: String,
    pub customer_avatar: String,
    pub display_name: String,
    pub status: String,
    pub handoff_state: String,
    pub assignee: String,
    pub unread: i64,
    pub last_message_at: Option<i64>,
    pub created_at: i64,
    pub preview: String,
    pub message_count: i64,
}

#[derive(Serialize, Clone)]
pub struct ConvMessage {
    pub id: i64,
    pub conversation_id: i64,
    pub customer_id: i64,
    pub direction: String,
    pub role: String,
    pub content: String,
    pub channel: String,
    pub status: String,
    pub created_at: i64,
}

pub const HANDOFF_BOT: &str = "bot";
pub const HANDOFF_PENDING: &str = "pending";
pub const HANDOFF_OPERATOR: &str = "with_operator";

/// Replace secret-looking values with the mask. Applied on every egress path.
pub fn redact_config(config: &serde_json::Value) -> serde_json::Value {
    let mut out = config.clone();
    if let Some(obj) = out.as_object_mut() {
        for k in SECRET_KEYS {
            if let Some(v) = obj.get_mut(*k) {
                if v.as_str().map(|s| !s.is_empty()).unwrap_or(false) {
                    *v = serde_json::Value::String(SECRET_MASK.to_string());
                }
            }
        }
    }
    out
}

/// Merge an incoming config over the stored one, skipping any key whose value is
/// still the mask. Without this, saving a form that only displayed `••••••`
/// would overwrite the real token with the mask string.
pub fn merge_config(stored: &serde_json::Value, incoming: &serde_json::Value) -> serde_json::Value {
    let mut out = stored.clone();
    let (Some(obj), Some(inc)) = (out.as_object_mut(), incoming.as_object()) else {
        return incoming.clone();
    };
    for (k, v) in inc {
        if v.as_str() == Some(SECRET_MASK) {
            continue;
        }
        obj.insert(k.clone(), v.clone());
    }
    serde_json::Value::Object(obj.clone())
}

impl Db {
    // ---- channels (our connected accounts) ----

    pub fn list_channels_all(&self) -> Result<Vec<Channel>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM channels ORDER BY created_at")?;
            let rows = stmt
                .query_map([], Self::row_to_channel)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn enabled_channels(&self) -> Result<Vec<Channel>> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT * FROM channels WHERE enabled=1 ORDER BY id")?;
            let rows = stmt
                .query_map([], Self::row_to_channel)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn get_channel(&self, id: i64) -> Result<Option<Channel>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT * FROM channels WHERE id=?1",
                    params![id],
                    Self::row_to_channel,
                )
                .optional()?;
            Ok(row)
        })
    }

    /// First enabled channel of a kind. Lets callers say "send over Telegram"
    /// without knowing which account is wired up.
    pub fn channel_of_kind(&self, kind: &str) -> Result<Option<Channel>> {
        self.with(|c| {
            let row = c
                .query_row(
                    "SELECT * FROM channels WHERE kind=?1 AND enabled=1 ORDER BY id LIMIT 1",
                    params![kind],
                    Self::row_to_channel,
                )
                .optional()?;
            Ok(row)
        })
    }

    pub fn create_channel(&self, input: &ChannelInput, now: i64) -> Result<i64> {
        let kind = input.kind.trim().to_lowercase();
        if !crate::db::CHANNEL_KINDS.contains(&kind.as_str()) {
            return Err(anyhow!("unknown channel kind '{kind}'"));
        }
        let config = if input.config.is_null() {
            serde_json::json!({})
        } else {
            input.config.clone()
        };
        self.with(|c| {
            c.execute(
                "INSERT INTO channels(kind, name, config, enabled, created_at) VALUES(?1,?2,?3,1,?4)",
                params![kind, input.name.trim(), config.to_string(), now],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn update_channel_cfg(&self, id: i64, patch: &ChannelPatch) -> Result<()> {
        self.with(|c| {
            let stored: Option<String> = c
                .query_row(
                    "SELECT config FROM channels WHERE id=?1",
                    params![id],
                    |r| r.get(0),
                )
                .optional()?;
            let stored = stored.ok_or_else(|| anyhow!("channel {id} not found"))?;
            if let Some(v) = &patch.name {
                c.execute(
                    "UPDATE channels SET name=?2 WHERE id=?1",
                    params![id, v.trim()],
                )?;
            }
            if let Some(v) = &patch.config {
                let cur: serde_json::Value =
                    serde_json::from_str(&stored).unwrap_or(serde_json::json!({}));
                let merged = merge_config(&cur, v);
                c.execute(
                    "UPDATE channels SET config=?2 WHERE id=?1",
                    params![id, merged.to_string()],
                )?;
            }
            if let Some(v) = patch.enabled {
                c.execute(
                    "UPDATE channels SET enabled=?2 WHERE id=?1",
                    params![id, v as i64],
                )?;
            }
            Ok(())
        })
    }

    pub fn delete_channel_cfg(&self, id: i64) -> Result<()> {
        self.with(|c| {
            let n = c.execute("DELETE FROM channels WHERE id=?1", params![id])?;
            if n == 0 {
                return Err(anyhow!("channel {id} not found"));
            }
            Ok(())
        })
    }

    /// Record the outcome of a poll. `cursor` is `None` when the adapter has
    /// nothing new to remember, which keeps a failed poll from rewinding it.
    pub fn set_channel_sync(
        &self,
        id: i64,
        status: &str,
        error: &str,
        cursor: Option<&str>,
        now: i64,
    ) -> Result<()> {
        self.with(|c| {
            c.execute(
                "UPDATE channels SET last_status=?2, last_error=?3, last_sync_at=?4 WHERE id=?1",
                params![id, status, error, now],
            )?;
            if let Some(cur) = cursor {
                c.execute(
                    "UPDATE channels SET cursor=?2 WHERE id=?1",
                    params![id, cur],
                )?;
            }
            Ok(())
        })
    }

    // ---- identity resolution ----

    /// Find the customer who owns this platform identity.
    ///
    /// Tried in order: an exact `customer_channels` row for this kind, then any
    /// channel row with the same value regardless of kind (people paste the same
    /// phone under `phone` and `zalo`), then the customers table's own email and
    /// phone columns. Returns 0 when nobody claims it — never guesses.
    pub fn resolve_customer(&self, kind: &str, external_id: &str) -> Result<i64> {
        let value = external_id.trim();
        if value.is_empty() {
            return Ok(0);
        }
        self.with(|c| {
            let exact: Option<i64> = c
                .query_row(
                    "SELECT customer_id FROM customer_channels
                     WHERE kind=?1 AND LOWER(value)=LOWER(?2) LIMIT 1",
                    params![kind, value],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = exact {
                return Ok(id);
            }
            let any_kind: Option<i64> = c
                .query_row(
                    "SELECT customer_id FROM customer_channels WHERE LOWER(value)=LOWER(?1) LIMIT 1",
                    params![value],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = any_kind {
                return Ok(id);
            }
            let builtin: Option<i64> = c
                .query_row(
                    "SELECT id FROM customers
                     WHERE (email <> '' AND LOWER(email)=LOWER(?1))
                        OR (phone <> '' AND phone=?1)
                     LIMIT 1",
                    params![value],
                    |r| r.get(0),
                )
                .optional()?;
            Ok(builtin.unwrap_or(0))
        })
    }

    /// Attach a thread to a customer, and remember the identity on that customer
    /// so the next inbound resolves without a human in the loop.
    pub fn link_conversation(&self, conv_id: i64, customer_id: i64, now: i64) -> Result<()> {
        self.with(|c| {
            let conv: Option<(String, String)> = c
                .query_row(
                    "SELECT channel_kind, external_id FROM conversations WHERE id=?1",
                    params![conv_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let (kind, external_id) = conv.ok_or_else(|| anyhow!("conversation {conv_id} not found"))?;
            let ok: i64 = c.query_row(
                "SELECT COUNT(*) FROM customers WHERE id=?1",
                params![customer_id],
                |r| r.get(0),
            )?;
            if ok == 0 {
                return Err(anyhow!("customer {customer_id} not found"));
            }
            c.execute(
                "UPDATE conversations SET customer_id=?2 WHERE id=?1",
                params![conv_id, customer_id],
            )?;
            c.execute(
                "UPDATE conv_messages SET customer_id=?2 WHERE conversation_id=?1",
                params![conv_id, customer_id],
            )?;
            if !external_id.is_empty() {
                let dup: i64 = c.query_row(
                    "SELECT COUNT(*) FROM customer_channels
                     WHERE customer_id=?1 AND kind=?2 AND LOWER(value)=LOWER(?3)",
                    params![customer_id, kind, external_id],
                    |r| r.get(0),
                )?;
                if dup == 0 {
                    c.execute(
                        "INSERT INTO customer_channels(customer_id, kind, value, label, created_at, updated_at)
                         VALUES(?1,?2,?3,'inbox',?4,?4)",
                        params![customer_id, kind, external_id, now],
                    )?;
                }
            }
            Ok(())
        })?;
        let _ = self.reindex_customer(customer_id);
        Ok(())
    }

    // ---- conversations ----

    /// Idempotent on (channel_kind, external_id) — the UNIQUE index makes
    /// cold-start and reply collapse into one path. Resolves the customer on
    /// first sight.
    pub fn get_or_create_conversation(
        &self,
        channel_id: i64,
        kind: &str,
        external_id: &str,
        display_name: &str,
        now: i64,
    ) -> Result<Conversation> {
        let customer_id = self.resolve_customer(kind, external_id)?;
        self.with(|c| {
            c.execute(
                "INSERT OR IGNORE INTO conversations(channel_id, channel_kind, external_id,
                        customer_id, display_name, status, created_at)
                 VALUES(?1,?2,?3,?4,?5,'open',?6)",
                params![
                    channel_id,
                    kind,
                    external_id,
                    customer_id,
                    display_name.trim(),
                    now
                ],
            )?;
            // An existing thread may have been created before the person was in
            // the CRM; adopt the resolution now that one exists.
            if customer_id != 0 {
                c.execute(
                    "UPDATE conversations SET customer_id=?3
                     WHERE channel_kind=?1 AND external_id=?2 AND customer_id=0",
                    params![kind, external_id, customer_id],
                )?;
            }
            if !display_name.trim().is_empty() {
                c.execute(
                    "UPDATE conversations SET display_name=?3
                     WHERE channel_kind=?1 AND external_id=?2 AND display_name=''",
                    params![kind, external_id, display_name.trim()],
                )?;
            }
            Ok(())
        })?;
        self.conversation_by_external(kind, external_id)?
            .ok_or_else(|| anyhow!("failed to create conversation"))
    }

    pub fn conversation_by_external(
        &self,
        kind: &str,
        external_id: &str,
    ) -> Result<Option<Conversation>> {
        self.with(|c| {
            let row = c
                .query_row(
                    &format!("{CONV_SELECT} WHERE v.channel_kind=?1 AND v.external_id=?2"),
                    params![kind, external_id],
                    Self::row_to_conversation,
                )
                .optional()?;
            Ok(row)
        })
    }

    pub fn get_conversation(&self, id: i64) -> Result<Option<Conversation>> {
        self.with(|c| {
            let row = c
                .query_row(
                    &format!("{CONV_SELECT} WHERE v.id=?1"),
                    params![id],
                    Self::row_to_conversation,
                )
                .optional()?;
            Ok(row)
        })
    }

    pub fn list_conversations(
        &self,
        status: Option<&str>,
        kind: Option<&str>,
        customer_id: Option<i64>,
        q: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Conversation>> {
        self.with(|c| {
            let status = status.map(|s| s.trim()).filter(|s| !s.is_empty());
            let kind = kind.map(|s| s.trim()).filter(|s| !s.is_empty());
            let like = q
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| format!("%{}%", s.to_lowercase()));
            let sql = format!(
                "{CONV_SELECT}
                 WHERE (?1 IS NULL OR v.status = ?1)
                   AND (?2 IS NULL OR v.channel_kind = ?2)
                   AND (?3 IS NULL OR v.customer_id = ?3)
                   AND (?4 IS NULL OR LOWER(v.display_name) LIKE ?4
                        OR LOWER(COALESCE(cu.name,'')) LIKE ?4
                        OR LOWER(v.external_id) LIKE ?4)
                 ORDER BY COALESCE(v.last_message_at, v.created_at) DESC
                 LIMIT ?5"
            );
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt
                .query_map(
                    params![status, kind, customer_id, like, limit],
                    Self::row_to_conversation,
                )?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    pub fn set_conversation_status(&self, id: i64, status: &str) -> Result<()> {
        if !["open", "snoozed", "closed"].contains(&status) {
            return Err(anyhow!("unknown conversation status '{status}'"));
        }
        self.with(|c| {
            let n = c.execute(
                "UPDATE conversations SET status=?2 WHERE id=?1",
                params![id, status],
            )?;
            if n == 0 {
                return Err(anyhow!("conversation {id} not found"));
            }
            Ok(())
        })
    }

    pub fn set_handoff(&self, id: i64, state: &str) -> Result<()> {
        if ![HANDOFF_BOT, HANDOFF_PENDING, HANDOFF_OPERATOR].contains(&state) {
            return Err(anyhow!("unknown handoff state '{state}'"));
        }
        self.with(|c| {
            let n = c.execute(
                "UPDATE conversations SET handoff_state=?2 WHERE id=?1",
                params![id, state],
            )?;
            if n == 0 {
                return Err(anyhow!("conversation {id} not found"));
            }
            Ok(())
        })
    }

    pub fn mark_conversation_read(&self, id: i64) -> Result<()> {
        self.with(|c| {
            c.execute("UPDATE conversations SET unread=0 WHERE id=?1", params![id])?;
            Ok(())
        })
    }

    // ---- messages ----

    pub fn add_conv_message(
        &self,
        conversation_id: i64,
        direction: &str,
        role: &str,
        content: &str,
        status: &str,
        now: i64,
    ) -> Result<i64> {
        let id =
            self.with(|c| {
                let conv: Option<(i64, String)> = c
                    .query_row(
                        "SELECT customer_id, channel_kind FROM conversations WHERE id=?1",
                        params![conversation_id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?;
                let (customer_id, kind) =
                    conv.ok_or_else(|| anyhow!("conversation {conversation_id} not found"))?;
                c.execute(
                "INSERT INTO conv_messages(conversation_id, customer_id, direction, role, content,
                        channel, status, created_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![conversation_id, customer_id, direction, role, content, kind, status, now],
            )?;
                let id = c.last_insert_rowid();
                c.execute(
                    "UPDATE conversations SET last_message_at=?2,
                        unread = CASE WHEN ?3 = 'inbound' THEN unread + 1 ELSE unread END
                 WHERE id=?1",
                    params![conversation_id, now, direction],
                )?;
                Ok(id)
            })?;
        Ok(id)
    }

    pub fn list_conv_messages(&self, conversation_id: i64, limit: i64) -> Result<Vec<ConvMessage>> {
        self.with(|c| {
            // Newest-first in SQL for the LIMIT, then flipped so the caller gets
            // chronological order — the last N messages, not the first N.
            let mut stmt = c.prepare(
                "SELECT * FROM (
                     SELECT * FROM conv_messages WHERE conversation_id=?1
                     ORDER BY created_at DESC, id DESC LIMIT ?2
                 ) ORDER BY created_at, id",
            )?;
            let rows = stmt
                .query_map(params![conversation_id, limit], Self::row_to_conv_message)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Recent traffic with one person across every thread — the transcript the
    /// sales engine grounds a draft on.
    pub fn recent_messages_of_customer(
        &self,
        customer_id: i64,
        limit: i64,
    ) -> Result<Vec<ConvMessage>> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM (
                     SELECT * FROM conv_messages WHERE customer_id=?1
                     ORDER BY created_at DESC, id DESC LIMIT ?2
                 ) ORDER BY created_at, id",
            )?;
            let rows = stmt
                .query_map(params![customer_id, limit], Self::row_to_conv_message)?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows)
        })
    }

    /// Outbound messages actually delivered to this person in the last 24h.
    /// The rate-limit input for the guardrail — counts `sent` only, so drafts
    /// parked in the review queue don't burn the budget.
    pub fn count_outbound_24h(&self, customer_id: i64, now: i64) -> Result<i64> {
        self.with(|c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM conv_messages
                 WHERE customer_id=?1 AND direction='outbound' AND status='sent'
                   AND created_at >= ?2",
                params![customer_id, now - 86_400],
                |r| r.get(0),
            )?;
            Ok(n)
        })
    }

    pub fn count_inbound(&self, customer_id: i64) -> Result<i64> {
        self.with(|c| {
            let n: i64 = c.query_row(
                "SELECT COUNT(*) FROM conv_messages WHERE customer_id=?1 AND direction='inbound'",
                params![customer_id],
                |r| r.get(0),
            )?;
            Ok(n)
        })
    }

    pub fn inbox_stats(&self) -> Result<serde_json::Value> {
        self.with(|c| {
            let open: i64 = c.query_row(
                "SELECT COUNT(*) FROM conversations WHERE status='open'",
                [],
                |r| r.get(0),
            )?;
            let unread: i64 = c.query_row(
                "SELECT COUNT(*) FROM conversations WHERE unread > 0 AND status='open'",
                [],
                |r| r.get(0),
            )?;
            let waiting: i64 = c.query_row(
                "SELECT COUNT(*) FROM conversations WHERE handoff_state <> 'bot' AND status='open'",
                [],
                |r| r.get(0),
            )?;
            let unlinked: i64 = c.query_row(
                "SELECT COUNT(*) FROM conversations WHERE customer_id=0 AND status='open'",
                [],
                |r| r.get(0),
            )?;
            let channels: i64 =
                c.query_row("SELECT COUNT(*) FROM channels WHERE enabled=1", [], |r| {
                    r.get(0)
                })?;
            Ok(serde_json::json!({
                "openConversations": open,
                "unread": unread,
                "waitingOnHuman": waiting,
                "unlinked": unlinked,
                "connectedChannels": channels,
            }))
        })
    }

    // ---- row mappers ----

    fn row_to_channel(r: &rusqlite::Row) -> rusqlite::Result<Channel> {
        let cfg: String = r.get("config")?;
        Ok(Channel {
            id: r.get("id")?,
            kind: r.get("kind")?,
            name: r.get("name")?,
            config: serde_json::from_str(&cfg).unwrap_or(serde_json::json!({})),
            enabled: r.get::<_, i64>("enabled")? != 0,
            cursor: r.get("cursor")?,
            last_sync_at: r.get("last_sync_at")?,
            last_status: r.get("last_status")?,
            last_error: r.get("last_error")?,
            created_at: r.get("created_at")?,
        })
    }

    fn row_to_conversation(r: &rusqlite::Row) -> rusqlite::Result<Conversation> {
        Ok(Conversation {
            id: r.get("id")?,
            channel_id: r.get("channel_id")?,
            channel_kind: r.get("channel_kind")?,
            external_id: r.get("external_id")?,
            customer_id: r.get("customer_id")?,
            customer_name: r.get("customer_name")?,
            customer_avatar: r.get("customer_avatar")?,
            display_name: r.get("display_name")?,
            status: r.get("status")?,
            handoff_state: r.get("handoff_state")?,
            assignee: r.get("assignee")?,
            unread: r.get("unread")?,
            last_message_at: r.get("last_message_at")?,
            created_at: r.get("created_at")?,
            preview: r.get("preview")?,
            message_count: r.get("message_count")?,
        })
    }

    fn row_to_conv_message(r: &rusqlite::Row) -> rusqlite::Result<ConvMessage> {
        Ok(ConvMessage {
            id: r.get("id")?,
            conversation_id: r.get("conversation_id")?,
            customer_id: r.get("customer_id")?,
            direction: r.get("direction")?,
            role: r.get("role")?,
            content: r.get("content")?,
            channel: r.get("channel")?,
            status: r.get("status")?,
            created_at: r.get("created_at")?,
        })
    }
}

/// Shared projection so list/get/by-external all return the same shape. The
/// correlated subqueries are what let the list view render a thread row without
/// a second round trip per conversation.
const CONV_SELECT: &str = "
    SELECT v.*,
           COALESCE(cu.name, '')       AS customer_name,
           COALESCE(cu.avatar_url, '') AS customer_avatar,
           COALESCE((SELECT m.content FROM conv_messages m
                      WHERE m.conversation_id = v.id
                      ORDER BY m.created_at DESC, m.id DESC LIMIT 1), '') AS preview,
           (SELECT COUNT(*) FROM conv_messages m WHERE m.conversation_id = v.id) AS message_count
    FROM conversations v
    LEFT JOIN customers cu ON cu.id = v.customer_id
";
