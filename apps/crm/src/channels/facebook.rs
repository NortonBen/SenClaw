//! Facebook Messenger adapter (Graph API v21.0, polling inbound + Send API
//! outbound). No webhooks. Config: `{ "page_id", "access_token" }` (a
//! long-lived Page access token). `cursor` stores the newest message time (ms).

use crate::channels::{http, now_ms, now_secs, Inbound};
use crate::db::Db;
use crate::db_inbox::Channel;
use serde_json::{json, Value};
use std::sync::Arc;

const GRAPH: &str = "https://graph.facebook.com/v21.0";

fn cfg<'a>(ch: &'a Channel, key: &str) -> &'a str {
    ch.config.get(key).and_then(|v| v.as_str()).unwrap_or("")
}

/// Days since the Unix epoch for a proleptic-Gregorian date. Hinnant's
/// `days_from_civil`: the era arithmetic is what makes leap years (including the
/// 100/400 rules) fall out without a lookup table.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m + 9) % 12; // March = 0
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Parse an all-ASCII run of digits.
fn num(b: &[u8]) -> Option<i64> {
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(b.iter().fold(0i64, |n, c| n * 10 + (c - b'0') as i64))
}

/// Parse a Graph RFC3339 timestamp into epoch millis (0 on failure).
///
/// Hand-rolled rather than pulling in a date library for one field: Graph emits
/// both `+0000` and `+00:00` offset forms, and this is the only date parsing the
/// CRM does. Byte-oriented throughout — the input is untrusted, so no slicing
/// that could split a multibyte char.
pub fn rfc3339_ms(s: &str) -> i64 {
    parse_rfc3339_ms(s).unwrap_or(0)
}

fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || b[13] != b':' || b[16] != b':' {
        return None;
    }
    if !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    let year = num(&b[0..4])?;
    let month = num(&b[5..7])?;
    let day = num(&b[8..10])?;
    let hour = num(&b[11..13])?;
    let min = num(&b[14..16])?;
    let sec = num(&b[17..19])?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // 60 is a leap second; Graph won't emit one, but rejecting it would be wrong.
    if hour > 23 || min > 59 || sec > 60 {
        return None;
    }

    let mut i = 19;
    // Optional fractional seconds — keep 3 digits, ignore any further precision.
    let mut frac_ms = 0i64;
    if i < b.len() && matches!(b[i], b'.' | b',') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
        let digits = &b[start..i];
        for k in 0..3 {
            frac_ms = frac_ms * 10 + digits.get(k).map(|c| (c - b'0') as i64).unwrap_or(0);
        }
    }

    // Offset: absent or Z means UTC; otherwise ±HH:MM / ±HHMM / ±HH.
    let offset_secs = match b.get(i) {
        None => 0,
        Some(b'Z') | Some(b'z') if i + 1 == b.len() => 0,
        Some(sign @ (b'+' | b'-')) => {
            let sign = if *sign == b'-' { -1 } else { 1 };
            let rest = &b[i + 1..];
            let (oh, om) = match rest.len() {
                2 => (num(rest)?, 0),
                4 => (num(&rest[0..2])?, num(&rest[2..4])?),
                5 if rest[2] == b':' => (num(&rest[0..2])?, num(&rest[3..5])?),
                _ => return None,
            };
            if oh > 23 || om > 59 {
                return None;
            }
            sign * (oh * 3600 + om * 60)
        }
        _ => return None,
    };

    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3600 + min * 60 + sec - offset_secs;
    Some(secs * 1000 + frac_ms)
}

/// Normalize one conversation's `messages` into inbound CUSTOMER messages newer
/// than `since_ms`. A message is from the customer when `from.id != page_id`.
/// Returns `(messages, newest_ms)`. Pure — unit-tested below.
pub fn normalize_messages(page_id: &str, list: &[Value], since_ms: i64) -> (Vec<Inbound>, i64) {
    let mut out = Vec::new();
    let mut newest = since_ms;
    for m in list {
        let t = rfc3339_ms(m.get("created_time").and_then(|x| x.as_str()).unwrap_or(""));
        newest = newest.max(t);
        let from_id = m["from"]["id"].as_str().unwrap_or("");
        let is_customer = !from_id.is_empty() && from_id != page_id;
        let text = m
            .get("message")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if is_customer && t > since_ms && !text.is_empty() {
            out.push(Inbound {
                external_id: from_id.to_string(),
                customer_name: m["from"]["name"].as_str().unwrap_or("").to_string(),
                text,
            });
        }
    }
    (out, newest)
}

pub async fn poll(db: &Arc<Db>, ch: &Channel) -> Result<Vec<Inbound>, String> {
    let page_id = cfg(ch, "page_id");
    let token = cfg(ch, "access_token");
    if page_id.is_empty() || token.is_empty() {
        return Err("kênh Facebook thiếu page_id/access_token".into());
    }
    // Cold start backfills a week rather than the whole history.
    let since: i64 = ch
        .cursor
        .parse()
        .unwrap_or_else(|_| now_ms() - 7 * 24 * 3600 * 1000);

    let convs: Value = http()
        .get(format!("{GRAPH}/{page_id}/conversations"))
        .query(&[
            ("fields", "id,updated_time"),
            ("limit", "25"),
            ("access_token", token),
        ])
        .send()
        .await
        .map_err(|e| format!("facebook conversations lỗi: {e}"))?
        .json()
        .await
        .map_err(|e| format!("facebook phản hồi lỗi: {e}"))?;
    if let Some(err) = convs.get("error") {
        return Err(format!(
            "facebook lỗi: {}",
            err["message"].as_str().unwrap_or("")
        ));
    }

    let mut out = Vec::new();
    let mut newest = since;
    for conv in convs["data"]
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .take(25)
    {
        let Some(conv_id) = conv["id"].as_str() else {
            continue;
        };
        let msgs: Value = http()
            .get(format!("{GRAPH}/{conv_id}/messages"))
            .query(&[
                ("fields", "id,message,from,created_time"),
                ("limit", "25"),
                ("access_token", token),
            ])
            .send()
            .await
            .map_err(|e| format!("facebook messages lỗi: {e}"))?
            .json()
            .await
            .map_err(|e| format!("facebook phản hồi lỗi: {e}"))?;
        let (m, max_ms) = normalize_messages(
            page_id,
            msgs["data"].as_array().unwrap_or(&Vec::new()),
            since,
        );
        newest = newest.max(max_ms);
        out.extend(m);
    }
    if newest > since {
        let _ = db.set_channel_sync(ch.id, "ok", "", Some(&newest.to_string()), now_secs());
    }
    Ok(out)
}

pub async fn send(
    _db: &Arc<Db>,
    ch: &Channel,
    external_id: &str,
    text: &str,
) -> Result<(), String> {
    let page_id = cfg(ch, "page_id");
    let token = cfg(ch, "access_token");
    if page_id.is_empty() || token.is_empty() {
        return Err("kênh Facebook thiếu page_id/access_token".into());
    }
    let resp = http()
        .post(format!("{GRAPH}/{page_id}/messages"))
        .query(&[("access_token", token)])
        .json(&json!({
            "recipient": { "id": external_id },
            "messaging_type": "RESPONSE",
            "message": { "text": text },
        }))
        .send()
        .await
        .map_err(|e| format!("facebook gửi lỗi: {e}"))?;
    let v: Value = resp
        .json()
        .await
        .map_err(|e| format!("facebook phản hồi lỗi: {e}"))?;
    if let Some(err) = v.get("error") {
        return Err(format!(
            "facebook từ chối gửi: {}",
            err["message"].as_str().unwrap_or("")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rfc3339() {
        assert!(
            rfc3339_ms("2026-07-16T09:30:00+0000") > 0
                || rfc3339_ms("2026-07-16T09:30:00+00:00") > 0
        );
        assert_eq!(rfc3339_ms("not-a-date"), 0);
    }

    #[test]
    fn parses_known_instants_exactly() {
        // Anchors verified against the epoch definition itself.
        assert_eq!(rfc3339_ms("1970-01-01T00:00:00Z"), 0);
        assert_eq!(rfc3339_ms("1970-01-01T00:00:01Z"), 1_000);
        // 2026-07-16T09:30:00Z == 1784194200 (leap years since 1970 included).
        assert_eq!(rfc3339_ms("2026-07-16T09:30:00Z"), 1_784_194_200_000);
        // A leap day must not shift the arithmetic.
        assert_eq!(rfc3339_ms("2024-02-29T00:00:00Z"), 1_709_164_800_000);
    }

    #[test]
    fn both_graph_offset_forms_agree() {
        // Graph emits `+0000`; RFC3339 proper wants `+00:00`. Same instant.
        let a = rfc3339_ms("2026-07-16T09:30:00+0000");
        let b = rfc3339_ms("2026-07-16T09:30:00+00:00");
        let z = rfc3339_ms("2026-07-16T09:30:00Z");
        assert_eq!(a, z);
        assert_eq!(b, z);
        // A real offset shifts the instant the other way.
        assert_eq!(rfc3339_ms("2026-07-16T16:30:00+07:00"), z);
        assert_eq!(rfc3339_ms("2026-07-16T04:30:00-0500"), z);
    }

    #[test]
    fn parses_fractional_seconds() {
        let base = rfc3339_ms("2026-07-16T09:30:00Z");
        assert_eq!(rfc3339_ms("2026-07-16T09:30:00.250Z"), base + 250);
        assert_eq!(rfc3339_ms("2026-07-16T09:30:00.5Z"), base + 500);
        // Sub-millisecond precision is truncated, not rejected.
        assert_eq!(rfc3339_ms("2026-07-16T09:30:00.123456Z"), base + 123);
    }

    #[test]
    fn rejects_junk_without_panicking() {
        // Chief risk here is slicing a multibyte char: every input must return 0.
        for junk in [
            "",
            "2026",
            "2026-07-16",
            "2026/07/16T09:30:00Z",
            "2026-13-16T09:30:00Z", // month 13
            "2026-07-16T25:30:00Z", // hour 25
            "2026-07-16T09:30:00+99:00",
            "2026-07-16T09:30:00.Z", // empty fraction
            "20xx-07-16T09:30:00Z",
            "chào anh, hôm nay thế nào ế ế ế",
            "🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂",
        ] {
            assert_eq!(rfc3339_ms(junk), 0, "must reject {junk:?}");
        }
    }

    #[test]
    fn customer_vs_page_direction() {
        let list = vec![
            json!({ "message": "hỏi", "from": { "id": "PSID1", "name": "Lan" }, "created_time": "2026-07-16T10:00:00+00:00" }),
            json!({ "message": "page trả lời", "from": { "id": "PAGE", "name": "Shop" }, "created_time": "2026-07-16T10:01:00+00:00" }),
        ];
        let (msgs, _newest) = normalize_messages("PAGE", &list, 0);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].external_id, "PSID1");
        assert_eq!(msgs[0].text, "hỏi");
    }

    #[test]
    fn page_messages_still_advance_the_cursor() {
        // Our own reply must move the cursor even though it yields no inbound.
        let list = vec![
            json!({ "message": "page trả lời", "from": { "id": "PAGE", "name": "Shop" }, "created_time": "2026-07-16T10:01:00+00:00" }),
        ];
        let (msgs, newest) = normalize_messages("PAGE", &list, 0);
        assert!(msgs.is_empty());
        assert_eq!(newest, rfc3339_ms("2026-07-16T10:01:00Z"));
    }
}
