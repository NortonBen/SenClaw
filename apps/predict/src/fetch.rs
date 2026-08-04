//! HTTP fetchers for every external data source. All keyless (verified live
//! 2026-07-27, see docs/sieu-du-doan-app-design.md §2). Each returns parsed
//! data; callers persist via `Db`. Network errors bubble up as `anyhow::Error`
//! so schedulers can log-and-continue.

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::time::Duration;

use crate::lottery;
use crate::timeutil::parse_iso_utc;

const UA: &str = "senclaw-predict/0.1 (+https://github.com/midea-ai/SenClaw)";

pub fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(UA)
        .timeout(Duration::from_secs(30))
        .build()
        .expect("reqwest client")
}

// ---- ClubElo (Elo ratings, CSV) ----

/// `http://api.clubelo.com/YYYY-MM-DD` → Rank,Club,Country,Level,Elo,From,To
pub async fn clubelo(
    http: &reqwest::Client,
    date: &str,
) -> Result<Vec<(String, String, f64, i64)>> {
    let url = format!("http://api.clubelo.com/{date}");
    let text = http
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let rows = parse_clubelo_csv(&text);
    if rows.is_empty() {
        return Err(anyhow!("ClubElo trả về 0 dòng ({url})"));
    }
    Ok(rows)
}

pub fn parse_clubelo_csv(text: &str) -> Vec<(String, String, f64, i64)> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() < 5 {
            continue;
        }
        let Ok(elo) = cols[4].trim().parse::<f64>() else {
            continue;
        };
        let rank = cols[0].trim().parse::<i64>().unwrap_or(0); // "None" for non-ranked
        let club = cols[1].trim();
        if club.is_empty() {
            continue;
        }
        out.push((club.to_string(), cols[2].trim().to_string(), elo, rank));
    }
    out
}

// ---- XSMB dataset (daily CSV on GitHub) ----

const XSMB_CSV_URL: &str =
    "https://raw.githubusercontent.com/khiemdoan/vietnam-lottery-xsmb-analysis/main/data/xsmb.csv";

pub async fn xsmb_csv(http: &reqwest::Client) -> Result<Vec<lottery::Draw>> {
    let text = http
        .get(XSMB_CSV_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let draws = lottery::parse_csv(&text);
    if draws.is_empty() {
        return Err(anyhow!("xsmb.csv parse ra 0 kỳ quay"));
    }
    Ok(draws)
}

// ---- Open-Meteo (forecast + archive) ----

/// Vietnamese cities the UI/skill can name. (name, lat, lon)
pub const CITIES: &[(&str, f64, f64)] = &[
    ("Hà Nội", 21.0285, 105.8542),
    ("TP.HCM", 10.8231, 106.6297),
    ("Đà Nẵng", 16.0544, 108.2022),
    ("Hải Phòng", 20.8449, 106.6881),
    ("Cần Thơ", 10.0452, 105.7469),
    ("Huế", 16.4637, 107.5909),
    ("Nha Trang", 12.2388, 109.1967),
    ("Đà Lạt", 11.9404, 108.4583),
    ("Vinh", 18.6796, 105.6813),
    ("Quy Nhơn", 13.7830, 109.2196),
];

/// Geocode any place name via Open-Meteo (keyless) → (display name, lat, lon).
/// Lets users add cities beyond the built-in list.
pub async fn geocode(
    http: &reqwest::Client,
    name: &str,
) -> Result<Vec<(String, f64, f64, String)>> {
    let q = urlencode(name.trim());
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={q}&count=5&language=vi&format=json"
    );
    let v: Value = http
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(v["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|r| {
                    Some((
                        r["name"].as_str()?.to_string(),
                        r["latitude"].as_f64()?,
                        r["longitude"].as_f64()?,
                        format!(
                            "{}{}",
                            r["admin1"]
                                .as_str()
                                .map(|a| format!("{a}, "))
                                .unwrap_or_default(),
                            r["country"]
                                .as_str()
                                .or(r["country_code"].as_str())
                                .unwrap_or("")
                        ),
                    ))
                })
                .collect()
        })
        .unwrap_or_default())
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// Loose city lookup over the BUILT-IN list only (callers fall back to the
/// user's saved custom coordinates — see `Db::city_coord`).
pub fn find_city(name: &str) -> Option<(&'static str, f64, f64)> {
    let q = name.trim().to_lowercase();
    let alias = match q.as_str() {
        "hcm" | "ho chi minh" | "hồ chí minh" | "sài gòn" | "saigon" | "sg" => "tp.hcm",
        "hanoi" | "ha noi" => "hà nội",
        "danang" | "da nang" => "đà nẵng",
        other => other,
    };
    CITIES
        .iter()
        .find(|(n, _, _)| n.to_lowercase() == alias)
        .copied()
        .or_else(|| {
            CITIES
                .iter()
                .find(|(n, _, _)| n.to_lowercase().contains(alias))
                .copied()
        })
}

/// 7-day daily forecast (+ current) for a city.
pub async fn open_meteo_forecast(http: &reqwest::Client, lat: f64, lon: f64) -> Result<Value> {
    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
         &daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max,precipitation_sum\
         &current=temperature_2m,relative_humidity_2m,weather_code\
         &timezone=Asia%2FBangkok&forecast_days=7"
    );
    Ok(http
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// Observed daily precipitation for a past date (archive API) — used to resolve
/// ledgered rain forecasts. Returns mm (None when the archive has no value yet).
pub async fn open_meteo_observed_rain(
    http: &reqwest::Client,
    lat: f64,
    lon: f64,
    date: &str,
) -> Result<Option<f64>> {
    let url = format!(
        "https://archive-api.open-meteo.com/v1/archive?latitude={lat}&longitude={lon}\
         &start_date={date}&end_date={date}&daily=precipitation_sum&timezone=Asia%2FBangkok"
    );
    let v: Value = http
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(v["daily"]["precipitation_sum"][0].as_f64())
}

// ---- Gold & FX ----

pub async fn gold_xau_usd(http: &reqwest::Client) -> Result<f64> {
    let v: Value = http
        .get("https://api.gold-api.com/price/XAU")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    v["price"]
        .as_f64()
        .ok_or_else(|| anyhow!("gold-api: thiếu 'price'"))
}

pub async fn fx_usd_vnd(http: &reqwest::Client) -> Result<f64> {
    let v: Value = http
        .get("https://open.er-api.com/v6/latest/USD")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    v["rates"]["VND"]
        .as_f64()
        .ok_or_else(|| anyhow!("er-api: thiếu rates.VND"))
}

// ---- TheSportsDB fixtures/results (free test key `3`) ----

/// League ids the settings UI offers. (TheSportsDB id, display name)
pub const LEAGUES: &[(&str, &str)] = &[
    ("4328", "Ngoại hạng Anh"),
    ("4335", "La Liga"),
    ("4331", "Bundesliga"),
    ("4332", "Serie A"),
    ("4334", "Ligue 1"),
    ("4480", "Champions League"),
];

pub fn league_name(id: &str) -> &'static str {
    LEAGUES
        .iter()
        .find(|(i, _)| *i == id)
        .map(|(_, n)| *n)
        .unwrap_or("Giải khác")
}

pub struct FetchedFixture {
    pub event_id: String,
    pub home: String,
    pub away: String,
    pub kickoff_ts: i64,
    pub home_score: Option<i64>,
    pub away_score: Option<i64>,
}

async fn tsdb_events(
    http: &reqwest::Client,
    endpoint: &str,
    league_id: &str,
) -> Result<Vec<FetchedFixture>> {
    let url = format!("https://www.thesportsdb.com/api/v1/json/3/{endpoint}.php?id={league_id}");
    let v: Value = http
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let events = v["events"].as_array().cloned().unwrap_or_default();
    Ok(events.iter().filter_map(parse_tsdb_event).collect())
}

pub fn parse_tsdb_event(e: &Value) -> Option<FetchedFixture> {
    let event_id = e["idEvent"].as_str()?.to_string();
    let home = e["strHomeTeam"].as_str()?.to_string();
    let away = e["strAwayTeam"].as_str()?.to_string();
    let kickoff_ts = e["strTimestamp"]
        .as_str()
        .and_then(parse_iso_utc)
        .unwrap_or(0);
    let score = |k: &str| {
        e[k].as_str()
            .and_then(|s| s.parse::<i64>().ok())
            .or_else(|| e[k].as_i64())
    };
    Some(FetchedFixture {
        event_id,
        home,
        away,
        kickoff_ts,
        home_score: score("intHomeScore"),
        away_score: score("intAwayScore"),
    })
}

pub async fn fixtures_upcoming(
    http: &reqwest::Client,
    league_id: &str,
) -> Result<Vec<FetchedFixture>> {
    tsdb_events(http, "eventsnextleague", league_id).await
}

pub async fn results_recent(
    http: &reqwest::Client,
    league_id: &str,
) -> Result<Vec<FetchedFixture>> {
    tsdb_events(http, "eventspastleague", league_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn clubelo_csv_parse() {
        let csv = "Rank,Club,Country,Level,Elo,From,To\n\
                   1,Arsenal,ENG,1,2063.75805664,2026-05-31,2026-08-21\n\
                   None,Bayern II,GER,3,1400.5,2026-01-01,2026-08-21\n\
                   bad line\n";
        let rows = parse_clubelo_csv(csv);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "Arsenal");
        assert_eq!(rows[0].3, 1);
        assert_eq!(rows[1].3, 0); // "None" rank → 0
    }

    #[test]
    fn tsdb_event_parse() {
        let e = json!({
            "idEvent": "2494000",
            "strHomeTeam": "Arsenal",
            "strAwayTeam": "Coventry City",
            "strTimestamp": "2026-08-21T19:00:00",
            "intHomeScore": null,
            "intAwayScore": null
        });
        let f = parse_tsdb_event(&e).unwrap();
        assert_eq!(f.event_id, "2494000");
        assert!(f.kickoff_ts > 0);
        assert!(f.home_score.is_none());

        let done = json!({
            "idEvent": "1", "strHomeTeam": "A", "strAwayTeam": "B",
            "strTimestamp": "2026-07-20T14:00:00",
            "intHomeScore": "2", "intAwayScore": "1"
        });
        let f2 = parse_tsdb_event(&done).unwrap();
        assert_eq!(f2.home_score, Some(2));
        assert_eq!(f2.away_score, Some(1));
    }

    #[test]
    fn city_lookup() {
        assert_eq!(find_city("Hà Nội").unwrap().0, "Hà Nội");
        assert_eq!(find_city("saigon").unwrap().0, "TP.HCM");
        assert_eq!(find_city("da nang").unwrap().0, "Đà Nẵng");
        assert!(find_city("Tokyo").is_none());
    }
}
