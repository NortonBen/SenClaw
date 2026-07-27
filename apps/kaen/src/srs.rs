//! SRS engine — faithful port of kaizen's `study.service.ts`.
//!
//! Review ladder (Doc.md): level 0 = new; answering correctly climbs
//! 1→2→…→6 with intervals [30min, 1d, 3d, 7d, 30d, 90d]; a wrong answer
//! always schedules a 30-minute urgent retry. Reviews of levels ≥ 2 are
//! snapped to the user's first study slot ("khung giờ vàng").
//!
//! One deliberate divergence from the TS original: kaizen set the slot hour
//! on a `moment.utc()` object, so "08:00" meant 08:00 **UTC** (15:00 VN) —
//! its own `snapToStudySlotInTimezone` util existed but was never called.
//! Here the slot is interpreted in the user's timezone, which is what the
//! PRD describes.

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

/// Review intervals in minutes, indexed by `new_level - 1` (levels 1..=6).
pub const INTERVALS_MIN: [i64; 6] = [30, 1440, 4320, 10080, 43200, 129600];
pub const MAX_LEVEL: i64 = 6;

pub fn fmt(dt: DateTime<Utc>) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

pub fn parse(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

pub fn parse_tz(name: &str) -> Tz {
    name.parse().unwrap_or(chrono_tz::Asia::Ho_Chi_Minh)
}

/// The card-progress fields the algorithm reads and writes.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressData {
    pub level: i64,
    pub next_review: DateTime<Utc>,
    pub is_urgent: bool,
    pub last_reviewed: DateTime<Utc>,
    pub first_due_at: Option<DateTime<Utc>>,
    pub notification_sent_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ReviewAction {
    Create(ProgressData),
    Update(ProgressData),
    /// Overdue by more than a day and the level fell to 0 — kaizen deletes the
    /// row so the card counts as never learned again.
    Remove,
}

fn slot_hm(slots: &[String]) -> (u32, u32) {
    // kaizen only ever snaps to the FIRST (earliest) slot; default 08:00.
    let first = slots.first().map(String::as_str).unwrap_or("08:00");
    let mut it = first.split(':');
    let h = it.next().and_then(|v| v.parse().ok()).unwrap_or(8);
    let m = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    (h.min(23), m.min(59))
}

pub fn local_instant(date: NaiveDate, h: u32, m: u32, tz: Tz) -> DateTime<Utc> {
    match tz.with_ymd_and_hms(date.year(), date.month(), date.day(), h, m, 0) {
        LocalResult::Single(dt) => dt.with_timezone(&Utc),
        LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
        // DST spring-forward gap: the wall time doesn't exist; use one hour later.
        LocalResult::None => {
            let naive = date.and_hms_opt(h, m, 0).unwrap() + Duration::hours(1);
            match tz.from_local_datetime(&naive) {
                LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
                LocalResult::None => Utc.from_utc_datetime(&naive),
            }
        }
    }
}

/// `calculateNextReview` — returns (next_review, is_urgent).
pub fn calculate_next_review(
    current_level: i64,
    is_correct: bool,
    slots: &[String],
    tz: Tz,
    now: DateTime<Utc>,
) -> (DateTime<Utc>, bool) {
    // Wrong → urgent 30-minute retry, no snapping.
    if !is_correct {
        return (now + Duration::minutes(30), true);
    }

    let new_level = (current_level + 1).min(MAX_LEVEL);
    let idx = ((new_level - 1).max(0) as usize).min(INTERVALS_MIN.len() - 1);
    let minutes = INTERVALS_MIN[idx];

    // First rung (30 minutes) is also urgent and unsnapped.
    if minutes == 30 {
        return (now + Duration::minutes(30), true);
    }

    let target = now + Duration::minutes(minutes);
    let (h, m) = slot_hm(slots);
    let local_date = target.with_timezone(&tz).date_naive();
    let mut next = local_instant(local_date, h, m, tz);
    if next <= target {
        next = local_instant(local_date + Duration::days(1), h, m, tz);
    }
    (next, false)
}

/// `submitReview` / `submitSpacedRepetitionReview` core — the three branches:
/// due-on-time, overdue by more than a day, and voluntary early review.
pub fn apply_review(
    existing: Option<&ProgressData>,
    is_correct: bool,
    slots: &[String],
    tz: Tz,
    now: DateTime<Utc>,
) -> ReviewAction {
    // notificationSentAt rule shared by every update path: only re-arm the
    // reminder when the new due time is comfortably (>= 1h) in the future.
    let notif = |next_review: DateTime<Utc>| -> Option<DateTime<Utc>> {
        if next_review > now + Duration::hours(1) {
            None
        } else {
            Some(now)
        }
    };

    let Some(p) = existing else {
        // New card: correct promotes straight to level 1, wrong stays at 0;
        // either way the first retry is 30 minutes out.
        let (next_review, is_urgent) = calculate_next_review(0, is_correct, slots, tz, now);
        return ReviewAction::Create(ProgressData {
            level: if is_correct { 1 } else { 0 },
            next_review,
            is_urgent,
            last_reviewed: now,
            first_due_at: None,
            notification_sent_at: None,
        });
    };

    let due = p.next_review <= now;
    if due {
        // Anchor lateness at the FIRST time the card came due, not at the last
        // reschedule, so repeated 30-minute punts don't hide real neglect.
        let first_due = p.first_due_at.unwrap_or(p.next_review);
        let within_one_day = now - first_due <= Duration::days(1);

        if !within_one_day {
            // Neglected for over a day: demote one level before grading.
            let new_level = (p.level - 1).max(0);
            if new_level == 0 {
                return ReviewAction::Remove;
            }
            let (next_review, is_urgent) =
                calculate_next_review(new_level, is_correct, slots, tz, now);
            ReviewAction::Update(ProgressData {
                level: new_level,
                next_review,
                is_urgent,
                last_reviewed: now,
                first_due_at: None,
                notification_sent_at: notif(next_review),
            })
        } else {
            let (next_review, is_urgent) =
                calculate_next_review(p.level, is_correct, slots, tz, now);
            let new_level = if is_correct {
                (p.level + 1).min(MAX_LEVEL)
            } else {
                (p.level - 1).max(0)
            };
            ReviewAction::Update(ProgressData {
                level: new_level,
                next_review,
                is_urgent,
                last_reviewed: now,
                first_due_at: None,
                notification_sent_at: notif(next_review),
            })
        }
    } else {
        // Early (voluntary) review: never changes the level.
        if is_correct {
            let (next_review, is_urgent) = calculate_next_review(p.level, true, slots, tz, now);
            ReviewAction::Update(ProgressData {
                level: p.level,
                next_review,
                is_urgent,
                last_reviewed: now,
                first_due_at: p.first_due_at,
                notification_sent_at: notif(next_review),
            })
        } else {
            let early = now + Duration::minutes(30);
            if early < p.next_review {
                ReviewAction::Update(ProgressData {
                    level: p.level,
                    next_review: early,
                    is_urgent: true,
                    last_reviewed: now,
                    first_due_at: p.first_due_at,
                    notification_sent_at: None,
                })
            } else {
                ReviewAction::Update(ProgressData {
                    last_reviewed: now,
                    ..p.clone()
                })
            }
        }
    }
}

/// XP for one graded card: base 10, +5 bonus for a correct TYPING answer.
pub fn xp_for(mode: &str, is_correct: bool) -> i64 {
    if mode.eq_ignore_ascii_case("TYPING") && is_correct {
        15
    } else {
        10
    }
}

/// UTC instant of local midnight (start of day) in `tz` for the day containing `t`.
pub fn start_of_day(tz: Tz, t: DateTime<Utc>) -> DateTime<Utc> {
    local_instant(t.with_timezone(&tz).date_naive(), 0, 0, tz)
}

/// UTC instant of local end-of-day (23:59:59.999) in `tz`.
pub fn end_of_day(tz: Tz, t: DateTime<Utc>) -> DateTime<Utc> {
    start_of_day(tz, t) + Duration::days(1) - Duration::milliseconds(1)
}

/// `updateStreakAndLastStudyDate` — returns (new_streak, should_update_last_study_date).
pub fn next_streak(
    last_study_date: Option<DateTime<Utc>>,
    current_streak: i64,
    tz: Tz,
    now: DateTime<Utc>,
) -> (i64, bool) {
    let Some(last) = last_study_date else {
        return (1, true); // first ever study
    };
    let sot = start_of_day(tz, now);
    let sol = start_of_day(tz, last);
    let days_diff = (sot - sol).num_seconds().div_euclid(86_400);

    if days_diff <= 0 {
        // Same day. Exception: a zeroed streak restarts at 1.
        if current_streak == 0 {
            (1, true)
        } else {
            (current_streak, false)
        }
    } else if days_diff == 1 {
        (current_streak + 1, true)
    } else {
        (0, true) // missed at least one full day
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TZ: Tz = chrono_tz::Asia::Ho_Chi_Minh; // UTC+7, no DST

    fn utc(s: &str) -> DateTime<Utc> {
        parse(s).unwrap()
    }

    fn slots() -> Vec<String> {
        vec!["08:00".into(), "20:00".into()]
    }

    fn prog(level: i64, next_review: &str) -> ProgressData {
        ProgressData {
            level,
            next_review: utc(next_review),
            is_urgent: false,
            last_reviewed: utc("2026-07-01T00:00:00.000Z"),
            first_due_at: None,
            notification_sent_at: None,
        }
    }

    #[test]
    fn wrong_answer_always_means_30_minutes_urgent() {
        let now = utc("2026-07-20T03:00:00.000Z");
        for level in 0..=6 {
            let (next, urgent) = calculate_next_review(level, false, &slots(), TZ, now);
            assert_eq!(next, now + Duration::minutes(30));
            assert!(urgent);
        }
    }

    #[test]
    fn first_correct_answer_is_the_30_minute_rung() {
        let now = utc("2026-07-20T03:00:00.000Z");
        let (next, urgent) = calculate_next_review(0, true, &slots(), TZ, now);
        assert_eq!(next, now + Duration::minutes(30));
        assert!(urgent);
    }

    #[test]
    fn level1_correct_snaps_to_next_morning_slot_in_user_timezone() {
        // now = 10:00 VN on Jul 20. Interval for level 1→2 is 24h → target
        // 10:00 VN Jul 21, past that day's 08:00 slot → snap to 08:00 VN Jul 22
        // (01:00 UTC). The TS original would have produced 08:00 UTC — the
        // deliberate divergence documented in the module header.
        let now = utc("2026-07-20T03:00:00.000Z");
        let (next, urgent) = calculate_next_review(1, true, &slots(), TZ, now);
        assert_eq!(next, utc("2026-07-22T01:00:00.000Z"));
        assert!(!urgent);
    }

    #[test]
    fn slot_later_the_same_day_is_used_without_rolling_over() {
        // now = 05:00 VN Jul 20; +24h target = 05:00 VN Jul 21, before that
        // day's 08:00 slot → snap lands the SAME day (Jul 21) at 08:00 VN.
        let now = utc("2026-07-19T22:00:00.000Z");
        let (next, _) = calculate_next_review(1, true, &slots(), TZ, now);
        assert_eq!(next, utc("2026-07-21T01:00:00.000Z"));
    }

    #[test]
    fn empty_slots_default_to_08_00() {
        let now = utc("2026-07-20T03:00:00.000Z");
        let (next, _) = calculate_next_review(1, true, &[], TZ, now);
        assert_eq!(next, utc("2026-07-22T01:00:00.000Z"));
    }

    #[test]
    fn level_caps_at_6_with_90_day_interval() {
        let now = utc("2026-07-20T03:00:00.000Z");
        let (next6, _) = calculate_next_review(6, true, &slots(), TZ, now);
        let (next5, _) = calculate_next_review(5, true, &slots(), TZ, now);
        assert_eq!(next6, next5, "level 6 keeps the level-6 interval");
        assert!(next6 > now + Duration::days(89));
    }

    #[test]
    fn new_card_correct_creates_level_1() {
        let now = utc("2026-07-20T03:00:00.000Z");
        match apply_review(None, true, &slots(), TZ, now) {
            ReviewAction::Create(p) => {
                assert_eq!(p.level, 1);
                assert_eq!(p.next_review, now + Duration::minutes(30));
                assert!(p.is_urgent);
            }
            other => panic!("expected Create, got {other:?}"),
        }
    }

    #[test]
    fn new_card_wrong_creates_level_0() {
        let now = utc("2026-07-20T03:00:00.000Z");
        match apply_review(None, false, &slots(), TZ, now) {
            ReviewAction::Create(p) => {
                assert_eq!(p.level, 0);
                assert!(p.is_urgent);
            }
            other => panic!("expected Create, got {other:?}"),
        }
    }

    #[test]
    fn due_on_time_correct_promotes_and_clears_first_due() {
        let now = utc("2026-07-20T03:00:00.000Z");
        let mut p = prog(2, "2026-07-20T02:00:00.000Z"); // due 1h ago
        p.first_due_at = Some(utc("2026-07-20T02:00:00.000Z"));
        match apply_review(Some(&p), true, &slots(), TZ, now) {
            ReviewAction::Update(u) => {
                assert_eq!(u.level, 3);
                assert_eq!(u.first_due_at, None);
                assert!(!u.is_urgent);
                // 3-day interval re-armed the notification.
                assert_eq!(u.notification_sent_at, None);
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn due_on_time_wrong_demotes_and_stamps_notification() {
        let now = utc("2026-07-20T03:00:00.000Z");
        let p = prog(3, "2026-07-20T02:30:00.000Z");
        match apply_review(Some(&p), false, &slots(), TZ, now) {
            ReviewAction::Update(u) => {
                assert_eq!(u.level, 2);
                assert_eq!(u.next_review, now + Duration::minutes(30));
                assert!(u.is_urgent);
                // 30-minute retry is < 1h out → suppress re-notification.
                assert_eq!(u.notification_sent_at, Some(now));
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn overdue_more_than_a_day_at_level_1_removes_the_card() {
        let now = utc("2026-07-20T03:00:00.000Z");
        let p = prog(1, "2026-07-18T03:00:00.000Z"); // due 2 days ago
        assert_eq!(
            apply_review(Some(&p), true, &slots(), TZ, now),
            ReviewAction::Remove
        );
    }

    #[test]
    fn overdue_more_than_a_day_demotes_before_grading() {
        let now = utc("2026-07-20T03:00:00.000Z");
        let p = prog(4, "2026-07-18T00:00:00.000Z");
        match apply_review(Some(&p), true, &slots(), TZ, now) {
            ReviewAction::Update(u) => {
                // Level drops 4→3 regardless of the correct answer; the correct
                // answer only shapes the next interval (computed from level 3).
                assert_eq!(u.level, 3);
                assert!(!u.is_urgent);
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn overdue_anchor_is_first_due_at_not_latest_punt() {
        let now = utc("2026-07-20T03:00:00.000Z");
        // next_review was punted to 30 minutes ago, but the card FIRST came due
        // 3 days ago — lateness is judged from that anchor.
        let mut p = prog(2, "2026-07-20T02:30:00.000Z");
        p.first_due_at = Some(utc("2026-07-17T02:00:00.000Z"));
        match apply_review(Some(&p), true, &slots(), TZ, now) {
            ReviewAction::Update(u) => assert_eq!(u.level, 1, "demoted despite recent punt"),
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn early_review_correct_reschedules_without_promotion() {
        let now = utc("2026-07-20T03:00:00.000Z");
        let p = prog(3, "2026-07-25T01:00:00.000Z"); // not due for 5 days
        match apply_review(Some(&p), true, &slots(), TZ, now) {
            ReviewAction::Update(u) => {
                assert_eq!(u.level, 3, "early review never promotes");
                assert!(u.next_review > now + Duration::days(6), "7d interval from now");
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn early_review_wrong_pulls_review_forward_only_if_sooner() {
        let now = utc("2026-07-20T03:00:00.000Z");
        let far = prog(3, "2026-07-25T01:00:00.000Z");
        match apply_review(Some(&far), false, &slots(), TZ, now) {
            ReviewAction::Update(u) => {
                assert_eq!(u.next_review, now + Duration::minutes(30));
                assert!(u.is_urgent);
                assert_eq!(u.level, 3);
            }
            other => panic!("expected Update, got {other:?}"),
        }
        // Already due sooner than +30m → untouched except last_reviewed.
        let near = prog(3, "2026-07-20T03:10:00.000Z");
        match apply_review(Some(&near), false, &slots(), TZ, now) {
            ReviewAction::Update(u) => {
                assert_eq!(u.next_review, near.next_review);
                assert_eq!(u.is_urgent, near.is_urgent);
                assert_eq!(u.last_reviewed, now);
            }
            other => panic!("expected Update, got {other:?}"),
        }
    }

    #[test]
    fn streak_transitions() {
        let tz = TZ;
        let now = utc("2026-07-20T03:00:00.000Z"); // 10:00 VN Jul 20

        // First study ever.
        assert_eq!(next_streak(None, 0, tz, now), (1, true));
        // Same local day: unchanged…
        let today = utc("2026-07-19T23:00:00.000Z"); // 06:00 VN Jul 20
        assert_eq!(next_streak(Some(today), 4, tz, now), (4, false));
        // …unless the streak was zeroed, which restarts at 1.
        assert_eq!(next_streak(Some(today), 0, tz, now), (1, true));
        // Yesterday (local): +1.
        let yesterday = utc("2026-07-19T10:00:00.000Z"); // 17:00 VN Jul 19
        assert_eq!(next_streak(Some(yesterday), 4, tz, now), (5, true));
        // Two local days ago: reset.
        let stale = utc("2026-07-18T10:00:00.000Z");
        assert_eq!(next_streak(Some(stale), 4, tz, now), (0, true));
        // Timezone matters: 23:30 UTC Jul 19 is 06:30 VN Jul 20 — same VN day.
        let late_utc = utc("2026-07-19T23:30:00.000Z");
        assert_eq!(next_streak(Some(late_utc), 2, tz, now), (2, false));
    }

    #[test]
    fn xp_bonus_only_for_correct_typing() {
        assert_eq!(xp_for("TYPING", true), 15);
        assert_eq!(xp_for("TYPING", false), 10);
        assert_eq!(xp_for("FLIP", true), 10);
    }

    #[test]
    fn timestamp_format_round_trips_and_sorts() {
        let t = utc("2026-07-20T03:04:05.678Z");
        assert_eq!(fmt(t), "2026-07-20T03:04:05.678Z");
        assert_eq!(parse(&fmt(t)), Some(t));
        // Lexicographic order == chronological order for the fixed format.
        assert!(fmt(t) < fmt(t + Duration::milliseconds(1)));
    }
}
