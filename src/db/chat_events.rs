//! Persistence for ephemeral chat events (agent state transitions,
//! permission and question requests + their resolutions). Replayed on
//! subscribe via a `chat:history` frame so the Web UI can rebuild
//! mid-flight interactions after a page reload.

use anyhow::Result;
use rusqlite::params;

#[derive(Debug, Clone)]
pub struct StoredChatEvent {
    pub id: i64,
    pub chat_jid: String,
    pub event_type: String,
    pub request_id: Option<String>,
    pub payload_json: String,
    pub timestamp: String,
}

impl super::Db {
    pub fn insert_chat_event(
        &self,
        chat_jid: &str,
        event_type: &str,
        request_id: Option<&str>,
        payload_json: &str,
        timestamp: &str,
    ) -> Result<()> {
        self.with_conn(|c| {
            c.execute(
                r#"
                INSERT INTO chat_events (chat_jid, event_type, request_id, payload, timestamp)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![chat_jid, event_type, request_id, payload_json, timestamp],
            )?;
            Ok(())
        })
    }

    /// Fetch chat events for a chat in chronological order. When `limit` is
    /// set, returns the most recent N rows but in ascending order so the
    /// caller can replay them in time order.
    pub fn get_chat_events(
        &self,
        chat_jid: &str,
        limit: Option<u32>,
    ) -> Result<Vec<StoredChatEvent>> {
        self.with_conn(|c| {
            let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<StoredChatEvent> {
                Ok(StoredChatEvent {
                    id: r.get(0)?,
                    chat_jid: r.get(1)?,
                    event_type: r.get(2)?,
                    request_id: r.get(3)?,
                    payload_json: r.get(4)?,
                    timestamp: r.get(5)?,
                })
            };
            let rows: Vec<StoredChatEvent> = if let Some(lim) = limit {
                let mut stmt = c.prepare(
                    "SELECT id, chat_jid, event_type, request_id, payload, timestamp
                     FROM chat_events
                     WHERE chat_jid = ?1
                     ORDER BY timestamp DESC, id DESC
                     LIMIT ?2",
                )?;
                let mut v: Vec<StoredChatEvent> = stmt
                    .query_map(params![chat_jid, lim as i64], map_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                v.reverse();
                v
            } else {
                let mut stmt = c.prepare(
                    "SELECT id, chat_jid, event_type, request_id, payload, timestamp
                     FROM chat_events
                     WHERE chat_jid = ?1
                     ORDER BY timestamp ASC, id ASC",
                )?;
                let v: Vec<StoredChatEvent> = stmt
                    .query_map(params![chat_jid], map_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                v
            };
            Ok(rows)
        })
    }

    pub fn delete_chat_events_for_jid(&self, chat_jid: &str) -> Result<usize> {
        self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM chat_events WHERE chat_jid = ?1",
                params![chat_jid],
            )?)
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::db::Db;

    #[test]
    fn round_trip_preserves_order_and_request_id() {
        let cfg = Config::from_env();
        let db = Db::open_in_memory(&cfg).expect("open db");

        let jid = "tg:1";
        db.insert_chat_event(jid, "agent:state", None, r#"{"state":"idle"}"#, "2026-05-19T10:00:00Z").unwrap();
        db.insert_chat_event(jid, "permission:request", Some("req-1"), r#"{"toolName":"Bash"}"#, "2026-05-19T10:00:01Z").unwrap();
        db.insert_chat_event(jid, "permission:resolved", Some("req-1"), r#"{"key":"allow"}"#, "2026-05-19T10:00:02Z").unwrap();

        let rows = db.get_chat_events(jid, None).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].event_type, "agent:state");
        assert_eq!(rows[1].event_type, "permission:request");
        assert_eq!(rows[1].request_id.as_deref(), Some("req-1"));
        assert_eq!(rows[2].event_type, "permission:resolved");

        // Limit returns most-recent N but in ascending order.
        let recent = db.get_chat_events(jid, Some(2)).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].event_type, "permission:request");
        assert_eq!(recent[1].event_type, "permission:resolved");

        // Per-jid isolation.
        assert!(db.get_chat_events("tg:other", None).unwrap().is_empty());

        // Delete.
        assert_eq!(db.delete_chat_events_for_jid(jid).unwrap(), 3);
        assert!(db.get_chat_events(jid, None).unwrap().is_empty());
    }
}
