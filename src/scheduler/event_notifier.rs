//! Event notification loop.
//!
//! Polls `space_events` every `interval_sec` seconds and:
//! 1. Updates event `status` based on current time (upcoming → ongoing → done).
//! 2. Fires a reminder notification when `start_at - reminder_min*60s <= now`
//!    and `reminder_sent_at IS NULL`.
//! 3. Fires re-notifications every `renotify_min` minutes while `status = 'ongoing'`.
//!
//! Notifications are pushed via `WebSocketGateway` as `space:event:reminder` WS events
//! and logged to the console with local time.

use std::sync::Arc;
use std::time::Duration;

use rusqlite::params;
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::db::Db;
use crate::util::local_time::local_iso_string_now;

/// Callback seam so the notifier can push WS events without hard-coupling to
/// `WebSocketGateway` (keeps the scheduler crate independent).
pub trait EventNotifySink: Send + Sync + 'static {
    fn notify_event_reminder(&self, event_id: &str, title: &str, start_at_ms: i64, kind: &str);
}

/// No-op sink used when the WS gateway is not wired.
pub struct NoopEventSink;
impl EventNotifySink for NoopEventSink {
    fn notify_event_reminder(&self, _event_id: &str, _title: &str, _start_at_ms: i64, _kind: &str) {}
}

pub struct EventNotifier {
    db: Arc<Db>,
    sink: Arc<dyn EventNotifySink>,
    interval: Duration,
}

impl EventNotifier {
    pub fn new(db: Arc<Db>, sink: Arc<dyn EventNotifySink>, interval_sec: u64) -> Self {
        Self {
            db,
            sink,
            interval: Duration::from_secs(interval_sec.max(10)),
        }
    }

    pub fn start(self) -> JoinHandle<()> {
        let interval = self.interval;
        info!(interval_sec = interval.as_secs(), "[EventNotifier] started");
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                ticker.tick().await;
                if let Err(e) = self.tick() {
                    warn!(error = %e, "[EventNotifier] tick failed");
                }
            }
        })
    }

    pub fn tick(&self) -> anyhow::Result<()> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.db.with_conn(|conn| {
            // ── 1. Transition upcoming → ongoing / ongoing → done ────────────
            conn.execute(
                "UPDATE space_events
                 SET status = 'ongoing', updated_at = ?1
                 WHERE status = 'upcoming' AND deleted_at IS NULL
                   AND start_at <= ?1 AND end_at > ?1",
                params![now_ms],
            )?;
            conn.execute(
                "UPDATE space_events
                 SET status = 'done', updated_at = ?1
                 WHERE status IN ('upcoming','ongoing') AND deleted_at IS NULL
                   AND end_at <= ?1",
                params![now_ms],
            )?;

            // ── 2. Fire reminders ────────────────────────────────────────────
            // trigger_at = start_at - reminder_min * 60_000
            let mut stmt = conn.prepare(
                "SELECT id, title, start_at
                 FROM space_events
                 WHERE deleted_at IS NULL
                   AND reminder_min IS NOT NULL
                   AND reminder_sent_at IS NULL
                   AND status IN ('upcoming','ongoing')
                   AND (start_at - reminder_min * 60000) <= ?1",
            )?;
            let reminders: Vec<(String, String, i64)> = stmt
                .query_map(params![now_ms], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
                })?
                .filter_map(|r| r.ok())
                .collect();

            for (id, title, start_at) in reminders {
                info!(
                    "[EventNotifier] reminder → \"{}\" starts at {} (local: {})",
                    title,
                    start_at,
                    local_iso_string_now()
                );
                self.sink.notify_event_reminder(&id, &title, start_at, "reminder");
                conn.execute(
                    "UPDATE space_events SET reminder_sent_at = ?1, updated_at = ?1 WHERE id = ?2",
                    params![now_ms, id],
                )?;
            }

            // ── 3. Re-notifications for ongoing events ───────────────────────
            let mut stmt = conn.prepare(
                "SELECT id, title, start_at
                 FROM space_events
                 WHERE deleted_at IS NULL
                   AND status = 'ongoing'
                   AND renotify_min IS NOT NULL
                   AND (renotify_sent_at IS NULL
                        OR (?1 - renotify_sent_at) >= renotify_min * 60000)",
            )?;
            let renotifies: Vec<(String, String, i64)> = stmt
                .query_map(params![now_ms], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?))
                })?
                .filter_map(|r| r.ok())
                .collect();

            for (id, title, start_at) in renotifies {
                info!(
                    "[EventNotifier] re-notify → \"{}\" ongoing (local: {})",
                    title,
                    local_iso_string_now()
                );
                self.sink.notify_event_reminder(&id, &title, start_at, "renotify");
                conn.execute(
                    "UPDATE space_events SET renotify_sent_at = ?1, updated_at = ?1 WHERE id = ?2",
                    params![now_ms, id],
                )?;
            }

            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Recorder(Mutex<Vec<(String, String)>>);
    impl EventNotifySink for Recorder {
        fn notify_event_reminder(&self, event_id: &str, _title: &str, _start_at_ms: i64, kind: &str) {
            self.0.lock().unwrap().push((event_id.to_string(), kind.to_string()));
        }
    }

    fn insert_event(
        conn: &rusqlite::Connection,
        id: &str,
        title: &str,
        start_ms: i64,
        end_ms: i64,
        reminder_min: Option<i64>,
        renotify_min: Option<i64>,
    ) {
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO space_events
             (id, title, start_at, end_at, reminder_min, renotify_min, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?7)",
            params![id, title, start_ms, end_ms, reminder_min, renotify_min, now],
        )
        .unwrap();
    }

    #[test]
    fn reminder_fires_when_due() {
        let cfg = crate::config::Config::from_env();
        let db = Arc::new(Db::open_in_memory(&cfg).unwrap());
        let now_ms = chrono::Utc::now().timestamp_millis();
        // Event starts in 2 min, reminder = 5 min → should fire now
        db.with_conn(|conn| {
            insert_event(conn, "e1", "Meeting", now_ms + 2 * 60_000, now_ms + 62 * 60_000, Some(5), None);
            Ok(())
        })
        .unwrap();

        let rec = Arc::new(Recorder(Mutex::new(vec![])));
        let notifier = EventNotifier::new(db, rec.clone(), 60);
        notifier.tick().unwrap();

        let fired = rec.0.lock().unwrap().clone();
        assert!(fired.iter().any(|(id, kind)| id == "e1" && kind == "reminder"));
    }

    #[test]
    fn reminder_not_fired_twice() {
        let cfg = crate::config::Config::from_env();
        let db = Arc::new(Db::open_in_memory(&cfg).unwrap());
        let now_ms = chrono::Utc::now().timestamp_millis();
        db.with_conn(|conn| {
            insert_event(conn, "e2", "Standup", now_ms + 60_000, now_ms + 120_000, Some(10), None);
            Ok(())
        })
        .unwrap();

        let rec = Arc::new(Recorder(Mutex::new(vec![])));
        let notifier = EventNotifier::new(db, rec.clone(), 60);
        notifier.tick().unwrap();
        notifier.tick().unwrap();

        let count = rec.0.lock().unwrap().iter().filter(|(id, _)| id == "e2").count();
        assert_eq!(count, 1, "reminder should fire exactly once");
    }

    #[test]
    fn status_transitions_upcoming_to_done() {
        let cfg = crate::config::Config::from_env();
        let db = Arc::new(Db::open_in_memory(&cfg).unwrap());
        let now_ms = chrono::Utc::now().timestamp_millis();
        // Event already ended
        db.with_conn(|conn| {
            insert_event(conn, "e3", "Past", now_ms - 120_000, now_ms - 60_000, None, None);
            Ok(())
        })
        .unwrap();

        let notifier = EventNotifier::new(db.clone(), Arc::new(NoopEventSink), 60);
        notifier.tick().unwrap();

        let status: String = db
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT status FROM space_events WHERE id = 'e3'",
                    [],
                    |r| r.get(0),
                )
                .map_err(anyhow::Error::from)
            })
            .unwrap();
        assert_eq!(status, "done");
    }
}
