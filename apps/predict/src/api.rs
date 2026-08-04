//! HTTP API + shared value builders. Every REST endpoint, MCP tool and the
//! engine reuse the same `*_value` helpers so numbers are identical everywhere.
//! Domain disclaimers (lottery/market) are attached here or in the LLM layer —
//! in code, unconditionally.

use axum::{
    extract::{Path, Query, State},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use app_space_sdk::SpaceClient;

use crate::db::{now, Db, PredictionInput};
use crate::timeutil::{parse_date_days, vn_date, vn_hm, VN_OFFSET};
use crate::{
    builder, engine, evidence, fetch, football, ledger, llm, lottery, market, methodology, topic,
};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Db>,
    pub sc: SpaceClient,
    pub http: reqwest::Client,
    pub mcp_tx: tokio::sync::broadcast::Sender<String>,
}

pub fn make_state() -> AppState {
    let db = Arc::new(Db::open_default().expect("open predict db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(100);
    AppState {
        db,
        sc: SpaceClient::from_env(),
        http: fetch::http(),
        mcp_tx,
    }
}

#[cfg(test)]
pub fn test_state() -> AppState {
    let db = Arc::new(Db::open_memory().expect("mem db"));
    let (mcp_tx, _) = tokio::sync::broadcast::channel(4);
    AppState {
        db,
        sc: SpaceClient::new("http://127.0.0.1:1", "predict"),
        http: fetch::http(),
        mcp_tx,
    }
}

// ---- status / overview ----

pub fn status_value(s: &AppState) -> Value {
    let summary = s.db.score_summary();
    json!({
        "ok": true,
        "app": "predict",
        "elo_teams": s.db.elo_count(),
        "elo_updated_at": s.db.elo_updated_at(),
        "fixtures": s.db.fixtures_count(),
        "lottery_draws": s.db.draws_count(),
        "lottery_latest": s.db.latest_draw().map(|(d, _, _)| d),
        "gold_latest": s.db.latest_price("XAU_USD").map(|(ts, p)| json!({ "ts": ts, "xau_usd": p })),
        "cities": s.db.cities(),
        "leagues": s.db.leagues().iter().map(|id| json!({ "id": id, "name": fetch::league_name(id) })).collect::<Vec<_>>(),
        "ledger": summary,
        "vn_date": vn_date(now()),
    })
}

/// Fast, local-only snapshot for the UI dashboard (engine keeps data fresh).
pub fn overview_value(s: &AppState) -> Value {
    let default_city =
        s.db.cities()
            .first()
            .cloned()
            .unwrap_or_else(|| "Hà Nội".into());
    let weather =
        s.db.weather_get(
            fetch::find_city(&default_city)
                .map(|(n, _, _)| n)
                .unwrap_or("Hà Nội"),
        )
        .map(|(v, _)| compact_weather(&v));
    let fixtures = s.db.fixtures_upcoming(now(), 5);
    let preds: Vec<Value> = fixtures
        .iter()
        .map(|f| engine::predict_for_fixture(s, f))
        .collect();
    json!({
        "gold": gold_snapshot(s),
        "weather_city": default_city,
        "weather": weather,
        "lottery_latest": s.db.latest_draw().map(|(d, n, _)| draw_json(&d, &n)),
        "football_today": preds,
        "ledger": s.db.score_summary(),
        "activity": s.db.recent_activity(15),
    })
}

// ---- football ----

pub async fn football_today_value(s: &AppState, days: i64, with_ledger: bool) -> Value {
    engine::ensure_elo(s, false).await;
    engine::ensure_fixtures(s, false).await;
    let horizon = now() + days.clamp(1, 14) * 86400;
    let fixtures: Vec<_> =
        s.db.fixtures_upcoming(now(), 50)
            .into_iter()
            .filter(|f| f.kickoff_ts <= horizon)
            .collect();
    let preds: Vec<Value> = fixtures
        .iter()
        .map(|f| engine::predict_for_fixture(s, f))
        .collect();
    let mut ledgered = 0;
    if with_ledger {
        let rows: Vec<(String, Value, i64)> = fixtures
            .iter()
            .zip(preds.iter())
            .map(|(f, p)| (f.event_id.clone(), p.clone(), f.kickoff_ts))
            .collect();
        ledgered = engine::ledger_upcoming_fixtures(s, &rows);
    }
    json!({ "matches": preds, "count": preds.len(), "ledgered": ledgered })
}

pub async fn predict_match_value(s: &AppState, home: &str, away: &str, article: bool) -> Value {
    engine::ensure_elo(s, false).await;
    let table = s.db.all_elo();
    let (elo_h, mh) =
        football::find_elo(&table, home).unwrap_or((football::FALLBACK_ELO, String::new()));
    let (elo_a, ma) =
        football::find_elo(&table, away).unwrap_or((football::FALLBACK_ELO, String::new()));
    let mut pred = football::predict(home, away, elo_h, elo_a);
    pred["elo_matched"] = json!(!mh.is_empty() && !ma.is_empty());
    if mh.is_empty() || ma.is_empty() {
        pred["note"] = json!("Một trong hai đội không có trong bảng ClubElo — dùng Elo trung bình 1600, độ tin cậy thấp.");
    }
    if article {
        if let Some(text) = llm::football_article(&s.sc, &pred).await {
            pred["article"] = json!(text);
        }
    }
    pred
}

pub fn elo_top_value(s: &AppState, limit: i64) -> Value {
    json!({ "teams": s.db.elo_top(limit.clamp(1, 100)) })
}

// ---- lottery ----

/// Group the 27 prize numbers into the familiar XSMB prize tiers.
pub fn draw_json(date: &str, numbers: &[i64]) -> Value {
    fn seg(n: &[i64], a: usize, b: usize) -> Vec<i64> {
        n.get(a..b).map(|s| s.to_vec()).unwrap_or_default()
    }
    let loto: Vec<String> = numbers
        .iter()
        .map(|n| lottery::fmt_loto((n.rem_euclid(100)) as u8))
        .collect();
    json!({
        "date": date,
        "special": numbers.first().copied(),
        "prizes": {
            "db": seg(numbers, 0, 1), "g1": seg(numbers, 1, 2), "g2": seg(numbers, 2, 4),
            "g3": seg(numbers, 4, 10), "g4": seg(numbers, 10, 14), "g5": seg(numbers, 14, 20),
            "g6": seg(numbers, 20, 23), "g7": seg(numbers, 23, 27),
        },
        "loto": loto,
    })
}

pub async fn lottery_latest_value(s: &AppState) -> Value {
    engine::ensure_draws(s, false).await;
    match s.db.latest_draw() {
        Some((d, n, _)) => draw_json(&d, &n),
        None => json!({ "error": "chưa có dữ liệu xổ số (nguồn chưa fetch được)" }),
    }
}

pub async fn lottery_stats_value(s: &AppState, days: i64) -> Value {
    engine::ensure_draws(s, false).await;
    let rows: Vec<(String, Vec<u8>)> =
        s.db.draws(days.clamp(7, 365))
            .into_iter()
            .map(|(d, _, l)| (d, l))
            .collect();
    if rows.is_empty() {
        return json!({ "error": "chưa có dữ liệu xổ số" });
    }
    let stats = lottery::loto_stats(&rows);
    let (heads, tails) = lottery::head_tail(&stats);
    json!({
        "window_draws": stats.window,
        "from": rows.last().map(|(d, _)| d.clone()),
        "to": rows.first().map(|(d, _)| d.clone()),
        "top_frequent": lottery::top_frequent(&stats, 10).iter()
            .map(|(n, c)| json!({ "loto": lottery::fmt_loto(*n), "count": c })).collect::<Vec<_>>(),
        "top_gan": lottery::top_gan(&stats, 10).iter()
            .map(|(n, g)| json!({ "loto": lottery::fmt_loto(*n), "days_absent": g })).collect::<Vec<_>>(),
        "heads": heads, "tails": tails,
        "disclaimer": lottery::DISCLAIMER,
    })
}

/// Next XSMB draw's VN date + a due timestamp safely after results publish.
pub fn next_draw(now_ts: i64) -> (String, i64) {
    let (h, m) = vn_hm(now_ts);
    let today = vn_date(now_ts);
    let date = if h < 18 || (h == 18 && m < 15) {
        today
    } else {
        // after cutoff → target tomorrow's draw
        vn_date(now_ts + 86400)
    };
    let days = parse_date_days(&date).unwrap_or(0);
    // 19:30 VN of the draw day.
    let due = days * 86400 - VN_OFFSET + 19 * 3600 + 1800;
    (date, due)
}

pub async fn lottery_suggest_value(s: &AppState, count: usize, note: bool) -> Value {
    engine::ensure_draws(s, false).await;
    let rows: Vec<(String, Vec<u8>)> = s.db.draws(30).into_iter().map(|(d, _, l)| (d, l)).collect();
    if rows.is_empty() {
        return json!({ "error": "chưa có dữ liệu xổ số" });
    }
    let n = count.clamp(1, 10);
    let stats = lottery::loto_stats(&rows);
    let picks = lottery::suggest(&stats, n);
    let picks_fmt: Vec<String> = picks.iter().map(|p| lottery::fmt_loto(*p)).collect();
    // Honest hit probability: at least one of n picks among 27 lotos.
    let p1 = lottery::baseline_hit_prob();
    let p_any = 1.0 - (1.0 - p1).powi(n as i32);
    let (draw_date, due) = next_draw(now());
    let key = format!("{}|{}", draw_date, picks_fmt.join("-"));
    let mut ledger_id = None;
    if !s.db.has_open_prediction("lottery", "key", &key) {
        ledger_id =
            s.db.add_prediction(&PredictionInput {
                domain: "lottery".into(),
                subject: format!("Chốt vui {} kỳ {}", picks_fmt.join(", "), draw_date),
                detail: json!({ "key": key, "date": draw_date, "numbers": picks }),
                probs: json!({ "hit": round3(p_any), "miss": round3(1.0 - p_any) }),
                due_at: due,
            })
            .ok();
    }
    let mut out = json!({
        "picks": picks_fmt,
        "for_draw": draw_date,
        "p_hit_honest": round3(p_any),
        "basis": "tần suất 30 kỳ + lô gan 2–7 kỳ (heuristic giải trí, không phải dự đoán thật)",
        "ledger_id": ledger_id,
        "disclaimer": lottery::DISCLAIMER,
    });
    if note {
        let summary = json!({
            "picks": picks_fmt,
            "top_frequent": lottery::top_frequent(&stats, 5).iter()
                .map(|(n2, c)| json!({ "loto": lottery::fmt_loto(*n2), "count": c })).collect::<Vec<_>>(),
            "top_gan": lottery::top_gan(&stats, 5).iter()
                .map(|(n2, g)| json!({ "loto": lottery::fmt_loto(*n2), "days_absent": g })).collect::<Vec<_>>(),
        });
        out["note"] = json!(llm::lottery_note(&s.sc, &summary).await);
    }
    out
}

// ---- weather ----

fn compact_weather(payload: &Value) -> Value {
    let d = &payload["daily"];
    let days: Vec<Value> = d["time"]
        .as_array()
        .map(|times| {
            times
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    json!({
                        "date": t,
                        "t_max": d["temperature_2m_max"][i],
                        "t_min": d["temperature_2m_min"][i],
                        "rain_prob": d["precipitation_probability_max"][i],
                        "rain_mm": d["precipitation_sum"][i],
                        "code": d["weather_code"][i],
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({ "current": payload["current"], "days": days })
}

pub async fn weather_value(s: &AppState, city: &str, advice: bool) -> Value {
    let Some((name_owned, lat, lon)) = s.db.city_coord(city) else {
        return json!({ "error": format!(
            "không biết địa điểm '{city}'. Thêm địa điểm bất kỳ ở tab Cài đặt (tìm theo tên), hoặc dùng: {}",
            fetch::CITIES.iter().map(|(n, _, _)| *n).collect::<Vec<_>>().join(", ")
        ) });
    };
    let name: &str = &name_owned;
    let cached = s.db.weather_get(name).filter(|(_, t)| now() - t < 3 * 3600);
    let payload = match cached {
        Some((v, _)) => v,
        None => match fetch::open_meteo_forecast(&s.http, lat, lon).await {
            Ok(v) => {
                let _ = s.db.weather_set(name, &v);
                v
            }
            Err(e) => return json!({ "error": format!("Open-Meteo lỗi: {e}") }),
        },
    };
    let mut out = compact_weather(&payload);
    out["city"] = json!(name);
    if advice {
        if let Some(text) = llm::weather_advice(&s.sc, name, &out["days"]).await {
            out["advice"] = json!(text);
        }
    }
    out
}

// ---- market ----

fn round3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn gold_snapshot(s: &AppState) -> Value {
    let day = 24 * 3600;
    let xau = s.db.latest_price("XAU_USD");
    let fx = s.db.latest_price("USD_VND");
    let luong = s.db.latest_price("XAU_VND_LUONG");
    let series7 = s.db.price_series("XAU_USD", now() - 7 * day);
    json!({
        "xau_usd": xau.map(|(ts, p)| json!({ "ts": ts, "price": round3(p) })),
        "usd_vnd": fx.map(|(ts, p)| json!({ "ts": ts, "price": p.round() })),
        "xau_vnd_luong_trieu": luong.map(|(ts, p)| json!({ "ts": ts, "price": round3(p) })),
        "momentum_24h_pct": market::momentum_pct(&series7, day).map(round3),
        "range_7d": market::range(&series7).map(|(lo, hi)| json!([round3(lo), round3(hi)])),
        "disclaimer": market::DISCLAIMER,
    })
}

pub async fn gold_value(s: &AppState) -> Value {
    engine::ensure_gold(s, false).await;
    gold_snapshot(s)
}

pub async fn gold_trend_value(s: &AppState, note: bool) -> Value {
    engine::ensure_gold(s, false).await;
    let day = 24 * 3600;
    let series = s.db.price_series("XAU_USD", now() - 30 * day);
    let series_luong = s.db.price_series("XAU_VND_LUONG", now() - 30 * day);
    let mut out = json!({
        "points": series.len(),
        "trend": market::trend_label(&series, 24, 24 * 7),
        "sma_1d": market::sma(&series, 24).map(round3),
        "sma_7d": market::sma(&series, 24 * 7).map(round3),
        "momentum_24h_pct": market::momentum_pct(&series, day).map(round3),
        "momentum_7d_pct": market::momentum_pct(&series, 7 * day).map(round3),
        "series_xau_usd": series.iter().rev().take(168).rev().map(|(ts, p)| json!([ts, round3(*p)])).collect::<Vec<_>>(),
        "series_vnd_luong": series_luong.iter().rev().take(168).rev().map(|(ts, p)| json!([ts, round3(*p)])).collect::<Vec<_>>(),
        "disclaimer": market::DISCLAIMER,
    });
    if note {
        let snap = json!({
            "trend": out["trend"], "sma_1d": out["sma_1d"], "sma_7d": out["sma_7d"],
            "momentum_24h_pct": out["momentum_24h_pct"], "momentum_7d_pct": out["momentum_7d_pct"],
            "xau_usd": s.db.latest_price("XAU_USD").map(|(_, p)| round3(p)),
            "xau_vnd_luong_trieu": s.db.latest_price("XAU_VND_LUONG").map(|(_, p)| round3(p)),
        });
        out["note"] = json!(llm::market_note(&s.sc, &snap).await);
    }
    out
}

// ---- brief ----

pub async fn brief_value(s: &AppState, narrate: bool) -> Value {
    engine::ensure_gold(s, false).await;
    engine::ensure_weather(s, false).await;
    let weather: Vec<Value> =
        s.db.cities()
            .iter()
            .filter_map(|c| fetch::find_city(c))
            .filter_map(|(name, _, _)| {
                s.db.weather_get(name).map(|(v, _)| {
                    let c = compact_weather(&v);
                    json!({ "city": name, "today": c["days"][0], "tomorrow": c["days"][1] })
                })
            })
            .collect();
    let football = football_today_value(s, 1, true).await;
    let mut data = json!({
        "date_vn": vn_date(now()),
        "weather": weather,
        "gold": gold_snapshot(s),
        "football_today": football["matches"],
        "lottery_yesterday": s.db.latest_draw().map(|(d, n, _)| draw_json(&d, &n)),
        "ledger_score": s.db.score_summary(),
    });
    if narrate {
        if let Some(text) = llm::morning_brief(&s.sc, &data).await {
            data["brief"] = json!(text);
        }
    }
    data
}

// ---- ledger ----

pub fn ledger_make_value(
    s: &AppState,
    domain: &str,
    subject: &str,
    probs: Option<Value>,
    p: Option<f64>,
    due_days: Option<i64>,
    due_at: Option<i64>,
) -> Value {
    if subject.trim().is_empty() {
        return json!({ "error": "thiếu 'subject'" });
    }
    let probs = match (probs, p) {
        (Some(v), _) if v.is_object() => v,
        (_, Some(p)) if (0.0..=1.0).contains(&p) => {
            json!({ "yes": round3(p), "no": round3(1.0 - p) })
        }
        _ => {
            return json!({ "error": "cần 'probs' (map outcome→xác suất) hoặc 'p' (0..1 cho outcome yes/no)" })
        }
    };
    let sum: f64 = probs
        .as_object()
        .map(|m| m.values().filter_map(|v| v.as_f64()).sum())
        .unwrap_or(0.0);
    if !(0.98..=1.02).contains(&sum) {
        return json!({ "error": format!("tổng xác suất phải ≈ 1 (đang là {sum:.3})") });
    }
    let due = due_at.unwrap_or_else(|| now() + due_days.unwrap_or(7).clamp(0, 3650) * 86400);
    let domain = if ["football", "lottery", "weather", "market", "generic"].contains(&domain) {
        domain
    } else {
        "generic"
    };
    match s.db.add_prediction(&PredictionInput {
        domain: domain.into(),
        subject: subject.trim().into(),
        detail: json!({ "manual": true }),
        probs,
        due_at: due,
    }) {
        Ok(id) => json!({ "ok": true, "id": id, "due_at": due }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub fn ledger_list_value(
    s: &AppState,
    domain: Option<&str>,
    status: Option<&str>,
    limit: i64,
) -> Value {
    json!({ "predictions": s.db.list_predictions(domain, status, limit.clamp(1, 500)) })
}

pub async fn ledger_resolve_value(s: &AppState, id: i64, outcome: &str) -> Value {
    let Some(p) = s.db.get_prediction(id) else {
        return json!({ "error": "không có dự đoán này" });
    };
    if p.resolved_at.is_some() {
        return json!({ "error": "đã resolve rồi" });
    }
    let (b, correct) = ledger::score(&p.probs, outcome);
    if let Err(e) = s.db.resolve_prediction(id, outcome, b, correct) {
        return json!({ "error": e.to_string() });
    }
    // Postmortem (điều răn 8): với dự đoán có trace thuộc một chủ đề, rút MỘT
    // bài học quy trình và lưu vào tri thức chủ đề (topic_rules source=lesson).
    let mut lesson_out = Value::Null;
    if p.detail.get("trace").map(|t| !t.is_null()).unwrap_or(false) {
        if let Some(topic_name) = p.detail["topic"].as_str() {
            if let Some((tid, _, _, _)) = s.db.find_topic(topic_name) {
                let dossier = json!({
                    "question": p.subject,
                    "p_committed": p.probs,
                    "trace": p.detail["trace"],
                    "outcome": outcome,
                    "brier": round3(b),
                    "correct": correct,
                });
                if let Some(lesson) = llm::sf_lesson(&s.sc, &dossier).await {
                    let conf = if correct { 0.6 } else { 0.4 };
                    if s.db
                        .add_topic_rule(tid, &format!("Bài học: {lesson}"), conf, "lesson")
                        .is_ok()
                    {
                        s.db.log("lesson", &lesson, topic_name);
                        lesson_out = json!(lesson);
                    }
                }
            }
        }
    }
    json!({ "ok": true, "id": id, "outcome": outcome, "brier": round3(b), "correct": correct, "lesson": lesson_out })
}

pub fn ledger_score_value(s: &AppState) -> Value {
    json!({
        "summary": s.db.score_summary(),
        "calibration": s.db.calibration_buckets(),
        "explain": "Brier: 0 = hoàn hảo, 2 = sai hoàn toàn. Calibration: nhóm dự đoán theo % tự tin — nhóm 70% lý tưởng phải đúng ~70%.",
    })
}

// ---- generic topics ("form chung") ----

#[cfg(test)]
pub fn topic_create_value(s: &AppState, name: &str, description: &str, fields: &Value) -> Value {
    topic_create_full(s, name, description, fields, &json!({}), "")
}

/// Tạo chủ đề với đủ hai phần: **tĩnh** (bối cảnh cố định + tài liệu hướng dẫn)
/// và **động** (schema dữ liệu nhập theo thời gian).
pub fn topic_create_full(
    s: &AppState,
    name: &str,
    description: &str,
    fields: &Value,
    static_cfg: &Value,
    guide: &str,
) -> Value {
    if name.trim().is_empty() {
        return json!({ "error": "thiếu 'name'" });
    }
    let parsed = topic::parse_fields(fields);
    if parsed.is_empty() {
        return json!({ "error": "cần ít nhất một trường dữ liệu động (vd ngày, giá trị)" });
    }
    let static_map = topic::parse_static(static_cfg);
    match s.db.create_topic_full(
        name,
        description,
        &topic::fields_json(&parsed),
        &json!({ "kind": "manual" }),
        &static_map,
        guide,
    ) {
        Ok(id) => {
            s.db.log(
                "topic",
                &format!("tạo chủ đề '{}'", name.trim()),
                &id.to_string(),
            );
            json!({ "ok": true, "id": id, "fields": topic::fields_json(&parsed), "static": static_map, "guide": guide.trim() })
        }
        Err(e) if e.to_string().contains("UNIQUE") => json!({ "error": "đã có chủ đề trùng tên" }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub fn topic_list_value(s: &AppState) -> Value {
    json!({ "topics": s.db.list_topics() })
}

fn topic_or_err(s: &AppState, key: &str) -> Result<(i64, String, String, Value), Value> {
    s.db.find_topic(key).ok_or_else(|| {
        json!({ "error": format!("không có chủ đề '{key}' — xem danh sách bằng predict_topic_list") })
    })
}

pub fn topic_add_value(s: &AppState, key: &str, data: &Value, note: &str) -> Value {
    let (tid, _, _, fields) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let fields = topic::parse_fields(&fields);
    match topic::validate_record(&fields, data) {
        Ok(rec) => match s.db.add_topic_record(tid, &rec, note) {
            Ok(id) => json!({ "ok": true, "id": id, "data": rec }),
            Err(e) => json!({ "error": e.to_string() }),
        },
        Err(e) => json!({ "error": e }),
    }
}

/// Bulk import: `csv` text (header row → field names) or `records` JSON array.
pub fn topic_import_value(
    s: &AppState,
    key: &str,
    csv: Option<&str>,
    records: Option<&Value>,
) -> Value {
    let (tid, _, _, fields) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let fields = topic::parse_fields(&fields);
    let (rows, mut errors) = match (csv, records.and_then(|r| r.as_array())) {
        (Some(csv), _) if !csv.trim().is_empty() => topic::parse_csv_records(&fields, csv),
        (_, Some(arr)) => {
            let mut ok = Vec::new();
            let mut errs = Vec::new();
            for (i, r) in arr.iter().enumerate() {
                match topic::validate_record(&fields, r) {
                    Ok(v) => ok.push(v),
                    Err(e) => errs.push(format!("bản ghi {}: {e}", i + 1)),
                }
            }
            (ok, errs)
        }
        _ => {
            return json!({ "error": "cần 'csv' (chuỗi, dòng đầu là tên trường) hoặc 'records' (mảng object)" })
        }
    };
    let mut imported = 0usize;
    for r in &rows {
        match s.db.add_topic_record(tid, r, "import") {
            Ok(_) => imported += 1,
            Err(e) => errors.push(e.to_string()),
        }
    }
    s.db.log(
        "topic",
        &format!("import {imported} bản ghi vào chủ đề #{tid}"),
        &tid.to_string(),
    );
    json!({ "ok": true, "imported": imported, "errors": errors })
}

/// Thêm tài liệu / thông tin ngoài số liệu cho chủ đề. `date` gắn tài liệu với
/// một ngày, `ref` gắn với một giá trị/ngữ cảnh (vd "giá=124", "đợt lạnh").
pub fn topic_doc_add_value(
    s: &AppState,
    key: &str,
    title: &str,
    content: &str,
    date: &str,
    r#ref: &str,
) -> Value {
    let (tid, _, _, _) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    if title.trim().is_empty() && content.trim().is_empty() {
        return json!({ "error": "cần 'title' hoặc 'content'" });
    }
    if !date.trim().is_empty() && crate::timeutil::parse_date_days(date.trim()).is_none() {
        return json!({ "error": "'date' phải dạng YYYY-MM-DD (hoặc bỏ trống)" });
    }
    match s.db.add_topic_doc(tid, title, content, date, r#ref) {
        Ok(id) => json!({ "ok": true, "id": id }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

pub fn topic_docs_value(s: &AppState, key: &str, q: &str, limit: i64) -> Value {
    let (tid, name, _, _) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    json!({ "topic": name, "docs": s.db.list_topic_docs(tid, q, limit.clamp(1, 200)) })
}

/// Tài liệu liên quan tới một câu hỏi: khớp ngày trong câu hỏi/bản ghi gần đây,
/// khớp từ khoá, cộng các tài liệu chung (không gắn ngày).
fn relevant_docs(s: &AppState, tid: i64, question: &str, limit: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<i64> = Vec::new();
    let mut push = |docs: Vec<Value>, out: &mut Vec<Value>, seen: &mut Vec<i64>| {
        for d in docs {
            if let Some(id) = d["id"].as_i64() {
                if !seen.contains(&id) && out.len() < limit {
                    seen.push(id);
                    out.push(d);
                }
            }
        }
    };
    // Từ khoá dài trong câu hỏi.
    let mut words: Vec<&str> = question
        .split_whitespace()
        .filter(|w| w.chars().count() >= 4)
        .collect();
    words.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
    for w in words.into_iter().take(3) {
        push(s.db.list_topic_docs(tid, w, 5), &mut out, &mut seen);
    }
    // Còn chỗ thì lấy tài liệu mới nhất (ưu tiên có ngày).
    push(
        s.db.list_topic_docs(tid, "", limit as i64),
        &mut out,
        &mut seen,
    );
    out
}

pub fn topic_search_value(s: &AppState, key: &str, q: &str, limit: i64) -> Value {
    let (tid, name, _, fields) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    json!({
        "topic": name,
        "fields": fields,
        "records": s.db.search_topic_records(tid, q, limit.clamp(1, 500)),
    })
}

fn topic_meta_json(name: &str, description: &str, fields: &Value, n_records: usize) -> Value {
    json!({ "name": name, "description": description, "fields": fields, "total_records": n_records })
}

/// Meta + bối cảnh TĨNH + tài liệu hướng dẫn — dùng cho mọi lời gọi AI của chủ đề.
fn topic_meta_ctx(
    s: &AppState,
    tid: i64,
    name: &str,
    description: &str,
    fields: &Value,
    n_records: usize,
) -> Value {
    let (static_map, guide) = s.db.topic_context(tid);
    let mut m = topic_meta_json(name, description, fields, n_records);
    m["static"] = static_map;
    m["guide"] = json!(guide);
    m
}

/// AI thiết kế chủ đề từ mô tả tự do — trả về PROPOSAL cho người dùng sửa
/// trước khi tạo (UI), hoặc để tạo thẳng (agent).
pub async fn topic_design_value(s: &AppState, wish: &str) -> Value {
    if wish.trim().is_empty() {
        return json!({ "error": "hãy mô tả chủ đề bạn muốn theo dõi & dự đoán" });
    }
    match llm::design_topic(&s.sc, wish.trim()).await {
        Some(proposal) => json!({ "ok": true, "proposal": proposal }),
        None => {
            json!({ "error": "AI không thiết kế được (bridge lỗi hoặc mô tả quá mơ hồ) — thử mô tả rõ hơn hoặc tự thiết lập trường" })
        }
    }
}

/// Tạo chủ đề từ template. Với `weather`, địa điểm nhập TỰ DO: nơi nào chưa có
/// trong bảng built-in sẽ được geocode (Open-Meteo) và lưu toạ độ trước khi tạo.
pub async fn topic_from_template_async(s: &AppState, template: &str, params: &Value) -> Value {
    if template == "weather" {
        if let Some(city) = params["city"]
            .as_str()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            if s.db.city_coord(city).is_none() {
                let added =
                    place_add_value(s, city, params["lat"].as_f64(), params["lon"].as_f64()).await;
                if added["error"].is_string() {
                    return added;
                }
                let resolved = added["name"].as_str().unwrap_or(city).to_string();
                let mut p = params.clone();
                p["city"] = json!(resolved);
                return topic_from_template_value(s, template, &p);
            }
        }
    }
    topic_from_template_value(s, template, params)
}

/// Tạo chủ đề từ template (công cụ build) + sync ngay một nhịp từ dữ liệu local.
pub fn topic_from_template_value(s: &AppState, template: &str, params: &Value) -> Value {
    // Giải bóng đá: id tự do — nhớ tên hiển thị người dùng đặt.
    if template == "football" {
        if let (Some(id), Some(label)) = (
            params["league"]
                .as_str()
                .map(str::trim)
                .filter(|l| !l.is_empty()),
            params["league_name"]
                .as_str()
                .map(str::trim)
                .filter(|l| !l.is_empty()),
        ) {
            let _ = s.db.add_custom_league(id, label);
        }
    }
    let Some((mut name, description, fields, source)) = builder::instantiate(template, params)
    else {
        return json!({ "error": format!("template '{template}' không tồn tại hoặc tham số sai (city/league)") });
    };
    // Giải tự thêm: dùng đúng tên người dùng đặt thay vì "Giải khác".
    if let Some(id) = source["league"].as_str() {
        name = format!("Bóng đá {}", s.db.league_label(id));
    }
    let parsed = topic::parse_fields(&fields);
    match s
        .db
        .create_topic_src(&name, &description, &topic::fields_json(&parsed), &source)
    {
        Ok(id) => {
            // Bối cảnh TĨNH của connector: vị trí (weather) / giải (football).
            if let Some(city) = source["city"].as_str() {
                let _ = s.db.set_topic_static_key(id, "vị trí", city);
            }
            if let Some(league) = source["league"].as_str() {
                let _ =
                    s.db.set_topic_static_key(id, "giải", &s.db.league_label(league));
            }
            let appended = engine::sync_topic(s, id, &source);
            s.db.log(
                "topic",
                &format!("build chủ đề '{name}' từ template {template}"),
                &id.to_string(),
            );
            json!({ "ok": true, "id": id, "name": name, "source": source, "synced": appended })
        }
        Err(e) if e.to_string().contains("UNIQUE") => {
            json!({ "error": format!("đã có chủ đề '{name}' rồi") })
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

/// Sửa **nguồn dữ liệu** của chủ đề connector: đổi địa điểm (weather, geocode
/// tự do) hoặc đổi giải (football). Cấu hình nguồn thuộc về từng chủ đề.
pub async fn topic_source_update_value(s: &AppState, key: &str, patch: &Value) -> Value {
    let (tid, current_name, _, _) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let mut source = s.db.topic_source(tid);
    // Tên mặc định theo nguồn cũ → sẽ đổi theo nguồn mới; tên do user đặt giữ nguyên.
    let default_old_name = match source["kind"].as_str().unwrap_or("") {
        "weather" => source["city"].as_str().map(|c| format!("Thời tiết {c}")),
        "football" => source["league"]
            .as_str()
            .map(|l| format!("Bóng đá {}", s.db.league_label(l))),
        _ => None,
    };
    let name_is_default = default_old_name.as_deref() == Some(current_name.as_str());
    match source["kind"].as_str().unwrap_or("manual") {
        "weather" => {
            let Some(city) = patch["city"]
                .as_str()
                .map(str::trim)
                .filter(|c| !c.is_empty())
            else {
                return json!({ "error": "cần 'city' — tên địa điểm bất kỳ" });
            };
            let resolved = match s.db.city_coord(city) {
                Some((n, _, _)) => n,
                None => {
                    let added =
                        place_add_value(s, city, patch["lat"].as_f64(), patch["lon"].as_f64())
                            .await;
                    if added["error"].is_string() {
                        return added;
                    }
                    added["name"].as_str().unwrap_or(city).to_string()
                }
            };
            source["city"] = json!(resolved);
        }
        "football" => {
            let Some(league) = patch["league"]
                .as_str()
                .map(str::trim)
                .filter(|l| !l.is_empty())
            else {
                return json!({ "error": "cần 'league' — id giải trên TheSportsDB" });
            };
            if !league.chars().all(|c| c.is_ascii_digit()) {
                return json!({ "error": "id giải phải là số (vd 4328 = Ngoại hạng Anh)" });
            }
            if let Some(label) = patch["league_name"]
                .as_str()
                .map(str::trim)
                .filter(|l| !l.is_empty())
            {
                let _ = s.db.add_custom_league(league, label);
            }
            source["league"] = json!(league);
        }
        _ => return json!({ "error": "chủ đề nhập tay không có nguồn để cấu hình" }),
    }
    if let Err(e) = s.db.set_topic_source(tid, &source) {
        return json!({ "error": e.to_string() });
    }
    if let Some(city) = source["city"].as_str() {
        let _ = s.db.set_topic_static_key(tid, "vị trí", city);
    }
    if let Some(league) = source["league"].as_str() {
        let _ =
            s.db.set_topic_static_key(tid, "giải", &s.db.league_label(league));
    }
    // Đổi tên theo nguồn mới nếu tên đang là mặc định (kéo cả domain sổ điểm).
    let mut renamed = Value::Null;
    if name_is_default {
        let new_name = match source["kind"].as_str().unwrap_or("") {
            "weather" => source["city"].as_str().map(|c| format!("Thời tiết {c}")),
            "football" => source["league"]
                .as_str()
                .map(|l| format!("Bóng đá {}", s.db.league_label(l))),
            _ => None,
        };
        if let Some(n) = new_name.filter(|n| n != &current_name) {
            let r = topic_update_value(s, &tid.to_string(), Some(&n), None, None);
            if r["ok"] == json!(true) {
                renamed = json!(n);
            }
        }
    }
    s.db.log(
        "topic",
        &format!("đổi nguồn chủ đề #{tid} → {source}"),
        &tid.to_string(),
    );
    // Kéo dữ liệu mới về ngay cho nguồn vừa đổi.
    engine::ensure_weather(s, true).await;
    engine::ensure_fixtures(s, true).await;
    let appended = engine::sync_topic(s, tid, &source);
    json!({ "ok": true, "source": source, "appended": appended, "renamed": renamed })
}

/// Sửa chủ đề: tên / mô tả / trường. Đổi tên kéo theo đổi domain của các dự
/// đoán cũ để sổ điểm chủ đề không bị đứt gãy.
pub fn topic_update_value(
    s: &AppState,
    key: &str,
    name: Option<&str>,
    description: Option<&str>,
    fields: Option<&Value>,
) -> Value {
    topic_update_full(s, key, name, description, fields, None, None)
}

/// Sửa chủ đề đầy đủ: tên/mô tả/trường ĐỘNG + cấu hình TĨNH + tài liệu hướng dẫn.
pub fn topic_update_full(
    s: &AppState,
    key: &str,
    name: Option<&str>,
    description: Option<&str>,
    fields: Option<&Value>,
    static_cfg: Option<&Value>,
    guide: Option<&str>,
) -> Value {
    let (tid, old_name, _, _) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let new_fields = fields.map(|f| topic::fields_json(&topic::parse_fields(f)));
    if let Some(f) = &new_fields {
        if f.as_array().map(|a| a.is_empty()).unwrap_or(true) {
            return json!({ "error": "cần ít nhất một trường dữ liệu" });
        }
    }
    if let Err(e) =
        s.db.update_topic(tid, name, description, new_fields.as_ref())
    {
        return json!({
            "error": if e.to_string().contains("UNIQUE") { "đã có chủ đề trùng tên".to_string() } else { e.to_string() }
        });
    }
    let static_map = static_cfg.map(topic::parse_static);
    if static_map.is_some() || guide.is_some() {
        if let Err(e) = s.db.set_topic_context(tid, static_map.as_ref(), guide) {
            return json!({ "error": e.to_string() });
        }
    }
    let mut moved = 0usize;
    if let Some(n) = name
        .map(str::trim)
        .filter(|n| !n.is_empty() && *n != old_name)
    {
        let (from, to) = (topic::ledger_domain(&old_name), topic::ledger_domain(n));
        moved = s.db.rename_prediction_domain(&from, &to).unwrap_or(0);
        s.db.log(
            "topic",
            &format!("đổi tên '{old_name}' → '{n}' ({moved} dự đoán chuyển domain)"),
            &tid.to_string(),
        );
    }
    let (_, name_now, desc_now, fields_now) = s.db.find_topic(&tid.to_string()).unwrap();
    let (static_now, guide_now) = s.db.topic_context(tid);
    json!({ "ok": true, "id": tid, "name": name_now, "description": desc_now, "fields": fields_now,
            "static": static_now, "guide": guide_now, "predictions_moved": moved })
}

pub fn topic_sync_value(s: &AppState, key: &str) -> Value {
    let (tid, name, _, _) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let source = s.db.topic_source(tid);
    if source["kind"].as_str().unwrap_or("manual") == "manual" {
        return json!({ "error": "chủ đề nhập tay — không có connector để sync" });
    }
    json!({ "ok": true, "topic": name, "appended": engine::sync_topic(s, tid, &source) })
}

/// Dashboard riêng cho một chủ đề: thống kê, chuỗi thời gian, quy luật & bài
/// học, sổ điểm domain, dự đoán đang mở, bản ghi gần nhất.
pub fn topic_dashboard_value(s: &AppState, key: &str) -> Value {
    let (tid, name, description, fields) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let parsed = topic::parse_fields(&fields);
    let records = s.db.search_topic_records(tid, "", 365);
    let domain = topic::ledger_domain(&name);
    let score =
        s.db.score_summary()
            .into_iter()
            .find(|d| d["domain"].as_str() == Some(domain.as_str()));
    let (static_map, guide) = s.db.topic_context(tid);
    json!({
        "id": tid,
        "name": name,
        "description": description,
        "fields": fields,
        "static": static_map,
        "guide": guide,
        "source": s.db.topic_source(tid),
        "records_total": records.len(),
        "stats": topic::numeric_summary(&parsed, &records),
        "series": topic::series_by_date(&parsed, &records),
        "rules": s.db.list_topic_rules(tid),
        "domain": domain,
        "score": score,
        "open_predictions": s.db.list_predictions(Some(&domain), Some("open"), 10),
        "recent_resolved": s.db.track_record(&domain, 5),
        "latest_records": records.iter().take(10).collect::<Vec<_>>(),
        "docs": s.db.list_topic_docs(tid, "", 30),
    })
}

pub async fn topic_analyze_value(s: &AppState, key: &str) -> Value {
    let (tid, name, description, fields) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let records = s.db.search_topic_records(tid, "", 60);
    if records.is_empty() {
        return json!({ "error": "chủ đề chưa có dữ liệu — thêm bản ghi trước khi phân tích" });
    }
    let mut meta = topic_meta_ctx(s, tid, &name, &description, &fields, records.len());
    meta["documents"] = json!(s.db.list_topic_docs(tid, "", 15));
    match llm::topic_analyze(&s.sc, &meta, &json!(records)).await {
        Some(text) => json!({ "topic": name, "records_used": records.len(), "analysis": text }),
        None => json!({ "error": "LLM bridge không khả dụng — thử lại sau" }),
    }
}

pub async fn topic_rules_value(s: &AppState, key: &str, derive: bool) -> Value {
    let (tid, name, description, fields) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    if derive {
        let records = s.db.search_topic_records(tid, "", 80);
        if records.is_empty() {
            return json!({ "error": "chủ đề chưa có dữ liệu — không có gì để rút quy luật" });
        }
        let mut meta = topic_meta_ctx(s, tid, &name, &description, &fields, records.len());
        meta["documents"] = json!(s.db.list_topic_docs(tid, "", 15));
        let derived = llm::topic_derive_rules(&s.sc, &meta, &json!(records)).await;
        if derived.is_empty() {
            return json!({ "error": "LLM không rút được quy luật (bridge lỗi hoặc trả về không hợp lệ)" });
        }
        let _ = s.db.clear_ai_rules(tid);
        for (rule, conf) in &derived {
            let _ = s.db.add_topic_rule(tid, rule, *conf, "ai");
        }
        s.db.log(
            "topic",
            &format!("rút {} quy luật cho '{}'", derived.len(), name),
            &tid.to_string(),
        );
    }
    json!({ "topic": name, "rules": s.db.list_topic_rules(tid) })
}

pub fn topic_rule_add_value(s: &AppState, key: &str, rule: &str, confidence: f64) -> Value {
    let (tid, _, _, _) = match topic_or_err(s, key) {
        Ok(t) => t,
        Err(e) => return e,
    };
    if rule.trim().is_empty() {
        return json!({ "error": "thiếu 'rule'" });
    }
    match s.db.add_topic_rule(tid, rule, confidence, "user") {
        Ok(id) => json!({ "ok": true, "id": id }),
        Err(e) => json!({ "error": e.to_string() }),
    }
}

/// "Điều X có xảy ra không?" — full **Siêu Dự Báo** pipeline:
/// (1) Fermi decompose → (2) nền tảng dữ liệu (thống kê + quy luật + bài học +
/// track record) + bằng chứng ngoài qua Search app → (3) tổng hợp theo
/// checklist Tetlock thành trace có cấu trúc. Always ledgered (trace kèm theo)
/// for later Brier scoring + postmortem. Falls back to the simple single-call
/// forecast when the synthesizer fails.
pub async fn topic_ask_value(
    s: &AppState,
    key: Option<&str>,
    question: &str,
    due_days: i64,
) -> Value {
    if question.trim().is_empty() {
        return json!({ "error": "thiếu 'question'" });
    }
    // ---- Nền tảng dữ liệu (topic context) ----
    let mut docs: Vec<Value> = Vec::new();
    let (meta, rules, relevant, stats, domain, topic_name) = match key {
        Some(k) if !k.trim().is_empty() => {
            let (tid, name, description, fields) = match topic_or_err(s, k) {
                Ok(t) => t,
                Err(e) => return e,
            };
            let recent = s.db.search_topic_records(tid, "", 20);
            // Keyword hits from the question's longest words widen the context.
            let mut seen: Vec<i64> = recent.iter().filter_map(|r| r["id"].as_i64()).collect();
            let mut relevant = recent;
            let mut words: Vec<&str> = question
                .split_whitespace()
                .filter(|w| w.chars().count() >= 4)
                .collect();
            words.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
            for w in words.into_iter().take(3) {
                for hit in s.db.search_topic_records(tid, w, 10) {
                    if let Some(id) = hit["id"].as_i64() {
                        if !seen.contains(&id) {
                            seen.push(id);
                            relevant.push(hit);
                        }
                    }
                }
            }
            let parsed_fields = topic::parse_fields(&fields);
            let stats = topic::numeric_summary(&parsed_fields, &relevant);
            docs = relevant_docs(s, tid, question, 8);
            (
                Some(topic_meta_ctx(
                    s,
                    tid,
                    &name,
                    &description,
                    &fields,
                    seen.len(),
                )),
                json!(s.db.list_topic_rules(tid)),
                relevant,
                stats,
                topic::ledger_domain(&name),
                Some(name),
            )
        }
        _ => (
            None,
            json!([]),
            vec![],
            json!({}),
            "generic".to_string(),
            None,
        ),
    };
    let track = s.db.track_record(&domain, 10);

    // ---- Bước 1: Fermi decompose ----
    let (sub_questions, mut queries) = llm::sf_decompose(&s.sc, meta.as_ref(), question).await;
    if queries.is_empty() {
        queries = vec![question.trim().to_string()];
    }

    // ---- Bước 2: bằng chứng ngoài — khám phá MCP động, không gắn cứng địa chỉ ----
    let all_sources = evidence::discover(&s.http, &s.sc.base_url).await;
    let chosen = evidence::select(
        &all_sources,
        &s.db.get_setting("search_mcp").unwrap_or_default(),
        2,
    );
    let (news, news_note) = evidence::gather(&s.http, &chosen, &queries, 6).await;

    // ---- Bước 3: tổng hợp theo checklist Siêu Dự Báo ----
    let dossier = json!({
        "question": question.trim(),
        "topic": meta,
        "static_context": meta.as_ref().map(|m| m["static"].clone()).unwrap_or(json!({})),
        "guide": meta.as_ref().and_then(|m| m["guide"].as_str()).unwrap_or(""),
        "decomposition": sub_questions,
        "data_stats": stats,
        "recent_records": relevant.iter().take(20).collect::<Vec<_>>(),
        "documents": docs,
        "rules_and_lessons": rules,
        "track_record": track,
        "external_evidence": news,
        "evidence_note": news_note,
    });
    let checklist = methodology::methodology_prompt(&s.db);
    let (p, trace, mode) = match llm::sf_synthesize(&s.sc, &dossier, &checklist).await {
        Some(trace) => {
            let p = trace["p"].as_f64().unwrap_or(0.5);
            (p, Some(trace), "superforecast")
        }
        None => {
            // Fallback: single-call simple forecast.
            match llm::topic_forecast(&s.sc, meta.as_ref(), &rules, &json!(relevant), question)
                .await
            {
                Some((p, reasoning)) => (
                    p,
                    Some(json!({ "p": round3(p), "reasoning": reasoning })),
                    "simple",
                ),
                None => return json!({ "error": "LLM bridge không khả dụng — thử lại sau" }),
            }
        }
    };
    let due = now() + due_days.clamp(0, 3650) * 86400;
    let ledger_id =
        s.db.add_prediction(&PredictionInput {
            domain: domain.clone(),
            subject: question.trim().into(),
            detail: json!({
                "topic": topic_name,
                "question": question.trim(),
                "trace": trace,
                "mode": mode,
                "external_evidence_n": news.len(),
            }),
            probs: json!({ "yes": round3(p), "no": round3(1.0 - p) }),
            due_at: due,
        })
        .ok();
    s.db.log(
        "ask",
        &format!("siêu dự báo p={:.2} ({mode}): {}", p, question.trim()),
        &domain,
    );
    json!({
        "question": question.trim(),
        "topic": topic_name,
        "p_yes": round3(p),
        "mode": mode,
        "trace": trace,
        "decomposition": sub_questions,
        "external_evidence": news,
        "evidence_note": news_note,
        "domain": domain,
        "ledger_id": ledger_id,
        "due_at": due,
        "resolve_hint": "khi biết kết quả: predict_resolve {id, outcome: \"yes\"|\"no\"} — sổ chấm Brier + tự rút bài học (postmortem)",
    })
}

// ---- settings ----

/// Cài đặt CHUNG chỉ còn thứ thật sự dùng chung. Nguồn dữ liệu (địa điểm thời
/// tiết, giải bóng đá) là cấu hình CỦA TỪNG CHỦ ĐỀ — xem `/topics/:key/source`.
pub fn settings_value(s: &AppState) -> Value {
    json!({
        "search_mcp": s.db.get_setting("search_mcp").filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "auto".into()),
        "theme": s.db.get_setting("theme").unwrap_or_else(|| "system".into()),
        // Chỉ để hiển thị: nguồn nào đang được các chủ đề kéo về.
        "active_sources": {
            "weather_places": s.db.cities(),
            "football_leagues": s.db.leagues().iter().map(|id| json!({ "id": id, "name": s.db.league_label(id) })).collect::<Vec<_>>(),
        },
        "suggested_places": fetch::CITIES.iter().map(|(n, _, _)| *n).collect::<Vec<_>>(),
        "suggested_leagues": fetch::LEAGUES.iter().map(|(id, n)| json!({ "id": id, "name": n })).collect::<Vec<_>>(),
    })
}

/// Thêm địa điểm bất kỳ: geocode theo tên rồi lưu toạ độ (Open-Meteo, keyless).
pub async fn place_add_value(
    s: &AppState,
    query: &str,
    lat: Option<f64>,
    lon: Option<f64>,
) -> Value {
    let q = query.trim();
    if q.is_empty() {
        return json!({ "error": "nhập tên địa điểm cần thêm" });
    }
    // Toạ độ do người dùng cung cấp thì dùng thẳng, khỏi geocode.
    if let (Some(lat), Some(lon)) = (lat, lon) {
        return match s.db.add_custom_place(q, lat, lon, "toạ độ tự nhập") {
            Ok(()) => json!({ "ok": true, "name": q, "lat": lat, "lon": lon }),
            Err(e) => json!({ "error": e.to_string() }),
        };
    }
    match fetch::geocode(&s.http, q).await {
        Ok(hits) if !hits.is_empty() => {
            let (name, lat, lon, note) = hits[0].clone();
            match s.db.add_custom_place(&name, lat, lon, &note) {
                Ok(()) => json!({
                    "ok": true, "name": name, "lat": lat, "lon": lon, "note": note,
                    "alternatives": hits.iter().skip(1).map(|(n, la, lo, nt)| json!({ "name": n, "lat": la, "lon": lo, "note": nt })).collect::<Vec<_>>(),
                }),
                Err(e) => json!({ "error": e.to_string() }),
            }
        }
        Ok(_) => {
            json!({ "error": format!("không tìm thấy địa điểm '{q}' — thử tên khác hoặc nhập lat/lon trực tiếp") })
        }
        Err(e) => json!({ "error": format!("geocoding lỗi: {e}") }),
    }
}

pub fn place_remove_value(s: &AppState, name: &str) -> Value {
    let _ = s.db.remove_custom_place(name);
    // Bỏ luôn khỏi danh sách đang theo dõi.
    let kept: Vec<String> =
        s.db.cities()
            .into_iter()
            .filter(|c| c != name.trim())
            .collect();
    let _ = s.db.set_setting("cities", &kept.join(","));
    json!({ "ok": true })
}

// ---- tri thức (sửa được, reset về mặc định) ----

pub fn method_update_value(s: &AppState, body: &Value) -> Value {
    if body.get("reset").and_then(|r| r.as_bool()).unwrap_or(false) {
        let _ = s.db.set_setting("methodology", "");
        s.db.log(
            "method",
            "khôi phục tri thức mặc định (sách Siêu Dự Báo)",
            "",
        );
        return methodology::methodology_json(&s.db);
    }
    let normalized = methodology::normalize(body);
    match s.db.set_setting("methodology", &normalized.to_string()) {
        Ok(()) => {
            s.db.log("method", "cập nhật tri thức đánh giá", "");
            methodology::methodology_json(&s.db)
        }
        Err(e) => json!({ "error": e.to_string() }),
    }
}

// ---- REST router ----

pub fn api_router(state: AppState) -> Router {
    Router::new()
        .route("/status", get(|State(s): State<AppState>| async move { Json(status_value(&s)) }))
        .route("/overview", get(|State(s): State<AppState>| async move { Json(overview_value(&s)) }))
        .route(
            "/football/today",
            get(|State(s): State<AppState>, Query(q): Query<HashMap<String, String>>| async move {
                let days = q.get("days").and_then(|v| v.parse().ok()).unwrap_or(2);
                Json(football_today_value(&s, days, true).await)
            }),
        )
        .route(
            "/football/predict",
            post(|State(s): State<AppState>, Json(b): Json<Value>| async move {
                let home = b["home"].as_str().unwrap_or("").to_string();
                let away = b["away"].as_str().unwrap_or("").to_string();
                if home.is_empty() || away.is_empty() {
                    return Json(json!({ "error": "cần 'home' và 'away'" }));
                }
                Json(predict_match_value(&s, &home, &away, b["article"].as_bool().unwrap_or(false)).await)
            }),
        )
        .route(
            "/football/elo",
            get(|State(s): State<AppState>, Query(q): Query<HashMap<String, String>>| async move {
                let limit = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(30);
                Json(elo_top_value(&s, limit))
            }),
        )
        .route("/lottery/latest", get(|State(s): State<AppState>| async move { Json(lottery_latest_value(&s).await) }))
        .route(
            "/lottery/stats",
            get(|State(s): State<AppState>, Query(q): Query<HashMap<String, String>>| async move {
                let days = q.get("days").and_then(|v| v.parse().ok()).unwrap_or(30);
                Json(lottery_stats_value(&s, days).await)
            }),
        )
        .route(
            "/lottery/suggest",
            post(|State(s): State<AppState>, Json(b): Json<Value>| async move {
                let n = b["n"].as_u64().unwrap_or(3) as usize;
                Json(lottery_suggest_value(&s, n, b["note"].as_bool().unwrap_or(false)).await)
            }),
        )
        .route(
            "/weather",
            get(|State(s): State<AppState>, Query(q): Query<HashMap<String, String>>| async move {
                let city = q.get("city").cloned().unwrap_or_else(|| "Hà Nội".into());
                let advice = q.get("advice").map(|v| v == "1" || v == "true").unwrap_or(false);
                Json(weather_value(&s, &city, advice).await)
            }),
        )
        .route("/market/gold", get(|State(s): State<AppState>| async move { Json(gold_value(&s).await) }))
        .route(
            "/market/trend",
            get(|State(s): State<AppState>, Query(q): Query<HashMap<String, String>>| async move {
                let note = q.get("note").map(|v| v == "1" || v == "true").unwrap_or(false);
                Json(gold_trend_value(&s, note).await)
            }),
        )
        .route(
            "/brief",
            get(|State(s): State<AppState>, Query(q): Query<HashMap<String, String>>| async move {
                let narrate = q.get("narrate").map(|v| v == "1" || v == "true").unwrap_or(false);
                Json(brief_value(&s, narrate).await)
            }),
        )
        .route(
            "/ledger",
            get(|State(s): State<AppState>, Query(q): Query<HashMap<String, String>>| async move {
                Json(ledger_list_value(
                    &s,
                    q.get("domain").map(|x| x.as_str()).filter(|x| !x.is_empty()),
                    q.get("status").map(|x| x.as_str()).filter(|x| !x.is_empty()),
                    q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(100),
                ))
            })
            .post(|State(s): State<AppState>, Json(b): Json<Value>| async move {
                Json(ledger_make_value(
                    &s,
                    b["domain"].as_str().unwrap_or("generic"),
                    b["subject"].as_str().unwrap_or(""),
                    b.get("probs").cloned().filter(|v| !v.is_null()),
                    b["p"].as_f64(),
                    b["due_days"].as_i64(),
                    b["due_at"].as_i64(),
                ))
            }),
        )
        .route(
            "/ledger/:id/resolve",
            post(|State(s): State<AppState>, Path(id): Path<i64>, Json(b): Json<Value>| async move {
                let outcome = b["outcome"].as_str().unwrap_or("").to_string();
                if outcome.is_empty() {
                    return Json(json!({ "error": "thiếu 'outcome'" }));
                }
                Json(ledger_resolve_value(&s, id, &outcome).await)
            }),
        )
        .route("/ledger/score", get(|State(s): State<AppState>| async move { Json(ledger_score_value(&s)) }))
        .route(
            "/search-sources",
            get(|State(s): State<AppState>| async move {
                let all = evidence::discover(&s.http, &s.sc.base_url).await;
                let setting = s.db.get_setting("search_mcp").unwrap_or_default();
                let active: Vec<String> = evidence::select(&all, &setting, 2).iter().map(|x| x.key()).collect();
                Json(json!({
                    "selected": if setting.trim().is_empty() { "auto".to_string() } else { setting },
                    "active": active,
                    "sources": all.iter().map(|x| x.to_json()).collect::<Vec<_>>(),
                }))
            }),
        )
        .route(
            "/method",
            get(|State(s): State<AppState>| async move { Json(methodology::methodology_json(&s.db)) })
                .post(|State(s): State<AppState>, Json(b): Json<Value>| async move { Json(method_update_value(&s, &b)) }),
        )
        .route(
            "/method/default",
            get(|| async { Json(methodology::default_methodology()) }),
        )
        .route(
            "/places",
            post(|State(s): State<AppState>, Json(b): Json<Value>| async move {
                Json(place_add_value(&s, b["query"].as_str().unwrap_or(""), b["lat"].as_f64(), b["lon"].as_f64()).await)
            })
            .delete(|State(s): State<AppState>, Json(b): Json<Value>| async move {
                Json(place_remove_value(&s, b["name"].as_str().unwrap_or("")))
            }),
        )
        .route(
            "/tick",
            post(|State(s): State<AppState>| async move { Json(engine::run_all(&s, false).await) }),
        )
        .route(
            "/settings",
            get(|State(s): State<AppState>| async move { Json(settings_value(&s)) }).post(
                |State(s): State<AppState>, Json(b): Json<Value>| async move {
                    if let Some(v) = b["search_mcp"].as_str() {
                        let _ = s.db.set_setting("search_mcp", v.trim());
                    }
                    if let Some(arr) = b["search_mcp"].as_array() {
                        let keys: Vec<String> = arr.iter().filter_map(|x| x.as_str().map(String::from)).collect();
                        let _ = s.db.set_setting("search_mcp", &keys.join(","));
                    }
                    if let Some(t) = b["theme"].as_str() {
                        let _ = s.db.set_setting("theme", t.trim());
                    }
                    Json(settings_value(&s))
                },
            ),
        )
        .route(
            "/topics",
            get(|State(s): State<AppState>| async move { Json(topic_list_value(&s)) }).post(
                |State(s): State<AppState>, Json(b): Json<Value>| async move {
                    Json(topic_create_full(
                        &s,
                        b["name"].as_str().unwrap_or(""),
                        b["description"].as_str().unwrap_or(""),
                        b.get("fields").unwrap_or(&json!([])),
                        b.get("static").unwrap_or(&json!({})),
                        b["guide"].as_str().unwrap_or(""),
                    ))
                },
            ),
        )
        .route(
            "/topics/templates",
            get(|| async { Json(json!({ "templates": builder::templates_json() })) }),
        )
        .route(
            "/topics/from-template",
            post(|State(s): State<AppState>, Json(b): Json<Value>| async move {
                Json(topic_from_template_async(&s, b["template"].as_str().unwrap_or(""), &b["params"]).await)
            }),
        )
        .route(
            "/topics/design",
            post(|State(s): State<AppState>, Json(b): Json<Value>| async move {
                Json(topic_design_value(&s, b["wish"].as_str().unwrap_or("")).await)
            }),
        )
        .route(
            "/topics/:key/source",
            post(|State(s): State<AppState>, Path(key): Path<String>, Json(b): Json<Value>| async move {
                Json(topic_source_update_value(&s, &key, &b).await)
            }),
        )
        .route(
            "/topics/:key/sync",
            post(|State(s): State<AppState>, Path(key): Path<String>| async move {
                // Refresh nguồn local trước rồi mới nạp vào chủ đề.
                engine::ensure_gold(&s, false).await;
                engine::ensure_weather(&s, false).await;
                Json(topic_sync_value(&s, &key))
            }),
        )
        .route(
            "/topics/:key/dashboard",
            get(|State(s): State<AppState>, Path(key): Path<String>| async move {
                Json(topic_dashboard_value(&s, &key))
            }),
        )
        .route(
            "/topics/:key",
            post(|State(s): State<AppState>, Path(key): Path<String>, Json(b): Json<Value>| async move {
                Json(topic_update_full(
                    &s,
                    &key,
                    b["name"].as_str(),
                    b["description"].as_str(),
                    b.get("fields").filter(|f| f.is_array()),
                    b.get("static").filter(|v| v.is_object() || v.is_array()),
                    b["guide"].as_str(),
                ))
            })
            .delete(|State(s): State<AppState>, Path(key): Path<String>| async move {
                match s.db.find_topic(&key) {
                    Some((tid, name, _, _)) => {
                        let _ = s.db.delete_topic(tid);
                        s.db.log("topic", &format!("xoá chủ đề '{name}'"), &tid.to_string());
                        Json(json!({ "ok": true }))
                    }
                    None => Json(json!({ "error": "không có chủ đề này" })),
                }
            }),
        )
        .route(
            "/topics/:key/records",
            get(|State(s): State<AppState>, Path(key): Path<String>, Query(q): Query<HashMap<String, String>>| async move {
                Json(topic_search_value(
                    &s,
                    &key,
                    q.get("q").map(|x| x.as_str()).unwrap_or(""),
                    q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(100),
                ))
            })
            .post(|State(s): State<AppState>, Path(key): Path<String>, Json(b): Json<Value>| async move {
                if b.get("csv").is_some() || b.get("records").is_some() {
                    Json(topic_import_value(&s, &key, b["csv"].as_str(), b.get("records")))
                } else {
                    Json(topic_add_value(&s, &key, b.get("data").unwrap_or(&b), b["note"].as_str().unwrap_or("")))
                }
            }),
        )
        .route(
            "/topics/:key/records/:rid",
            axum::routing::delete(|State(s): State<AppState>, Path((key, rid)): Path<(String, i64)>| async move {
                match s.db.find_topic(&key) {
                    Some((tid, _, _, _)) => {
                        let _ = s.db.delete_topic_record(tid, rid);
                        Json(json!({ "ok": true }))
                    }
                    None => Json(json!({ "error": "không có chủ đề này" })),
                }
            }),
        )
        .route(
            "/topics/:key/docs",
            get(|State(s): State<AppState>, Path(key): Path<String>, Query(q): Query<HashMap<String, String>>| async move {
                Json(topic_docs_value(
                    &s,
                    &key,
                    q.get("q").map(|x| x.as_str()).unwrap_or(""),
                    q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50),
                ))
            })
            .post(|State(s): State<AppState>, Path(key): Path<String>, Json(b): Json<Value>| async move {
                Json(topic_doc_add_value(
                    &s,
                    &key,
                    b["title"].as_str().unwrap_or(""),
                    b["content"].as_str().unwrap_or(""),
                    b["date"].as_str().unwrap_or(""),
                    b["ref"].as_str().unwrap_or(""),
                ))
            }),
        )
        .route(
            "/topics/:key/docs/:did",
            axum::routing::delete(|State(s): State<AppState>, Path((key, did)): Path<(String, i64)>| async move {
                match s.db.find_topic(&key) {
                    Some((tid, _, _, _)) => {
                        let _ = s.db.delete_topic_doc(tid, did);
                        Json(json!({ "ok": true }))
                    }
                    None => Json(json!({ "error": "không có chủ đề này" })),
                }
            }),
        )
        .route(
            "/topics/:key/analyze",
            post(|State(s): State<AppState>, Path(key): Path<String>| async move {
                Json(topic_analyze_value(&s, &key).await)
            }),
        )
        .route(
            "/topics/:key/rules",
            get(|State(s): State<AppState>, Path(key): Path<String>| async move {
                Json(topic_rules_value(&s, &key, false).await)
            })
            .post(|State(s): State<AppState>, Path(key): Path<String>, Json(b): Json<Value>| async move {
                if b["derive"].as_bool().unwrap_or(false) {
                    Json(topic_rules_value(&s, &key, true).await)
                } else {
                    Json(topic_rule_add_value(&s, &key, b["rule"].as_str().unwrap_or(""), b["confidence"].as_f64().unwrap_or(0.5)))
                }
            }),
        )
        .route(
            "/topics/:key/rules/:rid",
            axum::routing::delete(|State(s): State<AppState>, Path((key, rid)): Path<(String, i64)>| async move {
                match s.db.find_topic(&key) {
                    Some((tid, _, _, _)) => {
                        let _ = s.db.delete_topic_rule(tid, rid);
                        Json(json!({ "ok": true }))
                    }
                    None => Json(json!({ "error": "không có chủ đề này" })),
                }
            }),
        )
        .route(
            "/ask",
            post(|State(s): State<AppState>, Json(b): Json<Value>| async move {
                Json(topic_ask_value(
                    &s,
                    b["topic"].as_str(),
                    b["question"].as_str().unwrap_or(""),
                    b["due_days"].as_i64().unwrap_or(30),
                ).await)
            }),
        )
        .route("/activity", get(|State(s): State<AppState>| async move { Json(json!({ "activity": s.db.recent_activity(50) })) }))
        .route("/mcp/sse", get(crate::mcp::mcp_sse))
        .route("/mcp/message", post(crate::mcp::mcp_message))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_json_groups_prizes() {
        let numbers: Vec<i64> = (1..=27).collect();
        let v = draw_json("2026-07-27", &numbers);
        assert_eq!(v["special"], 1);
        assert_eq!(v["prizes"]["g2"].as_array().unwrap().len(), 2);
        assert_eq!(v["prizes"]["g3"].as_array().unwrap().len(), 6);
        assert_eq!(v["prizes"]["g7"].as_array().unwrap().len(), 4);
        assert_eq!(v["loto"].as_array().unwrap().len(), 27);
        assert_eq!(v["loto"][0], "01");
    }

    #[test]
    fn ledger_make_validation() {
        let s = test_state();
        assert!(
            ledger_make_value(&s, "generic", "", None, Some(0.5), None, None)["error"].is_string()
        );
        assert!(ledger_make_value(&s, "generic", "X", None, None, None, None)["error"].is_string());
        let bad = ledger_make_value(
            &s,
            "generic",
            "X",
            Some(json!({ "a": 0.9, "b": 0.4 })),
            None,
            None,
            None,
        );
        assert!(bad["error"].is_string());
        let ok = ledger_make_value(
            &s,
            "custom-domain",
            "VN thắng Thái",
            None,
            Some(0.7),
            Some(3),
            None,
        );
        assert_eq!(ok["ok"], true);
        let p = s.db.get_prediction(ok["id"].as_i64().unwrap()).unwrap();
        assert_eq!(p.domain, "generic"); // unknown domain folded to generic
        assert_eq!(p.probs["yes"], 0.7);
    }

    #[tokio::test]
    async fn ledger_resolve_flow() {
        let s = test_state();
        let ok = ledger_make_value(&s, "generic", "test", None, Some(0.8), Some(0), None);
        let id = ok["id"].as_i64().unwrap();
        let r = ledger_resolve_value(&s, id, "yes").await;
        assert_eq!(r["correct"], true);
        assert!(r["lesson"].is_null()); // manual prediction (no trace) → no postmortem call
        assert!(ledger_resolve_value(&s, id, "yes").await["error"].is_string()); // double resolve
        assert!(ledger_resolve_value(&s, 999, "yes").await["error"].is_string());
    }

    #[test]
    fn next_draw_cutoff() {
        // 10:00 VN → today's draw; 19:00 VN → tomorrow's.
        let days = parse_date_days("2026-07-27").unwrap();
        let ts_morning = days * 86400 - VN_OFFSET + 10 * 3600;
        let (d1, due1) = next_draw(ts_morning);
        assert_eq!(d1, "2026-07-27");
        assert!(due1 > ts_morning);
        let ts_evening = days * 86400 - VN_OFFSET + 19 * 3600;
        let (d2, _) = next_draw(ts_evening);
        assert_eq!(d2, "2026-07-28");
    }

    #[test]
    fn topic_flow_offline() {
        let s = test_state();
        let created = topic_create_value(
            &s,
            "Giá cafe",
            "theo dõi giá cafe hàng ngày",
            &json!([{ "name": "ngày", "kind": "date" }, { "name": "giá", "kind": "number" }]),
        );
        assert_eq!(created["ok"], true);
        assert!(topic_create_value(&s, "Giá cafe", "", &json!([]))["error"].is_string());
        assert!(topic_create_value(&s, "", "", &json!([]))["error"].is_string());

        // Single add (typed coercion) + bad record.
        let add = topic_add_value(
            &s,
            "giá cafe",
            &json!({ "ngày": "2026-07-27", "giá": "120,5" }),
            "",
        );
        assert_eq!(add["ok"], true);
        assert_eq!(add["data"]["giá"], 120.5);
        assert!(topic_add_value(&s, "giá cafe", &json!({ "giá": "xx" }), "")["error"].is_string());

        // CSV import with one bad line.
        let imp = topic_import_value(
            &s,
            "Giá cafe",
            Some("ngày,giá\n2026-07-25,118\nbad,row\n"),
            None,
        );
        assert_eq!(imp["imported"], 1);
        assert_eq!(imp["errors"].as_array().unwrap().len(), 1);

        // JSON records import.
        let imp2 = topic_import_value(
            &s,
            "Giá cafe",
            None,
            Some(&json!([{ "ngày": "2026-07-26", "giá": 119 }])),
        );
        assert_eq!(imp2["imported"], 1);

        // Search.
        let all = topic_search_value(&s, "Giá cafe", "", 50);
        assert_eq!(all["records"].as_array().unwrap().len(), 3);
        let hit = topic_search_value(&s, "Giá cafe", "120.5", 50);
        assert_eq!(hit["records"].as_array().unwrap().len(), 1);
        assert!(topic_search_value(&s, "khác", "", 50)["error"].is_string());

        // Manual rule; analyze/derive error paths without LLM.
        assert_eq!(
            topic_rule_add_value(&s, "Giá cafe", "giá tăng sau mưa", 0.6)["ok"],
            true
        );
        assert!(topic_rule_add_value(&s, "Giá cafe", "", 0.6)["error"].is_string());
    }

    #[test]
    fn template_build_and_dashboard() {
        let s = test_state();
        // Build "Giá vàng" từ template; chưa có giá local → synced 0.
        let made = topic_from_template_value(&s, "gold", &json!({}));
        assert_eq!(made["ok"], true);
        assert_eq!(made["synced"], 0);
        // Trùng tên → lỗi thân thiện.
        assert!(topic_from_template_value(&s, "gold", &json!({}))["error"].is_string());
        assert!(topic_from_template_value(&s, "nope", &json!({}))["error"].is_string());

        // Nạp giá local rồi sync → 1 bản ghi hôm nay; sync lại → dedup 0.
        s.db.add_price("XAU_USD", 4000.0).unwrap();
        s.db.add_price("USD_VND", 26000.0).unwrap();
        s.db.add_price("XAU_VND_LUONG", 125.0).unwrap();
        let sync1 = topic_sync_value(&s, "Giá vàng & tỷ giá");
        assert_eq!(sync1["appended"], 1);
        assert_eq!(topic_sync_value(&s, "Giá vàng & tỷ giá")["appended"], 0);

        // Dashboard shape.
        let d = topic_dashboard_value(&s, "Giá vàng & tỷ giá");
        assert_eq!(d["records_total"], 1);
        assert_eq!(d["source"]["kind"], "gold");
        assert_eq!(d["stats"]["xau_usd"]["latest"], 4000.0);
        assert_eq!(d["series"]["xau_usd"].as_array().unwrap().len(), 1);
        assert!(d["domain"].as_str().unwrap().starts_with("topic:"));

        // Chủ đề manual không sync được.
        let blank = topic_from_template_value(&s, "blank", &json!({ "name": "Tay" }));
        assert_eq!(blank["ok"], true);
        assert!(topic_sync_value(&s, "Tay")["error"].is_string());
    }

    #[test]
    fn static_context_and_dynamic_fields() {
        let s = test_state();
        // Tạo chủ đề: tĩnh (vị trí + độ cao) vs động (ngày/nhiệt độ/gió).
        let made = topic_create_full(
            &s,
            "Thời tiết vườn",
            "theo dõi vi khí hậu vườn",
            &json!([
                { "name": "ngày", "kind": "date" },
                { "name": "nhiệt độ", "kind": "number" },
                { "name": "gió", "kind": "number" }
            ]),
            &json!([{ "name": "vị trí", "value": "Đà Lạt" }, { "name": "độ cao", "value": "1500m" }]),
            "Nhiệt độ giảm theo độ cao; gió mạnh làm sương muối khó hình thành.",
        );
        assert_eq!(made["ok"], true);
        assert_eq!(made["static"]["vị trí"], "Đà Lạt");
        assert_eq!(made["fields"].as_array().unwrap().len(), 3);

        let d = topic_dashboard_value(&s, "Thời tiết vườn");
        assert_eq!(d["static"]["độ cao"], "1500m");
        assert!(d["guide"].as_str().unwrap().contains("sương muối"));

        // Bối cảnh tĩnh + guide phải nằm trong meta gửi cho AI.
        let meta = topic_meta_ctx(
            &s,
            d["id"].as_i64().unwrap(),
            "Thời tiết vườn",
            "",
            &d["fields"],
            0,
        );
        assert_eq!(meta["static"]["vị trí"], "Đà Lạt");
        assert!(meta["guide"].as_str().unwrap().contains("độ cao"));

        // Sửa riêng phần tĩnh, giữ nguyên trường động.
        let upd = topic_update_full(
            &s,
            "Thời tiết vườn",
            None,
            None,
            None,
            Some(&json!({ "vị trí": "Nha Trang", "độ cao": "5m" })),
            Some("Vùng biển: gió biển chi phối nhiệt độ chiều."),
        );
        assert_eq!(upd["static"]["vị trí"], "Nha Trang");
        assert_eq!(upd["fields"].as_array().unwrap().len(), 3);
        assert!(upd["guide"].as_str().unwrap().contains("gió biển"));

        // Chủ đề không có trường động nào → từ chối.
        assert!(topic_create_full(&s, "Rỗng", "", &json!([]), &json!({}), "")["error"].is_string());
    }

    #[test]
    fn topic_documents_flow() {
        let s = test_state();
        topic_create_full(
            &s,
            "Vườn rau",
            "",
            &json!([{ "name": "ngày", "kind": "date" }, { "name": "nhiệt độ", "kind": "number" }]),
            &json!({}),
            "",
        );
        // Tài liệu gắn theo NGÀY và theo GIÁ TRỊ.
        let a = topic_doc_add_value(
            &s,
            "Vườn rau",
            "Đợt lạnh",
            "Không khí lạnh tăng cường về Tây Nguyên",
            "2026-07-27",
            "",
        );
        assert_eq!(a["ok"], true);
        let b = topic_doc_add_value(
            &s,
            "Vườn rau",
            "Ghi chú",
            "Đo lúc 6h sáng bằng nhiệt kế mới",
            "",
            "nhiệt độ",
        );
        assert_eq!(b["ok"], true);
        // Guard: rỗng và ngày sai định dạng.
        assert!(topic_doc_add_value(&s, "Vườn rau", "", "", "", "")["error"].is_string());
        assert!(
            topic_doc_add_value(&s, "Vườn rau", "x", "y", "27/07/2026", "")["error"].is_string()
        );
        assert!(topic_doc_add_value(&s, "Không có", "x", "y", "", "")["error"].is_string());

        let listed = topic_docs_value(&s, "Vườn rau", "", 50);
        assert_eq!(listed["docs"].as_array().unwrap().len(), 2);
        assert_eq!(
            topic_docs_value(&s, "Vườn rau", "lạnh", 50)["docs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            topic_docs_value(&s, "Vườn rau", "nhiệt độ", 50)["docs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        // Dashboard mang theo tài liệu; relevant_docs ưu tiên khớp từ khoá câu hỏi.
        let d = topic_dashboard_value(&s, "Vườn rau");
        assert_eq!(d["docs"].as_array().unwrap().len(), 2);
        let tid = d["id"].as_i64().unwrap();
        let rel = relevant_docs(&s, tid, "đợt lạnh này có gây sương muối không?", 8);
        assert_eq!(rel[0]["title"], "Đợt lạnh");
        assert_eq!(rel.len(), 2); // còn chỗ thì lấy nốt tài liệu chung
    }

    #[test]
    fn connector_topic_records_place_in_static() {
        let s = test_state();
        topic_from_template_value(&s, "weather", &json!({ "city": "Huế" }));
        let d = topic_dashboard_value(&s, "Thời tiết Huế");
        assert_eq!(d["static"]["vị trí"], "Huế");
        topic_from_template_value(&s, "football", &json!({ "league": "4332" }));
        let f = topic_dashboard_value(&s, "Bóng đá Serie A");
        assert_eq!(f["static"]["giải"], "Serie A");
    }

    #[test]
    fn topic_update_keeps_ledger_intact() {
        let s = test_state();
        topic_create_value(
            &s,
            "Cân nặng",
            "theo dõi",
            &json!([{ "name": "ngày", "kind": "date" }, { "name": "kg", "kind": "number" }]),
        );
        // Dự đoán cũ nằm ở domain theo tên cũ.
        let old_domain = topic::ledger_domain("Cân nặng");
        s.db.add_prediction(&PredictionInput {
            domain: old_domain.clone(),
            subject: "giảm 2kg".into(),
            detail: json!({}),
            probs: json!({ "yes": 0.6, "no": 0.4 }),
            due_at: 0,
        })
        .unwrap();

        // Đổi tên + thêm trường → dự đoán cũ chuyển domain mới.
        let upd = topic_update_value(
            &s,
            "Cân nặng",
            Some("Sức khoẻ"),
            Some("cân nặng + vận động"),
            Some(
                &json!([{ "name": "ngày", "kind": "date" }, { "name": "kg", "kind": "number" }, { "name": "tập", "kind": "bool" }]),
            ),
        );
        assert_eq!(upd["ok"], true);
        assert_eq!(upd["name"], "Sức khoẻ");
        assert_eq!(upd["predictions_moved"], 1);
        assert_eq!(upd["fields"].as_array().unwrap().len(), 3);
        assert!(s.db.find_topic("Sức khoẻ").is_some());
        assert!(s.db.find_topic("Cân nặng").is_none());
        let new_domain = topic::ledger_domain("Sức khoẻ");
        assert_eq!(s.db.list_predictions(Some(&new_domain), None, 10).len(), 1);
        assert_eq!(s.db.list_predictions(Some(&old_domain), None, 10).len(), 0);

        // Sửa mỗi mô tả — tên giữ nguyên, không chuyển domain.
        let only_desc = topic_update_value(&s, "Sức khoẻ", None, Some("chỉ mô tả"), None);
        assert_eq!(only_desc["predictions_moved"], 0);
        assert_eq!(only_desc["description"], "chỉ mô tả");
        // Guard: fields rỗng và chủ đề không tồn tại.
        assert!(
            topic_update_value(&s, "Sức khoẻ", None, None, Some(&json!([])))["error"].is_string()
        );
        assert!(topic_update_value(&s, "không có", Some("X"), None, None)["error"].is_string());
    }

    #[test]
    fn lottery_connector_sync_dedup() {
        let s = test_state();
        let numbers: Vec<i64> = (0..27).map(|i| 10000 + i).collect();
        let loto: Vec<u8> = numbers.iter().map(|n| (n % 100) as u8).collect();
        s.db.upsert_draw("2026-07-26", &numbers, &loto).unwrap();
        s.db.upsert_draw("2026-07-27", &numbers, &loto).unwrap();
        let made = topic_from_template_value(&s, "lottery", &json!({}));
        assert_eq!(made["synced"], 2);
        // Re-sync no duplicates; new draw appends one more.
        assert_eq!(topic_sync_value(&s, "Xổ số miền Bắc")["appended"], 0);
        s.db.upsert_draw("2026-07-28", &numbers, &loto).unwrap();
        assert_eq!(topic_sync_value(&s, "Xổ số miền Bắc")["appended"], 1);
    }

    #[tokio::test]
    async fn topic_design_validation_offline() {
        let s = test_state();
        assert!(topic_design_value(&s, "").await["error"].is_string());
        // No LLM bridge in tests → graceful error, nothing created.
        assert!(topic_design_value(&s, "theo dõi cân nặng").await["error"].is_string());
        assert!(s.db.list_topics().is_empty());
    }

    #[tokio::test]
    async fn topic_ask_validation_offline() {
        let s = test_state();
        assert!(topic_ask_value(&s, None, "", 7).await["error"].is_string());
        assert!(
            topic_ask_value(&s, Some("không có"), "X xảy ra không?", 7).await["error"].is_string()
        );
        // Free-form with no LLM bridge → graceful error, nothing ledgered.
        let r = topic_ask_value(&s, None, "Mai trời mưa không?", 7).await;
        assert!(r["error"].is_string());
        assert_eq!(s.db.list_predictions(None, None, 10).len(), 0);
    }

    #[test]
    fn status_and_settings_shape() {
        let s = test_state();
        let st = status_value(&s);
        assert_eq!(st["ok"], true);
        assert_eq!(st["lottery_draws"], 0);

        // Cài đặt chung không còn cities/leagues — nguồn thuộc về chủ đề.
        let se = settings_value(&s);
        assert!(se.get("cities").is_none() && se.get("leagues").is_none());
        assert!(se["search_mcp"].is_string() && se["theme"].is_string());
        assert_eq!(
            se["active_sources"]["weather_places"]
                .as_array()
                .unwrap()
                .len(),
            0
        );

        // Tạo chủ đề weather/football → nguồn active phản ánh đúng.
        topic_from_template_value(&s, "weather", &json!({ "city": "Đà Nẵng" }));
        topic_from_template_value(&s, "football", &json!({ "league": "4335" }));
        let se2 = settings_value(&s);
        assert_eq!(se2["active_sources"]["weather_places"][0], "Đà Nẵng");
        let leagues = se2["active_sources"]["football_leagues"]
            .as_array()
            .unwrap();
        assert_eq!(leagues.len(), 1);
        assert_eq!(leagues[0]["id"], "4335");
        assert_eq!(leagues[0]["name"], "La Liga");
    }

    #[tokio::test]
    async fn topic_source_switch() {
        let s = test_state();
        topic_from_template_value(&s, "football", &json!({ "league": "4328" }));
        // Đổi sang giải tự đặt tên → nguồn của chủ đề đổi theo, tên hiển thị nhớ được.
        let r = topic_source_update_value(
            &s,
            "Bóng đá Ngoại hạng Anh",
            &json!({ "league": "4344", "league_name": "V-League" }),
        )
        .await;
        assert_eq!(r["ok"], true);
        assert_eq!(r["source"]["league"], "4344");
        // Tên đang ở dạng mặc định → tự đổi theo giải mới.
        assert_eq!(r["renamed"], "Bóng đá V-League");
        assert!(s.db.find_topic("Bóng đá V-League").is_some());
        assert_eq!(s.db.league_label("4344"), "V-League");
        assert_eq!(s.db.leagues(), vec!["4344".to_string()]);
        // Tên do user đặt thì KHÔNG bị đổi khi chuyển nguồn.
        topic_update_value(&s, "Bóng đá V-League", Some("Kèo nhà tôi"), None, None);
        let r2 = topic_source_update_value(&s, "Kèo nhà tôi", &json!({ "league": "4335" })).await;
        assert_eq!(r2["ok"], true);
        assert!(r2["renamed"].is_null());
        assert!(s.db.find_topic("Kèo nhà tôi").is_some());
        // Guard: id không phải số, và chủ đề nhập tay.
        assert!(
            topic_source_update_value(&s, "Kèo nhà tôi", &json!({ "league": "abc" })).await
                ["error"]
                .is_string()
        );
        topic_create_value(&s, "Tay", "", &json!([{ "name": "x", "kind": "text" }]));
        assert!(
            topic_source_update_value(&s, "Tay", &json!({ "city": "Huế" })).await["error"]
                .is_string()
        );
    }
}
