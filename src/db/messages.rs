use anyhow::Result;
use rusqlite::{params, OptionalExtension};

use crate::types::StoredMessage;

use super::rows::row_to_message;

impl super::Db {
    // ============================================================
    // Messages (channel_messages + group_messages)
    // ============================================================

    /// Insert a message and FIFO-trim the chat to its retention limit.
    pub fn insert_message(&self, msg: &StoredMessage, default_limit: u32) -> Result<()> {
        self.with_conn(|c| {
            let limit: i64 = c
                .query_row(
                    "SELECT max_messages FROM groups WHERE jid = ?1",
                    params![msg.chat_jid],
                    |r| r.get::<_, Option<i64>>(0),
                )
                .optional()?
                .flatten()
                .unwrap_or(default_limit as i64);

            c.execute(
                r#"
                INSERT OR IGNORE INTO channel_messages
                  (message_id, chat_jid, sender_jid, sender_name, content,
                   timestamp, is_from_me, is_bot_reply, reply_to_id, media_type)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                "#,
                params![
                    msg.message_id,
                    msg.chat_jid,
                    msg.sender_jid,
                    msg.sender_name,
                    msg.content,
                    msg.timestamp,
                    msg.is_from_me as i64,
                    msg.is_bot_reply as i64,
                    msg.reply_to_id,
                    msg.media_type,
                ],
            )?;

            c.execute(
                r#"
                DELETE FROM channel_messages
                WHERE chat_jid = ?1
                  AND message_id NOT IN (
                    SELECT message_id FROM channel_messages
                    WHERE chat_jid = ?1
                    ORDER BY timestamp DESC
                    LIMIT ?2
                  )
                "#,
                params![msg.chat_jid, limit],
            )?;
            Ok(())
        })
    }

    pub fn get_messages(&self, chat_jid: &str, since: Option<&str>) -> Result<Vec<StoredMessage>> {
        self.with_conn(|c| {
            let rows: Vec<rusqlite::Result<Result<StoredMessage>>> = if let Some(since) = since {
                let mut stmt = c.prepare(
                    "SELECT * FROM channel_messages
                     WHERE chat_jid = ?1 AND timestamp > ?2
                     ORDER BY timestamp ASC",
                )?;
                let v: Vec<_> = stmt
                    .query_map(params![chat_jid, since], |r| Ok(row_to_message(r)))?
                    .collect();
                v
            } else {
                let mut stmt = c.prepare(
                    "SELECT * FROM channel_messages
                     WHERE chat_jid = ?1
                     ORDER BY timestamp ASC",
                )?;
                let v: Vec<_> = stmt
                    .query_map(params![chat_jid], |r| Ok(row_to_message(r)))?
                    .collect();
                v
            };
            rows.into_iter()
                .map(|r| r.map_err(anyhow::Error::from).and_then(|inner| inner))
                .collect()
        })
    }

    pub fn get_messages_paginated(
        &self,
        chat_jid: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<StoredMessage>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM channel_messages
                 WHERE chat_jid = ?1
                 ORDER BY timestamp DESC
                 LIMIT ?2 OFFSET ?3",
            )?;
            let rows = stmt
                .query_map(params![chat_jid, limit as i64, offset as i64], |r| {
                    Ok(row_to_message(r))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let mut results = Vec::new();
            for r in rows {
                results.push(r?);
            }
            Ok(results)
        })
    }

    /// Delete all messages for a chat JID.
    pub fn delete_messages_for_jid(&self, chat_jid: &str) -> Result<usize> {
        self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM channel_messages WHERE chat_jid = ?1",
                params![chat_jid],
            )?)
        })
    }

    pub fn count_messages(&self, chat_jid: &str) -> Result<usize> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM channel_messages WHERE chat_jid = ?1",
                params![chat_jid],
                |r| r.get::<_, usize>(0),
            )?)
        })
    }

    // ============================================================
    // Group messages (conversation history: user + bot responses)
    // ============================================================

    pub fn insert_group_message(&self, msg: &StoredMessage, default_limit: u32) -> Result<()> {
        self.with_conn(|c| {
            let limit: i64 = c
                .query_row(
                    "SELECT max_messages FROM groups WHERE jid = ?1",
                    params![msg.chat_jid],
                    |r| r.get::<_, Option<i64>>(0),
                )
                .optional()?
                .flatten()
                .unwrap_or(default_limit as i64);

            c.execute(
                r#"
                INSERT OR IGNORE INTO group_messages
                  (message_id, chat_jid, sender_jid, sender_name, content,
                   timestamp, is_from_me, is_bot_reply, reply_to_id, media_type)
                VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                "#,
                params![
                    msg.message_id,
                    msg.chat_jid,
                    msg.sender_jid,
                    msg.sender_name,
                    msg.content,
                    msg.timestamp,
                    msg.is_from_me as i64,
                    msg.is_bot_reply as i64,
                    msg.reply_to_id,
                    msg.media_type,
                ],
            )?;

            c.execute(
                r#"
                DELETE FROM group_messages
                WHERE chat_jid = ?1
                  AND message_id NOT IN (
                    SELECT message_id FROM group_messages
                    WHERE chat_jid = ?1
                    ORDER BY timestamp DESC
                    LIMIT ?2
                  )
                "#,
                params![msg.chat_jid, limit],
            )?;
            Ok(())
        })
    }

    pub fn get_group_messages(&self, chat_jid: &str, since: Option<&str>) -> Result<Vec<StoredMessage>> {
        self.with_conn(|c| {
            let rows: Vec<rusqlite::Result<Result<StoredMessage>>> = if let Some(since) = since {
                let mut stmt = c.prepare(
                    "SELECT * FROM group_messages
                     WHERE chat_jid = ?1 AND timestamp > ?2
                     ORDER BY timestamp ASC",
                )?;
                let v: Vec<_> = stmt
                    .query_map(params![chat_jid, since], |r| Ok(row_to_message(r)))?
                    .collect();
                v
            } else {
                let mut stmt = c.prepare(
                    "SELECT * FROM group_messages
                     WHERE chat_jid = ?1
                     ORDER BY timestamp ASC",
                )?;
                let v: Vec<_> = stmt
                    .query_map(params![chat_jid], |r| Ok(row_to_message(r)))?
                    .collect();
                v
            };
            rows.into_iter()
                .map(|r| r.map_err(anyhow::Error::from).and_then(|inner| inner))
                .collect()
        })
    }

    pub fn get_group_messages_paginated(
        &self,
        chat_jid: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<StoredMessage>> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT * FROM group_messages
                 WHERE chat_jid = ?1
                 ORDER BY timestamp DESC
                 LIMIT ?2 OFFSET ?3",
            )?;
            let rows = stmt
                .query_map(params![chat_jid, limit as i64, offset as i64], |r| {
                    Ok(row_to_message(r))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            let mut results = Vec::new();
            for r in rows {
                results.push(r?);
            }
            Ok(results)
        })
    }

    pub fn delete_group_messages_for_jid(&self, chat_jid: &str) -> Result<usize> {
        self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM group_messages WHERE chat_jid = ?1",
                params![chat_jid],
            )?)
        })
    }

    pub fn count_group_messages(&self, chat_jid: &str) -> Result<usize> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM group_messages WHERE chat_jid = ?1",
                params![chat_jid],
                |r| r.get::<_, usize>(0),
            )?)
        })
    }
}
