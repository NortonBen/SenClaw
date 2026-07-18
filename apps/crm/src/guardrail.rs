//! Guardrails for proactive outbound. Enforced in Rust, fail-closed.
//!
//! `sale::send` is the ONLY path to a customer's inbox, and it calls `gate()`
//! before anything leaves. The agent is never handed a raw channel send, so
//! these rules cannot be talked around by a clever prompt.
//!
//! Order of checks — first match wins:
//!   1. unsubscribed   → Blocked. Never send, never queue. There is no override.
//!   2. rate limit 24h → Review. Too many touches already landed today.
//!   3. risky keywords → Review. Reply: ≥1 keyword. Broadcast: ≥2 (a proactive
//!      message that merely mentions "giá" in passing is less alarming than a
//!      direct reply about it, so the broadcast bar is higher).
//!
//! Complaint detection runs separately, in `sale::process_inbound`, against the
//! CUSTOMER's text — it escalates to a human before any draft is written.

use crate::db::Db;
use std::sync::Arc;

/// Lowercase and strip Vietnamese tone marks, so "giá" matches "gia" and a
/// keyword list stays robust against however the customer types. Mirrors the
/// FTS5 `remove_diacritics 2` tokenizer the rest of the CRM searches with.
pub fn fold(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ả' | 'ã' | 'ạ' | 'ă' | 'ắ' | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' | 'â' | 'ấ' | 'ầ'
            | 'ẩ' | 'ẫ' | 'ậ' => 'a',
            'é' | 'è' | 'ẻ' | 'ẽ' | 'ẹ' | 'ê' | 'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' => 'e',
            'í' | 'ì' | 'ỉ' | 'ĩ' | 'ị' => 'i',
            'ó' | 'ò' | 'ỏ' | 'õ' | 'ọ' | 'ô' | 'ố' | 'ồ' | 'ổ' | 'ỗ' | 'ộ' | 'ơ' | 'ớ' | 'ờ'
            | 'ở' | 'ỡ' | 'ợ' => 'o',
            'ú' | 'ù' | 'ủ' | 'ũ' | 'ụ' | 'ư' | 'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự' => 'u',
            'ý' | 'ỳ' | 'ỷ' | 'ỹ' | 'ỵ' => 'y',
            'đ' => 'd',
            other => other,
        })
        .collect()
}

fn keywords(db: &Arc<Db>, key: &str) -> Vec<String> {
    db.setting_or(key, "")
        .split(',')
        .map(|s| fold(s.trim()))
        .filter(|s| !s.is_empty())
        .collect()
}

/// Which keywords appear in `text`, matched on folded text.
///
/// Single-word keywords match whole words only; multi-word keywords match as a
/// substring. The distinction matters because folding collapses distinct
/// Vietnamese words onto the same letters: the default complaint keyword `tệ`
/// folds to `te`, which a plain substring search finds inside `tên` ("name") —
/// so "cho mình xin tên anh" would escalate as a complaint. Phrases like
/// "báo giá" are unambiguous once folded, so they stay substring matches and
/// keep firing inside longer runs of text.
pub fn matched(text: &str, kws: &[String]) -> Vec<String> {
    let folded = fold(text);
    let words: Vec<&str> =
        folded.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    kws.iter()
        .filter(|kw| {
            if kw.contains(' ') {
                folded.contains(kw.as_str())
            } else {
                words.contains(&kw.as_str())
            }
        })
        .cloned()
        .collect()
}

pub fn detect_complaint(db: &Arc<Db>, text: &str) -> Vec<String> {
    matched(text, &keywords(db, "complaint_keywords"))
}

pub fn is_risky(db: &Arc<Db>, text: &str, is_reply: bool) -> bool {
    let n = matched(text, &keywords(db, "risky_keywords")).len();
    if is_reply {
        n >= 1
    } else {
        n >= 2
    }
}

/// The decision for a single outbound draft.
#[derive(Debug, PartialEq)]
pub enum Gate {
    /// Safe to deliver.
    Send,
    /// Divert to the review queue with this reason.
    Review(String),
    /// Blocked outright — do not send, do not queue.
    Blocked(String),
}

/// Evaluate one outbound draft.
///
/// `bypass_risky` is set only on the approve-from-review path, where a human has
/// already read the words. Rate limit and unsubscribe still apply even then: the
/// first is about volume regardless of content, and the second is a standing
/// instruction from the customer that no operator should be able to click past.
pub fn gate(
    db: &Arc<Db>,
    customer_id: i64,
    unsubscribed: bool,
    draft: &str,
    is_reply: bool,
    bypass_risky: bool,
    now: i64,
) -> Gate {
    if unsubscribed {
        return Gate::Blocked("khách đã hủy nhận tin (unsubscribed)".into());
    }
    let max: i64 = db.setting_or("max_messages_per_customer_24h", "3").parse().unwrap_or(3);
    let sent = db.count_outbound_24h(customer_id, now).unwrap_or(0);
    if sent >= max {
        return Gate::Review("rate_limit_exceeded".into());
    }
    if !bypass_risky && is_risky(db, draft, is_reply) {
        return Gate::Review("risky_keywords".into());
    }
    Gate::Send
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use std::sync::Arc;

    fn test_db() -> Arc<Db> {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).unwrap();
        // tempdir must outlive the Db; leak it for the duration of the test.
        std::mem::forget(dir);
        Arc::new(db)
    }

    fn customer(db: &Arc<Db>, name: &str) -> i64 {
        let input = serde_json::from_value(serde_json::json!({ "name": name })).unwrap();
        db.create_customer(&input, 1_000).unwrap()
    }

    #[test]
    fn fold_strips_vietnamese_tone_marks() {
        assert_eq!(fold("GIÁ"), "gia");
        assert_eq!(fold("Giảm Giá"), "giam gia");
        assert_eq!(fold("hợp đồng"), "hop dong");
        assert_eq!(fold("Đặt cọc"), "dat coc");
    }

    #[test]
    fn risky_thresholds_differ_for_reply_and_broadcast() {
        let db = test_db();
        // One keyword: a reply is risky, a broadcast is not.
        assert!(is_risky(&db, "bên mình báo giá nhé", true));
        assert!(!is_risky(&db, "bên mình báo giá nhé", false));
        // Two keywords trips the broadcast threshold too.
        assert!(is_risky(&db, "báo giá và hợp đồng", false));
    }

    #[test]
    fn risky_matches_without_diacritics() {
        let db = test_db();
        assert!(is_risky(&db, "ben minh bao gia nhe", true));
    }

    #[test]
    fn unsubscribed_is_blocked_even_when_human_approved() {
        let db = test_db();
        let id = customer(&db, "A");
        let g = gate(&db, id, true, "xin chào", false, true, 10_000);
        assert!(matches!(g, Gate::Blocked(_)), "unsubscribe must survive bypass_risky");
    }

    #[test]
    fn clean_draft_passes() {
        let db = test_db();
        let id = customer(&db, "B");
        assert_eq!(gate(&db, id, false, "chào anh, em gửi tài liệu ạ", false, false, 10_000), Gate::Send);
    }

    #[test]
    fn risky_draft_goes_to_review_then_passes_once_approved() {
        let db = test_db();
        let id = customer(&db, "C");
        let draft = "dạ báo giá bên em là 10 triệu";
        assert_eq!(
            gate(&db, id, false, draft, true, false, 10_000),
            Gate::Review("risky_keywords".into())
        );
        // A human read it — the risky rule steps aside.
        assert_eq!(gate(&db, id, false, draft, true, true, 10_000), Gate::Send);
    }

    #[test]
    fn rate_limit_counts_only_delivered_messages() {
        let db = test_db();
        let id = customer(&db, "D");
        let ch = db
            .create_channel(
                &serde_json::from_value(serde_json::json!({ "kind": "websocket" })).unwrap(),
                1_000,
            )
            .unwrap();
        let conv = db.get_or_create_conversation(ch, "websocket", "u1", "D", 1_000).unwrap();
        db.link_conversation(conv.id, id, 1_000).unwrap();

        let now = 10_000;
        // Three delivered messages inside the window hits the default cap of 3.
        for _ in 0..3 {
            db.add_conv_message(conv.id, "outbound", "assistant", "hi", "sent", now - 100).unwrap();
        }
        assert_eq!(
            gate(&db, id, false, "chào anh", false, false, now),
            Gate::Review("rate_limit_exceeded".into())
        );
    }

    #[test]
    fn queued_drafts_do_not_burn_the_rate_budget() {
        let db = test_db();
        let id = customer(&db, "E");
        let ch = db
            .create_channel(
                &serde_json::from_value(serde_json::json!({ "kind": "websocket" })).unwrap(),
                1_000,
            )
            .unwrap();
        let conv = db.get_or_create_conversation(ch, "websocket", "u2", "E", 1_000).unwrap();
        db.link_conversation(conv.id, id, 1_000).unwrap();

        let now = 10_000;
        for _ in 0..5 {
            db.add_conv_message(conv.id, "outbound", "assistant", "hi", "queued", now - 100).unwrap();
        }
        assert_eq!(gate(&db, id, false, "chào anh", false, false, now), Gate::Send);
    }

    #[test]
    fn messages_older_than_24h_leave_the_window() {
        let db = test_db();
        let id = customer(&db, "F");
        let ch = db
            .create_channel(
                &serde_json::from_value(serde_json::json!({ "kind": "websocket" })).unwrap(),
                1_000,
            )
            .unwrap();
        let conv = db.get_or_create_conversation(ch, "websocket", "u3", "F", 1_000).unwrap();
        db.link_conversation(conv.id, id, 1_000).unwrap();

        let now = 200_000;
        for _ in 0..3 {
            // 25 hours ago — outside the window.
            db.add_conv_message(conv.id, "outbound", "assistant", "hi", "sent", now - 90_000).unwrap();
        }
        assert_eq!(gate(&db, id, false, "chào anh", false, false, now), Gate::Send);
    }

    #[test]
    fn complaint_detection_reads_the_configured_list() {
        let db = test_db();
        assert!(detect_complaint(&db, "cảm ơn bên em nhé").is_empty());
        assert!(!detect_complaint(&db, "tôi muốn hoàn tiền, dịch vụ quá tệ").is_empty());
        assert!(!detect_complaint(&db, "toi muon hoan tien").is_empty());
    }

    /// Folding collapses `tệ` and `tên` onto the same letters, so a substring
    /// match would read "what's your name?" as a complaint and escalate it.
    #[test]
    fn short_keywords_do_not_fire_inside_longer_words() {
        let db = test_db();
        assert!(detect_complaint(&db, "cho mình xin tên anh với").is_empty());
        assert!(detect_complaint(&db, "chúc anh ăn Tết vui vẻ").is_empty());
        // The real word still trips it.
        assert!(!detect_complaint(&db, "dịch vụ tệ quá").is_empty());
    }

    /// "giá" is a whole word inside "giảm giá" and "báo giá" — it must still fire
    /// after the word-boundary change.
    #[test]
    fn keyword_phrases_and_words_still_match() {
        let db = test_db();
        assert!(is_risky(&db, "đang có giảm giá", true));
        assert!(is_risky(&db, "em gửi báo giá ạ", true));
        assert!(!is_risky(&db, "bên em có nhiều gói khác nhau", true));
    }

    /// The broadcast rule holds a message only when TWO distinct concerns appear.
    /// Matches are counted per keyword, so an overlapping list ("giá" *and*
    /// "báo giá") makes one phrase count twice and collapses the broadcast bar
    /// onto the reply bar. This pins the seeded list against that.
    #[test]
    fn seeded_risky_keywords_do_not_overlap() {
        let db = test_db();
        let kws: Vec<String> = db
            .setting_or("risky_keywords", "")
            .split(',')
            .map(|s| fold(s.trim()))
            .filter(|s| !s.is_empty())
            .collect();
        for a in &kws {
            for b in &kws {
                if a != b {
                    assert!(
                        !matched(b, std::slice::from_ref(a)).contains(a),
                        "seeded risky keyword {a:?} also matches {b:?} — one phrase would \
                         count twice and trip the broadcast threshold on its own"
                    );
                }
            }
        }
        // The behaviour that overlap breaks: one price mention is a reply-level
        // concern only, not enough to hold a proactive message.
        assert!(!is_risky(&db, "bên mình báo giá nhé", false));
        assert!(is_risky(&db, "báo giá và hợp đồng", false));
    }
}
