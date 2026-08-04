//! Pushing a plan's sessions onto the SenClaw calendar.
//!
//! Each session becomes one `space_events` row whose `link` points back at this
//! app: `/space/app/study?session=<id>`. That link is the whole point of the
//! feature — tapping the event (or its reminder) opens today's lesson instead
//! of just naming it.
//!
//! Two properties this module has to hold:
//!
//! * **Idempotent.** Syncing twice must not double-book the calendar. A session
//!   remembers its `event_id` and updates that row instead of inserting again.
//! * **Honest about the user's edits.** If the user deleted the event by hand,
//!   the update fails; we recreate it, but we never resurrect events for
//!   sessions the user already completed.

use serde_json::{json, Value};
use std::time::Duration;

use crate::config;
use crate::db::Db;

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("build http client")
}

fn base() -> String {
    config::senclaw_base_url().trim_end_matches('/').to_string()
}

/// Deep link to a session inside this app.
pub fn session_link(session_id: &str) -> String {
    format!(
        "/space/app/{}?session={}",
        config::app_id(),
        urlencode(session_id)
    )
}

fn urlencode(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || "-_.~".contains(c) {
                vec![c]
            } else {
                format!("%{:02X}", c as u32 as u8).chars().collect()
            }
        })
        .collect()
}

/// Local wall-clock (`YYYY-MM-DD`, `HH:MM`) → Unix milliseconds in `tz`.
pub fn local_ms(date: &str, hm: &str, tz: &str) -> Option<i64> {
    use chrono::NaiveDate;
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    let mut it = hm.split(':');
    let h: u32 = it.next()?.parse().ok()?;
    let m: u32 = it.next().unwrap_or("0").parse().unwrap_or(0);
    let tz = crate::srs::parse_tz(tz);
    Some(crate::srs::local_instant(d, h.min(23), m.min(59), tz).timestamp_millis())
}

#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub created: usize,
    pub updated: usize,
    pub skipped_done: usize,
    pub failed: Vec<String>,
}

/// Create or update one calendar event per session of a plan.
///
/// `reminder_min` of `None` still notifies: the daemon's EventNotifier pings
/// every event at its start time regardless.
pub async fn sync_plan(
    db: &Db,
    plan_id: &str,
    reminder_min: Option<i64>,
) -> Result<SyncReport, String> {
    let plan = db
        .plan_get(plan_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "không tìm thấy kế hoạch".to_string())?;
    let tz = plan["tz"].as_str().unwrap_or("Asia/Ho_Chi_Minh");
    let plan_title = plan["title"].as_str().unwrap_or("Kế hoạch học");
    let sessions = db.sessions_of_plan(plan_id).map_err(|e| e.to_string())?;
    let total = sessions.len();

    let client = http();
    let mut rep = SyncReport::default();

    for s in sessions {
        let sid = s["id"].as_str().unwrap_or_default().to_string();
        if s["status"].as_str() == Some("done") {
            rep.skipped_done += 1;
            continue;
        }
        let date = s["date"].as_str().unwrap_or_default();
        let hm = s["startHm"].as_str().unwrap_or("20:00");
        let minutes = s["minutes"].as_i64().unwrap_or(30).max(5);
        let Some(start) = local_ms(date, hm, tz) else {
            rep.failed.push(format!("{date}: giờ không hợp lệ"));
            continue;
        };
        let end = start + minutes * 60_000;

        let ord = s["ord"].as_i64().unwrap_or(0) + 1;
        let title = format!("📚 Buổi {ord}/{total} · {}", s["title"].as_str().unwrap_or(""));
        let description = describe(&s, plan_title);

        let body = json!({
            "title": title,
            "start_at": start,
            "end_at": end,
            "description": description,
            "reminder_min": reminder_min,
            "color": "#7c5cff",
            "link": session_link(&sid),
            "app_id": config::app_id(),
        });

        let existing = s["eventId"].as_str().filter(|e| !e.is_empty());
        let mut created_now = false;

        let ok = match existing {
            Some(eid) => {
                let url = format!("{}/api/space/calendar/events/{eid}", base());
                match client.patch(&url).json(&body).send().await {
                    Ok(r) if r.status().is_success() => true,
                    // The user deleted it, or the daemon lost it — recreate.
                    Ok(_) | Err(_) => {
                        created_now = true;
                        match create_event(&client, &body).await {
                            Ok(id) => {
                                db.session_set_event(&sid, Some(&id))
                                    .map_err(|e| e.to_string())?;
                                true
                            }
                            Err(e) => {
                                rep.failed.push(format!("{date}: {e}"));
                                false
                            }
                        }
                    }
                }
            }
            None => {
                created_now = true;
                match create_event(&client, &body).await {
                    Ok(id) => {
                        db.session_set_event(&sid, Some(&id))
                            .map_err(|e| e.to_string())?;
                        true
                    }
                    Err(e) => {
                        rep.failed.push(format!("{date}: {e}"));
                        false
                    }
                }
            }
        };

        if ok {
            if created_now {
                rep.created += 1;
            } else {
                rep.updated += 1;
            }
        }
    }

    Ok(rep)
}

fn describe(session: &Value, plan_title: &str) -> String {
    let mut lines = vec![format!("Kế hoạch: {plan_title}")];
    if let Some(items) = session["items"].as_array() {
        for it in items {
            let kind = match it["kind"].as_str().unwrap_or("") {
                "read" => "Đọc",
                "flashcard" => "Thẻ",
                "review" => "Ôn",
                "quiz" => "Trắc nghiệm",
                "recall" => "Tự diễn giải",
                other => other,
            };
            let title = it["sectionTitle"].as_str().unwrap_or("");
            let parts = it["parts"].as_i64().unwrap_or(1);
            let suffix = if parts > 1 {
                format!(" (phần {}/{})", it["part"].as_i64().unwrap_or(1), parts)
            } else {
                String::new()
            };
            lines.push(format!(
                "• {kind}: {title}{suffix} — {} phút",
                it["estMinutes"].as_i64().unwrap_or(0)
            ));
        }
    }
    lines.join("\n")
}

async fn create_event(client: &reqwest::Client, body: &Value) -> Result<String, String> {
    let url = format!("{}/api/space/calendar/events", base());
    let resp = client
        .post(&url)
        .json(body)
        .send()
        .await
        .map_err(|e| format!("gọi calendar lỗi: {e}"))?;
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        // Surface the daemon's own message — the link validator's rejection
        // text is the useful part when a route is wrong.
        return Err(v
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or(&format!("calendar trả {status}"))
            .to_string());
    }
    v.get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "calendar không trả id sự kiện".to_string())
}

/// Remove every calendar event a plan created.
pub async fn unsync_plan(db: &Db, plan_id: &str) -> Result<usize, String> {
    let client = http();
    let mut n = 0;
    for s in db.sessions_of_plan(plan_id).map_err(|e| e.to_string())? {
        let Some(eid) = s["eventId"].as_str().filter(|e| !e.is_empty()) else {
            continue;
        };
        let url = format!("{}/api/space/calendar/events/{eid}", base());
        if client.delete(&url).send().await.is_ok() {
            n += 1;
        }
        if let Some(sid) = s["id"].as_str() {
            let _ = db.session_set_event(sid, None);
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_link_is_an_internal_space_app_route() {
        let l = session_link("abc-123");
        assert!(l.starts_with("/space/app/"));
        assert!(l.contains("session=abc-123"));
        assert!(!l.contains("://"), "must never be an absolute URL");
    }

    #[test]
    fn a_session_id_with_odd_characters_is_encoded() {
        let l = session_link("a b&c=d");
        assert!(!l.contains(' '));
        assert!(l.contains("%20") || l.contains("%26"));
    }

    #[test]
    fn local_wall_clock_converts_with_the_plans_timezone() {
        // 20:00 in Ho Chi Minh (UTC+7) is 13:00 UTC.
        let ms = local_ms("2026-08-03", "20:00", "Asia/Ho_Chi_Minh").unwrap();
        let dt = chrono::DateTime::from_timestamp_millis(ms).unwrap();
        assert_eq!(dt.format("%Y-%m-%d %H:%M").to_string(), "2026-08-03 13:00");
    }

    #[test]
    fn an_unparsable_date_or_time_is_none_not_a_wrong_instant() {
        assert!(local_ms("hôm nay", "20:00", "Asia/Ho_Chi_Minh").is_none());
        assert!(local_ms("2026-08-03", "xx", "Asia/Ho_Chi_Minh").is_none());
    }

    #[test]
    fn the_event_description_lists_what_the_session_contains() {
        let s = serde_json::json!({
            "items": [
                {"kind": "read", "sectionTitle": "Chương 1", "estMinutes": 20, "part": 1, "parts": 2},
                {"kind": "quiz", "sectionTitle": "Chương 1", "estMinutes": 5, "part": 1, "parts": 1},
            ]
        });
        let d = describe(&s, "Ôn thi");
        assert!(d.contains("Ôn thi"));
        assert!(d.contains("Đọc: Chương 1 (phần 1/2)"));
        assert!(d.contains("Trắc nghiệm"));
    }
}
