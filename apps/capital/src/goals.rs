//! Goal engine (mục tiêu & kế hoạch): measure each goal against the live
//! ledger, evaluate progress vs. elapsed time (on_track / behind / at_risk),
//! compute the pace needed to make the deadline, and generate a deterministic
//! fallback plan when the AI planner is unavailable. Pure functions over
//! [`Snapshot`] + goal rows → unit-testable without I/O.

use crate::finance::{add_months, round2};
use crate::insight::Snapshot;
use serde_json::{json, Value};

/// Goal kinds and their measured metric:
/// * `reduce_debt`    — tổng dư nợ (hoặc dư nợ 1 nguồn nếu có source_id) về ≤ target (giảm)
/// * `payoff_source`  — tất toán 1 nguồn: dư nợ nguồn về 0 (giảm)
/// * `raise_equity`   — vốn chủ đã góp ≥ target (tăng)
/// * `raise_funding`  — tổng vốn đã nhận về (giải ngân mọi nguồn) ≥ target (tăng)
/// * `build_reserve`  — nguồn còn rút được ≥ target (tăng)
pub fn is_decreasing(kind: &str) -> bool {
    matches!(kind, "reduce_debt" | "payoff_source")
}

/// The current value of a goal's metric on a snapshot.
pub fn metric(snap: &Snapshot, kind: &str, source_id: Option<i64>) -> f64 {
    let active: Vec<_> = snap
        .sources
        .iter()
        .filter(|s| s.status == "active")
        .collect();
    match kind {
        "reduce_debt" => match source_id {
            Some(sid) => active
                .iter()
                .find(|s| s.id == sid)
                .map(|s| s.outstanding())
                .unwrap_or(0.0),
            None => round2(
                active
                    .iter()
                    .filter(|s| crate::finance::is_debt_kind(&s.kind))
                    .map(|s| s.outstanding())
                    .sum(),
            ),
        },
        "payoff_source" => source_id
            .and_then(|sid| active.iter().find(|s| s.id == sid))
            .map(|s| s.outstanding())
            .unwrap_or(0.0),
        "raise_equity" => round2(
            active
                .iter()
                .filter(|s| !crate::finance::is_debt_kind(&s.kind))
                .map(|s| s.outstanding().max(0.0))
                .sum(),
        ),
        "raise_funding" => round2(active.iter().map(|s| s.disbursed).sum()),
        "build_reserve" => round2(active.iter().map(|s| s.available()).sum()),
        _ => 0.0,
    }
}

/// Fractional years between two YYYY-MM(-DD) dates, month granularity.
fn ym_diff_months(from: &str, to: &str) -> f64 {
    let parse = |d: &str| -> Option<(f64, f64)> {
        if d.len() < 7 {
            return None;
        }
        Some((d[..4].parse().ok()?, d[5..7].parse().ok()?))
    };
    match (parse(from), parse(to)) {
        (Some((fy, fm)), Some((ty, tm))) => (ty - fy) * 12.0 + (tm - fm),
        _ => 0.0,
    }
}

/// Evaluate one goal row (as stored in the DB) against a snapshot.
/// Returns the goal merged with progress fields.
pub fn evaluate_goal(snap: &Snapshot, goal: &Value) -> Value {
    let kind = goal["kind"].as_str().unwrap_or("");
    let source_id = goal["source_id"].as_i64();
    let target = goal["target_amount"].as_f64().unwrap_or(0.0);
    let baseline = goal["baseline"].as_f64().unwrap_or(0.0);
    let deadline = goal["deadline"].as_str().unwrap_or("");
    let created = goal["created_date"].as_str().unwrap_or("");
    let status = goal["status"].as_str().unwrap_or("active");

    let current = metric(snap, kind, source_id);
    let down = is_decreasing(kind);

    // Progress: how much of baseline→target distance is covered (0–100).
    let span = if down {
        baseline - target
    } else {
        target - baseline
    };
    let covered = if down {
        baseline - current
    } else {
        current - baseline
    };
    let progress = if span <= 0.0 {
        // Target already met at creation (or degenerate input).
        if (down && current <= target) || (!down && current >= target) {
            100.0
        } else {
            0.0
        }
    } else {
        (covered / span * 100.0).clamp(0.0, 100.0)
    };
    let achieved = if down {
        current <= target
    } else {
        current >= target
    };

    // Time: elapsed share of created→deadline window.
    let total_months = ym_diff_months(created, deadline);
    let gone_months = ym_diff_months(created, &snap.today);
    let elapsed = if total_months > 0.0 {
        (gone_months / total_months * 100.0).clamp(0.0, 100.0)
    } else {
        0.0
    };

    // Pace: how much per month is still needed to hit the deadline.
    let remaining = if down {
        (current - target).max(0.0)
    } else {
        (target - current).max(0.0)
    };
    let months_left = ym_diff_months(&snap.today, deadline).max(0.0);
    let pace_per_month = if remaining > 0.0 && months_left >= 1.0 {
        round2(remaining / months_left)
    } else {
        remaining
    };

    let deadline_passed = !deadline.is_empty() && deadline < snap.today.as_str();
    let eval_status = if status != "active" {
        status.to_string()
    } else if achieved {
        "achieved".into()
    } else if deadline_passed {
        "overdue".into()
    } else if deadline.is_empty() || total_months <= 0.0 {
        "in_progress".into()
    } else if progress + 10.0 >= elapsed {
        "on_track".into()
    } else if progress + 25.0 >= elapsed {
        "behind".into()
    } else {
        "at_risk".into()
    };

    let mut out = goal.clone();
    let obj = out.as_object_mut().unwrap();
    obj.insert("current".into(), json!(round2(current)));
    obj.insert("progress_pct".into(), json!(round2(progress)));
    obj.insert("elapsed_pct".into(), json!(round2(elapsed)));
    obj.insert("remaining".into(), json!(round2(remaining)));
    obj.insert("months_left".into(), json!(months_left));
    obj.insert("pace_per_month".into(), json!(pace_per_month));
    obj.insert("eval_status".into(), json!(eval_status));
    out
}

/// Deterministic fallback plan: monthly (or quarterly for long horizons)
/// milestones splitting the remaining distance evenly. Used when the AI
/// planner is unavailable or returns garbage; steps are tagged source="auto".
pub fn fallback_plan(snap: &Snapshot, goal_eval: &Value) -> Vec<Value> {
    let kind = goal_eval["kind"].as_str().unwrap_or("");
    let remaining = goal_eval["remaining"].as_f64().unwrap_or(0.0);
    if remaining <= 0.0 {
        return vec![json!({
            "title": "Mục tiêu đã đạt — đánh dấu hoàn thành",
            "due_date": snap.today,
            "amount": 0.0,
        })];
    }
    let months_left = goal_eval["months_left"].as_f64().unwrap_or(0.0).max(1.0) as u32;
    // ≤12 steps: monthly when the horizon is short, quarterly when long.
    let step_months: u32 = if months_left > 12 { 3 } else { 1 };
    let n_steps = (months_left / step_months).max(1);
    let per_step = round2(remaining / n_steps as f64);

    let verb = match kind {
        "reduce_debt" | "payoff_source" => "Trả thêm",
        "raise_equity" => "Góp thêm vốn chủ",
        "raise_funding" => "Huy động thêm",
        "build_reserve" => "Tăng dự phòng thêm",
        _ => "Xử lý",
    };
    (1..=n_steps)
        .map(|k| {
            let amount = if k == n_steps { round2(remaining - per_step * (n_steps - 1) as f64) } else { per_step };
            json!({
                "title": format!("{verb} ~{} (mốc {}/{})", crate::insight::fmt_money_vn(amount), k, n_steps),
                "due_date": add_months(&snap.today, step_months * k),
                "amount": amount,
            })
        })
        .collect()
}

/// Parse the AI planner's reply into steps: expects a JSON array of
/// `{title, due_date?, amount?}` somewhere in the text. Returns None when
/// nothing usable is found so the caller can fall back to [`fallback_plan`].
pub fn parse_ai_plan(text: &str) -> Option<Vec<Value>> {
    let start = text.find('[')?;
    let end = text.rfind(']')?;
    if end <= start {
        return None;
    }
    let arr: Vec<Value> = serde_json::from_str(&text[start..=end]).ok()?;
    let steps: Vec<Value> = arr
        .into_iter()
        .filter_map(|s| {
            let title = s.get("title").and_then(|x| x.as_str())?.trim().to_string();
            if title.is_empty() {
                return None;
            }
            Some(json!({
                "title": title,
                "due_date": s.get("due_date").and_then(|x| x.as_str()).unwrap_or(""),
                "amount": s.get("amount").and_then(|x| x.as_f64()).unwrap_or(0.0),
            }))
        })
        .take(8)
        .collect();
    if steps.is_empty() {
        None
    } else {
        Some(steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::SourceRow;

    fn src(id: i64, kind: &str, total: f64, rate: f64, disbursed: f64, repaid: f64) -> SourceRow {
        SourceRow {
            id,
            name: format!("S{id}"),
            kind: kind.into(),
            provider: String::new(),
            total_amount: total,
            currency: "VND".into(),
            interest_rate: rate,
            rate_type: "fixed".into(),
            start_date: String::new(),
            end_date: String::new(),
            status: "active".into(),
            note: String::new(),
            disbursed,
            repaid_principal: repaid,
            interest_paid: 0.0,
            fees_paid: 0.0,
        }
    }

    fn snap() -> Snapshot {
        Snapshot {
            today: "2026-07-27".into(),
            sources: vec![
                src(1, "equity", 1_000.0, 0.0, 800.0, 0.0),
                src(2, "bank_loan", 1_000.0, 9.0, 900.0, 300.0), // outstanding 600
            ],
            unpaid: vec![],
        }
    }

    fn goal(
        kind: &str,
        target: f64,
        baseline: f64,
        source_id: Option<i64>,
        deadline: &str,
        created: &str,
    ) -> Value {
        json!({
            "id": 1, "name": "g", "kind": kind, "target_amount": target, "baseline": baseline,
            "source_id": source_id, "deadline": deadline, "status": "active", "note": "",
            "created_date": created,
        })
    }

    #[test]
    fn metrics_per_kind() {
        let s = snap();
        assert_eq!(metric(&s, "reduce_debt", None), 600.0);
        assert_eq!(metric(&s, "reduce_debt", Some(2)), 600.0);
        assert_eq!(metric(&s, "payoff_source", Some(2)), 600.0);
        assert_eq!(metric(&s, "raise_equity", None), 800.0);
        assert_eq!(metric(&s, "raise_funding", None), 1_700.0);
        // equity available 200 + loan available 100 (total - disbursed)
        assert_eq!(metric(&s, "build_reserve", None), 300.0);
    }

    #[test]
    fn on_track_goal() {
        // Baseline debt 900 → target 0 by 2027-07; today debt 600 → progress 33%,
        // elapsed 0% (created this month) → on_track.
        let g = goal("reduce_debt", 0.0, 900.0, None, "2027-07-31", "2026-07-01");
        let e = evaluate_goal(&snap(), &g);
        assert_eq!(e["eval_status"], "on_track");
        assert_eq!(e["progress_pct"].as_f64().unwrap().round(), 33.0);
        assert_eq!(e["remaining"], 600.0);
        // 12 months left → 50/month.
        assert_eq!(e["pace_per_month"], 50.0);
    }

    #[test]
    fn behind_and_at_risk_and_overdue() {
        // Created a year ago, deadline next month, no progress at all.
        let g = goal("reduce_debt", 0.0, 600.0, None, "2026-08-31", "2025-08-01");
        let e = evaluate_goal(&snap(), &g);
        assert_eq!(e["eval_status"], "at_risk", "{e}");
        // Deadline already passed.
        let g2 = goal("reduce_debt", 0.0, 600.0, None, "2026-06-30", "2025-08-01");
        assert_eq!(evaluate_goal(&snap(), &g2)["eval_status"], "overdue");
        // Small slip → behind: progress 33%, elapsed ~45% (created 2025-09, 22-month window).
        let g3 = goal("reduce_debt", 0.0, 900.0, None, "2027-07-01", "2025-09-01");
        let e3 = evaluate_goal(&snap(), &g3);
        assert_eq!(e3["eval_status"], "behind", "{e3}");
    }

    #[test]
    fn achieved_goal() {
        let g = goal(
            "raise_equity",
            700.0,
            100.0,
            None,
            "2027-01-01",
            "2026-01-01",
        );
        let e = evaluate_goal(&snap(), &g);
        assert_eq!(e["eval_status"], "achieved");
        assert_eq!(e["progress_pct"], 100.0);
        assert_eq!(e["remaining"], 0.0);
    }

    #[test]
    fn fallback_plan_splits_remaining() {
        let g = goal(
            "payoff_source",
            0.0,
            900.0,
            Some(2),
            "2027-01-27",
            "2026-07-01",
        );
        let e = evaluate_goal(&snap(), &g);
        let plan = fallback_plan(&snap(), &e);
        assert_eq!(plan.len(), 6, "{plan:?}"); // 6 months → 6 monthly steps
        let total: f64 = plan.iter().map(|s| s["amount"].as_f64().unwrap()).sum();
        assert_eq!(crate::finance::round2(total), 600.0);
        assert!(plan[0]["title"].as_str().unwrap().starts_with("Trả thêm"));
        // Long horizon → quarterly.
        let g2 = goal(
            "raise_funding",
            5_000.0,
            1_700.0,
            None,
            "2028-07-27",
            "2026-07-01",
        );
        let plan2 = fallback_plan(&snap(), &evaluate_goal(&snap(), &g2));
        assert!(plan2.len() <= 12 && plan2.len() >= 8, "{}", plan2.len());
    }

    #[test]
    fn ai_plan_parsing() {
        let ok = r#"Kế hoạch: [{"title":"Trả 100tr","due_date":"2026-09-01","amount":100},
            {"title":"Đảo nợ","due_date":"2026-10-01"}] xong."#;
        let steps = parse_ai_plan(ok).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1]["amount"], 0.0);
        assert!(parse_ai_plan("không có json").is_none());
        assert!(parse_ai_plan("[]").is_none());
        assert!(parse_ai_plan("[{\"nope\":1}]").is_none());
    }
}
