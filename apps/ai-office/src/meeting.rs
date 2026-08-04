//! Nhịp điều hành của Sếp: HỌP SÁNG (Giám đốc vận hành điểm tình hình + đề
//! xuất 3 ưu tiên) và HỌP TỐI (tổng kết ngày, chuẩn bị ngày mai). Mỗi biên
//! bản là một lượt LLM thật qua daemon bridge, với context là toàn cảnh văn
//! phòng: mục tiêu quý, bảng việc, việc chờ duyệt, việc lỗi, chi tiêu token.
//!
//! Kèm phần tính số cho DASHBOARD ĐIỀU HÀNH (bàn làm việc của Sếp): độ bám
//! hướng, tiến độ mục tiêu, chờ duyệt, nhịp họp (streak) và token trong tháng.

use crate::db::{Db, Meeting};
use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate};
use serde_json::{json, Value};
use std::sync::Arc;

/// Ngày local dạng YYYY-MM-DD — khoá biên bản họp và mốc "hôm nay".
pub fn local_day() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

/// Epoch giây của 00:00 ngày đầu tháng hiện tại (giờ local) — mốc cộng token.
pub fn month_start_ts() -> i64 {
    let now = Local::now();
    let first = now
        .date_naive()
        .with_day(1)
        .unwrap_or_else(|| now.date_naive());
    first
        .and_hms_opt(0, 0, 0)
        .and_then(|dt| dt.and_local_timezone(Local).single())
        .map(|dt| dt.timestamp())
        .unwrap_or(0)
}

/// NHỊP ĐIỀU HÀNH: số ngày họp sáng LIÊN TIẾP. Chuỗi kết thúc ở hôm nay nếu
/// hôm nay đã họp, còn chưa họp thì vẫn tính chuỗi tới hôm qua (chưa đứt —
/// hôm nay còn cơ hội họp). `days` là danh sách ngày đã họp sáng, mới nhất
/// trước. Trả `(số ngày liên tiếp, hôm nay đã họp sáng chưa)`.
pub fn morning_streak(days: &[String], today: &str) -> (i64, bool) {
    let parse = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    let Some(today_d) = parse(today) else {
        return (0, false);
    };
    let set: std::collections::HashSet<NaiveDate> =
        days.iter().filter_map(|d| parse(d)).collect();
    let has_today = set.contains(&today_d);
    let mut cursor = if has_today {
        today_d
    } else {
        today_d - ChronoDuration::days(1)
    };
    let mut streak = 0i64;
    while set.contains(&cursor) {
        streak += 1;
        cursor -= ChronoDuration::days(1);
    }
    (streak, has_today)
}

/// Token đã chi trong tháng: việc trên bảng + các phiên họp điều hành.
fn month_tokens(db: &Db) -> i64 {
    let tasks = db.tokens_since(month_start_ts()).unwrap_or(0);
    let meetings = db
        .get_setting(&meeting_tokens_key())
        .ok()
        .flatten()
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    tasks + meetings
}

fn meeting_tokens_key() -> String {
    format!("meeting_tokens_{}", Local::now().format("%Y-%m"))
}

/// Số liệu cho 5 thẻ KPI trên dashboard điều hành.
pub fn dashboard_json(db: &Db) -> Value {
    let (open, aligned) = db.open_task_stats().unwrap_or((0, 0));
    let goals = db.list_goals(false).unwrap_or_default();
    let avg_progress = if goals.is_empty() {
        0
    } else {
        goals.iter().map(|g| g.progress).sum::<i64>() / goals.len() as i64
    };
    let waiting = db.waiting_count().unwrap_or(0);
    let today = local_day();
    let days = db.morning_days().unwrap_or_default();
    let (streak, morning_today) = morning_streak(&days, &today);
    let evening_today = db
        .list_meetings(10)
        .unwrap_or_default()
        .iter()
        .any(|m| m.kind == "evening" && m.day == today);
    json!({
        "date": today,
        "alignment": {
            "open": open,
            "aligned": aligned,
            // null khi bảng trống — UI hiện "—" thay vì 0% gây hiểu nhầm.
            "percent": if open > 0 { Some(aligned * 100 / open) } else { None },
        },
        "goals": { "count": goals.len(), "avgProgress": avg_progress },
        "waiting": waiting,
        "streak": { "days": streak, "morningToday": morning_today, "eveningToday": evening_today },
        "budget": { "monthTokens": month_tokens(db), "openTasks": open },
    })
}

/// Toàn cảnh văn phòng nén thành context cho Giám đốc vận hành.
fn office_context(db: &Db) -> String {
    let goals = db.list_goals(false).unwrap_or_default();
    let goal_counts = db.goal_task_counts().unwrap_or_default();
    let tasks = db.list_tasks(200).unwrap_or_default();
    let teams = db.list_teams().unwrap_or_default();
    let team_name = |key: &str| {
        teams
            .iter()
            .find(|t| t.key == key)
            .map(|t| t.name.clone())
            .unwrap_or_else(|| key.to_uppercase())
    };
    let goal_title = |id: Option<i64>| -> String {
        match id.and_then(|gid| goals.iter().find(|g| g.id == gid)) {
            Some(g) => format!("🎯 {}", g.title),
            None => "⚠ CHƯA GẮN MỤC TIÊU".to_string(),
        }
    };

    let mut ctx = format!("Hôm nay: {}\n", Local::now().format("%A %d/%m/%Y %H:%M"));

    // Mục tiêu quý + tiến độ + số việc phục vụ.
    if goals.is_empty() {
        ctx.push_str("\nMỤC TIÊU QUÝ: chưa đặt mục tiêu nào.\n");
    } else {
        ctx.push_str("\nMỤC TIÊU QUÝ:\n");
        for g in &goals {
            let (total, open) = goal_counts.get(&g.id).copied().unwrap_or((0, 0));
            let krs = g
                .key_results
                .iter()
                .map(|k| format!("{} {}", if k.done { "✓" } else { "☐" }, k.text))
                .collect::<Vec<_>>()
                .join("; ");
            ctx.push_str(&format!(
                "- [{}%] {} ({}) — {} việc đang mở / {} tổng. KR: {}\n",
                g.progress,
                g.title,
                g.quarter,
                open,
                total,
                if krs.is_empty() { "chưa có" } else { &krs }
            ));
        }
    }

    // Bảng việc theo cột.
    let mut inbox = Vec::new();
    let mut doing = Vec::new();
    let mut waiting = Vec::new();
    let mut done24 = Vec::new();
    let mut errors = Vec::new();
    let now = crate::db::now();
    for t in &tasks {
        let line = format!(
            "#{} \"{}\" (đội {}, {})",
            t.id,
            t.title,
            team_name(&t.team),
            goal_title(t.goal_id)
        );
        match t.status.as_str() {
            "inbox" => inbox.push(line),
            "pending" | "planning" | "running" | "review" => doing.push(line),
            "error" => errors.push(line),
            "done" if t.approval == "waiting" => waiting.push(line),
            "done" => {
                if t.finished_at.map(|f| now - f < 86_400).unwrap_or(false) {
                    done24.push(line);
                }
            }
            _ => {}
        }
    }
    let section = |name: &str, items: &[String], max: usize| -> String {
        if items.is_empty() {
            return format!("\n{} (0):\n- (trống)\n", name);
        }
        let mut s = format!("\n{} ({}):\n", name, items.len());
        for it in items.iter().take(max) {
            s.push_str(&format!("- {}\n", it));
        }
        if items.len() > max {
            s.push_str(&format!("- … và {} việc nữa\n", items.len() - max));
        }
        s
    };
    ctx.push_str(&section("HỘP VIỆC — chưa chạy", &inbox, 8));
    ctx.push_str(&section("ĐANG LÀM", &doing, 8));
    ctx.push_str(&section("CHỜ SẾP DUYỆT", &waiting, 8));
    ctx.push_str(&section("HOÀN TẤT trong 24h qua", &done24, 6));
    if !errors.is_empty() {
        ctx.push_str(&section("VIỆC LỖI — cần Sếp xử lý", &errors, 4));
    }

    // Nhịp + ngân sách.
    let today = local_day();
    let days = db.morning_days().unwrap_or_default();
    let (streak, morning_today) = morning_streak(&days, &today);
    ctx.push_str(&format!(
        "\nNHỊP ĐIỀU HÀNH: {} ngày họp sáng liên tiếp{}. Token AI đã dùng trong tháng: {}.\n",
        streak,
        if morning_today {
            " (hôm nay đã họp sáng)"
        } else {
            " (hôm nay CHƯA họp sáng)"
        },
        month_tokens(db)
    ));
    ctx
}

const MEETING_MAX_TOKENS: u32 = 2000;

/// Chạy một phiên họp điều hành và lưu biên bản của ngày hôm nay.
/// `kind`: "morning" | "evening".
pub async fn run_meeting(db: &Arc<Db>, kind: &str) -> Result<Meeting, String> {
    let system = "Bạn là GIÁM ĐỐC VẬN HÀNH của một văn phòng AI \"công ty một người\" — cánh tay phải của Sếp (người dùng). Bạn nói tiếng Việt, giọng gọn gàng, thẳng thắn, thực dụng, tôn trọng thời gian của Sếp. Không lời chào thừa, không kể lể.";
    let ctx = office_context(db);
    let user = match kind {
        "evening" => format!(
            "{}\n\nHãy viết BIÊN BẢN HỌP TỐI — tổng kết ngày cho Sếp, đúng 3 phần với tiêu đề in đậm:\n**HÔM NAY ĐÃ LÀM** — 2–3 câu về những gì văn phòng đã hoàn thành/tiến triển hôm nay.\n**CÒN TỒN** — những việc chờ duyệt, đang dở hoặc lỗi mà Sếp cần để mắt (nêu tên việc cụ thể).\n**CHUẨN BỊ NGÀY MAI** — danh sách đánh số tối đa 3 việc cụ thể nên bắt đầu sáng mai, kèm lý do ngắn.",
            ctx
        ),
        _ => format!(
            "{}\n\nHãy viết BIÊN BẢN HỌP SÁNG cho Sếp, đúng 3 phần với tiêu đề in đậm:\n**TÌNH HÌNH** — 2–4 câu điểm tình hình công ty từ số liệu trên (mục tiêu nào chạy, nghẽn ở đâu).\n**3 ƯU TIÊN HÔM NAY** — danh sách đánh số 1–3, mỗi ưu tiên một câu, chỉ rõ việc/đội/mục tiêu liên quan và vì sao làm trước.\n**CẢNH BÁO** — 1–3 gạch đầu dòng: việc chưa gắn mục tiêu (lạc hướng), việc chờ duyệt tồn đọng, việc lỗi, mục tiêu giậm chân. Không có gì đáng lo thì nói ngắn gọn là ổn.",
            ctx
        ),
    };
    let (text, _model, _finish, usage) =
        crate::llm::bridge_llm(system, &user, MEETING_MAX_TOKENS).await?;
    let text = text.trim();
    if text.is_empty() {
        return Err("LLM trả về biên bản trống — thử lại".into());
    }
    // Cộng chi phí họp vào sổ token tháng (bảng việc chỉ đếm token của task).
    if let Some((tin, tout)) = usage {
        let key = meeting_tokens_key();
        let cur = db
            .get_setting(&key)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let _ = db.set_setting(&key, &(cur + tin + tout).to_string());
    }
    db.upsert_meeting(kind, &local_day(), text)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::morning_streak;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn streak_counts_consecutive_days_ending_today() {
        let days = s(&["2026-08-04", "2026-08-03", "2026-08-02"]);
        assert_eq!(morning_streak(&days, "2026-08-04"), (3, true));
    }

    #[test]
    fn streak_not_broken_before_todays_meeting() {
        // Hôm nay chưa họp: chuỗi tính tới hôm qua, morningToday=false.
        let days = s(&["2026-08-03", "2026-08-02"]);
        assert_eq!(morning_streak(&days, "2026-08-04"), (2, false));
    }

    #[test]
    fn streak_breaks_on_gap() {
        let days = s(&["2026-08-04", "2026-08-02"]);
        assert_eq!(morning_streak(&days, "2026-08-04"), (1, true));
        // Nghỉ hôm qua + hôm nay chưa họp → 0.
        let days = s(&["2026-08-02", "2026-08-01"]);
        assert_eq!(morning_streak(&days, "2026-08-04"), (0, false));
    }

    #[test]
    fn streak_empty() {
        assert_eq!(morning_streak(&[], "2026-08-04"), (0, false));
    }
}
