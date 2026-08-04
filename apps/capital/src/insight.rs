//! Smart layer for the Capital app: a deterministic, explainable **health
//! evaluation** (đánh giá sức khoẻ vốn — rule engine, no LLM) and a **what-if
//! simulation** (mô phỏng vay mới / trả nợ trước hạn) for decision support.
//! Both operate on a [`Snapshot`] so simulations can evaluate hypothetical
//! states without touching the DB, and everything stays unit-testable.
//!
//! The AI analysis ([`crate::llm`]) receives these findings as ground truth —
//! the LLM narrates and prioritizes, the numbers come from here.

use crate::db::{Db, SourceRow};
use crate::finance::{self, add_months, generate_schedule, is_debt_kind, round2};
use serde_json::{json, Value};

/// Everything the rule engine needs, detached from the DB.
#[derive(Clone)]
pub struct Snapshot {
    pub today: String,
    pub sources: Vec<SourceRow>,
    /// Unpaid installments: (due_date, total_due, source_id).
    pub unpaid: Vec<(String, f64, i64)>,
}

impl Snapshot {
    pub fn from_db(db: &Db, today: &str) -> Self {
        let sources = db.list_sources(None);
        let unpaid = db
            .list_schedule(None, None, today, 5000)
            .into_iter()
            .filter(|it| it["status"] != "paid")
            .map(|it| {
                (
                    it["due_date"].as_str().unwrap_or("").to_string(),
                    it["total_due"].as_f64().unwrap_or(0.0),
                    it["source_id"].as_i64().unwrap_or(0),
                )
            })
            .collect();
        Self {
            today: today.to_string(),
            sources,
            unpaid,
        }
    }

    fn active(&self) -> Vec<&SourceRow> {
        self.sources
            .iter()
            .filter(|s| s.status == "active")
            .collect()
    }

    fn debt_outstanding(&self) -> f64 {
        round2(
            self.active()
                .iter()
                .filter(|s| is_debt_kind(&s.kind))
                .map(|s| s.outstanding())
                .sum(),
        )
    }

    fn equity_in(&self) -> f64 {
        round2(
            self.active()
                .iter()
                .filter(|s| !is_debt_kind(&s.kind))
                .map(|s| s.outstanding().max(0.0))
                .sum(),
        )
    }

    fn available(&self) -> f64 {
        round2(self.active().iter().map(|s| s.available()).sum())
    }

    fn weighted_rate(&self) -> f64 {
        let (mut num, mut den) = (0.0, 0.0);
        for s in self.active() {
            if is_debt_kind(&s.kind) {
                let out = s.outstanding();
                num += out * s.interest_rate;
                den += out;
            }
        }
        if den > 0.0 {
            round2(num / den)
        } else {
            0.0
        }
    }

    fn de_ratio(&self) -> Option<f64> {
        let eq = self.equity_in();
        if eq > 0.0 {
            Some(round2(self.debt_outstanding() / eq))
        } else {
            None
        }
    }

    /// Sum of unpaid installments due within `days` of today (including overdue).
    fn due_within_days(&self, days: u32) -> f64 {
        // Month-granular horizon is enough here; 30 ≈ 1 month, 90 ≈ 3 months.
        let horizon = add_months(&self.today, days.div_ceil(30));
        round2(
            self.unpaid
                .iter()
                .filter(|(d, _, _)| d.as_str() <= horizon.as_str())
                .map(|(_, v, _)| v)
                .sum(),
        )
    }

    /// Aggregate unpaid obligations by month for the next 12 months.
    pub fn monthly_due_12m(&self) -> Vec<Value> {
        let end = add_months(&self.today, 12);
        let mut months: Vec<(String, f64)> = Vec::new();
        for (due, total, _) in &self.unpaid {
            if due.as_str() > end.as_str() || due.len() < 7 {
                continue;
            }
            let ym = due[..7].to_string();
            match months.iter_mut().find(|(m, _)| *m == ym) {
                Some((_, v)) => *v += total,
                None => months.push((ym, *total)),
            }
        }
        months.sort_by(|a, b| a.0.cmp(&b.0));
        months
            .into_iter()
            .map(|(m, v)| json!({ "month": m, "total_due": round2(v) }))
            .collect()
    }
}

struct Check {
    severity: &'static str, // good | warn | crit
    deduct: i64,
    title: String,
    detail: String,
}

/// Run the rule engine: score 0–100 + explainable findings.
pub fn evaluate(snap: &Snapshot) -> Value {
    let mut checks: Vec<Check> = Vec::new();
    let mut push = |severity: &'static str, deduct: i64, title: String, detail: String| {
        checks.push(Check {
            severity,
            deduct,
            title,
            detail,
        });
    };

    let debt = snap.debt_outstanding();
    let equity = snap.equity_in();
    let available = snap.available();

    // 1. Kỷ luật trả nợ — overdue installments.
    let overdue: Vec<&(String, f64, i64)> = snap
        .unpaid
        .iter()
        .filter(|(d, _, _)| d.as_str() < snap.today.as_str())
        .collect();
    if overdue.is_empty() {
        push(
            "good",
            0,
            "Không có kỳ trả nợ quá hạn".into(),
            "Kỷ luật thanh toán đang được giữ.".into(),
        );
    } else {
        let total: f64 = overdue.iter().map(|(_, v, _)| v).sum();
        push(
            "crit",
            25,
            format!("{} kỳ trả nợ QUÁ HẠN — {}", overdue.len(), fmt_money(total)),
            "Ưu tiên xử lý ngay: quá hạn phát sinh lãi phạt và ảnh hưởng lịch sử tín dụng.".into(),
        );
    }

    // 2. Thanh khoản 30 ngày — nghĩa vụ sắp tới so với nguồn còn rút được.
    let due30 = snap.due_within_days(30);
    if due30 > 0.0 {
        if due30 > available {
            push(
                "crit",
                20,
                format!("Nghĩa vụ 30 ngày ({}) VƯỢT nguồn còn rút được ({})", fmt_money(due30), fmt_money(available)),
                "Cần thu xếp dòng tiền ngoài sổ (doanh thu, góp thêm vốn) hoặc đàm phán giãn kỳ trả.".into(),
            );
        } else if due30 > available * 0.5 {
            push(
                "warn",
                10,
                format!(
                    "Nghĩa vụ 30 ngày ({}) chiếm hơn nửa nguồn còn rút được",
                    fmt_money(due30)
                ),
                "Thanh khoản còn nhưng mỏng — theo dõi sát dòng tiền vào.".into(),
            );
        } else {
            push(
                "good",
                0,
                "Thanh khoản 30 ngày ổn".into(),
                format!("Phải trả {} — trong khả năng.", fmt_money(due30)),
            );
        }
    }

    // 3. Đòn bẩy D/E.
    match snap.de_ratio() {
        Some(de) if de > 2.0 => push(
            "crit",
            15,
            format!("Đòn bẩy cao: D/E = {de}"),
            "Nợ gấp hơn 2 lần vốn chủ — rủi ro cao nếu dòng tiền kinh doanh chững lại.".into(),
        ),
        Some(de) if de > 1.0 => push(
            "warn",
            8,
            format!("Đòn bẩy trung bình: D/E = {de}"),
            "Nợ đã vượt vốn chủ; cân nhắc trước khi vay thêm.".into(),
        ),
        Some(de) => push("good", 0, format!("Đòn bẩy an toàn: D/E = {de}"), "Cơ cấu nợ/vốn chủ lành mạnh.".into()),
        None if debt > 0.0 => push(
            "warn",
            8,
            "Chưa ghi nhận vốn chủ sở hữu".into(),
            "Toàn bộ vốn là nợ vay (hoặc chưa ghi sổ vốn chủ) — thêm nguồn equity để chỉ số D/E có nghĩa.".into(),
        ),
        None => {}
    }

    // 4. Chi phí vốn vay.
    let wrate = snap.weighted_rate();
    if debt > 0.0 {
        if wrate > 15.0 {
            push(
                "crit",
                10,
                format!("Lãi suất bình quân rất cao: {wrate}%/năm"),
                "Ưu tiên đảo nợ / trả trước các khoản đắt nhất.".into(),
            );
        } else if wrate > 10.0 {
            push(
                "warn",
                5,
                format!("Lãi suất bình quân {wrate}%/năm"),
                "Có dư địa đàm phán lại hoặc đảo sang nguồn rẻ hơn.".into(),
            );
        } else {
            push(
                "good",
                0,
                format!("Chi phí vốn hợp lý: {wrate}%/năm"),
                "Lãi suất bình quân gia quyền ở mức tốt.".into(),
            );
        }
        // 4b. Khoản vay đắt bất thường so với mặt bằng.
        let expensive: Vec<String> = snap
            .active()
            .iter()
            .filter(|s| {
                is_debt_kind(&s.kind) && s.outstanding() > 0.0 && s.interest_rate >= wrate + 3.0
            })
            .map(|s| {
                format!(
                    "{} ({}%/năm, dư nợ {})",
                    s.name,
                    s.interest_rate,
                    fmt_money(s.outstanding())
                )
            })
            .collect();
        if !expensive.is_empty() {
            push(
                "warn",
                5,
                "Có khoản vay đắt hơn mặt bằng ≥3 điểm %".into(),
                format!("Ứng viên trả trước/đảo nợ: {}.", expensive.join("; ")),
            );
        }
    }

    // 5. Tập trung nguồn — một nguồn nợ chiếm quá nửa tổng dư nợ.
    if debt > 0.0 {
        if let Some(biggest) = snap
            .active()
            .iter()
            .filter(|s| is_debt_kind(&s.kind))
            .max_by(|a, b| a.outstanding().total_cmp(&b.outstanding()))
        {
            let share = biggest.outstanding() / debt;
            let debt_sources = snap
                .active()
                .iter()
                .filter(|s| is_debt_kind(&s.kind) && s.outstanding() > 0.0)
                .count();
            if share > 0.8 && debt_sources > 1 {
                push(
                    "warn",
                    5,
                    format!(
                        "Dư nợ tập trung {}% vào \"{}\"",
                        (share * 100.0).round(),
                        biggest.name
                    ),
                    "Phụ thuộc một chủ nợ; nếu nguồn này siết hạn mức sẽ khó xoay.".into(),
                );
            }
        }
    }

    // 6. Áp lực đáo hạn ≤ 90 ngày.
    let horizon90 = add_months(&snap.today, 3);
    let maturing: Vec<String> = snap
        .active()
        .iter()
        .filter(|s| {
            is_debt_kind(&s.kind)
                && s.outstanding() > 0.0
                && !s.end_date.is_empty()
                && s.end_date.as_str() <= horizon90.as_str()
        })
        .map(|s| {
            format!(
                "{} (đáo hạn {}, dư nợ {})",
                s.name,
                s.end_date,
                fmt_money(s.outstanding())
            )
        })
        .collect();
    if !maturing.is_empty() {
        push(
            "crit",
            10,
            format!(
                "{} nguồn vay đáo hạn trong 90 ngày mà còn dư nợ",
                maturing.len()
            ),
            format!(
                "{}. Chuẩn bị phương án tất toán hoặc gia hạn sớm.",
                maturing.join("; ")
            ),
        );
    }

    // 7. Hạn mức tín dụng gần cạn (mất room dự phòng).
    let tight: Vec<String> = snap
        .active()
        .iter()
        .filter(|s| {
            s.kind == "credit_line"
                && s.total_amount > 0.0
                && s.outstanding() / s.total_amount > 0.9
        })
        .map(|s| {
            format!(
                "{} ({}%)",
                s.name,
                ((s.outstanding() / s.total_amount) * 100.0).round()
            )
        })
        .collect();
    if !tight.is_empty() {
        push(
            "warn",
            5,
            "Hạn mức tín dụng dùng trên 90%".into(),
            format!("{} — room dự phòng thanh khoản gần cạn.", tight.join("; ")),
        );
    }

    // 8. Nợ có dư mà chưa có lịch trả.
    let unscheduled: Vec<String> = snap
        .active()
        .iter()
        .filter(|s| {
            is_debt_kind(&s.kind)
                && s.outstanding() > 0.0
                && !snap.unpaid.iter().any(|(_, _, sid)| *sid == s.id)
        })
        .map(|s| s.name.clone())
        .collect();
    if !unscheduled.is_empty() {
        push(
            "warn",
            5,
            "Khoản vay chưa có lịch trả nợ".into(),
            format!(
                "{} — sinh lịch để app nhắc kỳ hạn và tính đúng nghĩa vụ sắp tới.",
                unscheduled.join(", ")
            ),
        );
    }

    let score = (100 - checks.iter().map(|c| c.deduct).sum::<i64>()).clamp(0, 100);
    let (grade, label) = match score {
        85..=100 => ("A", "Khoẻ mạnh"),
        70..=84 => ("B", "Ổn định"),
        50..=69 => ("C", "Cần chú ý"),
        _ => ("D", "Rủi ro cao"),
    };

    json!({
        "score": score,
        "grade": grade,
        "label": label,
        "today": snap.today,
        "metrics": {
            "debt_outstanding": debt,
            "equity_in": equity,
            "available": available,
            "weighted_debt_rate": wrate,
            "de_ratio": snap.de_ratio(),
            "due_30d": due30,
            "due_90d": snap.due_within_days(90),
        },
        "findings": checks.iter().map(|c| json!({
            "severity": c.severity,
            "title": c.title,
            "detail": c.detail,
        })).collect::<Vec<_>>(),
    })
}

/// Compact Vietnamese money for finding/plan texts: 1.234.567 → "1,2 triệu".
pub fn fmt_money_vn(v: f64) -> String {
    let abs = v.abs();
    if abs >= 1e9 {
        format!("{:.2} tỷ", v / 1e9)
    } else if abs >= 1e6 {
        format!("{:.1} triệu", v / 1e6)
    } else {
        format!("{:.0}", v)
    }
}

fn fmt_money(v: f64) -> String {
    fmt_money_vn(v)
}

/// Simulation parameters, deserialized in `api.rs`.
pub struct NewLoanParams {
    pub amount: f64,
    pub annual_rate: f64,
    pub periods: u32,
    pub method: String,
    pub freq_months: u32,
}

/// What-if: take on a new loan. Returns before/after metrics + score delta and
/// a preview of the hypothetical schedule. Nothing is written to the DB.
pub fn simulate_new_loan(snap: &Snapshot, p: &NewLoanParams) -> Value {
    if p.amount <= 0.0 || p.periods == 0 {
        return json!({ "error": "cần amount > 0 và periods ≥ 1" });
    }
    let method = if p.method.is_empty() {
        "annuity"
    } else {
        &p.method
    };
    let items = generate_schedule(
        method,
        p.amount,
        p.annual_rate,
        p.periods,
        &snap.today,
        p.freq_months.max(1),
    );
    let total_interest: f64 = round2(items.iter().map(|i| i.interest).sum());
    let first_payment = round2(
        items
            .first()
            .map(|i| i.principal + i.interest)
            .unwrap_or(0.0),
    );

    // Hypothetical snapshot: one synthetic bank_loan, fully disbursed.
    let mut after = snap.clone();
    after.sources.push(SourceRow {
        id: -1,
        name: "(mô phỏng) khoản vay mới".into(),
        kind: "bank_loan".into(),
        provider: String::new(),
        total_amount: p.amount,
        currency: "VND".into(),
        interest_rate: p.annual_rate,
        rate_type: "fixed".into(),
        start_date: snap.today.clone(),
        end_date: String::new(),
        status: "active".into(),
        note: String::new(),
        disbursed: p.amount,
        repaid_principal: 0.0,
        interest_paid: 0.0,
        fees_paid: 0.0,
    });
    after.unpaid.extend(
        items
            .iter()
            .map(|i| (i.due_date.clone(), round2(i.principal + i.interest), -1)),
    );

    json!({
        "scenario": "new_loan",
        "params": { "amount": p.amount, "annual_rate": p.annual_rate, "periods": p.periods, "method": method, "freq_months": p.freq_months.max(1) },
        "loan": {
            "first_payment": first_payment,
            "total_interest": total_interest,
            "total_cost": round2(p.amount + total_interest),
            "schedule_preview": items.iter().take(3).map(|i| json!({
                "seq": i.seq, "due_date": i.due_date, "principal": i.principal, "interest": i.interest,
            })).collect::<Vec<_>>(),
        },
        "before": side_metrics(snap),
        "after": side_metrics(&after),
    })
}

/// What-if: repay `amount` of a source's principal early. Interest saved is a
/// SIMPLE estimate (amount × rate × remaining years to end_date, min 1 năm) —
/// labeled as such in the output.
pub fn simulate_early_repay(snap: &Snapshot, source_id: i64, amount: f64) -> Value {
    let Some(src) = snap
        .sources
        .iter()
        .find(|s| s.id == source_id && s.status == "active")
    else {
        return json!({ "error": format!("nguồn vốn #{source_id} không tồn tại hoặc không active") });
    };
    if !is_debt_kind(&src.kind) {
        return json!({ "error": "chỉ mô phỏng trả trước cho nguồn NỢ (vay/hạn mức/trái phiếu)" });
    }
    let repay = amount.min(src.outstanding());
    if repay <= 0.0 {
        return json!({ "error": "amount phải > 0 và nguồn phải còn dư nợ" });
    }

    // Remaining years until maturity (fallback 1 năm when no end_date).
    let years = if src.end_date.len() >= 7 {
        let ym = |d: &str| -> f64 {
            let y: f64 = d[..4].parse().unwrap_or(0.0);
            let m: f64 = d[5..7].parse().unwrap_or(1.0);
            y + m / 12.0
        };
        (ym(&src.end_date) - ym(&snap.today)).max(0.0)
    } else {
        1.0
    };
    let interest_saved = round2(repay * src.interest_rate / 100.0 * years.max(1.0 / 12.0));

    let mut after = snap.clone();
    if let Some(s) = after.sources.iter_mut().find(|s| s.id == source_id) {
        s.repaid_principal = round2(s.repaid_principal + repay);
    }
    // Drop the hypothetical repaid amount from that source's tail installments
    // (approximation: remove unpaid rows from the END until `repay` is covered).
    let mut remaining = repay;
    let mut rows: Vec<(String, f64, i64)> = after
        .unpaid
        .iter()
        .filter(|(_, _, sid)| *sid == source_id)
        .cloned()
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let mut dropped: Vec<(String, i64)> = Vec::new();
    for (due, total, _sid) in rows {
        if remaining <= 0.0 {
            break;
        }
        dropped.push((due, source_id));
        remaining -= total;
    }
    after
        .unpaid
        .retain(|(d, _, sid)| !dropped.iter().any(|(dd, dsid)| dd == d && dsid == sid));

    json!({
        "scenario": "early_repay",
        "params": { "source_id": source_id, "source_name": src.name, "amount": repay },
        "estimate": {
            "interest_saved": interest_saved,
            "note": "Ước tính đơn giản: số tiền trả trước × lãi suất × thời gian còn lại đến đáo hạn. Số chính xác phụ thuộc điều khoản hợp đồng (phí trả trước hạn…).",
        },
        "before": side_metrics(snap),
        "after": side_metrics(&after),
    })
}

/// The comparable metric block for before/after views.
fn side_metrics(snap: &Snapshot) -> Value {
    let eval = evaluate(snap);
    json!({
        "debt_outstanding": snap.debt_outstanding(),
        "de_ratio": snap.de_ratio(),
        "weighted_debt_rate": snap.weighted_rate(),
        "due_30d": snap.due_within_days(30),
        "score": eval["score"],
        "grade": eval["grade"],
        "monthly_due_12m": snap.monthly_due_12m(),
    })
}

/// Convenience for API/MCP: evaluate straight from the DB.
pub fn evaluate_db(db: &Db) -> Value {
    evaluate(&Snapshot::from_db(db, &finance::today()))
}

// ---------------------------------------------------------------------------
// Phân tích SỬ DỤNG nguồn tiền — where the drawn money actually went.
// ---------------------------------------------------------------------------

/// Usage analysis: disbursed money by allocation (mục đích), the unclassified
/// remainder, per-source utilization/idle capital, and explainable signals.
pub fn usage_analysis(db: &Db) -> Value {
    let sources = db.list_sources(None);
    let allocs = db.list_allocs();

    let total_disbursed: f64 = round2(sources.iter().map(|s| s.disbursed).sum());
    let allocated: f64 = round2(
        allocs
            .iter()
            .map(|a| a["used"].as_f64().unwrap_or(0.0))
            .sum(),
    );
    let unallocated = round2((total_disbursed - allocated).max(0.0));
    let unallocated_pct = if total_disbursed > 0.0 {
        round2(unallocated / total_disbursed * 100.0)
    } else {
        0.0
    };

    let breakdown: Vec<Value> = allocs
        .iter()
        .map(|a| {
            let used = a["used"].as_f64().unwrap_or(0.0);
            let target = a["target_amount"].as_f64().unwrap_or(0.0);
            json!({
                "id": a["id"],
                "name": a["name"],
                "status": a["status"],
                "used": used,
                "target_amount": target,
                "share_pct": if total_disbursed > 0.0 { round2(used / total_disbursed * 100.0) } else { 0.0 },
                "budget_used_pct": if target > 0.0 { json!(round2(used / target * 100.0)) } else { json!(null) },
                "over_budget": target > 0.0 && used > target,
            })
        })
        .collect();

    let utilization: Vec<Value> = sources
        .iter()
        .filter(|s| s.status == "active")
        .map(|s| {
            json!({
                "id": s.id,
                "name": s.name,
                "kind": s.kind,
                "committed": s.total_amount,
                "disbursed": s.disbursed,
                "utilization_pct": if s.total_amount > 0.0 { round2(s.disbursed / s.total_amount * 100.0) } else { 0.0 },
                "idle": s.available(),
            })
        })
        .collect();

    // Explainable signals about how money is being used.
    let mut signals: Vec<Value> = Vec::new();
    if total_disbursed > 0.0 && unallocated_pct > 50.0 {
        signals.push(json!({
            "severity": "warn",
            "title": format!("{unallocated_pct}% vốn giải ngân chưa gắn mục đích"),
            "detail": "Phần lớn tiền rút về chưa được phân bổ vào dự án/mục đích nào — gắn alloc_id khi giải ngân để đánh giá được hiệu quả sử dụng.",
        }));
    } else if total_disbursed > 0.0 && unallocated > 0.0 {
        signals.push(json!({
            "severity": "good",
            "title": format!("Đã phân loại {}% vốn giải ngân", round2(100.0 - unallocated_pct)),
            "detail": format!("Còn {} chưa gắn mục đích.", fmt_money(unallocated)),
        }));
    }
    for b in &breakdown {
        if b["over_budget"] == true {
            signals.push(json!({
                "severity": "warn",
                "title": format!("\"{}\" vượt ngân sách dự kiến", b["name"].as_str().unwrap_or("?")),
                "detail": format!(
                    "Đã rót {} / dự kiến {}.",
                    fmt_money(b["used"].as_f64().unwrap_or(0.0)),
                    fmt_money(b["target_amount"].as_f64().unwrap_or(0.0))
                ),
            }));
        }
    }
    let idle_total: f64 = round2(
        sources
            .iter()
            .filter(|s| s.status == "active")
            .map(|s| s.available())
            .sum(),
    );
    if idle_total > 0.0 && total_disbursed > 0.0 && idle_total > total_disbursed {
        signals.push(json!({
            "severity": "good",
            "title": format!("Còn {} chưa dùng đến", fmt_money(idle_total)),
            "detail": "Nguồn khả dụng lớn hơn phần đã giải ngân — dư địa an toàn, nhưng chú ý phí cam kết/duy trì hạn mức nếu có.",
        }));
    }

    json!({
        "total_disbursed": total_disbursed,
        "allocated": allocated,
        "unallocated": unallocated,
        "unallocated_pct": unallocated_pct,
        "by_allocation": breakdown,
        "by_source": utilization,
        "signals": signals,
    })
}

// ---------------------------------------------------------------------------
// Đánh giá TỪNG nguồn tiền — per-source scorecard (deterministic).
// ---------------------------------------------------------------------------

fn ts_to_date(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|d| d.date_naive().format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// Rate every ACTIVE source: cost vs. the book's weighted average, payment
/// discipline (from the schedule history), maturity/floating/utilization
/// risks → score 0–100, grade A–D, verdict + factor list. No LLM involved.
pub fn source_ratings(db: &Db, today: &str) -> Value {
    let snap = Snapshot::from_db(db, today);
    let wavg = snap.weighted_rate();
    let horizon90 = add_months(today, 3);

    let ratings: Vec<Value> = snap
        .sources
        .iter()
        .filter(|s| s.status == "active")
        .map(|s| {
            let mut score: i64 = 70;
            let mut factors: Vec<Value> = Vec::new();
            let mut push = |score: &mut i64, delta: i64, text: String| {
                *score += delta;
                let impact = if delta > 0 {
                    "+"
                } else if delta < 0 {
                    "-"
                } else {
                    "0"
                };
                factors.push(json!({ "impact": impact, "delta": delta, "text": text }));
            };

            if finance::is_debt_kind(&s.kind) {
                // Chi phí vốn so với mặt bằng sổ.
                if wavg > 0.0 && s.interest_rate <= wavg - 1.0 {
                    push(
                        &mut score,
                        10,
                        format!(
                            "Lãi suất {}%/năm — rẻ hơn mặt bằng sổ ({wavg}%)",
                            s.interest_rate
                        ),
                    );
                } else if wavg > 0.0 && s.interest_rate >= wavg + 3.0 {
                    push(
                        &mut score,
                        -15,
                        format!(
                            "Lãi suất {}%/năm — đắt hơn mặt bằng sổ ({wavg}%) ≥3 điểm",
                            s.interest_rate
                        ),
                    );
                } else {
                    push(
                        &mut score,
                        0,
                        format!("Lãi suất {}%/năm — quanh mặt bằng sổ", s.interest_rate),
                    );
                }

                // Kỷ luật trả nợ từ lịch sử schedule.
                let paid = db.list_schedule(Some(s.id), Some("paid"), today, 1000);
                let overdue_now = db
                    .list_schedule(Some(s.id), Some("overdue"), today, 1000)
                    .len();
                if overdue_now > 0 {
                    push(&mut score, -20, format!("Đang có {overdue_now} kỳ quá hạn"));
                } else if !paid.is_empty() {
                    let late = paid
                        .iter()
                        .filter(|p| {
                            // Ngày trả thực nếu có, fallback timestamp lúc bấm nút.
                            let mut d = p["paid_date"].as_str().unwrap_or("").to_string();
                            if d.is_empty() {
                                d = p["paid_at"].as_i64().map(ts_to_date).unwrap_or_default();
                            }
                            !d.is_empty() && d.as_str() > p["due_date"].as_str().unwrap_or("")
                        })
                        .count();
                    let ratio = late as f64 / paid.len() as f64;
                    if ratio <= 0.1 {
                        push(
                            &mut score,
                            10,
                            format!("Trả đúng hạn {}/{} kỳ", paid.len() - late, paid.len()),
                        );
                    } else if ratio > 0.3 {
                        push(&mut score, -10, format!("Trả trễ {late}/{} kỳ", paid.len()));
                    }
                }

                // Đáo hạn gần còn dư nợ.
                if s.outstanding() > 0.0
                    && !s.end_date.is_empty()
                    && s.end_date.as_str() <= horizon90.as_str()
                {
                    push(
                        &mut score,
                        -10,
                        format!(
                            "Đáo hạn {} mà còn dư nợ {}",
                            s.end_date,
                            fmt_money(s.outstanding())
                        ),
                    );
                }
                // Lãi thả nổi.
                if s.rate_type == "floating" {
                    push(
                        &mut score,
                        -5,
                        "Lãi thả nổi — rủi ro chi phí tăng theo thị trường".into(),
                    );
                }
                // Hạn mức: room dự phòng.
                if s.kind == "credit_line" && s.total_amount > 0.0 {
                    let util = s.outstanding() / s.total_amount;
                    if util > 0.9 {
                        push(
                            &mut score,
                            -10,
                            format!(
                                "Hạn mức đã dùng {}% — cạn room dự phòng",
                                (util * 100.0).round()
                            ),
                        );
                    } else if util < 0.3 {
                        push(
                            &mut score,
                            5,
                            format!(
                                "Hạn mức mới dùng {}% — còn nhiều room dự phòng",
                                (util * 100.0).round()
                            ),
                        );
                    }
                }
                // Nợ có dư mà không có lịch trả.
                if s.outstanding() > 0.0 && !snap.unpaid.iter().any(|(_, _, sid)| *sid == s.id) {
                    push(
                        &mut score,
                        -5,
                        "Chưa có lịch trả nợ — không theo dõi được kỳ hạn".into(),
                    );
                }
            } else {
                // Nguồn vốn chủ/tài trợ: chủ yếu là mức độ thực hiện cam kết.
                score = 80;
                if s.total_amount > 0.0 {
                    let done = s.disbursed / s.total_amount;
                    if done >= 1.0 {
                        push(&mut score, 10, "Đã góp/nhận đủ cam kết".into());
                    } else {
                        push(
                            &mut score,
                            0,
                            format!(
                                "Mới góp/nhận {}% cam kết — còn {}",
                                (done * 100.0).round(),
                                fmt_money(s.total_amount - s.disbursed)
                            ),
                        );
                    }
                }
                if s.interest_rate > 0.0 {
                    push(
                        &mut score,
                        0,
                        format!("Có cam kết lợi tức {}%/năm", s.interest_rate),
                    );
                }
            }

            let score = score.clamp(0, 100);
            let grade = match score {
                85..=100 => "A",
                70..=84 => "B",
                50..=69 => "C",
                _ => "D",
            };
            let verdict = match (grade, finance::is_debt_kind(&s.kind)) {
                ("A", true) => "Nguồn vay tốt — chi phí/kỷ luật ổn, ưu tiên giữ.",
                ("B", true) => "Nguồn vay ổn — theo dõi các điểm trừ bên dưới.",
                ("C", true) => "Nguồn vay cần chú ý — cân nhắc đàm phán lại hoặc trả bớt.",
                (_, true) => "Nguồn vay rủi ro — ứng viên hàng đầu để đảo nợ/tất toán.",
                ("A", false) => "Nguồn vốn chủ/tài trợ lành mạnh.",
                (_, false) => "Nguồn vốn chủ/tài trợ — xem điểm cần hoàn thiện bên dưới.",
            };
            json!({
                "id": s.id,
                "name": s.name,
                "kind": s.kind,
                "is_debt": finance::is_debt_kind(&s.kind),
                "outstanding": s.outstanding(),
                "interest_rate": s.interest_rate,
                "score": score,
                "grade": grade,
                "verdict": verdict,
                "factors": factors,
            })
        })
        .collect();

    json!({ "today": today, "weighted_debt_rate": wavg, "ratings": ratings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json as j;

    fn src(
        id: i64,
        name: &str,
        kind: &str,
        total: f64,
        rate: f64,
        disbursed: f64,
        repaid: f64,
        end_date: &str,
    ) -> SourceRow {
        SourceRow {
            id,
            name: name.into(),
            kind: kind.into(),
            provider: String::new(),
            total_amount: total,
            currency: "VND".into(),
            interest_rate: rate,
            rate_type: "fixed".into(),
            start_date: String::new(),
            end_date: end_date.into(),
            status: "active".into(),
            note: String::new(),
            disbursed,
            repaid_principal: repaid,
            interest_paid: 0.0,
            fees_paid: 0.0,
        }
    }

    fn healthy_snapshot() -> Snapshot {
        Snapshot {
            today: "2026-07-27".into(),
            sources: vec![
                src(1, "Vốn chủ", "equity", 1_000.0, 0.0, 1_000.0, 0.0, ""),
                src(
                    2,
                    "Vay A",
                    "bank_loan",
                    1_000.0,
                    8.0,
                    500.0,
                    100.0,
                    "2028-12-31",
                ),
            ],
            unpaid: vec![
                ("2026-08-15".into(), 50.0, 2),
                ("2026-09-15".into(), 50.0, 2),
            ],
        }
    }

    #[test]
    fn healthy_book_scores_high() {
        let e = evaluate(&healthy_snapshot());
        assert!(
            e["score"].as_i64().unwrap() >= 85,
            "score={} findings={}",
            e["score"],
            e["findings"]
        );
        assert_eq!(e["grade"], "A");
        assert_eq!(e["metrics"]["de_ratio"], j!(0.4));
    }

    #[test]
    fn overdue_and_leverage_tank_the_score() {
        let mut s = healthy_snapshot();
        s.unpaid.push(("2026-06-01".into(), 500.0, 2)); // overdue
        s.sources[1].disbursed = 3_000.0; // D/E = 2.9
        s.sources[1].total_amount = 3_000.0;
        let e = evaluate(&s);
        assert!(e["score"].as_i64().unwrap() < 70, "score={}", e["score"]);
        let sevs: Vec<&str> = e["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["severity"].as_str().unwrap())
            .collect();
        assert!(sevs.contains(&"crit"));
        let titles = e["findings"].to_string();
        assert!(titles.contains("QUÁ HẠN"));
        assert!(titles.contains("D/E"));
    }

    #[test]
    fn liquidity_crunch_detected() {
        let mut s = healthy_snapshot();
        // Obligations within 30 days exceed everything still drawable.
        s.unpaid = vec![("2026-08-01".into(), 5_000.0, 2)];
        let e = evaluate(&s);
        let titles = e["findings"].to_string();
        assert!(titles.contains("VƯỢT nguồn còn rút được"), "{titles}");
    }

    #[test]
    fn no_schedule_and_maturity_flags() {
        let s = Snapshot {
            today: "2026-07-27".into(),
            sources: vec![
                src(1, "Vốn chủ", "equity", 1_000.0, 0.0, 1_000.0, 0.0, ""),
                src(
                    2,
                    "Vay sắp đáo hạn",
                    "bank_loan",
                    500.0,
                    9.0,
                    500.0,
                    0.0,
                    "2026-09-01",
                ),
            ],
            unpaid: vec![],
        };
        let e = evaluate(&s);
        let titles = e["findings"].to_string();
        assert!(titles.contains("đáo hạn trong 90 ngày"), "{titles}");
        assert!(titles.contains("chưa có lịch trả nợ"), "{titles}");
    }

    #[test]
    fn simulate_new_loan_moves_metrics() {
        let s = healthy_snapshot();
        let r = simulate_new_loan(
            &s,
            &NewLoanParams {
                amount: 1_000.0,
                annual_rate: 12.0,
                periods: 12,
                method: "annuity".into(),
                freq_months: 1,
            },
        );
        assert!(r.get("error").is_none(), "{r}");
        let before_debt = r["before"]["debt_outstanding"].as_f64().unwrap();
        let after_debt = r["after"]["debt_outstanding"].as_f64().unwrap();
        assert_eq!(after_debt, before_debt + 1_000.0);
        assert!(r["loan"]["total_interest"].as_f64().unwrap() > 0.0);
        // D/E rises: 400/1000=0.4 → 1400/1000=1.4.
        assert_eq!(r["after"]["de_ratio"], j!(1.4));
        assert!(r["after"]["score"].as_i64().unwrap() <= r["before"]["score"].as_i64().unwrap());
        assert_eq!(r["loan"]["schedule_preview"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn simulate_early_repay_saves_interest() {
        let s = healthy_snapshot();
        let r = simulate_early_repay(&s, 2, 200.0);
        assert!(r.get("error").is_none(), "{r}");
        assert_eq!(r["after"]["debt_outstanding"].as_f64().unwrap(), 200.0); // 400 - 200
                                                                             // 200 × 8% × ~2.42 năm ≈ 38.7
        let saved = r["estimate"]["interest_saved"].as_f64().unwrap();
        assert!(saved > 30.0 && saved < 45.0, "saved={saved}");
        // Refuses equity sources and unknown ids.
        assert!(simulate_early_repay(&s, 1, 100.0).get("error").is_some());
        assert!(simulate_early_repay(&s, 99, 100.0).get("error").is_some());
    }

    #[test]
    fn usage_analysis_flags_unallocated_and_over_budget() {
        let db = crate::db::Db::open_memory().unwrap();
        let sid = db
            .add_source(
                "Vay A",
                "bank_loan",
                "NH",
                2_000.0,
                "VND",
                9.0,
                "fixed",
                "",
                "",
                "",
            )
            .unwrap();
        let aid = db.add_alloc("Dự án X", "", 300.0).unwrap();
        db.add_tx(sid, Some(aid), "disburse", 400.0, "2026-01-10", "")
            .unwrap(); // over budget
        db.add_tx(sid, None, "disburse", 600.0, "2026-02-10", "")
            .unwrap(); // unallocated
        let u = usage_analysis(&db);
        assert_eq!(u["total_disbursed"], 1_000.0);
        assert_eq!(u["allocated"], 400.0);
        assert_eq!(u["unallocated"], 600.0);
        assert_eq!(u["unallocated_pct"], 60.0);
        let sig = u["signals"].to_string();
        assert!(sig.contains("chưa gắn mục đích"), "{sig}");
        assert!(sig.contains("vượt ngân sách"), "{sig}");
        assert_eq!(u["by_allocation"][0]["over_budget"], true);
        assert_eq!(u["by_source"][0]["utilization_pct"], 50.0);
    }

    #[test]
    fn source_ratings_grade_cheap_vs_expensive() {
        let db = crate::db::Db::open_memory().unwrap();
        let cheap = db
            .add_source(
                "Vay rẻ",
                "bank_loan",
                "A",
                1_000.0,
                "VND",
                7.0,
                "fixed",
                "",
                "2029-01-01",
                "",
            )
            .unwrap();
        let dear = db
            .add_source(
                "Vay đắt",
                "personal_loan",
                "B",
                1_000.0,
                "VND",
                18.0,
                "floating",
                "",
                "",
                "",
            )
            .unwrap();
        let eq = db
            .add_source(
                "Vốn chủ",
                "equity",
                "",
                500.0,
                "VND",
                0.0,
                "fixed",
                "",
                "",
                "",
            )
            .unwrap();
        db.add_tx(cheap, None, "disburse", 800.0, "2026-01-05", "")
            .unwrap();
        db.add_tx(dear, None, "disburse", 400.0, "2026-01-05", "")
            .unwrap();
        db.add_tx(eq, None, "disburse", 500.0, "2026-01-05", "")
            .unwrap();
        // Cheap loan pays on time.
        let items =
            crate::finance::generate_schedule("equal_principal", 800.0, 7.0, 8, "2026-01-05", 1);
        db.replace_schedule(cheap, &items).unwrap();
        let first = db.list_schedule(Some(cheap), None, "2026-01-06", 1)[0]["id"]
            .as_i64()
            .unwrap();
        db.pay_schedule(first, false, "2026-02-01").unwrap();

        let r = source_ratings(&db, "2026-02-20");
        let ratings = r["ratings"].as_array().unwrap();
        assert_eq!(ratings.len(), 3);
        let get = |id: i64| ratings.iter().find(|x| x["id"] == id).unwrap().clone();
        let (rc, rd, re) = (get(cheap), get(dear), get(eq));
        // wavg = (800·7 + 400·18)/1200 ≈ 10.67 → cheap ≤ wavg−1 (+10), dear ≥ wavg+3 (−15).
        assert!(
            rc["score"].as_i64().unwrap() > rd["score"].as_i64().unwrap(),
            "cheap {} vs dear {}",
            rc["score"],
            rd["score"]
        );
        // Dear loan: expensive −15, floating −5, no schedule −5, overdue (Feb installment)… none scheduled → D/C range.
        assert!(matches!(rd["grade"].as_str().unwrap(), "C" | "D"), "{rd}");
        assert_eq!(re["is_debt"], false);
        assert!(re["factors"].to_string().contains("đủ cam kết"), "{re}");
    }

    #[test]
    fn monthly_due_aggregates() {
        let s = healthy_snapshot();
        let m = s.monthly_due_12m();
        assert_eq!(m.len(), 2);
        assert_eq!(m[0]["month"], "2026-08");
        assert_eq!(m[0]["total_due"], 50.0);
    }
}
