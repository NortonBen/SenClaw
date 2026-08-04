//! Data engine: staleness-aware fetchers (`ensure_*`), the auto-resolver that
//! scores due ledger rows against realized outcomes, and the background
//! scheduler loop. Every job is idempotent and cheap when fresh, so the loop
//! simply runs the whole set every 10 minutes; each `ensure_*` no-ops unless
//! its own staleness window has passed.

use serde_json::{json, Value};

use crate::api::AppState;
use crate::db::now;
use crate::timeutil::{date_str, parse_date_days, vn_date, vn_hm};
use crate::{fetch, football, ledger};

const HOUR: i64 = 3600;

fn mark(s: &AppState, key: &str) {
    let _ = s.db.set_setting(key, &now().to_string());
}

fn since(s: &AppState, key: &str) -> i64 {
    now()
        - s.db
            .get_setting(key)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0)
}

/// ClubElo snapshot — weekly refresh (ratings move only after matches).
pub async fn ensure_elo(s: &AppState, force: bool) -> Value {
    if !force && s.db.elo_count() > 0 && now() - s.db.elo_updated_at() < 7 * 24 * HOUR {
        return json!({ "source": "elo", "fresh": true });
    }
    match fetch::clubelo(&s.http, &date_str(now(), 0)).await {
        Ok(rows) => {
            let n = rows.len();
            for (club, country, elo, rank) in rows {
                let _ = s.db.upsert_elo(&club, &country, elo, rank);
            }
            s.db.log("fetch", &format!("ClubElo: {n} CLB"), "elo");
            json!({ "source": "elo", "updated": n })
        }
        Err(e) => json!({ "source": "elo", "error": e.to_string() }),
    }
}

/// Fixtures + recent results for every tracked league — 6-hourly.
pub async fn ensure_fixtures(s: &AppState, force: bool) -> Value {
    if !force && s.db.fixtures_count() > 0 && since(s, "fixtures_at") < 6 * HOUR {
        return json!({ "source": "fixtures", "fresh": true });
    }
    let mut upserted = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for league_id in s.db.leagues() {
        let name = fetch::league_name(&league_id);
        for res in [
            fetch::fixtures_upcoming(&s.http, &league_id).await,
            fetch::results_recent(&s.http, &league_id).await,
        ] {
            match res {
                Ok(rows) => {
                    for f in rows {
                        let status = if f.home_score.is_some() {
                            "finished"
                        } else {
                            "scheduled"
                        };
                        let _ = s.db.upsert_fixture(
                            &f.event_id,
                            &league_id,
                            name,
                            &f.home,
                            &f.away,
                            f.kickoff_ts,
                            f.home_score,
                            f.away_score,
                            status,
                        );
                        upserted += 1;
                    }
                }
                Err(e) => errors.push(format!("{league_id}: {e}")),
            }
        }
    }
    if upserted > 0 {
        mark(s, "fixtures_at");
        s.db.log("fetch", &format!("Fixtures: {upserted} trận"), "fixtures");
    }
    json!({ "source": "fixtures", "updated": upserted, "errors": errors })
}

/// XSMB draws. Full backfill on first run; afterwards refetch when a new draw
/// is expected (after 18:35 VN) and ours is stale, retrying at most every 20'.
pub async fn ensure_draws(s: &AppState, force: bool) -> Value {
    let today = vn_date(now());
    let latest = s.db.latest_draw().map(|(d, _, _)| d).unwrap_or_default();
    if !force {
        let (h, m) = vn_hm(now());
        let draw_out = h > 18 || (h == 18 && m >= 35);
        let expected = if draw_out {
            today.clone()
        } else {
            yesterday(&today)
        };
        if !latest.is_empty() && latest >= expected {
            return json!({ "source": "lottery", "fresh": true, "latest": latest });
        }
        if since(s, "lottery_try_at") < 20 * 60 {
            return json!({ "source": "lottery", "waiting": true, "latest": latest });
        }
    }
    mark(s, "lottery_try_at");
    match fetch::xsmb_csv(&s.http).await {
        Ok(draws) => {
            // Newest rows are at the file's end; upsert everything (idempotent).
            let n = draws.len();
            for d in &draws {
                let _ = s.db.upsert_draw(&d.date, &d.numbers, &d.loto());
            }
            let newest = s.db.latest_draw().map(|(d, _, _)| d).unwrap_or_default();
            s.db.log(
                "fetch",
                &format!("XSMB: {n} kỳ, mới nhất {newest}"),
                "lottery",
            );
            json!({ "source": "lottery", "updated": n, "latest": newest })
        }
        Err(e) => json!({ "source": "lottery", "error": e.to_string() }),
    }
}

fn yesterday(date: &str) -> String {
    parse_date_days(date)
        .map(|d| date_str((d - 1) * 86400, 0))
        .unwrap_or_default()
}

/// Gold + FX quarter-hourly; also records the derived VND/lượng series.
pub async fn ensure_gold(s: &AppState, force: bool) -> Value {
    if !force
        && s.db
            .latest_price("XAU_USD")
            .map(|(ts, _)| now() - ts < HOUR)
            .unwrap_or(false)
    {
        return json!({ "source": "gold", "fresh": true });
    }
    let xau = fetch::gold_xau_usd(&s.http).await;
    let fx = fetch::fx_usd_vnd(&s.http).await;
    match (xau, fx) {
        (Ok(x), Ok(r)) => {
            let _ = s.db.add_price("XAU_USD", x);
            let _ = s.db.add_price("USD_VND", r);
            let _ =
                s.db.add_price("XAU_VND_LUONG", crate::market::xau_to_vnd_luong(x, r));
            json!({ "source": "gold", "xau_usd": x, "usd_vnd": r })
        }
        (x, r) => json!({
            "source": "gold",
            "error": [x.err().map(|e| e.to_string()), r.err().map(|e| e.to_string())],
        }),
    }
}

/// Weather cache per configured city — 3-hourly.
pub async fn ensure_weather(s: &AppState, force: bool) -> Value {
    let mut refreshed = 0usize;
    for city in s.db.cities() {
        let Some((name, lat, lon)) = s.db.city_coord(&city) else {
            continue;
        };
        let fresh =
            s.db.weather_get(&name)
                .map(|(_, t)| now() - t < 3 * HOUR)
                .unwrap_or(false);
        if fresh && !force {
            continue;
        }
        if let Ok(v) = fetch::open_meteo_forecast(&s.http, lat, lon).await {
            let _ = s.db.weather_set(&name, &v);
            refreshed += 1;
        }
    }
    json!({ "source": "weather", "refreshed": refreshed })
}

/// Score every due, unresolved ledger row whose outcome is now knowable.
pub async fn resolve_all(s: &AppState) -> Value {
    let due = s.db.unresolved_due(now());
    let mut resolved = 0usize;
    for p in due {
        let outcome: Option<String> = match p.domain.as_str() {
            "football" => resolve_football(s, &p.detail),
            "lottery" => resolve_lottery(s, &p.detail),
            "weather" => resolve_weather(s, &p.detail).await,
            _ => None, // generic/market resolve manually via predict_resolve
        };
        if let Some(out) = outcome {
            let (b, correct) = ledger::score(&p.probs, &out);
            let _ = s.db.resolve_prediction(p.id, &out, b, correct);
            s.db.log(
                "resolve",
                &format!("#{} {} → {} (brier {:.3})", p.id, p.subject, out, b),
                &p.domain,
            );
            resolved += 1;
        }
    }
    json!({ "resolved": resolved })
}

fn resolve_football(s: &AppState, detail: &Value) -> Option<String> {
    let event_id = detail["event_id"].as_str()?;
    let f = s.db.fixture(event_id)?;
    let (h, a) = (f.home_score?, f.away_score?);
    Some(match h.cmp(&a) {
        std::cmp::Ordering::Greater => "H".into(),
        std::cmp::Ordering::Equal => "D".into(),
        std::cmp::Ordering::Less => "A".into(),
    })
}

fn resolve_lottery(s: &AppState, detail: &Value) -> Option<String> {
    let date = detail["date"].as_str()?;
    let picks: Vec<u8> = detail["numbers"]
        .as_array()?
        .iter()
        .filter_map(|v| v.as_u64().map(|n| (n % 100) as u8))
        .collect();
    let (_, _, loto) = s.db.draw_by_date(date)?;
    let hit = picks.iter().any(|p| loto.contains(p));
    Some(if hit { "hit".into() } else { "miss".into() })
}

async fn resolve_weather(s: &AppState, detail: &Value) -> Option<String> {
    let date = detail["date"].as_str()?;
    // Only resolvable once the day has fully passed in VN.
    if vn_date(now()) <= date.to_string() {
        return None;
    }
    let (lat, lon) = (detail["lat"].as_f64()?, detail["lon"].as_f64()?);
    let mm = fetch::open_meteo_observed_rain(&s.http, lat, lon, date)
        .await
        .ok()??;
    Some(if mm >= 1.0 {
        "rain".into()
    } else {
        "dry".into()
    })
}

/// Auto-ledger predictions for upcoming tracked fixtures (dedup by event_id).
pub fn ledger_upcoming_fixtures(s: &AppState, preds: &[(String, Value, i64)]) -> usize {
    let mut added = 0usize;
    for (event_id, pred, kickoff_ts) in preds {
        if *kickoff_ts <= now() || s.db.has_open_prediction("football", "event_id", event_id) {
            continue;
        }
        let subject = format!(
            "{} vs {}",
            pred["home"].as_str().unwrap_or("?"),
            pred["away"].as_str().unwrap_or("?")
        );
        let _ = s.db.add_prediction(&crate::db::PredictionInput {
            domain: "football".into(),
            subject,
            detail: json!({ "event_id": event_id }),
            probs: json!({
                "H": pred["p_home"], "D": pred["p_draw"], "A": pred["p_away"],
            }),
            // Give the result ~2.5h post-kickoff to land in TheSportsDB.
            due_at: kickoff_ts + (150 * 60),
        });
        added += 1;
    }
    added
}

// ---- topic connectors (công cụ build chủ đề) ----

/// Append one record if no existing record matches `dedup_key` (substring
/// search over data+note). Returns true when inserted.
fn append_unique(s: &AppState, tid: i64, dedup_key: &str, data: Value, note: &str) -> bool {
    if dedup_key.is_empty() || !s.db.search_topic_records(tid, dedup_key, 1).is_empty() {
        return false;
    }
    s.db.add_topic_record(tid, &data, note).is_ok()
}

/// Feed one connector topic from ALREADY-FETCHED local data (never fetches).
/// Returns how many records were appended.
pub fn sync_topic(s: &AppState, tid: i64, source: &Value) -> usize {
    let mut added = 0usize;
    match source["kind"].as_str().unwrap_or("manual") {
        "gold" => {
            let date = vn_date(now());
            let (Some((_, xau)), Some((_, fx)), Some((_, luong))) = (
                s.db.latest_price("XAU_USD"),
                s.db.latest_price("USD_VND"),
                s.db.latest_price("XAU_VND_LUONG"),
            ) else {
                return 0;
            };
            let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
            if append_unique(
                s,
                tid,
                &date,
                json!({ "ngày": date, "xau_usd": r3(xau), "usd_vnd": fx.round(), "trieu_luong": r3(luong) }),
                "sync:gold",
            ) {
                added += 1;
            }
        }
        "weather" => {
            let Some(city) = source["city"].as_str() else {
                return 0;
            };
            let Some((payload, _)) = s.db.weather_get(city) else {
                return 0;
            };
            let d = &payload["daily"];
            if let Some(date) = d["time"][0].as_str() {
                if append_unique(
                    s,
                    tid,
                    date,
                    json!({
                        "ngày": date,
                        "t_max": d["temperature_2m_max"][0],
                        "t_min": d["temperature_2m_min"][0],
                        "mua_prob": d["precipitation_probability_max"][0],
                    }),
                    // Ghi kèm địa điểm để biết bản ghi thuộc nơi nào khi đổi nguồn.
                    &format!("sync:weather:{city}"),
                ) {
                    added += 1;
                }
            }
        }
        "lottery" => {
            for (date, numbers, _) in s.db.draws(15) {
                let Some(special) = numbers.first() else {
                    continue;
                };
                if append_unique(
                    s,
                    tid,
                    &date,
                    json!({ "ngày": date, "dac_biet": special, "duoi_db": special.rem_euclid(100) }),
                    "sync:lottery",
                ) {
                    added += 1;
                }
            }
        }
        "football" => {
            let Some(league) = source["league"].as_str() else {
                return 0;
            };
            for f in s.db.fixtures_finished(league, 30) {
                let (Some(h), Some(a)) = (f.home_score, f.away_score) else {
                    continue;
                };
                let ket_qua = match h.cmp(&a) {
                    std::cmp::Ordering::Greater => "H",
                    std::cmp::Ordering::Equal => "D",
                    std::cmp::Ordering::Less => "A",
                };
                if append_unique(
                    s,
                    tid,
                    &f.event_id,
                    json!({
                        "ngày": vn_date(f.kickoff_ts),
                        "tran": format!("{} vs {}", f.home, f.away),
                        "ban_nha": h, "ban_khach": a, "ket_qua": ket_qua,
                    }),
                    &format!("sync:{}", f.event_id),
                ) {
                    added += 1;
                }
            }
        }
        _ => {}
    }
    if added > 0 {
        s.db.log(
            "sync",
            &format!("connector nạp {added} bản ghi vào chủ đề #{tid}"),
            "topics",
        );
    }
    added
}

/// Sync every connector topic. Cheap: reads only local tables.
pub fn sync_topics(s: &AppState) -> Value {
    let mut total = 0usize;
    for (tid, _, source) in s.db.connector_topics() {
        total += sync_topic(s, tid, &source);
    }
    json!({ "appended": total })
}

/// Run every job once; the per-source staleness guards make this cheap.
pub async fn run_all(s: &AppState, force: bool) -> Value {
    let elo = ensure_elo(s, force).await;
    let fixtures = ensure_fixtures(s, force).await;
    let draws = ensure_draws(s, force).await;
    let gold = ensure_gold(s, force).await;
    let weather = ensure_weather(s, force).await;
    let synced = sync_topics(s);
    let resolved = resolve_all(s).await;
    json!({
        "elo": elo, "fixtures": fixtures, "lottery": draws,
        "gold": gold, "weather": weather, "topics": synced, "resolve": resolved,
    })
}

pub fn spawn_scheduler(s: AppState) {
    tokio::spawn(async move {
        // First pass shortly after boot so the UI has data without waiting.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        loop {
            let _ = run_all(&s, false).await;
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
        }
    });
}

/// Assemble the numeric picture a match prediction needs, from local state.
pub fn predict_for_fixture(s: &AppState, f: &crate::db::Fixture) -> Value {
    let table = s.db.all_elo();
    let (elo_h, match_h) =
        football::find_elo(&table, &f.home).unwrap_or((football::FALLBACK_ELO, String::new()));
    let (elo_a, match_a) =
        football::find_elo(&table, &f.away).unwrap_or((football::FALLBACK_ELO, String::new()));
    let mut pred = football::predict(&f.home, &f.away, elo_h, elo_a);
    pred["event_id"] = json!(f.event_id);
    pred["league"] = json!(f.league_name);
    pred["kickoff_ts"] = json!(f.kickoff_ts);
    pred["kickoff_vn"] = json!(format!(
        "{} {:02}:{:02}",
        vn_date(f.kickoff_ts),
        vn_hm(f.kickoff_ts).0,
        vn_hm(f.kickoff_ts).1
    ));
    pred["elo_matched"] = json!(!match_h.is_empty() && !match_a.is_empty());
    pred
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::test_state;

    #[test]
    fn resolve_football_outcomes() {
        let s = test_state();
        s.db.upsert_fixture(
            "E1",
            "4328",
            "EPL",
            "A",
            "B",
            100,
            Some(2),
            Some(1),
            "finished",
        )
        .unwrap();
        s.db.upsert_fixture(
            "E2",
            "4328",
            "EPL",
            "A",
            "B",
            100,
            Some(1),
            Some(1),
            "finished",
        )
        .unwrap();
        s.db.upsert_fixture("E3", "4328", "EPL", "A", "B", 100, None, None, "scheduled")
            .unwrap();
        assert_eq!(
            resolve_football(&s, &json!({ "event_id": "E1" })),
            Some("H".into())
        );
        assert_eq!(
            resolve_football(&s, &json!({ "event_id": "E2" })),
            Some("D".into())
        );
        assert_eq!(resolve_football(&s, &json!({ "event_id": "E3" })), None);
        assert_eq!(resolve_football(&s, &json!({})), None);
    }

    #[test]
    fn resolve_lottery_hit_and_miss() {
        let s = test_state();
        let numbers: Vec<i64> = vec![42916; 27];
        let loto: Vec<u8> = numbers.iter().map(|n| (n % 100) as u8).collect(); // all 16
        s.db.upsert_draw("2026-07-26", &numbers, &loto).unwrap();
        assert_eq!(
            resolve_lottery(&s, &json!({ "date": "2026-07-26", "numbers": [16, 99] })),
            Some("hit".into())
        );
        assert_eq!(
            resolve_lottery(&s, &json!({ "date": "2026-07-26", "numbers": [15] })),
            Some("miss".into())
        );
        assert_eq!(
            resolve_lottery(&s, &json!({ "date": "2026-01-01", "numbers": [16] })),
            None
        );
    }

    #[tokio::test]
    async fn resolve_all_scores_ledger() {
        let s = test_state();
        s.db.upsert_fixture(
            "E9",
            "4328",
            "EPL",
            "Arsenal",
            "Chelsea",
            100,
            Some(3),
            Some(0),
            "finished",
        )
        .unwrap();
        let id =
            s.db.add_prediction(&crate::db::PredictionInput {
                domain: "football".into(),
                subject: "Arsenal vs Chelsea".into(),
                detail: json!({ "event_id": "E9" }),
                probs: json!({ "H": 0.7, "D": 0.2, "A": 0.1 }),
                due_at: 0,
            })
            .unwrap();
        let out = resolve_all(&s).await;
        assert_eq!(out["resolved"], 1);
        let p = s.db.get_prediction(id).unwrap();
        assert_eq!(p.outcome.as_deref(), Some("H"));
        assert_eq!(p.correct, Some(true));
        assert!(p.brier.unwrap() < 0.2);
    }

    #[test]
    fn ledger_upcoming_dedups() {
        let s = test_state();
        let pred = json!({ "home": "A", "away": "B", "p_home": 0.5, "p_draw": 0.3, "p_away": 0.2 });
        let future = now() + 86400;
        let rows = vec![("EV1".to_string(), pred.clone(), future)];
        assert_eq!(ledger_upcoming_fixtures(&s, &rows), 1);
        assert_eq!(ledger_upcoming_fixtures(&s, &rows), 0); // dedup
                                                            // Past kickoff never ledgered.
        let past = vec![("EV2".to_string(), pred, 100)];
        assert_eq!(ledger_upcoming_fixtures(&s, &past), 0);
    }

    #[test]
    fn yesterday_math() {
        assert_eq!(yesterday("2026-07-27"), "2026-07-26");
        assert_eq!(yesterday("2026-01-01"), "2025-12-31");
    }
}
