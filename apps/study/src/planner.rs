//! Turning a set of sections into a dated study plan.
//!
//! **The LLM does not schedule anything.** It supplies `est_minutes`,
//! `difficulty` and `prerequisites` per section (see `outline.rs`); everything
//! from there is arithmetic. A schedule decided by a model is a model's
//! opinion; a schedule decided by dividing work by available minutes is a fact
//! about the learner's calendar, and it is checkable.
//!
//! Three rules shape the output:
//!
//! * **Reserve time for review.** A plan that spends every minute on new
//!   material has no spacing effect and is the most common way self-made study
//!   plans fail. `content_ratio` (default 0.7) is the share of each session
//!   that may hold new content; the rest is review and retrieval practice.
//! * **Prerequisites are hard, order is not.** Sections are topologically
//!   sorted; among sections that are all ready, several documents are
//!   interleaved rather than finished one at a time.
//! * **Never silently drop work.** If the material does not fit the requested
//!   days × minutes, the planner still returns a plan for what fits, plus the
//!   exact list of what did not and three concrete ways to fix it. Quietly
//!   shipping a truncated plan reads to the learner as "you're done".

use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;

use crate::db::SectionRow;

// ── Templates ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Template {
    pub key: String,
    pub label: String,
    pub detail: String,
    pub days: i64,
    pub min_per_day: i64,
    pub review_offsets: Vec<i64>,
    pub blocks: Vec<String>,
    pub content_ratio: f64,
}

pub struct BuiltinTemplate {
    pub key: &'static str,
    pub label: &'static str,
    pub detail: &'static str,
    pub days: i64,
    pub min_per_day: i64,
    pub review_offsets: &'static [i64],
    pub blocks: &'static [&'static str],
    pub content_ratio: f64,
    pub sort: i64,
}

/// The five shipped rhythms. Review offsets are day gaps after first exposure;
/// they widen the way retention research says forgetting curves flatten, and
/// they are the reason `content_ratio` is below 1.
pub const BUILTIN_TEMPLATES: &[BuiltinTemplate] = &[
    BuiltinTemplate {
        key: "sprint",
        label: "Nước rút",
        detail: "7 ngày, 60–90 phút/ngày, ôn dày 1/2/4 ngày. Dùng khi sắp thi.",
        days: 7,
        min_per_day: 75,
        review_offsets: &[1, 2, 4],
        blocks: &["read", "flashcard", "quiz"],
        content_ratio: 0.65,
        sort: 10,
    },
    BuiltinTemplate {
        key: "standard",
        label: "Chuẩn",
        detail: "30 ngày, 30 phút/ngày, ôn giãn 1/3/7/16 ngày. Mặc định.",
        days: 30,
        min_per_day: 30,
        review_offsets: &[1, 3, 7, 16],
        blocks: &["read", "flashcard", "quiz"],
        content_ratio: 0.7,
        sort: 20,
    },
    BuiltinTemplate {
        key: "mastery",
        label: "Chuyên sâu",
        detail: "60 ngày, 45 phút/ngày, thêm khối tự diễn giải (Feynman) và ôn 35 ngày.",
        days: 60,
        min_per_day: 45,
        review_offsets: &[1, 3, 7, 16, 35],
        blocks: &["read", "flashcard", "recall", "quiz"],
        content_ratio: 0.6,
        sort: 30,
    },
    BuiltinTemplate {
        key: "micro",
        label: "Vi mô",
        detail: "45 ngày, 18 phút/ngày — phiên ngắn, thẻ là chính. Hợp lịch bận.",
        days: 45,
        min_per_day: 18,
        review_offsets: &[1, 3, 7, 16],
        blocks: &["flashcard", "read", "quiz"],
        content_ratio: 0.6,
        sort: 40,
    },
    BuiltinTemplate {
        key: "refresher",
        label: "Ôn lại",
        detail: "10 ngày, 20 phút/ngày — chỉ thẻ và trắc nghiệm, bỏ đọc. Dùng khi đã học rồi.",
        days: 10,
        min_per_day: 20,
        review_offsets: &[1, 3, 7],
        blocks: &["flashcard", "quiz"],
        content_ratio: 0.5,
        sort: 50,
    },
];

// ── Plan request / result ───────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PlanRequest {
    pub start_date: NaiveDate,
    /// Number of *study sessions*, not calendar days. With `weekdays`
    /// restricted, the calendar span is longer — the plan reports both.
    pub days: i64,
    pub min_per_day: i64,
    /// ISO weekday numbers, 1 = Monday … 7 = Sunday.
    pub weekdays: Vec<u32>,
    pub slot_hm: String,
    pub review_offsets: Vec<i64>,
    pub blocks: Vec<String>,
    pub content_ratio: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlannedItem {
    pub kind: String,
    pub section_id: Option<String>,
    pub section_title: String,
    pub est_minutes: i64,
    /// 1-based part index when a section is too big for one session.
    pub part: i64,
    pub parts: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedSession {
    pub ord: i64,
    pub date: String,
    pub start_hm: String,
    pub minutes: i64,
    pub title: String,
    pub items: Vec<PlannedItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dropped {
    pub section_id: String,
    pub title: String,
    pub est_minutes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanPreview {
    pub feasible: bool,
    pub sessions: Vec<PlannedSession>,
    pub total_est_minutes: i64,
    pub content_budget_minutes: i64,
    pub budget_minutes: i64,
    pub span_days: i64,
    pub dropped: Vec<Dropped>,
    /// Concrete ways to make the material fit. Empty when it already does.
    pub options: Vec<String>,
    pub notes: Vec<String>,
}

// ── Scheduling ──────────────────────────────────────────────────────────────

/// Dates of the first `count` sessions from `start`, honouring `weekdays`.
pub fn session_dates(start: NaiveDate, count: i64, weekdays: &[u32]) -> Vec<NaiveDate> {
    let allowed: HashSet<u32> = if weekdays.is_empty() {
        (1..=7).collect()
    } else {
        weekdays.iter().copied().filter(|d| (1..=7).contains(d)).collect()
    };
    let allowed: HashSet<u32> = if allowed.is_empty() {
        (1..=7).collect()
    } else {
        allowed
    };
    let mut out = Vec::new();
    let mut d = start;
    // A whole year of calendar is plenty; the guard exists so a caller that
    // passes an impossible weekday set cannot spin forever.
    for _ in 0..(count.max(1) * 7 + 400) {
        if out.len() as i64 >= count {
            break;
        }
        if allowed.contains(&d.weekday().number_from_monday()) {
            out.push(d);
        }
        d += Duration::days(1);
    }
    out
}

/// Topological order that interleaves independent work.
///
/// Kahn's algorithm, and when several sections are simultaneously ready it
/// prefers one from a *different* document than the last pick — finishing one
/// book before opening the next is exactly the blocked practice that hurts
/// retention. Within a single document the original order is kept: a textbook's
/// own sequence is a real prerequisite signal, and shuffling chapters to
/// simulate interleaving would fight the author.
///
/// A prerequisite cycle cannot stall the plan: whatever remains is appended in
/// document order and reported by the caller.
pub fn order_sections(sections: &[SectionRow]) -> (Vec<usize>, bool) {
    let index: HashMap<&str, usize> = sections
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.as_str(), i))
        .collect();

    let mut indeg = vec![0usize; sections.len()];
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); sections.len()];
    for (i, s) in sections.iter().enumerate() {
        for p in &s.prereq {
            if let Some(&pi) = index.get(p.as_str()) {
                if pi != i {
                    edges[pi].push(i);
                    indeg[i] += 1;
                }
            }
        }
    }

    let mut ready: Vec<usize> = (0..sections.len()).filter(|i| indeg[*i] == 0).collect();
    let mut out = Vec::with_capacity(sections.len());
    let mut last_doc: Option<String> = None;

    while !ready.is_empty() {
        ready.sort_unstable();
        let pick_pos = last_doc
            .as_ref()
            .and_then(|d| ready.iter().position(|i| &sections[*i].doc_id != d))
            .unwrap_or(0);
        let i = ready.remove(pick_pos);
        last_doc = Some(sections[i].doc_id.clone());
        out.push(i);
        for &n in &edges[i] {
            indeg[n] -= 1;
            if indeg[n] == 0 {
                ready.push(n);
            }
        }
    }

    let had_cycle = out.len() < sections.len();
    if had_cycle {
        let seen: HashSet<usize> = out.iter().copied().collect();
        for i in 0..sections.len() {
            if !seen.contains(&i) {
                out.push(i);
            }
        }
    }
    (out, had_cycle)
}

/// Build the plan. Pure function of its inputs — no clock, no database.
pub fn build(sections: &[SectionRow], req: &PlanRequest) -> PlanPreview {
    let mut notes = Vec::new();
    let days = req.days.max(1);
    let min_per_day = req.min_per_day.max(5);
    let ratio = req.content_ratio.clamp(0.3, 1.0);

    let dates = session_dates(req.start_date, days, &req.weekdays);
    let span_days = dates
        .last()
        .zip(dates.first())
        .map(|(l, f)| (*l - *f).num_days() + 1)
        .unwrap_or(0);

    let budget = days * min_per_day;
    let content_cap_per_session = ((min_per_day as f64) * ratio).floor().max(5.0) as i64;
    let review_cap_per_session = (min_per_day - content_cap_per_session).max(0);
    let content_budget = content_cap_per_session * days;

    let (order, had_cycle) = order_sections(sections);
    if had_cycle {
        notes.push(
            "phát hiện vòng lặp trong điều kiện tiên quyết — các mục còn lại được xếp theo thứ tự tài liệu"
                .to_string(),
        );
    }
    let total_est: i64 = sections.iter().map(|s| s.est_minutes.max(1)).sum();

    // ── Lay content into sessions ───────────────────────────────────────────
    let mut sessions: Vec<PlannedSession> = dates
        .iter()
        .enumerate()
        .map(|(i, d)| PlannedSession {
            ord: i as i64,
            date: d.format("%Y-%m-%d").to_string(),
            start_hm: req.slot_hm.clone(),
            minutes: 0,
            title: String::new(),
            items: Vec::new(),
        })
        .collect();

    let mut used_content = vec![0i64; sessions.len()];
    let mut used_review = vec![0i64; sessions.len()];
    let mut first_seen: Vec<(usize, String, String)> = Vec::new(); // (session idx, id, title)
    let mut dropped: Vec<Dropped> = Vec::new();

    let mut cursor = 0usize;
    for &si in &order {
        let s = &sections[si];
        let est = s.est_minutes.max(1);
        // A section bigger than one session's content capacity is split into
        // parts rather than pushed past the session's length.
        let parts = ((est as f64) / (content_cap_per_session as f64)).ceil() as i64;
        let parts = parts.max(1);
        let per_part = (est as f64 / parts as f64).ceil() as i64;

        let mut placed_any = false;
        for part in 1..=parts {
            // Find the next session with room.
            while cursor < sessions.len()
                && used_content[cursor] + per_part > content_cap_per_session
                && used_content[cursor] > 0
            {
                cursor += 1;
            }
            if cursor >= sessions.len() {
                break;
            }
            sessions[cursor].items.push(PlannedItem {
                kind: "read".into(),
                section_id: Some(s.id.clone()),
                section_title: s.title.clone(),
                est_minutes: per_part,
                part,
                parts,
            });
            used_content[cursor] += per_part;
            if part == 1 {
                first_seen.push((cursor, s.id.clone(), s.title.clone()));
            }
            placed_any = true;
            if used_content[cursor] >= content_cap_per_session {
                cursor += 1;
            }
        }
        if !placed_any {
            dropped.push(Dropped {
                section_id: s.id.clone(),
                title: s.title.clone(),
                est_minutes: est,
            });
        }
    }

    // ── Spaced review + retrieval practice ──────────────────────────────────
    let wants_quiz = req.blocks.iter().any(|b| b == "quiz");
    let wants_cards = req.blocks.iter().any(|b| b == "flashcard");
    let wants_recall = req.blocks.iter().any(|b| b == "recall");

    for (seen_at, id, title) in &first_seen {
        for off in &req.review_offsets {
            let target = seen_at + (*off as usize);
            if target >= sessions.len() {
                continue;
            }
            // Slide forward to a session that still has review budget, so a
            // busy day does not silently swallow the review.
            let mut t = target;
            let cost = 4;
            while t < sessions.len() && used_review[t] + cost > review_cap_per_session {
                t += 1;
            }
            if t >= sessions.len() {
                continue;
            }
            sessions[t].items.push(PlannedItem {
                kind: if wants_cards { "flashcard" } else { "review" }.into(),
                section_id: Some(id.clone()),
                section_title: title.clone(),
                est_minutes: cost,
                part: 1,
                parts: 1,
            });
            used_review[t] += cost;
        }
    }

    // Every session that taught something new ends with retrieval practice.
    for (i, s) in sessions.iter_mut().enumerate() {
        let taught: Vec<String> = s
            .items
            .iter()
            .filter(|it| it.kind == "read")
            .map(|it| it.section_title.clone())
            .collect();
        if taught.is_empty() {
            continue;
        }
        let left = review_cap_per_session - used_review[i];
        if wants_recall && left >= 8 {
            s.items.push(PlannedItem {
                kind: "recall".into(),
                section_id: None,
                section_title: taught[0].clone(),
                est_minutes: 4,
                part: 1,
                parts: 1,
            });
            used_review[i] += 4;
        }
        if wants_quiz && review_cap_per_session - used_review[i] >= 4 {
            s.items.push(PlannedItem {
                kind: "quiz".into(),
                section_id: None,
                section_title: taught[0].clone(),
                est_minutes: 5,
                part: 1,
                parts: 1,
            });
            used_review[i] += 5;
        }
    }

    // ── Titles, totals, pruning ─────────────────────────────────────────────
    let mut kept: Vec<PlannedSession> = Vec::new();
    for mut s in sessions.into_iter() {
        if s.items.is_empty() {
            continue;
        }
        s.minutes = s.items.iter().map(|i| i.est_minutes).sum();
        s.title = session_title(&s.items);
        s.ord = kept.len() as i64;
        kept.push(s);
    }

    // ── Feasibility ─────────────────────────────────────────────────────────
    let feasible = dropped.is_empty();
    let mut options = Vec::new();
    if !feasible {
        let need_sessions = ((total_est as f64) / (content_cap_per_session as f64)).ceil() as i64;
        let need_min = ((total_est as f64) / (days as f64) / ratio).ceil() as i64;
        options.push(format!(
            "giãn ra {need_sessions} buổi (đang đặt {days}) với {min_per_day} phút/buổi"
        ));
        options.push(format!(
            "giữ {days} buổi nhưng học {need_min} phút/buổi (đang đặt {min_per_day})"
        ));
        options.push(format!(
            "giữ nguyên nhịp và bỏ {} mục cuối — xem danh sách `dropped`",
            dropped.len()
        ));
        notes.push(format!(
            "cần {total_est} phút nội dung nhưng ngân sách chỉ có {content_budget} phút \
             ({content_cap_per_session} phút nội dung × {days} buổi)"
        ));
    }

    PlanPreview {
        feasible,
        sessions: kept,
        total_est_minutes: total_est,
        content_budget_minutes: content_budget,
        budget_minutes: budget,
        span_days,
        dropped,
        options,
        notes,
    }
}

fn session_title(items: &[PlannedItem]) -> String {
    let read: Vec<&PlannedItem> = items.iter().filter(|i| i.kind == "read").collect();
    match read.len() {
        0 => "Ôn tập".to_string(),
        1 => {
            let it = read[0];
            if it.parts > 1 {
                format!("{} (phần {}/{})", it.section_title, it.part, it.parts)
            } else {
                it.section_title.clone()
            }
        }
        n => format!("{} +{}", read[0].section_title, n - 1),
    }
}

pub fn parse_weekdays(raw: &str) -> Vec<u32> {
    raw.split(',')
        .filter_map(|s| s.trim().parse::<u32>().ok())
        .filter(|d| (1..=7).contains(d))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(id: &str, ord: i64, title: &str, est: i64, prereq: &[&str]) -> SectionRow {
        SectionRow {
            id: id.into(),
            doc_id: "d1".into(),
            ord,
            title: title.into(),
            level: 1,
            char_start: 0,
            char_end: 100,
            summary: None,
            key_points: vec![],
            difficulty: 3,
            est_minutes: est,
            prereq: prereq.iter().map(|s| s.to_string()).collect(),
            enriched_at: None,
        }
    }

    fn req(days: i64, min_per_day: i64) -> PlanRequest {
        PlanRequest {
            start_date: NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(), // a Monday
            days,
            min_per_day,
            weekdays: vec![1, 2, 3, 4, 5, 6, 7],
            slot_hm: "20:00".into(),
            review_offsets: vec![1, 3, 7],
            blocks: vec!["read".into(), "flashcard".into(), "quiz".into()],
            content_ratio: 0.7,
        }
    }

    #[test]
    fn session_dates_honour_the_selected_weekdays() {
        let start = NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(); // Monday
        let d = session_dates(start, 4, &[1, 3]); // Mon + Wed only
        assert_eq!(d.len(), 4);
        for x in &d {
            assert!([1, 3].contains(&x.weekday().number_from_monday()));
        }
        assert_eq!(d[1], NaiveDate::from_ymd_opt(2026, 8, 5).unwrap());
    }

    #[test]
    fn an_empty_weekday_list_means_every_day_rather_than_no_days() {
        let start = NaiveDate::from_ymd_opt(2026, 8, 3).unwrap();
        assert_eq!(session_dates(start, 3, &[]).len(), 3);
    }

    #[test]
    fn prerequisites_are_respected() {
        let sections = vec![
            sec("b", 0, "Nâng cao", 20, &["a"]),
            sec("a", 1, "Cơ bản", 20, &[]),
        ];
        let (order, cycle) = order_sections(&sections);
        assert!(!cycle);
        assert_eq!(order, vec![1, 0], "the prerequisite must come first");
    }

    #[test]
    fn a_prerequisite_cycle_does_not_lose_sections() {
        let sections = vec![sec("a", 0, "A", 10, &["b"]), sec("b", 1, "B", 10, &["a"])];
        let (order, cycle) = order_sections(&sections);
        assert!(cycle, "the cycle must be reported");
        assert_eq!(order.len(), 2, "no section may vanish because of a cycle");
    }

    #[test]
    fn independent_documents_are_interleaved_not_finished_one_at_a_time() {
        let mut a1 = sec("a1", 0, "A1", 10, &[]);
        let mut a2 = sec("a2", 1, "A2", 10, &[]);
        a1.doc_id = "A".into();
        a2.doc_id = "A".into();
        let mut b1 = sec("b1", 2, "B1", 10, &[]);
        let mut b2 = sec("b2", 3, "B2", 10, &[]);
        b1.doc_id = "B".into();
        b2.doc_id = "B".into();
        let sections = vec![a1, a2, b1, b2];
        let (order, _) = order_sections(&sections);
        let docs: Vec<&str> = order.iter().map(|i| sections[*i].doc_id.as_str()).collect();
        assert_eq!(docs, vec!["A", "B", "A", "B"]);
    }

    #[test]
    fn a_plan_that_fits_covers_every_section_and_reports_feasible() {
        let sections = vec![
            sec("a", 0, "A", 15, &[]),
            sec("b", 1, "B", 15, &[]),
            sec("c", 2, "C", 15, &[]),
        ];
        let p = build(&sections, &req(10, 30));
        assert!(p.feasible);
        assert!(p.dropped.is_empty());
        let covered: HashSet<&str> = p
            .sessions
            .iter()
            .flat_map(|s| s.items.iter())
            .filter(|i| i.kind == "read")
            .filter_map(|i| i.section_id.as_deref())
            .collect();
        assert_eq!(covered.len(), 3);
    }

    #[test]
    fn material_that_does_not_fit_is_reported_with_three_ways_out() {
        let sections: Vec<SectionRow> = (0..20)
            .map(|i| sec(&format!("s{i}"), i, &format!("Mục {i}"), 30, &[]))
            .collect();
        let p = build(&sections, &req(3, 30));
        assert!(!p.feasible);
        assert!(!p.dropped.is_empty(), "what didn't fit must be named");
        assert_eq!(p.options.len(), 3, "the learner gets choices, not a silent cut");
        assert!(p.options.iter().any(|o| o.contains("buổi")));
    }

    #[test]
    fn no_session_is_planned_longer_than_the_learner_asked_for() {
        let sections: Vec<SectionRow> = (0..12)
            .map(|i| sec(&format!("s{i}"), i, &format!("Mục {i}"), 12, &[]))
            .collect();
        let p = build(&sections, &req(14, 30));
        for s in &p.sessions {
            assert!(
                s.minutes <= 30 + 5,
                "session {} planned {} minutes for a 30-minute budget",
                s.ord,
                s.minutes
            );
        }
    }

    #[test]
    fn a_section_bigger_than_one_session_is_split_into_labelled_parts() {
        let sections = vec![sec("big", 0, "Chương dài", 90, &[])];
        let p = build(&sections, &req(10, 30));
        let parts: Vec<&PlannedItem> = p
            .sessions
            .iter()
            .flat_map(|s| s.items.iter())
            .filter(|i| i.kind == "read")
            .collect();
        assert!(parts.len() >= 4, "90 minutes cannot fit in one 21-minute slot");
        assert_eq!(parts[0].parts, parts.len() as i64);
        assert_eq!(parts[0].part, 1);
        assert!(p.sessions[0].title.contains("phần 1/"));
    }

    #[test]
    fn every_section_gets_spaced_reviews_after_it_is_first_taught() {
        let sections = vec![sec("a", 0, "A", 10, &[])];
        let p = build(&sections, &req(20, 30));
        let reviews: Vec<(i64, &PlannedItem)> = p
            .sessions
            .iter()
            .flat_map(|s| s.items.iter().map(move |i| (s.ord, i)))
            .filter(|(_, i)| i.kind == "flashcard")
            .collect();
        assert_eq!(reviews.len(), 3, "one per review offset");
        let ords: Vec<i64> = reviews.iter().map(|(o, _)| *o).collect();
        assert!(ords.windows(2).all(|w| w[0] < w[1]), "reviews must spread out");
    }

    #[test]
    fn a_session_that_teaches_something_ends_with_retrieval_practice() {
        let sections = vec![sec("a", 0, "A", 10, &[])];
        let p = build(&sections, &req(5, 30));
        assert_eq!(p.sessions[0].items.last().unwrap().kind, "quiz");
    }

    #[test]
    fn a_template_without_quizzes_does_not_get_them() {
        let sections = vec![sec("a", 0, "A", 10, &[])];
        let mut r = req(5, 30);
        r.blocks = vec!["read".into()];
        let p = build(&sections, &r);
        assert!(p
            .sessions
            .iter()
            .flat_map(|s| s.items.iter())
            .all(|i| i.kind != "quiz"));
    }

    #[test]
    fn empty_sessions_are_dropped_from_the_plan() {
        let sections = vec![sec("a", 0, "A", 10, &[])];
        let p = build(&sections, &req(30, 30));
        assert!(p.sessions.len() < 30, "no empty days on the calendar");
        assert!(p.sessions.iter().all(|s| !s.items.is_empty()));
    }

    #[test]
    fn the_five_builtin_templates_are_well_formed() {
        assert_eq!(BUILTIN_TEMPLATES.len(), 5);
        for t in BUILTIN_TEMPLATES {
            assert!(t.days > 0 && t.min_per_day > 0, "{}", t.key);
            assert!(!t.review_offsets.is_empty(), "{}", t.key);
            assert!(t.content_ratio > 0.3 && t.content_ratio <= 1.0, "{}", t.key);
            assert!(
                t.review_offsets.windows(2).all(|w| w[0] < w[1]),
                "{} review offsets must widen",
                t.key
            );
        }
    }

    #[test]
    fn weekday_parsing_ignores_junk() {
        assert_eq!(parse_weekdays("1,3,x,9,7"), vec![1, 3, 7]);
    }
}
