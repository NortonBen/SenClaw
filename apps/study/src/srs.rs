//! Spaced-repetition ladder.
//!
//! Ported from `apps/kaen/src/srs.rs` so the two learning apps schedule reviews
//! the same way. Level 0 = never learned; a correct answer climbs 1→6 with
//! intervals `[30 phút, 1 ngày, 3, 7, 30, 90]`, a wrong answer always schedules
//! an urgent 30-minute retry, and reviews at level ≥ 2 are snapped to the
//! learner's study slot **in their own timezone** (kaizen set the hour on a
//! `moment.utc()` object, so "08:00" meant 15:00 in Vietnam — do not reintroduce
//! that).
//!
//! Two deliberate divergences from Kaen:
//!
//! * **Neglect demotes, it never deletes.** Kaizen removes the progress row of
//!   a card left overdue past level 0, which makes the word count as never
//!   learned. Here a card is the learner's own material — deleting their
//!   history because they had a bad week is not ours to do.
//! * **Four self-grades** (`again/hard/good/easy`) instead of a binary, because
//!   a flashcard the learner *almost* had should not jump the full interval.

use chrono::{DateTime, Datelike, Duration, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;

/// Review intervals in minutes, indexed by `new_level - 1` (levels 1..=6).
pub const INTERVALS_MIN: [i64; 6] = [30, 1_440, 4_320, 10_080, 43_200, 129_600];
pub const MAX_LEVEL: i64 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    /// Forgot it — urgent retry, level down.
    Again,
    /// Recalled it with effort — same level, same interval again.
    Hard,
    /// Recalled it — one level up.
    Good,
    /// Instant — two levels up.
    Easy,
}

impl Grade {
    pub fn parse(s: &str) -> Option<Grade> {
        match s.trim().to_lowercase().as_str() {
            "again" | "quen" | "quên" | "0" => Some(Grade::Again),
            "hard" | "kho" | "khó" | "1" => Some(Grade::Hard),
            "good" | "duoc" | "được" | "2" => Some(Grade::Good),
            "easy" | "de" | "dễ" | "3" => Some(Grade::Easy),
            _ => None,
        }
    }

    pub fn is_correct(self) -> bool {
        !matches!(self, Grade::Again)
    }

    fn level_delta(self) -> i64 {
        match self {
            Grade::Again => -1,
            Grade::Hard => 0,
            Grade::Good => 1,
            Grade::Easy => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub level: i64,
    pub next_review: DateTime<Utc>,
    pub is_urgent: bool,
    pub last_reviewed: DateTime<Utc>,
    pub first_due_at: Option<DateTime<Utc>>,
    pub reviews: i64,
    pub lapses: i64,
}

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

fn slot_hm(slots: &[String]) -> (u32, u32) {
    let first = slots.first().map(String::as_str).unwrap_or("20:00");
    let mut it = first.split(':');
    let h = it.next().and_then(|v| v.parse().ok()).unwrap_or(20);
    let m = it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    (h.min(23), m.min(59))
}

/// Wall-clock time in `tz`, converted to UTC — DST-safe.
pub fn local_instant(date: NaiveDate, h: u32, m: u32, tz: Tz) -> DateTime<Utc> {
    match tz.with_ymd_and_hms(date.year(), date.month(), date.day(), h, m, 0) {
        LocalResult::Single(dt) => dt.with_timezone(&Utc),
        LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
        // Spring-forward gap: that wall time does not exist; use one hour later.
        LocalResult::None => {
            let naive = date.and_hms_opt(h, m, 0).unwrap() + Duration::hours(1);
            match tz.from_local_datetime(&naive) {
                LocalResult::Single(dt) | LocalResult::Ambiguous(dt, _) => dt.with_timezone(&Utc),
                LocalResult::None => Utc.from_utc_datetime(&naive),
            }
        }
    }
}

/// When the next review falls, given the level the card lands on.
pub fn next_review_for(
    new_level: i64,
    slots: &[String],
    tz: Tz,
    now: DateTime<Utc>,
) -> (DateTime<Utc>, bool) {
    let idx = ((new_level - 1).max(0) as usize).min(INTERVALS_MIN.len() - 1);
    let minutes = INTERVALS_MIN[idx];

    // The 30-minute rung is urgent and unsnapped — snapping it to tonight's
    // study slot would turn "try again shortly" into "try again tomorrow".
    if new_level <= 1 || minutes <= 30 {
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

/// Grade a review and produce the card's new progress.
pub fn apply(
    existing: Option<&Progress>,
    grade: Grade,
    slots: &[String],
    tz: Tz,
    now: DateTime<Utc>,
) -> Progress {
    let Some(p) = existing else {
        let level = if grade.is_correct() {
            grade.level_delta().max(1).min(MAX_LEVEL)
        } else {
            0
        };
        let (next_review, is_urgent) = next_review_for(level, slots, tz, now);
        return Progress {
            level,
            next_review,
            is_urgent,
            last_reviewed: now,
            first_due_at: None,
            reviews: 1,
            lapses: if grade.is_correct() { 0 } else { 1 },
        };
    };

    let due = p.next_review <= now;
    // Anchor lateness at the FIRST time the card came due, not at the last
    // reschedule, so repeated 30-minute punts don't hide real neglect.
    let first_due = p.first_due_at.unwrap_or(p.next_review);
    let neglected = due && (now - first_due > Duration::days(1));

    let base = if neglected { (p.level - 1).max(0) } else { p.level };
    let mut level = if due {
        (base + grade.level_delta()).clamp(0, MAX_LEVEL)
    } else {
        // Early, voluntary review never changes the level — otherwise a learner
        // can grind a card to level 6 in one sitting and never see it again.
        p.level
    };
    if !grade.is_correct() {
        level = level.min(p.level.saturating_sub(1).max(0));
    }

    let (next_review, is_urgent) = next_review_for(level, slots, tz, now);
    // An early review must not pull the due date closer than it already is.
    let next_review = if !due && grade.is_correct() {
        next_review.max(p.next_review)
    } else {
        next_review
    };

    Progress {
        level,
        next_review,
        is_urgent,
        last_reviewed: now,
        first_due_at: if due { None } else { p.first_due_at },
        reviews: p.reviews + 1,
        lapses: p.lapses + if grade.is_correct() { 0 } else { 1 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tz() -> Tz {
        chrono_tz::Asia::Ho_Chi_Minh
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-01T03:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn a_wrong_answer_always_retries_in_thirty_minutes() {
        let p = apply(None, Grade::Again, &["20:00".into()], tz(), now());
        assert_eq!(p.level, 0);
        assert!(p.is_urgent);
        assert_eq!(p.next_review, now() + Duration::minutes(30));
    }

    #[test]
    fn a_new_card_answered_well_starts_at_level_one() {
        let p = apply(None, Grade::Good, &["20:00".into()], tz(), now());
        assert_eq!(p.level, 1);
    }

    #[test]
    fn easy_climbs_faster_than_good_which_climbs_faster_than_hard() {
        let base = Progress {
            level: 2,
            next_review: now() - Duration::minutes(1),
            is_urgent: false,
            last_reviewed: now() - Duration::days(1),
            first_due_at: None,
            reviews: 1,
            lapses: 0,
        };
        let slots = vec!["20:00".to_string()];
        let hard = apply(Some(&base), Grade::Hard, &slots, tz(), now()).level;
        let good = apply(Some(&base), Grade::Good, &slots, tz(), now()).level;
        let easy = apply(Some(&base), Grade::Easy, &slots, tz(), now()).level;
        assert_eq!((hard, good, easy), (2, 3, 4));
    }

    #[test]
    fn reviews_from_level_two_up_land_on_the_study_slot_in_local_time() {
        let base = Progress {
            level: 1,
            next_review: now() - Duration::minutes(1),
            is_urgent: true,
            last_reviewed: now(),
            first_due_at: None,
            reviews: 1,
            lapses: 0,
        };
        let p = apply(Some(&base), Grade::Good, &["08:00".into()], tz(), now());
        assert_eq!(p.level, 2);
        let local = p.next_review.with_timezone(&tz());
        assert_eq!(local.format("%H:%M").to_string(), "08:00");
    }

    #[test]
    fn neglect_demotes_but_never_deletes_the_card() {
        let base = Progress {
            level: 3,
            next_review: now() - Duration::days(5),
            is_urgent: false,
            last_reviewed: now() - Duration::days(10),
            first_due_at: Some(now() - Duration::days(5)),
            reviews: 4,
            lapses: 0,
        };
        let p = apply(Some(&base), Grade::Good, &["20:00".into()], tz(), now());
        assert_eq!(p.level, 3, "demoted to 2 then promoted by the correct answer");

        // Even from level 1, neglected and wrong, the card survives at level 0.
        let low = Progress { level: 1, ..base };
        let p = apply(Some(&low), Grade::Again, &["20:00".into()], tz(), now());
        assert_eq!(p.level, 0);
        assert_eq!(p.lapses, 1);
    }

    #[test]
    fn an_early_review_does_not_change_the_level_or_pull_the_due_date_in() {
        let base = Progress {
            level: 4,
            next_review: now() + Duration::days(5),
            is_urgent: false,
            last_reviewed: now(),
            first_due_at: None,
            reviews: 3,
            lapses: 0,
        };
        let p = apply(Some(&base), Grade::Good, &["20:00".into()], tz(), now());
        assert_eq!(p.level, 4, "grinding a card early must not max it out");
        assert!(p.next_review >= base.next_review);
    }

    #[test]
    fn the_level_ceiling_holds() {
        let base = Progress {
            level: MAX_LEVEL,
            next_review: now() - Duration::minutes(1),
            is_urgent: false,
            last_reviewed: now(),
            first_due_at: None,
            reviews: 9,
            lapses: 0,
        };
        let p = apply(Some(&base), Grade::Easy, &["20:00".into()], tz(), now());
        assert_eq!(p.level, MAX_LEVEL);
    }

    #[test]
    fn grades_parse_from_vietnamese_and_english() {
        assert_eq!(Grade::parse("Quên"), Some(Grade::Again));
        assert_eq!(Grade::parse("good"), Some(Grade::Good));
        assert_eq!(Grade::parse("dễ"), Some(Grade::Easy));
        assert_eq!(Grade::parse("xyz"), None);
    }

    #[test]
    fn round_trip_of_the_timestamp_format() {
        let t = now();
        assert_eq!(parse(&fmt(t)).unwrap(), t);
    }
}
